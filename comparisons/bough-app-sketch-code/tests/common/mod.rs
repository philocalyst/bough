//! The pixel gate (as `comparisons/bough-suite/tests/common/mod.rs`): after every drawn frame the presented
//! surface equals a from-scratch full redraw of the same retained state through the same kernels, for every
//! ring age 1..=4 (a round-robin ring of `ring` surfaces repairs at age `ring`) and the windowed
//! content-addressed ring; at the end the retained app equals a fresh mount of the state it reached.
#![allow(dead_code)]
use bough::raw::*;

pub fn first_diff(a: &[u8], b: &[u8], w: usize) -> Option<(usize, usize, u8, u8)> {
    a.iter().zip(b).position(|(x, y)| x != y).map(|i| ((i / 4) % w, (i / 4) / w, a[i], b[i]))
}

/// Ring 0 is the windowed ring (4 surfaces, content-addressed); 1..=4 are round-robin.
pub const RINGS: [usize; 5] = [1, 2, 3, 4, 0];

/// A frame, then (if it drew) presented == full redraw. Returns whether it drew.
pub fn frame_checked<V: View>(app: &mut App<V>, what: &str) -> bool {
    let drew = app.frame();
    assert!(!app.bins_overflowed(), "{what}: row bin overflow");
    if drew {
        let w = app.rt.win_w as usize;
        let px = app.presented_pixels();
        let reference = app.reference_pixels();
        if px != reference {
            dump(&what.replace(' ', "_"), &px, &reference, w);
        }
        assert!(px == reference, "{what}: presented != full redraw at {:?}", first_diff(&px, &reference, w));
    }
    drew
}

/// The retained app's pixels equal `fresh`'s (a new mount of the same state, its first frame).
pub fn same_as_fresh<V: View, W: View>(app: &mut App<V>, fresh: &mut App<W>, what: &str) {
    let w = app.rt.win_w as usize;
    let px = app.presented_pixels();
    fresh.frame();
    let fp = fresh.presented_pixels();
    if let Ok(dir) = std::env::var("BOUGH_SC_SHOT") {
        write_png(&format!("{dir}/{}.png", what.replace(' ', "_")), &px, w, px.len() / 4 / w);
    }
    if px != fp {
        dump(&what.replace(' ', "_"), &px, &fp, w);
    }
    assert!(px == fp,"{what}: retained != fresh mount, first diff {:?}", first_diff(&px, &fp, w));
}

/// Set the ring an app runs at (0: the windowed ring of 4 content-addressed surfaces).
pub fn windowed<V: View>(app: &mut App<V>, ring: usize) {
    app.windowed = ring == 0;
}

/// Write BGRA pixels as a PNG (stored deflate blocks: no compression), for looking at a failing gate.
pub fn write_png(path: &str, bgra: &[u8], w: usize, h: usize) {
    fn crc(data: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for &b in data {
            c ^= b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut c = kind.to_vec();
        c.extend_from_slice(data);
        out.extend_from_slice(&c);
        out.extend_from_slice(&crc(&c).to_be_bytes());
    }
    let mut raw = Vec::with_capacity((w * 3 + 1) * h);
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            let p = &bgra[(y * w + x) * 4..];
            raw.extend_from_slice(&[p[2], p[1], p[0]]);
        }
    }
    let mut z = vec![0x78, 0x01];
    for (i, block) in raw.chunks(65535).enumerate() {
        let last = (i + 1) * 65535 >= raw.len();
        z.push(last as u8);
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + x as u32) % 65521;
        b = (b + a) % 65521;
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    std::fs::write(path, out).unwrap();
}

/// With `BOUGH_SC_DUMP=dir`, both pictures of a failing comparison and their difference box.
pub fn dump(name: &str, a: &[u8], b: &[u8], w: usize) {
    let Ok(dir) = std::env::var("BOUGH_SC_DUMP") else { return };
    let h = a.len() / 4 / w;
    write_png(&format!("{dir}/{name}-a.png"), a, w, h);
    write_png(&format!("{dir}/{name}-b.png"), b, w, h);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for i in 0..a.len() / 4 {
        if a[i * 4..i * 4 + 4] != b[i * 4..i * 4 + 4] {
            let (x, y) = (i % w, i / w);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    eprintln!("{name}: differing box ({x0}, {y0}) - ({x1}, {y1}) px");
}
