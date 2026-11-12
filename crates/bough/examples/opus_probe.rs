//! Opus collaborator probes (round 6, IDEAS-OPUS.md): what the pieces of one re-described row cost, in
//! instructions on the calling thread (`tinstr`), against candidate formulations. Not a gate; a measurement.
//!
//!     cargo run --release -p bough --example opus_probe
use bough::layout::SpecTable;
use bough::prelude::*;
use bough::seal::{SealCheck, SealWrite, Sealer};
use bough_metal::perf::tinstr;
use std::hint::black_box;

const N: usize = 1000;

fn tint(sel: u32, id: u32, i: usize) -> Color {
    if id == sel {
        rgb(0x2f6bff)
    } else if i % 2 == 0 {
        rgb(0xf5f5f5)
    } else {
        rgb(0xe6e6e6)
    }
}

/// The application row of `redescribe_cost` (Table<0>), as a description.
#[inline(always)]
fn line<'a>(rows: &'a [(u32, String)], sel: u32, i: usize) -> impl Element<()> + 'a {
    let (id, label) = &rows[i];
    row((text(*id).w(64.0), text(label.as_str())))
        .w(960.0)
        .h(32.0)
        .bg(tint(sel, *id, i))
}

#[inline(always)]
fn ctx<S: Sealer>(s: &mut S) {
    // describe.rs `seal_context`: clip | space << 16, depth, ox, oy
    s.word(0);
    s.word(1);
    s.f32(0.0);
    s.f32(0.0);
}

/// Slope of `f` over two repeat counts, per call of the inner N-row loop, per row.
fn slope(mut f: impl FnMut()) -> f64 {
    let mut run = |r: u32| {
        let t0 = tinstr();
        for _ in 0..r {
            f();
        }
        tinstr() - t0
    };
    run(2);
    let (lo, hi) = (run(4), run(12));
    (hi - lo) as f64 / 8.0 / N as f64
}

/// A "flat" seal of the same row: what a description that is built as bytes would hold (the dynamic
/// operands only; the shape and the constant operands are the item type's, checked once per part).
#[inline(always)]
fn flat(rows: &[(u32, String)], sel: u32, i: usize, out: &mut [u64; 4]) -> usize {
    let (id, label) = &rows[i];
    let c = tint(sel, *id, i).0 as u64;
    let b = label.as_bytes();
    // id as a word (not its decimal text), colour, label length; then up to 16 label bytes inline
    out[0] = *id as u64 | c << 32;
    out[1] = b.len() as u64;
    let mut w = [0u8; 16];
    let n = b.len().min(16);
    w[..n].copy_from_slice(&b[..n]);
    out[2] = u64::from_le_bytes(w[0..8].try_into().unwrap());
    out[3] = u64::from_le_bytes(w[8..16].try_into().unwrap());
    b.len()
}

fn main() {
    let rows: Vec<(u32, String)> = (0..N as u32).map(|i| (i, format!("Row label {i}"))).collect();
    let sel = u32::MAX;
    let mut descs = SpecTable::default();
    // retained seals, as update_dyn writes them
    let mut bytes: Vec<u8> = Vec::new();
    let mut at = Vec::new();
    for i in 0..N {
        let o = bytes.len();
        let mut w = SealWrite {
            out: &mut bytes,
            descs: &mut descs,
            ok: true,
        };
        ctx(&mut w);
        line(&rows, sel, i).seal(&mut w);
        assert!(w.ok);
        at.push((o, bytes.len() - o));
    }
    let avg = bytes.len() / N;

    // (a) build the description only
    let build = slope(|| {
        for i in 0..N {
            black_box(line(&rows, black_box(sel), i));
        }
    });
    // (b) build + SealCheck (what `sealed_as` does for an unchanged item)
    let check = slope(|| {
        let mut eq = 0;
        for i in 0..N {
            let e = line(&rows, black_box(sel), i);
            let (o, n) = at[i];
            let mut c = SealCheck::new(&bytes[o..o + n], &descs);
            ctx(&mut c);
            e.seal(&mut c);
            eq += c.equal() as usize;
        }
        assert_eq!(eq, N);
    });
    // (c) flat words: dynamic operands only, compared as four u64 xors (retained in a column, item-indexed)
    let mut ret = vec![[0u64; 4]; N];
    for i in 0..N {
        flat(&rows, sel, i, &mut ret[i]);
    }
    let flatc = slope(|| {
        let mut eq = 0;
        for i in 0..N {
            let mut w = [0u64; 4];
            let n = flat(&rows, black_box(sel), i, &mut w);
            let r = &ret[i];
            let mut d = (w[0] ^ r[0]) | (w[1] ^ r[1]) | (w[2] ^ r[2]) | (w[3] ^ r[3]);
            if n > 16 {
                // long labels: the tail is compared as bytes against the retained text (cold here)
                d |= 1;
            }
            eq += (d == 0) as usize;
        }
        assert_eq!(eq, N);
    });
    // (d) the whole description as a fixed-stride word record: the three specs inline (84 B each, no table
    // indirection), font size / line height, colours, the id as a word and the label's first 16 bytes; compared
    // with the retained record as 36 u64 xor-ors (what a part of fixed-shape items could keep as a column).
    const W: usize = 36;
    #[inline(always)]
    fn full(rows: &[(u32, String)], sel: u32, i: usize, out: &mut [u64; W]) {
        use bough::layout::Styled;
        let (id, label) = &rows[i];
        let mut a = text(*id).w(64.0);
        let mut b = text(label.as_str());
        let mut r = row(()).w(960.0).h(32.0);
        let bytes = unsafe { &mut *(out.as_mut_ptr() as *mut [u8; W * 8]) };
        bytes[0..84].copy_from_slice(r.spec_mut().bytes());
        bytes[84..168].copy_from_slice(a.spec_mut().bytes());
        bytes[168..252].copy_from_slice(b.spec_mut().bytes());
        let lb = label.as_bytes();
        let mut w = [0u8; 16];
        let n = lb.len().min(16);
        w[..n].copy_from_slice(&lb[..n]);
        bytes[252..256].copy_from_slice(&0u32.to_ne_bytes());
        out[32] = *id as u64 | (tint(sel, *id, i).0 as u64) << 32;
        out[33] = (14.0f32.to_bits() as u64) | (20.0f32.to_bits() as u64) << 32 | 0;
        out[34] = u64::from_le_bytes(w[0..8].try_into().unwrap());
        out[35] = u64::from_le_bytes(w[8..16].try_into().unwrap()) ^ (lb.len() as u64) << 56;
    }
    let mut ret_full = vec![[0u64; W]; N];
    for i in 0..N {
        full(&rows, sel, i, &mut ret_full[i]);
    }
    let fullc = slope(|| {
        let mut eq = 0;
        for i in 0..N {
            let mut w = [0u64; W];
            full(&rows, black_box(sel), i, &mut w);
            let r = &ret_full[i];
            let mut d = 0u64;
            for j in 0..W {
                d |= w[j] ^ r[j];
            }
            eq += (d == 0) as usize;
        }
        assert_eq!(eq, N);
    });
    println!("seal bytes per row: {avg}");
    println!("(d) whole description as a 288-byte word record (specs inline) + xor: {fullc:8.1} instr/row");
    println!("(a) describe one row (build only):            {build:8.1} instr/row");
    println!("(b) describe + SealCheck (sealed_as):          {check:8.1} instr/row");
    println!("(c) describe dynamic operands as 4 words + xor: {flatc:8.1} instr/row");
}
