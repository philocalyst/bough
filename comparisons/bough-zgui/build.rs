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
    out.push_str(&generate_layout("gallery_s13.tsv", "GALLERY_L", gallery_tag, false));
    out.push_str(&generate_layout("shell.tsv", "SHELL_L", shell_tag, true));
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

/// The layout template of a dumped document: the same DOM preorder, each element's computed layout
/// declarations (CSS values as zgui's cascade resolved them; the coordinates are not used), its paint, and
/// the text nodes as measured leaves. Distinct declaration lists are emitted once (`{name}_DECLS`).
fn generate_layout(file: &str, name: &str, tagger: Tagger, fixed_root: bool) -> String {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let path = format!("{dir}/../../bench/results/input/zgui/dump/{file}");
    let data = std::fs::read_to_string(&path).expect("dump");
    let mut lines = data.lines();
    let head: Vec<&str> = lines.next().unwrap().split('\t').collect();
    let col: HashMap<&str, usize> = head.iter().enumerate().map(|(i, h)| (*h, i)).collect();
    let rows: Vec<Vec<&str>> = lines.map(|l| l.split('\t').collect()).collect();
    let g = |r: &Vec<&str>, c: &str| -> String { r.get(col[c]).copied().unwrap_or("").to_string() };
    let other = |r: &Vec<&str>, k: &str| -> Option<String> {
        g(r, "other_non_initial").split(';').find_map(|kv| kv.strip_prefix(k).and_then(|v| v.strip_prefix(':')).map(|v| v.to_string()))
    };
    let mut decls: Vec<String> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut out = String::new();
    writeln!(out, "pub static {name}: &[LNode] = &[").unwrap();
    let mut st = (0u16, false);
    // inherited: nowrap by depth
    let mut nowrap: Vec<bool> = Vec::new();
    let mut n = 0usize;
    for r in &rows {
        let pre: usize = g(r, "preorder").parse().unwrap();
        let depth: usize = g(r, "depth").parse().unwrap();
        nowrap.truncate(depth);
        let inherited_nowrap = nowrap.last().copied().unwrap_or(false);
        let kind = g(r, "kind");
        let mut flags = 0u16;
        let (mut bg, mut bw, mut bc, mut radius, mut hover) = (0u32, 0.0f32, 0u32, 0.0f32, 0u32);
        let (mut text, mut font, mut lh, mut fg, mut face) = (String::new(), 0.0f32, 0.0f32, 0u32, 0u8);
        let mut style = 0usize;
        if kind == "text" {
            flags |= 8; // L_TEXT
            text = g(r, "text");
            font = px(&g(r, "font_size")).max(1.0);
            let l = g(r, "line_height");
            lh = if l.ends_with("px") { px(&l) } else { (font * 1.4).ceil() };
            fg = color(&g(r, "color")).unwrap_or(0xff00_0000);
            let weight: u32 = g(r, "font_weight").parse().unwrap_or(400);
            face = if g(r, "font_family").contains("ono") { 2 } else if weight >= 600 { 1 } else { 0 };
            if inherited_nowrap {
                flags |= 16; // L_NOWRAP
            }
            nowrap.push(inherited_nowrap);
        } else {
            let el_nowrap = other(r, "text-wrap-mode").map_or(inherited_nowrap, |v| v == "nowrap");
            nowrap.push(el_nowrap);
            let mut display = g(r, "display");
            match display.as_str() {
                "contents" => flags |= 32,
                "none" => flags |= 64,
                "inline" | "inline-block" | "" => display = "block".into(),
                "inline-flex" => display = "flex".into(),
                _ => {}
            }
            let mut d: Vec<(&str, String)> = Vec::new();
            let mut put = |p: &'static str, v: String, initial: &[&str]| {
                if !v.is_empty() && !initial.contains(&v.as_str()) {
                    d.push((p, v));
                }
            };
            put("Display", display.clone(), &[]);
            let pos = match g(r, "position").as_str() {
                "sticky" => "relative".to_string(),
                "fixed" => "absolute".to_string(),
                p => p.to_string(),
            };
            put("Position", pos, &["static"]);
            put("FlexDirection", g(r, "flex_direction"), &["row"]);
            put("FlexWrap", g(r, "flex_wrap"), &["nowrap"]);
            put("FlexGrow", g(r, "flex_grow"), &["0"]);
            put("FlexShrink", g(r, "flex_shrink"), &["1"]);
            put("FlexBasis", g(r, "flex_basis"), &["auto"]);
            let root = pre == 0;
            if root {
                // the template root is the root element's content: the port (or the window) gives its width
                if fixed_root {
                    put("Width", "100%".into(), &[]);
                    put("Height", "100%".into(), &[]);
                }
            } else {
                put("Width", g(r, "width"), &["auto"]);
                put("Height", g(r, "height"), &["auto"]);
            }
            put("MinWidth", g(r, "min_width"), &["auto"]);
            put("MinHeight", g(r, "min_height"), &["auto"]);
            put("MaxWidth", g(r, "max_width"), &["none"]);
            put("MaxHeight", g(r, "max_height"), &["none"]);
            for (p, c) in [("PadT", "padding_t"), ("PadR", "padding_r"), ("PadB", "padding_b"), ("PadL", "padding_l"), ("MarT", "margin_t"), ("MarR", "margin_r"), ("MarB", "margin_b"), ("MarL", "margin_l"), ("BorT", "border_w_t"), ("BorR", "border_w_r"), ("BorB", "border_w_b"), ("BorL", "border_w_l")] {
                put(p, g(r, c), &["0px"]);
            }
            put("RowGap", g(r, "row_gap"), &["normal", "0px"]);
            put("ColGap", g(r, "column_gap"), &["normal", "0px"]);
            put("AlignItems", g(r, "align_items"), &["normal"]);
            put("AlignSelf", g(r, "align_self"), &["auto"]);
            put("JustifyContent", g(r, "justify_content"), &["normal"]);
            if let Some(v) = other(r, "justify-items") {
                put("JustifyItems", v, &["normal", "legacy"]);
            }
            if !root {
                put("OverflowX", g(r, "overflow_x"), &["visible"]);
                put("OverflowY", g(r, "overflow_y"), &["visible"]);
            }
            put("GridTemplateColumns", g(r, "grid_template_columns"), &["none"]);
            if let Some(v) = other(r, "grid-template-rows") {
                put("GridTemplateRows", v, &["none"]);
            }
            if let Some(v) = other(r, "grid-auto-rows") {
                put("GridAutoRows", v, &["auto"]);
            }
            put("GridColumnStart", g(r, "grid_column_start"), &["auto"]);
            put("GridColumnEnd", g(r, "grid_column_end"), &["auto"]);
            for (p, k) in [("GridRowStart", "grid-row-start"), ("GridRowEnd", "grid-row-end"), ("Top", "top"), ("Right", "right"), ("Bottom", "bottom"), ("Left", "left"), ("AspectRatio", "aspect-ratio")] {
                if let Some(v) = other(r, k) {
                    put(p, v, &["auto"]);
                }
            }
            put("BoxSizing", g(r, "box_sizing"), &["border-box"]);
            let key: String = d.iter().map(|(p, v)| format!("(P::{p}, {v:?})")).collect::<Vec<_>>().join(", ");
            style = *index.entry(key.clone()).or_insert_with(|| {
                decls.push(key);
                decls.len() - 1
            });
            // paint
            let opacity: f32 = g(r, "opacity").parse().unwrap_or(1.0);
            if other(r, "visibility").as_deref() == Some("hidden") {
                flags |= 128;
            }
            bg = scale_alpha(color(&g(r, "bg_color")).unwrap_or(0), opacity);
            if g(r, "bg_image") != "none" && !g(r, "bg_image").is_empty() && bg >> 24 == 0 {
                bg = scale_alpha(0xffff_8b7d, opacity);
            }
            if g(r, "tag") == "vector" {
                bg = scale_alpha(color(&g(r, "color")).unwrap_or(0xff00_0000), 0.25 * opacity);
            }
            let ws = [px(&g(r, "border_w_t")), px(&g(r, "border_w_r")), px(&g(r, "border_w_b")), px(&g(r, "border_w_l"))];
            if ws[0] > 0.0 && ws.iter().all(|&w| w == ws[0]) {
                bw = ws[0];
                bc = scale_alpha(color(&g(r, "border_c_t")).unwrap_or(0), opacity);
            }
            radius = px(&g(r, "radius_tl")).min(64.0);
            let ov = |c: &str| matches!(g(r, c).as_str(), "hidden" | "auto" | "scroll" | "clip");
            if !root && (ov("overflow_x") || ov("overflow_y")) {
                flags |= 2; // L_CLIP
            }
            if g(r, "classes").split(' ').any(|c| c == "shell-row") {
                hover = 0xff26_2626;
            }
        }
        if !g(r, "tab_stop").is_empty() {
            flags |= 1; // L_FOCUS
        }
        let gr = |c: &str| g(r, c);
        let tag = tagger(&gr, &mut st);
        if tag != 0 && tag != 40 {
            flags |= 4; // L_HOOK
        }
        writeln!(
            out,
            "    LNode {{ depth: {depth}, style: {style}, size: [{:?}, {:?}], bg: {bg:#x}, border_w: {bw:?}, border: {bc:#x}, radius: {radius:?}, hover: {hover:#x}, text: {text:?}, font: {font:?}, line_h: {lh:?}, color: {fg:#x}, face: {face}, flags: {flags}, tag: {tag} }}, // {pre}",
            px(&g(r, "w")),
            px(&g(r, "h"))
        )
        .unwrap();
        n += 1;
    }
    writeln!(out, "];").unwrap();
    writeln!(out, "pub const {name}_NODES: usize = {n};").unwrap();
    writeln!(out, "pub static {name}_DECLS: &[Decls] = &[").unwrap();
    for d in &decls {
        writeln!(out, "    &[{d}],").unwrap();
    }
    writeln!(out, "];").unwrap();
    out
}
