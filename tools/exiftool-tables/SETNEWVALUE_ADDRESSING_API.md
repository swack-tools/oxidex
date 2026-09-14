# Source-selected static EXIF addressing

`setnewvalue_addressing.compile_addressing` derives address rows from
`convinv_rows.compile_rows`, then binds each row to its native
`module/table/full_name/raw_id/name` identity and the actual table `GROUPS`
values. It accepts only the ordinary `EXIF`/`IFD0` subset of SetNewValue's
group grammar. The complete captured SetNewValue and TagLookup::FindTagInfo
bodies must both match their closed source templates.

Run `setnewvalue_address_probe.pl EXIFTOOL_LIB rows.json` after row generation.
It records every native `FindTagInfo` candidate for each source row name. A
resolution is `resolved` only when the observed candidate set has exactly one
generated row after the optional `EXIF:` or `IFD0:` filter. This prevents an
unqualified partial row set from treating native `Software` as unique when
TagLookup has XMP, PNG, QuickTime, and other candidates.

`owned_unsupported` is terminal for a future public writer router. It covers
source-owned rows omitted from a final recipe, a missing lookup observation,
external candidates, ambiguity, unsupported spelling or qualifier, and
conflicting aliases to one physical identity. `outside_migrated_scope` is only
for names absent from the selected source rows. A missing final addressing
recipe must not fall back to the old writer.

The Rust generator emits static address rows, native lookup candidates, and
source-owned names. It remains inactive with
`inactive_source_addressing_no_public_setnewvalue_admission`.

This does not implement public SetNewValue. Unsupported portions include
wildcards, language suffixes, shortcuts, multiple/numbered/ID qualifiers,
ExifIFD and other group forms, priority/preferred/avoid handling, protected
tags, list recursion, deletion, NEW_VALUE construction, conversion, CHECK_PROC
execution, serialization, and file writes.
