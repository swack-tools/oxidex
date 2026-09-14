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

## Next acceptance requirements

- Compile and execute CheckExif with the exact generated CheckValue rule,
  including source-order changes, falsey properties, numeric/byte format
  coercion and missing-format behavior. A local-variable alias that changes
  native control flow must refuse admission.
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

There is no new production writer or manual-rule retirement to count at this
checkpoint. Helper proof, complete file behavior and release conformance remain
separate measurements.
