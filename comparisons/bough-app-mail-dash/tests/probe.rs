use bough::raw::*;
use bough_app_mail_dash::dash::{self, Col, Dash, Market};
use bough_app_mail_dash::{config, instructions};

fn span<R>(name: &str, f: impl FnOnce() -> R) -> R {
    let i0 = instructions();
    let r = f();
    println!("{name}: {:.3} M", (instructions() - i0) as f64 / 1e6);
    r
}

#[test]
fn probe_mail_parts() {
    use bough_app_mail_dash::mail::{self, Mail};
    let mut app = mail::mount(Mail::new(), config(mail::W, mail::H, 2.0), false);
    app.prof.enable();
    for _ in 0..3 {
        app.frame();
    }
    for k in 0..6 {
        let id = app.owner.rows.get(k % 2).unwrap().id;
        app.prof.acc = [0; 5];
        let l0 = app.rt.lay.prof.acc;
        span("turn open", || app.turn(|m| m.open(id)));
        println!("  phases update {:.3} layout {:.3}", app.prof.acc[0] as f64 / 1e6, app.prof.acc[1] as f64 / 1e6);
        let l1 = app.rt.lay.prof.acc;
        println!("  lay {:?}", (0..8).map(|i| (bough::layout::LP_NAMES[i], l1[i] - l0[i])).collect::<Vec<_>>());
        span("frame", || app.frame());
    }
    for k in 0..6 {
        let key = if k % 2 == 0 { Key::Char('r') } else { Key::Backspace };
        let ev = Ev { x: 0.0, y: 0.0, dx: 0.0, dy: 0.0, key: Some(key), mods: Mods::default(), button: Button::Primary, on: true };
        app.prof.acc = [0; 5];
        span("turn search", || app.turn(|m| m.search_key(&ev)));
        println!("  phases update {:.3} layout {:.3}", app.prof.acc[0] as f64 / 1e6, app.prof.acc[1] as f64 / 1e6);
        span("frame", || app.frame());
    }
    for _ in 0..6 {
        app.prof.acc = [0; 5];
        span("turn archive", || app.turn(|m| m.move_open(mail::ARCHIVE)));
        println!("  phases update {:.3} layout {:.3}", app.prof.acc[0] as f64 / 1e6, app.prof.acc[1] as f64 / 1e6);
        span("frame", || app.frame());
    }
}

#[test]
fn probe_dash_parts() {
    let mut mk = span("market new", || Market::new(100_000));
    for k in 0..30 {
        if k % 5 == 0 { span("market feed", || mk.feed()); } else { mk.feed(); }
    }
    let mut app = dash::mount(Dash::new(100_000), config(dash::W, dash::H, 2.0), false);
    for _ in 0..3 {
        app.frame();
    }
    app.turn(|d| d.sort_by(Col::Price));
    app.frame();
    for _ in 0..3 {
        span("turn feed", || app.turn(|d| d.feed()));
        span("frame", || app.frame());
    }
    for _ in 0..2 {
        span("turn model feed only", || app.write(|d| d.mk.feed()));
        app.flush();
        app.frame();
    }
    for k in 0..4 {
        span("turn sort", || app.turn(|d| d.sort_by(if k % 2 == 0 { Col::Volume } else { Col::Symbol })));
        span("frame", || app.frame());
    }
    for k in 0..4 {
        let key = if k % 2 == 0 { Key::Char('B') } else { Key::Backspace };
        let ev = Ev { x: 0.0, y: 0.0, dx: 0.0, dy: 0.0, key: Some(key), mods: Mods::default(), button: Button::Primary, on: true };
        span("write search", || app.write(|d| d.search_key(&ev)));
        span("flush", || app.flush());
        span("frame", || app.frame());
    }
}
