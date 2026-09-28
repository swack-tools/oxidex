# Rust API reference

This page lists the public read and write API at the `refactor/tag-machinery`
tip (v2.0.0-beta.1), with signatures taken from the source. For a tutorial,
see the [Rust library guide](/guide/library-api). For every item, run
`cargo doc --open`.

::: warning Beta
The API may still change before 2.0.0. The crate is not on crates.io:
depend on the Git repository.
:::

## Paths

| Item | Path |
| --- | --- |
| `Metadata`, `VERSION` | crate root (`oxidex::Metadata`, `oxidex::VERSION`) |
| `MetadataMap`, `TagValue`, `FileFormat`, `ReadOptions`, `ReadReport`, `ParseStatus`, `Diagnostic` | `oxidex::core` |
| `WriteOutcome`, `TagChange`, `apply_tag_changes` | `oxidex::core` (re-exported) and `oxidex::core::write_transaction` |
| `read_metadata`, `read_metadata_report`, `write_metadata`, `modify_tag`, `remove_tag`, `clear_all_metadata` | `oxidex::core` (re-exported) and `oxidex::core::operations` |
| `copy_metadata`, `copy_metadata_report`, `CopyReport`, `read_metadata_with_detector` | `oxidex::core::operations` only |
| `Result<T>`, `ExifToolError`, `TagNotWritten` | `oxidex::error` (not at the crate root) |

## Reading

```rust,ignore
pub fn read_metadata(path: &Path) -> Result<MetadataMap>;
pub fn read_metadata_report(path: &Path) -> Result<ReadReport>;
pub fn read_metadata_with_detector_and_options(
    path: &Path, detector: DetectorMode, options: &ReadOptions,
) -> Result<MetadataMap>;
```

- `read_metadata` returns `Ok` for any file it can open. A damaged file,
  or a type with no parser, gives only what could be recovered, possibly
  just the identity tags.
- `read_metadata_report` returns the same map together with a
  `ParseStatus` (`Parsed`, `Partial`, `IdentifiedOnly`, `Unsupported`) and
  the diagnostics. `report.into_metadata()` gives you the map.
- Keys are the group-qualified names `oxidex -j` prints (`IFD0:Make`,
  `ExifIFD:ISO`, `File:FileType`), and lookups match them exactly.

### `MetadataMap`

| Method | Signature |
| --- | --- |
| `get` | `fn get(&self, key: &str) -> Option<&TagValue>` |
| `get_string` / `get_integer` / `get_float` | typed `Option` accessors |
| `contains_key`, `keys`, `values`, `iter`, `len` | map-like access |
| `insert`, `remove`, `get_mut` | modify before a `write_metadata` |

`MetadataMap` keeps every occurrence of a tag internally, and the map API
shows the priority winner for each key. It serialises to JSON as
`{"Group:Tag": {"type": …, "value": …}}`.

### `TagValue`

```rust,ignore
pub enum TagValue {
    String(String),
    Integer(i64),
    Float(f64),
    Rational { numerator: i32, denominator: i32 },
    Binary(Vec<u8>),
    DateTime(DateTime<Utc>),
    Struct(Box<HashMap<String, TagValue>>),
    Array(Vec<TagValue>),
}
```

It has constructors (`TagValue::new_string`, …), predicates (`is_*`) and
accessors (`as_string`, …). `From` conversions exist for `&str`, `String`,
`i64`, `i32`, `f64` and `f32`.

## Writing

```rust,ignore
pub fn modify_tag(path: &Path, tag_name: &str, new_value: TagValue) -> Result<WriteOutcome>;
pub fn remove_tag(path: &Path, tag_name: &str) -> Result<WriteOutcome>;
pub fn write_metadata(path: &Path, metadata: &MetadataMap) -> Result<WriteOutcome>;
pub fn clear_all_metadata(path: &Path) -> Result<WriteOutcome>;
pub fn copy_metadata(source: &Path, dest: &Path, tags: Option<&[String]>) -> Result<WriteOutcome>;
pub fn copy_metadata_report(source: &Path, dest: &Path, tags: Option<&[String]>) -> Result<CopyReport>;
pub fn apply_tag_changes(path: &Path, changes: &[TagChange]) -> Result<WriteOutcome>;

#[non_exhaustive]
pub enum WriteOutcome {
    Updated,   // the file's bytes changed (ExifTool WriteInfo's 1)
    Unchanged, // every change was already in effect (WriteInfo's 2)
}
```

`tests/api_reference_signatures.rs` compile-checks these signatures, and a
test fails if this page stops stating them.

- `Ok` means every requested change is in the file, proven by reading it
  back. Anything else is an error, and the file is byte-identical to before
  the call. A key the file's writer cannot write, such as `XMP:Title` in a
  JPEG, is `ExifToolError::TagsNotWritten`, which names every such key.
- Writes edit an existing file in place, through a temporary file and a
  rename. A request that changes nothing does not create the temporary file.
- `write_metadata` sets every row of the map that is new, differs from the
  file, or was assigned after the read. It deletes a row only when the map
  was read from this same file and you removed that row. A map built from
  scratch, or read from another file, only sets.
- A later request for the same tag replaces an earlier one, and a
  `GROUP:All` deletion (`remove_tag(path, "EXIF:All")`) keeps its place in
  the request order, as in ExifTool.
- `copy_metadata(src, dest, None)` copies every writable tag and skips the
  rest, which `copy_metadata_report` lists. A named tag that cannot be copied
  is refused.
- Only JPEG (EXIF), TIFF and TIFF-based RAW, PNG and PDF are writable.
  Other formats return `UnsupportedFormat`. See
  [Writing metadata](/guide/writing) for what is proven.

## `Metadata`

A convenience wrapper around a `MetadataMap` and its source path.

| Method | Signature | Notes |
| --- | --- | --- |
| `from_path` | `fn from_path<P: AsRef<Path>>(path: P) -> Result<Metadata>` | reads the file |
| `new` | `fn new() -> Metadata` | empty |
| `get_string`, `get_integer`, `get_float`, `get`, `has_tag` | lookups | exact keys |
| `set_tag` | `fn set_tag<V: Into<TagValue>>(self, tag: &str, value: V) -> Metadata` | builder style; cannot fail |
| `insert`, `remove` | in-place changes | |
| `save` | `fn save(&self) -> Result<WriteOutcome>` | writes back to the source file |
| `write_to` | `fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<WriteOutcome>` | into another *existing* file; sets only |
| `copy_to` | `fn copy_to<P: AsRef<Path>>(&self, dest: P) -> Result<CopyBuilder>` | then `.with_tags(&[..])?` and `.execute()?` |
| `len`, `is_empty`, `iter`, `source_path`, `as_map`, `into_map` | access | |

## Errors

```rust,ignore
#[non_exhaustive]
pub enum ExifToolError {
    IoError(std::io::Error),
    ParseError { .. },
    TagNotFound { .. },
    InvalidTagValue { .. },
    UnsupportedFormat { .. },
    TagsNotWritten { tags: Vec<TagNotWritten> },
}
```

`ExifToolError` is `#[non_exhaustive]`, so a `match` on it needs a wildcard
arm. `err.tags_not_written()` returns the keys a refused write named.

A value is validated when it is written, not when it is set.

## Not public API

`oxidex::exiftool_tables` (the generated tables, the engines and the v2
`Session`) is reachable as a module, but it is internal machinery. It will
change without notice. `TagOccurrence` is re-exported as
`oxidex::core::TagOccurrence`, while `ValueChannel` is available at
`oxidex::core::tag_occurrence::ValueChannel`. These occurrence and
value-channel interfaces are beta APIs: their shape and stability are not
guaranteed before 2.0.0. The generated write modules under `oxidex::writers`
are crate-private.

## See also

- [C API](/reference/ffi-api)
- [Migrating from 1.x to 2.0](/guide/migrating-from-1x)
