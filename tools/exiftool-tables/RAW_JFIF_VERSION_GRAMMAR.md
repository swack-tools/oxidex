# Raw JFIF historical source profiles

The raw-property compiler recognizes complete source profiles for the preserved
ExifTool 11.78/12.64 pair and the existing 13.59 source. Selection compares all
17 captured executable function bodies. It does not select behavior from a
version string or source hash, and it does not independently mix functions from
different profiles. Inserted executable statements refuse compilation.

The original 13.59 grammar file remains unchanged. Its emitted Rust and report
have independent byte-identity checks against the output recorded before adding
historical profiles. Historical reports identify their own grammar digest.

The admitted path is unedited, unsigned JFIF DATAMEMBER extraction with default
options. The source table still determines property names, field order, offsets
and widths. The caller still determines signature, payload start, byte order,
creation skip markers and pending directories. Full source matching does not
claim to translate arbitrary Perl or support every branch of WriteJPEG.

## Reviewed differences

- Historical unsigned ReadValue/Get8u/Get16u/DoUnpackStd and SetByteOrder
  behavior, and JPEG marker-name decoding, are unchanged.
- 11.78 uses a truth-value fallback for directory length where newer source
  uses a defined/clamped length. The admitted JFIF caller provides the exact
  remaining payload length. Empty/truncated raw fields retain absence.
- 11.78 interpolates the package EXIF header in its existing-directory
  prescan. Historical profiles authenticate the captured header scalar and
  refuse any other header; a source hash alone does not establish its value.
- Historical callers lack newer hidden-data/trailer scanning and several
  non-JFIF APP segment handlers. Those changes do not alter the admitted JFIF
  scalar read or the creation point. This component does not serialize those
  other segments or translate their source behavior.
- Binary writing, unknown-tag and nested-directory handling also changed.
  The admitted fields have no conditions, hooks, masks or subdirectories and
  this specialization writes no JFIF field values.

## Portable evidence and deferred native proof

`testdata/raw_jfif_*_fact.json` contains selected-native captures, including
complete bodies, lexical format/endian maps, table rows and source provenance.
They are executable-grammar inputs rather than expected metadata values.

Run portable checks without Perl or Cargo:

```sh
cd tools/exiftool-tables
python3 -m unittest -v test_raw_jfif_versions.HistoricalRawJfifPortableTests
```

The separate native tests require `EXIFTOOL_PERL` and
`OXIDEX_RAW_JFIF_VERSION_LIBS`, a JSON map from `11.78` and `12.64` to their
already materialized `lib` directories. Run them only while holding the shared
validation lock:

```sh
python3 -m unittest -v test_raw_jfif_versions.HistoricalRawJfifNativeTests
```

Those tests recapture each actual historical library, compare actual WriteInfo
raw properties at every truncated-field boundary plus zero/unit cases, then
copy each native library, propagate an allowed byte-order operand, and insert
a dispatcher statement that must be refused.
They build no Rust binary. Public consumer validation additionally needs the
per-version generated binary and a version-specific acceptance adapter; the
ordinary 13.59 native-write baseline must not be bypassed or relabeled.

This profile work alone does not establish that full regeneration of all
artifacts succeeds for either historical release. Other source compilers and
the version-rehearsal write adapter retain independent acceptance requirements.
