set -u
export GIT_DIR=/root/.gpui-archive/gpui-rework.git
export GIT_AUTHOR_NAME="gpui archive" GIT_AUTHOR_EMAIL="archive@localhost" GIT_COMMITTER_NAME="gpui archive" GIT_COMMITTER_EMAIL="archive@localhost"
OUT=/root/.gpui-archive/nested; rm -rf $OUT; mkdir -p $OUT
IDX=/root/.gpui-archive/index/nested.idx; rm -f $IDX
wip=f3c34e3d8df7eae96acae932251921c6adf79bf7   # the tip already pushed; discards the first (bad) local attempt
GIT_INDEX_FILE=$IDX git read-tree "$wip"
add() { b=$(git hash-object -w "$1"); GIT_INDEX_FILE=$IDX git update-index --add --cacheinfo "100644,$b,.archive-nested-repos/$2"; }
for p in mygo nanoui; do
  ( unset GIT_DIR GIT_INDEX_FILE; cd /root/gpui-bench-luna/comparators/$p || exit
    G="git -c safe.directory=*"
    { echo "path in worktree: comparators/$p"; echo "origin: $($G remote get-url origin)"; echo "HEAD: $($G rev-parse HEAD)"; echo "head subject: $($G log -1 --format='%ad %an | %s' --date=short)"
      echo "commits not on any remote branch: $($G rev-list --count HEAD --not --remotes)"; echo "partial clone filter: $($G config remote.origin.partialclonefilter || true)"
      echo; echo "git status --porcelain --ignored:"; $G status --porcelain --ignored; } > $OUT/$p.info.txt
    $G diff HEAD --binary > $OUT/$p.tracked.patch
    $G ls-files -o --exclude-standard -z | tar --null -T - -cf $OUT/$p.untracked.tar 2>/dev/null
  )
  add $OUT/$p.info.txt "$p/info.txt"; add $OUT/$p.tracked.patch "$p/tracked.patch"; add $OUT/$p.untracked.tar "$p/untracked.tar"
  echo "$p: patch=$(wc -c < $OUT/$p.tracked.patch)B untracked.tar=$(wc -c < $OUT/$p.untracked.tar)B"
done
cat > $OUT/README.txt <<'TXT'
comparators/mygo and comparators/nanoui in /root/gpui-bench-luna are third-party git clones (partial clones) that were
untracked directories of the worktree, so the snapshot commit below this one does not contain them. Each directory here holds:
  info.txt       origin URL, HEAD sha, unpushed-commit count (0 for both), and git status at archive time
  tracked.patch  git diff HEAD --binary (local changes to tracked files)
  untracked.tar  untracked, non-ignored files
Ignored build output (nanoui/.zig-cache, ~200 MB) was not archived.

Also excluded from this worktree's snapshot because GitHub rejects files over 100 MB:
  reactivity/benchmarks/luna-accessibility-floor/runs/session-bus-late-activation-20261021/late-activation-probe-original
  104,280,640 bytes, sha256 c7f74470a0f53c2823c9f54d47e31c9d3ae38d595f65919e0ba3919599283343
  stored as the asset "luna-late-activation-probe-original" on the private release "archive-assets-20261024" of this repo.
TXT
add $OUT/README.txt README.txt
tree=$(GIT_INDEX_FILE=$IDX git write-tree)
c=$(git commit-tree "$tree" -p "$wip" -m "Record the nested comparator repos of gpui-bench-luna (untracked clones: mygo, nanoui) and the excluded >100 MB file")
git update-ref refs/heads/wt/gpui-bench-luna-wip "$c"
echo "new wip tip $c (parent $wip)"
git diff --stat "$wip" "$c" | tail -9
