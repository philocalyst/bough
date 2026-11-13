# bench-r8 (ilo-prod only). Source from bash. The zortax nix env provides the C toolchain, pkg-config, fontconfig,
# libxkbcommon, libwayland, lavapipe (read-only). Toolchain and cargo home are hard-linked copies of the platform lane's.
source /root/work/zortax/env/env.sh
R=/root/work/r8/bench
export RUSTUP_HOME=$R/rustup CARGO_HOME=$R/cargo RUSTC_WRAPPER= CARGO_BUILD_JOBS=8
unset CARGO_TARGET_DIR
export PATH=$CARGO_HOME/bin:$(ls -d /nix/store/*-rustup-1.29.3/bin | head -1):$PATH
export BOUGH_FONT=/root/work/zortax/env/result/share/fonts/truetype/DejaVuSans.ttf
export ZGX_FONT_FILE=$BOUGH_FONT
NP=$(cat $R/nixpaths.txt | tr '\n' ' ')
for p in $NP; do case $p in *-dejavu-fonts-*) DEJAVU=$p;; *-miscfiles-*) MISC=$p;; *-inter-*) INTER=$p;; esac; done
export BOUGH_DEJAVU_DIR=$DEJAVU/share/fonts/truetype BOUGH_BOLD_FONT=$DEJAVU/share/fonts/truetype/DejaVuSans-Bold.ttf BOUGH_WORDS=$MISC/share/web2
export BOUGH_VARIABLE_FONT=$INTER/share/fonts/truetype/InterVariable.ttf
export PERF=/nix/store/bw8frr0nk26m740a2mwaw5yi06mgxakb-perf-linux-7.3.1/bin/perf
build() { taskset -c 4-7,12-15 nice -n 10 "$@"; }
