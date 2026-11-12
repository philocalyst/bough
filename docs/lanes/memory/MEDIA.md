# Media in bough: the API, the storage, the contract with the kernel (lane memory, round 7)

Status: design, written before the code (2026-11-12). Each storage decision below is a hypothesis with a
prediction and a kill criterion; results are logged in `JOURNAL.md` and folded back here.

## 1. What a user writes

The test of the API is what the author does *not* write: no decode call, no cache, no size bookkeeping, no
placeholder state, no "is it loaded" flag, no virtualised list, no cancellation.

**An image in a list row** (a contact list of 100,000 rows written as plain children):

```rust
struct Contact { name: String, avatar: Media }
struct Contacts { all: Vec<Contact>, y: Signal<f32> }

impl View for Contacts {
    fn view(&self) -> impl Element<Self> + '_ {
        div().scroll(sig!(self.y)).child(each(self.all.iter().map(|c| {
            row((img(&c.avatar).size(40.0, 40.0).rounded(20.0), text(c.name.as_str()))).gap(8.0)
        })))
    }
}
// Media::file(path): nothing is read but the file's identity (stat); nothing is decoded until a pixel of
// the avatar is exposed, and then only at 40 x 40 pt times the window's scale.
```

**A gallery** (10,000 photos, cover-cropped into square cells):

```rust
struct Gallery { photos: Vec<Media>, y: Signal<f32> }

impl View for Gallery {
    fn view(&self) -> impl Element<Self> + '_ {
        div().scroll(sig!(self.y)).child(grid(5, self.photos.iter().map(|p| img(p).cover().aspect(1.0).rounded(6.0))))
    }
}
```

**An avatar that updates** (the image is a signal; the write is the update):

```rust
struct Profile { avatar: Signal<Media> }
impl Profile {
    fn upload(&mut self, bytes: Vec<u8>) { self.avatar.set(Media::owned(bytes)); }
}
// view: img(sig!(self.avatar)).size(64.0, 64.0).rounded(32.0)
// The old pixels stay on screen until the new image's first variant is resident (a held write, as an async
// transition holds a screen), then they cross-fade in the kernel; no placeholder flash, no handler code.
```

**A video grid**:

```rust
struct Wall { clips: Vec<Video> }
// view: grid(4, self.clips.iter().map(|v| video(v).cover().muted().looped()))
// Only clips with an exposed pixel decode frames; an unexposed clip holds its poster variant (or nothing).
```

**Content from elsewhere**: `Media::bytes(&'static [u8])` (embedded), `Media::owned(Vec<u8>)` (a network
response, a paste), `Media::file(path)`; later `Media::url(..)` through the async lane's resources. Equal bytes
are one `Media` (content-addressed); a file is identified by (device, inode, size, mtime) without reading it.

Modifiers: `.cover()` / `.contain()` / `.fill()` (fit), `.aspect(r)` (layout without the intrinsic size),
`.rounded(r)`, `.opacity(..)`; the intrinsic size, when layout needs it, is read from the header only.

## 2. Exposure is the only trigger (Amendment 2)

There is no media-specific visibility pass, cache policy, or invalidation path:

* **Materialised only where exposed.** An `img` that has never been exposed has no record, no layout result,
  no texels (it is inside a dormant run, the same as text or boxes). A record becoming exposed is what asks
  for pixels: its device-pixel box is the request (size, crop, scale), so "decoded only to the exposed region
  and scale" is a property of the request, not a policy.
* **Residency is paint state.** A record shows a *variant* (decoded pixels at one size) through tile slots. A
  variant's tiles landing is an ordinary paint write on the record's tiles (the damage rect of the landed
  tile), a variant replaced is the same. Retained == full redraw holds because every pixel change goes
  through the one damage path.
* **Release on leaving exposure.** A run that goes dormant releases its records; a record releases its
  variant reference; an unreferenced variant is a cache entry the calm trim or memory pressure takes.

## 3. Storage (platform-independent crate `bough-media`; the GPU only wraps its memory)

* **Encoded bytes are never copied**: files are mapped (clean, the OS can drop and re-read them), embedded
  bytes are static, owned bytes are kept once per content key (and may be spilled to the disk cache).
* **One tile pool.** Decoded pixels live in fixed slots of 64 x 64 texels. An RGBA8 slot is 16,384 bytes,
  one VM page on Apple silicon: slots are allocated, purged and mapped a page at a time. The pool is one
  anonymous mapping reserved as address space; only written slots are resident. The CPU executor reads it in
  place; the GPU executors wrap the same memory (Metal no-copy buffer, Vulkan host-pointer import), so there
  is no CPU copy, no texture and no upload. Icons and gradient ramps move into the same pool (the image atlas
  `deco::Images`, 4 MiB CPU + 4 MiB GPU, panicking at its ninth thumbnail, is deleted).
* **A variant** is (content key, device width, device height, crop, format) and owns a tile list. Two
  records showing the same content at the same size share it.
* **Progressive residency.** Every exposed or nearly exposed image has a *tiny* variant (at most 32 x 32
  texels, from the disk cache, the JPEG's DC coefficients or an EXIF thumbnail) before its first frame; the
  sharp variant's tiles land over it one by one. The sharp layer's missing tiles are simply absent commands,
  so the tiny layer shows through with no fallback logic in the kernel. The switch is a kernel-timed
  opacity transition of the sharp layer, not turns.
* **Decode off the main thread, at the display size.** Workers decode into slots directly (scaled IDCT for
  JPEG where the source is larger than 2x the box: 1/2, 1/4, 1/8; then one resample). A landing is published
  by the next turn (the completion queue is drained at the turn's start), so the main thread never waits for
  a decode and the first frame never waits for a photo.
* **Disk cache**: per variant, its tiles as written, keyed by content key and size; the next launch maps the
  file and copies slots (16 KiB memcpy, ~1-2 us) instead of decoding. Tiny variants of everything ever seen
  live in one file, so a gallery's first frame on a second launch is complete (blurred) at once.
* **Purgeable**: unreferenced variants are cache. The calm trim drops them (`madvise(DONTNEED)` on their
  slots); memory pressure (a dispatch source on macOS, PSI on Linux) does the same. Idle resident bytes are
  the tiles of exposed records only.

## 4. The contract with the kernel (proposal, sent to the lead for compute)

What media needs from the command format and bindings, smallest first:

1. **IMAGES = the tile pool, stride 64.** Binding 4 is the pool's memory, viewed as an image atlas 64 texels
   wide (slot `s` = texel rows `64 s .. 64 s + 63`, contiguous 16 KiB). The existing `K_IMAGE` sampler then
   needs no change: a resident 64 x 64 tile of a variant is one `K_IMAGE` command whose box is the tile's
   device rect and whose `par.z` is `4096 s`. The staging (CPU side) expands a media record into one command
   per resident tile meeting the frame's damage. Ramps (256 texels, linear) fit 16 to a slot. No tile table in
   the kernel at all.
2. **No upload.** The pool's memory is provided by bough-media; the GPU executor wraps it once (no-copy) and
   is told the dirty slot ranges only if its platform needs a copy (Vulkan without host-pointer import).
3. **The exact 1:1 image command** (compute already owns it): integer translation, scale 1, no rotation,
   opaque or premultiplied: a row copy or blend, no taps. Every media tile at rest is this case.
4. **Scaled variants during a resize or zoom** reuse transformed spaces: a variant of size (vw, vh) shown in a
   box (bw, bh) is the tile commands in a space scaled by (bw/vw, bh/vh); the existing bilinear taps apply.
5. **YUV 4:2:0 tiles** (later; experiment M5): `K_IMAGE` with a flag: a slot holds Y 64x64 + Cb 32x32 + Cr
   32x32 (6,144 bytes; two such tiles per 16 KiB page with room to spare), the fetch reads three bytes per tap
   and converts (BT.601 full range, integer). Photos and video frames share it.
6. **Cross-fade**: the sharp layer's opacity as an `F_ANIM` transition (start tick, duration), so landing
   costs one paint write and no turns.

## 5. Hypotheses (prediction and kill criterion before each run)

| # | hypothesis | prediction | kill |
|---|---|---|---|
| M1 | Decode off the main thread removes images from startup | feed `App::new` 148 M -> < 10 M of image work on the main thread; first frame without waiting for any decode | any decode or resample left on the main thread at mount |
| M2 | Tile pool bounded by exposure | gallery of 10,000 distinct photos, scrolled end to end: pool resident <= 3 x window pixels x 4 B (~34 MiB at 2000 x 1400) active, <= 1.2 x exposed pixels x 4 B idle | resident grows with photos seen |
| M3 | Resident bytes per visible pixel | sharp RGBA8 at rest: 4.5-6 B per exposed image pixel (tile rounding at edges) | > 8 B (worse than today's CPU copy + texture) |
| M4 | Time to first sharp pixel | thumbnail 360 x 240 px from a 640 x 427 JPEG: decode ~2-4 ms on a worker, landing next turn: < 1 frame + decode; from the disk cache: < 1 ms | > 2 frames + decode at an idle machine |
| M5 | YUV 4:2:0 slots | 2.67x fewer resident bytes for photos at < 15 % more kernel time per image pixel | more than 30 % kernel time or visible chroma error at 1:1 |
| M6 | Scaled IDCT | 4032 x 3024 JPEG to a 360 x 240 box: 1/8 decode 10x faster than full decode + resample | < 3x |
| M7 | Tiny variant from DC coefficients | a 16 x 16 placeholder for a 640 x 427 JPEG in < 0.3 ms (entropy decode only) | > 1 ms |
| M8 | Disk cache second launch | gallery first frame complete with tiny variants, sharp within decode-free landing (memcpy) | slower than decoding |
