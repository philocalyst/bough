//! Gates of the mail app: pixels (presented == full redraw at ring ages 1-4 and the windowed ring after each
//! interaction; retained == a fresh mount at the end), the model against a dumb oracle, and the layout the
//! harness clicks against the geometry it assumes.
mod common;
use bough::raw::*;
use bough_app_mail_dash::mail::{self, Mail, ARCHIVE, BOXES, INBOX, PEOPLE, SENT, STARRED, TOPICS, TRASH};
use bough_app_mail_dash::{config, lay_rect};
use common::*;

fn click(app: &mut App<Mail>, r: [f32; 4]) {
    let (x, y) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    app.pointer_down(x, y, Button::Primary);
    app.pointer_up(x, y, Button::Primary);
}
fn key(app: &mut App<Mail>, k: Key) {
    app.key_down(k, Mods::default());
}
/// The sidebar's box `k` (root row > sidebar wrapper > sidebar > the boxes' column > box).
fn mailbox(app: &App<Mail>, k: usize) -> [f32; 4] {
    lay_rect(&app.rt, &[0, 0, 1, k])
}

/// apps.rs's interactions (open alternating rows, search keystrokes, archive), then more of the app: star,
/// another mailbox, a two-letter search, back to the inbox, delete, the pointer leaving.
fn step(app: &mut App<Mail>, t: usize) {
    match t {
        1..=6 => click(app, mail::row_rect(app, t % 2)),
        7..=12 => {
            if t % 2 == 1 {
                click(app, lay_rect(&app.rt, &mail::SEARCH_PATH));
                key(app, Key::Char('r'));
            } else {
                key(app, Key::Backspace);
            }
        }
        13 => click(app, mail::row_rect(app, 0)),
        14..=19 => {
            let a = lay_rect(&app.rt, &mail::ARCHIVE_PATH);
            app.pointer_down(a[0] + 4.0, a[1] + a[3] / 2.0, Button::Primary);
            app.pointer_up(a[0] + 4.0, a[1] + a[3] / 2.0, Button::Primary);
        }
        20 => click(app, lay_rect(&app.rt, &[3, 0, 0, 1])),
        21 => click(app, mailbox(app, SENT as usize)),
        22 => click(app, mail::row_rect(app, 1)),
        23 => {
            click(app, lay_rect(&app.rt, &mail::SEARCH_PATH));
            key(app, Key::Char('a'));
            key(app, Key::Char('d'));
        }
        24 => {
            key(app, Key::Backspace);
            key(app, Key::Backspace);
        }
        25 => click(app, mailbox(app, INBOX as usize)),
        26 => click(app, mail::row_rect(app, 2)),
        27 => click(app, lay_rect(&app.rt, &[3, 0, 0, 3])),
        28 => click(app, mailbox(app, STARRED as usize)),
        29 => click(app, mail::row_rect(app, 0)),
        30 => click(app, mailbox(app, INBOX as usize)),
        _ => app.pointer_leave(),
    }
}
const STEPS: usize = 31;

#[cfg(target_os = "macos")]
#[test]
fn mail_is_pixel_exact() {
    for ring in RINGS {
        let cfg = || with_ring(config(mail::W, mail::H, 1.0), ring);
        let mut app = mail::mount(Mail::new(), cfg(), true);
        app.windowed = ring == 0;
        check("mail", app, STEPS, 1, step, |a| mail::mount(Mail::same_state(&a.owner), cfg(), true));
    }
}

/// The benchmark's own scale (2) on the windowed ring.
#[cfg(target_os = "macos")]
#[test]
fn mail_is_pixel_exact_at_scale_2() {
    let cfg = || with_ring(config(mail::W, mail::H, 2.0), 0);
    let mut app = mail::mount(Mail::new(), cfg(), true);
    app.windowed = true;
    check("mail x2", app, STEPS, 1, step, |a| mail::mount(Mail::same_state(&a.owner), cfg(), true));
}

/// What the list must show, the dumb way: every message tested on its haystack.
fn oracle(m: &Mail) -> Vec<u32> {
    let needle = m.query().to_lowercase();
    let mb = *m.mailbox;
    (0..m.msgs.len() as u32)
        .filter(|&i| {
            let x = &m.msgs[i as usize];
            let here = if mb == STARRED { x.starred } else { x.mailbox == mb };
            let hay = format!("{} {}", TOPICS[x.topic as usize], PEOPLE[x.person as usize].0).to_lowercase();
            here && hay.contains(&needle)
        })
        .collect()
}

fn ev(k: Key) -> Ev {
    Ev { x: 0.0, y: 0.0, dx: 0.0, dy: 0.0, key: Some(k), mods: Mods::default(), button: Button::Primary, on: true }
}

#[test]
fn shown_and_window_follow_the_oracle() {
    let mut app = mail::mount(Mail::new(), config(mail::W, mail::H, 1.0), false);
    let mut r = 0x9e37_79b9u64;
    for _ in 0..300 {
        r ^= r << 13;
        r ^= r >> 7;
        r ^= r << 17;
        app.turn(|m| match r % 7 {
            0 => m.search_key(&ev(Key::Char(b"raedi tq?"[(r >> 8) as usize % 9] as char))),
            1 => m.search_key(&ev(Key::Backspace)),
            2 => m.choose_mailbox([INBOX, STARRED, SENT, ARCHIVE, TRASH][(r >> 8) as usize % 5]),
            3 | 4 => {
                if !m.rows.is_empty() {
                    let k = (r >> 8) as usize % m.rows.len();
                    let id = m.rows.get(k).unwrap().id;
                    m.open(id);
                }
            }
            5 => m.move_open([ARCHIVE, TRASH, INBOX][(r >> 8) as usize % 3]),
            _ => m.toggle_star_open(),
        });
        app.frame();
        let m = &app.owner;
        assert_eq!(m.shown, oracle(m), "shown (query {:?}, mailbox {})", m.query(), BOXES[*m.mailbox as usize]);
        let ids: Vec<u32> = m.rows.iter().map(|r| r.id).collect();
        let lo = m.first.min(m.shown.len());
        assert_eq!(ids, m.shown[lo..(lo + mail::WINDOW).min(m.shown.len())], "the window's rows");
        for row in m.rows.iter() {
            assert_eq!(*row.sel, m.selected == Some(row.id));
            assert_eq!(*row.unread, m.msgs[row.id as usize].unread);
        }
        let mut counts = [0u32; 6];
        for x in &m.msgs {
            if x.unread {
                counts[x.mailbox as usize] += 1;
            }
        }
        assert_eq!(counts, m.counts, "unread counts");
    }
}

#[test]
fn the_layout_is_where_the_harness_clicks() {
    let app = mail::mount(Mail::new(), config(mail::W, mail::H, 2.0), false);
    assert_eq!(lay_rect(&app.rt, &[1, 1]), [mail::LIST_X, mail::LIST_TOP, mail::LIST_W, mail::LIST_H], "the list's viewport (its clip)");
    assert_eq!(lay_rect(&app.rt, &mail::SEARCH_PATH), [223.0, 40.0, 336.0, 36.0], "the search field");
    assert_eq!(lay_rect(&app.rt, &[3]), mail::DETAIL, "the detail pane (its clip)");
    assert_eq!(app.owner.rows.len(), mail::WINDOW);
    let a = lay_rect(&app.rt, &mail::ARCHIVE_PATH);
    assert!(a[0] > 572.0 && a[2] > 40.0 && a[3] == 38.0, "the Archive button: {a:?}");
}

#[test]
fn the_sample_is_zgx_s() {
    let m = mail::messages(4000);
    // the generator's first draw (Park-Miller from 42: 42 * 48271 = 2027382) picks the first sender
    assert_eq!(m[0].person, (2027382 % 8) as u8, "Donald Knuth");
    assert_eq!(m.len(), 4000);
    assert!(m.iter().all(|x| (2..=4).contains(&x.paras.iter().filter(|&&p| p > 0).count())));
    assert_eq!(m[0].day, 28);
    assert_eq!(m[39].day, 27);
    assert_eq!(m[3999].day, (28 - (4000 / 40) % 28) as u8);
    let inbox = m.iter().filter(|x| x.mailbox == INBOX).count();
    assert!((1700..1860).contains(&inbox), "about four ninths in the inbox: {inbox}");
}
