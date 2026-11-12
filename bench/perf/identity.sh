#!/usr/bin/env bash
# identity.sh [BINARY] : the identity record every result file starts with (MEASUREMENT.md). One `# key=value`
# line per fact: host, OS, CPU, frequency policy, load, checkout commit (+dirty), toolchain, and the binary's
# hash, size and build profile (from its path) when one is given.
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
[ -d "$HOME/.local/share/cargo/bin" ] && PATH="$HOME/.local/share/cargo/bin:$PATH"
c=$(git -C "$ROOT" rev-parse --short=10 HEAD 2>/dev/null)
d=$(git -C "$ROOT" status --porcelain --untracked-files=no 2>/dev/null | head -1)
echo "# host=$(hostname -s) os=$(uname -sr) arch=$(uname -m)"
if [ "$(uname)" = Darwin ]; then
  echo "# cpu=$(sysctl -n machdep.cpu.brand_string) cores=$(sysctl -n hw.ncpu) mem=$(( $(sysctl -n hw.memsize) / 1073741824 ))GiB"
  echo "# load=$(sysctl -n vm.loadavg | tr -d '{}' | xargs)"
else
  echo "# cpu=$(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2 | xargs) threads=$(nproc) mem=$(( $(grep MemTotal /proc/meminfo | awk '{print $2}') / 1048576 ))GiB"
  echo "# governor=$(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor 2>/dev/null) boost=$(cat /sys/devices/system/cpu/cpufreq/boost 2>/dev/null) nmi_watchdog=$(cat /proc/sys/kernel/nmi_watchdog) perf_event_paranoid=$(cat /proc/sys/kernel/perf_event_paranoid)"
  echo "# load=$(cut -d' ' -f1-3 /proc/loadavg)"
fi
echo "# commit=$c${d:+ dirty} date=$(date -u +%Y-%m-%dT%H:%M:%SZ) rustc=$(cd "$ROOT" && rustc -V 2>/dev/null | cut -d' ' -f2-4 | tr -d '()' | tr ' ' _)"
if [ -n "$1" ] && [ -f "$1" ]; then
  case $1 in */dist/*) p=dist ;; */release/*) p=release ;; */debug/*) p=debug ;; *) p=unknown ;; esac
  h=$( (sha256sum "$1" 2>/dev/null || shasum -a 256 "$1") | cut -c1-16)
  echo "# binary=$(basename "$1") profile=$p sha256=$h bytes=$(wc -c < "$1" | xargs)"
fi
