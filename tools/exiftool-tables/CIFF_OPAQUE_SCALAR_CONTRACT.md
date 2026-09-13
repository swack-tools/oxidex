# CIFF opaque scalar contract

This note records a source-backed native probe for the four still-omitted
`CanonRaw::Main` scalar rows. It adds no generated schema, reader, caller, or
activation.

The probe resolves `CanonRaw::Main` and invokes that table's live
`PROCESS_PROC` CODE reference against constructed CIFF10 blocks under both byte
orders. It authenticates that selected processor and `ValidateImage`
source/body facts, exposes the selected native row facts, and records one
chronological trace of `FoundTag`, warnings, and `ImageDataHash` calls. It uses
`File::RandomAccess` over the same complete block passed to the native
processor, so an external pointer is an absolute offset from the block start.

The observed contract is deliberately split:

- `FreeBytes` (`0x4001`) carries literal `Format => 'undef'`. An inline value
  has eight directory bytes, but native `ReadValue` uses the normal inline
  count of one and reports one byte.
- Unformatted type-class `0x20` values, including inline `0x6005` and external
  `0x2005`, bypass `ReadValue` and retain the complete byte buffer. They cannot
  be represented by a zero count of `Fmt::Undef`.
- `JpgFromRaw` (`0x2007`) and `ThumbnailImage` (`0x2008`) retain the complete
  buffer, preserve native group 2 `Preview`, and use the authenticated
  `ValidateImage` helper. The probe captures the native repair of
  `? D8 FF DB` to `FF D8 FF DB`.
- `RawData` (`0x2005`) invokes `ImageDataHash($raf, size, 'raw')` after seeking
  the external payload pointer and before normal reporting. The chronological
  trace captures this absolute pointer, size, mode, then the raw report; it
  does not manufacture a digest.

A future source-derived schema needs two payload forms: `ExplicitUndef` for a
literal table `Format => 'undef'`, and `UnformattedBytes` for recognized
unformatted type classes. `RawData` also needs an external-span hash callback,
and the two Preview rows need a closed, fingerprinted value-local
`ValidateImage` conversion. The deferred external read policy, requested-tag
behavior, invalid-image warning policy, and actual hash implementation are not
modeled by this checkpoint. Therefore the four current omission counts remain
unchanged.
