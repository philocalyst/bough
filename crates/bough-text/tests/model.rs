//! Round 3b: the writer's delta (`splice`, `push_str`) and the external value (`set_text` of the resulting string)
//! give the same text, selection and undo/redo history, over random edits (ASCII, Latin-1, CJK; inserts, deletes,
//! replacements, appends) with history limits in force.
use bough_text::model::TextModel;

#[test]
fn delta_equals_external_value() {
    let pieces = ["a", " 好", "é", "xyz", "", "fi", "\n", "ü好b"];
    let mut seed = 0x1234_5678u64;
    let mut rnd = move |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n.max(1) as u64) as usize
    };
    let mut a = TextModel::new("hello world".into());
    let mut b = TextModel::new("hello world".into());
    a.set_history_limits(20, 400);
    b.set_history_limits(20, 400);
    for step in 0..20_000 {
        let t = a.text().to_string();
        let bounds: Vec<usize> = (0..=t.len()).filter(|&i| t.is_char_boundary(i)).collect();
        match rnd(5) {
            0 | 1 => {
                let (i, j) = (bounds[rnd(bounds.len())], bounds[rnd(bounds.len())]);
                let (i, j) = (i.min(j), i.max(j));
                let ins = pieces[rnd(pieces.len())];
                let mut s = t.clone();
                s.replace_range(i..j, ins);
                // (an ambiguous edit, e.g. inside a repeated run, may be placed elsewhere by the external diff;
                // texts and undo/redo results must agree either way)
                let ra = a.splice(i..j, ins);
                assert_eq!(ra, b.set_text(s), "step {step}");
            }
            2 => {
                let ins = pieces[rnd(pieces.len())];
                let s = format!("{t}{ins}");
                a.push_str(ins);
                b.set_text(s);
            }
            3 => {
                assert_eq!(a.undo(), b.undo(), "undo at step {step}");
            }
            _ => {
                assert_eq!(a.redo(), b.redo(), "redo at step {step}");
            }
        }
        assert_eq!(a.text(), b.text(), "step {step}");
        if a.text().len() > 300 {
            let cut = a.text().char_indices().nth(40).map_or(0, |c| c.0);
            a.splice(0..cut, "");
            let s = b.text()[cut..].to_string();
            b.set_text(s);
        }
    }
}
