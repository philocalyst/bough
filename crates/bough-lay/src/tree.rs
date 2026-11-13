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
pub fn measurer<M: Measure>(m: M) -> MeasureFn {
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

    /// A paragraph: words of given widths separated by spaces, broken greedily, lines of one height.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Para {
        pub words: Rc<[f32]>,
        pub space: f32,
        pub line_h: f32,
    }
    impl Measure for Para {
        const WRAPS: bool = true;
        fn inline(&self) -> (f32, f32) {
            let mn = self.words.iter().cloned().fold(0.0f32, f32::max);
            let mut mx = 0.0;
            for (i, w) in self.words.iter().enumerate() {
                mx += w + if i > 0 { self.space } else { 0.0 };
            }
            (mn, mx)
        }
        fn block(&self, w: f32) -> Block {
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
}
