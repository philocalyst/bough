//! The bench's view of the one memo (`bough_face::Glyphs`) with the table shaper as its shaper, plus what the
//! bench alone keeps beside it: a break table per entry, built when first asked for.
#![allow(dead_code)]
use bough_face::{key, Glyphs};
use bough_text::stage::brk::{self, Brk, Extent};
use bough_text::{GlyphRun, Shaper};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ent {
    pub g0: u32,
    pub gn: u32,
    pub b0: u32,
    pub bn: u32,
    pub width: i32,
    pub ext: Extent,
    pub bytes: u32,
}
pub const NO_BREAKS: u32 = u32::MAX;

#[derive(Default)]
pub struct Memo {
    pub g: Glyphs,
    ents: Vec<Ent>,
    /// The entry of the line that starts at each lane index (0: none; entries are numbered from 1 here).
    at: Vec<u32>,
    pub brk: Vec<Brk>,
    pub first: Vec<u32>,
    run: GlyphRun,
}

impl Memo {
    pub fn new(_buckets: usize) -> Memo {
        Memo::default()
    }
    pub fn gid(&self) -> &[u16] {
        self.g.lanes().0
    }
    pub fn x(&self) -> &[i32] {
        self.g.lanes().1
    }
    pub fn len(&self) -> usize {
        self.g.len()
    }
    pub fn clear(&mut self) {
        self.g.clear();
        self.ents.clear();
        self.at.clear();
        self.brk.clear();
        self.first.clear();
    }
    /// The entry of `text`, shaped by the table shaper on a miss. `None`: not covered.
    #[inline]
    pub fn get(&mut self, font: u32, sh: &Shaper, text: &str) -> Option<u32> {
        let run = &mut self.run;
        let (r, width) = self.g.line_with(key(font, text.as_bytes()), font as u16, text.as_bytes(), |slot, x| {
            sh.shape_into(text, 1.0, run).then(|| {
                slot.extend_from_slice(&run.gid);
                x.extend_from_slice(&run.x);
                run.width
            })
        })?;
        if self.at.len() <= r.g0 as usize {
            self.at.resize(self.g.lanes().0.len() + 1, 0);
        }
        let i = self.at[r.g0 as usize];
        if i != 0 {
            return Some(i - 1);
        }
        // non-byte clusters need their break table now, while the run's clusters are at hand
        let ident = self.run.identity_clusters && text.len() == r.len();
        let mut e = Ent { g0: r.g0, gn: r.len() as u32, b0: 0, bn: NO_BREAKS, width, ext: Extent::default(), bytes: text.len() as u32 };
        if !ident {
            e.b0 = self.brk.len() as u32;
            e.ext = brk::build(text.as_bytes(), &self.run, 0, 0, &mut self.brk, &mut self.first);
            e.bn = self.brk.len() as u32 - e.b0;
        }
        self.ents.push(e);
        self.at[r.g0 as usize] = self.ents.len() as u32;
        Some(self.ents.len() as u32 - 1)
    }
    pub fn ent(&self, off: u32) -> Ent {
        self.ents[off as usize]
    }
    /// The entry with its break table, built now (from the lanes and `text`) if this is the first time.
    pub fn breaks(&mut self, off: u32, text: &str) -> Ent {
        let e = self.ents[off as usize];
        if e.bn != NO_BREAKS {
            return e;
        }
        let b0 = self.brk.len() as u32;
        let (a, z) = (e.g0 as usize, (e.g0 + e.gn) as usize);
        let ext = brk::build_bytes(text.as_bytes(), &self.g.lanes().1[a..z], e.width, 0, 0, &mut self.brk, &mut self.first);
        let e = Ent { b0, bn: self.brk.len() as u32 - b0, ext, ..e };
        self.ents[off as usize] = e;
        e
    }
}
