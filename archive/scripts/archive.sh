#!/bin/bash
# Build the archive repo on prod. Reads source repos/worktrees only; writes only under /root/.gpui-archive.
set -u
ARCH=/root/.gpui-archive
COPY=$ARCH/gpui-rework.git
rm -rf "$ARCH"; mkdir -p "$ARCH"
cp -al /root/gpui-ce3/.git "$COPY"   # hardlinks: git never rewrites objects in place; refs/config change by rename
rm -rf "$COPY/worktrees" "$COPY/hooks"
git --git-dir="$COPY" config core.bare true
git --git-dir="$COPY" config --unset core.worktree 2>/dev/null
G="git --git-dir=$COPY -c safe.directory=*"
# Hummingbird repos: pull their refs in under a separate namespace, so their objects are present.
$G fetch --no-tags /root/gpui-hummingbird-files-sol '+refs/heads/*:refs/hb-files/heads/*' 2>&1 | tail -2
$G fetch --no-tags /root/gpui-hummingbird-fresh-schema-sol '+refs/heads/*:refs/hb-fresh/heads/*' 2>&1 | tail -2
# Some worktree HEADs are only reachable in the hummingbird object stores; hardlink those objects in (no clobber).
cp -aln /root/gpui-hummingbird-files-sol/.git/objects/. "$COPY/objects/" 2>/dev/null
cp -aln /root/gpui-hummingbird-fresh-schema-sol/.git/objects/. "$COPY/objects/" 2>/dev/null
export GIT_AUTHOR_NAME="gpui archive" GIT_AUTHOR_EMAIL="archive@localhost" GIT_COMMITTER_NAME="gpui archive" GIT_COMMITTER_EMAIL="archive@localhost"
mkdir -p "$ARCH/index"
echo "== per-worktree refs"
for d in $(awk -F, 'NR>1 && ($2=="worktree" || $2=="repo"){print $1}' /root/.inventory.csv); do
  src=/root/$d
  sha=$(git -c safe.directory=* -C "$src" rev-parse HEAD 2>/dev/null) || { echo "NOHEAD $d"; continue; }
  # Make sure the commit is in the archive object store.
  if ! $G cat-file -e "$sha" 2>/dev/null; then
    case "$d" in
      gpui-hummingbird-*) echo "MISSING $d $sha (not found in archive)";;
      *) echo "MISSING $d $sha";;
    esac
  fi
  $G update-ref "refs/heads/wt/$d" "$sha" 2>/dev/null || echo "REFFAIL $d"
  mod=$(git -c safe.directory=* -C "$src" ls-files -m -o --exclude-standard | wc -l)
  if [ "$mod" -gt 0 ]; then
    list="$ARCH/index/$d.list"
    git -c safe.directory=* -C "$src" ls-files -m -o --exclude-standard -z > "$list"
    export GIT_INDEX_FILE="$ARCH/index/$d.idx"
    rm -f "$GIT_INDEX_FILE"
    $G read-tree "$sha"
    ( cd "$src" && GIT_DIR="$COPY" GIT_WORK_TREE="$src" GIT_INDEX_FILE="$GIT_INDEX_FILE" git -c safe.directory=* update-index --add --remove -z --stdin < "$list" ) || echo "UPDATEINDEX_FAIL $d"
    tree=$($G write-tree)
    commit=$($G commit-tree "$tree" -p "$sha" -m "WIP snapshot of $d: uncommitted and untracked (non-ignored) files at archive time")
    $G update-ref "refs/heads/wt/$d-wip" "$commit"
    unset GIT_INDEX_FILE
    echo "WIP $d $mod files -> $commit"
  fi
  echo "OK $d $sha"
done
echo "== stashes"
i=0
while $G rev-parse -q --verify "stash@{$i}" >/dev/null 2>&1; do
  $G update-ref "refs/heads/stash/$i" "$($G rev-parse "stash@{$i}")"; i=$((i+1))
done
echo "stash count $i"
echo "== archive refs"
$G for-each-ref --format='%(refname)' | awk -F/ '{print $2"/"$3}' | sort | uniq -c
echo "ARCHIVE_DONE"
