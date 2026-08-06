# Ten Unique Tag Fixes Design

## Goal

Close exactly ten independently verified ExifTool coverage gaps, with one fresh
agent assignment owning exactly one unique tag.

## Base and isolation

Work takes place in the linked worktree
`/home/allen/git/oxidex/.worktrees/ten-unique-tag-fixes` on branch
`tag-ten-unique-fixes-20260806`, based on local branch `codex` at
`49eaf84d0733ec8c435ed00c94abaa4596114991`.

The starting baseline must pass `cargo build` and `cargo test --workspace`.

## Tag selection

Choose candidates from measured gaps on the current worktree, using the
repository's pinned ExifTool release from `.exiftool-version`. The historical
verified-gap backlog may seed candidate discovery but is not proof that a gap
still exists.

Before finalizing the list and before every assignment, inspect live process
arguments with `ps`. Do not choose a tag named in another process's arguments.
Also exclude tags claimed by existing worktrees or branches.

The following names were already observed as claimed and are reserved:

- `BatteryLevel`
- `ComponentsConfiguration`
- `DustRemovalData`
- `LensInfo`
- `ImageWidth`
- `PreviewImageWidth`
- `SignType`
- `SourceImageWidth`
- `ThermalData`

The final list contains exactly ten distinct tag names. A group-qualified tag
still collides with the same bare tag name; group prefixes cannot be used to
evade uniqueness.

## Agent boundary

Dispatch one fresh agent assignment per tag. An assignment may investigate,
test, and modify only the parsing or conversion behavior required for its one
tag. It must not add adjacent tags, opportunistic table coverage, unrelated
refactors, generated tag-count changes offered as extraction coverage, or
approximate conversions.

Only three child agents can run concurrently because the controller occupies
the fourth concurrency slot. Assignments may therefore execute in waves, but
there are exactly ten fresh tag assignments in total.

Each assignment reports its tag, diagnosis, red/green test evidence, pinned
ExifTool evidence, files changed, test commands, commit, and concerns.

## Implementation rules

For every tag:

1. Confirm the gap still exists against the pinned oracle and identify its
   difference kind (`RENAME`, `MISSING`, `VALUE`, or `EXTRA`) before costing or
   coding it.
2. Check `src::exiftool_tables::find_table(module, table)` and
   `docs/TRANSCRIPTION.md` before deriving binary layout by hand.
3. Write a focused regression test and observe the expected failure before
   changing production code.
4. Implement the smallest exact fix. Never approximate a conversion; omit a
   value when exact behavior cannot be established.
5. Re-run the focused test, relevant parser tests, and a pinned comparison that
   demonstrates that specific tag improved without regressions.
6. Commit only that tag's change in an independent commit whose message and
   trailers identify the tag.

Agents share the worktree and therefore must not rewrite, discard, reset, or
stage another assignment's changes. The controller integrates completed
assignments serially and resolves overlapping-file work before dispatching the
next conflicting assignment.

## Review and completion

Every tag commit receives an independent task-scoped review for both spec
compliance and code quality. Critical and important findings must be fixed and
re-reviewed before that tag is accepted. After all ten assignments, run a
whole-branch review, `cargo fmt --check`, `cargo clippy`,
`cargo test --workspace`, table verification where relevant, and pinned
coverage comparisons for all ten tags.

Completion means exactly ten unique tags have evidence-backed improvements,
no reserved or concurrently claimed tag was reused, each tag has one focused
assignment and independent commit history, and the final verification suite is
clean.
