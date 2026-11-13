//! Admission of face images (the independent review's T07) and of runs (T04): a malformed image is refused by
//! `Face::check`, and one it admits cannot make a later read go out of range; a run from before a `clear`
//! draws nothing.
use bough_face::{Face, Glyphs};

static IMG: &[u8] = include_bytes!("../data/DejaVuSans-14.face");
static FACE: Face = Face::new(IMG);

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Use everything a frame uses of a face: no call may panic.
fn exercise(f: &Face) {
    let mut n = 0usize;
    for text in ["Row 00042 office fluffy \u{b7} na\u{ef}ve", "fi ffi fl AVATAR To. 1,234.50-", "\u{4e2d}x", ""] {
        let _ = f.shape(text.as_bytes(), |_, _, _| n += 1);
        let mut toks = Vec::new();
        let mut pens = Vec::new();
        f.shape_lossy(text.as_bytes(), |t, x, _| {
            toks.push(t as u16);
            pens.push(x);
        });
        f.spans(&toks, &pens, 0, (3, f.baseline(32)), [0, 0, 400, 32], |_, _, c| n += c.len());
        f.expand(&toks, &pens, 0, (3, 20), [0, 0, 400, 32], |b, t| {
            // the last texel the box reads is inside the atlas
            let last = t as usize + (b[3] - b[1] - 1) as usize * f.stride() + (b[2] - b[0] - 1) as usize;
            assert!(last < f.atlas().len());
        });
    }
    for t in 0..=u16::MAX {
        let s = f.slot(t);
        for r in 0..s.h {
            n += f.row(s, r).len();
        }
    }
    let _ = (f.cell(), f.size(), f.upem(), n);
}

#[test]
fn malformed_images_are_refused_or_harmless() {
    exercise(&FACE);
    assert!(Face::check(&[]).is_err());
    assert!(Face::check(&IMG[..IMG.len() - 1]).is_err());
    assert!(Face::check(&IMG[..3000]).is_err());
    let mut r = Rng(0x0bad_face_0bad_face);
    let (mut refused, mut admitted) = (0, 0);
    for round in 0..3000 {
        let mut b = IMG.to_vec();
        // corrupt a few bytes: the header and the tables most of the time, anywhere otherwise
        for _ in 0..1 + r.next() % 4 {
            let at = if round % 3 != 0 { (r.next() % 3200) as usize } else { (r.next() % b.len() as u64) as usize };
            b[at] = r.next() as u8;
        }
        let b: &'static [u8] = Box::leak(b.into_boxed_slice());
        match Face::check(b) {
            Err(_) => refused += 1,
            Ok(f) => {
                admitted += 1;
                exercise(&f);
            }
        }
    }
    assert!(refused > 300 && admitted > 300, "{refused} refused, {admitted} admitted");
}

#[test]
fn a_run_from_before_a_clear_draws_nothing() {
    let mut g = Glyphs::new();
    let (old, _) = g.line(0, &FACE, "before the collection").unwrap();
    assert_eq!(g.of(old).0.len(), old.len());
    g.clear();
    // new lines now occupy the lanes the old run named
    let (new, _) = g.line(0, &FACE, "something else entirely, and longer").unwrap();
    assert_eq!(new.g0, old.g0);
    assert!(g.of(old).0.is_empty() && g.of(old).1.is_empty(), "a stale run must not read another line's glyphs");
    assert_eq!(g.of(new).0.len(), new.len());
    assert_ne!(old.generation(), new.generation());
}

#[test]
fn the_memo_gives_its_memory_back() {
    let mut g = Glyphs::new();
    for i in 0..20_000 {
        g.line(0, &FACE, &format!("a line that is typed and never shown again {i}")).unwrap();
    }
    let big = g.bytes();
    assert!(g.over(1 << 20) && big > 1 << 20);
    g.clear();
    assert!(g.bytes() < 64 << 10, "{} bytes held after a clear (from {big})", g.bytes());
}
