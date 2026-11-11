#[path = "../../scroll_config.rs"]
mod scroll_config;

use zgx::compose::prelude::*;
use zgx_desktop::{Application, WindowOptions};

// The site's scroll workload: SCROLL_ROWS rows of 960 x 48 with 12 px padding, black 14 px "Item N" text,
// alternating #f5f5f5 / #e6e6e6, all mounted (no virtual_list), in a plain scroll view.
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    Application::new()
        .window(WindowOptions { title: "zgx Scroll".into(), width: 1000., height: 700., ..Default::default() })
        .run(|cx| {
            let offset = cx.ui.signal(0.0_f32);
            let rows = scroll_config::row_count();
            cx.render(
                scroll(offset).w_full().h_full().bg(rgb(0xffffff)).child(keyed(
                    move || (0..rows).collect::<Vec<usize>>(),
                    |index, _| {
                        div()
                            .w(960.)
                            .h(48.)
                            .p(12.)
                            .bg(rgb(if index % 2 == 0 { 0xf5f5f5 } else { 0xe6e6e6 }))
                            .child(text(format!("Item {}", index + 1)).text_size(14.).text_color(rgb(0x000000)))
                    },
                )),
            );
        })
}
