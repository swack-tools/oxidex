# Source-selected static EXIF addressing

`setnewvalue_addressing.compile_addressing` derives address rows from
`convinv_rows.compile_rows`, then binds each row to its native
`module/table/full_name/raw_id/name` identity and the actual table `GROUPS`
values. It accepts only the ordinary `EXIF`/`IFD0` subset of SetNewValue's
group grammar. The complete captured SetNewValue and TagLookup::FindTagInfo
bodies must both match their closed source templates.

Run `setnewvalue_address_probe.pl EXIFTOOL_LIB probe-input.json` after row
generation. The input seals the generated-row digest, query-name digest, dump
capture context, and exact SetNewValue/FindTagInfo source and deparse
identities. The probe rejects any preloaded `Image::ExifTool` package and every
loaded Image::ExifTool file outside the selected library. Its observation also
records the canonical Perl path/release plus the whole loaded ExifTool closure
manifest and digest. The dump and probe both verify their manifest digest, and
the probe requires every module it loaded to match the dump's `inc`, selected
relative source path, and SHA-256. The compiler verifies those joins before resolving or
emitting any operand; observations from a same-named but stale/mixed lookup
are refused. It records every native `FindTagInfo` candidate for each source row name. A
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

Ownership is qualified native identity, not a bare spelling alone. An
unqualified `Software` that native finds in EXIF and XMP is terminal, while an
explicit `XMP:Software` whose native candidates are all outside generated
EXIF/IFD0 rows is `outside_migrated_scope`; it must not be swallowed by the
EXIF migration. `resolve_batch` is atomic: if any request fails, it returns no
accepted operands, including when two aliases carry conflicting values.

The Rust generator emits static address rows, native lookup candidates, and
source-owned names. If source/template/probe compilation fails, it still emits
the current raw source-owned names with `SET_NEW_VALUE_ADDRESSING = None`, so a
router has a terminal ownership result instead of reopening a legacy route. It remains inactive with
`inactive_source_addressing_no_public_setnewvalue_admission`.

Current-source ownership cannot identify a tag that an upstream release has
removed or renamed: that needs a separately authenticated persisted ownership
ledger and source-removal policy. `setnewvalue_ownership_ledger.py` supplies
that persisted generated ledger: it derives qualified EXIF/IFD0 names from the
authenticated current source, validates the previous ledger's canonical digest
and source/capture identity, then carries the deterministic union forward.
Entries are explicitly `current` or `removed`; a removed or renamed source name
remains terminal and is emitted in `SET_NEW_VALUE_OWNED_NAMES` plus
`SET_NEW_VALUE_OWNED_QUALIFIED`.

The first ledger requires `--bootstrap-ownership-ledger`; absence of a prior
ledger is never treated as retirement. Later runs take `--ownership-ledger` and
may write `--write-ownership-ledger`. The ledger records source version,
capture closure identity, helper identity, row/query digests, type-sensitive
source fingerprints, and per-name history. It does not use a historical
handwritten tag list or authorize a legacy writer fallback.

This does not implement public SetNewValue. Unsupported portions include
wildcards, language suffixes, shortcuts, multiple/numbered/ID qualifiers,
ExifIFD and other group forms, priority/preferred/avoid handling, protected
tags, list recursion, deletion, NEW_VALUE construction, conversion, CHECK_PROC
execution, serialization, and file writes.
