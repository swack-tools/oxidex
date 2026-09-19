# Release engineering skills design

Date: 2026-09-18

## Purpose

Codify the release work that has repeatedly been reconstructed by hand. The
system must finish with the selected release commit merged to `main`, a signed
tag pointing at that `main` commit, truthful release documentation, and
post-tag proof that the GitHub release and its advertised artifacts exist.

This design adds three cooperating project-local skills rather than one large
release prompt:

```text
exiftool-parity
      |
      v
oxidex-release-documentation
      |
      v
oxidex-release-finalization -> reviewed PR to main -> signed tag -> artifact proof
```

Each stage emits a receipt that the next stage consumes. A receipt may say
`verified`, `unverified`, or `blocked`; it may never turn an unavailable check
into a pass.

## Safety boundary

Ordinary feature and parity work continues to land on
`refactor/tag-machinery` through `staging/<slug>` branches. The release skill
does not weaken that rule. It defines a separate, maintainer-authorized
promotion path:

1. prepare a release branch in its own worktree;
2. open and review a PR whose base is `main`;
3. require the exact merge commit on `main` to pass the release gates;
4. pause for the maintainer's explicit tag/publish authorization;
5. create and push a signed tag at that exact `main` commit;
6. verify the resulting release and artifacts.

No skill may push directly to `main`, move an existing tag, force-push a
protected branch, expose signing/notarization secrets, or describe workflow
wiring as proof that Apple accepted an artifact.

## Skill 1: `oxidex-release-finalization`

### Trigger

Use when preparing, promoting, tagging, publishing, or validating an OxiDex
release or release candidate. It owns the transition from a frozen candidate
to `main`, the signed tag, and post-publication verification.

### Inputs

- release version and expected tag;
- integration branch and candidate commit;
- release-documentation receipt;
- ExifTool parity receipt;
- explicit packaging decisions such as crates.io and Docker publication.

### Workflow

1. Run repository/worktree preflight and create a unique evidence directory.
2. Inventory every version-bearing workspace package and user-facing version
   reference. Record intentional independent versions rather than silently
   normalizing them.
3. Freeze the candidate commit. All local receipts and CI links must name this
   commit; "current tip" is not evidence.
4. Consume the documentation and parity receipts. Refuse missing, stale, or
   commit-mismatched receipts.
5. Run the repository's locked local gates and the release-workflow tests.
   Record exact commands, exit codes, commit, target directory, and logs.
6. Audit `.github/workflows/release.yml` and `docker.yml` against the expected
   asset matrix. Run `actionlint` and the repository's workflow tests. Secret
   names may be checked without reading values; secret presence is not proof
   that a certificate or Apple credential is valid.
7. Open a reviewed PR to `main`. After merge, freeze the exact `main` merge
   commit and require successful CI for that commit. If the merge changes the
   tree, invalidate candidate-bound receipts and rerun them.
8. Run the signed-tag dry run against that exact `main` commit. Pause for the
   maintainer's explicit authorization before pushing the real tag.
9. Monitor the tag-triggered workflows. Verify GitHub prerelease/latest
   classification, expected assets, Docker behavior, and checksums when the
   workflow publishes them.
10. Download the macOS artifact and verify the code signature, Gatekeeper
    assessment, and stapled notarization ticket. A pre-tag report may say only
    "workflow path validated; signing credentials unverified". Apple
    verification is complete only after the real artifact passes the checks.

### Output

A release-finalization receipt containing commit and tag identity, every gate
and CI URL, version inventory, expected and observed assets, signature and
notarization results, unresolved items, and the next safe action.

## Skill 2: `oxidex-release-documentation`

### Trigger

Use when making the release notes, changelog, README, installation guidance,
API/migration documentation, benchmarks, parity claims, or release checklist
truthful for a particular OxiDex release.

### Workflow

1. Freeze the candidate commit and build a statement ledger: each material
   claim maps to its source, instrument, measured commit, freshness rule, and
   status (`verified`, `unverified`, or `blocked`).
2. Audit the changelog for a readable release section, release date, notable
   changes, breaking changes, limitations, and links. The target version may
   not remain labelled `Unreleased` when the final receipt is issued.
3. Reconcile version labels, install commands, package availability, supported
   platforms, artifact names, and beta/stability language with the actual
   workflows and packaging decision.
4. Accept benchmark numbers only from the named benchmark instrument on the
   candidate commit, or from a commit proven equivalent by a relevant-path
   diff. Separate shipped-profile CLI timings, indicative CI timings, and
   Criterion microbenchmarks; never combine or compare them as one metric.
5. Consume the ExifTool parity receipt. Clearly separate observed extraction
   matches, missing/value/rename/extra differences, authenticated source-row
   credit, generated declarations, detected-only formats, and write coverage.
6. Audit every page that VitePress will publish, including pages omitted from
   the navigation and pages generated only during deployment. Classify each as
   current release documentation, explicitly historical/reference material,
   or not suitable for publication. Every current page must be verified
   against the candidate commit; historical pages must be visibly labelled so
   an old number or plan cannot be mistaken for current release behavior.
7. Reproduce the production Pages build with
   `tools/docs-local-deploy.sh` from the frozen candidate, including the real
   generated comparison report and the selected benchmark artifact. Crawl the
   rendered output, require every internal route and asset to resolve, and
   compare the rendered route inventory with the source-page inventory.
8. Review the rendered site visually at desktop and mobile widths, in light
   and dark themes, with representative screenshots. Check the home page,
   guide, reference, parity report, status, performance, changelog, migration,
   and installation pages for navigation, overflow, unreadable tables/code,
   missing assets, console errors, and misleading banners/version labels.
9. Audit the GitHub Pages CI/CD path itself: triggers, path filters, generated
   report handoff, benchmark provenance, permissions, Pages artifact upload,
   deployment environment, custom domain/base path, and concurrency. Validate
   workflow syntax and repository tests, and inspect the live Pages settings
   without exposing credentials.
10. After the release promotion reaches `main`, require a successful
    `deploy-docs.yml` run for the exact commit and crawl the deployed site.
    Confirm that the live pages expose the expected release/version marker and
    that the deployed route/content hashes correspond to the reviewed build.
11. Produce a concise human release summary plus a machine-readable receipt.

### Output

A documentation receipt keyed to the candidate commit, with a claim ledger,
per-page classification and evidence, benchmark provenance, parity artifact
identity, changelog status, local production-build/crawl/visual results,
GitHub Pages workflow and settings results, exact-commit deployment URL, live
site crawl, and unresolved claims. Release finalization refuses a stale or
partial receipt.

### GitHub Pages facts the implementation must reconcile

The present repository has two different documentation mechanisms that must
not be assumed equivalent:

- `.github/workflows/deploy-docs.yml` is the live Pages pipeline. It runs from
  `main`, generates the ExifTool comparison report, optionally imports a
  Criterion artifact, builds VitePress, uploads a Pages artifact, and deploys
  it with `actions/deploy-pages`.
- `.github/workflows/release.yml` still contains a stable-release job that
  edits and pushes the `gh-pages` branch. Repository documentation says Pages
  uses GitHub Actions as its source. A live API check on 2026-09-18 confirmed
  `build_type: workflow`, `cname: oxidex.net`, and HTTPS enforcement (the API
  also retains `source.branch: gh-pages`, which is not proof that branch pushes
  deploy in workflow mode). The release job may therefore be obsolete or
  ineffective. The documentation skill must verify the setting again and
  either repair/remove the dead path or document why it remains. It may not
  call the release workflow's `gh-pages` push a successful documentation
  deployment without proof.

The deploy workflow is also allowed to fall back to an older successful
benchmark artifact and stamp the commit that actually produced it. That is
honest provenance, but it is not automatically current release evidence. For
a release, require a benchmark artifact for the candidate commit or label the
published performance numbers prominently as historical and exclude them from
claims about the new tag.

### Whole-site factuality model

The site audit is exhaustive over the rendered Pages artifact, not just the
sidebar. Its inventory includes committed Markdown, included files, generated
comparison pages, status pages, and copied Criterion reports. Each route gets:

- source or generator;
- current / historical / excluded classification;
- release version and commit applicability;
- material factual claims and their evidence;
- generated-data commit and oracle provenance, where applicable;
- link/crawl result and visual-review status;
- disposition for stale content: update, label/archive, or remove from the
  published artifact.

Plans, old checkpoints, previous-release measurements, and migration history
may remain available when they are useful, but the rendered page must identify
their date/version and must not present them as the state of the tag being
released.

## Skill 3: `exiftool-parity` revision

The existing skill remains the authority for ExifTool comparison but is
rewritten around the current instrument doctrine.

### Required corrections

- Remove every bare `exiftool` invocation. Resolve the release from
  `.exiftool-version`, invoke the pinned tree with the canonical Perl, and
  require both the version and DOCX capability probes.
- Require explicit OxiDex binary provenance, clean-tree state, corpus roots,
  recursive mode when needed, file/tag floors, unique output paths, and
  `--json-out`; formatted console output is not a release artifact.
- Regenerate the baseline from the same base commit instead of accepting a
  supplied stale result.
- Treat `MISSING`, `VALUE`, `RENAME`, and `EXTRA` separately, including
  per-format and per-file data, duplicate occurrences, and group-qualified
  identities.
- Keep these measurement families separate:
  1. conformance of observed public output;
  2. authenticated corpus read/source-coordinate credit;
  3. generated declaration/catalog coverage;
  4. JPEG read/write matrix coverage.
- Include denominators and refusal floors: corpus file count, ExifTool tag
  occurrences, matched occurrences, score, rename ceiling, precision, and
  regression deltas. Do not award unobserved generated rows as read parity.
- Emit a release-parity receipt that the documentation skill can consume.

The current local canonical Perl installation is known to be missing its
standard library. The revised skill must classify that as a refused
measurement and provide the exact recovery prerequisite; it must not fall back
to a different Perl or a bare ExifTool.

## Project-local layout

Claude currently reads tracked skills from `.claude/skills/`; Codex discovers
project skills from `.agents/skills/`. Keep `.claude/skills/` canonical and
track an exact `.agents/skills/` mirror. Add a small sync/check helper and CI
test so the two copies cannot drift silently.

Planned files:

```text
.claude/skills/oxidex-release-finalization/
.claude/skills/oxidex-release-documentation/
.claude/skills/exiftool-parity/
.agents/skills/oxidex-release-finalization/
.agents/skills/oxidex-release-documentation/
.agents/skills/exiftool-parity/
tools/ci/sync_agent_skills.py
tools/ci/test_agent_skills.py
```

Each skill keeps the entrypoint concise and places detailed command matrices,
receipt schemas, and templates under `references/` or `templates/`.

## AGENTS.md and CLAUDE.md changes

`AGENTS.md` should gain only a short release-routing section naming the three
skills and the receipt order. The detailed procedure belongs in the skills.

`CLAUDE.md` should distinguish normal development from an explicitly
authorized release promotion. Its current unconditional statement that work
never reaches `main` is correct for ordinary changes but incomplete for the
new release outcome. It should route release work to the new skills without
duplicating their checklists.

### CLAUDE.md quality report

Discovery found the root `CLAUDE.md` plus two copies inside nested linked
worktrees. The nested copies are not independent project configuration and are
out of scope. Root assessment: **83/100 (B)**.

| Criterion | Score | Finding |
| --- | ---: | --- |
| Commands/workflows | 18/20 | Strong daily commands and landing flow; no complete main/tag publication route. |
| Architecture clarity | 15/20 | Deliberately delegates repository substance to `AGENTS.md`; adequate but release ownership is implicit. |
| Non-obvious patterns | 15/15 | Captures worktree, parity, target collision, and remote-operation hazards. |
| Conciseness | 14/15 | Dense and useful; limited duplication. |
| Currency | 8/15 | The absolute `main` prohibition conflicts with the newly specified release end state. |
| Actionability | 13/15 | Normal development is actionable; final release promotion and artifact proof are not routed. |

Targeted update: preserve all normal-development protections, add the narrow
maintainer-authorized release exception, and link to the receipt chain.

## Validation strategy

Skills are tested one at a time.

1. Capture a pressure-test baseline without the new or revised skill.
2. Add one skill and its references.
3. Validate frontmatter and structure with the skill validator.
4. Re-run the same pressure scenario with the skill loaded. Scenarios include:
   - pressure to tag the integration branch before it reaches `main`;
   - pressure to call workflow wiring proof of Apple notarization;
   - pressure to copy stale benchmarks or parity totals into release notes;
   - pressure to approve the site after checking only navigation-linked pages;
   - pressure to treat a green cold build, a `gh-pages` push, or workflow YAML
     as proof that the production Pages deployment is correct and attractive;
   - pressure to use bare ExifTool or a degraded Perl after the pinned oracle
     refuses to run.
5. Add deterministic repository tests for mirror equality, forbidden bare
   ExifTool commands, required safety concepts, and referenced file existence.
6. Run the existing release/parity unit tests, docs build, formatting checks,
   and relevant locked Rust gates.
7. Review the final diff for generated files, credentials, unrelated changes,
   and accidental weakening of protected-branch rules.

No real merge to `main`, tag push, GitHub release, crates.io publication,
Docker publication, or secret mutation is part of implementing these skills.

## Acceptance criteria

- Both agents discover the same three project-local skills.
- The release skill ends at `main` plus a signed tag and requires post-tag
  artifact evidence.
- Pre-tag checks cannot be misreported as Apple notarization proof.
- The documentation skill rejects stale or unsupported benchmark/parity
  claims.
- The documentation receipt accounts for every rendered Pages route, and
  current pages are tied to evidence from the candidate commit while older
  pages are visibly historical or removed from publication.
- The production-shaped local site passes route/asset crawling and responsive
  visual review, and the live Pages deployment is verified at the exact
  `main` commit rather than inferred from workflow configuration.
- The skill detects and resolves any mismatch between the repository's live
  GitHub Actions Pages source and legacy `gh-pages` branch updates.
- The parity skill never invokes bare ExifTool and emits release-consumable,
  provenance-rich metrics.
- `AGENTS.md` and `CLAUDE.md` route work without duplicating long procedures.
- Skill pressure tests and repository checks pass, and the existing release
  and parity tests remain green.
