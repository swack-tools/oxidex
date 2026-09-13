# Sony plain-table producer recovery

Started 2026-09-12 from freshly fetched `e664e063` on
`codex/sony-plain-recovery-20260912`. ExifTool remains pinned to **13.59**.
Local producer, pipeline, tooling and review acceptance are complete. Hosted
checks and landing are separate; the runtime repair remains a follow-up.

## Scope and outcome

`src/parsers/tiff/makernotes/sony/plain_tables.rs` names native Perl tables in
its generated header, but had no committed producer. The recovery must
reproduce the complete existing file: **six tables, 193 rows, 72 enum maps and
one bitmap**. The tables are CameraSettings, CameraSettings2, CameraSettings3,
FaceInfo1, FaceInfo2 and ShotInfo. Supported native expressions are translated
through a finite audited registry into the existing Rust DSL. Unsupported
executable metadata must refuse before output is written.

The first milestone recovers generation and independent declaration checks
without changing generated Rust or extraction behavior. The shared artifact
manifest gains Sony plain as its thirtieth output: 8 tier 1 and 22 tier 2.
The runner must generate it from the same fresh selected dump and verify the
formatted result against independently loaded Perl. Sony enciphered and Nikon
encrypted remain the two unrecovered original producers once this lands.

This is a reduction in manual upgrade maintenance. It earns no automatic
increase in generated-route attribution, and no whole-parser parity claim.

## Oracle and completed runtime characterization

The fresh local dump uses explicit system Perl **5.34.1**, pinned ExifTool
**13.59**, and a successful DOCX capability probe. Dump SHA-256:
`20b229b7eb02626c22a79f7acda7dd856c8ce2d573d827c4bfb62b9368579bc8`.
Loaded `Sony.pm` SHA-256:
`1db1f1905ec2b6abf4bbd90fa6d8bfa92d84c5a2399cc333c2b392f27d8d330f`.
This local producer input does not replace the expression ledger generated
with a different interpreter on the i7.

An external harness invokes the real `Sony::CameraSettings3` native
`ProcessDirectory` and the current production Rust `binary_data::process`
over the same 1,536-byte blocks, little endian, with model DSLR-A580 and
LensMount 0. Each packed word is at byte offset 276. The native keys are
`276` for FolderNumber and `276.1` for ImageNumber.

| Packed folder / image | Native FolderNumber | Rust FolderNumber | Native ImageNumber | Rust ImageNumber |
| --- | --- | --- | --- | --- |
| 0 / 0 | `000` | `000` | `0000` | omitted |
| 123 / 4567 | `123` | `123` | `4567` | omitted |
| 999 / 9999 | `999` | `999` | `9999` | omitted |

The runtime currently groups adjacent declarations by integer byte index and
selects one matching alternative. That conflates two distinct native raw keys.
The mask-shift concern was disproved: the current interpreter already shifts
masked values, and FolderNumber is correct. These observations come from a
direct production-module probe, not a CLI fixture or a corpus census.

Native `CameraSettings3[1015] LensType2` also carries scalar `PrintInt = 1`.
ExifTool documents this as HTML tag-ID formatting metadata. It is not a
missing extraction operation. The dump preserves its presence but not its
value, so the independent native verifier must guard the exact declaration.

## Producer and pipeline acceptance

Two root-run generations from the fresh dump match the complete committed
file, SHA-256
`e7e261a4b25aa34ecd0dfdc16ab89213af90c8ac39e6a6271a587e249aa13de8`.
Replaying the saved genuine Perl 5.38.2 dump
`193cf4e91326f53c7bdfd674cb10da0937c8bcdb4c0285ad4f507fd9f96a8fb8`
produces the same file. All three native OTHER closures have identical bodies
between those dumps; no speculative deparser aliases were added.

The production `regen-all.sh --tier2-only` pipeline completed in **30.783
seconds** with explicit Perl 5.34.1 and the pinned library. Every native
verifier passed, the write guard reported **zero net changes**, and
`artifacts.py diff --tier 2` found all **22 outputs** identical to HEAD.
The focused producer/manifest/runner/classifier suite passed **58 methods**.
The verifier's **13 full-CLI controls** also pass, including changed source
facts, source substitution, missing declarations and malformed Rust.

Review exposed a numeric-literal defect in the proposed verifier: Perl `010`
is octal eight, so treating it as Decimal ten was unsound. Leading-zero
numeric tokens now refuse before normalization; controls cover valid octal,
invalid octal, changed operators and quoted numeric strings. Exact CI Clippy
(`cargo clippy --all-features -- -D warnings`) passes. This evidence certifies
the finite declaration projection, not the handwritten runtime.

The full tooling suite ran **502 methods**, `OK (skipped=1)`, against the
fresh Perl 5.34 dump. The sole skip was the existing committed ledger's
Perl 5.38 dump identity requirement. Re-running the six `WholeDump` methods
against the saved genuine dump whose SHA matches that ledger passed all six,
including the provenance check. No ledger was regenerated or bypassed.
Workspace formatting, shell syntax and whitespace checks pass; independent
producer, verifier and integration reviews found no remaining blocker.

The acceptance contract for future changes remains:

1. Generate twice from a fresh pinned dump and compare the whole output with
   the committed file. Preserve exact expression bodies and raw numeric keys.
2. Run the independent verifier against live Perl, with complete Rust DSL
   parsing and rejection of missing, extra or changed declarations/maps.
   The verifier must not import the producer's translation dictionary.
3. Run focused negative controls, including altered conditions/conversions,
   unknown executable metadata, map changes and unsupported native PrintInt.
4. Run the manifest, real-shell and bump-classifier contracts, including
   failure injection at each new command. The shared undeclared-write guard
   is tested separately by injecting a write at the DICOM producer.
5. Run the entire tier-2 regeneration and its native verifiers with no generated
   output drift. Record formatting, lint, review and landing evidence.

## Next runtime repair

The separate [raw-ID repair](./sony-raw-id-runtime.md) now implements this
contract and records its bounded validation. The baseline characterization
above remains the evidence for the original omission.

Keep byte offsets separate from raw declaration identity. A generated sidecar
can carry exact native raw IDs aligned with the existing table rows, while an
opt-in reader groups true variants by those IDs. Check all alignment before
changing output or context, and preserve IDs through nested directories.
Sony enciphered and Minolta A100 still use this interpreter and must retain
their existing behavior until separately migrated.

Acceptance requires both packed fields above, true conditional alternatives,
nested distinct IDs at one offset, and malformed metadata leaving output and
context untouched. Independent verification must check exact table order,
row order and raw IDs. Only then is the missing-field repair ready for a
pinned real-file comparison and the ordinary runtime gate.
