export GIT_DIR=/root/.gpui-archive/gpui-rework.git
export GIT_SSH_COMMAND='ssh -o UserKnownHostsFile=/root/.gh_known_hosts -o StrictHostKeyChecking=yes -o BatchMode=yes -o ServerAliveInterval=30'
R=git@github.com:philocalyst/gpui-rework-archive.git
push() { l=$1; shift; git push --no-verify --porcelain "$R" "$@" > /root/.push.$l.log 2>&1; rc=$?
  echo "batch $l rc=$rc ok=$(grep -cE '^[*= ]\s' /root/.push.$l.log) rejected=$(grep -c '^!' /root/.push.$l.log)"
  [ $rc -ne 0 ] && grep -E '^!|error|fatal' /root/.push.$l.log | cut -c1-200 | head -4; }
push 3a-hb 'refs/hb-files/heads/*:refs/heads/hummingbird-files/*' 'refs/hb-fresh/heads/*:refs/heads/hummingbird-fresh/*'
push 3b-misc 'refs/preservation/*:refs/heads/ns/preservation/*' 'refs/qualification/*:refs/heads/ns/qualification/*' 'refs/sync-review/*:refs/heads/ns/sync-review/*'
push 3c-codex 'refs/codex/*:refs/heads/ns/codex/*'
push 3d-imports 'refs/codex-imports/*:refs/heads/ns/codex-imports/*'
echo PUSH3_DONE
