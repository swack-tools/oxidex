# Agent operational runbooks

Read the applicable procedure before remote publication, cleanup, or stack operations.
The mandatory summary lives in [AGENTS.md](../AGENTS.md).

## Local execution and concurrency

Use native desktop agents first, up to three workers, and CLI workers for
additional independent work. Select explicit models and efforts with
[Codex task routing](../.agents/skills/codex-task-routing/SKILL.md). Each writer
owns a separate worktree, branch and target directory. Keep one active published
candidate per dependency cluster.

Run workers and builds directly on this laptop without SSH, fleet or hub.
User-authorized CLI implementation and parser tasks use `codex --yolo exec`
through the project launcher, without a contradictory sandbox option. This
allows access to external operations evidence, targets and sccache without
approval prompts. Review and inventory use read-only sandboxes with explicit
approval policy `never`. No route changes global configuration or uses fast mode.

Choose configurable worker counts and `CARGO_BUILD_JOBS` from available cores
and memory; high local CPU concurrency is permitted. Give each worktree its own
`CARGO_TARGET_DIR`, reduce concurrency under memory pressure, and reserve corpus
timing measurements exclusively from competing builds.

## Maintainer identity

**Agent publication commands use the maintainer's account.**
Run agent-authored commits and Git/`gh` publication commands as `swackhamer`,
including PRs, comments, reviews and merges. Do not substitute another account.
Installed CI and review services retain their own service identities.
Resolve the token before every `gh`
command and fail closed if it's missing, rather than prefixing
`GH_TOKEN=$(gh auth token --user swackhamer)` directly onto the command: if
that substitution fails or returns empty, the `gh` command still runs, and
`gh` silently falls back to whatever account is otherwise active—the exact
account this rule forbids.
`SWACKHAMER_TOKEN=$(gh auth token --user swackhamer) || { echo "no token for swackhamer" >&2; exit 1; }`,
then `[ -n "$SWACKHAMER_TOKEN" ] || { echo "empty token for swackhamer" >&2; exit 1; }`,
then run `gh` with `GH_TOKEN="$SWACKHAMER_TOKEN"`. `GH_TOKEN` only
binds `gh`. `origin` is `git@github.com:swack-tools/oxidex.git`, an SSH
remote, so Git pushes, including those invoked by `gh stack`, authenticate with
whatever key `ssh` picks, which can be a different account's. Export the
maintainer's key for the whole session before pushing anything, so it covers
every path, not just an explicit `git push`:

```bash
[ -r "$HOME/.ssh/id_es25519_swackhamer" ] || {
  echo "maintainer SSH key is not readable" >&2
  exit 1
}
export GIT_SSH_COMMAND='ssh -F /dev/null -o IdentityAgent=none -o IdentitiesOnly=yes -o IdentityFile=~/.ssh/id_es25519_swackhamer'
```

Check readability before invoking SSH. Keep the explicit `IdentityFile` option:
OpenSSH can ignore a missing `-i` path and load default keys even with
`IdentitiesOnly=yes`. `-F /dev/null` excludes ambient SSH configuration.

Before pushing, check authorship, email and signing identity over the
commits about to go out—not the branch's whole history, which on a
`staging/*` branch includes inherited trunk commits authored by others—with `git log --format='%an|%ae|%cn|%ce|%G?|%GS' origin/refactor/tag-machinery..HEAD`
(use `@{u}..HEAD --not origin/refactor/tag-machinery` once the branch has its own upstream, excluding inherited integration commits). `%G?` reports that
a signature verifies, not who made it—a commit signed with a different,
independently valid key still prints `G`—so also check `%an`/`%cn` and
`%ae`/`%ce` against the maintainer's own name and known emails, and `%GS`
against the maintainer's signing identity (the principal recorded in
`gpg.ssh.allowedSignersFile`, for example `swackhamer@users.noreply.github.com`).
Fix any commit in that range that fails any of these before pushing. When a
stack push will publish more than the checked-out branch (`gh stack push`,
`--upstack`), run this check for every branch it pushes, not only the
current one.

## Cleanup and preservation

**Delete build directories you will not need again.** A separate
`CARGO_TARGET_DIR` per worktree keeps parallel builds from colliding, but each
one grows to 20–50 GB, and a single release push once left about 1 TB of them
behind. Build output can always be regenerated, so delete a target directory
once you are confident nothing will use it again:
- its worktree is removed;
- its branch has merged or been closed;
- the one-off measurement it served has been recorded, together with the
  binary's fingerprint and sha256.

Keep it while its binary is still evidence someone may re-check, or while a
live agent or an open PR still builds there. Delete only directories you
created, by exact path, never by glob across other agents' directories.

A scratch clone or bundle is not build output: it can hold the only copy of
a commit graph. Treat a scratch clone like a worktree: its uncommitted,
untracked and ignored files must be empty or preserved first (the same
`git status --porcelain --ignored --untracked-files=all` check as for a
worktree, below). Then
delete it only after proving, against the live remote, that every commit it
carries is reachable from `origin`. Each of Clone and Bundle needs two kinds
of check, and order matters: run the tag check first, because the commit
check below relies on it having already passed.

For a bundle, import its advertised refs into a fresh scratch bare repository
before the tag-first check below; an empty pre-import namespace proves nothing.
Run `git fetch --no-tags --no-prune-tags --no-auto-maintenance <origin-url> '+refs/heads/*:refs/remotes/origin/*'`, then
`git fetch <bundle> '+refs/*:refs/bundle/*'`. Add `'+HEAD:refs/bundle/HEAD'`
only when `git bundle list-heads <bundle>` advertises `HEAD`. Both fetches
must succeed. Do not re-import or change bundle refs between tag and commit
checks; a changed set requires repeating the tag check.

An annotated tag is its own object, separate from the commit it points to,
so a tag created but never pushed, or a same-named tag re-created to point
somewhere else, can be lost even when its target commit is already on
`origin`—pinning it under `refs/keep/*` only keeps it from being pruned
locally, it doesn't put it on the remote. So first diff its tags against
the live remote by `(refname, object)` pair, not just by name:
- **Clone**: `comm -23 <(git for-each-ref --format='%(refname) %(objectname)' refs/tags | sort) <(git ls-remote --tags origin | awk '{print $2, $1}' | sort)`
  must print nothing.
- **Bundle**: its tags land under `refs/bundle/tags/*` in the scratch
  repository, not `refs/tags/*`, so rewrite the prefix before comparing.
  That scratch repository was fetched straight from `<origin-url>`, never
  through `git remote add origin`, so it has no remote named `origin`—use the URL, not the name, or `git ls-remote --tags origin` fails outright:
  `comm -23 <(git for-each-ref --format='%(refname) %(objectname)' refs/bundle/tags | sed 's#^refs/bundle/tags/#refs/tags/#' | sort) <(git ls-remote --tags <origin-url> | awk '{print $2, $1}' | sort)`
  must print nothing.

Push any tag either check lists first, or otherwise archive it. From a
Clone, that's `git push origin <tag>`. From a Bundle's scratch repository,
there's still no remote named `origin` there, and the tag itself lives
under `refs/bundle/tags/<tag>`, not `refs/tags/<tag>`, so push by URL and
full refspec instead:
`git push <origin-url> refs/bundle/tags/<tag>:refs/tags/<tag>`. Once the tag
check comes back clean, every local (or bundled) tag is
proven to match a tag already on `origin`, so a commit reachable only
through one of those tags counts as reachable too—`--remotes=origin` alone
only covers branches, and without this, history that's on `origin` solely
via a tag (nothing unusual: a tag can exist without its branch ever being
pushed) would wrongly look unsafe to delete forever. Now check commits:
- **Clone.** A SHA saved to a text file is not a ref, so it does not keep the
  object reachable: pruning can delete the clone's only reference to a
  commit, and `git fetch --prune` also runs `maintenance --auto` after
  fetching by default (`git fetch -h`), which can garbage-collect the
  now-unreachable object before anything below checks it. Record the object
  IDs alone, one per line, in `<evidence>/clone-refs.txt`—`git for-each-ref --format='%(objectname)' > <evidence>/clone-refs.txt`,
  plus the SHAs in `.git/FETCH_HEAD`, if any—then pin every one of them to
  a real ref so the objects survive regardless of what fetch or gc does
  next: for each SHA, `git update-ref "refs/keep/<n>" "$sha"` (any unique
  name per line works; nothing reads the ref's name back). Then run
  `git fetch --no-tags --no-prune-tags --no-auto-maintenance --prune origin '+refs/heads/*:refs/remotes/origin/*'`, so `origin/*` matches
  the live remote without also running gc in the same step. Then both
  checks must come back empty:
  - `git rev-list --all --reflog $(cat <evidence>/clone-refs.txt) --not --remotes=origin --tags`
    prints nothing—`--tags` is safe to add only because the tag check
    above already confirmed local tags match `origin`'s. This reads
    `clone-refs.txt` as SHAs only; if it ever carries refnames too (for
    example a raw `for-each-ref` dump with both columns), a since-pruned
    refname makes this fail outright.
  - `git fsck --unreachable --no-reflogs` lists no commit that isn't
    reachable from `origin`.

  Delete the `refs/keep/*` refs together with the clone once both checks come
  back empty and the clone is confirmed disposable.
- **Bundle.** After importing the refs and verifying their tags above, run
  `git rev-list --glob=refs/bundle --not --remotes=origin --glob=refs/bundle/tags`.
  It must print nothing. Excluding `refs/bundle/tags` is safe only because
  those exact imported objects have already matched the remote tag objects.
  Delete the scratch repository only after the checks pass.

Anything unreachable must be pushed or archived first.

Before removing a worktree, also preserve its private HEAD reflog: a detached
commit may no longer be reachable from its current HEAD. Save
`git reflog show HEAD --format=%H > <evidence>/worktree-head-reflog-shas.txt`
from that worktree. Run `git fetch --no-tags --no-prune-tags --no-auto-maintenance --prune origin '+refs/heads/*:refs/remotes/origin/*'`
before checking reachability, so a deleted remote ref cannot hide an orphan.
The explicit branch refspec and disabled tag pruning preserve unpublished
annotated tags even when ambient Git settings enable tag pruning.
Do the remote tag-object comparison above against these fresh refs, then check
`git rev-list --stdin --not HEAD --remotes=origin --tags < <evidence>/worktree-head-reflog-shas.txt`.
Archive or push every listed commit with a real ref before removal; a SHA text
file alone is not an archive. The tag exclusion is allowed only after local
tag object/ref pairs have passed the remote check above. Preserve any otherwise
unpublished annotated tags too. The current HEAD and its ancestry are checked
separately below, including the exact-head squash-merge exception. For managed
Codex worktrees use the archive tool, after preserving any reflog-only commits
and ignored evidence that its snapshot does not include.

A worktree is removable only when all of these hold:
- it has no uncommitted or untracked files (`git status --porcelain` is empty);
- it has no commits missing from `origin`, using the fresh refs fetched
  immediately before the reflog check above. If you fetch again, repeat both
  the reflog and HEAD checks against the new ref set before removal. Then
  `git rev-list HEAD --not --remotes=origin --tags` prints nothing, or its branch's
  PR has squash-merged into `refactor/tag-machinery`. Check that with
  `gh pr view <n> --json state,mergeCommit,headRefOid,baseRefName`: `state`
  and `mergeCommit` alone don't establish this, since `mergeCommit` names the
  new commit created on the base branch, not the PR's own head. Confirm
  `baseRefName` is `refactor/tag-machinery` (a PR merged into some other
  branch doesn't count) and that `headRefOid` is the exact commit you're
  about to remove. A squash merge leaves the branch's own commits
  unreachable from `origin`, but its change is in the trunk;
- its ignored files hold nothing worth keeping. `git worktree remove` deletes
  ignored files without asking, and `git status` does not show them, so list
  them with `git status --porcelain --ignored --untracked-files=all`: without
  `--untracked-files=all`, an ignored directory (for example `runs/`) collapses to
  one line for the directory and hides every file inside it, which is
  exactly the evidence this check exists to catch. `HANDOFF.md` is ignored,
  and it is the resume record, so copy it and any ignored evidence to
  `${OXIDEX_OPS_DIR:-$HOME/oxidex-ops}/evidence/<run>/` before removing the worktree, unless the
  maintainer has said it is disposable.

Use Codex's managed worktree archive tool for managed checkouts. For other
worktrees, use `git worktree remove` after the preservation checks, never
`rm -rf`. Destructive cleanup still needs the approved dry-run plan required
by AGENTS.md; this runbook alone does not authorize deletion.

**The stash is shared by every worktree.** `refs/stash` belongs to the
repository, not to a worktree. A bare `git stash pop` or `git stash apply`
therefore takes whatever was stashed last in any worktree, possibly another
agent's work-in-progress from a different branch. It has already happened
here once. Git kept the other stash only because the pop conflicted. Prefer a
WIP commit on your own branch, or a patch file in your evidence directory,
over the stash. If you must stash, name your branch in the message and
record its object ID without re-reading the shared `stash@{0}`. Create the
entry and capture its ID in one step, then store it:
`sha=$(git stash create "<branch>: <why>") && git stash store -m "<branch>: <why>" "$sha"`.
`git stash create` doesn't clean the tree, and it doesn't include untracked
files, so check `git stash show -p "$sha"` before discarding your changes.
To apply it later, use that recorded ID: `git stash apply "$sha"`.
`stash@{N}` is a reflog position that another agent's push renumbers. If you
don't have the ID, find the entry with `git stash list`, resolve it with
`git rev-parse stash@{N}`, check its message with
`git log -1 --format=%s <sha>`, and only then apply that ID. Never run a bare
`pop`, `apply` or `drop`, never apply by position, and never `git stash clear`.

## Stacking dependent PRs

The integration target is `refactor/tag-machinery`; ordinary development does
not merge into `main`. Prefer one stable candidate per dependency cluster.
Do not propagate every exploratory fix through every child. Preserve child
branches, dirty patches, findings, and evidence while the parent is repaired.
Once it lands, reconstruct the next candidate from the fresh integration tip.
Independent changes remain separate; consolidate only changes whose shared
implementation boundary requires joint acceptance.

For a true stack, record its parent/child order and immutable parent heads in
`HANDOFF.md` and each PR body. Open a child against its parent's branch so review
sees the child's own diff. A parent's new head invalidates the child's previous
acceptance. Coordinate a stable update, use a signed merge into the published
child, and run local review and affected validation before pushing it. Do not
rewrite published branches or change shared Git signing settings; pass `-S` to
the merge or `--gpg-sign` to a rebase of an unpublished branch.

Land bottom first. Never merge a child into its parent's branch. After a parent
squash-merges, retarget its child to `refactor/tag-machinery` first, then merge
the new integration tip and push. Retargeting alone does not trigger this repo's
pull-request CI. If the push preceded retargeting, obtain a new pull-request
run for the actual base (for example, close/reopen after verifying the head is
unchanged); a head-only workflow-dispatch run does not prove the new merge tree.
Every merge still requires final-head CI, cleared threads and independent proof
of the central behavior. Use `gh pr merge <n> --squash --match-head-commit <sha>`
only after confirming its base is the integration branch.

GitHub stacks are optional. Inspect the installed CLI/API's actual proposed PR
set before using a stack-wide operation. Do not run automatic rebase/sync or
noninteractive stack merge commands that may rewrite or merge other layers.
If the stack mechanism cannot preserve the reviewed heads and required landing
target, use separate replacement PRs against the integration branch. Link and
close superseded PRs only after verifying the replacement mapping; preserve
unresolved findings and old branches. PR closure never proves a fix.

Local review follows [Codex task routing](../.agents/skills/codex-task-routing/SKILL.md):
one scoped full review, repair, and review of subsequent deltas. Review the full
candidate again when its base or a shared safety boundary changes. Use the
prescribed composed-core acceptance review for write-core changes. Do not require
multiple blanket `xhigh` passes. Validate findings rather than assuming a model's
claim is correct. After two repair rounds, diagnose a common cause or narrow the
scope before continuing. A cost stop never waives an unresolved defect.

After merging, keep a checkout that is useful for subsequent work. If cleanup
is needed, preserve evidence and verify the exact PR head, integration base and
merge commit before retiring a squash-merged branch. `git branch -d` may refuse
a squash-merged branch because its original commits are not ancestors of the
new squash commit; do not treat that as proof the work is lost. Use the managed
archive tool where available. Manual forced deletion requires the separately
approved cleanup plan and preservation checks above, including a fresh ancestry
check of the PR's merge commit in `origin/refactor/tag-machinery`.

## Review guidelines

Review actionable defects introduced by the candidate and its interactions.
Back parity claims with source or actual output from the ExifTool release pinned
in `.exiftool-version`; a plausible guess is not evidence. Review priorities are:

- Wrong values under real tag names, including wrong units, conversions, groups
  or physical copies; silent partial writes or no-ops; dropped metadata or
  damaged offsets, CRCs and IFD chains.
- Crashes, unbounded resource use on crafted inputs, security defects, leaked
  secrets, and fail-open locks, leases, ownership or process-lifecycle checks.
- Demonstrated divergence on requests both tools accept, missing or unnamed
  refusals, tests that cannot fail or bypass required oracle checks, and stale
  or mislabeled measurements.

A refusal can be intentional when it names the tag, uses the typed per-tag error
where available, and is documented as unsupported. Do not turn an explicit
refusal into silent partial success to mimic ExifTool's skip behavior. Distinguish
read support from write support and a per-file transaction from batch atomicity.

Formatting, naming preferences, and behavior choices that both match the contract
are not correctness findings. Validate a reported issue, add a reproducing
regression when appropriate, and preserve its evidence or disproof in the finding
ledger. A clean review is scoped evidence, not a guarantee of bug-free code.
