# Preserve the native writer's input pipeline

The generated writer must follow the selected ExifTool's complete call order.
Correct individual helpers can still produce the wrong result when composed
in the wrong order. This is part of the [read/write version plan](read-write-version-plan.md),
not a separate or narrower completion target.

## What native source currently does

The inspected source is ExifTool 13.59, executed with Perl 5.38.2 and ambient
ExifTool configuration disabled. These steps describe that release. Source
changes must change the generated pipeline or produce an explicit unsupported
gap; this description must not become a permanent hand-written tag rule.

1. `Writer.pl::SetNewValue` normalizes a defined scalar input through `Sanitize`
   before applying inverse conversions. For the default options, a Perl
   UTF8-flagged string becomes UTF-8 bytes. Deletion remains undefined.
2. `ConvInv` applies the selected inverse conversions and calls the effective
   table checker, subject to source controls such as WriteCheck and RawConvInv.
   Those controls need their own admission; a checker recipe is not permission
   to ignore them.
3. `WriteExif.pl::CheckExif` selects the first true Format, Writable or table
   WRITABLE property in source order. It handles the missing-format branch or
   delegates to the captured CheckValue function with the selected count.
4. The file writer handles deletion separately. For a defined value it calls
   WriteValue, then applies the source's encoding rules, including CharsetEXIF
   for applicable string fields. Final stored byte length determines the IFD
   count. Physical placement and preservation of unrelated data are separate
   parts of the complete operation.

## Measured input-normalization effect

Instrument: actual native SetNewValue with trace wrappers around the original
Sanitize and CheckValue functions. The fixture uses native HostComputer tag
information with an explicitly injected Count; it does not change production
tag definitions. Each result records input, the states before and after both
helpers, validation error and staged value. Callable source/body hashes are
recorded before installing the wrappers.

All 24 cases ran against the canonical source and a copied Writer.pl whose
Sanitize version guard alone changed from `>= 5.006` to `>= 10.006`. Nine trace
records changed. With the fixture Count set to 3, the UTF8-flagged input `éé`
becomes four bytes before validation in the canonical run and is rejected.
The altered source retains two characters and accepts/pads the value. Thus a
direct CheckValue Unicode test cannot certify public API input behavior.

This is native mutation evidence, not generated execution or a release-upgrade
pass. No file was written by this probe. An initial probe failed because it
loaded WriteExif.pl before its Exif.pm dependency; that failed instrument log
is retained separately from the corrected results.

Evidence relative to the continuation evidence root:
`shared-pilot/write-upgrade-integration-20260913/input-normalization-20260914/`.
The cases, trace program, canonical and changed JSON, source copy, state record
and failed-loader-order log are preserved there.

## Validation composition is merged

PR #764 merged as `8988302c` after all five required hosted checks passed at
`56fba56e`. Its source-derived CheckExif recipe composes the actual generated
CheckValue recipe. The direct native/generated instrument covers 19 cases
across canonical source and three source mutations, for 76 matching outcomes.
The same helper comparison separately passed for selected releases 11.78 and
12.64. This is helper proof; neither full version conformance nor public file
writing is established by it.

## Next acceptance requirements

- Translate input normalization and prove its relationship to inverse
  conversions and validation. Preserve default UTF-8, embedded NUL, defined
  empty values and deletion. Unsupported option/dependency behavior remains
  visible; an ASCII-only route does not satisfy this requirement.
- Compile the writer's encoding and count rules, then compare actual generated
  JPEG/TIFF insert, update, growth, shrinkage and deletion with the native
  release in both byte orders. Preserve unrelated metadata and payload.
- Change supported native name, type, placement and processing behavior in a
  copied source; regeneration must change actual output without per-tag code
  edits. Retire the replaced manual tag lookup only after this proof.

Input normalization needs both direct dependencies and callback references.
Sanitize assigns SetWarning by CODE reference; searching only for calls misses
that binding. Its Encode functions also belong to the interpreter library,
outside the selected ExifTool library. Preserve those observations explicitly.
Capturing the source body or parsing its guards is not executable admission.

The shared Rust UTF-8 primitive may implement standard encoding, just as shared
operations implement pack/unpack. The ExifTool source must still choose the
encoding literal, input flags, guards, options and call order. Before admitting
that path, compare controlled pristine and final-loaded native bindings,
encoding registry and resolved method, and check typed Unicode/NUL/byte
semantics. Replaced bindings or registry entries must refuse. Raw interpreter
and library hashes remain evidence; source-to-XS binary linkage is not claimed.
Unsupported manual packing and XML/HTML paths remain work to complete.

## Generated sanitization execution checkpoint

The next candidate captures a pristine interpreter and the actual final table
producer's encoding state. It validates exact typed vectors (including undef),
the requested and actual functions, and the registry's resolved method. It
joins those callable fingerprints to Sanitize's captured dependencies before
emitting Rust operands. Portable artifacts exclude machine paths and binary
hashes; optional raw diagnostics retain them outside the source tree.

`test_sanitize_rust.py` captures actual native source, generates its operands
and compiles the real `generated_sanitize.rs` executor with standalone rustc.
Base cases cover Unicode, NUL, bytes, undef, scalar references and inactive escape
options. It tests a copied version-guard mutation and, when present in source,
removes both EncodeHangs guards in another copy. Source/body identities are
checked before comparison. Both option values are compared when the selected
source ignores EncodeHangs or cannot reach manual packing.

The current-pin permanent regression passed 260 native/Rust comparisons.
Separate actual 11.78 and 12.64 native sources passed 208 comparisons each;
both lack the option guards and the generated runtime follows their bodies.
The compiler compares complete source shapes and refuses mixed guards or
unmodeled statements, instead of keeping the previous release's behavior.

Normal regeneration owns the sanitization rules and ledger (38 artifacts).
Official full regeneration and the pre-repair full gate passed; targeted
three-release proofs, source/codegen tests and Clippy passed after repairing
the historical grammar. See the [progress record](../AUTOGENERATION-PROGRESS.md)
for precise counts and evidence boundaries. Exact-head publication gates remain
pending. No public writer is activated by direct-helper proof; inverse
conversions, charset/count, physical writing and remaining sanitizer branches
still need their own proof.

There is no new production writer or manual-rule retirement to count at this
checkpoint. Helper proof, complete file behavior and release conformance remain
separate measurements.

## Remaining final file stage

The [native final-stage audit](https://github.com/swack-tools/oxidex/blob/045b739ddab2eda260b306aa67e948fbaeeb59cf/tools/exiftool-tables/NATIVE_SCALAR_WRITE_FINAL_STAGE_AUDIT.md)
records the remaining source controls between the scalar helper and the actual
IFD edit. Resolve conversion format and on-wire type before WriteValue, then
apply any source-selected charset recoding and calculate the final count.
New and existing entries use different format selection rules. Preserve native
NoOverwrite/warning outcomes rather than substituting a fatal error and calling
it parity. Unicode text and embedded NUL remain part of the default scalar path;
explicit CharsetEXIF recoding is additional behavior to implement.

The source's TIFF format name/number/size registries are not currently in the
write sidecar. Capturing those actual final-loaded registries is the next fact
extraction task; hardcoded TIFF type choices cannot stand in for upstream type
changes. The audit proposes a contract and requirements, not completed runtime.
