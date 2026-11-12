# The scene image: format, loader, and what it is worth (threads lane, for the e2e lane)

Status: **built for columns** (`Arena::save` / `Arena::open`, `World::save` / `World::open` / `World::adopt`,
`crates/bough-x/tests/image.rs`); sections for pixels, glyphs and shapes are reserved in the head and not
written yet. Read `ARENA.md` first: the image is the arena.

## 1. Format (v1, magic `boughx01`)

```text
0          shared part, 256 KiB: head (4 KiB) | dirty bitmap (zero) | ranges
256 KiB    one half: plan | order | spaces | live | columns, up to the last used line, padded to 64 KiB
then       optional sections, each at a 64 KiB boundary, named in head.sections as (file offset, bytes):
           0 the first frame's pixels   1 glyph columns   2 layout shapes   3 spare
```

The file is the memory, byte for byte. The head carries: magic; a fold of the layout constants; the identity
of the binary that wrote it (size and modification time of the executable); the counts and the bump cursor;
the surface size; the block the scene was shaded with; a checksum of the structural part (head, plan, order,
spaces, live words, ranges). The sheet at 10,000 rows (764 records exposed) is 458,752 bytes, of which about
100 KiB is ever touched.

## 2. What makes a page mappable read-only and copy-on-write on first write

1. **No address in it.** Pages are named by lines from the half's base; kernels and descriptors by 32-bit
   offsets from an anchor symbol of the binary. So the bytes are right wherever the file is mapped and
   wherever the binary is loaded, and (if the image is linked into the executable, section 5) the linker has
   no relocation to apply to it, so the loader never touches its pages.
2. **Nothing is written on load.** No fix-up pass: the loader reads the head and the structural part to check
   them and writes nothing. Pages stay shared with the page cache until the first write.
3. **File layout = memory layout**, the half starting on a 64 KiB boundary of the file (a multiple of every
   VM page size in use).
4. **Mapped `MAP_PRIVATE`**: the first write to a page copies that page and no other. Both halves map the same
   file pages (W at its address, F at its address), so before the first turn they cost no memory of their own;
   after it, W owns the pages the turn wrote and F the pages a publish copied.
5. **Any bytes are a record.** The checksum covers what a pass follows without checking; record words and
   rects are not covered and need not be, because a kernel indexes nothing with them. That rule is what the
   raster lane's kernels have to keep.

## 3. Loader

```rust
let mut world = World::<B>::open(path)?;        // map; check head, bounds, checksum. Nothing mounted.
world.paint_front(&world.shown.clone(), &mut backend);   // the first frame: the view has not run
world.adopt(&view, NWORDS)?;                    // before the first turn (or at once, if exactness comes first)
```

* `open` refuses (`Refused::{Io, Layout, Binary, Bounds, Checksum}`): not an image or other layout constants;
  written by another binary; a count or offset outside the file; a structural part that is not what was
  written. The caller mounts instead.
* `adopt` runs the view with the re-description sink against the mapped columns. Allocation requests are
  replayed (the same requests in the same order get the same columns, checked against the image), so the
  store tree is found again without writing; every operand and rect the view gives is compared with what is
  mapped, and what differs is written and damaged like any change. An image of another *state* is therefore
  repaired; an image of another *shape* (other columns, other sizes) fails `adopt`, and the caller drops the
  world and mounts. A list's window is taken as it is only if its live bits and rects are what the current
  state exposes; otherwise its rows are written anew.
* Until `adopt`, the world can be painted and nothing else.

Gates (Mac and ilo-prod): the mapped scene paints the saved pixels before the view runs; adopting the same
state damages no tile and writes no page, and 40 further steps match a fresh mount; a stale state (theme,
scroll, an edit, a row inserted) is repaired to a fresh mount's pixels; a view with no rows is refused; 3,000
corruptions (truncations, single bits, bursts in the structural part and in the columns): 762 refused, 252
repaired, 1,986 without effect, none out of bounds. Threat model: the image is a cache the application wrote
for itself; it is protected against corruption, not against a forger with the same permissions.

## 4. What it is worth: measured, and not much for the scene itself

mothership (quiet), 10,000-row sheet, 764 records, median of 200, wall time including system calls:

| path | user instructions | wall, warm | wall, caches cold |
|---|---:|---:|---:|
| mount (allocate, discover, write 764 records) | 86,398 | 41.3 us | 55.8 us |
| map the image (`open`) | 7,620 | 25.5 us | 37.4 us |
| map and adopt | 95,516 | 40.8 us | 56.9 us |
| a full frame, reference raster, 2560 x 1600 | 8,719,204 | 2,116 us | 2,295 us |

Both mount and map are dominated by the same system calls (reserve, map, unmap of the previous world in this
cell). **On the new core the scene's share of start-up is about 40 microseconds mounted and 25 mapped; the
image saves about 15.** That is the prediction of the journal (I1) killed by its own number: mapping is 0.62x
of mount in wall time, not under a fifth. The 65 ms and 565 ms on the scoreboard are not in mounting the
exposed rows once the core is this one: they are in the old core's mount of everything, and in what surrounds
the scene (process start, fonts, the first raster, the compositor round trips, the model's own construction:
building the 1,000,000-row model is the application's time and can run after the first present).

Where an image still earns its place, in order:

1. **The first frame's pixels** (section 0 of the format): 2.1 ms of raster at this size is fifty times the
   scene. If the pixel section is page-aligned XRGB of the configured size, the first `wl_shm` pool can be
   created from the image's own file descriptor at that offset: the compositor maps the page cache, and the
   first commit needs no raster and no copy. Valid only if the configure size and the state match (the head
   has both); otherwise raster as usual. Not built; it needs the e2e lane's wl_shm path to say what it accepts
   (a read-only regular file as a pool, stride, format).
2. **Mount-stage products that are expensive per record**, when they exist: shaped glyph columns and layout
   shapes (sections 1 and 2). With `Num` formatted while shading and rects by arithmetic, the sheet has none.
3. The column image as built: worth keeping because it is free (the arena is already position independent),
   and because `adopt` is the same code path as "the first mount is build-time data": a build step that
   mounts the initial state and saves is exactly `World::save`.

## 5. Build-time image inside the executable (design, not built)

For "nothing before main" and no file to find: reserve a zero-initialised writable section of fixed size in
the binary (a `static mut` with `#[link_section]`), build, run the binary once with a bake switch (mount the
initial state headless, publish, save), and patch the bytes into that section of the executable file. Code
layout does not change, so the kernel offsets in the image are the final binary's, and the binary-identity
check becomes a constant. At start the loader has already mapped the section privately from the executable:
the working half is that memory, with no system call; the published half is a second private mapping of the
same file range (one call) made when the first frame needs it, or the first frame reads W before any turn.
This needs `Arena` to accept memory it did not reserve; the arena's own layout does not change.
