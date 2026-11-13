# links.sh MODE : point gb/apps/<fw>/target at a set of binaries. MODE=zortax (the eleven zortax builds as cmp-1;
# bough = plain 74e665786c app) or MODE=scenes (iced, egui, gpui-kit = the bench lane's scene-enabled builds;
# bough = the scenes app). Symlinks only: nothing under /root/work/zortax or /root/work/r7 is written.
B=/root/work/r8/bench; GB=$B/gb
Z=/root/work/zortax/gui-bench/apps; S=/root/work/r7/bench/gb/apps
for fw in dioxus egui floem freya freya-056a5b4 gpui-kit gpui-kit-fast iced vizia zgui; do
  rm -rf $GB/apps/$fw/target; mkdir -p $GB/apps/$fw
  src=$Z/$fw/target
  if [ "$1" = scenes ]; then case $fw in iced|egui|gpui-kit) src=$S/$fw/target;; esac; fi
  ln -s $src $GB/apps/$fw/target
done
rm -f $GB/apps/bough/target
if [ "$1" = scenes ]; then ln -s $B/bin/scenes $GB/apps/bough/target; else ln -s $B/bin/plain $GB/apps/bough/target; fi
ls -la $GB/apps/*/target | awk '{print $9, $10, $11}'
