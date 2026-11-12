# Fan-out: three operands, written before they are built (core lane, round 7)

For judging the API. Nothing here exists yet except where a line says so. The two scenes are the bench lane's
`sheet` and `ide` (`comparisons/bough-workloads`), rewritten as their authors would have written them had the
operands existed; their module docs list exactly these as "wanted and not expressible".

## What the scenes do today, and what it costs

Both pack unrelated state into one signal by hand because an operand reads one signal:

```rust
// sheet: theme bit | hovered column, read by 200,000 wires
look: Signal<u32>,
.bg(sig!(self.look).map(move |l: &u32| fill(*l, col, row)))
.color(sig!(self.look).map(|l: &u32| ink(*l)))

// ide: theme bit | hovered symbol, read by 4,312 identifier spans
.color(sig!(self.look).map(move |l: &u32| tok_color(*l, class, sym)))
```

A hover writes `look`, so every wire on it runs. On `r7/core` `1dfa2907e1` the sheet's hover is 6.47 M
instructions (18.44 M on the base): the text colours are skipped as a class, but `fill` is evaluated for all
100,000 cells because each captured its own `(col, row)`. The theme is 21.1 M: 200,000 stores of colours that
are all the same function of one bit.

## The three operands

### 1. An operand over several signals

A tuple of wired operands maps like one. The closure takes each value; it is `Copy` and captures at most 8
bytes (the wire's recipe also holds the other signal's offset).

```rust
.bg((sig!(self.theme), sig!(self.hover)).map(move |dark: &bool, h: &u32| fill(*dark, *h == col, row)))
```

One wire per signal, the same closure on both. No packing, no unpacking, and each signal keeps its own type.

### 2. `is(key)`: a boolean that is indexed, not evaluated

`sig!(self.hover).is(col)` is the operand "this signal equals this constant". Alone it maps like a `bool`
signal; in a tuple it is one of the values:

```rust
.bg(sig!(self.hover).is(col).map(|hot: bool| if hot { HOT } else { COLD }))
.bg((sig!(self.theme), sig!(self.hover).is(col)).map(move |dark: &bool, hot: bool| fill(*dark, hot, row)))
```

What the type says is what the runtime uses: the operand's value can only change when the signal moves to or
from `key`. The signal keeps its `is` dependents in an index by key; a write from `a` to `b` runs the
dependents keyed `a` and those keyed `b` and no others. A selection, a hovered row, an active tab, a current
line, an expanded folder are all this operand. `T` is `Copy + Eq + Hash`, at most 8 bytes.

### 3. A palette entry: a colour that is not a value in the record

```rust
theme: Palette<N>,                     // a cell of the owner, like a Signal: N colours
.color(sig!(self.theme).entry(INK))    // a colour operand: "entry INK of this palette"
```

The record holds the entry's index; the kernel resolves it against the palette table when it shades (the
compute lane's integer kernel takes the table as one more buffer). Writing the palette (a theme switch, an
inspector editing one colour, a transition between two palettes evaluated at the kernel's clock) is one table
write and damage: no wire runs, no instance is stored to, and no record carries a resolved colour. Two
operands compose with it: `is(key)` picks between two entries (`.pick(HOT, EVEN)`: the record's index is what
a hover changes), and an entry can be given per record (`.entry(if row % 2 == 0 { EVEN } else { ODD })`).

Until the kernel reads a table, `entry(k)` lowers to what exists: all the wires of one `(palette, k)` are one
segment, evaluated once and applied as a lane. The view code does not change when the kernel takes over.

## The sheet, rewritten

```rust
pub struct Sheet {
    rows: usize,
    cells: [Signal<f32>; CELLS],
    sums: [Signal<f32>; COLS],
    /// Was `look: Signal<u32>` (theme bit | hovered column, `NO_HOVER` when none).
    theme: Palette<6>,
    hover: Signal<Option<u8>>,
    sel: Signal<u32>,
    scroll: Signal<f32>,
}

const EVEN: usize = 0;
const ODD: usize = 1;
const HOT: usize = 2;
const INK: usize = 3;
const CHROME: usize = 4;
const PAGE: usize = 5;
const LIGHT: [u32; 6] = [0xffffff, 0xf6f7f9, 0xdbe8ff, 0x1d2129, 0xeceef2, 0xffffff];
const DARK: [u32; 6] = [0x15171c, 0x1b1e24, 0x26406b, 0xe6e9ef, 0x23262d, 0x15171c];

impl Sheet {
    fn hover_header(&mut self, col: u8, on: bool) {
        // was: read `look`, mask the low byte, compare, repack
        self.hover.set(if on { Some(col) } else { None });
    }
    fn toggle_theme(&mut self) {
        // was: `look ^ DARK`, and 200,000 wires
        let dark = self.theme.get(PAGE) == rgb(DARK[PAGE]);
        self.theme.set_all(if dark { LIGHT } else { DARK }.map(rgb));
    }
}

impl View for Sheet {
    fn view(&self) -> impl Element<Self> + '_ {
        let theme = sig!(self.theme);
        let header_cell = move |c: usize| {
            div()
                .w(CELL_W)
                .h(HEADER)
                .px(6.0)
                // was: `.bg(sig!(self.look).map(move |l| if l & 0xff == col { rgb(0x9db8f0) } else { chrome(*l) }))`
                .bg(sig!(self.hover).is(Some(c as u8)).pick(rgb(0x9db8f0), theme.entry(CHROME)))
                .child(text(COLUMN_NAMES[c]).font(12.0, HEADER).color(theme.entry(INK)))
                .on_hover(move |s: &mut Sheet, e: &Ev| s.hover_header(c as u8, e.on))
        };
        let cell = move |r: usize, c: usize| {
            div()
                .w(CELL_W)
                .h(CELL_H)
                .px(6.0)
                // was: `.bg(sig!(self.look).map(move |l| fill(*l, col, row)))`, a six-way match per cell per write
                .bg(sig!(self.hover).is(Some(c as u8)).pick(theme.entry(HOT), theme.entry(if r % 2 == 0 { EVEN } else { ODD })))
                .child(text(sig!(self.cells).at(r * COLS + c).num(|v: &f32| *v, VALUE)).font(12.0, CELL_H).color(theme.entry(INK)))
        };
        // rows, toolbar, header, footer, selection: as before with `theme.entry(CHROME)` / `theme.entry(PAGE)`
        // for `sig!(self.look).map(|l| chrome(*l))` / `page(*l)`
        ..
    }
}
```

What the author no longer writes: the packing of two facts into a `u32` and its masks; `fill`, `ink`, `chrome`
and `page` as functions of the packed word (the palettes are two arrays); the note in the module doc that a
hover "is meant to touch that column's cells only". What the runtime no longer does: on a hover, anything for
the 90,000 cells of the other columns (the index holds 10,000 dependents under each column's key; a move from
column 3 to none runs those 10,000 and the header cell); on a theme switch, anything per cell.

Predictions for the sheet at 100,000 cells, RENDER=0, instructions per step (now 6.47 M and 21.1 M):

* hover: 10,000 dependents of one key, each a store of a palette index and a dirty bit: about 25 instructions
  each through a lane, plus the frame: **under 400 K**.
* theme, once the kernel reads the table: one 24-byte table write, the window marked whole, one frame's slot:
  **under 5 K**, all of it the frame. Before that (the segment lowering): six evaluations and 200,000 lane
  stores at about 15 each: about 3 M.

## The ide's identifier spans, rewritten

```rust
// was: one `look: Signal<u32>` = theme bit | hovered symbol + 1, and `theme: Signal<u32>` for the rest
theme: Palette<{ CLASSES + 1 }>,     // one entry per token class, and the highlight
hover: Signal<Option<u16>>,          // the hovered symbol

let span = move |t: &Tok, indent: f32| {
    let p = text(t.text.trim()).font(12.0, LINE_H).margin(..);
    if t.class as u32 == ID {
        // was: `.color(sig!(self.look).map(move |l| tok_color(*l, class, sym)))` on 4,312 spans, all of which
        // ran on every hover
        Either::R(
            p.color(sig!(self.hover).is(Some(t.sym)).pick(theme.entry(HIGHLIGHT), theme.entry(t.class as usize)))
                .on_hover(move |s: &mut Ide, e: &Ev| s.hover.set(e.on.then_some(t.sym))),
        )
    } else {
        Either::L(p.color(theme.entry(t.class as usize)))
    }
};
```

The hover of `value` runs its 2,189 occurrences and nothing else (today all 4,312 identifier spans:
430 K instructions; predicted under 100 K). `tok_color`, `tok_hex` and the `look >> 1 == sym` packing go.

## What this does not cover, on purpose

* A projection with captures that is not an equality (`|l| shade(*l, depth)`): the wires of one cell and one
  kind already run as one monomorphised loop over their captures (a segment); the captures are stored with the
  wires today and move to lanes of their own next (no call and no wire per member; the loop body is the
  closure, inlined). No API for it: it is how `.map` with captures runs.
* `is` on something that is not a constant per record (`is(sig!(other))`): not indexable; it is the tuple map.
* Ranges (`in_range(a..b)`: a selection of several rows): the same index with interval keys; worth adding when
  a scene needs it.
* List rows reading their owner's signals (the chat scene's twelve look signals and their copies in arrival
  rows): a separate gap, the row owner's access to its parent, not an operand.

## Questions for the reader

1. `.pick(a, b)` on `is(..)` takes operands (a constant, a palette entry). Is `pick` the right word next to
   `.map(|hot: bool| ..)`?
2. `Palette<N>` as its own cell type, or `Signal<[Color; N]>` with `.entry(k)` available on any signal of an
   array? The second is less to learn; the first can refuse `.map` (a mapped palette colour cannot be resolved
   by the kernel).
3. Should a colour constant in a view be allowed at all once palettes exist, or is `rgb(..)` in a description
   the thing an inspector cannot reach?
