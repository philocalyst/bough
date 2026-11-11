//! Baselines: full redraw, single bounding rect (Chromium `DamageAccumulator`),
//! and zgx (scene.rs `add_damage_as` / `DamageGrid` + zgx-gpu `cluster_damage`
//! / `merge_damage`, ported to device px: zgx's 8 pt cell = 16 px, its 16 pt
//! cluster start = 32 px).

use super::*;

pub struct Full {
    vp: R,
}
impl Full {
    pub fn new() -> Self {
        Full { vp: R::E }
    }
}
impl Default for Full {
    fn default() -> Self {
        Self::new()
    }
}
impl Form for Full {
    fn name(&self) -> String {
        "full".into()
    }
    fn reset(&mut self, _tr: &Trace, sc: &Scene, _ring: usize) {
        self.vp = sc.viewport;
    }
    fn record(&mut self, _f: &Frame, _sc: &Scene) {}
    fn repair(&mut self, _surf: usize, _age: u32, sc: &Scene, out: &mut Out) {
        cull_rect(sc, self.vp, &mut out.list);
        out.close(self.vp);
    }
    fn region(&self, emit: &mut dyn FnMut(R)) {
        emit(self.vp)
    }
    fn mem_bytes(&self) -> usize {
        0
    }
}

pub struct BBox {
    hist: [R; 4],
    ep: usize,
    last: R,
}
impl BBox {
    pub fn new() -> Self {
        BBox { hist: [R::E; 4], ep: 0, last: R::E }
    }
}
impl Default for BBox {
    fn default() -> Self {
        Self::new()
    }
}
impl Form for BBox {
    fn name(&self) -> String {
        "bbox".into()
    }
    fn reset(&mut self, _tr: &Trace, _sc: &Scene, _ring: usize) {
        *self = BBox::new();
    }
    fn record(&mut self, f: &Frame, _sc: &Scene) {
        let mut u = R::E;
        bounds_events(f, &mut |r| u = u.union(r));
        self.ep += 1;
        self.hist[self.ep & 3] = u;
    }
    fn repair(&mut self, _surf: usize, age: u32, sc: &Scene, out: &mut Out) {
        let mut u = R::E;
        for k in 0..age as usize {
            u = u.union(self.hist[(self.ep - k) & 3]);
        }
        self.last = u;
        cull_rect(sc, u, &mut out.list);
        out.close(u);
    }
    fn region(&self, emit: &mut dyn FnMut(R)) {
        if !self.last.is_empty() {
            emit(self.last)
        }
    }
    fn mem_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
    }
}

const DAMAGE_RECTS: usize = 64;
const MAX_REGIONS: usize = 16;
const NEAR_PIXELS: i64 = 16_384;

pub struct Zgx {
    /// `true`: >64 rects collapse to the viewport (the older zgx); `false`:
    /// >64 rects move to the 16 px `DamageGrid` (current zgx).
    collapse: bool,
    vp: R,
    list: Vec<R>,
    recent: [R; 4],
    grid: Option<Plane>,
    hist: Vec<Vec<R>>,
    ep: usize,
    regions: Vec<R>,
    cell_last: i32,
    damage: Vec<R>,
    // cluster scratch
    owner: Vec<u32>,
    parent: Vec<u32>,
}

impl Zgx {
    pub fn new(collapse: bool) -> Self {
        Zgx {
            collapse,
            vp: R::E,
            list: vec![],
            recent: [R::E; 4],
            grid: None,
            hist: vec![vec![]; 4],
            ep: 0,
            regions: vec![],
            cell_last: 32,
            damage: vec![],
            owner: vec![],
            parent: vec![],
        }
    }
    #[inline]
    fn add(&mut self, r: R) {
        let mut r = r.inter(self.vp);
        if r.is_empty() {
            return;
        }
        if self.list.first() == Some(&self.vp) {
            return;
        }
        if self.recent.iter().any(|o| o.contains(r)) {
            return;
        }
        self.recent.rotate_right(1);
        self.recent[0] = r;
        if let Some(g) = &mut self.grid {
            g.mark(r);
            return;
        }
        while let Some(i) = self.list.iter().position(|o| o.meets(r)) {
            r = self.list.swap_remove(i).union(r);
        }
        self.list.push(r);
        if self.list.len() > DAMAGE_RECTS {
            if self.collapse {
                self.list.clear();
                self.list.push(self.vp);
            } else {
                let mut g = Plane::new(self.vp.x1, self.vp.y1, 4);
                for r in self.list.drain(..) {
                    g.mark(r);
                }
                self.grid = Some(g);
            }
        }
    }
    /// zgx `DamageGrid::rects`: each row's runs, a run repeated straight down
    /// joining as one rectangle.
    fn grid_rects(g: &Plane, vp: R, done: &mut Vec<R>) {
        let mut open: Vec<(i32, i32, i32)> = vec![];
        let mut next: Vec<(i32, i32, i32)> = vec![];
        let t = 1 << g.sh;
        let close = |(l, r, top): (i32, i32, i32), row: i32, done: &mut Vec<R>| {
            let rect = R::new(l * t, top * t, r * t, row * t).clip(vp);
            if !rect.is_empty() {
                done.push(rect);
            }
        };
        for row in 0..=g.rows {
            let mut at = 0;
            if row < g.rows {
                let mut c = 0;
                while c < g.cols {
                    let w = g.bits[row as usize * g.stride + (c >> 6) as usize] >> (c & 63);
                    if w == 0 {
                        c = ((c >> 6) + 1) << 6;
                        continue;
                    }
                    c += w.trailing_zeros() as i32;
                    if c >= g.cols {
                        break;
                    }
                    let start = c;
                    while c < g.cols && g.get(c, row) {
                        c += 1;
                    }
                    while at < open.len() && open[at].0 < start {
                        close(open[at], row, done);
                        at += 1;
                    }
                    let top = match open.get(at) {
                        Some(&(l, r, top)) if (l, r) == (start, c) => {
                            at += 1;
                            top
                        }
                        _ => row,
                    };
                    next.push((start, c, top));
                }
            }
            for &o in &open[at..] {
                close(o, row, done);
            }
            std::mem::swap(&mut open, &mut next);
            next.clear();
        }
    }
    fn root(parent: &mut [u32], mut i: u32) -> u32 {
        while parent[i as usize] != i {
            parent[i as usize] = parent[parent[i as usize] as usize];
            i = parent[i as usize];
        }
        i
    }
    fn groups(&mut self, cell: i32) -> Option<Vec<R>> {
        let cols = (self.vp.x1 + cell - 1) / cell;
        let rows = (self.vp.y1 + cell - 1) / cell;
        self.owner.clear();
        self.owner.resize((cols * rows) as usize, u32::MAX);
        self.parent.clear();
        self.parent.extend(0..self.damage.len() as u32);
        let mut count = self.damage.len();
        for (i, r) in self.damage.iter().enumerate() {
            let mut group = i as u32;
            for row in (r.y0 / cell).min(rows)..((r.y1 + cell - 1) / cell).min(rows) {
                for col in (r.x0 / cell).min(cols)..((r.x1 + cell - 1) / cell).min(cols) {
                    let slot = &mut self.owner[(row * cols + col) as usize];
                    if *slot == u32::MAX {
                        *slot = i as u32;
                    } else {
                        let other = Self::root(&mut self.parent, *slot);
                        count -= (group != other) as usize;
                        self.parent[group as usize] = other;
                        group = other;
                    }
                }
            }
        }
        if count > MAX_REGIONS && cell < self.vp.x1.max(self.vp.y1) {
            return None;
        }
        let mut bounds = vec![R::E; self.damage.len()];
        for i in 0..self.damage.len() {
            let g = Self::root(&mut self.parent, i as u32) as usize;
            bounds[g] = bounds[g].union(self.damage[i]);
        }
        Some(bounds.into_iter().filter(|r| !r.is_empty()).collect())
    }
    fn cluster(&mut self) {
        let mut cell = self.cell_last.max(32);
        let mut found = self.groups(cell);
        if found.is_some() {
            while cell > 32 {
                let finer = self.groups(cell / 2);
                if finer.is_none() {
                    break;
                }
                cell /= 2;
                found = finer;
            }
        } else {
            loop {
                cell *= 2;
                found = self.groups(cell);
                if found.is_some() {
                    break;
                }
            }
        }
        self.cell_last = cell;
        self.damage = found.unwrap();
    }
    fn merge(&mut self) {
        let area = |r: R| r.area();
        let close = |a: R, b: R| {
            a.meets(b) || area(a.union(b)) <= area(a) + area(b) + area(a).min(area(b)) / 2 + NEAR_PIXELS
        };
        self.regions.clear();
        for &d in &self.damage {
            let mut r = d.clip(self.vp);
            if r.is_empty() {
                continue;
            }
            let mut i = 0;
            while i < self.regions.len() {
                if close(self.regions[i], r) {
                    r = r.union(self.regions.swap_remove(i));
                    i = 0;
                } else {
                    i += 1;
                }
            }
            self.regions.push(r);
        }
    }
}

impl Form for Zgx {
    fn name(&self) -> String {
        if self.collapse { "zgx-vp".into() } else { "zgx".into() }
    }
    fn reset(&mut self, _tr: &Trace, sc: &Scene, _ring: usize) {
        *self = Zgx::new(self.collapse);
        self.vp = sc.viewport;
    }
    fn record(&mut self, f: &Frame, _sc: &Scene) {
        self.list.clear();
        self.grid = None;
        self.recent = [R::E; 4];
        bounds_events(f, &mut |r| self.add(r));
        self.ep += 1;
        let mut h = std::mem::take(&mut self.hist[self.ep & 3]);
        h.clear();
        match &self.grid {
            Some(g) => Self::grid_rects(g, self.vp, &mut h),
            None => h.extend_from_slice(&self.list),
        }
        self.hist[self.ep & 3] = h;
    }
    fn repair(&mut self, _surf: usize, age: u32, sc: &Scene, out: &mut Out) {
        self.damage.clear();
        for k in 0..age as usize {
            let h = &self.hist[(self.ep - k) & 3];
            self.damage.extend_from_slice(h);
        }
        if self.damage.len() > MAX_REGIONS {
            self.cluster();
        }
        self.merge();
        for &r in &self.regions {
            cull_rect(sc, r, &mut out.list);
            out.close(r);
        }
    }
    fn region(&self, emit: &mut dyn FnMut(R)) {
        for &r in &self.regions {
            emit(r);
        }
    }
    fn mem_bytes(&self) -> usize {
        self.hist.iter().map(|h| h.capacity() * 16).sum::<usize>() + self.list.capacity() * 16
    }
}
