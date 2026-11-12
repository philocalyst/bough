//! The devtools server driven over its socket, as an agent drives it: query the tree, act, record a session,
//! replay it on a fresh app and compare pixels, take a PNG of the retained frame with no window (GPU,
//! offscreen). Negative control: a replay missing one recorded turn must differ.
use bough::devtools::{Devtools, J, serve};
use bough::prelude::*;
use bough::raw::{App, Config};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

const FONT: &[u8] = zgx_workload::FONT_BYTES;

struct Counter {
    count: Signal<u32>,
    note: Signal<String>,
}
impl Counter {
    fn inc(&mut self) {
        *self.count += 1;
    }
}
impl View for Counter {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(rgb(0xf0f0f0))
            .p(8.0)
            .gap(6.0)
            .child(text(sig!(self.count).map(|n| format!("Count: {n}"))))
            .child(button("+").on_click(Counter::inc))
            .child(text_input(sig!(self.note)))
            .child(div().w(40.0).h(20.0).bg(sig!(self.count).map(|n| if n % 2 == 0 { rgb(0x2266cc) } else { rgb(0xcc6622) })))
    }
}

fn make() -> App<Counter> {
    let mut a = App::new(Counter { count: Signal::new(0), note: Signal::new(String::new()) }, Config::new(240.0, 160.0, 2.0, FONT.to_vec()), true);
    a.frame();
    a
}

struct Client {
    w: UnixStream,
    r: BufReader<UnixStream>,
}
impl Client {
    fn connect(path: &std::path::Path) -> Client {
        for _ in 0..500 {
            if let Ok(s) = UnixStream::connect(path) {
                return Client { r: BufReader::new(s.try_clone().unwrap()), w: s };
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the server did not come up");
    }
    fn ask(&mut self, req: &str) -> J {
        self.w.write_all(req.as_bytes()).unwrap();
        self.w.write_all(b"\n").unwrap();
        let mut line = String::new();
        self.r.read_line(&mut line).unwrap();
        let j = J::parse(&line).unwrap_or_else(|e| panic!("reply {line:?}: {e}"));
        assert_eq!(j.get("ok"), Some(&J::Bool(true)), "{req} -> {line}");
        j
    }
}

#[test]
fn an_agent_queries_acts_records_replays_and_takes_a_png() {
    let dir = std::env::temp_dir();
    let sock = dir.join(format!("bough-dt-{}.sock", std::process::id()));
    let png = dir.join(format!("bough-dt-{}.png", std::process::id()));
    let server = {
        let sock = sock.clone();
        std::thread::spawn(move || {
            let mut app = make();
            let mut dev = Devtools::with_factory(make);
            serve(&mut app, &mut dev, &sock).unwrap();
            *app.owner.count
        })
    };
    let mut c = Client::connect(&sock);
    c.ask(r#"{"op":"ping"}"#);
    let t = c.ask(r#"{"op":"tree"}"#);
    assert!(t.get("dump").and_then(J::str).unwrap().contains("\"Count: 0\""));
    c.ask(r#"{"op":"record","on":true}"#);
    c.ask(r#"{"op":"click","sel":"button \"+\""}"#);
    c.ask(r#"{"op":"click","sel":"button \"+\""}"#);
    c.ask(r#"{"op":"type","sel":"textinput","text":"agent"}"#);
    c.ask(r#"{"op":"tick","dt":0.25}"#);
    c.ask(r#"{"op":"set","path":"count","value":"5"}"#);
    let n = c.ask(r#"{"op":"get","sel":"text ~\"Count\""}"#);
    let node = n.get("node").unwrap();
    assert_eq!(node.get("name").and_then(J::str), Some("Count: 5"));
    let b = &node.get("binds").and_then(J::arr).unwrap()[0];
    assert_eq!(b.get("path").and_then(J::str), Some("count"));
    assert_eq!(b.get("value").and_then(J::str), Some("5"));
    let v = c.ask(r#"{"op":"read","path":"note"}"#);
    assert_eq!(v.get("value").and_then(J::str), Some("\"agent\""));
    // the session replays to the same tree and the same pixels
    let s = c.ask(r#"{"op":"session"}"#);
    let turns = s.get("turns").and_then(J::arr).unwrap().to_vec();
    assert!(turns.len() >= 10, "{turns:?}");
    let r = c.ask(r#"{"op":"replay"}"#);
    assert_eq!(r.get("tree_equal"), Some(&J::Bool(true)), "{r}");
    assert_eq!(r.get("diff_pixels").and_then(J::num), Some(0.0), "{r}");
    // negative control: without the last write the replay differs (the counter, its colour square)
    let cut: Vec<J> = turns.iter().filter(|t| t.get("t").and_then(J::str) != Some("set")).cloned().collect();
    let r = c.ask(&format!(r#"{{"op":"replay","turns":{}}}"#, J::Arr(cut)));
    assert_eq!(r.get("tree_equal"), Some(&J::Bool(false)), "{r}");
    assert!(r.get("diff_pixels").and_then(J::num).unwrap() > 0.0, "{r}");
    // a PNG of the retained frame, no window
    let p = c.ask(&format!(r#"{{"op":"screenshot","path":{}}}"#, J::Str(png.display().to_string())));
    assert_eq!((p.get("w").and_then(J::num), p.get("h").and_then(J::num)), (Some(480.0), Some(320.0)));
    let bytes = std::fs::read(&png).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 480);
    let _ = std::fs::remove_file(&png);
    // errors are replies, not crashes
    c.w.write_all(b"{\"op\":\"click\",\"sel\":\"\\\"nothing\\\"\"}\n").unwrap();
    let mut line = String::new();
    c.r.read_line(&mut line).unwrap();
    assert!(line.contains("\"ok\":false") && line.contains("nothing matches"), "{line}");
    c.ask(r#"{"op":"quit"}"#);
    assert_eq!(server.join().unwrap(), 5);
    assert!(!sock.exists());
}

#[test]
fn turns_round_trip_through_json_exactly() {
    use bough::devtools::{turn_json, turn_of};
    use bough::find::Turn;
    use bough::input::{Button, Key, Mods};
    let ts = [
        Turn::Move { x: 12.345_678, y: 0.1 },
        Turn::Down { x: 1.0 / 3.0, y: 2.5, button: Button::Primary },
        Turn::Up { x: 1.0 / 3.0, y: 2.5, button: Button::Secondary },
        Turn::Leave,
        Turn::Wheel { x: 3.0, y: 4.0, dy: -120.25 },
        Turn::Key { key: Key::Char('é'), mods: Mods { shift: true, cmd: true, ..Mods::default() } },
        Turn::Key { key: Key::PageDown, mods: Mods::default() },
        Turn::Text("a \"quoted\"\nline".into()),
        Turn::Tick(1.0 / 60.0),
        Turn::Set { owner: 2, path: "rows[3].title".into(), value: "x".into() },
        Turn::Frame,
    ];
    for t in &ts {
        let s = turn_json(t).to_string();
        assert_eq!(&turn_of(&J::parse(&s).unwrap()).unwrap(), t, "{s}");
    }
}
