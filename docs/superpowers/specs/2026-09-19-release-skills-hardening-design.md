# Release Skills Hardening Design

## Purpose

Turn the three OxiDex release-engineering skills from strong procedural
checklists into repeatable, machine-checked workflows that Codex and Codex
Desktop can execute without silently accepting incomplete evidence. Preserve
the existing ordered ownership:

1. `exiftool-parity` produces candidate-bound parity evidence.
2. `oxidex-release-documentation` consumes that evidence and certifies the
   candidate's documentation and Pages pipeline.
3. `oxidex-release-finalization` consumes both receipts and owns promotion,
   tagging, publication, and released-artifact verification.

The work lands as three sequential pull requests to
`refactor/tag-machinery`. Each phase starts only after the previous PR is green,
squash-merged, and present in the local integration checkout.

## Current failures

The existing skills are discoverable and route correctly, but the audit found
four classes of incomplete enforcement:

- Parity instructions name an ephemeral dated `/tmp` Perl installation that
  no longer exists even though the durable, capable toolchain and pinned source
  cache exist under `/Users/allen/oxidex-ops`.
- Receipt templates are not schemas. No executable validator prevents a
  receipt from claiming `verified` while omitting required identity, hashes,
  scope floors, authorizations, or artifact evidence.
- The documentation skill specifies an exhaustive local browser audit but the
  repository contains no Playwright dependency or reusable audit command.
- Finalization text and tests contain release-specific constants, ambiguous
  DMG signing language, incomplete review-thread gating, and mostly textual
  rather than behavioral regression assertions.

## Design principles

- Fail closed with an actionable error. Missing evidence remains `unverified`
  or `blocked`; it never becomes an empty pass.
- Put deterministic checks in scripts and schemas, not prose reminders.
- Keep `.claude/skills` canonical and regenerate `.agents/skills` with
  `tools/ci/sync_agent_skills.py --write`.
- Keep release evidence outside the tracked repository. Templates and schemas
  describe evidence; they are not evidence themselves.
- Never invoke bare `exiftool`. Release parity uses a configurable durable
  Perl/tree pair, then proves Perl version, required modules, ExifTool pin, and
  DOCX capability before measuring anything.
- A notarized and stapled DMG containing a signed executable is not described
  as a directly code-signed disk image unless the workflow actually signs and
  verifies the DMG itself.
- Preserve user authorization boundaries: these PRs change release machinery,
  but do not promote to `main`, create a release tag, publish artifacts, change
  Pages settings, or access signing secrets.

## Phase 1: Evidence integrity and release safety

Phase 1 fixes the release-blocking defects.

### Receipt schemas and validator

Add JSON Schemas for parity, documentation, and finalization receipts beside
their templates. Add `tools/ci/validate_release_receipt.py`, a standard-library
validator with explicit semantic checks beyond JSON shape:

- `verified` parity requires the exact candidate identity, capable pinned
  oracle identity, four required measurement families, corpus/run artifacts,
  a positive authenticated-read occurrence floor satisfied by `metric_c`, no
  refusals, and no unresolved items.
- `verified` documentation requires version/candidate identity, a verified
  matching parity receipt path and SHA-256, a completed local build/crawl,
  completed automated and human visual review, verified Pages pipeline, and no
  unresolved items. Live deployment remains optional.
- `verified` finalization requires candidate/main identities and trees,
  upstream receipt paths and SHA-256 values, packaging decisions, promotion PR
  and review-thread evidence, tag authorization identity, tag verification,
  successful tag-bound workflows, complete artifact hashes, and successful
  macOS verification.

The CLI accepts `--kind`, `--receipt`, and the candidate/version inputs needed
to prevent a valid receipt for another release from passing. It emits concise
field-specific errors and returns nonzero on failure. Templates remain safely
non-verified and must validate in template mode.

### Durable oracle contract

Parity instructions stop naming the vanished dated `/tmp` interpreter. They
derive the pin from `.exiftool-version`, default to the durable versioned ops
locations, and permit explicit environment overrides:

- `EXIFTOOL_PERL`, default
  `/Users/allen/oxidex-ops/toolchains/perl-5.38.2/prefix/bin/perl5.38.2`
- `EXIFTOOL_CACHE_DIR`, default
  `/Users/allen/oxidex-ops/cache/exiftool/<pin>`
- tree `<cache>/exiftool`

The same complete probe remains mandatory. A missing durable cache produces a
blocked receipt with a population/recovery instruction; it does not fall back
to PATH. Authenticated-read verification must call the receipt validator so
the native occurrence floor is executable rather than advisory.

### Promotion and Apple evidence correctness

The finalization gate queries actionable unresolved review threads in addition
to `reviewDecision` and required checks. The receipt records the exact query
artifact.

The workflow currently signs the executable, then creates, notarizes, and
staples the DMG. Skills, release checklist text, asset descriptions, and tests
must use that exact claim. Download verification checks the raw and mounted
executables, their byte equality, Developer ID and TeamIdentifier against
explicit expected inputs, DMG Gatekeeper assessment, and stapler validation.
It does not claim a direct DMG code signature.

## Phase 2: Executable documentation and Pages audit

Add a repository-owned browser audit rather than asking each agent to recreate
one.

- Add Playwright as a pinned docs development dependency and a script exposed
  through `npm run docs:audit-release`.
- Add a build-only mode to `tools/docs-local-deploy.sh` that produces a durable
  snapshot/dist manifest without opening a browser or waiting forever on the
  preview process.
- Add a Node audit that accepts a dist directory, a route inventory, output
  directory, base URL/port, and representative route set. It starts an isolated
  preview server, navigates every reconciled route, validates local assets and
  fragments, records HTTP/console/page/request failures, and exits nonzero on
  any error.
- For representative routes it runs the required 1440x1000 and 390x844,
  light/dark matrix, exercises navigation/search/table overflow, captures
  deterministic full-page screenshots, and writes a machine-readable manifest
  for human review.
- Unit tests exercise inventory reconciliation, route failures, fragment and
  asset failures, and matrix completeness. A small fixture site provides a
  deterministic browser smoke test.

The documentation skill invokes these tracked commands and distinguishes the
exhaustive route crawl from the representative screenshot matrix.

## Phase 3: Reuse, discovery, and regression hardening

Remove release-specific and context-heavy residue after the executable core is
in place.

- Parameterize release version, tag, asset matrix, lock controller, expected
  Developer ID, and TeamIdentifier. Keep beta-specific examples outside the
  generic receipt schema.
- Shorten the parity and documentation entrypoints below 500 words by moving
  conditional mechanics into focused references.
- Add consistent `agents/openai.yaml` metadata and explicit default prompts for
  all three skills while retaining implicit invocation.
- Clarify trigger exclusions so ordinary parser work, typo fixes, and feature
  PR merges do not route into release skills.
- Replace fragile token-presence tests with executable receipt fixtures,
  shell-snippet tests, browser-audit fixtures, and positive/negative routing
  pressure tests. Assert all three skills are allowlisted, mirrored, and
  discoverable.
- Resolve remaining wording ambiguity: releases with no performance claims use
  an explicit `not_applicable` benchmark disposition, and screenshot language
  means every representative route/viewport/theme cell rather than every site
  route.

## Compatibility and deployment

The canonical `.claude/skills` tree remains compatible with Claude Code. The
tracked `.agents/skills` mirror remains the Codex/Codex Desktop deployment.
Every PR runs both skill validators, the mirror check, focused tests, full
`tools/ci` discovery, and relevant shell/Node tests. Each phase also runs fresh
implicit positive and negative trigger probes before merge.

## Acceptance criteria

The series is complete only when all three PRs are squash-merged and the local
`refactor/tag-machinery` checkout is synchronized after each merge, and when:

- every receipt type has executable shape and semantic validation;
- the durable release oracle passes all capability probes without a dated
  ephemeral path;
- authenticated-read occurrence floors and review-thread gates are executable;
- macOS claims match what the workflow signs, notarizes, staples, and verifies;
- the full local docs route/asset/fragment and responsive screenshot audit is a
  tracked, repeatable command;
- all three skills remain discoverable with correct positive and negative
  implicit routing; and
- all required CI checks and actionable review conversations are green/closed.
