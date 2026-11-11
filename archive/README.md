# gpui rework archive: brief

Written 2026-10-24. Source host: **Nudox Prod** (`ilo-prod`, `root@87.99.136.215`). Destination: **private** GitHub repo `philocalyst/gpui-rework-archive` (https://github.com/philocalyst/gpui-rework-archive).

This repo is the preserved state of the GPUI clean-room rework as it stood on prod: every worktree's commit, every worktree's uncommitted/untracked work, the three stashes, and all local branches and tags. It is a git archive, not a working checkout. Nothing on prod was modified or deleted.

## Research continuation — 2026-10-29

The architecture/performance investigation is now captured on **168 `local/*` branches**, including 27 new branches and 2 updated tips since October 28. Implementations, baselines, failed experiments and original verdicts are preserved.

- **[Current research index](RESEARCH_BRANCHES.md)** — latest component replay qualification, BBTree/display representation experiments, structural/layout investigations and the production/observer cost correction.
- **[Exact snapshot manifest](RESEARCH_MANIFEST.tsv)** — captured commits and provenance for every research branch.
- **[October 28 snapshot](RESEARCH_BRANCHES_2026-10-28.md)** — preserved historical index.

Publication is preservation, not adoption or an overall ZGX victory. The latest prototype still has linear producer work and storage losses; the original end-to-end/hardware goal remains unproven. External executable/build archives are not uploaded. Historical “local only” wording in sealed dossiers predates this authorized publication.

The numbered sections and appendices below describe the original October 24 archive and retain their historical counts.

## 1. Status at a glance

| Item | State |
|---|---|
| All 211 git worktrees (1 main repo + 2 standalone Hummingbird repos + 208 linked worktrees) | Pushed as `wt/<directory>` branches |
| Uncommitted + untracked work in the 53 dirty worktrees (474 files) | Pushed as `wt/<directory>-wip` branches (one commit on top of that worktree's HEAD) |
| 3 repo-wide stashes | Pushed as `stash/0`, `stash/1`, `stash/2` |
| All other local branches (364, including `main`) and tags (9) | Pushed under their original names |
| 103 branches contain the two >100 MB tarballs | **Rewritten** in the archive only (tarballs replaced by a text placeholder). Their SHAs differ from prod. See section 5 |
| 3 files >100 MB (2 committed tarballs + 1 untracked probe file) | Not in git. Stored as assets on a private GitHub release (section 5) |
| Gitignored files, `target/` build output, 22 non-git artifact directories, remote-tracking refs | **Not pushed** (section 6) |
| Local `.tgz` / copy to this Mac | **Not done.** This Mac has about 6.4 GB free; the source tree is about 177 GiB without build output (section 7) |
| Rex | Nudox Prod's rex server was unreachable (connection refused on port 7297), so everything went over plain SSH |

### Push result (verified)

- 654 branches and 9 tags (663 refs) were pushed and verified; `archive-index` (the branch this brief is on) and the release assets were added afterwards, so GitHub shows 655 branches. `git ls-remote` against the archive's refs matches ref for ref: 0 missing, 0 extra, 0 with a different SHA. The repo reports `PRIVATE`.
- Pushed in four batches from prod over a forwarded SSH agent: `main` and tags; the 366 other local branches; the 264 `wt/*` branches; then the extra namespaces. The extras had to be retried in smaller batches (see "Other adjustments", section 5).
- GitHub printed non-blocking warnings (recommended 50 MB limit, hard limit 100 MB) for five committed files, all present in the archive as-is:
  `reactivity/evidence/accesskit-close/qualification.tar.gz` (50.9 MB), `fearless-integration/producer-controls.tar.gz` (52.3 MB), `inbox-reader-refresh/qualification.tar.gz` (65.4 MB), `uniform-packet/legacy-timing/captures.tar.gz.part-01` (64.0 MB) and `.part-02` (54.2 MB).
- Spot checks of the dirty snapshots: `inspector-contained-owner` 311 files (+200,345 lines), `component-default-simd-sol-20261024` 25 files, `text-query-baseline` 1 file, all matching the file counts recorded at snapshot time.

## 2. What the project is (as of the last commit on prod)

The main repo is `/root/gpui-ce3` (remote lineage: `nudoxorg/gpui-ce`); the active work lives in its `reactivity/` directory, a **clean-room GPUI implementation** (`reactivity/README.md`):

- Real views, typed state, Cephal layout, text, transformed input and live inspector properties run through one exclusively owned runtime. Demo apps (Counter, Todo, a color/transform gallery, a composed Workspace, a 250,000-recording Library) use the same engine natively and in headless tests.
- Crates under `reactivity/crates/` include `gpui`, `canvas` (reference renderer) and `gpu` (consumes the canvas's borrowed scene via `wgsl-rs`).
- Workspace pins `nightly-2026-10-23`.
- Design documents (all in `reactivity/`): DESIGN, CONTRACT, REACTIVITY, CONCURRENCY, GRAPHICS, FORMULATIONS, MEMORY, TESTING, PLAN, EXPERIMENTS, REVIEW. **STATUS.md** (about 82 KB) is the running record of what is integrated, what is only measured, and what gates remain; `evidence/` holds the per-experiment dossiers (about 137 directories, 3.3 GB on prod).
- Latest main-repo commit on prod: `d464b7de9e` "Record the Todo oracle false pass and per-phase detection evidence" (2026-10-24), on branch `codex/reactivity-cleanroom`.
- Recent work visible in STATUS.md: IME grapheme restart over certified ASCII prefixes, scoped IME composition wired to accessibility, per-phase Todo recording oracle, Remote first-lock repair (Darwin mutex allocation), Reader one-shot handoff, packed tree and typed child composition.
- The 208 linked worktrees are mostly one-experiment-per-worktree branches (names ending in `-sol` or `-sol-20261024` for the recent ones, `-control`, `-mutant` for A/B and mutation controls), plus `production/*`, `spike/*`, `docs-*`, and two Hummingbird port repos.

## 3. How the archive is laid out

| Branch pattern | What it is | Count |
|---|---|---|
| `wt/<dir>` | The commit checked out in `/root/<dir>` at archive time. `<dir>` is the directory name on prod (for example `wt/gpui-text-sol`). Includes `wt/gpui-ce3` (main repo) and the two Hummingbird repos | 211 |
| `wt/<dir>-wip` | `wt/<dir>` plus **one commit** containing that worktree's uncommitted changes and untracked (non-ignored) files. Author `gpui archive <archive@localhost>`, unsigned | 53 |
| `stash/0`, `stash/1`, `stash/2` | `git stash` entries of `gpui-ce3` (section 4) | 3 |
| `<original branch name>` (`main`, `codex/...`, `production/...`, `spike/...`, ...) | The repo's own local branches, same names as on prod | 364 (including `main`) |
| `hummingbird-files/*`, `hummingbird-fresh/*` | Branches of the two standalone Hummingbird repos (`gpui-hummingbird-files-sol`, `gpui-hummingbird-fresh-schema-sol`) | 1 + 2 |
| `ns/preservation/*`, `ns/qualification/*`, `ns/sync-review/*`, `ns/codex/*`, `ns/codex-imports/*` | Non-branch ref namespaces from the repo (they would not be visible on GitHub as-is, so they were mapped under `ns/`) | 20 |
| tags | `v0.2.0`, `v0.3.0-preview0`, `v0.3.0-preview2`, `v0.3.1`, `v0.4.0-preview0/1/2`, `v0.4.1`, `v0.5.0-preview0` | 9 |
| `archive-index` | **Default branch.** This brief, `MANIFEST.tsv`, `REFMAP.tsv`, `SHA256SUMS` and the `scripts/` used to build and push the archive. No project source code | 1 |

Because the 208 linked worktrees share one object store with the main repo, everything is one repository here: any `wt/...` branch can be checked out from a single clone.

### Restoring things

```sh
git clone git@github.com:philocalyst/gpui-rework-archive.git        # all branches; expect a large clone
git worktree add ../gpui-text-sol wt/gpui-text-sol                   # a worktree at its committed state
git worktree add ../gpui-x-wip wt/gpui-inspector-contained-owner-wip # committed state + the uncommitted work
git diff wt/gpui-inspector-contained-owner wt/gpui-inspector-contained-owner-wip   # only the uncommitted work
git ls-tree -r --name-only wt/gpui-ce3 | head                        # browse without checking out
```

`MANIFEST.tsv` on `archive-index` maps every `wt/<dir>` back to its original branch name (or `DETACHED`), prod SHA, archive SHA, and snapshot details. Appendix A below is the same data.

## 4. Dirty worktrees and stashes

### How the uncommitted work was captured without touching prod

For each worktree with changes, a throwaway index file was seeded from the worktree's HEAD, every modified and untracked non-ignored path (`git ls-files -m -o --exclude-standard`) was added to that index with the worktree as the source, and one commit was created from the resulting tree. The real index, HEAD, refs and files of every worktree were never written to. The archive object store is a hard-linked copy of `gpui-ce3/.git`, so new objects were written only under `/root/.gpui-archive` on prod.

What a snapshot contains: tracked-file modifications (including deletions) and untracked files that are not gitignored. What it does not contain: gitignored files (see section 6) and anything over 95 MB (one file, section 5).

### The 53 worktrees with uncommitted work (474 files in total)

| Directory | Original branch | Files captured (modified + untracked, non-ignored) | Snapshot branch |
|---|---|---|---|
| `gpui-inspector-contained-owner` | `inspector-contained-owner-sol` | 311 | `wt/gpui-inspector-contained-owner-wip` |
| `gpui-component-default-simd-sol-20261024` | `codex/component-default-simd-sol-20261024` | 25 | `wt/gpui-component-default-simd-sol-20261024-wip` |
| `gpui-production-elements` | `codex/gpui-production-elements` | 16 | `wt/gpui-production-elements-wip` |
| `gpui-production-native-upgrade` | `production/native-upgrade` | 16 | `wt/gpui-production-native-upgrade-wip` |
| `gpui-identity-hole` | `codex/identity-hole` | 11 | `wt/gpui-identity-hole-wip` |
| `gpui-scroll-baseline` | `production/scroll-baseline` | 7 | `wt/gpui-scroll-baseline-wip` |
| `gpui-production-color` | `production/color` | 6 | `wt/gpui-production-color-wip` |
| `gpui-style-dimensions` | `codex/style-dimensions` | 6 | `wt/gpui-style-dimensions-wip` |
| `gpui-thin-text-baseline` | `production/thin-text-baseline` | 5 | `wt/gpui-thin-text-baseline-wip` |
| `gpui-production-presentation` | `production/presentation` | 4 | `wt/gpui-production-presentation-wip` |
| `gpui-route-columns-baseline` | *(detached)* | 4 | `wt/gpui-route-columns-baseline-wip` |
| `gpui-shape-storage` | `codex/shape-storage` | 4 | `wt/gpui-shape-storage-wip` |
| `gpui-production-solid` | `production/native-solid` | 3 | `wt/gpui-production-solid-wip` |
| `gpui-route-profile-baseline` | *(detached)* | 3 | `wt/gpui-route-profile-baseline-wip` |
| `gpui-route-profile-candidate` | *(detached)* | 3 | `wt/gpui-route-profile-candidate-wip` |
| `gpui-style-startup-current` | *(detached)* | 3 | `wt/gpui-style-startup-current-wip` |
| `gpui-unicode-baseline` | *(detached)* | 3 | `wt/gpui-unicode-baseline-wip` |
| `gpui-bench-luna` | *(detached)* | 2 + 7 nested-repo records | `wt/gpui-bench-luna-wip` |
| `gpui-hit-parent` | `codex/hit-run-parent` | 2 | `wt/gpui-hit-parent-wip` |
| `gpui-identity-index` | `codex/exact-identity-index` | 2 | `wt/gpui-identity-index-wip` |
| `gpui-identity-index-base` | *(detached)* | 2 | `wt/gpui-identity-index-base-wip` |
| `gpui-identity-runs` | `codex/identity-runs` | 2 | `wt/gpui-identity-runs-wip` |
| `gpui-paint-5f4` | *(detached)* | 2 | `wt/gpui-paint-5f4-wip` |
| `gpui-production-gpu` | `production/gpu-review` | 2 | `wt/gpui-production-gpu-wip` |
| `gpui-style-batch-counted` | *(detached)* | 2 | `wt/gpui-style-batch-counted-wip` |
| `gpui-bin-miss-bounds` | *(detached)* | 1 | `wt/gpui-bin-miss-bounds-wip` |
| `gpui-bin-miss-inline` | *(detached)* | 1 | `wt/gpui-bin-miss-inline-wip` |
| `gpui-bin-phase-parent` | *(detached)* | 1 | `wt/gpui-bin-phase-parent-wip` |
| `gpui-cursor-free` | `codex/cursor-free-raster` | 1 | `wt/gpui-cursor-free-wip` |
| `gpui-gpu-packet-baseline` | *(detached)* | 1 | `wt/gpui-gpu-packet-baseline-wip` |
| `gpui-hit-candidate` | `codex/hit-candidate-byte` | 1 | `wt/gpui-hit-candidate-wip` |
| `gpui-hit-links` | `codex/packed-presentation-links` | 1 | `wt/gpui-hit-links-wip` |
| `gpui-hit-runs` | `codex/hit-run-proof` | 1 | `wt/gpui-hit-runs-wip` |
| `gpui-identity-hole-hot` | `codex/identity-hole-hot` | 1 | `wt/gpui-identity-hole-hot-wip` |
| `gpui-identity-inline` | `codex/identity-inline` | 1 | `wt/gpui-identity-inline-wip` |
| `gpui-image-axis-baseline` | `production/image-axis-baseline` | 1 | `wt/gpui-image-axis-baseline-wip` |
| `gpui-lazy-raster-bins` | `codex/lazy-raster-bins` | 1 | `wt/gpui-lazy-raster-bins-wip` |
| `gpui-literal-dispatch` | `codex/literal-dispatch` | 1 | `wt/gpui-literal-dispatch-wip` |
| `gpui-literal-dispatch-base` | *(detached)* | 1 | `wt/gpui-literal-dispatch-base-wip` |
| `gpui-production-components` | `production/components` | 1 | `wt/gpui-production-components-wip` |
| `gpui-question-channel-baseline` | *(detached)* | 1 | `wt/gpui-question-channel-baseline-wip` |
| `gpui-question-inputs-base` | *(detached)* | 1 | `wt/gpui-question-inputs-base-wip` |
| `gpui-raster-borrowed-rows` | `codex/borrowed-raster-rows` | 1 | `wt/gpui-raster-borrowed-rows-wip` |
| `gpui-raster-cuts` | `codex/raster-cut-ownership` | 1 | `wt/gpui-raster-cuts-wip` |
| `gpui-raster-narrow` | `codex/raster-narrow-indices` | 1 | `wt/gpui-raster-narrow-wip` |
| `gpui-reactivity-coverage` | `spike/reactivity-coverage` | 1 | `wt/gpui-reactivity-coverage-wip` |
| `gpui-reactivity-templates` | `spike/reactivity-templates` | 1 | `wt/gpui-reactivity-templates-wip` |
| `gpui-reactivity-tree` | `spike/reactivity-layout` | 1 | `wt/gpui-reactivity-tree-wip` |
| `gpui-sample-init` | `codex/lazy-sample-initialization` | 1 | `wt/gpui-sample-init-wip` |
| `gpui-shape-baseline` | *(detached)* | 1 | `wt/gpui-shape-baseline-wip` |
| `gpui-style-startup-base` | *(detached)* | 1 | `wt/gpui-style-startup-base-wip` |
| `gpui-text-query-baseline` | *(detached)* | 1 | `wt/gpui-text-query-baseline-wip` |
| `gpui-text-selection-editor-control` | *(detached)* | 1 | `wt/gpui-text-selection-editor-control-wip` |

### Stashes (`gpui-ce3`)

| Branch | Stash message | Date | Author |
|---|---|---|---|
| `stash/0` | On `accessibility-selection-sol`: a11y-initial-slice | 2026-10-22 | Codex |
| `stash/1` | On `shader-paint`: shader paint before atomic split | 2026-10-06 | Philocalyst |
| `stash/2` | On `gpu-split`: codex-pre-upstream-rebase-gpu-split | 2026-09-04 | Philocalyst |

Each stash branch points at the stash commit itself (a merge of the working tree and index at stash time); `git show stash/0` shows the changes.

## 5. What was altered: the two tarballs and the rewritten branches

GitHub refuses any push that contains a file over 100 MB. Three files exceeded that:

| File | Size | sha256 | Where it went |
|---|---|---|---|
| `reactivity/evidence/uniform-packet/descending-timing/qualified.tar.gz` (committed) | 127,976,708 B | `3007f00e361e0e0162561b4ff3f36c2328b81ea5a64430dcd0a9834a34775d1c` | Release asset `uniform-packet-descending-timing-qualified.tar.gz` |
| `reactivity/evidence/uniform-packet/failed-gpu-campaign/captures.tar.gz` (committed) | 113,642,939 B | `32c5e9e633c34d28b8de951d20d62c2090df352dbbc788f1d8deb0d93ef914b8` | Release asset `uniform-packet-failed-gpu-campaign-captures.tar.gz` |
| `reactivity/benchmarks/luna-accessibility-floor/runs/session-bus-late-activation-20261021/late-activation-probe-original` (untracked, in `/root/gpui-bench-luna`) | 104,280,640 B | `c7f74470a0f53c2823c9f54d47e31c9d3ae38d595f65919e0ba3919599283343` | Release asset `luna-late-activation-probe-original` |

The assets are on the private release `archive-assets-20261024` of this repo, and the originals are still on prod untouched.

The two committed tarballs were introduced by commit `e7ba208bb1` ("Preserve the failed fixed GPU campaign and stronger borrowed-kernel research", 2026-10-23) and are inherited by every branch descended from it. **In the archive only**, those branches were rewritten from `e7ba208bb1` forward: the two files were replaced by a small text placeholder (path, size, SHA-1, SHA-256 and the asset name). Nothing else about the commits changed: same trees, authors, dates and messages. No commit in the rewritten range was signed, so no signatures were lost.

Consequences:

- **103 refs have different SHAs in the archive than on prod** (all of them list the new and old tip in `REFMAP.tsv` and Appendix B). That is 17 `wt/<dir>` worktree branches + 1 `-wip` branch, 21 `codex/*`, 23 `spike/*`, 36 other branches, 2 `hummingbird-fresh/*`, and 3 `ns/qualification/*`.
- Commit `d464b7de9e`, the tip of the main `reactivity` work on prod (`wt/gpui-ce3`, `codex/reactivity-cleanroom`), is one of them. Its archive SHA is `cc193f85f5`.
- The other 560 pushed refs (branches and tags) have exactly the same SHAs as prod.
- The placeholder text blobs are `dfee48a262` and `d4b8a57720`.
- To get a byte-exact checkout on the affected branches, download the release asset and drop it at the original path.

### Other adjustments to know about

- **Tree-only refs wrapped in commits.** `refs/codex/turn-diffs/checkpoints/*` (3 refs, written by the Codex app) point at bare trees on prod. GitHub only accepts commits as branch tips, so each tree was wrapped in a one-commit, parentless branch `ns/codex/turn-diffs/<uuid>`; the original ref name is in the commit message. Mapping: `15715ca1-…` tree `c6fe0960da` → commit `fe44979e29`; `f78f59ee-…` tree `569a4774e5` → `6e6e6861e1`; `c8d2d850-…` tree `9fd9eb6620` → `691fe396f7`.
- **Nested git repos inside `gpui-bench-luna`.** `comparators/mygo` (`github.com/egoist/mygo` @ `ea23ad2975`) and `comparators/nanoui` (`github.com/justrach/nanoui` @ `dfa7f7ab4e`) are third-party partial clones sitting untracked in that worktree, and a plain snapshot cannot include a directory that is itself a repo. Neither has unpushed commits. Their local changes (a `build.zig` edit in nanoui; a new `examples/luna-bench/` in both) were recorded on `wt/gpui-bench-luna-wip` under `.archive-nested-repos/` as `info.txt`, `tracked.patch` and `untracked.tar` (the branch has a second commit for this). Ignored build output there (`nanoui/.zig-cache`, ~200 MB) was not kept.
- **Namespaces moved under `ns/`.** Refs outside `refs/heads` and `refs/tags` (`refs/preservation`, `refs/qualification`, `refs/sync-review`, `refs/codex`, `refs/codex-imports`) were pushed as branches `ns/<namespace>/...`; the two Hummingbird repos' branches were pushed as `hummingbird-files/...` and `hummingbird-fresh/...`.

## 6. What is **not** in the archive

1. **Gitignored files in every worktree.** Notably all `target/` build output (`/root/gpui-ce3/reactivity/target` alone is 509 GB; the whole tree is 532 GB), plus anything else a `.gitignore` excludes.
2. **22 directories on prod that are not git repos** (about 131 GB):

   | Directory | Size |
   |---|---|
   | `gpui-future-target` | 43.1 GB |
   | `gpui-a11y-target` | 25.0 GB |
   | `gpui-text-selection-editor-target` | 16.1 GB |
   | `gpui-inspector-ui-target` | 13.1 GB |
   | `gpui-boolean-cohort-sol-artifacts-20261024` | 8.4 GB |
   | `gpui-component-mac-relocation-sol-20261024` | 6.0 GB |
   | `gpui-text-selection-target` | 3.2 GB |
   | `gpui-component-ime-sol-artifacts-20261024` | 2.9 GB |
   | `gpui-component-simd-sol-artifacts-20261024` | 2.9 GB |
   | `gpui-palette-phase` | 2.2 GB |
   | `gpui-text-selection-cut-target` | 1.4 GB |
   | `gpui-empty-floor-target-default` / `-thin` | 1.0 GB each |
   | `gpui-cursor-free-target` | 866 MB |
   | `gpui-future-release-target` | 527 MB |
   | `gpui-typed-binding-sol-artifacts-20261024`, `gpui-component-review-sol-artifacts-20261024` | 252 MB each |
   | `gpui-migration-tool-target` | 234 MB |
   | `gpui-text-query-baseline-target` | 223 MB |
   | `gpui-length-port-sol-artifacts-20261024` | 25 MB |
   | `gpui-route-profile-artifacts` | 112 KB |
   | `gpui-presentation-vendor-staging` | 12 KB |

   The `*-target` ones are build caches. The seven `*-artifacts-*`, `gpui-component-mac-relocation-sol-20261024` and `gpui-palette-phase` directories (about 26 GB) are probably run outputs and I did not open them; if any holds evidence you want, it needs a separate copy. They are the one place where work could exist that this archive does not cover.
3. **Remote-tracking refs** (78: `origin/*`, `upstream/*`). They mirror `nudoxorg/gpui-ce` and upstream GPUI, which are already on GitHub. Commits reachable only from them are not in the archive.
4. **Reflogs**, the worktree-private files under `.git/worktrees/*` (each worktree's index, HEAD log), and git hooks.
5. Sources of the 22 non-git directories are not repos, so no history exists for them.

## 7. The original `.tgz` / local-copy request

The first request was to move all outstanding data from prod to this Mac and bundle the source as a `.tgz`. That was not done:

- This Mac has about 6.4 GB of free disk.
- On prod, everything under `gpui*` except directories literally named `target` is about 177 GiB. That figure includes the ~131 GB of non-git directories listed in section 6, most of which are build caches themselves. The main repo alone is 532 GB because of `reactivity/target`.
- The git content of all 211 worktrees is about 1.7 GB, which is what this repo holds.

The git archive is the practical equivalent of the source bundle. If you want a single file on this Mac, either of these fits within the free disk space:

```sh
git clone --mirror git@github.com:philocalyst/gpui-rework-archive.git      # on this Mac
# or, on prod, from the archive copy (about 1.7 GB):
git --git-dir=/root/.gpui-archive/gpui-rework.git bundle create /root/gpui-rework.bundle --all
```

The non-git directories (item 2 in section 6) and gitignored data cannot come over without an external drive or a different machine.

## 8. Account, auth and identity notes

- **Personal account = `philocalyst`** (confirmed). The SSH key `/run/secrets/ssh/github` (`SHA256:wlcVZoKr6VOplFs38gb1ZfplmkzMoR5Ja6+JopYkG2o`) is registered on it, and `ssh -T git@github.com` authenticates as `philocalyst`. The SSH user is always `git`. `81milwir` is a second `gh` login with no SSH key on it; it was not used.
- The earlier draft of this brief said SSH could not work and that `81milwir` looked like the personal account. Both statements were wrong and are superseded by this document.
- **The key never left this Mac.** Prod pushed using SSH agent forwarding (`ssh -A`), with GitHub's host key pinned in a temporary known-hosts file. Nothing about the key or any token was written on prod.
- **Org code in a personal account.** `gpui-ce3` descends from `nudoxorg/gpui-ce`, and `philocalyst` is a member of `nudoxorg`. The repo is private; it contains org-lineage code, which was the owner's call here. Be careful about making it public or adding collaborators.
- **Identity.** Original commits keep their original authors (`Codex`, `𝑷𝒉𝒊𝒍𝒐𝒄𝒂𝒍𝒚𝒔𝒕`). The 53 snapshot commits are authored `gpui archive <archive@localhost>` and are unsigned; this `archive-index` commit uses the regular global git identity, unsigned.
- **Forks are public.** The original plan was to push onto a fork of `gpui-ce`; a fork of a public repo cannot be made private, so a separate private repo was used instead.

## 9. Left on prod (nothing deleted)

All under `/root`, created only by this work, all hidden dotfiles except the archive directory:

| Path | What |
|---|---|
| `/root/.gpui-archive/` | The archive: hard-linked copy of `gpui-ce3/.git` (~1.7 GB, mostly shared inodes with the source repo), `MANIFEST.tsv`, `REFMAP.tsv`, `index/` |
| `/root/.inventory.sh`, `.inventory.csv`, `.inventory.err` | Read-only inventory of the 211 worktrees |
| `/root/.size.sh`, `.size.csv` | Size measurements |
| `/root/.archive.sh`, `.archive.log`, `.fix.sh`, `.fix.log`, `.rewrite.sh`, `.rewrite.log`, `.push.sh`, `.push3.sh`, `.push.*.log`, `.big.sh`, `.chk.sh`, `.count.sh`, `.diag.sh`, `.codex*.sh`, `.nested*.sh`, `.verify.sh` | Build, patch, rewrite, push and diagnostic scripts and logs (copies of the main ones are in `scripts/` on this branch) |
| `/root/.expected.refs`, `.remote.refs`, `.codex.objs` | Ref lists used to verify the push |
| `/root/.gh_known_hosts` | GitHub's host key (public) used to pin the push |

The archive's object files are hard links to `gpui-ce3/.git/objects`, so the 1.7 GB is mostly shared with the source repo and deleting `/root/.gpui-archive` only removes the extra links; it does not touch the source. Delete it only after you have confirmed the GitHub copy.

## 10. Open items and recommended next steps

1. Decide what to do about the 22 non-git directories (section 6), especially the `*-artifacts-*` ones.
2. Decide whether you want the `.git` bundle on this Mac (section 7).
3. Rename or keep the repo; make sure it stays private. Do not add collaborators without thinking about the org lineage.
4. The `-wip` branches are snapshots at archive time. Worktrees on prod still change; take a fresh snapshot if work continues. The same scripts can be rerun (they only read the worktrees).
5. After you confirm the archive looks right on GitHub, clean up the prod leftovers (section 9). Nothing was deleted on prod.

## Appendix A: every worktree

Archive branch is `wt/<directory>`. ⚠ marks a head whose SHA differs from prod because the branch was rewritten (section 5); the "HEAD (prod)" column is the SHA to use with prod. Full SHAs are in `MANIFEST.tsv`.

| # | Directory (on prod) | Original branch | Archive branch | HEAD (prod) | HEAD (archive) | Dirty snapshot branch | Files in snapshot |
|---|---|---|---|---|---|---|---|
| 1 | `gpui-a11y-lifecycle` | `sol/a11y-lifecycle` | `wt/gpui-a11y-lifecycle` | `9c447ad078` | `9c447ad078` | — | — |
| 2 | `gpui-accessibility-codec-control` | `accessibility-codec-control-sol` | `wt/gpui-accessibility-codec-control` | `7564ef38c1` | `7564ef38c1` | — | — |
| 3 | `gpui-accessibility-selection` | `accessibility-selection-sol` | `wt/gpui-accessibility-selection` | `5291da1310` | `5291da1310` | — | — |
| 4 | `gpui-acknowledged-text-controls` | `acknowledged-text-controls-sol` | `wt/gpui-acknowledged-text-controls` | `aa204f6768` | `aa204f6768` | — | — |
| 5 | `gpui-acknowledged-text-geometry` | `acknowledged-text-geometry-sol` | `wt/gpui-acknowledged-text-geometry` | `866e612d91` | `866e612d91` | — | — |
| 6 | `gpui-axis-samples` | `codex/axis-samples` | `wt/gpui-axis-samples` | `f43c068527` | `f43c068527` | — | — |
| 7 | `gpui-bench-luna` | *(detached)* | `wt/gpui-bench-luna` | `d91737d5ac` | `d91737d5ac` | `wt/gpui-bench-luna-wip` | 2 + 7 nested-repo records |
| 8 | `gpui-bin-miss` | `codex/bin-miss` | `wt/gpui-bin-miss` | `29a4f1c1b5` | `29a4f1c1b5` | — | — |
| 9 | `gpui-bin-miss-bounds` | *(detached)* | `wt/gpui-bin-miss-bounds` | `6a5b205350` | `6a5b205350` | `wt/gpui-bin-miss-bounds-wip` | 1 |
| 10 | `gpui-bin-miss-inline` | *(detached)* | `wt/gpui-bin-miss-inline` | `ac2ec9cb88` | `ac2ec9cb88` | `wt/gpui-bin-miss-inline-wip` | 1 |
| 11 | `gpui-bin-phase-parent` | *(detached)* | `wt/gpui-bin-phase-parent` | `b1fc101435` | `b1fc101435` | `wt/gpui-bin-phase-parent-wip` | 1 |
| 12 | `gpui-boolean-cohort-mutant-sol-20261024` | `codex/boolean-cohort-mutant-sol-20261024` | `wt/gpui-boolean-cohort-mutant-sol-20261024` | `e5f3a4c31c` | `cd5847fc10` ⚠ | — | — |
| 13 | `gpui-boolean-cohort-sol-20261024` | `codex/boolean-cohort-sol-20261024` | `wt/gpui-boolean-cohort-sol-20261024` | `a82c69a021` | `6a1cdad3a3` ⚠ | — | — |
| 14 | `gpui-bounds-baseline` | `codex/bounds-baseline` | `wt/gpui-bounds-baseline` | `b60af3a4f9` | `b60af3a4f9` | — | — |
| 15 | `gpui-bounds-kernel` | `codex/bounds-kernel` | `wt/gpui-bounds-kernel` | `e9c92d7481` | `e9c92d7481` | — | — |
| 16 | `gpui-bounds-provenance` | `codex/bounds-provenance` | `wt/gpui-bounds-provenance` | `3aa9d56c4f` | `3aa9d56c4f` | — | — |
| 17 | `gpui-bounds-provenance-baseline` | `codex/bounds-provenance-baseline` | `wt/gpui-bounds-provenance-baseline` | `9176c2ce5e` | `9176c2ce5e` | — | — |
| 18 | `gpui-caret-transaction-sol` | *(detached)* | `wt/gpui-caret-transaction-sol` | `31c4f2b037` | `31c4f2b037` | — | — |
| 19 | `gpui-ce3` | `codex/reactivity-cleanroom` | `wt/gpui-ce3` | `d464b7de9e` | `cc193f85f5` ⚠ | — | — |
| 20 | `gpui-cephal-flat-oof` | `cephal-flat-oof-sol` | `wt/gpui-cephal-flat-oof` | `81eb1be69b` | `81eb1be69b` | — | — |
| 21 | `gpui-cephal-hidden-marker` | `cephal-hidden-marker-sol` | `wt/gpui-cephal-hidden-marker` | `c7a75d376b` | `c7a75d376b` | — | — |
| 22 | `gpui-clips-sol` | `nested-clips-sol` | `wt/gpui-clips-sol` | `b9fcb13255` | `789d1250a4` ⚠ | — | — |
| 23 | `gpui-cmap-baseline` | *(detached)* | `wt/gpui-cmap-baseline` | `af0fdb837a` | `af0fdb837a` | — | — |
| 24 | `gpui-compact-parent` | `codex/compact-parent` | `wt/gpui-compact-parent` | `1e54a3731c` | `1e54a3731c` | — | — |
| 25 | `gpui-component-default-simd-sol-20261024` | `codex/component-default-simd-sol-20261024` | `wt/gpui-component-default-simd-sol-20261024` | `14006e63d0` | `6dd938c054` ⚠ | `wt/gpui-component-default-simd-sol-20261024-wip` | 25 |
| 26 | `gpui-component-ime-review-sol-20261024` | `codex/component-ime-review-sol-20261024` | `wt/gpui-component-ime-review-sol-20261024` | `91add627e5` | `077393f6c8` ⚠ | — | — |
| 27 | `gpui-component-port-sol-20261024` | `codex/component-port-sol-20261024` | `wt/gpui-component-port-sol-20261024` | `0103a0e70c` | `a380dce4ec` ⚠ | — | — |
| 28 | `gpui-component-review-sol-20261024` | `codex/component-review-sol-20261024` | `wt/gpui-component-review-sol-20261024` | `b74a2c01e1` | `4cab281f40` ⚠ | — | — |
| 29 | `gpui-contained-owner-unit-baseline` | `contained-owner-unit-baseline-sol` | `wt/gpui-contained-owner-unit-baseline` | `e6c231d90f` | `e6c231d90f` | — | — |
| 30 | `gpui-contained-property-exchange` | `contained-property-exchange-sol` | `wt/gpui-contained-property-exchange` | `5b8fbc95e5` | `5b8fbc95e5` | — | — |
| 31 | `gpui-counted-style-palette` | `codex/counted-style-palette` | `wt/gpui-counted-style-palette` | `19345abf5a` | `19345abf5a` | — | — |
| 32 | `gpui-current-main-bench` | *(detached)* | `wt/gpui-current-main-bench` | `dcdadf4d52` | `dcdadf4d52` | — | — |
| 33 | `gpui-current-native-bench` | *(detached)* | `wt/gpui-current-native-bench` | `724ece73cb` | `724ece73cb` | — | — |
| 34 | `gpui-cursor-free` | `codex/cursor-free-raster` | `wt/gpui-cursor-free` | `96f4dff27c` | `96f4dff27c` | `wt/gpui-cursor-free-wip` | 1 |
| 35 | `gpui-deferred-xim-sol` | `deferred-xim-sol` | `wt/gpui-deferred-xim-sol` | `0afe46628b` | `0afe46628b` | — | — |
| 36 | `gpui-designer-gamut` | `designer-gamut-sol` | `wt/gpui-designer-gamut` | `86d0effdca` | `86d0effdca` | — | — |
| 37 | `gpui-empty-floor-88` | *(detached)* | `wt/gpui-empty-floor-88` | `146e4f307a` | `146e4f307a` | — | — |
| 38 | `gpui-external-params-sol` | `codex/lazy-external-params-sol` | `wt/gpui-external-params-sol` | `5a7187ad8c` | `5a7187ad8c` | — | — |
| 39 | `gpui-fearless-abi` | `fearless-uniform-output-sol` | `wt/gpui-fearless-abi` | `c8df36cb5d` | `c8df36cb5d` | — | — |
| 40 | `gpui-fearless-geometry` | `fearless-presence-geometry-sol` | `wt/gpui-fearless-geometry` | `e19f0a1049` | `e19f0a1049` | — | — |
| 41 | `gpui-fearless-pipeline` | `fearless-soa-sol` | `wt/gpui-fearless-pipeline` | `846dd17ad0` | `846dd17ad0` | — | — |
| 42 | `gpui-flex-ports` | `codex/flex-ports` | `wt/gpui-flex-ports` | `1f2ceb4581` | `1f2ceb4581` | — | — |
| 43 | `gpui-future-control-baseline` | *(detached)* | `wt/gpui-future-control-baseline` | `8158386886` | `8158386886` | — | — |
| 44 | `gpui-future-sol` | `future-adapter-sol` | `wt/gpui-future-sol` | `ea4f064208` | `ea4f064208` | — | — |
| 45 | `gpui-gamut-adaptive` | `designer-gamut-adaptive-sol` | `wt/gpui-gamut-adaptive` | `f2bf1655ae` | `f2bf1655ae` | — | — |
| 46 | `gpui-gamut-lattice` | `designer-gamut-lattice-sol` | `wt/gpui-gamut-lattice` | `c7514aae50` | `c7514aae50` | — | — |
| 47 | `gpui-glyph-baseline` | *(detached)* | `wt/gpui-glyph-baseline` | `62f6841939` | `62f6841939` | — | — |
| 48 | `gpui-gpu-packet-baseline` | *(detached)* | `wt/gpui-gpu-packet-baseline` | `112f6cf53d` | `112f6cf53d` | `wt/gpui-gpu-packet-baseline-wip` | 1 |
| 49 | `gpui-gpu-packet-reuse` | `codex/gpu-packet-reuse` | `wt/gpui-gpu-packet-reuse` | `def945ff98` | `def945ff98` | — | — |
| 50 | `gpui-hit-candidate` | `codex/hit-candidate-byte` | `wt/gpui-hit-candidate` | `33a282000f` | `33a282000f` | `wt/gpui-hit-candidate-wip` | 1 |
| 51 | `gpui-hit-links` | `codex/packed-presentation-links` | `wt/gpui-hit-links` | `50a082654d` | `50a082654d` | `wt/gpui-hit-links-wip` | 1 |
| 52 | `gpui-hit-parent` | `codex/hit-run-parent` | `wt/gpui-hit-parent` | `de18f040c9` | `de18f040c9` | `wt/gpui-hit-parent-wip` | 2 |
| 53 | `gpui-hit-runs` | `codex/hit-run-proof` | `wt/gpui-hit-runs` | `966a13ff7d` | `966a13ff7d` | `wt/gpui-hit-runs-wip` | 1 |
| 54 | `gpui-hummingbird-audio-sol` | `codex/hummingbird-audio-sol` | `wt/gpui-hummingbird-audio-sol` | `0fc824ba78` | `0fc824ba78` | — | — |
| 55 | `gpui-hummingbird-browser-allocation-sol` | `codex/hummingbird-known-width-sol` | `wt/gpui-hummingbird-browser-allocation-sol` | `02638ee5fa` | `02638ee5fa` | — | — |
| 56 | `gpui-hummingbird-browser-compose-sol` | *(detached)* | `wt/gpui-hummingbird-browser-compose-sol` | `79a4edd63c` | `12ffdd48de` ⚠ | — | — |
| 57 | `gpui-hummingbird-browser-negative-control-sol` | *(detached)* | `wt/gpui-hummingbird-browser-negative-control-sol` | `730b482d02` | `730b482d02` | — | — |
| 58 | `gpui-hummingbird-browser-sol` | `codex/hummingbird-browser-sol` | `wt/gpui-hummingbird-browser-sol` | `dfc4f4624f` | `dfc4f4624f` | — | — |
| 59 | `gpui-hummingbird-eos-authority-sol` | `codex/hummingbird-eos-authority-sol` | `wt/gpui-hummingbird-eos-authority-sol` | `9145411365` | `9145411365` | — | — |
| 60 | `gpui-hummingbird-files-sol` | `codex/hummingbird-files-sol` | `wt/gpui-hummingbird-files-sol` | `1df152ea25` | `1df152ea25` | — | — |
| 61 | `gpui-hummingbird-fresh-schema-sol` | `codex/hummingbird-fresh-schema-sol` | `wt/gpui-hummingbird-fresh-schema-sol` | `db72fb087d` | `fa386ada79` ⚠ | — | — |
| 62 | `gpui-hummingbird-library-intent-sol` | `codex/hummingbird-library-intent-sol` | `wt/gpui-hummingbird-library-intent-sol` | `21f82e28b5` | `21f82e28b5` | — | — |
| 63 | `gpui-hummingbird-library-oneclick-sol` | `codex/hummingbird-library-oneclick-sol` | `wt/gpui-hummingbird-library-oneclick-sol` | `1cb7b7d5e6` | `1cb7b7d5e6` | — | — |
| 64 | `gpui-hummingbird-library-play-sol` | `codex/hummingbird-library-play-sol` | `wt/gpui-hummingbird-library-play-sol` | `e8ad0dab98` | `e8ad0dab98` | — | — |
| 65 | `gpui-hummingbird-main-port-sol` | `codex/hummingbird-main-port-sol` | `wt/gpui-hummingbird-main-port-sol` | `736afe8bfb` | `736afe8bfb` | — | — |
| 66 | `gpui-hummingbird-one-engine-sol` | `codex/hummingbird-one-engine-sol` | `wt/gpui-hummingbird-one-engine-sol` | `282b1c09b9` | `282b1c09b9` | — | — |
| 67 | `gpui-hummingbird-port-authority-sol` | `codex/hummingbird-port-authority-sol` | `wt/gpui-hummingbird-port-authority-sol` | `b1d2a8d849` | `b1d2a8d849` | — | — |
| 68 | `gpui-hummingbird-source-authority-sol` | `codex/hummingbird-source-authority-sol` | `wt/gpui-hummingbird-source-authority-sol` | `b31b34475e` | `b31b34475e` | — | — |
| 69 | `gpui-hummingbird-stop-publication-sol` | `codex/hummingbird-stop-publication-sol` | `wt/gpui-hummingbird-stop-publication-sol` | `33eeef116a` | `33eeef116a` | — | — |
| 70 | `gpui-hummingbird-transport-control-sol` | *(detached)* | `wt/gpui-hummingbird-transport-control-sol` | `e62a4ae5ae` | `e62a4ae5ae` | — | — |
| 71 | `gpui-identity-hole` | `codex/identity-hole` | `wt/gpui-identity-hole` | `2eae1a93fb` | `2eae1a93fb` | `wt/gpui-identity-hole-wip` | 11 |
| 72 | `gpui-identity-hole-hot` | `codex/identity-hole-hot` | `wt/gpui-identity-hole-hot` | `412c476b69` | `412c476b69` | `wt/gpui-identity-hole-hot-wip` | 1 |
| 73 | `gpui-identity-index` | `codex/exact-identity-index` | `wt/gpui-identity-index` | `582bca6387` | `582bca6387` | `wt/gpui-identity-index-wip` | 2 |
| 74 | `gpui-identity-index-base` | *(detached)* | `wt/gpui-identity-index-base` | `59467a4136` | `59467a4136` | `wt/gpui-identity-index-base-wip` | 2 |
| 75 | `gpui-identity-inline` | `codex/identity-inline` | `wt/gpui-identity-inline` | `ab7925fd66` | `ab7925fd66` | `wt/gpui-identity-inline-wip` | 1 |
| 76 | `gpui-identity-order-baseline` | *(detached)* | `wt/gpui-identity-order-baseline` | `b15133e90d` | `b15133e90d` | — | — |
| 77 | `gpui-identity-runs` | `codex/identity-runs` | `wt/gpui-identity-runs` | `75544537f7` | `75544537f7` | `wt/gpui-identity-runs-wip` | 2 |
| 78 | `gpui-image-axis-baseline` | `production/image-axis-baseline` | `wt/gpui-image-axis-baseline` | `724ece73cb` | `724ece73cb` | `wt/gpui-image-axis-baseline-wip` | 1 |
| 79 | `gpui-images-baseline` | *(detached)* | `wt/gpui-images-baseline` | `5b0a38b222` | `5b0a38b222` | — | — |
| 80 | `gpui-inspector-contained-owner` | `inspector-contained-owner-sol` | `wt/gpui-inspector-contained-owner` | `5ffbe09e18` | `5ffbe09e18` | `wt/gpui-inspector-contained-owner-wip` | 311 |
| 81 | `gpui-inspector-ui-sol` | `inspector-ui-sol` | `wt/gpui-inspector-ui-sol` | `f39428c37a` | `f39428c37a` | — | — |
| 82 | `gpui-layout-route` | `codex/layout-route` | `wt/gpui-layout-route` | `3f440fdd56` | `3f440fdd56` | — | — |
| 83 | `gpui-lazy-raster-bins` | `codex/lazy-raster-bins` | `wt/gpui-lazy-raster-bins` | `3724ece72f` | `3724ece72f` | `wt/gpui-lazy-raster-bins-wip` | 1 |
| 84 | `gpui-length-port-sol-20261024` | `codex/typed-length-port-sol-20261024` | `wt/gpui-length-port-sol-20261024` | `c31dd34833` | `2c1a3c2ae4` ⚠ | — | — |
| 85 | `gpui-lifecycle-floor-research` | `codex/lifecycle-floor-research` | `wt/gpui-lifecycle-floor-research` | `21b1eaf4e8` | `21b1eaf4e8` | — | — |
| 86 | `gpui-literal-dispatch` | `codex/literal-dispatch` | `wt/gpui-literal-dispatch` | `8560675273` | `8560675273` | `wt/gpui-literal-dispatch-wip` | 1 |
| 87 | `gpui-literal-dispatch-base` | *(detached)* | `wt/gpui-literal-dispatch-base` | `09a9a229f1` | `09a9a229f1` | `wt/gpui-literal-dispatch-base-wip` | 1 |
| 88 | `gpui-lowered-solid-fault` | `spike/lowered-solid-fault` | `wt/gpui-lowered-solid-fault` | `6b730a0edf` | `8ac297ca32` ⚠ | — | — |
| 89 | `gpui-lowered-solid-sol` | `spike/lowered-solid-sampler` | `wt/gpui-lowered-solid-sol` | `7ccc65d267` | `f0f39dcf92` ⚠ | — | — |
| 90 | `gpui-luna-formulation` | *(detached)* | `wt/gpui-luna-formulation` | `a6addf6994` | `a6addf6994` | — | — |
| 91 | `gpui-migration-tool` | `migration-tool-sol` | `wt/gpui-migration-tool` | `7a1e6a0d1c` | `7a1e6a0d1c` | — | — |
| 92 | `gpui-monotonic-caret-baseline` | *(detached)* | `wt/gpui-monotonic-caret-baseline` | `1d5bc3061c` | `1d5bc3061c` | — | — |
| 93 | `gpui-monotonic-caret-borrowed` | `monotonic-caret-borrowed-sol` | `wt/gpui-monotonic-caret-borrowed` | `61ccf6ed4c` | `61ccf6ed4c` | — | — |
| 94 | `gpui-monotonic-caret-boundary` | `monotonic-caret-boundary-sol` | `wt/gpui-monotonic-caret-boundary` | `3582a9e865` | `3582a9e865` | — | — |
| 95 | `gpui-monotonic-caret-eight` | `monotonic-caret-eight-sol` | `wt/gpui-monotonic-caret-eight` | `c3acaf4681` | `c3acaf4681` | — | — |
| 96 | `gpui-monotonic-caret-inline` | `monotonic-caret-inline-sol` | `wt/gpui-monotonic-caret-inline` | `cca9061a22` | `cca9061a22` | — | — |
| 97 | `gpui-monotonic-caret-shared-pair` | `monotonic-caret-shared-pair-sol` | `wt/gpui-monotonic-caret-shared-pair` | `176da955a2` | `176da955a2` | — | — |
| 98 | `gpui-monotonic-caret-stream` | `monotonic-caret-stream-sol` | `wt/gpui-monotonic-caret-stream` | `2bba0323c1` | `2bba0323c1` | — | — |
| 99 | `gpui-opaque-proof` | `codex/opaque-color-proof` | `wt/gpui-opaque-proof` | `ff9072eb07` | `ff9072eb07` | — | — |
| 100 | `gpui-option-lookup` | `codex/option-lookup` | `wt/gpui-option-lookup` | `ddfc53c588` | `ddfc53c588` | — | — |
| 101 | `gpui-option-lookup-b25-baseline` | *(detached)* | `wt/gpui-option-lookup-b25-baseline` | `b25b45f2c1` | `b25b45f2c1` | — | — |
| 102 | `gpui-option-lookup-original-baseline` | *(detached)* | `wt/gpui-option-lookup-original-baseline` | `b15133e90d` | `b15133e90d` | — | — |
| 103 | `gpui-paint-5f4` | *(detached)* | `wt/gpui-paint-5f4` | `5f4db5f288` | `5f4db5f288` | `wt/gpui-paint-5f4-wip` | 2 |
| 104 | `gpui-paint-coupling` | `codex/paint-coupling` | `wt/gpui-paint-coupling` | `00f3fcbe07` | `00f3fcbe07` | — | — |
| 105 | `gpui-paint-literal-cow` | `codex/paint-literal-cow` | `wt/gpui-paint-literal-cow` | `c6e81a18b4` | `c6e81a18b4` | — | — |
| 106 | `gpui-paint-literal-cow-control` | `codex/paint-literal-cow-control` | `wt/gpui-paint-literal-cow-control` | `4681430fef` | `4681430fef` | — | — |
| 107 | `gpui-paint-patch` | `codex/paint-patch` | `wt/gpui-paint-patch` | `e71d8b0c2e` | `e71d8b0c2e` | — | — |
| 108 | `gpui-paint-slot-rebind` | `codex/paint-slot-rebind` | `wt/gpui-paint-slot-rebind` | `b61120c24e` | `b61120c24e` | — | — |
| 109 | `gpui-paint-slot-refinement` | `codex/paint-slot-refinement` | `wt/gpui-paint-slot-refinement` | `854406bb5f` | `854406bb5f` | — | — |
| 110 | `gpui-paint-slots` | `codex/paint-slots` | `wt/gpui-paint-slots` | `ef631b11a6` | `ef631b11a6` | — | — |
| 111 | `gpui-paint-slots-control` | `codex/paint-slots-control` | `wt/gpui-paint-slots-control` | `e01acb28cc` | `e01acb28cc` | — | — |
| 112 | `gpui-pan-viewports` | `codex/pan-viewports` | `wt/gpui-pan-viewports` | `75ae8f35e1` | `75ae8f35e1` | — | — |
| 113 | `gpui-persistent-damage` | `codex/persistent-damage` | `wt/gpui-persistent-damage` | `0c050e0147` | `0c050e0147` | — | — |
| 114 | `gpui-presented-spaces` | `codex/presented-space-lanes` | `wt/gpui-presented-spaces` | `2144b3f7ba` | `2144b3f7ba` | — | — |
| 115 | `gpui-production-accessibility-sparse` | `production/accessibility-sparse` | `wt/gpui-production-accessibility-sparse` | `01c6e858b7` | `01c6e858b7` | — | — |
| 116 | `gpui-production-accesskit` | `production/accesskit` | `wt/gpui-production-accesskit` | `7aa7388d29` | `7aa7388d29` | — | — |
| 117 | `gpui-production-canvas` | `production/canvas-parallel` | `wt/gpui-production-canvas` | `5249fd4f3b` | `5249fd4f3b` | — | — |
| 118 | `gpui-production-cmap` | `production/cached-cmap` | `wt/gpui-production-cmap` | `9914602302` | `9914602302` | — | — |
| 119 | `gpui-production-color` | `production/color` | `wt/gpui-production-color` | `35b7933c37` | `35b7933c37` | `wt/gpui-production-color-wip` | 6 |
| 120 | `gpui-production-color-packing` | `production/color-packing` | `wt/gpui-production-color-packing` | `25e37db992` | `25e37db992` | — | — |
| 121 | `gpui-production-components` | `production/components` | `wt/gpui-production-components` | `d331959adf` | `d331959adf` | `wt/gpui-production-components-wip` | 1 |
| 122 | `gpui-production-elements` | `codex/gpui-production-elements` | `wt/gpui-production-elements` | `9db0b905a8` | `9db0b905a8` | `wt/gpui-production-elements-wip` | 16 |
| 123 | `gpui-production-focus` | `production/focus` | `wt/gpui-production-focus` | `afacb3a0b8` | `afacb3a0b8` | — | — |
| 124 | `gpui-production-font` | `production/font-lazy` | `wt/gpui-production-font` | `99b788db26` | `99b788db26` | — | — |
| 125 | `gpui-production-gpu` | `production/gpu-review` | `wt/gpui-production-gpu` | `b0f67d97f3` | `b0f67d97f3` | `wt/gpui-production-gpu-wip` | 2 |
| 126 | `gpui-production-gpu-runs` | `production/gpu-runs` | `wt/gpui-production-gpu-runs` | `02e58d768f` | `02e58d768f` | — | — |
| 127 | `gpui-production-image-axis` | `production/image-axis` | `wt/gpui-production-image-axis` | `29250fb8db` | `29250fb8db` | — | — |
| 128 | `gpui-production-images` | `production/images` | `wt/gpui-production-images` | `1ecc3a48ca` | `1ecc3a48ca` | — | — |
| 129 | `gpui-production-library-cancellation` | `codex/library-cancellation` | `wt/gpui-production-library-cancellation` | `7477106ed5` | `7477106ed5` | — | — |
| 130 | `gpui-production-list` | `production/virtual-list` | `wt/gpui-production-list` | `8610638d2e` | `8610638d2e` | — | — |
| 131 | `gpui-production-native-invalidation` | `production/native-invalidation` | `wt/gpui-production-native-invalidation` | `58daaa471c` | `58daaa471c` | — | — |
| 132 | `gpui-production-native-upgrade` | `production/native-upgrade` | `wt/gpui-production-native-upgrade` | `d4c80b2c7c` | `d4c80b2c7c` | `wt/gpui-production-native-upgrade-wip` | 16 |
| 133 | `gpui-production-packet` | `production/packet` | `wt/gpui-production-packet` | `fe4125e4d8` | `fe4125e4d8` | — | — |
| 134 | `gpui-production-presentation` | `production/presentation` | `wt/gpui-production-presentation` | `294de32654` | `294de32654` | `wt/gpui-production-presentation-wip` | 4 |
| 135 | `gpui-production-presentation-layers` | `production/presentation-layers` | `wt/gpui-production-presentation-layers` | `cee39dffa8` | `cee39dffa8` | — | — |
| 136 | `gpui-production-question-mailbox` | `production/question-mailbox` | `wt/gpui-production-question-mailbox` | `e8017da488` | `e8017da488` | — | — |
| 137 | `gpui-production-question-sealed` | `codex/question-sealed-staging` | `wt/gpui-production-question-sealed` | `8fb2c37625` | `8fb2c37625` | — | — |
| 138 | `gpui-production-reader` | `production/reader-question` | `wt/gpui-production-reader` | `c6ac1481d1` | `c6ac1481d1` | — | — |
| 139 | `gpui-production-remote` | `production/remote` | `wt/gpui-production-remote` | `fdfc505817` | `fdfc505817` | — | — |
| 140 | `gpui-production-remote-guard` | `production/remote-guard` | `wt/gpui-production-remote-guard` | `6fbdfe013a` | `6fbdfe013a` | — | — |
| 141 | `gpui-production-runs` | `production/canvas-runs` | `wt/gpui-production-runs` | `ef8df5e3c4` | `ef8df5e3c4` | — | — |
| 142 | `gpui-production-scroll` | `production/scroll-viewports` | `wt/gpui-production-scroll` | `ed0e53c053` | `ed0e53c053` | — | — |
| 143 | `gpui-production-semantics` | `production/semantic-metadata` | `wt/gpui-production-semantics` | `2cc36c6842` | `2cc36c6842` | — | — |
| 144 | `gpui-production-solid` | `production/native-solid` | `wt/gpui-production-solid` | `73c5e7261d` | `73c5e7261d` | `wt/gpui-production-solid-wip` | 3 |
| 145 | `gpui-production-sources` | `production-typed-sources` | `wt/gpui-production-sources` | `34c4c0c35d` | `34c4c0c35d` | — | — |
| 146 | `gpui-production-thin-text` | `production/thin-text` | `wt/gpui-production-thin-text` | `f7d09d7af2` | `f7d09d7af2` | — | — |
| 147 | `gpui-production-unicode` | `production/unicode-text` | `wt/gpui-production-unicode` | `e1dc99e07a` | `e1dc99e07a` | — | — |
| 148 | `gpui-property-change-union` | `property-change-union-sol` | `wt/gpui-property-change-union` | `1950605a93` | `1950605a93` | — | — |
| 149 | `gpui-question-channel-baseline` | *(detached)* | `wt/gpui-question-channel-baseline` | `c6ac1481d1` | `c6ac1481d1` | `wt/gpui-question-channel-baseline-wip` | 1 |
| 150 | `gpui-question-inputs` | `codex/question-inputs` | `wt/gpui-question-inputs` | `0eac384ddf` | `0eac384ddf` | — | — |
| 151 | `gpui-question-inputs-base` | *(detached)* | `wt/gpui-question-inputs-base` | `724ece73cb` | `724ece73cb` | `wt/gpui-question-inputs-base-wip` | 1 |
| 152 | `gpui-raster-borrowed-rows` | `codex/borrowed-raster-rows` | `wt/gpui-raster-borrowed-rows` | `aed7aa1160` | `aed7aa1160` | `wt/gpui-raster-borrowed-rows-wip` | 1 |
| 153 | `gpui-raster-cuts` | `codex/raster-cut-ownership` | `wt/gpui-raster-cuts` | `d50ba094b5` | `d50ba094b5` | `wt/gpui-raster-cuts-wip` | 1 |
| 154 | `gpui-raster-narrow` | `codex/raster-narrow-indices` | `wt/gpui-raster-narrow` | `25d851b380` | `25d851b380` | `wt/gpui-raster-narrow-wip` | 1 |
| 155 | `gpui-reactive-styles` | `codex/reactive-styles` | `wt/gpui-reactive-styles` | `b6f71b4a3a` | `b6f71b4a3a` | — | — |
| 156 | `gpui-reactivity-algebra` | `spike/reactivity-algebra` | `wt/gpui-reactivity-algebra` | `8799d34dbe` | `8799d34dbe` | — | — |
| 157 | `gpui-reactivity-api` | `codex/reactivity-api-proof` | `wt/gpui-reactivity-api` | `bb9bbfe2e1` | `bb9bbfe2e1` | — | — |
| 158 | `gpui-reactivity-coverage` | `spike/reactivity-coverage` | `wt/gpui-reactivity-coverage` | `e63bf8cc8f` | `e63bf8cc8f` | `wt/gpui-reactivity-coverage-wip` | 1 |
| 159 | `gpui-reactivity-state` | `spike/reactivity-state` | `wt/gpui-reactivity-state` | `2756d0c9b0` | `2756d0c9b0` | — | — |
| 160 | `gpui-reactivity-templates` | `spike/reactivity-templates` | `wt/gpui-reactivity-templates` | `d1ade9996d` | `d1ade9996d` | `wt/gpui-reactivity-templates-wip` | 1 |
| 161 | `gpui-reactivity-tree` | `spike/reactivity-layout` | `wt/gpui-reactivity-tree` | `2014cdf6d3` | `2014cdf6d3` | `wt/gpui-reactivity-tree-wip` | 1 |
| 162 | `gpui-remote-owner-main-rehearsal` | `codex/remote-owner-main-rehearsal` | `wt/gpui-remote-owner-main-rehearsal` | `680adc1c16` | `680adc1c16` | — | — |
| 163 | `gpui-resolved-index` | `codex/resolve-target-once` | `wt/gpui-resolved-index` | `cf1e7b8982` | `cf1e7b8982` | — | — |
| 164 | `gpui-retained-style-baseline` | `codex/retained-style-baseline` | `wt/gpui-retained-style-baseline` | `a7b0c7b08d` | `a7b0c7b08d` | — | — |
| 165 | `gpui-retained-styles` | `codex/retained-style-lanes` | `wt/gpui-retained-styles` | `9d417687d3` | `9d417687d3` | — | — |
| 166 | `gpui-rounded-rectangles` | `codex/rounded-rectangles` | `wt/gpui-rounded-rectangles` | `1617d42af3` | `1617d42af3` | — | — |
| 167 | `gpui-route-baseline` | *(detached)* | `wt/gpui-route-baseline` | `40382ed1d9` | `40382ed1d9` | — | — |
| 168 | `gpui-route-columns` | `codex/retained-route-split-word` | `wt/gpui-route-columns` | `9ad0d69f13` | `9ad0d69f13` | — | — |
| 169 | `gpui-route-columns-baseline` | *(detached)* | `wt/gpui-route-columns-baseline` | `89aeabd539` | `89aeabd539` | `wt/gpui-route-columns-baseline-wip` | 4 |
| 170 | `gpui-route-profile-baseline` | *(detached)* | `wt/gpui-route-profile-baseline` | `40382ed1d9` | `40382ed1d9` | `wt/gpui-route-profile-baseline-wip` | 3 |
| 171 | `gpui-route-profile-candidate` | *(detached)* | `wt/gpui-route-profile-candidate` | `c8c84c15be` | `c8c84c15be` | `wt/gpui-route-profile-candidate-wip` | 3 |
| 172 | `gpui-sample-init` | `codex/lazy-sample-initialization` | `wt/gpui-sample-init` | `349d229d4e` | `349d229d4e` | `wt/gpui-sample-init-wip` | 1 |
| 173 | `gpui-scroll-baseline` | `production/scroll-baseline` | `wt/gpui-scroll-baseline` | `dcdadf4d52` | `dcdadf4d52` | `wt/gpui-scroll-baseline-wip` | 7 |
| 174 | `gpui-semantic-stream` | `codex/pointer-protocol` | `wt/gpui-semantic-stream` | `c6db932473` | `c6db932473` | — | — |
| 175 | `gpui-shape-baseline` | *(detached)* | `wt/gpui-shape-baseline` | `0c3dd8b3e1` | `0c3dd8b3e1` | `wt/gpui-shape-baseline-wip` | 1 |
| 176 | `gpui-shape-final` | `codex/shape-final` | `wt/gpui-shape-final` | `a0ac99fad4` | `a0ac99fad4` | — | — |
| 177 | `gpui-shape-storage` | `codex/shape-storage` | `wt/gpui-shape-storage` | `adcb132de7` | `adcb132de7` | `wt/gpui-shape-storage-wip` | 4 |
| 178 | `gpui-simd-formulations` | `codex/simd-formulations-luna` | `wt/gpui-simd-formulations` | `6b4957677a` | `6b4957677a` | — | — |
| 179 | `gpui-spaces-baseline` | `codex/presented-space-baseline` | `wt/gpui-spaces-baseline` | `5c912171c0` | `5c912171c0` | — | — |
| 180 | `gpui-style-batch` | `codex/style-batch` | `wt/gpui-style-batch` | `351337ae59` | `351337ae59` | — | — |
| 181 | `gpui-style-batch-counted` | *(detached)* | `wt/gpui-style-batch-counted` | `642aa715ec` | `642aa715ec` | `wt/gpui-style-batch-counted-wip` | 2 |
| 182 | `gpui-style-dimensions` | `codex/style-dimensions` | `wt/gpui-style-dimensions` | `067eebce12` | `067eebce12` | `wt/gpui-style-dimensions-wip` | 6 |
| 183 | `gpui-style-startup-base` | *(detached)* | `wt/gpui-style-startup-base` | `f01947e679` | `f01947e679` | `wt/gpui-style-startup-base-wip` | 1 |
| 184 | `gpui-style-startup-current` | *(detached)* | `wt/gpui-style-startup-current` | `b6f71b4a3a` | `b6f71b4a3a` | `wt/gpui-style-startup-current-wip` | 3 |
| 185 | `gpui-text-batch-break` | `text-batch-break-sol` | `wt/gpui-text-batch-break` | `0d8165d452` | `0d8165d452` | — | — |
| 186 | `gpui-text-checkpoint-reuse` | `text-checkpoint-reuse-sol` | `wt/gpui-text-checkpoint-reuse` | `bd8319039a` | `bd8319039a` | — | — |
| 187 | `gpui-text-control` | `text-transaction-control` | `wt/gpui-text-control` | `02b72aad06` | `8e49a026fe` ⚠ | — | — |
| 188 | `gpui-text-map-12` | `text-map-12-sol` | `wt/gpui-text-map-12` | `ce2816b3f2` | `ce2816b3f2` | — | — |
| 189 | `gpui-text-map-12-line-reuse` | `text-map-12-line-reuse-sol` | `wt/gpui-text-map-12-line-reuse` | `eeeeccd9a5` | `eeeeccd9a5` | — | — |
| 190 | `gpui-text-map-12-query` | `text-map-12-query-sol` | `wt/gpui-text-map-12-query` | `4b864b6b2f` | `4b864b6b2f` | — | — |
| 191 | `gpui-text-map-8` | `text-map-8-sol` | `wt/gpui-text-map-8` | `068497ed7e` | `068497ed7e` | — | — |
| 192 | `gpui-text-map-8-direct` | `text-map-8-direct-sol` | `wt/gpui-text-map-8-direct` | `f515b1dcfc` | `f515b1dcfc` | — | — |
| 193 | `gpui-text-map-8-fused` | `text-map-8-fused-sol` | `wt/gpui-text-map-8-fused` | `26e590b196` | `26e590b196` | — | — |
| 194 | `gpui-text-map-capacity` | `text-map-capacity-sol` | `wt/gpui-text-map-capacity` | `32e8ee1284` | `32e8ee1284` | — | — |
| 195 | `gpui-text-query-baseline` | *(detached)* | `wt/gpui-text-query-baseline` | `c36685f68a` | `c36685f68a` | `wt/gpui-text-query-baseline-wip` | 1 |
| 196 | `gpui-text-selection` | `text-selection-sol` | `wt/gpui-text-selection` | `c7d00c2444` | `c7d00c2444` | — | — |
| 197 | `gpui-text-selection-cut` | `text-selection-cut-sol` | `wt/gpui-text-selection-cut` | `92cb6c14a7` | `92cb6c14a7` | — | — |
| 198 | `gpui-text-selection-editor` | `text-selection-editor-sol` | `wt/gpui-text-selection-editor` | `7521137891` | `7521137891` | — | — |
| 199 | `gpui-text-selection-editor-control` | *(detached)* | `wt/gpui-text-selection-editor-control` | `87ef973963` | `87ef973963` | `wt/gpui-text-selection-editor-control-wip` | 1 |
| 200 | `gpui-text-sol` | `text-transaction-sol` | `wt/gpui-text-sol` | `4e6bb2971c` | `6345e171df` ⚠ | — | — |
| 201 | `gpui-thin-text-baseline` | `production/thin-text-baseline` | `wt/gpui-thin-text-baseline` | `d88f1a3f82` | `d88f1a3f82` | `wt/gpui-thin-text-baseline-wip` | 5 |
| 202 | `gpui-typed-binding-mutant-sol-20261024` | `codex/typed-binding-mutant-sol-20261024` | `wt/gpui-typed-binding-mutant-sol-20261024` | `d4563f3796` | `445dda771b` ⚠ | — | — |
| 203 | `gpui-typed-binding-sol-20261024` | `codex/typed-binding-sol-20261024` | `wt/gpui-typed-binding-sol-20261024` | `9e6d1a5f65` | `49ab34dd90` ⚠ | — | — |
| 204 | `gpui-typed-catalog` | `typed-catalog-sol` | `wt/gpui-typed-catalog` | `2e133e4a23` | `2e133e4a23` | — | — |
| 205 | `gpui-unicode-baseline` | *(detached)* | `wt/gpui-unicode-baseline` | `146cd9912d` | `146cd9912d` | `wt/gpui-unicode-baseline-wip` | 3 |
| 206 | `gpui-validated-lookup` | `codex/validated-lookup` | `wt/gpui-validated-lookup` | `354186bb49` | `354186bb49` | — | — |
| 207 | `gpui-validated-sites` | `codex/validated-sites` | `wt/gpui-validated-sites` | `9ec9655d03` | `9ec9655d03` | — | — |
| 208 | `gpui-wgpu30` | `codex/wgpu30` | `wt/gpui-wgpu30` | `2d84fa33a9` | `2d84fa33a9` | — | — |
| 209 | `gpui-wgsl-buildtime` | `wgsl-buildtime-sol` | `wt/gpui-wgsl-buildtime` | `6d2973774a` | `6d2973774a` | — | — |
| 210 | `gpui-x11-ime` | `luna/x11-ime-floor` | `wt/gpui-x11-ime` | `9edb9a6ee0` | `9edb9a6ee0` | — | — |
| 211 | `gpui-zbus6-atspi` | `zbus6-atspi-reviewed-sol` | `wt/gpui-zbus6-atspi` | `c667d28a4c` | `c667d28a4c` | — | — |

## Appendix B: refs whose SHA differs from prod (103)

Only the SHAs differ; trees (apart from the two replaced files), authors, dates and messages are identical. Full SHAs are in `REFMAP.tsv`.

| Ref | Prod SHA | Archive SHA |
|---|---|---|
| `refs/hb-fresh/heads/codex/hummingbird-fresh-schema-sol` | `db72fb087d` | `fa386ada79` |
| `refs/hb-fresh/heads/master` | `64162234a0` | `307c3a03dc` |
| `refs/heads/clips-control-sol` | `3205619ca9` | `cb7cb20040` |
| `refs/heads/codex/a11y-close-composition-root` | `34e9290b7e` | `0ba007cdaa` |
| `refs/heads/codex/boolean-cohort-mutant-sol-20261024` | `e5f3a4c31c` | `cd5847fc10` |
| `refs/heads/codex/boolean-cohort-sol-20261024` | `a82c69a021` | `6a1cdad3a3` |
| `refs/heads/codex/boolean-cohort-sol-qualified-31` | `31a6496bf7` | `225246d646` |
| `refs/heads/codex/boxed-text-shipping-root` | `37d0321fe2` | `10aef1d7d3` |
| `refs/heads/codex/component-default-simd-sol-20261024` | `14006e63d0` | `6dd938c054` |
| `refs/heads/codex/component-ime-initial-control-sol-20261024` | `21fd5a9c16` | `863dc4ad82` |
| `refs/heads/codex/component-ime-review-sol-20261024` | `91add627e5` | `077393f6c8` |
| `refs/heads/codex/component-port-sol-20261024` | `0103a0e70c` | `a380dce4ec` |
| `refs/heads/codex/component-review-sol-20261024` | `b74a2c01e1` | `4cab281f40` |
| `refs/heads/codex/heavy-recording-root-20261024` | `fe4e535c40` | `bc3a4f3b91` |
| `refs/heads/codex/ime-ascii-restart-root-20261024` | `4b2e6d3df7` | `0c3892b41a` |
| `refs/heads/codex/ime-composed-review-root-20261024` | `6305951748` | `a86109b0f4` |
| `refs/heads/codex/reactivity-cleanroom` | `d464b7de9e` | `cc193f85f5` |
| `refs/heads/codex/simd-lattice-root` | `c418c95e22` | `2e739084f1` |
| `refs/heads/codex/spike-child-sizing-mutant-root` | `36f4877ebb` | `322a628596` |
| `refs/heads/codex/spike-component-outer-scope-mutant-root` | `c9617ea5c1` | `f2b428054a` |
| `refs/heads/codex/todo-phase-oracle-root-20261024` | `87395d77e1` | `d524c72619` |
| `refs/heads/codex/typed-binding-mutant-sol-20261024` | `d4563f3796` | `445dda771b` |
| `refs/heads/codex/typed-binding-sol-20261024` | `9e6d1a5f65` | `49ab34dd90` |
| `refs/heads/codex/typed-length-port-sol-20261024` | `c31dd34833` | `2c1a3c2ae4` |
| `refs/heads/fault/banked-row-root` | `a39c766dd8` | `d332cc051a` |
| `refs/heads/flat-text-sol-20261024` | `ca9f00a65d` | `0a3d51b50c` |
| `refs/heads/gradient-bank-audit-base-e0c` | `e0c8701ade` | `4b12f59710` |
| `refs/heads/gradient-bank-audit-before-684` | `6843118d39` | `b1b53d34d5` |
| `refs/heads/gradient-bank-rejected-finalizer-c197` | `c1977dd9b6` | `53f0185680` |
| `refs/heads/gradient-bank-scratch-a2dd` | `a2dd48e908` | `21d030c3e9` |
| `refs/heads/gradient-bank-solid-fb97` | `fb97b82078` | `b5a1177d44` |
| `refs/heads/gradient-lease-audit-negative` | `8669466b1e` | `5aa7eb5c38` |
| `refs/heads/gradient-lease-audit-positive` | `ea574d759c` | `d508a024b5` |
| `refs/heads/ime-composition-review-sol-20261024` | `b21e9c7d47` | `358440dbc8` |
| `refs/heads/ime-composition-sol-tested-a623-20261024` | `a623ee8e3f` | `1d3e6bbd6b` |
| `refs/heads/ime-direct-lease-sol-20261024` | `4db013697e` | `8b2e015883` |
| `refs/heads/ime-direct-lease-source-review-sol-20261024` | `6e0524a616` | `3233e4a309` |
| `refs/heads/ime-direct-middle-qualified-sol-20261024` | `e04a8bc256` | `7475536274` |
| `refs/heads/ime-direct-three-fragment-qualified-sol-20261024` | `45a85d32f6` | `ee64e982a1` |
| `refs/heads/ime-mutant-epoch-final-sol-20261024` | `c14b372bfd` | `787134e475` |
| `refs/heads/ime-mutant-epoch-sol-20261024` | `d490592027` | `cb0bc3d6c5` |
| `refs/heads/ime-mutant-presented-sol-20261024` | `537c0052f9` | `9a1f672950` |
| `refs/heads/ime-mutant-requested-retarget-sol-20261024` | `f30add9fca` | `e45b98020a` |
| `refs/heads/nested-clips-sol` | `b9fcb13255` | `789d1250a4` |
| `refs/heads/perceptual-gradient-bernstein-sol` | `ab27ccb5ca` | `d1e84fe0aa` |
| `refs/heads/perceptual-gradient-cost-baseline-sol` | `5be9000ae5` | `8b20664e4d` |
| `refs/heads/perceptual-gradient-mutant-hue-policy-sol` | `fa20e1d7b7` | `8bdf4883d4` |
| `refs/heads/perceptual-gradient-profile-sol` | `ab62ce3fef` | `19c7fd66d3` |
| `refs/heads/perceptual-gradient-selector-sol` | `6e89df4c14` | `c5a4324838` |
| `refs/heads/perceptual-gradient-sol` | `4fd69a41c6` | `26d1073352` |
| `refs/heads/perceptual-gradient-strict-diagnostic-sol` | `d9434dc1ed` | `2ef6fcb616` |
| `refs/heads/reader-ticket-candidate-sol-20261024` | `bcbee2b1a1` | `fa4b5cd006` |
| `refs/heads/reader-ticket-mutant-admission-sol-20261024` | `c6b13d5185` | `a43705c080` |
| `refs/heads/reader-ticket-mutant-exhaustion-sol-20261024` | `79aaf3041b` | `0667089e54` |
| `refs/heads/remote-parking-control-sol-20261024` | `094cf92863` | `cf05ee6fc3` |
| `refs/heads/remote-parking-review-sol-20261024` | `d483cd70a2` | `a7c5d2b7d1` |
| `refs/heads/spike/banked-sample-root` | `1a553e6726` | `bec5a26206` |
| `refs/heads/spike/constant-samples-sol-20261024` | `5fc71d1595` | `35077b3db2` |
| `refs/heads/spike/lowered-solid-fault` | `6b730a0edf` | `8ac297ca32` |
| `refs/heads/spike/lowered-solid-sampler` | `7ccc65d267` | `f0f39dcf92` |
| `refs/heads/spike/mapped-combined-root` | `4b44b21a18` | `f0bc989c7e` |
| `refs/heads/spike/mapped-paint-root` | `44126d3ccf` | `2748441182` |
| `refs/heads/spike/mapped-properties-outline-sol` | `dca92e19c8` | `891d118830` |
| `refs/heads/spike/mapped-properties-sol` | `51a5fc8396` | `8fc60849f4` |
| `refs/heads/spike/optional-clip-accelerator-baseline-sol-20261024` | `f2874c35fc` | `f3b2ce8374` |
| `refs/heads/spike/optional-clip-accelerator-sol-20261024` | `62791dd29d` | `31a0b79ecc` |
| `refs/heads/spike/sample-clip-reuse-sol-20261024` | `044e831ce9` | `c536021c83` |
| `refs/heads/spike/sample-clip-reuse-sol-tag-only-20261024` | `0e6ce20a65` | `f0a0b76ea9` |
| `refs/heads/spike/shaped-capacity-control-sol-20261024` | `29c16dc4a5` | `0be3376d5a` |
| `refs/heads/spike/shaped-count-control-sol-20261024` | `51077dc55e` | `e4b1ca7ba7` |
| `refs/heads/spike/shaped-driver-base-sol-20261024` | `83eef523d6` | `16e64ebc83` |
| `refs/heads/spike/shaped-driver-final-sol-20261024` | `c95ddfac48` | `9a78c64e4b` |
| `refs/heads/spike/shaped-handoff-mutant-sol-20261024` | `5f9e6f1215` | `083dd056e6` |
| `refs/heads/spike/shaped-handoff-sol-20261024` | `a1263f4ee6` | `0f3dea42a2` |
| `refs/heads/spike/text-blocks-sol` | `ac26d70a8b` | `e5310569f9` |
| `refs/heads/spike/text-capacity-root` | `71ed91f0ae` | `31c729c73f` |
| `refs/heads/spike/text-ecow-owner-sol` | `74698bf684` | `a450c30f68` |
| `refs/heads/spike/text-metric-scale-sol` | `5fcd17f378` | `81519d3c61` |
| `refs/heads/spike/text-owner-sol` | `bbe0c2058c` | `4e10917c14` |
| `refs/heads/text-owner-sol-20261024` | `eea7c73ea1` | `76803de94f` |
| `refs/heads/text-transaction-control` | `02b72aad06` | `8e49a026fe` |
| `refs/heads/text-transaction-sol` | `4e6bb2971c` | `6345e171df` |
| `refs/heads/wt/gpui-boolean-cohort-mutant-sol-20261024` | `e5f3a4c31c` | `cd5847fc10` |
| `refs/heads/wt/gpui-boolean-cohort-sol-20261024` | `a82c69a021` | `6a1cdad3a3` |
| `refs/heads/wt/gpui-ce3` | `d464b7de9e` | `cc193f85f5` |
| `refs/heads/wt/gpui-clips-sol` | `b9fcb13255` | `789d1250a4` |
| `refs/heads/wt/gpui-component-default-simd-sol-20261024` | `14006e63d0` | `6dd938c054` |
| `refs/heads/wt/gpui-component-default-simd-sol-20261024-wip` | `14fcf87e09` | `68ae9bc9dd` |
| `refs/heads/wt/gpui-component-ime-review-sol-20261024` | `91add627e5` | `077393f6c8` |
| `refs/heads/wt/gpui-component-port-sol-20261024` | `0103a0e70c` | `a380dce4ec` |
| `refs/heads/wt/gpui-component-review-sol-20261024` | `b74a2c01e1` | `4cab281f40` |
| `refs/heads/wt/gpui-hummingbird-browser-compose-sol` | `79a4edd63c` | `12ffdd48de` |
| `refs/heads/wt/gpui-hummingbird-fresh-schema-sol` | `db72fb087d` | `fa386ada79` |
| `refs/heads/wt/gpui-length-port-sol-20261024` | `c31dd34833` | `2c1a3c2ae4` |
| `refs/heads/wt/gpui-lowered-solid-fault` | `6b730a0edf` | `8ac297ca32` |
| `refs/heads/wt/gpui-lowered-solid-sol` | `7ccc65d267` | `f0f39dcf92` |
| `refs/heads/wt/gpui-text-control` | `02b72aad06` | `8e49a026fe` |
| `refs/heads/wt/gpui-text-sol` | `4e6bb2971c` | `6345e171df` |
| `refs/heads/wt/gpui-typed-binding-mutant-sol-20261024` | `d4563f3796` | `445dda771b` |
| `refs/heads/wt/gpui-typed-binding-sol-20261024` | `9e6d1a5f65` | `49ab34dd90` |
| `refs/qualification/packed-columns-20261023/fault-cluster` | `56697b7e29` | `4e147850d2` |
| `refs/qualification/packed-columns-20261023/fault-fallback` | `171b1a0e1f` | `6c21302792` |
| `refs/qualification/packed-columns-20261023/fault-id` | `02595b4771` | `d18ee1e084` |
