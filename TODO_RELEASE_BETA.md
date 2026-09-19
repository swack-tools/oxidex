# TODO: Promote v2.0.0-beta.1 to `main`

This is the living release ledger for OxiDex `v2.0.0-beta.1`. Add notes,
evidence links, blocking defects, decisions, and exact SHAs here as the work
progresses.

The goal is to make `refactor/tag-machinery` and all supporting documentation,
tests, measurements, and delivery automation ready for a real integration into
`main`. The goal is **not** to rename `refactor/tag-machinery`, replace `main`
with an unreviewed snapshot, or tag a commit that is not reachable from
`origin/main`.

## Release contract

The release is complete only when all of these statements are true:

- [ ] The reconciled code has been merged into `main` through a reviewed PR.
- [ ] Every retained `main` change and every refactor change has an explicit
      disposition; no side of a conflict was accepted wholesale without review.
- [ ] The exact resulting `main` SHA passes all required tests, generated-file
      verification, parity gates, documentation checks, and release builds.
- [ ] Version strings, the changelog, release notes, installation instructions,
      benchmarks, parity claims, and known limitations describe that exact SHA.
- [ ] The macOS ARM64 artifact builds, is signed with the Developer ID identity,
      and the DMG is accepted, stapled, and validated by Apple notarization.
- [ ] The release workflow creates a GitHub **pre-release**, does not move the
      stable `Latest` release, and uploads every expected artifact.
- [ ] The Docker workflow publishes only the exact beta tags from a commit
      reachable from `main`; it does not move `latest` or other stable tags.
- [ ] Signed tag `v2.0.0-beta.1` points to the verified release commit on
      `main` and is not moved or recreated.

## Current snapshot

Refresh this section whenever the candidate changes. Historical green runs are
context, not evidence for a later release SHA.

| Item | Current observation | Release implication |
|---|---|---|
| `origin/main` | `4a38afde` | Refresh before integration. |
| `origin/refactor/tag-machinery` | `67d58b95` | Current candidate, not frozen. |
| Merge base | See the divergence report | The histories genuinely diverged. |
| Divergence | 45 main-only / 520 refactor-only commits | Requires deliberate reconciliation. |
| Merge simulation | 32 conflicted files: 30 content, 1 add/add, 1 modify/delete | A direct merge is not release-ready. |
| Current refactor CI | Green at `67d58b95` | Useful baseline only; rerun on the final reconciled SHA. |
| Current benchmark workflow | Green at `67d58b95` | Indicative; committed release claims still need refresh. |
| Beta tag/release | Neither currently exists | Do not create until the final `main` SHA is frozen. |

Current green-run references:

- CI: <https://github.com/swack-tools/oxidex/actions/runs/35423314373>
- Indicative benchmarks:
  <https://github.com/swack-tools/oxidex/actions/runs/35423314366>

The earlier temporary `v2.0.0-beta.1` run at `8f05ec44` is **not** release
evidence. The tag was deleted while Actions runners were checking it out, so
all platform jobs failed at checkout before compilation, signing, or
notarization. A future rehearsal must leave its tag or ref available until all
jobs finish.

## 1. Coordination and evidence discipline

- [ ] Refresh `origin/main` and `origin/refactor/tag-machinery`; record both
      full SHAs before starting each release wave.
- [ ] Use one dedicated worktree, branch, and `CARGO_TARGET_DIR` per task.
- [ ] Run `tools/preflight.sh --upstream` before the first edit and before
      remote operations.
- [ ] Preserve unrelated dirty files and worktrees; never rewrite the protected
      checkout in place.
- [ ] Use the shared build lock for Cargo builds/tests/clippy and the exclusive
      lock for corpus sweeps and read-regression gates.
- [ ] Use only the pinned ExifTool release and pinned Perl runtime. Never use a
      bare `exiftool` invocation.
- [ ] Require both oracle probes: ExifTool version and
      `OOXML.docx -> FileType: DOCX`.
- [ ] Store durable receipts with the exact instrument, binary path and SHA,
      source SHA/dirty state, oracle versions, corpus path/file count, and
      metric floors.
- [ ] Update `HANDOFF.md` at each meaningful milestone with the exact next
      command and current PR/CI state.

Notes:

> Add coordination notes here.

## 2. Finish functional and ExifTool-parity work

Already landed on `refactor/tag-machinery`:

- [x] DJI float forward-port (`#844`).
- [x] Canon hand-subtable forward-port (`#849`).
- [x] CIFF-in-JPEG and Leica CameraIFD forward-port (`#851`).
- [x] Google HDRP, MakerNote, and GContainer forward-port (`#852`).
- [x] Sony MakerNote forward-port (`#853`).
- [x] Generated Exif::Main mixed-mode and byte-exact runtime work (`#848`,
      `#850`).

Still required:

- [ ] Finish the Olympus port on the generated path. Do not reintroduce the
      displaced hand-owned implementation merely to make a cherry-pick apply.
- [ ] Finish the long-tail and remaining DJI/main ports: Nikon, Pentax,
      Panasonic, Kodak, Casio, HP, Ricoh, Samsung, MediaJukebox, Vivo, and any
      residual composite or XMP dependencies.
- [ ] Decide whether autogeneration Step 2a typed values are a beta blocker.
      Record the decision and rationale here.
- [ ] Re-run the main-divergence measurement against the latest candidate;
      replace the pre-port counts in
      `docs/reference/main-divergence-2026-09-18.md` with current facts or
      clearly mark the old measurement as historical.
- [ ] Run the occurrence-aware combined-corpus comparison against the pinned
      oracle. Record matched, MISSING, VALUE, EXTRA, RENAME, ceiling, file
      count, and oracle occurrence count.
- [ ] Require zero newly lost proven reads under the read-regression gate.
- [ ] Investigate every remaining case that `main` matches and the release
      candidate does not; classify it as ported, superseded, intentionally
      removed, or unresolved.
- [ ] Keep tag-definition counts, generated-table share, parser-route coverage,
      observed reads, write coverage, and output conformance as separate
      metrics.

Evidence:

| Measurement | Candidate SHA | Instrument/receipt | Result |
|---|---|---|---|
| Combined-corpus conformance | | | |
| `t/images` read-regression gate | | | |
| Generated-table verification | | | |
| Remaining main-only behavior | | | |

## 3. Fix and validate macOS release linking

The intended Darwin-specific Cargo configuration is:

```toml
[target.aarch64-apple-darwin]
rustflags = [
    "-C", "strip=none",
    "-C", "link-arg=-Wl,-S,-x",
]

[target.x86_64-apple-darwin]
rustflags = [
    "-C", "strip=none",
    "-C", "link-arg=-Wl,-S,-x",
]
```

- [ ] Commit the `.cargo/config.toml` change from its dedicated worktree.
- [ ] Reproduce the original misaligned `LINKEDIT` string-pool failure on the
      appropriate baseline or retain a durable existing reproduction receipt.
- [ ] Prove the new configuration fixes the failing macOS release build.
- [ ] Inspect the produced Mach-O binary and dylib rather than treating a zero
      Cargo exit code as sufficient proof.
- [ ] Confirm the linker removed debug information and local symbols as
      intended without corrupting exports required by the CLI, C ABI, or
      dynamic library.
- [ ] Prove Linux remains target-isolated: its rustc invocation must still have
      exactly one `-C strip=symbols` and no Darwin link arguments.
- [ ] Validate both configured Darwin targets or explicitly document why only
      ARM64 is shipped and how x86_64 remains tested.
- [ ] Run the release workflow's actual `just build-release` path on macOS.
- [ ] Sign the binary and verify it with `codesign --verify --strict --verbose`.
- [ ] Build, notarize, staple, and validate the DMG.
- [ ] Record artifact sizes and symbol/export inspection results before and
      after the change.

Evidence:

| Check | SHA | Command/run | Result |
|---|---|---|---|
| macOS ARM64 release build | | | |
| macOS x86_64 build/config | | | |
| Mach-O/export inspection | | | |
| Linux isolation | | | |
| Signing | | | |
| Notarization/stapling | | | |

## 4. Version and package audit

- [ ] Confirm the intended release spelling is consistently
      `2.0.0-beta.1`; Python documentation may use `2.0.0b1` only when
      explaining PEP 440.
- [ ] Audit every Cargo workspace package version and every exact inter-crate
      dependency requirement.
- [ ] Confirm `oxidex-tags-shared` intentionally remains `0.1.0`, or change it
      with an explicit compatibility rationale.
- [ ] Regenerate and verify `Cargo.lock` after all manifest changes.
- [ ] Verify `cargo metadata` reports the intended package graph and versions.
- [ ] Verify the built CLI reports the intended version.
- [ ] Search all tracked files for stale `1.x`, beta, branch, installation,
      artifact-name, and release-channel claims; classify every hit.
- [ ] Keep the root crate `publish = false` unless the crates.io ownership and
      package-size blockers are deliberately resolved.
- [ ] Record the crates.io decision: transfer name, alternate package name, or
      no crates.io publication for this beta.
- [ ] Ensure every installation example matches that decision.
- [ ] Decide whether tag crates will be published manually; if yes, rehearse
      the complete dependency order with `cargo publish --dry-run`.

Decision:

> Record the crates.io and package-publication decision here.

## 5. Documentation and factual-release audit

- [ ] Replace `## [2.0.0-beta.1] - Unreleased` in `CHANGELOG.md` with the real
      release date only after the release candidate is frozen.
- [ ] Make the changelog readable as a user-facing summary, not a commit dump.
- [ ] Ensure the migration guide reflects the final API and output behavior.
- [ ] Update documentation that still tells users to switch to
      `refactor/tag-machinery`; after promotion it should point to the signed
      tag or `main` as appropriate.
- [ ] Update `docs/RELEASE-2.0.0-beta.1.md` so its workflow culminates on
      `main`, not a tag on the refactor branch.
- [ ] Remove the statement that Docker will be skipped because the tag is not
      on `main`; the final tag must be reachable from `main`.
- [ ] State known limitations plainly: measured partial ExifTool parity,
      detected-but-not-parsed formats, per-format write support, beta API
      instability, and any intentionally deferred regressions.
- [ ] Verify every numerical parity claim from a machine-readable receipt for
      the final candidate.
- [ ] Re-run committed benchmarks at the final candidate SHA, or prove the
      measured SHA has no relevant source, dependency, configuration, or
      harness differences.
- [ ] Keep noisy shared-runner numbers labelled indicative. Do not copy them
      into stable benchmark claims without appropriate controls.
- [ ] Confirm benchmark commands use the shipped release profile and the pinned
      ExifTool oracle with its capability probe.
- [ ] Build the documentation site exactly as CI/deployment does.
- [ ] Run link, stale-reference, and spelling checks.
- [ ] Verify badges, download URLs, GitHub Release links, artifact names,
      Homebrew status, crates.io status, and Docker instructions against
      reality.
- [ ] Review `README.md`, `AGENTS.md`, and `CLAUDE.md` for contradictory branch,
      release, measurement, or command guidance.

Evidence:

| Documentation check | SHA | Instrument/output | Result |
|---|---|---|---|
| Docs build | | | |
| Link/stale-reference checks | | | |
| Benchmark refresh | | | |
| Changelog review | | | |
| Version-string audit | | | |

## 6. Codify the repeated release workflow as skills

- [ ] Create a project-local release-engineering skill whose terminal state is
      a tested commit merged to `main`, signed tag created from that exact SHA,
      release workflows complete, and published artifacts verified.
- [ ] Include explicit gates for version coherence, test evidence, PR/main
      ancestry, GitHub Release creation, Docker publication, macOS signing, and
      Apple notarization.
- [ ] Create a separate release-documentation skill that requires provenance
      for changelog, benchmark, parity, installation, compatibility, and known-
      limitation claims.
- [ ] Update the `exiftool-parity` skill to forbid bare ExifTool invocation,
      require both oracle probes and corpus floors, use occurrence-aware keys,
      distinguish all coverage/conformance metrics, and emit a stable release
      receipt consumed by the documentation skill.
- [ ] Correct any stale paths, bare `exiftool` examples, obsolete generated-
      table references, and weak acceptance criteria in the current parity
      skill.
- [ ] Decide and document the canonical project-local skill location and how
      `.claude/skills` and `.agents/skills` remain synchronized without silent
      drift.
- [ ] Pressure-test each skill against representative release failures before
      treating it as operational guidance.
- [ ] Update `AGENTS.md` and `CLAUDE.md` routing so agents discover the skills
      at the correct points without weakening protected-branch or pinned-oracle
      rules.

Notes:

> Record the skill layout and validation decisions here.

## 7. Validate CI/CD before promotion

- [ ] Confirm pull requests into `main` run the full CI workflow.
- [ ] Add or verify required `main` status checks. The current active ruleset
      requires a PR, squash merge, and signed commits but does not presently
      report required CI checks through the ruleset API.
- [ ] Decide whether release-profile build, generated tables, and corpus read
      regression must all be required checks for the promotion PR.
- [ ] Ensure CI tests the exact PR merge result, not merely a stale branch head.
- [ ] Ensure a post-merge push run tests the exact resulting `main` SHA.
- [ ] Validate release-workflow YAML and its regression tests against the final
      intended behavior.
- [ ] Add a safe full-pipeline rehearsal mechanism, such as an explicit dry-run
      or release-candidate tag flow, that exercises checkout and all platform
      builds without accidentally publishing the final release.
- [ ] During a tag-based rehearsal, do not delete the tag until every workflow
      has reached a terminal state.
- [ ] Verify all referenced Actions secret names exist. Never print secret
      values.
- [ ] Verify Linux x86_64/ARM64, Windows x86_64, and macOS ARM64 release
      artifacts are produced with the expected names.
- [ ] Verify `create-release` waits for every platform build and publishes only
      after all have succeeded.
- [ ] Verify a SemVer prerelease becomes a GitHub prerelease with
      `make_latest: false`.
- [ ] Verify stable documentation deployment is intentionally skipped for the
      beta, or change that policy deliberately and test it.
- [ ] Verify the Docker ancestry gate accepts the final tag because its SHA is
      reachable from `origin/main`.
- [ ] Verify beta Docker publication creates only exact beta tags and does not
      move `latest`, major, or minor floating tags.
- [ ] Decide whether checksums, SBOMs, provenance attestations, or signatures
      for non-macOS artifacts are release requirements; document any deferral.

Evidence:

| Workflow/gate | Candidate SHA or tag | Run | Result |
|---|---|---|---|
| Promotion PR CI | | | |
| Post-merge `main` CI | | | |
| Release rehearsal | | | |
| macOS signing/notarization | | | |
| Docker rehearsal/gate | | | |

## 8. Curate history without rewriting the shared branch

Do not force-push a rewritten `refactor/tag-machinery`. Existing worktrees,
PRs, evidence receipts, signatures, and documentation refer to its current
SHAs.

- [ ] Freeze the release-ready refactor tip and create a signed archival ref or
      tag before any history curation.
- [ ] Inventory the 520 refactor-side commits and classify them as durable
      milestones, generated updates, parity ports, release work, fixups,
      reverts, experiments, or superseded work.
- [ ] Decide the desired history shape:
  - preserve the full refactor history as the second parent of one release
    integration merge, keeping `main` first-parent history clean; or
  - construct a separate cleaned promotion branch with a reviewed set of
    thematic, signed commits while retaining the archival ref.
- [ ] If using a curated branch, define squash boundaries by behavior and
      provenance, not merely by commit count.
- [ ] Preserve authorship, issue/PR references, generated-source provenance,
      and release-relevant evidence links in curated commit messages.
- [ ] Prove the curated candidate's tree is identical to the validated
      reconciled tree before accepting the cleanup.
- [ ] Re-run all gates after history reconstruction; earlier SHAs and receipts
      do not automatically transfer to rewritten commits.
- [ ] Never combine history cleanup with unreviewed behavior changes.

Decision:

> Record whether the release preserves full ancestry or uses a curated commit
> series, including the archival ref name.

## 9. Reconcile `main` and the release candidate

This is a real integration. It is not a rename, force-update, or exact-tree
replacement of `main`.

- [ ] Refresh and freeze the exact `origin/main` and release-candidate SHAs.
- [ ] Create a dedicated integration worktree and branch from the chosen
      release-candidate history.
- [ ] Merge current `origin/main` into that integration branch normally.
- [ ] Resolve every textual conflict deliberately. The current simulation has
      conflicts in comparison, composite, core helpers, timestamp handling,
      image/audio/raw/QuickTime parsers, MakerNotes, XMP, and tests.
- [ ] Audit automatically merged files as carefully as conflicted files; a
      clean textual merge can still restore obsolete code or duplicate a
      semantic forward-port.
- [ ] Produce a disposition ledger for all 45 main-only commits: retained,
      forward-ported, superseded, intentionally obsolete, or still unresolved.
- [ ] Confirm no stopped fleet tooling, obsolete comparison normalization, or
      displaced hand parser is reintroduced merely because it merged cleanly.
- [ ] Re-run formatting, clippy, workspace tests, release builds, generated
      verification, corpus read regression, occurrence-aware parity, docs, and
      benchmarks on the reconciled result.
- [ ] Open the promotion PR against `main` only after the reconciled branch is
      clean and all evidence refers to its exact SHA.
- [ ] Resolve the current policy mismatch: `main` allows only squash merges. If
      preserving both histories with a merge commit is chosen, deliberately
      update the ruleset for this promotion and restore the normal policy
      afterwards. Do not bypass the rules silently.
- [ ] Wait for every promotion PR check to finish successfully before merging.

Main-only commit disposition:

| Commit/range | Behavior | Disposition | Evidence/notes |
|---|---|---|---|
| `cec6d16a..4a38afde` | 45 commits to classify individually | | |

Conflict-resolution notes:

| Path | Resolution | Why it is correct | Test/evidence |
|---|---|---|---|
| | | | |

## 10. Qualify the resulting `main` SHA

- [ ] Record the merged `main` SHA and freeze it as the only tag candidate.
- [ ] Confirm GitHub reports that commit as signed and verified.
- [ ] Confirm the `main` push CI run completes successfully rather than being
      cancelled by another push.
- [ ] Inspect the `Corpus Read Regression Gate` log for an explicit PASS and
      zero lost proven reads.
- [ ] Confirm the generated-table aggregate and every shard pass.
- [ ] Confirm release-profile builds pass on the merged SHA.
- [ ] Re-run or transfer parity and benchmark receipts only when their source,
      dependencies, configuration, and instrument are proven identical.
- [ ] Confirm the documentation build and factual audit refer to this SHA.
- [ ] Run `OXIDEX_TAG_DRY_RUN=1 just tag 2.0.0-beta.1 <main-sha>` and retain the
      output.
- [ ] Confirm no local or remote `v2.0.0-beta.1` tag or GitHub release exists.
- [ ] Obtain the maintainer's explicit final publish decision.

Final candidate:

| Field | Value |
|---|---|
| `main` SHA | |
| GitHub verification | |
| CI run | |
| Parity receipt | |
| Benchmark receipt | |
| Docs receipt | |
| Tag dry run | |

## 11. Tag, publish, and verify

- [ ] Create signed tag `v2.0.0-beta.1` at the frozen `main` SHA using
      `just tag 2.0.0-beta.1 <main-sha>`.
- [ ] Verify the tag signature locally before pushing.
- [ ] Push the tag with the required SSH identity.
- [ ] Do not delete, move, or recreate the tag while workflows are running.
- [ ] Watch both Release and Docker workflows through terminal completion.
- [ ] Verify the GitHub release is named correctly, marked prerelease, and not
      marked Latest.
- [ ] Download and inspect every published artifact:
  - Linux x86_64 musl binary;
  - Linux ARM64 musl binary;
  - Windows x86_64 executable;
  - signed macOS ARM64 binary;
  - signed, notarized, and stapled macOS DMG.
- [ ] Run basic `--version` and metadata-reading smoke tests on applicable
      artifacts.
- [ ] Verify the downloaded macOS binary's signature and assess the downloaded
      DMG with `codesign`, `spctl`, and `stapler`, not only the build-directory
      originals.
- [ ] Verify exact beta Docker tags resolve and run, and verify stable floating
      tags did not move.
- [ ] Verify stable docs behavior matches the documented prerelease policy.
- [ ] Record final URLs, artifact checksums, workflow runs, and observed smoke
      results below.

Published release evidence:

| Artifact/result | URL or checksum | Verification |
|---|---|---|
| GitHub prerelease | | |
| Linux x86_64 | | |
| Linux ARM64 | | |
| Windows x86_64 | | |
| macOS ARM64 binary | | |
| macOS DMG | | |
| Docker beta tags | | |

## 12. Failure and rollback rules

- [ ] A failed or refused measurement is not a pass; preserve its logs and fix
      the instrument or defect before proceeding.
- [ ] Do not retag a different commit as `v2.0.0-beta.1` after publication.
      Fix post-publication defects in a later prerelease such as beta.2.
- [ ] Do not delete a remote tag or GitHub release without an explicit
      maintainer decision and an impact plan covering users, Docker tags, and
      cached artifacts.
- [ ] If promotion PR validation fails, leave the integration branch and
      worktree intact for diagnosis.
- [ ] If post-merge `main` validation fails, do not tag. Fix forward through a
      reviewed PR unless the maintainer explicitly chooses a revert.
- [ ] If a release workflow partially publishes, inventory every external
      artifact before taking cleanup action; do not assume workflow failure
      means nothing was published.

## Open decisions and notes

| Date | Decision or blocker | Owner | Status/next action |
|---|---|---|---|
| | | | |

## Final sign-off

- [ ] Functional/parity sign-off
- [ ] macOS signing/notarization sign-off
- [ ] Documentation and benchmark sign-off
- [ ] CI/CD and artifact sign-off
- [ ] History and main-reconciliation sign-off
- [ ] Maintainer publish approval
- [ ] `v2.0.0-beta.1` verified on `main`
