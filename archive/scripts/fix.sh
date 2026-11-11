#!/bin/bash
# In-place patch of /root/.gpui-archive: redo WIP snapshots with a work tree set, import the missing hummingbird objects.
# Reads source worktrees only; writes only under /root/.gpui-archive.
set -u
export ARCH=/root/.gpui-archive COPY=/root/.gpui-archive/gpui-rework.git
g() { git --git-dir="$COPY" -c 'safe.directory=*' "$@"; }
export -f g
export GIT_AUTHOR_NAME="gpui archive" GIT_AUTHOR_EMAIL="archive@localhost" GIT_COMMITTER_NAME="gpui archive" GIT_COMMITTER_EMAIL="archive@localhost"

cp -aln /root/gpui-hummingbird-files-sol/.git/objects/. "$COPY/objects/" 2>/dev/null
cp -aln /root/gpui-hummingbird-fresh-schema-sol/.git/objects/. "$COPY/objects/" 2>/dev/null

snap() {
  d=$1; src=/root/$d; idx=$ARCH/index/$d.idx; list=$ARCH/index/$d.list
  sha=$(git -c 'safe.directory=*' -C "$src" rev-parse HEAD 2>/dev/null) || { echo "NOHEAD $d"; return; }
  g cat-file -e "$sha" 2>/dev/null || { echo "MISSING $d $sha"; return; }
  g update-ref "refs/heads/wt/$d" "$sha" || { echo "REFFAIL $d"; return; }
  git -c 'safe.directory=*' -C "$src" ls-files -m -o --exclude-standard -z > "$list"
  [ -s "$list" ] || { printf '%s\t%s\t-\t0\t0\t0\n' "$d" "$sha" > "$ARCH/index/$d.res"; echo "CLEAN $d"; return; }
  # GitHub rejects blobs over 100 MB: leave those out of the snapshot and record them.
  ( cd "$src" && xargs -0 -a "$list" stat -c '%s %n' 2>/dev/null | awk '$1>95000000' ) > "$ARCH/index/$d.big"
  if [ -s "$ARCH/index/$d.big" ]; then
    cut -d' ' -f2- "$ARCH/index/$d.big" > "$ARCH/index/$d.big.names"
    tr '\0' '\n' < "$list" | grep -vxFf "$ARCH/index/$d.big.names" | tr '\n' '\0' > "$list.f"; mv "$list.f" "$list"
    echo "BIG $d $(wc -l < "$ARCH/index/$d.big") file(s) excluded"
  fi
  rm -f "$idx"
  GIT_INDEX_FILE=$idx g read-tree "$sha" || { echo "READTREE_FAIL $d"; return; }
  ( cd "$src" && GIT_DIR=$COPY GIT_WORK_TREE=$src GIT_INDEX_FILE=$idx git -c 'safe.directory=*' update-index --add --remove -z --stdin < "$list" ) || echo "UPDATEINDEX_FAIL $d"
  tree=$(GIT_INDEX_FILE=$idx g write-tree)
  commit=$(g commit-tree "$tree" -p "$sha" -m "WIP snapshot of $d: uncommitted and untracked (non-ignored) files at archive time")
  g update-ref "refs/heads/wt/$d-wip" "$commit"
  n=$(tr -cd '\0' < "$list" | wc -c)
  ch=$(g diff-tree -r --no-commit-id --name-only "$sha" "$commit" | wc -l)
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$d" "$sha" "$commit" "$n" "$ch" "$(wc -l < "$ARCH/index/$d.big")" > "$ARCH/index/$d.res"
  echo "WIP $d listed=$n changed_vs_head=$ch -> $commit"
}
export -f snap

echo "== snapshots"
awk -F, 'NR>1 && ($2=="worktree" || $2=="repo"){print $1}' /root/.inventory.csv | xargs -P 8 -I{} bash -c 'snap "$1"' _ {}

echo "== stashes"
i=0
while g rev-parse -q --verify "stash@{$i}" >/dev/null 2>&1; do
  g update-ref "refs/heads/stash/$i" "$(g rev-parse "stash@{$i}")"; i=$((i+1))
done
echo "stash count $i"

echo "== manifest"
# dir, kind, branch, inventory head, modified, untracked, wt ref, wip commit, listed, changed, excluded_big
{
  printf 'dir\tkind\tbranch\tinventory_head\tmodified\tuntracked\twt_ref_sha\twip_commit\twip_listed\twip_changed\twip_big_excluded\n'
  awk -F, -v OFS='\t' -v idx="$ARCH/index" 'NR>1 && ($2=="worktree"||$2=="repo"){
      res=""; f=idx "/" $1 ".res"; if ((getline line < f) > 0) res=line; close(f)
      split(res, a, "\t"); print $1,$2,$3,$4,$5,$6,(a[2]==""?"-":a[2]),(a[3]==""?"-":a[3]),(a[4]==""?0:a[4]),(a[5]==""?0:a[5]),(a[6]==""?0:a[6]) }' /root/.inventory.csv
} > "$ARCH/MANIFEST.tsv"
wc -l "$ARCH/MANIFEST.tsv"

echo "== large objects (>95MB) anywhere in archive"
g cat-file --batch-all-objects --batch-check='%(objectsize) %(objecttype) %(objectname)' | awk '$1>95000000' | head
echo "== ref namespaces"
g for-each-ref --format='%(refname)' | awk -F/ '{print $2}' | sort | uniq -c
echo "SNAP_DONE"
