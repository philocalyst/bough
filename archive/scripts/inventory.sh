#!/bin/bash
# Read-only: report HEAD, branch, dirty counts for every gpui* git worktree/repo on prod.
cd /root
echo "dir,kind,branch,head,modified,untracked,stash_count"
for d in gpui*; do
  if [ -f "$d/.git" ]; then kind=worktree; elif [ -d "$d/.git" ]; then kind=repo; else continue; fi
  g="git -c safe.directory=* -C $d"
  br=$($g symbolic-ref --short -q HEAD || echo DETACHED)
  head=$($g rev-parse --short HEAD 2>/dev/null)
  mod=$($g diff --name-only HEAD 2>/dev/null | wc -l)
  unt=$($g ls-files --others --exclude-standard 2>/dev/null | wc -l)
  st=$($g stash list 2>/dev/null | wc -l)
  echo "$d,$kind,$br,$head,$mod,$unt,$st"
done
echo "DONE"
