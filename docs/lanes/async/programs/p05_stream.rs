//! 5. Streamed responses: a live price (a question whose answer is a sequence) and a chat reply streamed
//! token by token (a command whose result is a sequence).
//!
//! Ticker: switching symbol drops the old stream (the server sees the subscription go) and keeps the old
//! symbol's price and name together on screen until the new symbol's first tick; every later tick is its
//! own frame. Chat: the message shows at once; the reply grows a token per turn; Stop drops the stream
//! where it is; a second message while a reply streams waits for it (actions land in the order sent).
//!
//! Not written: subscribe/unsubscribe bookkeeping, a "which symbol is this tick for" check, a token
//! buffer, an abort controller, a "streaming" flag (the action knows it is running).
use crate::api::{self, Token};
use bough::prelude::*;

pub struct Ticker {
    pub symbol: Signal<String>,
    pub price: Signal<Option<f64>>,
}

impl Ticker {
    pub fn new(symbol: &str) -> Ticker {
        Ticker { symbol: Signal::new(symbol.to_string()), price: Signal::new(None) }
    }
    pub fn pick(&mut self, s: &str) {
        *self.symbol = s.to_string();
    }
}

const COL: Flex = Flex::column().gap(4.0);

impl View for Ticker {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).child((
            text(sig!(self.symbol).map(|s| s.clone())),
            text(sig!(self.price).map(|p| p.map_or("\u{2014}".to_string(), |p| format!("{p:.2}")))),
            // each tick replaces the price; a new symbol drops the old stream
            follow(sig!(self.symbol), api::ticks).into(sig!(self.price)),
        ))
    }
}

pub struct Line {
    pub mine: bool,
    pub text: String,
}

pub struct Chat {
    pub log: Signal<Vec<Line>>,
    pub draft: Signal<String>,
    pub reply: Action<String, Token>,
}

impl Chat {
    pub fn new() -> Chat {
        Chat { log: Signal::new(Vec::new()), draft: Signal::new(String::new()), reply: Action::stream(api::complete).then(Chat::token) }
    }
    pub fn send(&mut self) {
        let prompt = std::mem::take(&mut *self.draft);
        self.log.push(Line { mine: true, text: prompt.clone() });
        self.reply.run(prompt);
    }
    /// A reply's first token opens its line; every token extends the last line.
    fn token(&mut self, t: Token) {
        if self.log.last().is_none_or(|l| l.mine) {
            self.log.push(Line { mine: false, text: String::new() });
        }
        self.log.last_mut().unwrap().text.push_str(&t.0);
    }
    pub fn stop(&mut self) {
        self.reply.cancel();
    }
}

impl View for Chat {
    fn view(&self) -> impl Element<Self> + '_ {
        div().layout(COL).child((
            each(self.log.iter().map(|l| text(if l.mine { format!("> {}", l.text) } else { l.text.clone() }))),
            self.reply.running().then(|| button("Stop").on_click(Chat::stop)),
            text_input(sig!(self.draft)),
            button("Send").on_click(Chat::send),
        ))
    }
}
