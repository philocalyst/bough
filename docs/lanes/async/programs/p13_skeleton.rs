//! 13. A skeleton that morphs into content: a profile card is drawn as grey placeholders until its data
//! arrives; then each placeholder becomes the part it stood for (the grey disc grows into the avatar, the
//! two bars into the name and the bio) instead of being swapped out.
//!
//! The pairing is by tag: a placeholder and the content part with the same `tag` are one part in two
//! states. When the answer lands, the card is released *with* the morph: for each pair the kernel moves and
//! scales the content from the placeholder's box to its own and cross-fades the two, from the release time
//! over 300 ms. These are per-part parameters of the kernel (two boxes, two alphas, a start and a duration,
//! advanced by the presenter's clock); no turn runs while it plays, and the damage is the union of each pair's
//! boxes, known at the release. Parts without a partner fade. The placeholder records are released when the
//! morph ends (one turn, scheduled for the end time).
//!
//! Not written: measuring the placeholders and the content, FLIP arithmetic, an animation loop, a timer to
//! remove the placeholders, a "loaded" flag the skeleton and the content both test.
use crate::api::{self, User};
use bough::prelude::*;

pub struct Card {
    pub id: Signal<u64>,
    pub profile: Signal<Option<(User, Vec<String>)>>,
}

impl Card {
    pub fn new(id: u64) -> Card {
        Card { id: Signal::new(id), profile: Signal::new(None) }
    }
}

const GREY: Color = rgb(0xe3e6ea);
const ROW: Flex = Flex::row().gap(12.0);
const COL: Flex = Flex::column().gap(6.0);

fn skeleton() -> impl Element<Card> {
    div().layout(ROW).child((
        block(48.0, 48.0).bg(GREY).rounded(24.0).tag(1),
        div().layout(COL).child((block(140.0, 14.0).bg(GREY).rounded(4.0).tag(2), block(220.0, 12.0).bg(GREY).rounded(4.0).tag(3))),
    ))
}

fn content((u, bio): &(User, Vec<String>)) -> impl Element<Card> + '_ {
    div().layout(ROW).child((
        block(48.0, 48.0).bg(rgb(0x2f6bff)).rounded(24.0).tag(1),
        div().layout(COL).child((text(u.name.clone()).tag(2), para(bio.join(" ")).tag(3))),
    ))
}

impl View for Card {
    fn view(&self) -> impl Element<Self> + '_ {
        div().p(16.0).child((
            morph(Curve::smooth(0.3), match &*self.profile {
                None => Arm2::A(skeleton()),
                Some(p) => Arm2::B(content(p)),
            }),
            ask(sig!(self.id), api::profile).into(sig!(self.profile)),
        ))
    }
}
