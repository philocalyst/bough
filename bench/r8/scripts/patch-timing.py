# patch-timing.py : the smallest local patch to gui-bench's runner for round 8's unthrottled runs: the environment variable
# GB_TIMING (fixed | unthrottled | vrr:MIN-MAX, default fixed) sets the timing model of the session's output
# (OutputConfig::timing, which fakeland already provides). With GB_TIMING unset the runner behaves as before.
import re, sys
p = "/root/work/r8/bench/gb/runner/src/cell.rs"
s = open(p).read()
if "GB_TIMING" in s:
    print("already patched"); sys.exit()
old = '''    let session = Fakeland::builder()
        .output(OutputConfig::new("DP-1").mode(OUTPUT.0, OUTPUT.1, Hz(cell.hz)))'''
new = '''    let mut output = OutputConfig::new("DP-1").mode(OUTPUT.0, OUTPUT.1, Hz(cell.hz));
    // round 8 local patch: GB_TIMING=fixed|unthrottled|vrr:MIN-MAX selects fakeland's timing model (default fixed)
    match std::env::var("GB_TIMING").ok().as_deref() {
        None | Some("") | Some("fixed") => {}
        Some("unthrottled") => output = output.timing(fakeland::TimingConfig::unthrottled()),
        Some(v) if v.starts_with("vrr:") => {
            let (lo, hi) = v[4..].split_once('-').context("GB_TIMING=vrr:MIN-MAX")?;
            output = output.timing(fakeland::TimingConfig::vrr(Hz(lo.parse()?), Hz(hi.parse()?)));
        }
        Some(other) => bail!("GB_TIMING={other}: fixed, unthrottled or vrr:MIN-MAX"),
    }
    let session = Fakeland::builder()
        .output(output)'''
assert old in s
open(p, "w").write(s.replace(old, new))
print("patched", p)
