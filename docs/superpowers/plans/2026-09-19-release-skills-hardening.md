# Release Skills Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make OxiDex release skills executable and machine-checked through three sequential, independently reviewed and squash-merged PRs.

**Architecture:** Phase 1 adds standard-library receipt validation and repairs the release oracle, promotion, and Apple-evidence contracts. Phase 2 adds a tracked Playwright-based documentation audit. Phase 3 parameterizes and compresses the skills, improves Desktop metadata, and replaces remaining textual checks with behavioral tests. `.claude/skills` stays canonical and `.agents/skills` remains a generated exact mirror.

**Tech Stack:** Python 3 standard library and `unittest`, JSON Schema-compatible checked templates, Bash, Node.js 24, Playwright, VitePress, GitHub CLI, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-19-release-skills-hardening-design.md`

## Global Constraints

- One phase per `staging/release-skills-phaseN` branch and isolated worktree.
- Base every phase on freshly fetched `origin/refactor/tag-machinery` after the previous phase is squash-merged.
- Never edit the protected main checkout directly; preserve its five user-owned untracked files.
- Keep `.claude/skills` canonical and update `.agents/skills` only with `python3 tools/ci/sync_agent_skills.py --write`.
- Write and run a failing test before every implementation change.
- Run Cargo work only through the shared lock and a phase-specific `CARGO_TARGET_DIR`; these phases should not need Cargo changes.
- Never invoke bare `exiftool`; capability-check the explicit Perl/tree pair.
- No phase promotes to `main`, pushes a release tag, publishes a release, modifies Pages settings, or reads signing secrets.
- Push with `GIT_SSH_COMMAND="ssh -o IdentityAgent=none -o IdentitiesOnly=yes -i /Users/allen/.ssh/id_es25519_swackhamer"`.
- For each PR: attach it to the task, wait for all required checks, inspect review decision and unresolved actionable conversations, fix failures through RED→GREEN, squash-merge, fetch, and fast-forward the local integration checkout before starting the next phase.

## Review Focus

- A receipt with `status: verified` but a missing or null required evidence field must fail validation with the exact field path.
- A valid receipt for another candidate/version must not pass merely because its schema is valid.
- Oracle selection must use the repository pin and explicit capable Perl/tree; PATH fallback must remain prohibited for release evidence.
- The DMG claim must distinguish executable code signing from image notarization/stapling.
- The browser audit must detect unlinked routes, bad fragments, HTTP failures, and missing responsive/theme matrix cells without relying on sidebar traversal.

---

## Phase 1 PR: Evidence integrity and release safety

### Task 1: Add failing receipt-validator tests

**Files:**
- Create: `tools/ci/test_validate_release_receipt.py`
- Create: `tools/ci/testdata/release_receipts/*.json`
- Test: `tools/ci/test_validate_release_receipt.py`

**Interfaces:**
- Consumes: receipt JSON plus expected version/candidate SHA.
- Produces: tests defining `validate(kind, receipt, expected_version, expected_sha) -> list[str]` and CLI exit behavior.

- [ ] Create fixtures for all three untouched templates and minimally complete verified receipts.
- [ ] Add tests proving templates are accepted only with `--template`, not as verified evidence.
- [ ] Add one test per required semantic: matching identity, upstream receipt hash, positive/satisfied native floor, four parity families, docs browser/human review, Pages pipeline, PR/review-thread proof, authorization, tag verification, workflow identity, artifact hashes, and macOS proof.
- [ ] Run `python3 -m unittest tools.ci.test_validate_release_receipt -v`.
- [ ] Expected: import failure for `tools.ci.validate_release_receipt`.

### Task 2: Implement schemas and validator

**Files:**
- Create: `tools/ci/validate_release_receipt.py`
- Create: `.claude/skills/exiftool-parity/templates/release-parity-receipt.schema.json`
- Create: `.claude/skills/oxidex-release-documentation/templates/documentation-release-receipt.schema.json`
- Create: `.claude/skills/oxidex-release-finalization/templates/release-finalization-receipt.schema.json`
- Modify: all three receipt templates
- Modify: `tools/ci/test_agent_skills.py`

**Interfaces:**
- Produces: `load_receipt(path)`, `validate_receipt(kind, payload, expected_version=None, expected_sha=None, template=False)`, and CLI `--kind --receipt [--version] [--candidate-sha] [--template]`.
- Errors: stable dotted field paths, one error per line, exit 2 for invalid input and 0 for valid input.

- [ ] Implement structural helpers for objects, strings, hashes, status values, arrays, and dotted-path errors.
- [ ] Implement kind-specific semantic validation matching the test fixtures.
- [ ] Expand finalization template with explicit `candidate_tree`, `main_tree`, upstream receipt identities/hashes, `packaging`, `promotion`, `authorization`, `tag_verification`, and structured workflow/artifact sections.
- [ ] Add parity floor evidence and explicit docs benchmark disposition fields.
- [ ] Run the focused validator tests; expected all pass.
- [ ] Run template validation for all three kinds with `--template`; expected exit 0.

### Task 3: Repair oracle and finalization contracts

**Files:**
- Modify: `.claude/skills/exiftool-parity/SKILL.md`
- Modify: `.claude/skills/exiftool-parity/references/harnesses.md`
- Modify: `.claude/skills/exiftool-parity/references/release-metrics.md`
- Modify: `.claude/skills/oxidex-release-finalization/references/gates.md`
- Modify: `.claude/skills/oxidex-release-finalization/references/github-release-and-macos.md`
- Modify: `.github/workflows/release.yml`
- Modify: `docs/contributing/release-checklist.md`
- Modify: `tools/ci/test_agent_skills.py`

**Interfaces:**
- Oracle inputs: `EXIFTOOL_PERL`, `EXIFTOOL_CACHE_DIR`, repository `.exiftool-version`.
- Review proof: persisted GraphQL response with zero unresolved actionable threads.
- Apple proof: expected Developer ID and TeamIdentifier inputs plus raw/mounted executable and DMG notarization evidence.

- [ ] Add RED tests rejecting the dated `/tmp/oxidex-perl538-build-*` path and requiring the durable configurable defaults and validator calls.
- [ ] Add RED tests that execute the oracle probe recipe against fixture executables and reject PATH fallback.
- [ ] Add RED tests requiring GraphQL unresolved-thread evidence before merge.
- [ ] Add RED tests asserting workflow/skills call the artifact a notarized/stapled DMG containing a signed executable, not a signed DMG.
- [ ] Update the skill recipes and release wording minimally to pass.
- [ ] Run the real durable oracle module/version/DOCX probes and record their exact paths in the PR body.
- [ ] Sync `.agents/skills` from canonical.

### Task 4: Verify, review, and land Phase 1

**Files:**
- Modify: `docs/superpowers/skill-tests/2026-09-18-release-engineering-skills.md`
- Include: this specification and plan in the Phase 1 PR.

- [ ] Run both skill validators for all three canonical and mirrored skills.
- [ ] Run `python3 -m unittest tools.ci.test_validate_release_receipt tools.ci.test_agent_skills -v`.
- [ ] Run `python3 -m unittest discover -s tools/ci -p 'test_*.py'`.
- [ ] Run `python3 tools/ci/sync_agent_skills.py --check`, `git diff --check`, `typos` on changed files, and `actionlint` for changed workflows.
- [ ] Run fresh positive and negative implicit trigger probes for parity and finalization; append prompts/results to the skill-test record.
- [ ] Dispatch a whole-branch reviewer; fix Critical/Important findings through RED→GREEN and rerun the suite.
- [ ] Commit, push, open a PR to `refactor/tag-machinery`, attach it, and wait for required CI and review resolution.
- [ ] Squash-merge, fetch `origin/refactor/tag-machinery`, fast-forward the local integration checkout without touching untracked files, and verify the merge tree.

---

## Phase 2 PR: Executable documentation and Pages audit

### Task 5: Add browser-audit RED fixtures

**Files:**
- Modify: `docs/package.json`
- Modify: `docs/package-lock.json`
- Create: `tools/docs/release-audit.mjs`
- Create: `tools/docs/test-release-audit.mjs`
- Create: `tools/docs/testdata/release-audit-site/**`
- Modify: `tools/docs-local-deploy.sh`

**Interfaces:**
- CLI: `node tools/docs/release-audit.mjs --dist DIR --inventory FILE --output DIR --representatives FILE`.
- Output: `crawl.json`, `visual-matrix.json`, deterministic screenshots, server log, and exit status.

- [ ] Add Node tests for route inventory completeness, 404/500 responses, local asset failure, fragment failure, console/page/request failures, and exact matrix cells.
- [ ] Add shell test proving `tools/docs-local-deploy.sh --build-only --output DIR` terminates and writes a snapshot manifest.
- [ ] Run the tests before implementation; expected failures for missing commands/options.

### Task 6: Implement the deterministic local audit

**Files:** same as Task 5 plus:
- Modify: `.claude/skills/oxidex-release-documentation/SKILL.md`
- Modify: `.claude/skills/oxidex-release-documentation/references/github-pages-audit.md`
- Modify: `.claude/skills/oxidex-release-documentation/templates/documentation-release-receipt.json`
- Modify: `tools/ci/test_agent_skills.py`

- [ ] Pin Playwright and add `docs:audit-release`.
- [ ] Implement build-only snapshot output without changing the existing interactive default.
- [ ] Implement exhaustive route/asset/fragment crawl and representative desktop/mobile light/dark matrix.
- [ ] Add deterministic screenshots, interaction checks, structured errors, and nonzero failure semantics.
- [ ] Update the skill to invoke the tracked commands and point receipt fields at their manifests.
- [ ] Sync the agent mirror and run unit plus real local fixture browser tests.

### Task 7: Verify, review, and land Phase 2

- [ ] Run docs build, link checks, browser fixture suite, a production-equivalent build-only snapshot, and the audit against that snapshot.
- [ ] Run all skill validators, mirror check, focused tests, full `tools/ci` discovery, `git diff --check`, and `typos`.
- [ ] Run fresh positive/negative documentation trigger probes and record them.
- [ ] Obtain whole-branch review and resolve Critical/Important findings through RED→GREEN.
- [ ] Push, open/attach the Phase 2 PR, wait for required CI and actionable review resolution, squash-merge, and synchronize the local integration checkout.

---

## Phase 3 PR: Reuse, discovery, and regression hardening

### Task 8: Parameterize and compact skills

**Files:**
- Modify: all three canonical `SKILL.md` entrypoints and focused references
- Create: `.claude/skills/exiftool-parity/agents/openai.yaml`
- Modify: the other two `agents/openai.yaml`
- Modify: `tools/ci/test_agent_skills.py`
- Modify: `docs/superpowers/skill-tests/2026-09-18-release-engineering-skills.md`

- [ ] Add RED tests requiring all three allowlisted skills, valid UI metadata/default prompts, implicit invocation, and entrypoints below 500 words.
- [ ] Add RED tests rejecting hard-coded beta version, tag, asset, and dated lock paths from generic recipes/templates.
- [ ] Add behavioral fixtures for receipt validation and shell snippets so required tokens in comments cannot pass.
- [ ] Add `not_applicable` benchmark disposition and clarify representative screenshot matrix language.
- [ ] Shorten entrypoints, move conditional mechanics to references, parameterize inputs, and update metadata.
- [ ] Sync mirrors and run validator/tests.

### Task 9: Final trigger pressure tests and Phase 3 landing

- [ ] Run at least one positive and one negative implicit trigger scenario for each skill in fresh Codex contexts.
- [ ] Run one full-release pressure scenario; require parity → documentation → finalization ordering and correct authorization stops.
- [ ] Run the complete skill, validator, docs-audit, workflow, and `tools/ci` suites.
- [ ] Run Codex CLI and Codex Desktop discovery smoke tests and record versions/results.
- [ ] Obtain whole-branch review; resolve Critical/Important findings through RED→GREEN.
- [ ] Push, open/attach the Phase 3 PR, wait for required CI and actionable review resolution, squash-merge, and synchronize the local integration checkout.
- [ ] Verify all three squash commits are ancestors of local and remote `refactor/tag-machinery`, with the protected checkout's original untracked files preserved.
