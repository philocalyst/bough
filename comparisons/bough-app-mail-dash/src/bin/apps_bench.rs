//! zgx's `apps_bench` (`crates/zgx-script/examples/apps_bench.rs` + `support/apps.rs`) for the mail and
//! dashboard apps on bough: each app's interactions, each followed by its frame, drawn offscreen on the GPU at
//! twice the points (`RENDER=1`, the default) or not drawn (`RENDER=0`: the frame ends at the tile list), the
//! time and instructions per interaction printed as zgx prints them.
//!
//! `APP=mail|dashboard` (both if unset), `REPS` (20), `ROWS` (the dashboard's, 100,000), `ONLY` / `SKIP` as
//! zgx's, `BOUGH_PROFILE=1` for phases.
use bough::raw::*;
use bough_app_mail_dash::dash::{self, Col, Dash};
use bough_app_mail_dash::harness::{render_from_env, scale_from_env, Bench};
use bough_app_mail_dash::mail::{self, Mail};
use bough_app_mail_dash::{config, env_usize, lay_rect};

/// apps.rs `mail`.
fn mail(render: bool, scale: f32) {
    let app = mail::mount(Mail::new(), config(mail::W, mail::H, scale), render);
    let mut b = Bench::new(app, render);
    b.measure("mail: open a message", |b, k| {
        let r = mail::row_rect(&b.app, k % 2);
        b.click_mid(r);
    });
    b.measure("mail: search keystroke", |b, k| {
        if k % 2 == 0 {
            let f = lay_rect(&b.app.rt, &mail::SEARCH_PATH);
            b.click_mid(f);
            b.key(Key::Char('r'));
        } else {
            b.key(Key::Backspace);
        }
    });
    let r = mail::row_rect(&b.app, 0);
    b.click_mid(r);
    b.frames(30);
    b.measure("mail: archive", |b, _| {
        let a = lay_rect(&b.app.rt, &mail::ARCHIVE_PATH);
        b.click_at(a[0] + 4.0, a[1] + a[3] / 2.0);
    });
}

/// apps.rs `dashboard` (the feed paused from the start, so each tick is the one measured).
fn dashboard(render: bool, scale: f32) {
    let rows = env_usize("ROWS", 100_000);
    let app = dash::mount(Dash::new(rows), config(dash::W, dash::H, scale), render);
    let mut b = Bench::new(app, render);
    b.frames(2);
    assert!(!*b.app.owner.live, "the live feed is paused");
    let click = |b: &mut Bench<Dash>, c: Col| {
        let r = lay_rect(&b.app.rt, &dash::sort_path(c));
        b.click_mid(r);
    };
    click(&mut b, Col::Price);
    b.frames(2);
    b.measure("dashboard: feed tick, sorted by price", |b, _| {
        b.app.turn(|d| d.feed());
    });
    // two columns in turn: each sort back to the other's order
    b.measure("dashboard: sort", |b, k| click(b, if k % 2 == 0 { Col::Volume } else { Col::Symbol }));
    // three others in turn: none the last sort's
    b.measure("dashboard: sort, a column not sorted by last", |b, k| click(b, [Col::Price, Col::Change, Col::Sector][k % 3]));
    b.measure("dashboard: filter keystroke", |b, k| {
        if k % 2 == 0 {
            let f = lay_rect(&b.app.rt, &dash::SEARCH_PATH);
            b.click_mid(f);
            b.key(Key::Char('B'));
        } else {
            b.key(Key::Backspace);
        }
    });
}

fn main() {
    let (render, scale) = (render_from_env(), scale_from_env());
    let only = std::env::var("APP").ok();
    if only.as_deref().is_none_or(|a| a == "mail") {
        mail(render, scale);
    }
    if only.as_deref().is_none_or(|a| a == "dashboard") {
        dashboard(render, scale);
    }
}
