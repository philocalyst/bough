//! The Core Animation side of foreign elements (`foreign.rs`): a [`ForeignHost`] that keeps one `CALayer` per
//! layer-composited element among the siblings of bough's surface layer.
//!
//! The arrangement under the window's root layer, bottom to top:
//!
//! ```text
//! root
//! ├─ foreign layers of plane Below, in paint order      (Mode::Under: seen through the surface's holes)
//! ├─ bough's surface layer (not opaque once a Below layer exists: its alpha is the composition)
//! └─ clip wrappers of plane Above, in paint order       (Mode::Over: a wrapper with the element's clip,
//!                                                         the content layer inside it)
//! ```
//!
//! A Below layer needs only its frame: clip, rounded corners and everything bough paints over the element
//! are already in the surface's alpha. An Above layer is clipped and rounded here, by Core Animation.
//! Every change is one explicit transaction without implicit animations. (Not yet joined to the transaction
//! in which the publisher thread swaps the surface: a moving hole and its layer may differ by one refresh.)
use crate::foreign::{ForeignHost, Placement, Plane};
use objc2::Message;
use objc2::rc::Retained;
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::NSString;
use objc2_quartz_core::{CALayer, CATransaction};

struct Item {
    id: u64,
    plane: Plane,
    order: u32,
    content: Retained<CALayer>,
    /// `Above`: the wrapper that clips the content to the element's clip.
    wrap: Option<Retained<CALayer>>,
}

pub struct LayerHost {
    root: Retained<CALayer>,
    surface: Retained<CALayer>,
    make: Box<dyn FnMut(u64) -> Retained<CALayer>>,
    items: Vec<Item>,
}

impl LayerHost {
    /// A host for the window whose surface layer is `surface` (already attached to the window's root
    /// layer). `make(id)` creates the content layer of foreign element `id` (a web view's layer, an
    /// `AVPlayerLayer`, a layer with an `IOSurface` as contents).
    pub fn new(surface: &CALayer, make: impl FnMut(u64) -> Retained<CALayer> + 'static) -> LayerHost {
        let root = surface.superlayer().expect("the surface layer is attached");
        surface.setName(Some(&NSString::from_str("bough")));
        LayerHost { root, surface: surface.retain(), make: Box::new(make), items: Vec::new() }
    }

    /// Root coordinates are y-up points; placements are y-down device px.
    fn frame(&self, x0: f32, y0: f32, x1: f32, y1: f32, scale: f32) -> CGRect {
        let s = scale as f64;
        let h = self.surface.frame().size.height;
        CGRect::new(CGPoint::new(x0 as f64 / s, h - y1 as f64 / s), CGSize::new((x1 - x0) as f64 / s, (y1 - y0) as f64 / s))
    }

    /// Put the sublayers in the documented order.
    fn restack(&mut self) {
        self.items.sort_by_key(|i| i.order);
        for i in self.items.iter().filter(|i| i.plane == Plane::Below) {
            self.root.addSublayer(&i.content); // re-adding a sublayer moves it to the top
        }
        self.root.addSublayer(&self.surface);
        for i in self.items.iter().filter(|i| i.plane == Plane::Above) {
            self.root.addSublayer(i.wrap.as_ref().unwrap());
        }
        // with a layer beneath it the surface's alpha is the composition
        self.surface.setOpaque(!self.items.iter().any(|i| i.plane == Plane::Below));
    }
}

impl ForeignHost for LayerHost {
    fn place(&mut self, p: &Placement) {
        if p.plane == Plane::Inline {
            return;
        }
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let k = match self.items.iter().position(|i| i.id == p.id) {
            Some(k) => k,
            None => {
                let content = (self.make)(p.id);
                content.setName(Some(&NSString::from_str(&format!("foreign-{}", p.id))));
                let wrap = (p.plane == Plane::Above).then(|| {
                    let w = CALayer::new();
                    w.setMasksToBounds(true);
                    w.setName(Some(&NSString::from_str(&format!("clip-{}", p.id))));
                    w.addSublayer(&content);
                    w
                });
                self.items.push(Item { id: p.id, plane: p.plane, order: u32::MAX, content, wrap });
                self.items.len() - 1
            }
        };
        let [x0, y0, x1, y1] = p.rect;
        match &self.items[k].wrap {
            None => self.items[k].content.setFrame(self.frame(x0, y0, x1, y1, p.scale)),
            Some(w) => {
                // the wrapper is the clip; the content sits in it at the box
                let [cx0, cy0, cx1, cy1] = p.clip.map(|v| v as f32);
                let wf = self.frame(cx0, cy0, cx1, cy1, p.scale);
                w.setFrame(wf);
                let f = self.frame(x0, y0, x1, y1, p.scale);
                let c = &self.items[k].content;
                c.setFrame(CGRect::new(CGPoint::new(f.origin.x - wf.origin.x, f.origin.y - wf.origin.y), f.size));
                c.setCornerRadius((p.radius / p.scale) as f64);
                c.setMasksToBounds(p.radius > 0.0);
            }
        }
        if self.items[k].order != p.order {
            self.items[k].order = p.order;
            self.restack();
        }
        CATransaction::commit();
    }

    fn remove(&mut self, id: u64) {
        let Some(k) = self.items.iter().position(|i| i.id == id) else { return };
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        let it = self.items.remove(k);
        it.wrap.as_ref().unwrap_or(&it.content).removeFromSuperlayer();
        self.restack();
        CATransaction::commit();
    }
}
