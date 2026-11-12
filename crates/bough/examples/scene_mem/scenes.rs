//! Heavy scenes for the memory lane (round 7), written against `bough::prelude` the way an application is:
//!
//! * `feed`: a virtualised social feed of 2,000 posts (a `List<Post>` in a scroll box): avatar, name, time,
//!   2-4 wrapped lines, a photo in every third post, an action row with a live like counter.
//! * `board`: a design board of 40 x 30 = 1,200 mounted cards (no virtualisation): title, wrapped body, three
//!   rounded tags, borders, nested boxes; the "many layers" case.
//! * `gallery`: rows of five thumbnails in a virtualised list; `GALLERY_N` images (default 10,000), distinct.
//!
//! Images come from files under `MEDIA_DIR` (the lane's generated test media; see the journal). With today's
//! API (`image(&'static [u8])`, PNG only, one 1024 x 1024 atlas, nothing evicted) the gallery cannot show more
//! distinct images than fit the atlas once; that is measured, not avoided.
use bough::prelude::*;

pub fn media_dir() -> String {
    std::env::var("MEDIA_DIR").unwrap_or_else(|_| "media".into())
}

/// `n` PNG files of the baseline set (`png/qNNN.png`, 320 x 214), leaked: `image()` takes `&'static [u8]`.
pub fn pngs(n: usize) -> Vec<&'static [u8]> {
    let d = media_dir();
    (0..n).map(|i| &*Box::leak(std::fs::read(format!("{d}/png/q{:03}.png", i % 100)).expect("baseline png").into_boxed_slice())).collect()
}

const AVATARS: [&[u8]; 4] = [
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-0.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-3.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-5.png"),
    include_bytes!("../../../../comparisons/bough-awagy/assets/dashboard-8.png"),
];

const WORDS: [&str; 24] = [
    "the", "layout", "memory", "frame", "signal", "glyph", "atlas", "surface", "tile", "row", "media", "decode", "scroll", "window",
    "record", "page", "column", "region", "idle", "cache", "photo", "video", "gallery", "resident",
];

fn sentence(seed: usize, words: usize) -> String {
    let mut s = String::new();
    let mut x = seed.wrapping_mul(2654435761) | 1;
    for k in 0..words {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        if k > 0 {
            s.push(' ');
        }
        s.push_str(WORDS[x % WORDS.len()]);
    }
    s
}

// ------------------------------------------------------------------------------------------------ feed

pub struct Post {
    name: String,
    body: String,
    likes: Signal<u32>,
    avatar: &'static [u8],
    photo: Option<&'static [u8]>,
}

impl View for Post {
    fn view(&self) -> impl Element<Self> + '_ {
        column((
            row((
                image(self.avatar).w(40.0).h(40.0),
                column((text(self.name.as_str()).font(15.0, 20.0), text("2 h").font(12.0, 16.0).color(rgb(0x707070)))).gap(2.0),
            ))
            .gap(10.0)
            .items(Align::Center),
            para(self.body.as_str()).font(14.0, 20.0).color(rgb(0x202020)).w_pct(1.0),
            self.photo.map(|p| image(p).w(480.0).h(270.0)),
            row((
                button("Like").h(28.0),
                text(sig!(self.likes).map(|n| format!("{n} likes"))).font(13.0, 18.0),
                button("Share").h(28.0),
            ))
            .gap(12.0)
            .items(Align::Center),
        ))
        .gap(8.0)
        .p(12.0)
        .w_pct(1.0)
        .bg(rgb(0xffffff))
        .rounded(8.0)
    }
}

pub struct Feed {
    posts: List<Post>,
    y: Signal<f32>,
}

impl View for Feed {
    fn view(&self) -> impl Element<Self> + '_ {
        div()
            .w_pct(1.0)
            .h_pct(1.0)
            .bg(rgb(0xeef0f3))
            .child(div().w_pct(1.0).h_pct(1.0).min_h(0.0).scroll(sig!(self.y)).child(list(sig!(self.posts)).w_pct(1.0).estimate(160.0)))
    }
}

pub fn feed(n: usize) -> Feed {
    // one photo for every post: today's image atlas (1024 x 1024, nothing evicted) holds one 960 x 540 px decode
    let photos = pngs(1);
    let posts = List::from_iter_in((0..n).map(|i| Post {
        name: format!("Person {}", i + 1),
        body: sentence(i, 12 + (i * 7) % 30),
        likes: Signal::new((i * 13 % 500) as u32),
        avatar: AVATARS[i % AVATARS.len()],
        photo: (i % 3 == 0).then(|| photos[i % photos.len()]),
    }));
    Feed { posts, y: Signal::new(0.0) }
}

// ------------------------------------------------------------------------------------------------ board

pub struct Board {
    rows: usize,
    cols: usize,
    y: Signal<f32>,
}

fn card(i: usize) -> impl Element<Board> {
    let tag = |c: u32| block(36.0, 14.0).bg(rgb(c)).rounded(7.0);
    column((
        text(format!("Card {}", i + 1)).font(14.0, 18.0),
        para(sentence(i, 10)).font(11.0, 14.0).color(rgb(0x505050)).w(156.0),
        row((tag(0xff6b6b), tag(0x4ecdc4), tag(0xffe66d))).gap(6.0),
    ))
    .gap(6.0)
    .p(10.0)
    .w(176.0)
    .h(120.0)
    .bg(rgb(0xffffff))
    .rounded(6.0)
    .border(1.0, rgb(0xd0d4da))
}

impl View for Board {
    fn view(&self) -> impl Element<Self> + '_ {
        let cols = self.cols;
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0xf4f5f7)).scroll(sig!(self.y)).child(each((0..self.rows).map(move |r| row(each((0..cols).map(move |c| card(r * cols + c)))).gap(8.0).p(4.0))))
    }
}

pub fn board(rows: usize, cols: usize) -> Board {
    Board { rows, cols, y: Signal::new(0.0) }
}

// ------------------------------------------------------------------------------------------------ gallery (today's API)

pub struct Strip {
    imgs: [&'static [u8]; 5],
}

impl View for Strip {
    fn view(&self) -> impl Element<Self> + '_ {
        row(each(self.imgs.iter().map(|&p| image(p).w(180.0).h(120.0)))).gap(8.0).p(4.0)
    }
}

pub struct GalleryAtlas {
    strips: List<Strip>,
    y: Signal<f32>,
}

impl View for GalleryAtlas {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w_pct(1.0).h_pct(1.0).bg(rgb(0x111111)).scroll(sig!(self.y)).child(list(sig!(self.strips)).w_pct(1.0).estimate(128.0))
    }
}

/// `n` images, `distinct` of them different (the baseline PNG set has 100).
pub fn gallery_atlas(n: usize, distinct: usize) -> GalleryAtlas {
    let p = pngs(distinct.min(100));
    let strips = List::from_iter_in((0..n.div_ceil(5)).map(|r| Strip { imgs: std::array::from_fn(|k| p[(r * 5 + k) % p.len()]) }));
    GalleryAtlas { strips, y: Signal::new(0.0) }
}
