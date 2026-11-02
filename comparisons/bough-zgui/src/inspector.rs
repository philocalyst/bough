//! The inspector (round 4): an ordinary bough app, written only against the public API (`bough::prelude`,
//! `bough::inspect`), opened on another running bough app. It picks a record by pointer (the target's own
//! hit testing), shows the record tree around it, the record's kind, ink, layout box and transform, and an
//! overlay of the target's window: the picked ink, its layout box and the tiles each target frame repaired.
//! Reflected state (the owner's fields, editable) waits for the api agent's typed reflection: the state
//! panel is a list of (name, value) rows ready for it. Friction while writing it: INSPECTOR.md.
use bough::inspect::RecordView;
use bough::prelude::*;

/// The overlay's scale (target device px -> inspector points).
pub const MINI: f32 = 0.25;
/// Rows of the record tree shown around the pick.
pub const TREE_ROWS: usize = 22;
const TILE: f32 = 32.0 * MINI;
const PANEL_X: f32 = 420.0;

pub struct Inspector {
    /// The target window's tile grid (fixed: an inspector is opened on a window size).
    pub tiles: (usize, usize),
    /// Per tile row, the columns the target's last frame repaired.
    pub damage: Signal<Vec<u128>>,
    /// The picked record's ink and layout box in overlay coordinates: origin, width, (y, h) span.
    pub ink_at: Signal<(f32, f32)>,
    pub ink_w: Signal<f32>,
    pub ink_h: Signal<(f32, f32)>,
    pub box_at: Signal<(f32, f32)>,
    pub box_w: Signal<f32>,
    pub box_h: Signal<(f32, f32)>,
    /// The record tree window and the picked record's description.
    pub tree: Signal<Vec<String>>,
    pub info: Signal<String>,
    /// Reflected state rows (name, value): filled by reflection when it lands.
    pub state: Signal<Vec<String>>,
    pub picked: Option<usize>,
}

impl Inspector {
    pub fn new(target_w: f32, target_h: f32) -> Inspector {
        Inspector {
            tiles: ((target_w / 32.0).ceil() as usize, (target_h / 32.0).ceil() as usize),
            damage: Signal::new(Vec::new()),
            ink_at: Signal::new((0.0, 0.0)),
            ink_w: Signal::new(0.0),
            ink_h: Signal::new((0.0, 0.0)),
            box_at: Signal::new((0.0, 0.0)),
            box_w: Signal::new(0.0),
            box_h: Signal::new((0.0, 0.0)),
            tree: Signal::new(Vec::new()),
            info: Signal::new("pick a record".into()),
            state: Signal::new(vec!["reflection: pending (api agent)".into()]),
            picked: None,
        }
    }
}

const DAMAGE: Color = Color::rgba(255, 140, 0, 110);
const CLEAR: Color = Color::rgba(0, 0, 0, 0);

impl View for Inspector {
    const FIXED: bool = true;
    fn view(&self) -> impl Element<Self> + '_ {
        let (tx, ty) = self.tiles;
        let mini_w = tx as f32 * TILE;
        let mini_h = ty as f32 * TILE;
        // the damage overlay: one quad per target tile, coloured when the last frame repaired it
        let tiles = each((0..tx * ty).map(move |i| {
            let (cx, cy) = ((i % tx) as u32, (i / tx) as u32);
            quad(10.0 + cx as f32 * TILE, 10.0 + cy as f32 * TILE, TILE, TILE)
                .bg(sig!(self.damage).map(move |d: &Vec<u128>| if d.get(cy as usize).is_some_and(|r| r >> cx & 1 != 0) { DAMAGE } else { CLEAR }))
        }));
        let rows = each((0..TREE_ROWS).map(move |i| text(sig!(self.tree).map(move |t: &Vec<String>| t.get(i).cloned().unwrap_or_default())).at(PANEL_X, 10.0 + i as f32 * 16.0).size(12.0).line_height(16.0).color(rgb(0xd8d8d8))));
        let state = each((0..6).map(move |i| text(sig!(self.state).map(move |t: &Vec<String>| t.get(i).cloned().unwrap_or_default())).at(10.0, 40.0 + mini_h + i as f32 * 16.0).size(12.0).line_height(16.0).color(rgb(0xa0e0a0))));
        (
            quad(0.0, 0.0, 1000.0, 600.0).bg(rgb(0x1b1d22)),
            quad(10.0, 10.0, mini_w, mini_h).bg(rgb(0x2a2d35)),
            tiles,
            // the picked record's layout box (blue) and ink (red), moved by offset groups
            offset(sig!(self.box_at), quad(10.0, 10.0, 0.0, 0.0).width(sig!(self.box_w)).span(sig!(self.box_h)).bg(Color::rgba(60, 130, 255, 90))),
            offset(sig!(self.ink_at), quad(10.0, 10.0, 0.0, 0.0).width(sig!(self.ink_w)).span(sig!(self.ink_h)).bg(Color::rgba(255, 60, 60, 110))),
            text(sig!(self.info)).at(10.0, 18.0 + mini_h).size(13.0).line_height(18.0).color(rgb(0xffffff)),
            state,
            rows,
        )
    }
}

fn describe(r: &RecordView) -> String {
    let [x0, y0, x1, y1] = r.ink.to_xyxy();
    let lay = r.layout.map_or(String::from("-"), |b| format!("{:.0},{:.0} {:.0}x{:.0}", b[0], b[1], b[2], b[3]));
    format!("#{} {} d{} ink {},{}..{},{} layout {} clip {}{}", r.pos, r.kind_name(), r.depth, x0, y0, x1, y1, lay, r.clip, if r.transformed { " transformed" } else { "" })
}

/// Pick the record at the target's window point (x, y) (points).
pub fn pick<T: View>(insp: &mut App<Inspector>, target: &App<T>, x: f32, y: f32) {
    let p = target.pick_record(x, y);
    insp.write(|i| i.picked = p);
    sync(insp, target);
}

/// Pull what the inspector shows from the target (after each target frame).
pub fn sync<T: View>(insp: &mut App<Inspector>, target: &App<T>) {
    let damage = target.last_repair();
    let picked = insp.owner.picked;
    let view = picked.map(|p| target.record_view(p));
    let tree = picked.map(|p| tree_window(target, p)).unwrap_or_default();
    insp.turn(|i| {
        i.damage.set(damage);
        if let Some(r) = view {
            let [x0, y0, x1, y1] = r.ink.to_xyxy();
            i.ink_at.set((x0 as f32 * MINI, 0.0));
            i.ink_w.set((x1 - x0).max(0) as f32 * MINI);
            i.ink_h.set((10.0 + y0 as f32 * MINI, (y1 - y0).max(0) as f32 * MINI));
            if let Some(b) = r.layout {
                i.box_at.set((b[0] * MINI, 0.0));
                i.box_w.set(b[2] * MINI);
                i.box_h.set((10.0 + b[1] * MINI, b[3] * MINI));
            }
            i.info.set(describe(&r));
            i.tree.set(tree);
        }
    });
}

/// The record tree around `p`: its ancestors (outermost first), then `p` and the records after it.
fn tree_window<T: View>(target: &App<T>, p: usize) -> Vec<String> {
    let mut anc = target.record_ancestors(p);
    anc.reverse();
    let mut v: Vec<usize> = anc.into_iter().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect();
    let n = target.record_count();
    v.extend((p..n).take(TREE_ROWS - v.len()));
    v.into_iter()
        .map(|q| {
            let r = target.record_view(q);
            format!("{}{}{} {}", "  ".repeat((r.depth as usize).min(12)), if q == p { "> " } else { "" }, r.kind_name(), q)
        })
        .collect()
}
