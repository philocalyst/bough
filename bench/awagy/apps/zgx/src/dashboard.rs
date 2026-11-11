use std::sync::Arc;
use zgx::{compose::prelude::*, image::ImageData};
use zgx_desktop::{Application, WindowOptions};

const IMAGES: [&[u8]; 10] = [
    include_bytes!("../../assets/images/dashboard-0.png"),
    include_bytes!("../../assets/images/dashboard-1.png"),
    include_bytes!("../../assets/images/dashboard-2.png"),
    include_bytes!("../../assets/images/dashboard-3.png"),
    include_bytes!("../../assets/images/dashboard-4.png"),
    include_bytes!("../../assets/images/dashboard-5.png"),
    include_bytes!("../../assets/images/dashboard-6.png"),
    include_bytes!("../../assets/images/dashboard-7.png"),
    include_bytes!("../../assets/images/dashboard-8.png"),
    include_bytes!("../../assets/images/dashboard-9.png"),
];

// The site's dashboard workload: title, a toolbar (text input at (24, 70) 360 x 40, Refresh at (416, 70) and the
// Enabled/Disabled toggle at (624, 70), both 160 x 40), a "Refresh count" line, and from y = 180 a scroll view
// of 1000 records (64 x 64 image and two lines of text), all mounted. Reached by layout, not coordinates.
pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let black = rgb(0x000000);
    Application::new()
        .window(WindowOptions { title: "zgx Dashboard".into(), width: 1000., height: 700., ..Default::default() })
        .run(move |cx| {
            let images: Vec<Arc<ImageData>> = IMAGES.iter().map(|bytes| zgx_gpu::assets::decode_image(bytes).expect("png")).collect();
            let notes = cx.ui.signal("Project notes".to_owned());
            let count = cx.ui.signal(0_u32);
            let enabled = cx.ui.signal(false);
            let offset = cx.ui.signal(0.0_f32);
            let shown_count = count.clone();
            let shown_enabled = enabled.clone();
            cx.render(
                column()
                    .w_full()
                    .h_full()
                    .bg(rgb(0xffffff))
                    .text_size(14.)
                    .text_color(black)
                    .child(div().h(70.).shrink_0().pl(24.).pt(16.).child(text("Project dashboard").text_size(24.).text_color(black)))
                    .child(
                        row()
                            .h(40.)
                            .shrink_0()
                            .px(24.)
                            .gap(32.)
                            .items_center()
                            .child(text_input("Project notes", notes).w(360.).h(40.).shrink_0().p(10.).bg(rgb(0xf1f1f1)).text_color(black))
                            .child(
                                button()
                                    .w(160.)
                                    .h(40.)
                                    .shrink_0()
                                    .bg(rgb(0xe8e8e8))
                                    .justify_center()
                                    .items_center()
                                    .child(text("Refresh").text_color(black))
                                    .on_click(move || { count.set(count.get() + 1); }),
                            )
                            .child(
                                button()
                                    .w(160.)
                                    .h(40.)
                                    .shrink_0()
                                    .ml(16.)
                                    .bg(rgb(0xdbe9dc))
                                    .justify_center()
                                    .items_center()
                                    .child(text_signal(move || if shown_enabled.get() { "Enabled".into() } else { "Disabled".into() }).text_color(black))
                                    .on_click(move || { enabled.set(!enabled.get()); }),
                            ),
                    )
                    .child(div().h(70.).shrink_0().pl(24.).pt(14.).child(text_signal(move || format!("Refresh count: {}", shown_count.get())).text_color(black)))
                    .child(scroll(offset).w_full().grow().min_h(0.).child(keyed(
                        || (0..1000).collect::<Vec<usize>>(),
                        move |index, _| {
                            row()
                                .w_full()
                                .h(88.)
                                .shrink_0()
                                .px(24.)
                                .py(12.)
                                .gap(16.)
                                .items_center()
                                .bg(rgb(if index % 2 == 0 { 0xffffff } else { 0xf6f6f6 }))
                                .child(image("Project image", images[index % images.len()].clone()).w(64.).h(64.).shrink_0())
                                .child(
                                    column()
                                        .gap(4.)
                                        .child(text(format!("Project {}", index + 1)).text_color(black))
                                        .child(text("A project record with descriptive dashboard information.").text_color(black)),
                                )
                        },
                    ))),
            );
        })
}
