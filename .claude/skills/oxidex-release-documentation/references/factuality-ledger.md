# Factuality ledger

Use this before editing release prose. Freeze the candidate's full commit and
record version, oracle pin, parity receipt path/hash and the source/workflow
revisions consulted. A prior release's receipt cannot certify this candidate.
Never turn generated tag declarations, detected format identities, or a
conformance ceiling into observed extraction coverage.

For every material claim, fill these required fields in `claims`:

| Field | Meaning |
| --- | --- |
| `statement` | Exact user-facing assertion, including its number/unit/scope |
| `route` | Rendered URL path and heading; use an explicit source-only path if excluded |
| `classification` | `current`, `historical`, or `excluded` |
| `source` | Candidate file/line, receipt, workflow run or artifact backing the claim |
| `instrument` | Named measuring command and options; identify source inspection for non-measured facts |
| `measured_sha` | Full commit actually measured, never the current SHA by assumption |
| `freshness_rule` | Exact-candidate requirement or explicit historical date/commit boundary |
| `status` | `verified`, `blocked`, or `unverified` |
| `evidence_path` | Durable raw evidence and hash manifest location |

Populate `pages` with one row per source/route relationship. Each row records
`source`, `source_kind` (`committed`, `generated`, `copied`, or `framework`),
`route`, `classification`, `reason`, `claim_indexes`, `crawl_status`, `status`,
and `evidence_path`. Include unlinked pages: navigation visibility does not
control VitePress publication. Mixed current/historical pages are `current`
with individual historical claims marked explicitly.

- **Current:** each present-tense statement must match candidate source and
  measurements. Correct or remove unsupported claims; keep the receipt blocked
  until the replacement build is audited.
- **Historical:** visibly label the original version/date/commit context
  on the rendered page. Old benchmark multipliers in a changelog remain history;
  copying them into the home page or release summary makes a new current claim.
- **Excluded:** record the exact `srcExclude`/build rule or source-only purpose
  and confirm no rendered route exists. An unlinked but rendered Markdown page
  is not excluded. Copied Criterion HTML is included even though it has no
  Markdown source. Explain framework pages such as `404.html` in the census.

Validate at least these classes against candidate files, not remembered state:

| Claim class | Evidence to reconcile |
| --- | --- |
| Version/changelog | Cargo workspace/package versions, lockfile, docs config, headings and changelog release/date; distinguish prerelease from stable |
| Installation | Actual CLI/package names, available release assets, package registry channels, command output, architecture and OS constraints |
| Migration | Before/after commands and behavior, removed/changed options and compatibility caveats |
| Platforms | Release workflow target matrix plus successful artifact evidence; cross-compilation alone does not prove execution on that OS |
| Parity/coverage | Exact-SHA parity receipt, pinned capability-checked oracle, corpus identity and named instrument; preserve missing/wrong/extra limitations |
| Status/roadmap | Implemented and measured behavior versus planned work; archived plans remain visibly historical |
| Performance | Each measurement's provenance and disposition under the benchmark policy |

Locate pages through the complete census, not a fixed filename list. Resolve
contradictions across home, guides, reference, status and release notes. A
source correction invalidates the build and screenshots for that content;
record the new commit and repeat affected checks plus the whole-site crawl.

Receipt states: `unverified` means required evidence has not been collected;
`blocked` means a known unmet requirement. `ready_for_promotion` means the
candidate-bound local audit and pipeline checks all pass, with live deployment
explicitly `pending`; it permits the documentation gate for a PR to `main`,
not tagging or release approval. `verified` means both phases, including
exact-main live deployment, pass. Preserve `promotion_readiness.candidate_sha`
and its evidence as the receipt advances from `candidate_local` to `main_live`;
record the merged SHA separately in `live_deployment.main_sha`. Empty arrays,
null identities and unreviewed exclusions
cannot establish completeness. The receipt starts empty intentionally: expand
it with all observed rows and evidence, never count example rows as coverage.
