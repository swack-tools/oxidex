# Hydrated layout projection

`dump_tables.pl` keeps its existing `modules` projection as the default. It
continues to walk only the requested module stashes, so current readers and
code generators retain their module/table-keyed input contract.

`--hydrated-layouts` adds `hydrated_layouts`, a separate
`oxidex_hydrated_layout_projection_v1` document member. The producer loads
ExifTool's complete table registry with `LoadAllTables`, resolves each catalog
identity with `GetTagTable`, and serializes its full-name-keyed metadata, rows,
formats, conversions, and processor facts through the ordinary dump serializers.
A no-filter invocation must serialize every identity in
`%Image::ExifTool::allTables`; `table_count`, `requested_table_count`, and
`available_table_count` conserve that set.

For bounded native tests, repeat `--hydrated-layout-table <full-name>` after
`--hydrated-layouts`. This makes an explicitly labelled subset and rejects a
name outside the hydrated registry. It is not a replacement for a complete
capture.

`Image::ExifTool::Extra` and `Image::ExifTool::Composite` are normal full-name
layout records with kinds `extra_generated` and `composite_aggregate`.
`Image::ExifTool::Shortcuts::Main` is not an `allTables` layout: it remains a
separate `shortcut_macro_table` helper with an entry count, rather than a
fabricated row set. Hydrated runtime graphs can contain cyclic or shared
structures. A property pointing at a catalog table is emitted as a
`tag_table` reference with its complete `table_full_names`; other references
carry an `object_id` into `shared_reference_objects`. `Table` and `TagID` are
captured directly on every hydrated row; all other non-metadata runtime fields
are retained with values in `_extra_properties`. The reference is an explicit
source/runtime binding, never an omitted property.

This projection is source/layout evidence. It does not enable a Rust parser or
writer, prove a carrier reaches any table, or replace the existing catalog
reconciliation and full capture gate. `native_write_tables` remains the legacy,
selected-module writer sidecar: hydrated catalog tables do not expand its scope
or make complete writer facts claimable.
