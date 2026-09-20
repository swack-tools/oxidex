# Rust library

::: warning Beta: v2.0.0-beta.1
The library API may still change before 2.0.0. This is pre-tag guidance: the
v2.0.0-beta.1 signed tag is pending. OxiDex is not published on crates.io (the
`oxidex` name there belongs to an unrelated crate), so depend on the Git
repository for development use. The final crates.io decision and signed-tag
dependency instructions remain pending. The 2.0 API differs from 1.x; see
[Migrating from 1.x to 2.0](/guide/migrating-from-1x).
:::

```toml
[dependencies]
oxidex = { git = "https://github.com/swack-tools/oxidex", branch = "refactor/tag-machinery" } # pre-tag development only
```

The API is synchronous. It takes file paths and returns
`oxidex::error::Result<T>`. For the full item list, run `cargo doc --open`.
The [API reference](/reference/api-reference) lists the public items and
their signatures.

## Reading

```rust,no_run
use oxidex::core::operations::read_metadata;
use std::path::Path;

fn main() -> oxidex::error::Result<()> {
    let map = read_metadata(Path::new("photo.jpg"))?;

    if let Some(make) = map.get_string("IFD0:Make") {
        println!("Make: {make}");
    }
    for (key, value) in map.iter() {
        println!("{key}: {value:?}");
    }
    Ok(())
}
```

**Keys are group-qualified**, and they are the same keys `oxidex -j` prints:
the family 1 group and the tag name, such as `IFD0:Make`, `ExifIFD:ISO`,
`GPS:GPSLatitude` or `Canon:LensModel`. Filesystem and identity facts live
under `File:`, such as `File:FileType` and `File:MIMEType`. A lookup by bare
name, or by the family 0 group (`EXIF:Make`), finds nothing. To see which
keys a file produces, run `oxidex -j file`.

Values are held as decoded, which is what `oxidex --no-print-conv` prints.
The CLI applies ExifTool's print conversion (`Flash: 0` → `No Flash`) when
it formats output. `MetadataMap` accessors:

| Method | Returns |
| --- | --- |
| `get(key)` | `Option<&TagValue>` |
| `get_string(key)`, `get_integer(key)`, `get_float(key)` | typed `Option`s |
| `contains_key(key)`, `keys()`, `values()`, `iter()`, `len()` | as for a map |

`TagValue` has eight variants: `String`, `Integer`, `Float`,
`Rational { numerator, denominator }`, `Binary`, `DateTime`, `Struct` and
`Array`. Match on it with a wildcard arm, because the beta may add variants.

### Knowing how far a read got

`read_metadata` succeeds even when a file is damaged or has no parser. The
result then holds only what could be recovered, which may be just the
identity tags. When the difference matters, use `read_metadata_report`:

```rust,no_run
use oxidex::core::{read_metadata_report, ParseStatus};
use std::path::Path;

fn main() -> oxidex::error::Result<()> {
    let report = read_metadata_report(Path::new("file.bin"))?;
    match report.status {
        ParseStatus::Parsed => {}
        ParseStatus::Partial => eprintln!("partial read: {:?}", report.diagnostics),
        ParseStatus::IdentifiedOnly => eprintln!("identified, but no parser"),
        ParseStatus::Unsupported => eprintln!("not identified"),
    }
    let _map = report.into_metadata();
    Ok(())
}
```

`IdentifiedOnly` is the "detected is not parsed" case. `FileType` is
correct, and nothing else was read.

## Writing

Only some formats and tags can be written, and fewer are proven against
ExifTool. Read [Writing metadata](/guide/writing) first.

One tag at a time:

```rust,no_run
use oxidex::core::operations::{modify_tag, remove_tag};
use oxidex::core::TagValue;
use std::path::Path;

fn main() -> oxidex::error::Result<()> {
    let path = Path::new("photo.jpg");
    modify_tag(path, "IFD0:Artist", TagValue::new_string("Jane Doe"))?;
    remove_tag(path, "IFD0:Artist")?;
    Ok(())
}
```

With the `Metadata` builder, several changes are made in one write:

```rust,no_run
use oxidex::Metadata;

fn main() -> oxidex::error::Result<()> {
    Metadata::from_path("photo.jpg")?
        .set_tag("EXIF:Artist", "Jane Doe")
        .set_tag("EXIF:Copyright", "2026 Jane Doe")
        .save()?;
    Ok(())
}
```

- `set_tag` consumes and returns the `Metadata`, and cannot fail. Values
  are validated when you call `save()` or `write_to()`.
- `save()` writes back to the file the metadata was read from. `write_to(path)`
  writes the same changes into another *existing* file of a writable
  format. It does not create a new file.
- A write re-reads the target and writes only the tags that changed. It
  goes through a temporary file and a rename, so a failure leaves the
  original intact.

### Copying between files

```rust,no_run
use oxidex::Metadata;

fn main() -> oxidex::error::Result<()> {
    let source = Metadata::from_path("source.jpg")?;
    source.copy_to("dest.jpg")?.execute()?;                                        // everything
    source.copy_to("dest.jpg")?.with_tags(&["IFD0:Make", "IFD0:Model"])?.execute()?; // selected tags
    Ok(())
}
```

## Errors

`oxidex::error::ExifToolError` has five variants: `IoError`, `ParseError`,
`TagNotFound`, `InvalidTagValue` and `UnsupportedFormat`. Writing to a
format without a writer returns `UnsupportedFormat`.

```rust,no_run
use oxidex::core::operations::modify_tag;
use oxidex::core::TagValue;
use oxidex::error::ExifToolError;
use std::path::Path;

fn main() {
    match modify_tag(Path::new("clip.mp4"), "IFD0:Artist", TagValue::new_string("x")) {
        Ok(()) => {}
        Err(ExifToolError::UnsupportedFormat { .. }) => eprintln!("this format cannot be written"),
        Err(e) => eprintln!("{e}"),
    }
}
```

## Many files

The API is plain functions over paths, so use your own parallelism. The CLI
itself uses rayon:

```rust,no_run
use oxidex::core::operations::read_metadata;
use rayon::prelude::*;
use std::path::PathBuf;

fn main() {
    let files: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    let results: Vec<_> = files.par_iter().map(|p| (p, read_metadata(p))).collect();
    for (path, result) in results {
        match result {
            Ok(map) => println!("{}: {} tags", path.display(), map.len()),
            Err(e) => eprintln!("{}: {e}", path.display()),
        }
    }
}
```

## From other languages

The C API is described in the [C API reference](/reference/ffi-api). It
exports fifteen `exiftool_*` functions, with a header generated by cbindgen
(`just cbindgen`). The MCP server, [oxidex-mcp](/guide/mcp-integration), is
a separate project.
