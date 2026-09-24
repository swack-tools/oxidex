# TODO

Deferred work that a release knowingly left out, with the reason and the
evidence. The beta release ledger is `TODO_RELEASE_BETA.md`; this file does not
replace it.

## Task 18 — deferred deletion-ledger infrastructure

### What the spec required

The Task 18 PRD (`controller/prds/18-proven-replaced-deletion.md` in the ops
evidence tree) put every deletion behind a proof record:

- `docs/reference/generated-runtime-deletion-ledger.json`, one row per deleted
  literal `path::symbol`, with its source fields, replacement owner, candidate
  source and binary hashes, and bound receipts (oracle, attribution,
  zero-reachability, oracle capability);
- a verifier (`tools/exiftool-tables/runtime_deletion_ledger.py` plus its
  tests), a `tests/generated_runtime_deletion_controls.rs` control, and a
  `just verify-runtime-deletions` recipe that would also reject new manual
  ownership of a generator-supported field;
- controller attestations: an authenticated, controller-approved finite
  candidate appendix, an attestation identity on every receipt, and a
  controller reconciliation-manifest digest.

### What beta.1 did instead

By maintainer decision on 2026-09-23, beta.1 skipped the ledger, the verifier,
the deletion-controls test, the recipe and every controller-attestation field.
The controller that would have attested them was retired the same day.

Each change landed as a small commit and was proven to the Task 17 standard
against a baseline binary rebuilt from `2791053b`, whose tree equals the #937
merge `1c92e42d`. The standard:

- conformance deltas of zero in every category and file;
- an identical read gate (lost 0, the same newly-credited list);
- byte-identical `oxidex -j` and `oxidex -j --no-print-conv` over both corpora
  (combined-samples, 4,249 files; `t/images`, 194 files);
- an unchanged genshare census;
- `runtime_ownership.py verify` passing.

Changes landed:

- deleted `DirEngineRows::drain` (a test-only alias of `finish`);
- inlined `EXIF_IFD_SILENCE_EDGES`, which was always true;
- merged `parse_ifd1_with_session`'s duplicate hand fallback into its twin;
- deleted `DirEngineRows::drain_ifd0` (an alias of `finish_ifd0`);
- moved the XP strings 0x9c9b-0x9c9f to their generated arms, after a
  byte-identical knockout trial.

The generated XP arm declines when the UCS-2 holds a surrogate code unit,
such as an emoji's pair or a lone surrogate. ExifTool's UCS2 `Decode` turns
it into bytes that are not valid UTF-8. The static fallback behind the arm,
`exprs::decode_ucs2`, then kept the NUL terminator, a trailing tail and a
byte-order mark that the hand decoder drops. A review of #940 found this. On
the IFD0 walks, an XP id whose arm declines an entry now goes back to the hand
decoder for that directory (`IFD0_HAND_ON_DECLINE`,
`DirEngineRows::keep_hand_on_decline`), which restores the pre-move output.
The ownership inventory records that fallback as five `IFD0/0x9c9b`-`0x9c9f`
residual rows with `residual_disposition: fallback-on-decline`, beside their
generated rows.

Still open: the same decline reaches the static fallback for XP tags in
ExifIFD. This predates Task 18, because those ids were never hand-kept there.
Fixing `exprs::decode_ucs2` itself would also change `XP_DIP_XML`.

The 0x9400 AmbientTemperature move was tried and abandoned. The generated arm
prints `0 C` where ExifTool 13.59 prints `-0 C`, on 5 Olympus files.

Evidence, including per-trial diffs, is under
`$OXIDEX_OPS_DIR/evidence/20260919-beta1-functional/task18-proven-deletion/`.
`OXIDEX_OPS_DIR` defaults to `~/oxidex-ops` (`scripts/ops_paths.py`).

### What remains (49 KEEP)

The candidate inventory is
`$OXIDEX_OPS_DIR/evidence/20260919-beta1-functional/task18-proven-deletion/materialization.md`
§2, which has each candidate's call sites and reasons. It is counted at
symbol, branch or id-group granularity. Of its 49 KEEP, D5 was deleted here.
B1 joined KEEP when its knockout failed, so 49 remain:

| Group | Count | Why it stays |
|---|---|---|
| A1-A4: Exif::Main hand arms (ExifIFD, IFD0, TIFF IFD0/1/2+, refused special arms) | 4 | They are the only producer of residual, refused and Unknown ids. They are also the live fallback when the engine is absent at runtime or leaves an entry `Unread`. |
| B1: `EXIF_IFD_HAND_KEPT` 0x9400 AmbientTemperature | 1 | The knockout failed. The generated arm drops the sign of `-0 C` (see above). |
| B3: `IFD0_HAND_KEPT` structural ids (IPTC, GeoTIFF, ModelTransform, PrintIM) | 1 | Structural traversal, which the PRD makes non-waivable. |
| B4: `IFD1_RESIDUAL_IDS` | 1 | All 7 ids are refused by the generator, so the hand arm is their only producer. |
| B5: the Copyright transition | 1 | Blocked on a generator and ledger change outside the lease. It must move atomically with B4, or the tag is inserted twice. |
| C1: `format_for_exiftool` / `format_tag_value*` | 1 | Live CLI entry points and public API. |
| C2-C10: `exiftool_compat` rule clauses | 27 | Hand-parsed producers still reach them: GPS, XMP, MakerNotes, ICC, APP14, the live Exif fallbacks and writers. Static reachability rules out deletion. |
| C11-C12: APEX helpers, public `is_*` predicates | 2 | They have live callers, and narrowing the public API is not a deletion. |
| D4, D6-D10: replay/`take_ifd0`/`owner`, the yields, `with_ifd1_forms`, priority shims, engine-off fallbacks, the Interop DCF arm | 6 | They carry output or are reachable at runtime. Retiring the yield or the fallback would change behaviour, which is not a zero-delta deletion. |
| E1-E5: engine-stage adapters, `perl_length`, `member_value`, keyed/serial directory walkers, attribution guard sites | 5 | Nothing is duplicated after Task 17. Some have different semantics, some are non-waivable public API, and the attribution instrument needs its guard sites. |

If the deletion ledger is built later, it should re-home the PRD's controller
attestations, for example to maintainer approval recorded on the PR plus an
evidence-manifest digest. Without that change, the verifier cannot pass
(materialization.md §5 A1).
