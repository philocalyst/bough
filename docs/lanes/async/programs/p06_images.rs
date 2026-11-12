//! 6. Image decode: a gallery whose thumbnails are fetched on the UI thread's executor and decoded on a
//! worker, each row owning its own question.
//!
//! A row's question lives as long as the row is mounted: a row scrolled out of a virtual list, or removed,
//! drops its fetch or its decode (the decode's worker sees the cancel flag at its next check and stops). The
//! decode is the only `Send` part; the fetch is not required to be. Thumbnails arrive one by one, each its
//! own frame, and a row never shows another row's pixels: the answer lands in the row that asked.
//!
//! Not written: a request map keyed by row, a cancel-on-unmount hook, a thread pool, a channel back to
//! the UI thread, a "row still there?" check.
use crate::api::{self, Image};
use bough::prelude::*;

pub struct Photo {
    pub url: Signal<String>,
    pub pixels: Signal<Option<Image>>,
}

impl Photo {
    pub fn new(url: &str) -> Photo {
        Photo { url: Signal::new(url.to_string()), pixels: Signal::new(None) }
    }
}

/// Bytes from the network, pixels from a worker. `compute` runs its closure on bough's worker pool; dropping
/// the future (the row went away) raises the closure's cancel flag.
pub async fn thumbnail(url: String) -> Option<Image> {
    let bytes = api::fetch(url).await;
    compute(move |cancel: &Cancel| decode(&bytes, cancel)).await
}

/// A stand-in decoder (the media lane owns the real one): checks the flag between rows, as a real one would.
pub fn decode(bytes: &[u8], cancel: &Cancel) -> Option<Image> {
    let mut acc = 0u32;
    for row in bytes.chunks(64) {
        if cancel.is_set() {
            return None;
        }
        acc = row.iter().fold(acc, |a, &b| a.rotate_left(5) ^ b as u32);
    }
    Some(Image { w: 64, h: 64, rgba: acc | 0xff00_0000 })
}

impl View for Photo {
    fn view(&self) -> impl Element<Self> + '_ {
        div().w(64.0).h(64.0).bg(sig!(self.pixels).map(|p| p.as_ref().map_or(rgb(0xe0e0e0), |i| Color(i.rgba)))).child((
            // per row: asked when the row mounts, dropped when it unmounts
            ask(sig!(self.url), thumbnail).into(sig!(self.pixels)),
        ))
    }
}

pub struct Gallery {
    pub photos: List<Photo>,
}

impl View for Gallery {
    fn view(&self) -> impl Element<Self> + '_ {
        list(sig!(self.photos)).virtualized(64.0)
    }
}
