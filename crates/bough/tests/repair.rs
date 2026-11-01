//! `rects_from_plane` against a per-tile oracle: the rects are disjoint and cover exactly the set tiles.
use bough::repair::rects_from_plane;
use bough_core::damage::{MAX_TILES_X, MAX_TILES_Y, TILE};
use bough_core::{NRect, Plane};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> i32 {
        (self.next() % n) as i32
    }
}

#[test]
fn rects_cover_exactly_the_set_tiles() {
    let (w, h) = (2560, 1600);
    let window = NRect::new(0, 0, w, h);
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let mut out = Vec::new();
    let mut sc = bough::repair::RectScratch::default();
    for case in 0..400 {
        let mut p = Plane::EMPTY;
        let n = 1 + rng.below(if case % 4 == 0 { 400 } else { 30 } as u64);
        for _ in 0..n {
            let (x, y) = (rng.below(w as u64 + 100) - 50, rng.below(h as u64 + 100) - 50);
            let (rw, rh) = (1 + rng.below(300), 1 + rng.below(200));
            p.add(NRect::new(x, y, x + rw, y + rh).canon(), window);
        }
        let ok = rects_from_plane(&p, w, h, 512, &mut sc, &mut out);
        if !ok {
            assert_eq!(out.len(), 1);
            continue;
        }
        let (tw, th) = ((w / TILE) as usize + 1, (h / TILE) as usize + 1);
        assert!(tw <= MAX_TILES_X && th <= MAX_TILES_Y);
        let mut cover = vec![0u8; tw * th];
        for r in &out {
            assert!(r[0] % TILE == 0 && r[1] % TILE == 0 && r[0] < r[2] && r[1] < r[3]);
            for ty in r[1] / TILE..(r[3] + TILE - 1) / TILE {
                for tx in r[0] / TILE..(r[2] + TILE - 1) / TILE {
                    cover[ty as usize * tw + tx as usize] += 1;
                }
            }
        }
        for ty in 0..th {
            for tx in 0..tw {
                let want = p.tile(tx, ty) as u8;
                let want = if (tx as i32) * TILE >= w || (ty as i32) * TILE >= h { 0 } else { want };
                assert_eq!(cover[ty * tw + tx], want, "case {case} tile ({tx},{ty})");
            }
        }
    }
}
