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
- [ ] **Benchmark disposition verified.** For any claimed beta.1 speed,
      measure the exact frozen candidate `$SHA` with the shipped-profile
      instrument and pinned, capability-checked ExifTool; preserve its raw
      artifacts and receipt. The committed CLI figures currently name
      `8f04e288` (OxiDex 1.2.1), and the CI figures are also historical.
      If those figures remain, verify their source artifacts, label them
      visibly historical in the rendered pages, and record beta.1 performance
      as unmeasured; do not attribute old results to `$SHA`. If no performance
      claim remains, remove unsupported speed language and record
      `not_applicable` with a reason. Inspect any cited indicative CI run and
      artifact separately; a successful push job alone does not establish
      shipped-profile speed. Complete the release-documentation evidence and
      rendered-page audit for the chosen disposition before checking this box.
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
git fetch origin main --tags
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
just tag 2.0.0-beta.1 "$SHA"
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

**crates.io: disabled for this beta.** Option (c) is selected: there is no
crates.io publication for the root crate or any tag crate. Every workspace
manifest sets `publish = false`, and workflow tests reject a registry upload
step. Do not publish any workspace crate manually as part of this beta.
Changing this policy requires a separate reviewed decision and updated
installation guidance.

The signed beta tag and final `main` SHA remain pending. Until those inputs
exist, use a development branch or commit for source builds and Git
dependencies; do not describe a development build as a release asset.

**Homebrew.** Homebrew is disabled for this beta. There is no active formula;
the placeholder is retained as `packaging/homebrew/oxidex.rb.disabled` and
must remain quarantined until a real release asset, checksum, tests, workflow,
and approved policy exist. A tag does not enable it.

## Versions

| Crate | v1.2.1 | now | Why |
|---|---|---|---|
| `oxidex` | 1.2.1 | 2.0.0-beta.1 | Maintainer's decision for this line. |
| `oxidex-tags-core` | 1.0.4 | 2.0.0-beta.1 | Public API broke: `types::{Tag, TagTable, TagDatabase}` moved to `oxidex-tags-shared`. They're re-exported at the crate root, but the `oxidex_tags_core::types::` paths are gone. Current YAML definition counts are identified below. |
| `oxidex-tags-camera`, `-media`, `-image`, `-document`, `-specialty` | 1.0.4 | 2.0.0-beta.1 | Each one publicly re-exports `oxidex_tags_core::types::*` and now exposes `oxidex_tags_shared` types, so a major bump of core is a major bump of each. |
| `oxidex-tags` | 1.0.4 | 2.0.0-beta.1 | Facade: re-exports `core` as a module (so `oxidex_tags::core::types::Tag` is gone) and every domain crate above. |
| `oxidex-tags-shared` | (did not exist) | 0.1.0 | New since v1.2.1 and never released, so there's no earlier interface to break. It gained the `description`/`license` metadata a publish needs. |

Evidence, `cargo-semver-checks` 0.50.0
(`cargo semver-checks check-release -p <crate> --baseline-rev v1.2.1 --release-type minor`,
toolchain 1.97.1): `oxidex-tags-core` fails `struct_missing` for `Tag`,
`TagTable` and `TagDatabase` at `oxidex_tags_core::types::` and requires a
new major version. The five domain crates and `oxidex-tags` pass all 196
type-level checks. That earlier check is not an exact-candidate runtime
or data-compatibility measurement: it does not follow items re-exported from
another crate or inspect table contents. The source-level API changes above
explain the beta version decision; do not infer current table membership from
that older check. Counting `- id:` rows in the six `*_tags.yaml` source files
gives 32,683 at `v1.2.1` and 32,256 at integration commit
`c7e98a76921eb387cbe9d2f3202a93b7a2902947`. Separately,
`docs/public/measurements/status.json` reports 32,256 current generated tag
definitions. These are definition inventories, not a count of tags OxiDex
extracts; extraction coverage requires its own parity receipt.

Inter-crate requirements pin the tag crates exactly (`=2.0.0-beta.1`), so
a later beta of one tag crate can't be mixed with this beta of another.
`oxidex-tags-shared` is required as `0.1.0`.

Python: `bindings/python` is a ctypes wrapper (`oxidex.py`) with no
`pyproject.toml`, `setup.py` or `__version__`, so it carries no version to
bump. If it is ever packaged, this release is `2.0.0b1` in PEP 440.
