# Prepared glyph images

| file | face | sha256 of the face | size | made by |
|---|---|---|---|---|
| `dejavu-sans-14.bgi` | DejaVu Sans 2.37 (`DejaVuSans.ttf`, 757,076 bytes) | `7da195a74c55bef988d0d48f9508bd5d849425c1770dba5d7bfc6ce9ed848954` | 14 px | `cargo run --release -p bough-glyph --example bake -- DejaVuSans.ttf 14 crates/bough-glyph/data/dejavu-sans-14.bgi` |
| `dejavu-sans-28.bgi` | the same | the same | 28 px (14 px at scale 2) | the same command with `28` |

190 characters (Latin-1 without the soft hyphen), 5 ligatures (ff, fi, fl, ffi, ffl), 1,087 kerned pairs,
16,527 coverage bytes; 96,813 bytes in all, of which 76 KB is the kerning table (a square of `i16`; read only at
the pairs a frame shows).

Gate: `BOUGH_GLYPH_FONT=DejaVuSans.ttf cargo test --release -p bough-glyph` (placement and coverage equal to
swash's on 4,013 strings; mutants `BOUGH_GLYPH_MUTANT=kern` and `=cov` must fail).
