# Inactive write descriptor API

`write_descriptors.py` consumes `native_write_tables` and may be selected with
`codegen.py --write-out`. Its generated Rust is not re-exported by OxiDex and
creates no writer route. It is an input candidate for a later, independently
verified writer-mechanism contract.

The first closed class is a plain `Exif::Main` scalar row with native
`Writable => 'string'` and a literal effective `WriteGroup` in `IFD0`, `ExifIFD`,
or `GPS`. Each
candidate carries only source-derived `raw_id`, `name`, `WritePhysicalGroup`,
and `WriteValueType::Ascii`, together with its exact fully-qualified native
table identity and effective native table groups. Group values apply
`GetTagTable`'s false-value defaults and retain the group-0 context a later
`CharsetEXIF` encoding contract needs. They are source facts, not a routing or
encoding decision. The candidate also carries actual loaded autoload/
`WRITE_PROC`/`CHECK_PROC` provenance: fully-qualified callable name, normalized
library-relative source file, lowercase SHA-256 source digest, B::Deparse
SHA-256, and captured direct dependencies.

Those procedure facts are **not** a writer admission and are not interpreted
as an implementation of `WriteExif` or `CheckExif`. A later writer must require
its own closed native-mechanism contract before it can use a candidate.

## Shared value-helper provenance

The dump also carries a top-level `native_write_helpers` map with
`write_value` and `check_value` facts. Each uses the established CODE fact
shape: `resolved`, callable name, deparse, normalized source path, source and
deparse SHA-256 values, and bounded direct dependency facts. The dumper records
the final package bindings only after every requested table module and permitted
writer autoload has settled. It then loads `Image/ExifTool/Writer.pl` solely to
make these shared helper bindings observable; it never calls a native writer.
An unreadable helper module or missing callable remains an explicit unresolved
fact.

`requested_binding` is the fully-qualified package glob the caller requested.
It remains distinct from `__name`: a later source module may legitimately bind
that glob to an anonymous CODE ref, whose actual callable name is
`Image::ExifTool::__ANON__`. Consumers must bind calls through
`requested_binding` while retaining the actual CV's body and provenance from
the remaining fact fields.

This map is provenance for a future default UTF-8 scalar mechanism, not proof
that `WriteValue` or `CheckValue` is safe to execute. A mechanism compiler must
still recognize the required native bodies and test their effects. Non-default
`CharsetEXIF` encoding requires a separate option and `Encode` contract; it
must not be inferred from these helper facts. The sidecar is ignored by the
inactive descriptor generator, so adding or changing helper provenance cannot
admit a writer candidate by itself.

Every source table and every source row alternative not in the initial class is
recorded in `OMITTED_WRITE_NATIVE_TABLES` or `OMITTED_WRITE_NATIVE_ROWS` with
named reasons. Reader omission flags are never used for this accounting. The
sidecars preserve source identity, including zero-row tables and array
alternative order.

The descriptor faithfully carries ordinary native physical `IFD0`, `ExifIFD`,
and `GPS` group strings. A future runtime may stage those writer primitives
separately; their presence here is not an activation claim. A changed name or
compatible added string row produces a changed descriptor; an unmodeled
physical group, type, write control, unknown property, or unresolved procedure
provenance is a refusal rather than a guessed writer operation.

The compiler binds the outer sidecar map key to the inner `module`, `table`,
and `full_name` before applying this source class. It rejects malformed or
non-relative provenance paths, non-SHA-256 digests, and unrepresentable group
maps. `--write-out` is optional: requesting this inactive artifact does not
change the ordinary generated binary artifact.
