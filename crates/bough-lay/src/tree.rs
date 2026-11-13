//! An element as a style tree: what the general solver (`vendor/cephal`) is given. This is the differential
//! oracle for every layout the types express, and the fallback for what they do not. Cold code.
use crate::el::{Block, Measure, content};
use crate::flex::Spec;
use crate::val::{Av, Cx, V};
use bough_expr::U;
use std::rc::Rc;

/// Content size from (known content width, known content height, available width; `None` = max-content,
/// `Some(NaN)` = min-content).
pub type MeasureFn = Rc<dyn Fn(Option<f32>, Option<f32>, Option<f32>) -> (f32, f32)>;

pub enum Kind {
    Flex,
    Leaf(MeasureFn),
    /// An absent optional child: no node in the solver's tree, `total` records with empty rects.
    Gone,
}
pub struct Node {
    pub spec: Spec,
    pub kind: Kind,
    pub kids: Vec<Node>,
    /// Records of this node itself (0 or 1).
    pub leaves: usize,
    /// Records of the subtree.
    pub total: usize,
}
impl Node {
    pub fn gone(total: usize) -> Node {
        Node { spec: Spec::new(false), kind: Kind::Gone, kids: Vec::new(), leaves: 0, total }
    }
}

/// The measure function of a leaf, for the solver: the same code the typed path runs.
pub fn measurer<M: Measure + Clone + 'static>(m: M) -> MeasureFn {
    Rc::new(move |kw, kh, av| {
        let u = U::default();
        let mut cx = Cx::new(1.0, &u, [0.0; 2]);
        let av = match av {
            None => Av::Max,
            Some(a) if a.is_nan() => Av::Min,
            Some(a) => Av::Def(V::k(a)),
        };
        let [w, h] = content(&m, &mut cx, kw.map(V::k), kh.map(V::k), av);
        (w.v, h.v)
    })
}

/// Stand-ins for the text lane's leaves, with the same contract.
pub mod standin {
    use super::*;

    /// One line of fixed size (a label, a number).
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Line(pub f32, pub f32);
    impl Measure for Line {
        const WRAPS: bool = false;
        #[inline(always)]
        fn inline(&self) -> (f32, f32) {
            (self.0, self.0)
        }
        #[inline(always)]
        fn block(&self, _: f32) -> Block {
            Block { w: self.0, h: self.1, lo: f32::NEG_INFINITY, hi: f32::INFINITY }
        }
    }

    /// A paragraph: words of given widths separated by spaces, broken greedily, lines of one height. As the
    /// text lane's leaf will, it keeps its inline sizes and the last answer with its interval, so a question
    /// at a width inside that interval is a range check.
    #[derive(Clone, Debug)]
    pub struct Para {
        pub words: Rc<[f32]>,
        pub space: f32,
        pub line_h: f32,
        inline: (f32, f32),
        last: std::cell::Cell<Block>,
    }
    impl Para {
        pub fn new(words: Rc<[f32]>, space: f32, line_h: f32) -> Para {
            let mn = words.iter().cloned().fold(0.0f32, f32::max);
            let mut mx = 0.0;
            for (i, w) in words.iter().enumerate() {
                mx += w + if i > 0 { space } else { 0.0 };
            }
            Para { words, space, line_h, inline: (mn, mx), last: std::cell::Cell::new(Block { w: 0.0, h: 0.0, lo: 1.0, hi: 0.0 }) }
        }
        fn breaks(&self, w: f32) -> Block {
            let (mut lines, mut cur, mut widest, mut hi) = (1u32, 0.0f32, 0.0f32, f32::INFINITY);
            for (i, &ww) in self.words.iter().enumerate() {
                if i == 0 {
                    cur = ww;
                } else if cur + self.space + ww <= w {
                    cur += self.space + ww;
                } else {
                    // this word did not fit: the line would take it at this width
                    hi = hi.min(cur + self.space + ww);
                    widest = widest.max(cur);
                    cur = ww;
                    lines += 1;
                }
            }
            widest = widest.max(cur);
            Block { w: widest, h: lines as f32 * self.line_h, lo: widest, hi }
        }
    }
    impl Measure for Para {
        const WRAPS: bool = true;
        #[inline(always)]
        fn inline(&self) -> (f32, f32) {
            self.inline
        }
        #[inline(always)]
        fn block(&self, w: f32) -> Block {
            let b = self.last.get();
            if (w >= b.lo) & (w < b.hi) {
                return b;
            }
            let b = self.breaks(w);
            self.last.set(b);
            b
        }
    }
}

impl crate::shape::Wrap for standin::Para {
    #[inline]
    fn inline(&self) -> (f32, f32) {
        Measure::inline(self)
    }
    #[inline]
    fn block(&self, w: f32) -> Block {
        Measure::block(self, w)
    }
    fn key(&self) -> [u32; 2] {
        // (a stand-in: the text lane's key is a content hash; here the words' storage and count)
        [Rc::as_ptr(&self.words) as *const f32 as usize as u32, self.words.len() as u32]
    }
    fn rc(&self) -> Rc<dyn crate::shape::Wrap> {
        Rc::new(self.clone())
    }
}
