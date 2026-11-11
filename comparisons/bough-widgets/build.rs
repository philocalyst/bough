//! Generates `Values` (512 `Signal<String>` fields) and `with_fields!`: bough's `text_field` edits the
//! *root owner's* signal at a compiler-proven field offset (`sig!`), so one field per row needs one named
//! field per row (an array element or an entity's signal cannot be named by `sig!`, and a field's edits are
//! applied to the root owner at that offset: an entity's signal is not reachable).
use std::fmt::Write;

const N: usize = 512;

fn main() {
    let mut s = String::new();
    writeln!(s, "pub const MAX_ROWS: usize = {N};").unwrap();
    writeln!(s, "/// One `Signal<String>` per row (zgx: `ui.signal(String::new())`).").unwrap();
    writeln!(s, "pub struct Values {{").unwrap();
    for i in 0..N {
        writeln!(s, "    pub v{i}: Signal<String>,").unwrap();
    }
    writeln!(s, "}}\nimpl Values {{\n    pub fn new() -> Values {{\n        Values {{").unwrap();
    for i in 0..N {
        writeln!(s, "            v{i}: Signal::new(String::new()),").unwrap();
    }
    writeln!(s, "        }}\n    }}\n    pub fn get(&self, i: usize) -> &Signal<String> {{\n        match i {{").unwrap();
    for i in 0..N {
        writeln!(s, "            {i} => &self.v{i},").unwrap();
    }
    writeln!(s, "            _ => panic!(\"row {{i}} of at most {N}\"),\n        }}\n    }}\n}}").unwrap();
    writeln!(s, "impl Default for Values {{\n    fn default() -> Self {{\n        Self::new()\n    }}\n}}").unwrap();
    // `with_fields!(Owner, &owner, n, |i, w| body)`: body for every row i < n with w = sig!(owner.values.v{i})
    writeln!(s, "macro_rules! with_fields {{\n    ($owner:ty, $v:expr, $n:expr, |$i:ident, $w:ident| $body:expr) => {{{{\n        let v: &$owner = $v;\n        let n: usize = $n;").unwrap();
    for i in 0..N {
        writeln!(s, "        if {i} < n {{\n            let $i = {i}usize;\n            let $w = sig!(v: $owner => values.v{i});\n            $body;\n        }}").unwrap();
    }
    writeln!(s, "    }}}};\n}}").unwrap();
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("values.rs");
    std::fs::write(out, s).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
