# Release readiness: v2.0.0-beta.1

The intended first pre-release of the `refactor/tag-machinery` line. This page
lists what must be true **before** the tag is pushed, and then exactly what the
maintainer runs and what the workflows do in response. It is a checklist, not
a release receipt: no final `main` SHA, date, tag, assets, parity receipt, or
benchmark receipt has been recorded here. Nothing here has been tagged or
published; publishing is the maintainer's decision.

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
- [x] **crates.io decision recorded.** Option (c) is selected for this beta:
      no crates.io package is published, and the install instructions match
      that policy.
- [ ] **Tag SHA chosen and frozen.** After the reviewed promotion PR is merged,
      record it from the protected release branch: `SHA=$(git rev-parse origin/main)`
      after the last merge you intend to ship. Every box below is about
      *this* SHA, not "the tip" at some other moment.
- [ ] **Push CI green on that SHA.** The `CI` workflow's `push` run for `$SHA`
      concluded `success` (not `cancelled`; a newer push cancels the older
      run, so a cancelled run is no evidence either way):
      `gh run list --workflow CI --branch main --event push --commit "$SHA" --json conclusion,url`
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

Run from a clean checkout of the exact reviewed `main` commit. First perform
the local dry run. It exercises every guard and creates and verifies the signed
tag locally, then removes it without pushing:

```bash
SSH='ssh -o IdentityAgent=none -o IdentitiesOnly=yes -i /Users/allen/.ssh/id_es25519_swackhamer'
GIT_SSH_COMMAND="$SSH" git fetch origin main --tags
SHA=$(git rev-parse origin/main)
test "$(git rev-parse HEAD)" = "$SHA"
OXIDEX_TAG_DRY_RUN=1 just tag 2.0.0-beta.1 "$SHA"
```

Stop after the dry run and obtain separate maintainer authorization for the
exact tuple `v2.0.0-beta.1@$SHA`. Authorization to prepare or merge the
promotion PR is not tag authorization. Only after that exact authorization is
recorded may the maintainer run the real recipe:

```bash
export OXIDEX_TAG_AUTHORIZATION="v2.0.0-beta.1@$SHA"
GIT_SSH_COMMAND="$SSH" just tag 2.0.0-beta.1 "$SHA"
```

`just tag` refuses anything not reachable from `origin/main`, a commit GitHub
does not report as signed and verified, an existing local or remote tag, a
`Cargo.toml` version mismatch, or an authorization value that is not the exact
tag and full commit SHA. It creates and verifies the signed tag before its
single non-forcing push. Do not replace this recipe with manual tag commands.

### What the push triggers

**`release.yml` (GitHub Release).**

1. `verify-version` runs `tools/ci/release_version.py`. It fails the whole
   run if the tag and `Cargo.toml` disagree, and emits `prerelease=true`
   because the tag contains `-`.
2. Builds Linux x86_64/arm64 (musl), Windows x86_64, and a signed universal
   macOS binary plus notarized DMG (`oxidex-v2.0.0-beta.1.dmg`).
3. `create-release` creates a fail-closed draft **pre-release** with
   `make_latest: false`, uploads the exact asset set, and records an independent
   run/attempt provenance artifact before any release asset is uploaded.
4. `verify-release-assets` downloads that exact run's provenance and the draft
   release, binds them to the tag commit and release ID, checks the exact asset
   set and digests, and exercises Gatekeeper on both downloaded executables.
5. `publish-release` independently repeats the provenance, release-ID, draft,
   and asset checks before publishing. `/releases/latest` stays on v1.2.1.
6. `update-docs` is **skipped** for a pre-release: the stable gh-pages
   changelog and version dropdown are left alone.

If a failed run leaves a draft release, stop and inspect it manually. Do not
delete, reuse, overwrite, or rerun against that draft; resolve the incident and
obtain explicit authorization for a new immutable tag.

**`docker.yml` (Docker Hub `swackhamer/oxidex`).** Its first job only
publishes a tag reachable from `origin/main`; the guarded recipe above and the
release workflow enforce the same ancestry. For this pre-release it publishes
only `:v2.0.0-beta.1` and `:2.0.0-beta.1` and never moves `:latest`.

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
- [x] **(c) No crates.io for the beta.** Ship the GitHub pre-release
      binaries only. Rust users take a git dependency on the signed tag:
      `oxidex = { git = "https://github.com/swack-tools/oxidex", tag = "v2.0.0-beta.1" }`.
      This needs nothing else from this PR, and it is what happens by
      default if no box above is ticked.

The docs tell users not to run `cargo install oxidex` and to depend on Git,
which matches the selected policy. The README no longer advertises a crates.io
package or docs.rs API page for this beta.

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

**Homebrew.** Homebrew is disabled for this beta. There is no active formula;
the placeholder is retained as `packaging/homebrew/oxidex.rb.disabled` and
must remain quarantined until a real release asset, checksum, tests, workflow,
and approved policy exist. A tag does not enable it.

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
