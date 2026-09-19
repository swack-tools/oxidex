# Release readiness: v2.0.0-beta.1

The first pre-release of the `refactor/tag-machinery` line. This page lists
what must be true **before** the tag is pushed, and then exactly what the
maintainer runs and what the workflows do in response. Nothing here has been
tagged or published; publishing is the maintainer's decision.

`2.0.0-beta.1` is a SemVer pre-release: the dot before the number is what
makes `beta.10` sort after `beta.2`. Python tooling would spell it `2.0.0b1`
(PEP 440), but nothing in this repository ships a Python package version (see
"Versions" below).

## Before the tag

Every box needs its evidence (a link or a named instrument's output), not a
recollection.

- [x] **Docs overhaul merged.** Merged as #839 (`93143151`), with follow-up
      #840. The site's version label and banner name v2.0.0-beta.1.
- [x] **CHANGELOG entry present.** `CHANGELOG.md` has
      `## [2.0.0-beta.1] - Unreleased` (from #839). Before tagging, replace
      `Unreleased` with the release date.
- [x] **Migration guide present.** `docs/guide/migrating-from-1x.md` (from
      #839).
- [ ] **crates.io decision recorded.** One of options (a), (b) or (c) under
      "crates.io" below is chosen, and the install instructions match it.
- [ ] **Tag SHA chosen and frozen.** Record it: `SHA=$(git rev-parse origin/refactor/tag-machinery)`
      after the last merge you intend to ship. Every box below is about
      *this* SHA, not "the tip" at some other moment.
- [ ] **Push CI green on that SHA.** The `CI` workflow's `push` run for `$SHA`
      concluded `success` (not `cancelled`; a newer push cancels the older
      run, so a cancelled run is no evidence either way):
      `gh run list --workflow CI --branch refactor/tag-machinery --event push --commit "$SHA" --json conclusion,url`
- [ ] **Corpus read regression gate passing on that SHA.** Within that same
      run, the `Corpus Read Regression Gate` job concluded `success` with a
      `verdict: PASS` line in its log. Exit 1 is a code regression (read the
      `LOST` lines); exit 2 is a refused *measurement*: re-run it, don't
      count it as a pass.
- [ ] **Benchmarks current.** The committed figures
      (`benches/benchmark_results.md`, `docs/performance/`) were measured at
      `$SHA`, or at a commit where `git diff --stat <measured>..$SHA -- src oxidex-tags* Cargo.lock`
      is empty. Today they name `8f04e288` (oxidex 1.2.1); if they are older
      than `$SHA` by any reader change, re-run `benches/exiftool_comparison.sh`
      against the pinned ExifTool 13.59 and commit the result first. The
      `Benchmarks (indicative)` push run for `$SHA` concluded `success`.
- [ ] **Known limitations stated.** The release notes or the CHANGELOG entry
      say plainly: extraction parity with ExifTool is partial and measured
      (link the conformance score, not the tag-definition count); which
      formats are detected but not parsed (identity tags only); write
      support varies per format; this is a beta and its API may change
      before 2.0.0.
- [ ] **Version agrees with the tag.** `Cargo.toml` `[package] version` is
      `2.0.0-beta.1` at `$SHA`. (The release workflow now refuses a mismatch
      in its first job, but finding out there costs a failed release run.)

## Tag and publish

Run from a checkout with the swackhamer key (the tag must be signed):

```bash
SSH='ssh -o IdentityAgent=none -o IdentitiesOnly=yes -i /Users/allen/.ssh/id_es25519_swackhamer'
GIT_SSH_COMMAND="$SSH" git fetch origin --tags
SHA=<the frozen SHA from the checklist>
git show -s --format='%H %s' "$SHA"
git show "$SHA":Cargo.toml | grep -m1 '^version = "2.0.0-beta.1"$'   # must print the line
git tag -s v2.0.0-beta.1 -m "OxiDex v2.0.0-beta.1" "$SHA"
git tag -v v2.0.0-beta.1
GIT_SSH_COMMAND="$SSH" git push origin "refs/tags/v2.0.0-beta.1"
```

Don't use `just tag`/`just release`: that recipe makes an unsigned tag of
`HEAD` and pushes it at once.

### What the push triggers

**`release.yml` (GitHub Release).**

1. `verify-version` runs `tools/ci/release_version.py`. It fails the whole
   run if the tag and `Cargo.toml` disagree, and emits `prerelease=true`
   because the tag contains `-`.
2. Builds Linux x86_64/arm64 (musl), Windows x86_64, and a signed, notarized
   macOS arm64 binary and DMG (`oxidex-v2.0.0-beta.1.dmg`).
3. `create-release` publishes GitHub release "Release v2.0.0-beta.1" as a
   **pre-release** with `make_latest: false`, so `/releases/latest` stays on
   v1.2.1.
4. `update-docs` is **skipped** for a pre-release: the stable gh-pages
   changelog and version dropdown are left alone.

**`docker.yml` (Docker Hub `swackhamer/oxidex`).** Its first job only
publishes a tag reachable from `origin/main`. `refactor/tag-machinery` is
not merged into `main`, so for this tag it logs *"not reachable from
origin/main; Docker image publication is skipped"* and publishes **no image**.
That is the existing policy and this release doesn't change it. If a beta
image is wanted, that is a separate maintainer decision about the gate.
When a pre-release tag is on `main`, it now publishes only
`:v2.0.0-beta.1` and `:2.0.0-beta.1` and never moves `:latest`.

**crates.io: not published by this release, and blocked for the root crate.**
No workflow publishes to crates.io, and a tag push doesn't publish crates.
`tools/ci/test_release_workflow.py` fails if any workflow gains a
non-dry-run `cargo publish`, and the root `Cargo.toml` now carries
`publish = false`, so a tag can't leave a half-published set of crates
(crates.io only yanks, it never deletes). Checked 2026-09-18 against
`https://crates.io/api/v1/crates/<name>` and `/owners`, with the probe
validated on `serde` (200):

- **`oxidex`: taken, not ours.** It is 0.0.1, created 2025-08-12, owned by
  crates.io user `qodeninja`, repository `github.com/oxidex-rs/oxidex`,
  described as a "reserved name stub". `cargo publish` of the root crate
  fails on ownership.
- **`oxidex-tags`, `-core`, `-camera`, `-media`, `-image`, `-document`,
  `-specialty`, `-shared`: free.** None exists (404 on the crate and its
  owners), so nothing has been published under them by anyone, the
  maintainer included.
- **Size, independent of the name.** The root crate packages to 288.7 MiB
  (25.7 MiB compressed) and crates.io's limit is 10 MiB. It needs an
  `include`/`exclude` list whatever it's called (mostly test data).

The name is the maintainer's decision. The options:

- [ ] **(a) Get the name transferred.** Ask `qodeninja` directly (the
      `oxidex-rs/oxidex` repository is the contact point). If that goes
      nowhere, crates.io's usage policy handles squatted placeholder crates
      through help@crates.io, case by case and with no guaranteed outcome
      or timeline. That rules it out as a blocker for a beta.
- [ ] **(b) Publish under another name.** These are verified free on
      crates.io's API and sparse index (both 404): `oxidex-rs`,
      `oxidex-metadata`, `swack-oxidex`. Renaming is cheaper than it
      sounds, because `Cargo.toml` already pins `[lib] name = "oxidex"` and
      `[[bin]] name = "oxidex"`. Rust imports (`use oxidex::...`), the
      `oxidex` binary, `liboxidex.{a,dylib,so}`, the C header and the
      Python ctypes binding all stay as they are. What changes:
      `[package] name` and `Cargo.lock`; `fuzz/Cargo.toml` and
      `benches/spike/Cargo.toml` (`oxidex = { path = ... }` becomes
      `oxidex = { package = "<new>", path = ... }`); every `-p oxidex` in
      docs and `CLAUDE.md`; the install instructions (`cargo install <new>`
      in `docs/guide/getting-started.md`, and the "not on crates.io" notes
      in `docs/guide/library-api.md`, `troubleshooting.md`,
      `migrating-from-1x.md` and `docs/reference/api-reference.md`); the
      crates.io badge in `README.md`; and the snippet in the
      `src/parsers/magika_detector.rs` docs. The tag crates keep their
      names.
- [ ] **(c) No crates.io for the beta.** Ship the GitHub pre-release
      binaries only. Rust users take a git dependency on the signed tag:
      `oxidex = { git = "https://github.com/swack-tools/oxidex", tag = "v2.0.0-beta.1" }`.
      This needs nothing else from this PR, and it is what happens by
      default if no box above is ticked.

The docs already tell users not to run `cargo install oxidex` and to depend
on Git, which matches (c). One leftover: the `README.md` crates.io badge
points at `crates.io/crates/oxidex`, which is the other account's stub.

If the tag crates are published (under (a) or (b)), do it by hand. Cargo
orders the crates by dependency, and `publish = false` keeps the root crate
out:

```bash
cargo publish --workspace --exclude oxidex --dry-run   # rehearse (passes today)
cargo publish --workspace --exclude oxidex             # uploads in this order:
# oxidex-tags-shared 0.1.0, oxidex-tags-core, -camera, -document, -image,
# -media, -specialty, then oxidex-tags (all 2.0.0-beta.1)
```

A published pre-release is only selected by a requirement that names a
pre-release (`=2.0.0-beta.1` or `^2.0.0-beta.1`). `^2` will not pick it,
and that is intended.

**Homebrew.** `packaging/homebrew/oxidex.rb` is a template that doesn't
track releases. A tag doesn't change it.

## Versions

| Crate | v1.2.1 | now | Why |
|---|---|---|---|
| `oxidex` | 1.2.1 | 2.0.0-beta.1 | Maintainer's decision for this line. |
| `oxidex-tags-core` | 1.0.4 | 2.0.0-beta.1 | Public API broke: `types::{Tag, TagTable, TagDatabase}` moved to `oxidex-tags-shared`. They're re-exported at the crate root, but the `oxidex_tags_core::types::` paths are gone. Its data broke too: 11 of 118 tables were removed and tag definitions fell from 4,166 to 1,455, so `get_tag_table` returns `None` for names that used to resolve. |
| `oxidex-tags-camera`, `-media`, `-image`, `-document`, `-specialty` | 1.0.4 | 2.0.0-beta.1 | Each one publicly re-exports `oxidex_tags_core::types::*` and now exposes `oxidex_tags_shared` types, so a major bump of core is a major bump of each. Each one also lost tables that `get_tag_table` used to find: camera 19 of 599, media 12 of 125, document 5 of 55, image 1 of 64, specialty 1 of 18. |
| `oxidex-tags` | 1.0.4 | 2.0.0-beta.1 | Facade: re-exports `core` as a module (so `oxidex_tags::core::types::Tag` is gone) and every domain crate above. |
| `oxidex-tags-shared` | (did not exist) | 0.1.0 | New since v1.2.1 and never released, so there's no earlier interface to break. It gained the `description`/`license` metadata a publish needs. |

Evidence, `cargo-semver-checks` 0.50.0
(`cargo semver-checks check-release -p <crate> --baseline-rev v1.2.1 --release-type minor`,
toolchain 1.97.1): `oxidex-tags-core` fails `struct_missing` for `Tag`,
`TagTable` and `TagDatabase` at `oxidex_tags_core::types::` and requires a
new major version. The five domain crates and `oxidex-tags` pass all 196
type-level checks. That tool can't see two of the breaks above. It doesn't
follow items re-exported from another crate (so it misses `core`'s breakage
reaching the facade and the domain crates' switch to exporting
`oxidex_tags_core` 2.x types), and it doesn't look at data, which is where
the removed tables are. Those two are why the rest move to 2.0.0-beta.1
too. Table and definition counts come from the `*_tags.yaml` sources at
`v1.2.1` and at the tip: 32,683 tag definitions before, 16,684 now.

Inter-crate requirements pin the tag crates exactly (`=2.0.0-beta.1`), so
a later beta of one tag crate can't be mixed with this beta of another.
`oxidex-tags-shared` is required as `0.1.0`.

Python: `bindings/python` is a ctypes wrapper (`oxidex.py`) with no
`pyproject.toml`, `setup.py` or `__version__`, so it carries no version to
bump. If it is ever packaged, this release is `2.0.0b1` in PEP 440.
