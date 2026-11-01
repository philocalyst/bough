use bough_text::*;
use ttf_parser::{Face, GlyphId, Tag};
use ttf_parser::gsub::SubstitutionSubtable;
fn main() { lig_kerns();
    let data = zgx_workload::FONT_BYTES;
    let s = SimpleScript::build(data, 0).unwrap();
    // position-equivalence: same row (adv+kern vs every right neighbour incl pad) and same column (kern from every left).
    let cov: Vec<u8> = (0u8..=255).filter(|&c| s.covered.contains(c)).collect();
    let step = |a: u8, b: Option<u8>| -> i32 { let k = match b { Some(b) => s.pair(a,b), None => s.matrix[s.class1[a as usize] as usize * s.n2] }; if k == TRIG { i32::MIN } else { s.adv[a as usize] as i32 + k as i32 } };
    let mut classes: Vec<Vec<u8>> = vec![];
    'o: for &c in &cov {
        for cl in classes.iter_mut() {
            let r = cl[0];
            let same_row = cov.iter().all(|&n| step(c, Some(n)) == step(r, Some(n))) && step(c,None)==step(r,None);
            let same_col = cov.iter().all(|&p| step(p, Some(c)) == step(p, Some(r)));
            if same_row && same_col { cl.push(c); continue 'o; }
        }
        classes.push(vec![c]);
    }
    println!("{} position-equivalence classes over {} covered cps", classes.len(), cov.len());
    for cl in &classes { if cl.len() > 1 { println!("  [{}] adv {}", cl.iter().map(|&c| if c < 128 {(c as char).to_string()} else {format!("\\x{c:02x}")}).collect::<String>(), s.adv[cl[0] as usize]); } }
    // digits: who kerns with digits?
    let digits = b"0123456789";
    for &d in digits { let k: Vec<String> = cov.iter().filter(|&&n| s.pair(d,n)!=0).map(|&n| format!("{}{}:{}", d as char, n as char, s.pair(d,n))).collect(); if !k.is_empty() {println!("digit row kerns {:?}", k);} }
    for &d in digits { let k: Vec<String> = cov.iter().filter(|&&p| s.pair(p,d)!=0).map(|&p| format!("{}{}:{}", p as char, d as char, s.pair(p,d))).collect(); if !k.is_empty() {println!("digit col kerns {:?}", k);} }
    println!("digit advs {:?}", digits.iter().map(|&d| s.adv[d as usize]).collect::<Vec<_>>());
    // ligature lookup 18
    let face = Face::parse(data, 0).unwrap();
    let gsub = face.tables().gsub.unwrap();
    for li in [3u16, 4, 18, 1] {
        let l = gsub.lookups.get(li).unwrap();
        println!("GSUB lookup {li}: type? subtables {} flags {:#x}", l.subtables.len(), l.flags.0);
        for si in 0..l.subtables.len() {
            if let Some(SubstitutionSubtable::Ligature(lg)) = l.subtables.get::<SubstitutionSubtable>(si) {
                for c in 0..=255u8 { if !s.covered.contains(c) {continue;} let g = GlyphId(s.gid[c as usize]); if let Some(ci) = lg.coverage.get(g) { let set = lg.ligature_sets.get(ci).unwrap(); for k in 0..set.len() { let lig = set.get(k).unwrap(); let comps: Vec<u16> = lig.components.into_iter().map(|x| x.0).collect(); let names: Vec<String> = comps.iter().map(|&x| (0..=255u8).find(|&cc| s.gid[cc as usize]==x && s.covered.contains(cc)).map(|cc| (cc as char).to_string()).unwrap_or(format!("g{x}"))).collect(); println!("   lig: {} + {:?} -> gid {} (adv {:?})", c as char, names, lig.glyph.0, face.glyph_hor_advance(lig.glyph)); } } }
            }
        }
    }
    let gpos = face.tables().gpos.unwrap();
    for li in [4u16, 12, 13, 14, 15] { let l = gpos.lookups.get(li).unwrap(); println!("GPOS lookup {li}: kind {:?} subtables {} flags {:#x}", l.subtables.get::<ttf_parser::gpos::PositioningSubtable>(0).map(|x| match x { ttf_parser::gpos::PositioningSubtable::Pair(_) => "pair", ttf_parser::gpos::PositioningSubtable::MarkToBase(_) => "mark2base", ttf_parser::gpos::PositioningSubtable::MarkToMark(_) => "mark2mark", ttf_parser::gpos::PositioningSubtable::MarkToLigature(_) => "mark2lig", _ => "other"}), l.subtables.len(), l.flags.0); }
    let _ = Tag::from_bytes(b"liga");
}
#[allow(dead_code)]
pub fn lig_kerns() {
    let s = SimpleScript::build(zgx_workload::FONT_BYTES, 0).unwrap();
    let t = s.tokens.as_ref().unwrap();
    let mut n = 0;
    for &(_, _, lt) in &t.ligs {
        for u in 0..256usize {
            let st = t.step[(lt as usize) << 8 | u];
            let st0 = t.step[(lt as usize) << 8];
            if st != 0xFFFF && st != st0 { n += 1; if n < 20 { println!("lig tok {lt} then {:?}: step {st} vs adv {st0}", u as u8 as char); } }
        }
        for a in 0..256usize {
            let st = t.step[a << 8 | lt as usize];
            let st0 = t.step[a << 8];
            if st != 0xFFFF && st != st0 && a != 1 { n += 1; if n < 40 { println!("{:?} then lig tok {lt}: step {st} vs adv {st0}", a as u8 as char); } }
        }
    }
    println!("{n} nonzero kerning pairs involve a ligature glyph");
}
