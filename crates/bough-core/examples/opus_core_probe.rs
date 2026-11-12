//! Opus collaborator probes (round 6, `docs/lanes/core/IDEAS-OPUS.md`): instructions on the calling thread for
//! the core write paths, split into their parts, against two candidate formulations:
//!
//! * a deferred ink write (store + group bit; summaries and damage settled once per dirty page per frame), and
//! * a gapped directory (a packed-memory array of page slots: inserting a page shifts inside one 64-slot chunk
//!   and adds to the chunk counts after it; no global re-index).
//!
//!     cargo run --release -p bough-core --example opus_core_probe
use bough_core::bough::Handle;
use bough_core::nrect::NRect;
use bough_core::page::{CAP, GROUP, NGROUPS, Page};
use bough_core::{Bough, Plane, RecordInit};
use std::hint::black_box;

unsafe extern "C" {
    fn thread_selfcounts(kind: i32, buf: *mut u64, nbytes: usize) -> i32;
}
#[inline(never)]
fn tinstr() -> u64 {
    let mut b = [0u64; 2];
    unsafe { thread_selfcounts(1, b.as_mut_ptr(), 16) };
    b[0]
}
/// Instructions per call of `f` (slope between 2k and 6k calls: the counter's own cost cancels).
fn per(mut f: impl FnMut(usize)) -> f64 {
    let mut run = |n: usize| {
        let t0 = tinstr();
        for i in 0..n {
            f(i);
        }
        tinstr() - t0
    };
    run(1000);
    let (a, b) = (run(2000), run(6000));
    (b - a) as f64 / 4000.0
}

/// Row-like boxes 944 x 30 px, record i at y = i mod 800 (all inside a 1280 x 800 window), 64 to a page.
fn rows(n: usize) -> Vec<RecordInit> {
    (0..n)
        .map(|i| RecordInit::new(1, NRect::new(8, i as i32 % 800, 952, i as i32 % 800 + 30), 1, i as u32))
        .collect()
}

fn main() {
    let n = 10_000;
    let mut b = Bough::new(1280, 800);
    b.insert_run(0, &rows(n));
    let hs: Vec<Handle> = (0..n).map(|p| b.attach_handle(p)).collect();
    b.next_epoch();
    // a record moves by one pixel and back (two inks), handles spread over the pages
    let pick = |i: usize| hs[(i * 7919) % n];
    let inks = |i: usize| {
        let y = ((i * 7919) % n) as i32 % 800;
        [NRect::new(8, y, 952, y + 30), NRect::new(9, y, 953, y + 30)]
    };
    let with = per(|i| {
        b.set_ink_handle(black_box(pick(i / 2)), inks(i / 2)[i & 1]);
    });
    let edges = per(|i| {
        b.set_ink_handle_edges(black_box(pick(i / 2)), inks(i / 2)[i & 1], 5, 2);
    });
    let nodmg = per(|i| {
        b.set_ink_handle_nodamage(black_box(pick(i / 2)), inks(i / 2)[i & 1]);
    });
    let touch = per(|i| {
        b.touch_handle(black_box(pick(i)));
    });
    let mut batch: Vec<(Handle, NRect)> = Vec::with_capacity(64);
    let batch64 = per(|i| {
        batch.clear();
        for k in 0..64 {
            let j = i * 64 + k;
            batch.push((pick(j / 2), inks(j / 2)[j & 1]));
        }
        b.set_inks_handles(black_box(&batch), true);
    }) / 64.0;
    let batch64n = per(|i| {
        batch.clear();
        for k in 0..64 {
            let j = i * 64 + k;
            batch.push((pick(j / 2), inks(j / 2)[j & 1]));
        }
        b.set_inks_handles(black_box(&batch), false);
    }) / 64.0;
    let base = per(|i| {
        black_box((pick(i / 2), inks(i / 2)[i & 1]));
    });
    let hit = per(|i| {
        black_box(b.hit(black_box(100 + (i as i32 % 700)), black_box((i as i32 * 13) % 790)));
    });
    println!(
        "core writes, 10,000 rows (157 pages), random handles, one-pixel moves; harness overhead subtracted ({base:.1})"
    );
    println!("  set_ink_handle (damage old+new):        {:7.1}", with - base);
    println!("  set_ink_handle_edges (band 2, x edges): {:7.1}", edges - base);
    println!("  set_ink_handle_nodamage:                {:7.1}", nodmg - base);
    println!("  touch_handle:                           {:7.1}", touch - base);
    println!(
        "  set_inks_handles, 64 a batch, damage:   {:7.1} per write",
        batch64 - base
    );
    println!(
        "  set_inks_handles, 64 a batch, no damage:{:7.1} per write",
        batch64n - base
    );
    println!("  hit (random point in the window):       {:7.1}", hit);

    // ---------------------------------------------------------------------------------------------------
    // Candidate: deferred writes on packed handles. A handle is one u64 word (page id 20 bits, slot 6 bits,
    // generation 32 bits): no pointer in the table, the page is `base + id * 1472`. A write stores the stored
    // ink and ORs a group bit; the page's first write of the frame snapshots its ink column (the "before"
    // image the damage is derived from). `settle` (once per frame per dirty page): changed slots by an 8-lane
    // compare per dirty group, their group summaries, old and new marked.
    let npages = n.div_ceil(CAP);
    let mut pages: Vec<Page> = Vec::with_capacity(npages);
    for p in 0..npages {
        let mut pg = unsafe { std::mem::zeroed::<Page>() };
        pg.depth = [0xFFFF; CAP];
        pg.ink = [NRect::EMPTY; CAP];
        for s in 0..CAP {
            let i = p * CAP + s;
            if i < n {
                let y = (i as i32) % 800;
                pg.ink[s] = NRect::new(8, y, 952, y + 30);
                pg.depth[s] = 1;
            }
        }
        for g in 0..NGROUPS {
            pg.regroup(g);
        }
        pg.id = p as u32;
        pages.push(pg);
    }
    let tab: Vec<u64> = (0..n)
        .map(|i| ((i / CAP) as u64) << 38 | ((i % CAP) as u64) << 32 | 7)
        .collect();
    let mut before: Vec<[NRect; CAP]> = vec![[NRect::EMPTY; CAP]; npages];
    let mut dirty: Vec<u32> = Vec::with_capacity(npages);
    let mut plane = Plane::EMPTY;
    plane.n = 25;
    let window = NRect::new(0, 0, 1280, 800);
    #[inline(always)]
    fn write(
        pages: &mut [Page],
        before: &mut [[NRect; CAP]],
        dirty: &mut Vec<u32>,
        tab: &[u64],
        h: u32,
        generation: u32,
        ink: NRect,
    ) -> bool {
        let e = unsafe { *tab.get_unchecked(h as usize) };
        if e as u32 != generation {
            return false;
        }
        let (pid, slot) = ((e >> 38) as usize, ((e >> 32) & 63) as usize);
        let p = unsafe { pages.get_unchecked_mut(pid) };
        if p.dirty == 0 {
            unsafe { *before.get_unchecked_mut(pid) = p.ink };
            dirty.push(pid as u32);
        }
        unsafe { *p.ink.get_unchecked_mut(slot) = ink.canon() };
        p.dirty |= 1 << (slot / GROUP);
        true
    }
    fn settle(pages: &mut [Page], before: &[[NRect; CAP]], dirty: &mut Vec<u32>, plane: &mut Plane, window: NRect) {
        for &pid in dirty.iter() {
            let p = &mut pages[pid as usize];
            let old = &before[pid as usize];
            let mut gm = core::mem::take(&mut p.dirty) as u32;
            while gm != 0 {
                let g = gm.trailing_zeros() as usize;
                gm &= gm - 1;
                // changed slots of the group: 8 rects compared as two 64-byte blocks
                let (a, o) = (&p.ink[g * GROUP..g * GROUP + GROUP], &old[g * GROUP..g * GROUP + GROUP]);
                let mut cm = 0u32;
                for l in 0..GROUP {
                    cm |= ((a[l] != o[l]) as u32) << l;
                }
                while cm != 0 {
                    let l = cm.trailing_zeros() as usize;
                    cm &= cm - 1;
                    plane.add(o[l], window);
                    plane.add(a[l], window);
                }
                p.regroup(g);
            }
        }
        dirty.clear();
    }
    // the same 1-pixel moves; settle every 64 writes (a frame of 64 writes)
    let defer = per(|i| {
        let k = ((i / 2) * 7919) % n;
        let y = (k as i32) % 800;
        let ink = if i & 1 == 0 {
            NRect::new(9, y, 953, y + 30)
        } else {
            NRect::new(8, y, 952, y + 30)
        };
        write(&mut pages, &mut before, &mut dirty, &tab, black_box(k as u32), 7, ink);
        if i % 64 == 63 {
            settle(&mut pages, &before, &mut dirty, &mut plane, window);
        }
    });
    let defer_base = per(|i| {
        let k = ((i / 2) * 7919) % n;
        let y = (k as i32) % 800;
        black_box((
            k as u32,
            if i & 1 == 0 {
                NRect::new(9, y, 953, y + 30)
            } else {
                NRect::new(8, y, 952, y + 30)
            },
        ));
    });
    println!("candidate: packed u64 handle + deferred write, settled per 64 writes:");
    println!(
        "  v1 page snapshot on first touch + settle (damage old+new): {:7.1}",
        defer - defer_base
    );

    // v2: no snapshot. The write logs (old, new) — the old ink is in the line the store touches anyway — and
    // the page; settle marks each logged pair and regroups each dirty group once.
    let mut log: Vec<(NRect, NRect)> = Vec::with_capacity(256);
    #[inline(always)]
    fn write2(
        pages: &mut [Page],
        dirty: &mut Vec<u32>,
        log: &mut Vec<(NRect, NRect)>,
        tab: &[u64],
        h: u32,
        generation: u32,
        ink: NRect,
    ) -> bool {
        let e = unsafe { *tab.get_unchecked(h as usize) };
        if e as u32 != generation {
            return false;
        }
        let (pid, slot) = ((e >> 38) as usize, ((e >> 32) & 63) as usize);
        let p = unsafe { pages.get_unchecked_mut(pid) };
        let ink = ink.canon();
        let old = unsafe { *p.ink.get_unchecked(slot) };
        if old == ink {
            return true;
        }
        if p.dirty == 0 {
            dirty.push(pid as u32);
        }
        log.push((old, ink));
        unsafe { *p.ink.get_unchecked_mut(slot) = ink };
        p.dirty |= 1 << (slot / GROUP);
        true
    }
    fn settle2(
        pages: &mut [Page],
        dirty: &mut Vec<u32>,
        log: &mut Vec<(NRect, NRect)>,
        plane: &mut Plane,
        window: NRect,
        mark: bool,
    ) {
        if mark {
            for &(o, a) in log.iter() {
                plane.add(o, window);
                plane.add(a, window);
            }
        }
        log.clear();
        for &pid in dirty.iter() {
            let p = unsafe { pages.get_unchecked_mut(pid as usize) };
            let mut gm = core::mem::take(&mut p.dirty) as u32;
            while gm != 0 {
                p.regroup(gm.trailing_zeros() as usize);
                gm &= gm - 1;
            }
        }
        dirty.clear();
    }
    for mark in [true, false] {
        let d2 = per(|i| {
            let k = ((i / 2) * 7919) % n;
            let y = (k as i32) % 800;
            let ink = if i & 1 == 0 {
                NRect::new(9, y, 953, y + 30)
            } else {
                NRect::new(8, y, 952, y + 30)
            };
            write2(&mut pages, &mut dirty, &mut log, &tab, black_box(k as u32), 7, ink);
            if i % 64 == 63 {
                settle2(&mut pages, &mut dirty, &mut log, &mut plane, window, mark);
            }
        });
        println!(
            "  v2 logged (old,new), settle per 64 writes, marks {}: {:7.1}",
            if mark { "on " } else { "off" },
            d2 - defer_base
        );
    }
    // the same writes in paint order (a list reflowing / scrolling: consecutive records), 64 a frame
    let d3 = per(|i| {
        let (f, j) = (i / 64, i % 64);
        let k = ((f / 2) * 64 + j) % n;
        let y = (k as i32) % 800;
        let ink = if f & 1 == 0 {
            NRect::new(9, y, 953, y + 30)
        } else {
            NRect::new(8, y, 952, y + 30)
        };
        write2(&mut pages, &mut dirty, &mut log, &tab, black_box(k as u32), 7, ink);
        if i % 64 == 63 {
            settle2(&mut pages, &mut dirty, &mut log, &mut plane, window, true);
        }
    });
    let mut bseq: Vec<(Handle, NRect)> = Vec::with_capacity(64);
    let cur_seq = per(|i| {
        bseq.clear();
        for k in 0..64 {
            let j = ((i / 2) * 64 + k) % n;
            let y = (j as i32) % 800;
            let ink = if i & 1 == 0 {
                NRect::new(9, y, 953, y + 30)
            } else {
                NRect::new(8, y, 952, y + 30)
            };
            bseq.push((hs[j], ink));
        }
        b.set_inks_handles(black_box(&bseq), true);
    }) / 64.0;
    println!(
        "  consecutive records (64 a frame): v2 {:7.1} vs set_inks_handles today {:7.1} (incl. ~{:.0} of harness)",
        d3 - defer_base,
        cur_seq,
        defer_base
    );

    // ---------------------------------------------------------------------------------------------------
    // Candidate: gapped directory. The present directory shifts five columns and re-indexes every later page
    // header on a page insert (TABLE-R6: insert8-front 556,844 instructions warm at 1e6). Here: directory slots
    // grouped in chunks of 64 slots, each chunk at most 48 live (16 gaps, sentinel EMPTY ink and length 0);
    // a page insert shifts inside its chunk only (the dir index of the pages it moves is rewritten, at most
    // 63), and the chunk record counts after it take one NEON add. A full chunk splits (amortised).
    const CH: usize = 64;
    struct Gapped {
        ptr: Vec<u64>,
        ink: Vec<NRect>,
        len: Vec<u32>,
        md: Vec<u16>,
        id: Vec<u32>,
        live: Vec<u8>,
        /// records before chunk c (a prefix count per chunk; within a chunk: a scan of <= 64 lengths)
        cstart: Vec<u32>,
        hdr_dir: Vec<u32>, // stands for the pages' `dir` field (a write per moved page)
    }
    impl Gapped {
        fn new(pages: usize) -> Gapped {
            let chunks = pages.div_ceil(48) + 1;
            let slots = chunks * CH;
            let mut g = Gapped {
                ptr: vec![0; slots],
                ink: vec![NRect::EMPTY; slots],
                len: vec![0; slots],
                md: vec![0xFFFF; slots],
                id: vec![0; slots],
                live: vec![0; chunks],
                cstart: vec![0; chunks + 1],
                hdr_dir: vec![0; pages * 2],
            };
            let mut p = 0;
            for c in 0..chunks {
                for s in 0..48 {
                    if p < pages {
                        let d = c * CH + s;
                        g.ptr[d] = p as u64;
                        g.len[d] = 64;
                        g.ink[d] = NRect::new(0, 0, 10, 10);
                        g.md[d] = 1;
                        g.id[d] = p as u32;
                        g.hdr_dir[p] = d as u32;
                        g.live[c] += 1;
                        p += 1;
                    }
                }
                g.cstart[c + 1] = g.cstart[c] + 64 * g.live[c] as u32;
            }
            g
        }
        /// Insert a page of `len` records before the page at slot `d` (live chunks are packed at their front).
        #[inline(never)]
        fn insert_before(&mut self, d: usize, pid: usize, len: u32) {
            let c = d / CH;
            let end = c * CH + self.live[c] as usize;
            assert!(end < (c + 1) * CH, "chunk full: split (not in this probe)");
            // shift [d, end) one slot right in every column; moved pages learn their new slot
            self.ptr.copy_within(d..end, d + 1);
            self.ink.copy_within(d..end, d + 1);
            self.len.copy_within(d..end, d + 1);
            self.md.copy_within(d..end, d + 1);
            self.id.copy_within(d..end, d + 1);
            for k in d + 1..=end {
                let p = self.id[k] as usize;
                self.hdr_dir[p] = k as u32;
            }
            self.ptr[d] = pid as u64;
            self.id[d] = pid as u32;
            self.len[d] = len;
            self.ink[d] = NRect::new(0, 0, 10, 10);
            self.md[d] = 1;
            self.hdr_dir[pid] = d as u32;
            self.live[c] += 1;
            for x in &mut self.cstart[c + 1..] {
                *x += len;
            }
        }
        #[inline(never)]
        fn remove_at(&mut self, d: usize) {
            let c = d / CH;
            let end = c * CH + self.live[c] as usize;
            let len = self.len[d];
            self.ptr.copy_within(d + 1..end, d);
            self.ink.copy_within(d + 1..end, d);
            self.len.copy_within(d + 1..end, d);
            self.md.copy_within(d + 1..end, d);
            self.id.copy_within(d + 1..end, d);
            for k in d..end - 1 {
                let p = self.id[k] as usize;
                self.hdr_dir[p] = k as u32;
            }
            self.ink[end - 1] = NRect::EMPTY;
            self.len[end - 1] = 0;
            self.md[end - 1] = 0xFFFF;
            self.live[c] -= 1;
            for x in &mut self.cstart[c + 1..] {
                *x -= len;
            }
        }
    }
    for &records in &[1_000usize, 10_000, 100_000, 1_000_000] {
        let pages = records / 64;
        let mut g = Gapped::new(pages);
        // the front page's chunk: insert a page and take it out again (insert8-front splits a page: one new page)
        let ins = per(|i| {
            if i & 1 == 0 {
                g.insert_before(black_box(0), pages, 8);
            } else {
                g.remove_at(black_box(0));
            }
        });
        println!(
            "candidate: gapped directory, {records:>9} records ({pages:>5} pages): page insert/remove at the front {ins:9.1}"
        );
    }

    // ---------------------------------------------------------------------------------------------------
    // Marks: what one damage mark costs today (`Plane::add`, a 944 x 30 px row), against a mark kept as a
    // u128 column mask ORed into the rows it spans, with the y-range kept in registers by the caller.
    let mut pl = Plane::EMPTY;
    pl.n = 25;
    let mark_now = per(|i| {
        let y = (i as i32 * 37) % 770;
        pl.add(black_box(NRect::new(8, y, 952, y + 30)), window);
    });
    let mut rows128 = [0u128; 128];
    let (mut y0, mut y1) = (u32::MAX, 0u32);
    let mark_min = per(|i| {
        let y = (i as i32 * 37) % 770;
        let r = black_box(NRect::new(8, y, 952, y + 30));
        let [x0, ry0, x1, ry1] = r.to_xyxy();
        let (tx0, ty0, tx1, ty1) = (
            (x0 >> 5) as u32,
            (ry0 >> 5) as u32,
            (((x1 - 1) >> 5) + 1) as u32,
            (((ry1 - 1) >> 5) + 1) as u32,
        );
        let m = (u128::MAX >> (128 - (tx1 - tx0))) << tx0;
        for t in ty0..ty1 {
            unsafe { *rows128.get_unchecked_mut(t as usize) |= m };
        }
        y0 = y0.min(ty0);
        y1 = y1.max(ty1);
    });
    black_box((&rows128, y0, y1));
    let mbase = per(|i| {
        let y = (i as i32 * 37) % 770;
        black_box(NRect::new(8, y, 952, y + 30));
    });
    println!(
        "damage mark of one 944x30 row: Plane::add {:6.1}; u128 mask, y-range in registers {:6.1}",
        mark_now - mbase,
        mark_min - mbase
    );

    // ---------------------------------------------------------------------------------------------------
    // Geometry: a record's ink from its laid-out box, as `Rt::ink_rect` does it (f32 box on the 1/256 grid +
    // the space's i32 offset, floor/ceil, then the 16-bit NRect form), against i32 fixed point (1/256 px)
    // where the ink is two shifts and an add per edge; one record, then a 64-slot column.
    let sp = [37i32, -1200];
    let fbox: Vec<[f32; 4]> = (0..64)
        .map(|i| [12.25 + i as f32, 1000.0 + 32.0 * i as f32, 944.5, 30.0])
        .collect();
    let ibox: Vec<[i32; 4]> = fbox.iter().map(|b| b.map(|v| (v * 256.0) as i32)).collect();
    let margin = 1.0f32;
    let ink_f = per(|i| {
        let b = fbox[i & 63];
        let (o0, o1) = (black_box(sp[0]) as f32, black_box(sp[1]) as f32);
        let x0 = (o0 + b[0] - margin).floor() as i32;
        let y0 = (o1 + b[1] - margin).floor() as i32;
        let x1 = (o0 + b[0] + b[2] + margin).ceil() as i32;
        let y1 = (o1 + b[1] + b[3] + margin).ceil() as i32;
        black_box(NRect::new(x0, y0, x1, y1));
    });
    let ink_i = per(|i| {
        let b = ibox[i & 63];
        let (o0, o1) = (black_box(sp[0]), black_box(sp[1]));
        let m = 256;
        let x0 = ((b[0] - m) >> 8) + o0;
        let y0 = ((b[1] - m) >> 8) + o1;
        let x1 = ((b[0] + b[2] + m + 255) >> 8) + o0;
        let y1 = ((b[1] + b[3] + m + 255) >> 8) + o1;
        black_box(NRect::new(x0, y0, x1, y1));
    });
    let ib = per(|i| {
        black_box((fbox[i & 63], ibox[i & 63], black_box(sp[0])));
    });
    // a whole column: 64 boxes -> 64 inks (the shape a write-back or a subtree move takes in fixed point)
    let mut out = [NRect::EMPTY; 64];
    let col_i = per(|_| {
        let (o0, o1) = (black_box(sp[0]), black_box(sp[1]));
        for (k, b) in ibox.iter().enumerate() {
            let x0 = ((b[0] - 256) >> 8) + o0;
            let y0 = ((b[1] - 256) >> 8) + o1;
            let x1 = ((b[0] + b[2] + 511) >> 8) + o0;
            let y1 = ((b[1] + b[3] + 511) >> 8) + o1;
            let c = |v: i32| v.clamp(-32767, 32767) as i16;
            out[k] = NRect([c(x0), c(y0), -c(x1), -c(y1)]);
        }
        black_box(&out);
    }) / 64.0;
    println!(
        "ink from a box: f32 floor/ceil {:5.1}; i32 1/256 fixed point {:5.1}; fixed point, 64-slot column {:5.1} per slot",
        ink_f - ib,
        ink_i - ib,
        col_i
    );
}
