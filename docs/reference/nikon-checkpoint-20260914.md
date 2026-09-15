> Historical Nikon generator checkpoint preserved in PR #779.
> Integration and unresolved review items still require validation.

# Migration goal checkpoint — 2026-09-14

The maintainer stopped this goal and requested PRs preserving the current work.
Do not resume autonomous migration or merge unfinished checkpoints until a new
request establishes the next goal. These branches overlap; they are not four
independent changes that can be merged in any order.

## Measured state

- Reading: conformance.py with pinned ExifTool 13.59, 4,238 files,
  468,087 / 480,769 expected occurrences = 97.3621427338%. This measurement
  belongs to source b0cc3c62, merged as 3cf7c522; it is not a fresh measurement
  of this checkpoint and includes generated and handwritten readers.
- Writing: overall coverage is unknown. The full archived source inventory
  contains 1,512 tables, 33,073 rows and 34,260 variants; only 2,699 explicitly
  declare Writable true. Missing effective values do not mean unsupported.
  No inventory count is an overall public-write coverage denominator.
- Inventory tool PR #770 merged as 304d6339 with all five enabled CI checks
  passed, no unresolved review threads; Benchmarks was skipped.

## Preserved work

Recovered Nikon encrypted-table generator and tier-2 wiring; captured the
stored ProcessNikonEncrypted callback, direct helper provenance and live
Decrypt lexical arrays. The runtime imports generated XLAT0/XLAT1 instead
of handwritten arrays. Captured 512-byte lookup SHA-256:
79f0e716581a40357e6e6872aa64459f91c5cb572b14576a74773fb74bdfd3c4.

The source-hash check now joins Nikon helper provenance to the callback rather
than pinning Nikon.pm to one release's whole-file hash. Algorithm body hashes
remain recognized contracts. Focused Python checks passed before closeout;
no full reader parity claim has been established for this checkpoint.

## Outstanding

- Complete the copied-source native mutation test: change a lexical lookup
  initializer, capture again, generate the changed byte and compare native and
  Rust decryption. A changed JSON fixture alone is insufficient.
- Supply semantic evidence for the newly admitted ProcessBinaryData deparse
  body 283954c79e2a9893469d57fd476091c34b57589f8c8f2e8c44cd62c067738fbf.
- Review full regeneration differences. A proposed map-renumbering rewrite was
  defective and removed during closeout; the complete output of the original
  generator traversal is preserved instead. After resolving map contents and
  subtable names, root comparison still found six differing tables. Do not
  assume all remaining changes are harmless reordering.
- Run official regen-all.sh --tier2-only, artifact agreement, meaningful native
  Nikon cases and the full reader parity gate before merging.

This checkpoint is preservation of unfinished work, not a claim that future
ExifTool parsing algorithm changes can already be translated automatically.

## Closeout checks

Final cargo fmt check and workspace all-features Clippy (-D warnings) passed.
These are formatting/compilation/lint results; the outstanding parity and
regeneration gates above are still required.
