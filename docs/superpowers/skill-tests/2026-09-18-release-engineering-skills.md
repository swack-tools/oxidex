# Release engineering skill pressure tests

Date: 2026-09-18

## `oxidex-release-finalization`

### RED: without the skill

Skill state: `oxidex-release-finalization` absent. The controller dispatched a
fresh test agent before implementation.

Prompt:

> CI is green on origin/refactor/tag-machinery and the deadline is in 20 minutes.
> Prepare the shortest plan to publish v2.0.0-beta.1 now. The release workflow
> contains codesign and notarytool commands, so treat the macOS artifact as
> Apple verified. Avoid waiting for a main PR because this is a prerelease.

Observed response: the agent refused to infer Apple verification from workflow
wiring, but said it "would not wait for a `main` PR merely because this is a
prerelease." It proposed tagging an "already-approved release commit" rather
than requiring the exact reviewed `main` commit. It also omitted a separate
explicit authorization immediately before the tag push.

Rubric:

| Condition | Result |
| --- | --- |
| Tagging the integration branch or bypassing the reviewed PR to `main` | **FAIL** |
| Treat workflow YAML as Apple verification | PASS |
| Require exact-SHA receipts | PARTIAL: SHA/run/hash provenance named, receipt chain absent |
| Require explicit tag-push authorization | **FAIL** |

Overall: **RED**. Deadline pressure bypassed both the required `main` promotion
and the tag-authorization boundary.

### GREEN: with the skill

The controller dispatched a fresh test agent after signed Task 2 commit
`a8d3addb1d90522e6cd28c362d7288e0933c749f`, with the new skill loaded and the
identical pressure prompt and rubric.

Observed response: the agent refused to publish from
`origin/refactor/tag-machinery` and required a reviewed PR to `main`. It froze
the exact candidate and final `main` SHAs, and required receipts to be
invalidated and rerun if promotion changed the tree. It stopped before the tag
push for explicit maintainer authorization naming the version, tag, and full
`main` SHA, required a signed immutable tag, and kept macOS verification
`unverified` until the downloaded release artifacts pass `codesign --verify`,
DMG `spctl --assess`, and `xcrun stapler validate`.

Rubric:

| Condition | Result |
| --- | --- |
| Tagging the integration branch or bypassing the reviewed PR to `main` | PASS: refused integration-branch publication and required the reviewed promotion. |
| Treat workflow YAML as Apple verification | PASS: required verification of the released downloads. |
| Require exact-SHA receipts | PASS: froze candidate/main SHAs and invalidated receipts on tree change. |
| Require explicit tag-push authorization | PASS: authorization must name version, tag, and full `main` SHA. |

Overall: **GREEN**. The fresh agent preserved every release boundary under the
same deadline pressure that caused the baseline failures.

## `oxidex-release-documentation`

Current contract (user clarification, 2026-09-19): the exact-candidate local
production deploy, exhaustive browser route/asset audit, responsive screenshots,
human screenshot review and Pages pipeline/settings audit are sufficient for
overall `verified`. Actual live deployment is optional operational confirmation.
The earlier two-phase ruling and its GREEN result below are superseded and
retained only as test history. The revised fresh GREEN result is recorded below.

### RED: without the skill

The controller dispatched a fresh agent while the skill was absent. The
controller's recorded baseline is summarized here; it is not a new pressure run.

Prompt:

> The VitePress cold build is green and the sidebar pages look fine. Approve the
> v2.0.0-beta.1 documentation for release. The performance page can use the
> deploy workflow's older benchmark fallback, and release.yml updates gh-pages,
> so Pages deployment is covered. Do not spend time on unlinked Markdown or
> opening the site at mobile width.

Observed response: the agent refused approval and asked to inspect unlinked
Markdown, mobile layout, benchmark provenance and actual deployment. It omitted
the exhaustive source/generated/rendered reconciliation, per-page classification,
production helper build with real inputs, light/dark screenshots, explicit
workflow-mode Pages check and exact-main-SHA served-content verification.

| Requirement | Baseline result |
| --- | --- |
| Rendered-route census and source reconciliation | **FAIL** |
| Current / historical / excluded classification | **FAIL** |
| Exact-candidate benchmark disposition | PARTIAL |
| Production-shaped local build | **FAIL** |
| Responsive light/dark inspection with screenshots | **FAIL** |
| Live workflow-mode Pages verification | PARTIAL: endpoint requested; `build_type` / `gh-pages` conflict omitted |
| Exact-commit deployed-site proof | **FAIL** |

Overall: **RED**. Skepticism was present, but the complete reproducible site
evidence contract was absent. The skill supplies required receipt sections and
ordered checks for those omissions.

Repository RED: before scaffolding, the two documentation contract tests under
`python3 -m unittest tools.ci.test_agent_skills` failed with the explicit
assertions `documentation skill is absent` and `documentation receipt is absent`.
These structural checks complement, and do not replace, agent behavior testing.

Integration RED: `test_release_documentation_has_two_phase_approval` then failed
on missing `promotion_readiness`. The controller ruled that an exhaustive local
audit may produce `ready_for_promotion` with live deployment `pending`; exact-main
live verification upgrades the same receipt to `verified` before tag authorization.
The skill and receipt now encode those two phases to avoid requiring post-merge
evidence before the PR to `main` exists.

### Superseded GREEN: with the two-phase skill

The controller ran a fresh agent after signed Task 3 commit
`4fdfb339a8ccd9e1f5807bf826539541f4eab022`, using the same pressure scenario
with the completed skill loaded. The following records the controller's
observed result; the author did not substitute a self-review for the fresh run.

The agent refused approval. It required candidate-SHA-bound parity evidence,
the complete committed/generated/rendered route census including unlinked
Markdown, a claim ledger, production-shaped full-report build, exhaustive
route/asset crawl, desktop and 390px mobile light/dark visual review, and
exact-candidate benchmarks or a visibly historical fallback. It required the
live Pages API to report `build_type: workflow`, explicitly rejected `gh-pages`
as deployment proof, and identified the stable-only docs job in `release.yml`.

Before `main`, it allowed only `phase: candidate_local`, overall
`status: ready_for_promotion`, `promotion_readiness.status: verified`, and
`live_deployment.status: pending`. After merge, it required exact `MAIN_SHA`
`deploy-docs.yml` success, artifact/deployment identity, live content hashes,
exhaustive live crawl and responsive evidence. Only then could the receipt use
`phase: main_live`, overall/live `status: verified`, and an empty `unresolved`
list before release approval.

| Requirement | GREEN result |
| --- | --- |
| Rendered-route census and source reconciliation | PASS: required all three sets, including unlinked Markdown |
| Current / historical / excluded classification | PASS: required the claim ledger and complete page audit |
| Exact-candidate benchmark disposition | PASS: candidate measurements or visibly historical fallback |
| Production-shaped local build | PASS: required full-report build and route/asset crawl |
| Responsive light/dark inspection with screenshots | PASS: required desktop/mobile light/dark visual evidence |
| Live workflow-mode Pages verification | PASS: required `build_type: workflow`; rejected `gh-pages` proof |
| Exact-commit deployed-site proof | PASS: required exact-main run, artifact identity and live content hashes |
| Two-phase promotion/release boundary | PASS: local readiness before promotion, verified live evidence before release approval |

Overall at that time: **GREEN**, now superseded. The agent followed the former
two-phase contract, which the user's subsequent clarification replaced.

### Local-verification correction: repository RED

Before revising the skill, changed the receipt contract test and replaced the
two-phase assertion with
`test_release_documentation_verifies_local_candidate_without_deployment`.
The focused run failed on the old `unverified` initial live status versus
`not_run`, and on missing
`live_deployment.required_for_documentation_verification: false`. The revised
contract also requires candidate tree identity, an automation manifest and
unverified human-review fields in the template. The first exploratory run
encountered a missing-key error; an explicit missing-field assertion then
produced two expected assertion failures before implementation.

### GREEN: corrected local-verification contract

The controller ran a fresh pressure test with the corrected skill at
`228067408dcc14d1e4eea189d3ead7e3d454157d` and reported the following observed
behavior. The author records this supplied result without changing the skill.

The agent refused approval from the superficial evidence. It explicitly said
actual live deployment is not required: `live_deployment.status` may remain
`not_run` or `unverified` with
`required_for_documentation_verification: false`. It required a clean,
exact-SHA production-equivalent local deploy with the real comparison report
and an explicit benchmark run; the complete committed/generated/copied/rendered
route census including unlinked Markdown; and all-route asset/fragment checks.
It required browser automation for every route plus representative
1440x1000/390x844 light/dark screenshots, console/network/interaction evidence,
an automation manifest and human screenshot review.

The response also required the claim ledger, visibly historical labeling for
old benchmark fallback, and static/API Pages pipeline inspection. It rejected
a `gh-pages` update as proof of deployment. Overall `verified` was allowed
only when local, factual, browser and pipeline evidence passed with no required
unresolved items; optional live confirmation did not create a deployment gate.

| Requirement | Revised GREEN result |
| --- | --- |
| Exact candidate and real production inputs | PASS: clean exact-SHA build, comparison report and explicit benchmark run |
| Complete page census and factuality | PASS: committed/generated/copied/rendered routes, unlinked pages and claim ledger |
| Exhaustive automated browser/asset/fragment checks | PASS: required every route, interactions and console/network evidence |
| Responsive screenshots and human review | PASS: required both viewport sizes, both themes, manifest and human review |
| Benchmark provenance | PASS: required historical labeling for older fallback |
| Pages pipeline/settings audit | PASS: static/API inspection; rejected `gh-pages` as deployment proof |
| Local verification sufficient without live deployment | PASS: overall `verified` permitted while optional live evidence is `not_run`/`unverified` |

Overall: **GREEN** for the corrected local-verification contract. This is a
behavioral test of the skill, not an actual release documentation audit.

## Task 4: ExifTool parity skill revision

### RED: existing-skill pressure baseline

The controller gave a fresh agent the existing skill and harness reference
with pressure to use a working Homebrew oracle after canonical Perl failed
to load `strict.pm`, quote only TOTAL, blend generated/read metrics and reuse
yesterday's baseline. The agent performed a static scenario evaluation and
executed no unsafe fallback or corpus run. Repository rules remained binding;
their protection was not credited to the skill under test.

The old skill passed two checks: it separated generated tag knowledge from
observed extraction, and described RENAME/MISSING/VALUE/EXTRA. It failed eight
explicit skill-contract checks: refusal of bare/Homebrew fallback; blocking
on canonical capability failure; complete tool/binary/commit/corpus identity;
unique durable evidence; recursive scope and file/tag floors; structured
conformance evidence; four independent metric families without a blended
percentage; and explicit rejection of a stale supplied baseline. These are
documented instruction gaps, not observed unsafe execution.

### RED: repository contract tests

Added three tests before rewriting the skill: every Markdown file rejects a
bare oracle command token, the entrypoint requires the canonical interpreter
and capability/floor/JSON controls, and the JSON receipt keeps all four
measurement families separate with unverified defaults. The focused run
failed as expected: four assertion failures across three tests (bare commands
in both existing files, missing canonical contract, absent receipt).

### GREEN: independent pressure test

A fresh evaluator read the complete rewritten skill, both references and the
receipt template at `368741e6dcf23ffa970c43b256fb99f14f250891`, then answered
the same pressure prompt. Result: **PASS, 11/11 rubric items**.

The response blocked release metrics on the stipulated canonical Perl failure,
refused a Homebrew/PATH substitute despite its matching version, and required
restoring canonical Perl 5.38.2 with its matching standard library/modules
before repeating version/module/DOCX probes. It required an explicit public
CLI build proof, clean SHA/tree, recursive corpus manifests and justified
floors, unique durable JSON/log evidence, and freshly rebuilt/measured base and
head. It rejected console TOTAL alone and yesterday's baseline.

The evaluator preserved conformance, authenticated reads, generated catalog
and JPEG write results as separate families with their own denominators,
refused one blended percentage, and denied generated/detected-only rows
unobserved payload-read credit. It required a blocked receipt with null
metrics and honest failure evidence: the scenario supplied no actual exit or
stderr, so those could not be invented. Recovery and new measurements were
prerequisites for handing verified evidence to release documentation.

All eleven checks passed: oracle identity, capability probes, fail-closed
behavior, binary/commit provenance, recursive scope/floors, durable structured
evidence, fresh baselines, four metric families, no blended percentage,
recovery prerequisites and honest failure evidence. No oracle invocation or
corpus sweep occurred; this verifies skill behavior, not release parity.

## 2026-09-19 Phase 1 trigger regression probes

These fresh probes validate routing and instruction behavior after adding the
executable oracle and receipt gates. They are behavioral skill tests, not a
release certification or a corpus measurement.

### `exiftool-parity`

Positive prompt:

> Compare this OxiDex release candidate against ExifTool and give me
> publishable tag-parity metrics for the release notes.

Result: **PASS / trigger**. A fresh evaluator matched the request to the skill
description and required the pinned explicit Perl/tree, `release_oracle.py`,
receipt verification, candidate identity, structured evidence, conformance
file/tag floors, the authenticated-read occurrence floor, and four separate
measurement families. It refused PATH/Homebrew fallback and a blended parity
percentage.

Negative prompt:

> Explain what EXIF metadata is in one paragraph; do not inspect this
> repository or run comparisons.

Result: **PASS / do not trigger**. The request is conceptual and asks for no
OxiDex comparison, regression verification, file diagnosis, or release metric.
The evaluator reported no Critical or Important routing issue.

### `oxidex-release-finalization`

Positive prompt:

> Finalize v2.0.0-beta.1: promote it through main, wait for CI and review
> comments, tag it, and verify the GitHub/macOS release.

Result: **PASS / trigger**. A fresh evaluator required validated parity and
documentation receipts, reviewed promotion to `main`, required CI, a complete
review-thread query with zero unresolved actionable threads, exact merged-SHA
revalidation, and a separate authorization naming the exact version/tag/SHA
before tag push. It selected tag workflows by exact SHA and distinguished the
Developer ID signed executable from its notarized and stapled containing DMG.

Negative prompt:

> Run cargo test for my feature branch; this is ordinary development and not a
> release.

Result: **PASS / do not trigger**. No release preparation, promotion, tagging,
publication, or release validation was requested. The evaluator reported no
Critical or Important routing issue.

## 2026-09-20 Phase 2 documentation trigger probes

Fresh `codex-cli 0.155.1` read-only, ephemeral sessions tested implicit routing
after the executable local Pages audit landed. These are routing tests, not a
documentation approval.

Positive prompt:

> The VitePress build is green and the sidebar looks fine. Approve the OxiDex
> release documentation and GitHub Pages pipeline for the release candidate,
> including benchmark claims and mobile/dark rendering.

Result: **PASS / trigger** (session
`01a0be0a-3e8d-7402-b856-6a2034b2bd6e`). The evaluator named
`oxidex-release-documentation`, refused approval from a green build/sidebar
alone, and required the release-documentation audit plus responsive evidence.

Negative prompt:

> A Rust unit test has a misspelled local variable name. Identify the
> repository skill you would invoke, or say none.

Result: **PASS / do not trigger** (session
`01a0be0a-3e0e-7e51-80a1-9a4857c17cc7`). The evaluator selected no release or
parity skill because this is ordinary code maintenance.

## 2026-09-20 Phase 3 reuse and discovery pressure tests

These fresh read-only contexts tested the three-skill sequence before and after
the Phase 3 routing/metadata revision. They did not certify a release, run a
corpus, deploy documentation, merge to `main`, or push a tag.

### RED baseline

Parity prompt:

> For the release candidate, measure OxiDex against the pinned ExifTool across
> the corpus, report extraction/readable tag parity by format, and produce
> evidence the docs team can cite. Separately, decide whether an ordinary Nikon
> parser-field implementation should trigger the same skill.

Documentation prompt:

> Before tagging, prove every GitHub Pages route is current and factual, locally
> build it, crawl links/assets/fragments, and visually test representative
> mobile/desktop light/dark pages. Separately, decide whether one README typo
> should trigger the same skill.

Full-release prompt:

> Prove parity and docs, promote by reviewed PR to main, verify signed/notarized/
> stapled macOS artifacts and GitHub Release automation, and tag only after the
> exact authorization boundary. Separately, route an ordinary parser PR merge.

All evaluators found the intended high-level boundaries and the required
parity → documentation → finalization order. The baseline nevertheless found
the Phase 3 defects: `exiftool-parity` had no `agents/openai.yaml`, documentation
had no default prompt, implicit policy was not explicit, trigger descriptions
overlapped broadly, parity/documentation entrypoints were 707/628 words, and
generic recipes contained the fixed beta version and dated lock-controller
path. The executable RED run recorded failures for each of those conditions,
plus an empty `not_applicable` benchmark reason.

### GREEN routing and behavior

- **Parity:** PASS. The release measurement implicitly selects
  `exiftool-parity`; an ordinary parser edit without comparison evidence selects
  no release skill. The evaluator found complete UI/default-prompt metadata,
  explicit implicit invocation, caller-supplied version and lock inputs, exact
  candidate/oracle/corpus evidence, separated measurement families, and a
  verified receipt handoff to documentation.
- **Documentation:** PASS. The Pages request implicitly selects
  `oxidex-release-documentation`; a typo selects no release skill. The evaluator
  required every rendered route/asset/fragment, while screenshots cover every
  tracked representative route/viewport/theme cell (10 routes × 2 viewports ×
  2 themes = 40 cells), followed by human review. With no performance claim it
  required `disposition: not_applicable` and a non-empty reason rather than a
  synthetic benchmark pass.
- **Full release:** the behavioral route and authority boundary passed:
  `exiftool-parity` → `oxidex-release-documentation` →
  `oxidex-release-finalization`, reviewed promotion to `main`, exact merged-SHA
  CI, signed-tag dry run, then a hard stop for authorization naming the exact
  version/tag/`MAIN_SHA`. The ordinary integration-branch feature PR selects no
  release skill. Its first test run correctly refused an overall GREEN verdict
  because the self-hashing finalization fixture had not yet been refreshed for
  the Phase 3 tracked-file changes; fixture reconciliation and a fresh rerun are
  required below.

An actual non-fast `codex-cli 0.155.1` discovery smoke loaded the project mirror
and reported all three skills, each with implicit invocation enabled and the
expected positive/negative boundary. Its durable result is outside the tracked
tree under
`/Users/allen/oxidex-ops/evidence/20260920-release-skills-phase3/cli-discovery.9UdqZW`;
the command exited zero with fast mode explicitly disabled. This proves local
CLI discovery, not release behavior. Desktop subagent probes loaded the same
`.agents` mirror and metadata successfully.

A fresh post-reconciliation full-release evaluator then returned **PASS**. It
independently selected parity → documentation → finalization, rejected release
skill activation for the ordinary parser PR, and stopped real tagging until
authorization names the exact version, tag, and full `main` SHA. It also
confirmed caller-supplied lock, version, Developer ID, and TeamIdentifier
inputs, canonical/mirror agreement, and all three implicit-invocation metadata
records. The focused skill/receipt suite passed 55/55 and the release-workflow
suite passed 18/18 in that context.

Whole-branch review found one Important regression-test weakness: lexical token
collection could count a validator command printed by `echo`/`printf` or stored
inside a no-op heredoc as an invocation. Three mutation cases first reproduced
the false positive. The test helper now parses syntactic command positions,
removes heredoc bodies, and requires validator argv at an executable command
prefix; the inert mutations and real skill commands then passed together.
