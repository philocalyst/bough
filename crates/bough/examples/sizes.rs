//! The sizes of the per-row structs the memory ledger counts (lane memory, round 6): what a mounted row holds
//! beside its GPU data. `cargo run --release -p bough --example sizes`
use std::mem::size_of;

fn main() {
    use bough::layout::{LeafCtx, Spec};
    use bough_text::para::Para;
    println!("LeafCtx {}", size_of::<LeafCtx>());
    println!("  GlyphRun (run) {}", size_of::<bough_text::GlyphRun>());
    println!("  Vec<u8> (text) + Vec<u32> (starts) {}", 2 * size_of::<Vec<u8>>());
    println!("  Option<Shaped> (block) {}", size_of::<Option<bough_text::window::Shaped>>());
    println!("  Option<Spec> (user) {}", size_of::<Option<Spec>>());
    println!("  Para (para) {}", size_of::<Para>());
    println!("Spec {}", size_of::<Spec>());
    println!("rt::Node {}", size_of::<bough::rt::Node>());
    println!("text::TextNode {}", size_of::<bough::text::TextNode>());
    println!("  TShape {}", size_of::<bough::text::TShape>());
    println!("  String {}", size_of::<String>());
    println!("describe::DynItem {}", size_of::<bough::describe::DynItem>());
    println!("  rt::Scope {}", size_of::<bough::rt::Scope>());
    println!("layout::LInfo {}", size_of::<bough::layout::LInfo>());
    println!("cephal Layout {}", size_of::<cephal::tree::Layout>());
    println!("cephal NodeCache (pointer) {}", size_of::<cephal::tree::NodeCache>());
    println!("cephal LayoutOutput {}", size_of::<cephal::tree::LayoutOutput>());
    println!("cephal LayoutInput {}", size_of::<cephal::tree::LayoutInput>());
    println!("instance {}", size_of::<bough_frame::Instance>());
}
