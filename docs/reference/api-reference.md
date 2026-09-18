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
| `read_metadata`, `read_metadata_report`, `write_metadata`, `modify_tag`, `remove_tag`, `clear_all_metadata` | `oxidex::core` (re-exported) and `oxidex::core::operations` |
| `copy_metadata`, `read_metadata_with_detector` | `oxidex::core::operations` only |
| `Result<T>`, `ExifToolError` | `oxidex::error` (not at the crate root) |

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
pub fn modify_tag(path: &Path, tag_name: &str, new_value: TagValue) -> Result<()>;
pub fn remove_tag(path: &Path, tag_name: &str) -> Result<()>;
pub fn write_metadata(path: &Path, metadata: &MetadataMap) -> Result<()>;
pub fn clear_all_metadata(path: &Path) -> Result<()>;
pub fn copy_metadata(source: &Path, dest: &Path, tags: Option<&[String]>) -> Result<()>;
```

- Writes edit an existing file in place, through a temporary file and a
  rename.
- `write_metadata` re-reads the file and applies only the changes between
  that read and the map you pass.
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
| `save` | `fn save(&self) -> Result<()>` | writes back to the source file |
| `write_to` | `fn write_to<P: AsRef<Path>>(&self, path: P) -> Result<()>` | into another *existing* file |
| `copy_to` | `fn copy_to<P: AsRef<Path>>(&self, dest: P) -> Result<CopyBuilder>` | then `.with_tags(&[..])?` and `.execute()?` |
| `len`, `is_empty`, `iter`, `source_path`, `as_map`, `into_map` | access | |

## Errors

```rust,ignore
pub enum ExifToolError {
    IoError(std::io::Error),
    ParseError { .. },
    TagNotFound { .. },
    InvalidTagValue { .. },
    UnsupportedFormat { .. },
}
```

A value is validated when it is written, not when it is set.

## Not public API

`oxidex::exiftool_tables` (the generated tables, the engines and the v2
`Session`) is reachable as a module, but it is internal machinery. It will
change without notice. The generated write modules under `oxidex::writers`
are crate-private.

## See also

- [C API](/reference/ffi-api)
- [Migrating from 1.x to 2.0](/guide/migrating-from-1x)
