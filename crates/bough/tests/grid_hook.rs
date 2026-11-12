//! CSS grid through a layout template: the grid algorithm is reached through `lay_deep`'s install-on-use hook
//! (the size lane's lever S3), which the first interned grid style installs. Hand-computed geometry at scale 1.
//! Mutant: without `enable_grid_for` in `Deep::intern`, the hook is absent and the mount panics.
use bough::raw::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

const fn node(depth: u16, style: u16) -> LNode {
    LNode {
        depth,
        style,
        size: [0.0, 0.0],
        bg: 0xff20_4060,
        border_w: 0.0,
        border: 0,
        radius: 0.0,
        hover: 0,
        text: "",
        font: 0.0,
        line_h: 0.0,
        color: 0,
        face: 0,
        flags: 0,
        tag: 0,
        deco: 0,
    }
}

static NODES: &[LNode] = &[node(0, 0), node(1, 1), node(1, 1), node(1, 1), node(1, 2)];
static DECLS: &[Decls] = &[
    // two 100 px columns and auto rows, 10 px gaps, 8 px padding, auto height
    &[
        (P::Display, "grid"),
        (P::GridTemplateColumns, "100px 100px"),
        (P::ColGap, "10px"),
        (P::RowGap, "10px"),
        (P::PadL, "8px"),
        (P::PadT, "8px"),
        (P::Width, "300px"),
    ],
    &[(P::Height, "40px")],
    // spans both columns
    &[(P::Height, "20px"), (P::GridColumnStart, "1"), (P::GridColumnEnd, "3")],
];

struct G;
impl View for G {
    fn view(&self) -> impl Element<Self> + '_ {
        ltemplate(NODES, DECLS, [300.0, f32::NAN], |_: &G, _, _| LHooked::Default)
    }
}

#[test]
fn a_grid_template_lays_out_through_the_hook() {
    let app = App::new(G, Config::new(300.0, 200.0, 1.0, FONT.to_vec()).once(), false);
    let grid = app.rt.lay.first_root();
    // auto height (no stretched rows): 8 + 40 + 10 + 40 + 10 + 20
    assert_eq!(app.rt.lay.eng.rect(grid), (0.0, 0.0, 300.0, 128.0));
    // items in row-major auto placement: (8, 8), (118, 8), (8, 58); the spanning item on row 3: 210 wide
    assert_eq!(app.rt.lay_child_rect(grid, 0), (8.0, 8.0, 100.0, 40.0));
    assert_eq!(app.rt.lay_child_rect(grid, 1), (118.0, 8.0, 100.0, 40.0));
    assert_eq!(app.rt.lay_child_rect(grid, 2), (8.0, 58.0, 100.0, 40.0));
    assert_eq!(app.rt.lay_child_rect(grid, 3), (8.0, 108.0, 210.0, 20.0));
}
