# Nikon settings generator recovery

Started 2026-09-11 from freshly fetched integration `79101d7d`, after #743.
ExifTool remains pinned to 13.59. This is a producer recovery for existing output,
not an extraction improvement, parser retirement or new attribution percentage.

## What changed

`tools/exiftool-tables/gen_nikon_settings_tables.py` reads the loaded
`NikonSettings::Main` dump and emits the entire existing `settings_tables.rs`.
It preserves lexical decimal-ID order, same-ID variant order, names, enum maps,
masks and the existing finite Condition/RawConv/ValueConv/PrintConv vocabulary.
New expressions and executable fields refuse before the output is opened.
Unknown tags are omitted and counted; mixed known/Unknown alternatives refuse
because deleting an Unknown veto could expose a later fallback.

The shared inventory and tier-2 regeneration now include Nikon settings:
29 managed outputs, comprising 8 tier 1 and 21 tier 2. The classifier derives
three remaining missing producers: Sony plain, Sony enciphered and Nikon encrypted.
The separate coarse enum-fingerprint check still includes all historical targets.

## Evidence and limits

A fresh `dump_tables.pl` invocation with explicit system Perl and selected 13.59
source found 203 IDs / 234 variants. The generator emits 197 rows and 131 maps;
37 Unknown rows are omitted. The complete generated file is byte-identical to
its committed counterpart, SHA-256
`470f1687d1eebd5e3052932a1ce3d02994cdb129cbcde66edc263a40374d5a9f`.
The source's DOCX capability probe also passed. All execution records are kept
in the owned recovery evidence directory referenced by the local handoff.

`verify_nikon_settings.py` independently reads live Perl facts and parses the
emitted Rust; it does not import the producer or grade against its JSON dump.
Its contract is declaration correspondence, including ordered variants and
explicit projections. It does not prove every custom-parser behavior. The
handwritten `ProcessNikonSettings` adaptation remains upgrade review work even
when its upstream function keeps the same name.

Two existing runtime limitations remain explicit:

- `Main[366] AFAreaMode` has a native RawConv state store; Rust emits the value
  but does not propagate this store to other directories. The generator accepts
  only that exact key/name/expression projection and reports it.
- `Main[266] BracketProgram`, under `BracketSet == 5`, declares Mask 15.
  The Rust parser applies it; the native custom processing path does not.
  The generator preserves and reports that exact standing declaration, and
  refuses new nonzero masks pending runtime review. A native 13.59
  `ProcessNikonSettings` byte fixture with BracketSet 5 and BracketProgram 27
  returns `Unknown (27)` and stores 27; the current Rust path masks it to 11
  before conversion. This existing discrepancy is not hidden by a PASS for
  declaration correspondence.

No generated Rust or handwritten runtime changes are needed for this recovery.
The historical 28-output upgrade rehearsal remains historical evidence; it is
not relabeled as a 29-output acceptance run.

## Acceptance

Ten producer refusal/output-preservation controls and the four real-shell
orchestration controls pass. `cargo fmt --all -- --check`, shell syntax and
`cargo clippy --all-features -- -D warnings` (the actual CI lint command) pass.
The broader `cargo clippy --workspace --all-targets -- -D warnings` check fails
on the pre-existing `clippy::duplicate_mod` in `tests/unit/audio/../../common/mod.rs`;
this recovery changes no Rust source or Cargo configuration.

The independent verifier passes all 197 rows / 131 maps / 37 Unknown omissions;
its 15 CLI controls pass. Two producer runs reproduce the same complete-file
SHA above. The complete `regen-all.sh --tier2-only` pipeline passes in 32.925
seconds with zero declared net changes; all 21 tier-2 outputs match HEAD under
`artifacts.py diff --tier 2`. The full 29-output inventory is not a claim that
tier 1 was regenerated during this bounded check.

The final combined tooling suite reports **445 methods run in 198.830 seconds,
OK (skipped=1)**. Four structural WholeDump methods run against the fresh native
13.59 dump; only its committed-ledger correspondence check skips because that
ledger records the other Perl/dump provenance. No ledger is weakened or rewritten.
The earlier 425-method run lacked the full dump and the 15 verifier controls;
its class-level skip left all five WholeDump methods unavailable.

## Useful next work

1. Give each Nikon runtime limitation a native carrier regression and measured
   correction, keeping declaration recovery separate from behavioral claims.
2. Recover Sony plain tables after proving their upstream keys `276` and
   `276.1` remain separate fields at one byte offset. Current integer-index
   grouping treats FolderNumber/ImageNumber as alternatives; a packed-word
   fixture must prove both before changing that interpreter.
3. Recover Sony enciphered and Nikon encrypted producers with explicit
   unsupported callbacks, nested routing and state contracts.
4. Join generated declarations, current producers and observed attribution in
   the ledger. A producer's recovery changes maintenance ownership; the reported
   5.37% hand-table-walker dependency is neither the gain from this one file nor
   proof that all of that class is cheap to migrate.
