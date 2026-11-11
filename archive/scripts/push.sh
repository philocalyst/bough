#!/bin/bash
# Push the archive repo to the private GitHub repo. Reads only the archive copy; uses the forwarded ssh-agent (no key on this host).
export GIT_DIR=/root/.gpui-archive/gpui-rework.git
export GIT_SSH_COMMAND='ssh -o UserKnownHostsFile=/root/.gh_known_hosts -o StrictHostKeyChecking=yes -o BatchMode=yes -o ServerAliveInterval=30'
R=git@github.com:philocalyst/gpui-rework-archive.git
ssh -o UserKnownHostsFile=/root/.gh_known_hosts -o StrictHostKeyChecking=yes -o BatchMode=yes -T git@github.com 2>&1 | head -1
push() { # label refspec...
  l=$1; shift; t0=$(date +%s)
  git push --no-verify --porcelain "$R" "$@" > "/root/.push.$l.log" 2>&1; rc=$?
  echo "batch $l rc=$rc $(( $(date +%s)-t0 ))s refs_ok=$(grep -c '^\*\|^ \|^+' /root/.push.$l.log) rejected=$(grep -c '^!' /root/.push.$l.log)"
  [ $rc -ne 0 ] && tail -8 "/root/.push.$l.log"
  return $rc
}
push 0-main  refs/heads/main:refs/heads/main 'refs/tags/*:refs/tags/*' || exit 1
others=$(git for-each-ref --format='%(refname)' refs/heads | grep -v -e '^refs/heads/wt/' -e '^refs/heads/main$' | sed 's|.*|&:&|')
push 1-heads $others || exit 1
push 2-wt 'refs/heads/wt/*:refs/heads/wt/*' || exit 1
push 3-extra '+refs/hb-files/heads/*:refs/heads/hummingbird-files/*' '+refs/hb-fresh/heads/*:refs/heads/hummingbird-fresh/*' \
  'refs/preservation/*:refs/heads/ns/preservation/*' 'refs/qualification/*:refs/heads/ns/qualification/*' \
  'refs/sync-review/*:refs/heads/ns/sync-review/*' 'refs/codex/*:refs/heads/ns/codex/*' 'refs/codex-imports/*:refs/heads/ns/codex-imports/*' || exit 1
echo PUSH_DONE
