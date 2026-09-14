# Native write format registry fact

`dump_tables.pl` emits `native_write_format_registry` after the selected native
process has loaded its requested table modules and settled writer helpers. It
reads the live `Image::ExifTool::Exif` globals rather than parsing source or
reconstructing a list: `@formatName`, `@formatSize`, and `%formatNumber`.

The fact is separate from `modules` and `native_write_tables`, so it cannot
alter table-reading behavior or admit a writer. It carries library-relative
Exif.pm provenance, preserves sparse array slots as JSON `null`, and retains
aliases in `%formatNumber`. Capture never loads Exif after other final facts
have been recorded: when that module was not loaded, the fact explicitly
refuses with `format_registry_not_loaded`. Generic structural checks require every canonical
array name to map to its index and every numeric-map target to have a name and
positive size. An unknown/malformed live shape emits a visible `state: refused`
with a reason instead of a fabricated registry.

This supplies source facts for a later final TIFF type/count stage only. It
does not route tags, execute conversions, encode text, rebuild IFDs, or claim
writer parity.
