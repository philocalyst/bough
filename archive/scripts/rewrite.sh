#!/bin/bash
# Replace the two >100 MB tarballs with text placeholders in the ARCHIVE copy only (GitHub rejects >100 MB blobs).
# Source repos/worktrees are never touched; git only ever writes new files / renames over refs in the hardlinked copy.
# Only refs whose history contains an introducing commit are rewritten, and only that part of their history.
set -u
export GIT_DIR=/root/.gpui-archive/gpui-rework.git
export FILTER_BRANCH_SQUELCH_WARNING=1
cd /root/.gpui-archive || exit 1

export P1=reactivity/evidence/uniform-packet/descending-timing/qualified.tar.gz B1=24f59a19505577fed8e838e6906583290a4dd462
export P2=reactivity/evidence/uniform-packet/failed-gpu-campaign/captures.tar.gz B2=80d48b2fb2691be7d6b053029725cc090be898f1

# Undo the first (over-broad) attempt: put every ref back to its pre-rewrite tip.
if [ -f index/refs.before.tsv ]; then
  while IFS=$'\t' read -r ref sha; do git update-ref "$ref" "$sha"; done < index/refs.before.tsv
  git for-each-ref --format='%(refname)' 'refs/original/' | xargs -r -n1 git update-ref -d
else
  git for-each-ref --format='%(refname)%09%(objectname)' > index/refs.before.tsv
fi

ph() { # path blob asset-name
  sz=$(git cat-file -s "$2"); sum=$(git cat-file blob "$2" | sha256sum | cut -d' ' -f1)
  printf 'Removed from this archive: GitHub rejects files over 100 MB.\n\noriginal path:   %s\noriginal size:   %s bytes\noriginal sha1:   %s (git blob)\noriginal sha256: %s\n\nThe original file is attached to the private GitHub release "archive-assets-20261024" of this repo as "%s".\n' "$1" "$sz" "$2" "$sum" "$3" | git hash-object -w --stdin
  printf '%s  %s\n' "$sum" "$3" >> index/SHA256SUMS
}
: > index/SHA256SUMS
export PH1=$(ph "$P1" "$B1" uniform-packet-descending-timing-qualified.tar.gz)
export PH2=$(ph "$P2" "$B2" uniform-packet-failed-gpu-campaign-captures.tar.gz)

# Every commit that adds either path with the big blob, in any ref; keep only the oldest ones (no other adder as ancestor).
adders=$( { git log --all --diff-filter=A --format=%H -- "$P1"; git log --all --diff-filter=A --format=%H -- "$P2"; } | sort -u )
roots=""; for c in $adders; do
  keep=1; for o in $adders; do [ "$o" != "$c" ] && git merge-base --is-ancestor "$o" "$c" && keep=0; done
  [ $keep = 1 ] && roots="$roots $c"
done
echo "adders: $(echo $adders | wc -w); roots:$roots"
neg=""; pos=""
for c in $roots; do for p in $(git rev-list --parents -n1 "$c" | cut -d' ' -f2-); do neg="$neg ^$p"; done; pos="$pos $(git for-each-ref --contains "$c" --format='%(refname)' | grep -v '^refs/original/' | tr '\n' ' ')"; done
pos=$(echo $pos | tr ' ' '\n' | sort -u | tr '\n' ' ')
echo "refs to rewrite: $(echo $pos | wc -w)"
echo "commits to walk: $(git rev-list --count $pos $neg)"
echo "signed commits in walk: $(git rev-list $pos $neg | while read c; do git cat-file commit "$c" | grep -q '^gpgsig' && echo 1; done | wc -l)"

mkdir -p /root/.gpui-archive/fb-tmp
git filter-branch -f -d /root/.gpui-archive/fb-tmp --index-filter '
  if git ls-files -s -- "$P1" | grep -q "$B1"; then git update-index --cacheinfo "100644,$PH1,$P1"; fi
  if git ls-files -s -- "$P2" | grep -q "$B2"; then git update-index --cacheinfo "100644,$PH2,$P2"; fi
' -- $pos $neg 2>&1 | grep -v 'is unchanged' | tail -6

echo "== old -> new tips"
git for-each-ref --format='%(refname)%09%(objectname)' | grep -v '^refs/original/' > index/refs.after.tsv
awk -F'\t' -v OFS='\t' 'NR==FNR{old[$1]=$2; next} ($1 in old) && old[$1]!=$2 {print $1, old[$1], $2}' index/refs.before.tsv index/refs.after.tsv > REFMAP.tsv
wc -l REFMAP.tsv; awk -F/ '{print $2"/"$3}' REFMAP.tsv | cut -f1 | sort | uniq -c | sort -rn | head -12
git for-each-ref --format='%(refname)' 'refs/original/' | xargs -r -n1 git update-ref -d
rm -rf /root/.gpui-archive/fb-tmp

echo "== large objects reachable from any ref (>95MB)"
git rev-list --objects --all | git cat-file --batch-check='%(objectsize) %(objectname) %(rest)' | awk '$1>95000000'
echo "== wip refs: $(git for-each-ref 'refs/heads/wt/*-wip' | wc -l); stash refs: $(git for-each-ref 'refs/heads/stash/*' | wc -l); total refs: $(git for-each-ref | wc -l)"
echo "REWRITE_DONE"
