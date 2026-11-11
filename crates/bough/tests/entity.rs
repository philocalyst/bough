//! `Entities<T>` / `Entity<T>` and `View::changed`: two views of one entity follow its edits; removed
//! handles answer `None`; retained == fresh mount; `changed` sees list deltas and entity edits and the
//! state it writes settles in the same turn.
use bough::raw::*;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Doc {
    name: Signal<String>,
    hue: Signal<u32>,
}
impl View for Doc {
    fn view(&self) -> impl Element<Self> + '_ {
        quad(0.0, 0.0, 120.0, 30.0).bg(sig!(self.hue).map(|h| rgb(*h))).children(text(sig!(self.name)).at(4.0, 4.0))
    }
}

struct Row {
    v: Signal<u32>,
}
impl View for Row {
    fn view(&self) -> impl Element<Self> + '_ {
        bough::layout::label(sig!(self.v).map(|v| format!("row {v}")))
    }
}

struct Root {
    docs: Entities<Doc>,
    a: Entity<Doc>,
    b: Entity<Doc>,
    rows: List<Row>,
    log: Signal<String>,
    seen: u32,
}
impl View for Root {
    fn view(&self) -> impl Element<Self> + '_ {
        (
            at(0.0, 0.0, entity_view(sig!(self.docs), self.a)),
            at(0.0, 40.0, entity_view(sig!(self.docs), self.a)),
            at(0.0, 80.0, entity_view(sig!(self.docs), self.b)),
            text(sig!(self.log)).at(0.0, 120.0),
            at(0.0, 140.0, bough::layout::column(list(sig!(self.rows)))),
        )
    }
    fn changed(&mut self, d: Changed<'_, Self>) {
        let mut parts = Vec::new();
        if let Some(ops) = d.list(sig!(self.rows)) {
            parts.push(format!("list {}", ops.len()));
        }
        if let Some(e) = d.entities(sig!(self.docs)) {
            parts.push(format!("ents {}", e.len()));
        }
        if !parts.is_empty() {
            self.seen += 1;
            self.log.set(format!("{} #{}", parts.join(" "), self.seen));
        }
    }
}

fn root() -> Root {
    let mut docs = Entities::with_capacity(8);
    let a = docs.insert(Doc { name: Signal::new("alpha".into()), hue: Signal::new(0x336699) });
    let b = docs.insert(Doc { name: Signal::new("beta".into()), hue: Signal::new(0x993366) });
    Root { docs, a, b, rows: List::new(), log: Signal::new(String::new()), seen: 0 }
}

type Item7 = (u8, [f32; 4], u32, u32, f32, f32, [u16; 2]);
fn dump(app: &App<impl View>) -> Vec<(u8, u16, [i16; 4], Vec<Item7>)> {
    app.rt.bough.records().iter().map(|r| (r.kind, r.depth, r.ink.0, if r.ink.is_empty() { Vec::new() } else { app.rt.expand_payload(r.payload) })).collect()
}

#[test]
fn entities_and_changed() {
    let cfg = || Config::new(200.0, 260.0, 1.0, FONT.to_vec()).once();
    let mut app = App::new(root(), cfg(), false);
    app.frame();
    app.turn(|r| {
        let a = r.a;
        let d = r.docs.edit(a).unwrap();
        d.name.set("ALPHA".into());
        *d.hue = 0x00ff00;
        r.rows.push(Row { v: Signal::new(1) });
        r.rows.push(Row { v: Signal::new(2) });
    });
    app.frame();
    assert_eq!(*app.owner.log, "list 2 ents 1 #1");
    app.turn(|r| {
        *r.rows.edit(0).v = 10;
    });
    app.frame();
    assert_eq!(*app.owner.log, "list 1 #2");
    // fresh mount of the same values
    let mut f = {
        let mut r = root();
        let a = r.a;
        let d = r.docs.edit(a).unwrap();
        d.name.set("ALPHA".into());
        *d.hue = 0x00ff00;
        r.rows.push(Row { v: Signal::new(10) });
        r.rows.push(Row { v: Signal::new(2) });
        r.log.set("list 1 #2".into());
        App::new(r, cfg(), false)
    };
    f.frame();
    assert!(dump(&app) == dump(&f), "retained != fresh");
    // removal: the handle dies
    let b = app.owner.b;
    app.turn(|r| {
        assert!(r.docs.remove(b).is_some());
    });
    assert!(app.owner.docs.get(b).is_none());
    assert!(app.owner.docs.get(app.owner.a).is_some());
}
