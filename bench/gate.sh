#!/usr/bin/env bash
# The one-command gate every lane runs before it stops (BRIEF.md 9.2, CONTRIBUTING.md, ROUND6.md rule 8), and what
# .github/workflows/bough.yml runs. From anywhere inside a checkout:
#
#   bench/gate.sh                          # steps below against the merge base with $BOUGH_BASE (default: main)
#   bench/gate.sh --base lead/round6-base  # another base ref
#   bench/gate.sh --all                    # every workspace crate's tests, not only the changed ones
#   bench/gate.sh --steps check,test       # a subset; a step named here runs unconditionally
#   bench/gate.sh --dry-run                # print the plan (changed files, crates, steps), run nothing
#   bench/gate.sh --list-changed           # print the changed crates, one per line
#
# Steps, in order (default: hygiene check test sabotage; clippy is opt-in):
#   hygiene   the diff adds no absolute machine paths (/Users/, /home/, /private/tmp/...) to a manifest, script or
#             source file
#   check     `cargo check --workspace --all-targets`; fails on any rustc/clippy warning. Build-script notices
#             (`warning: pkg@ver: ...`, e.g. "no Metal compiler") are printed but do not fail: they depend on the box.
#   clippy    `cargo clippy --workspace --all-targets --no-deps` (workspace members only, never vendor/); reports
#             only, because the tree is not clippy-clean yet (a failing clippy is a warning here, not a FAIL)
#   test      `cargo test --release -p <crate>...` for the crates with changed files (all with --all, or when a
#             root manifest, the lockfile, the toolchain file or .cargo/ changed). `--no-fail-fast`.
#   sabotage  only when damage or presenter files changed (or when named in --steps): the vlist pixel gates must pass
#             as they are and must FAIL with `--features bough/sabotage_copy` (BRIEF.md 9.2 item 4)
#
# Exit status: 0 only if every step that ran passed. A summary of every step is printed last.
# Environment: the same as ROUND6.md. This script sets PATH to rustup's proxies when ~/.local/share/cargo/bin exists,
# clears RUSTC_WRAPPER, defaults CARGO_BUILD_JOBS to 4 and CARGO_TARGET_DIR to <checkout>/target (never share one
# between worktrees). It refuses to run on a toolchain that is not the pinned nightly.
set -u

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT" || exit 2

BASE=${BOUGH_BASE:-main}
STEPS=""
ALL=0
DRY=0
LIST=0
while [ $# -gt 0 ]; do
  case $1 in
    --base) BASE=$2; shift 2 ;;
    --steps) STEPS=$2; shift 2 ;;
    --all) ALL=1; shift ;;
    --dry-run) DRY=1; shift ;;
    --list-changed) LIST=1; shift ;;
    -h|--help) sed -n '2,/^set -u/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "gate: unknown argument $1 (see --help)" >&2; exit 2 ;;
  esac
done
EXPLICIT=0
if [ -n "$STEPS" ]; then EXPLICIT=1; else STEPS="hygiene,check,test,sabotage"; fi
want() { case ",$STEPS," in *",$1,"*) return 0 ;; *) return 1 ;; esac; }

# ---- environment (ROUND6.md)
[ -d "$HOME/.local/share/cargo/bin" ] && export PATH="$HOME/.local/share/cargo/bin:$PATH"
export RUSTC_WRAPPER=
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-4}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$ROOT/target}

# ---- what changed relative to the base (committed, staged, unstaged and untracked)
if ! git rev-parse --verify -q "$BASE^{commit}" >/dev/null && git rev-parse --verify -q "origin/$BASE^{commit}" >/dev/null; then
  BASE=origin/$BASE   # a CI checkout has no local branch of that name
fi
if ! git rev-parse --verify -q "$BASE^{commit}" >/dev/null; then
  echo "gate: base ref '$BASE' not found (pass --base <ref> or set BOUGH_BASE)" >&2; exit 2
fi
MB=$(git merge-base "$BASE" HEAD) || { echo "gate: no merge base with $BASE" >&2; exit 2; }
CHANGED=$( { git diff --name-only "$MB"; git ls-files --others --exclude-standard; } | sort -u )

# the workspace package owning a file: the nearest Cargo.toml under crates/ or comparisons/ (vendor/, reactivity/,
# designs/, archive/ and bench/ manifests are not workspace members)
crate_of() {
  d=$(dirname "$1")
  case $d in crates/*|comparisons/*) ;; *) return 1 ;; esac
  while [ "$d" != "." ] && [ "$d" != "/" ]; do
    if [ -f "$d/Cargo.toml" ]; then
      awk '/^\[package\]/{p=1;next} /^\[/{p=0} p && /^name[ ]*=/{gsub(/[" ]/,"",$0); sub(/^name=/,"",$0); print; exit}' "$d/Cargo.toml"
      return 0
    fi
    d=$(dirname "$d")
  done
  return 1
}
CRATES=""
ROOT_CHANGED=0
for f in $CHANGED; do
  case $f in Cargo.toml|Cargo.lock|rust-toolchain.toml|clippy.toml|.cargo/*) ROOT_CHANGED=1 ;; esac
  c=$(crate_of "$f") || continue
  [ -n "$c" ] && CRATES="$CRATES
$c"
done
CRATES=$(printf '%s\n' "$CRATES" | sed '/^$/d' | sort -u)
if [ "$LIST" = 1 ]; then printf '%s\n' "$CRATES"; exit 0; fi

# damage or presenter files
SAB_FILES=$(printf '%s\n' "$CHANGED" | grep -E '^crates/(bough-damage-lab|bough-metal|bough-gpu)/|^crates/bough/src/(app|changed|backend/|layout|list|listview|mlist)|^crates/bough/Cargo.toml$' || true)
RUN_SAB=0
{ [ -n "$SAB_FILES" ] || { want sabotage && [ "$EXPLICIT" = 1 ]; }; } && RUN_SAB=1

if [ "$DRY" = 1 ]; then
  echo "base           $BASE (merge base $(git rev-parse --short "$MB"))"
  echo "changed files  $(printf '%s\n' "$CHANGED" | sed '/^$/d' | wc -l | tr -d ' ')"
  echo "changed crates $(printf '%s' "$CRATES" | tr '\n' ' ')"
  echo "all crates     $([ "$ALL" = 1 ] || [ "$ROOT_CHANGED" = 1 ] && echo yes || echo no)  (root manifest/lock/toolchain changed: $ROOT_CHANGED)"
  echo "steps          $STEPS"
  echo "sabotage       $([ "$RUN_SAB" = 1 ] && echo "would run" || echo "skipped (no damage or presenter file changed)")"
  [ -n "$SAB_FILES" ] && printf '                 %s\n' $SAB_FILES
  exit 0
fi

# ---- the toolchain must be the pinned nightly
V=$(cargo --version 2>&1)
PIN=$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml)
case $V in
  *nightly*) ;;
  *) echo "gate: '$V' is not the pinned $PIN. Put rustup's proxies first on PATH (ROUND6.md, Environment)." >&2; exit 2 ;;
esac
echo "gate: $V; target $CARGO_TARGET_DIR; base $BASE ($(git rev-parse --short "$MB")); steps $STEPS"

SUMMARY=""
FAILED=0
record() { SUMMARY="$SUMMARY
  $1  $2"; [ "$1" = FAIL ] && FAILED=1; return 0; }

# ---- hygiene
if want hygiene; then
  echo "== hygiene: absolute paths added since the merge base"
  BAD=$(git diff -U0 "$MB" -- '*.toml' '*.sh' '*.py' '*.rs' '*.yml' '*.yaml' ':!vendor' ':!reactivity' ':!designs' ':!archive' ':!bench/results' ':!bench/baselines' \
        | grep -E '^\+[^+]' | grep -E '/Users/|/home/[a-z]|/private/tmp/|/root/' || true)
  if [ -n "$BAD" ]; then printf '%s\n' "$BAD"; record FAIL "hygiene: added lines name an absolute machine path (use BOUGH_REF or a path relative to the checkout)"
  else record PASS "hygiene"; fi
fi

# ---- check
if want check; then
  echo "== check: cargo check --workspace --all-targets"
  LOG=$(mktemp -t bough-gate-check.XXXXXX)
  cargo check --workspace --all-targets 2>&1 | tee "$LOG"
  RC=${PIPESTATUS[0]}
  if [ "$RC" -ne 0 ]; then
    record FAIL "check: cargo check failed"
  else
    # a rustc warning is `warning: <text>`; a build-script notice is `warning: pkg@ver: <text>`
    W=$(grep -E '^warning: ' "$LOG" | grep -vE '^warning: [A-Za-z0-9_.-]+@[0-9][^ ]*: ' || true)
    NOTICES=$(grep -cE '^warning: [A-Za-z0-9_.-]+@[0-9][^ ]*: ' "$LOG" || true)
    if [ -n "$W" ]; then record FAIL "check: warnings (BRIEF 4.3 #8 wants zero): $(printf '%s\n' "$W" | wc -l | tr -d ' ') lines, first: $(printf '%s\n' "$W" | head -1)"
    else record PASS "check: zero warnings ($NOTICES build-script notices, not counted)"; fi
  fi
  rm -f "$LOG"
fi

# ---- clippy (report only)
if want clippy; then
  echo "== clippy: workspace members only (report)"
  if cargo clippy --workspace --all-targets --no-deps; then record PASS "clippy: clean"
  else record WARN "clippy: reported findings or errors (tree is not clippy-clean; not a gate yet)"; fi
fi

# ---- test
if want test; then
  if [ "$ALL" = 1 ] || [ "$ROOT_CHANGED" = 1 ]; then
    echo "== test: cargo test --release --workspace --no-fail-fast  (all crates; root manifest/lock/toolchain changed: $ROOT_CHANGED)"
    if cargo test --release --workspace --no-fail-fast; then record PASS "test: whole workspace"; else record FAIL "test: whole workspace"; fi
  elif [ -z "$CRATES" ]; then
    record SKIP "test: no workspace crate has a changed file"
  else
    ARGS=""; for c in $CRATES; do ARGS="$ARGS -p $c"; done
    echo "== test: cargo test --release --no-fail-fast$ARGS"
    # shellcheck disable=SC2086
    if cargo test --release --no-fail-fast $ARGS; then record PASS "test:$ARGS"; else record FAIL "test:$ARGS"; fi
  fi
fi

# ---- sabotage
if want sabotage; then
  if [ "$RUN_SAB" = 0 ]; then
    record SKIP "sabotage: no damage or presenter file changed"
  else
    echo "== sabotage: vlist pixel gates must pass, then fail with bough/sabotage_copy"
    if ! cargo test --release -p bough-vlist --test correctness; then
      record FAIL "sabotage: the vlist gates fail without sabotage, so the negative control proves nothing"
    elif cargo test --release -p bough-vlist --test correctness --features bough/sabotage_copy; then
      record FAIL "sabotage: the gates still PASS with sabotage_copy (they no longer detect a one-pixel-off copy)"
    else
      record PASS "sabotage: gates pass as-is and fail with sabotage_copy"
    fi
  fi
fi

echo
echo "gate summary (base $BASE):$SUMMARY"
[ "$FAILED" = 0 ] && echo "gate: PASS" || echo "gate: FAIL"
exit $FAILED
