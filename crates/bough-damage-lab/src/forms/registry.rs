//! Formulations by name (CLI and report use these names).

use super::*;

pub const V1: &[&str] = &[
    "full", "zgx", "zgx-vp", "bbox", "tile4", "tile8", "tile16", "tile32", "tile2L", "exact4", "exact8", "exact16",
    "stamps16", "sig16",
];

pub fn make(name: &str) -> Box<dyn Form> {
    let tile = |s: &str| -> u32 { s.parse::<u32>().unwrap().trailing_zeros() };
    match name {
        "full" => Box::new(basic::Full::new()),
        "zgx" => Box::new(basic::Zgx::new(false)),
        "zgx-vp" => Box::new(basic::Zgx::new(true)),
        "bbox" => Box::new(basic::BBox::new()),
        "tile2L" => Box::new(tiles::Tiles2::new(false)),
        "exact2L" => Box::new(tiles::Tiles2::new(true)),
        "hw32" => Box::new(hw::HwForm::new()),
        "hw32c" => {
            let mut f = hw::HwForm::new();
            f.runs = true;
            Box::new(f)
        }
        n if n.starts_with("xb") => Box::new(bytemap::XBytes::new(tile(&n[2..]))),
        n if n.starts_with("bands") => Box::new(bands::Bands::new(tile(&n[5..]))),
        n if n.starts_with("tstamp") => Box::new(tiles::TileStamps::new(tile(&n[6..]))),
        n if n.starts_with("tile") => Box::new(tiles::Tiles::new(tile(&n[4..]), 0)),
        n if n.starts_with("exact") => Box::new(tiles::Tiles::new(tile(&n[5..]), 1)),
        n if n.starts_with("xs") => {
            let c = n.ends_with('c');
            let mut t = tiles::Tiles::new(tile(&n[2..n.len() - c as usize]), 2);
            t.use_sat = true;
            t.runs = c;
            Box::new(t)
        }
        n if n.starts_with("xg") && n.ends_with('c') => {
            let mut t = tiles::Tiles::new(tile(&n[2..n.len() - 1]), 2);
            t.runs = true;
            Box::new(t)
        }
        n if n.starts_with("xg") => Box::new(tiles::Tiles::new(tile(&n[2..]), 2)),
        n if n.starts_with("stamps") => Box::new(stamps::Stamps::new(tile(&n[6..]))),
        n if n.starts_with("sig") => Box::new(sig::Sig::new(tile(&n[3..]))),
        _ => panic!("unknown formulation {name}"),
    }
}

/// Deliberately broken formulations: the verifier must reject every one.
pub mod mutants {
    use super::*;
    pub struct Bug {
        pub kind: u8,
        p: Plane,
        acc: Plane,
        prev: Plane,
        bboxprev: R,
    }
    impl Bug {
        pub fn new(kind: u8) -> Self {
            Bug { kind, p: Plane::new(1, 1, 4), acc: Plane::new(1, 1, 4), prev: Plane::new(1, 1, 4), bboxprev: R::E }
        }
    }
    impl Form for Bug {
        fn name(&self) -> String {
            ["bug-new-only", "bug-no-corners", "bug-glyph-xor", "bug-age1-only", "bug-cull-bbox-only", "bug-drop-last"]
                [self.kind as usize]
                .into()
        }
        fn reset(&mut self, _tr: &Trace, sc: &Scene, _ring: usize) {
            self.p = Plane::new(sc.w, sc.h, 4);
            self.acc = self.p.clone();
            self.prev = self.p.clone();
        }
        fn record(&mut self, f: &Frame, _sc: &Scene) {
            std::mem::swap(&mut self.prev, &mut self.p);
            self.p.clear();
            let p = &mut self.p;
            match self.kind {
                0 => {
                    for e in &f.nodes {
                        if !e.new_b.is_empty() {
                            p.mark(e.new_b)
                        }
                    }
                    for g in &f.groups {
                        if !g.new_b.is_empty() {
                            p.mark(g.new_b)
                        }
                    }
                }
                1 | 2 => {
                    let k = self.kind;
                    let mut one = |o: &PState, n: &PState| {
                        if o == n {
                            return;
                        }
                        if o.cov.is_empty() || n.cov.is_empty() || o.key != n.key || o.kind == SOLID || o.kind == TINT {
                            exact_slot(o, n, &mut |r| p.mark(r));
                        } else if o.kind == ROUND && k == 1 {
                            R::xor(o.cov, n.cov, &mut |r| p.mark(r));
                        } else if o.kind == GLYPH && k == 2 {
                            R::xor(o.cov, n.cov, &mut |r| p.mark(r));
                        } else {
                            exact_slot(o, n, &mut |r| p.mark(r));
                        }
                    };
                    for g in &f.groups {
                        for j in g.at as usize..(g.at + g.n) as usize {
                            one(&f.gold[j], &f.gnew[j]);
                        }
                    }
                    for e in &f.nodes {
                        for k in e.at as usize..(e.at + e.n) as usize {
                            one(&f.olds[k], &f.news[k]);
                        }
                    }
                }
                _ => exact_events(f, &mut |r| p.mark(r)),
            }
        }
        fn repair(&mut self, _surf: usize, age: u32, sc: &Scene, out: &mut Out) {
            self.acc.clear();
            self.acc.or_from(&self.p);
            if age >= 2 && self.kind != 3 {
                self.acc.or_from(&self.prev);
            }
            match self.kind {
                4 => cull_rect(sc, self.acc.px_bbox(), &mut out.list),
                _ => cull_plane(sc, &self.acc, &mut out.list),
            }
            if self.kind == 5 {
                out.list.pop();
            }
            out.close(self.acc.px_bbox());
            self.bboxprev = self.acc.px_bbox();
        }
        fn region(&self, emit: &mut dyn FnMut(R)) {
            self.acc.emit(emit)
        }
        fn mem_bytes(&self) -> usize {
            0
        }
    }
}

pub fn make_any(name: &str) -> Box<dyn Form> {
    let bugs = ["bug-new-only", "bug-no-corners", "bug-glyph-xor", "bug-age1-only", "bug-cull-bbox-only", "bug-drop-last"];
    if let Some(k) = bugs.iter().position(|b| *b == name) {
        return Box::new(mutants::Bug::new(k as u8));
    }
    make(name)
}
