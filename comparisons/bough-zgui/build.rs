//! Generates the gallery template (`gallery_nodes.rs` in OUT_DIR) from the dump of zgui's laid-out
//! document (`bench/results/input/zgui/dump/gallery_s13.tsv`: zgui's own DOM preorder, window boxes and
//! computed styles at 1600 x 1000, scale 1). The document is then a `static` table: mounting it is a pass
//! over constants, with no style sheet to parse and no cascade to run.
use std::collections::HashMap;
use std::fmt::Write;

fn color(s: &str) -> Option<u32> {
    let h = s.strip_prefix('#')?;
    if h.len() != 8 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    let (r, g, b, a) = (v >> 24, (v >> 16) & 255, (v >> 8) & 255, v & 255);
    Some(r | g << 8 | b << 16 | a << 24)
}

fn px(s: &str) -> f32 {
    s.trim_end_matches("px").parse().unwrap_or(0.0)
}

fn scale_alpha(c: u32, k: f32) -> u32 {
    let a = ((c >> 24) as f32 * k).round().clamp(0.0, 255.0) as u32;
    (c & 0x00ff_ffff) | a << 24
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let mut out = generate("gallery_s13.tsv", "GALLERY", gallery_tag);
    out.push_str(&generate("shell.tsv", "SHELL", shell_tag));
    std::fs::write(format!("{}/gallery_nodes.rs", std::env::var("OUT_DIR").unwrap()), out).unwrap();
}

/// Gallery hooks: the four probe swatches (100..), the text field the 22nd Tab reaches (2), the running
/// animations (3 spinner, 4 skeleton, 5 progress slide, 6 OTP caret).
fn gallery_tag(g: &dyn Fn(&str) -> String, st: &mut (u16, bool)) -> u16 {
    if g("classes").split(' ').any(|c| c == "swatch") {
        st.0 += 1;
        return 100 + st.0 - 1;
    }
    if g("tag") == "field" && !st.1 && g("tab_stop") == "21" {
        st.1 = true;
        return 2;
    }
    match g("animation_name").as_str() {
        "zui-spinner-turn" => 3,
        "zui-skeleton-pulse" => 4,
        "zui-progress-slide" => 5,
        "zui-otp-caret-blink" => 6,
        _ => 0,
    }
}

/// Shell hooks: tooltip triggers (10 + n), page rows (40), the page panel (50), the dock handle (51), the
/// dock panel with the log (52).
fn shell_tag(g: &dyn Fn(&str) -> String, st: &mut (u16, bool)) -> u16 {
    let classes = g("classes");
    let has = |c: &str| classes.split(' ').any(|x| x == c);
    if has("zui-tooltip__trigger") {
        st.0 += 1;
        return 10 + st.0 - 1;
    }
    if has("shell-row") {
        return 40;
    }
    if has("zui-resizable__panel") {
        let t = if st.1 { 52 } else { 50 };
        st.1 = true;
        return t;
    }
    if g("testid") == "dock-handle" {
        return 51;
    }
    0
}

type Tagger = fn(&dyn Fn(&str) -> String, &mut (u16, bool)) -> u16;

fn generate(file: &str, name: &str, tagger: Tagger) -> String {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let path = format!("{dir}/../../bench/results/input/zgui/dump/{file}");
    println!("cargo:rerun-if-changed={path}");
    let data = std::fs::read_to_string(&path).expect("gallery dump");
    let mut lines = data.lines();
    let head: Vec<&str> = lines.next().unwrap().split('\t').collect();
    let col: HashMap<&str, usize> = head.iter().enumerate().map(|(i, h)| (*h, i)).collect();
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split('\t').collect()).collect();
    let g = |r: &Vec<&str>, c: &str| -> String { r.get(col[c]).copied().unwrap_or("").to_string() };

    let mut out = String::new();
    writeln!(out, "pub static {name}: &[TNode] = &[").unwrap();
    let mut st = (0u16, false);
    // parents by depth (for text nodes, whose fragment box is often 0x0: the parent's line box)
    let mut parents: Vec<[f32; 4]> = Vec::new();
    let mut n = 0usize;
    for r in &rows {
        let pre: usize = g(r, "preorder").parse().unwrap();
        let depth: usize = g(r, "depth").parse().unwrap();
        if pre == 0 {
            parents.truncate(0);
            parents.push([0.0, 0.0, 1600.0, 1000.0]);
            continue; // the root is the scroll view's port
        }
        let rect = if g(r, "x").is_empty() {
            [0.0; 4]
        } else {
            [px(&g(r, "x")), px(&g(r, "y")), px(&g(r, "w")), px(&g(r, "h"))]
        };
        parents.truncate(depth);
        while parents.len() < depth {
            parents.push(parents.last().copied().unwrap_or([0.0; 4]));
        }
        let parent = parents.last().copied().unwrap_or([0.0; 4]);
        parents.push(rect);
        let d = depth - 1;
        let kind = g(r, "kind");
        let display = g(r, "display");
        let mut flags = 0u16;
        let tag;
        let (mut bg, mut bw, mut bc, mut radius) = (0u32, 0.0f32, 0u32, 0.0f32);
        let (mut text, mut font, mut lh, mut fg) = (String::new(), 0.0f32, 0.0f32, 0u32);
        let mut rect = rect;
        if kind == "text" {
            text = g(r, "text");
            font = px(&g(r, "font_size")).max(1.0);
            let l = g(r, "line_height");
            lh = if l.ends_with("px") { px(&l) } else { (font * 1.4).ceil() };
            fg = color(&g(r, "color")).unwrap_or(0xff00_0000);
            if rect[2] <= 0.0 {
                rect = [parent[0], parent[1], parent[2], lh];
            }
            if text.is_empty() {
                // an empty run (an empty field's value): a paint-less marker
                font = 0.0;
            }
        } else if display != "none" && display != "contents" {
            let opacity: f32 = g(r, "opacity").parse().unwrap_or(1.0);
            bg = scale_alpha(color(&g(r, "bg_color")).unwrap_or(0), opacity);
            if g(r, "bg_image") != "none" && !g(r, "bg_image").is_empty() && bg >> 24 == 0 {
                bg = scale_alpha(0xffff_8b7d, opacity); // a gradient: its middle stop, solid
            }
            if g(r, "tag") == "vector" {
                // an SVG icon: drawn as a tinted square of its current colour (no path rendering)
                bg = scale_alpha(color(&g(r, "color")).unwrap_or(0xff00_0000), 0.25 * opacity);
            }
            let ws = [px(&g(r, "border_w_t")), px(&g(r, "border_w_r")), px(&g(r, "border_w_b")), px(&g(r, "border_w_l"))];
            if ws[0] > 0.0 && ws.iter().all(|&w| w == ws[0]) {
                bw = ws[0];
                bc = scale_alpha(color(&g(r, "border_c_t")).unwrap_or(0), opacity);
            }
            radius = px(&g(r, "radius_tl")).min(rect[2].min(rect[3]) * 0.5);
            let ov = |c: &str| matches!(g(r, c).as_str(), "hidden" | "auto" | "scroll" | "clip");
            if ov("overflow_x") || ov("overflow_y") {
                flags |= 2; // T_CLIP
            }
        }
        if !g(r, "tab_stop").is_empty() {
            flags |= 1; // T_FOCUS
        }
        let gr = |c: &str| g(r, c);
        tag = tagger(&gr, &mut st);
        if tag != 0 {
            flags |= 4; // T_HOOK
        }
        writeln!(
            out,
            "    TNode {{ depth: {d}, rect: [{:?}, {:?}, {:?}, {:?}], bg: {bg:#x}, border_w: {bw:?}, border: {bc:#x}, radius: {radius:?}, text: {text:?}, font: {font:?}, line_h: {lh:?}, color: {fg:#x}, flags: {flags}, tag: {tag} }}, // {pre}",
            rect[0], rect[1], rect[2], rect[3]
        )
        .unwrap();
        n += 1;
    }
    writeln!(out, "];").unwrap();
    writeln!(out, "pub const {name}_NODES: usize = {n};").unwrap();
    out
}
