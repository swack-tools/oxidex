# Writing metadata

::: warning Beta: v2.0.0-beta.1
Write support is narrower than read support, and most of it has not yet
been proven against ExifTool. This page separates what is **proven** from
what is merely **implemented**. Output and API may still change before 2.0.0.
:::

## Which formats can be written

`write_metadata` (`src/core/operations.rs`) chooses a writer by the detected
format:

| Format | What is written |
| --- | --- |
| **JPEG** | The EXIF APP1 segment. XMP and IPTC in JPEG are **not** written. |
| **TIFF**, and TIFF-structured RAW (for example NEF, CR2, ARW, DNG, PEF, RW2, IIQ) | IFD entries, edited in place by the surgical TIFF writer. The file header must be a walkable classic TIFF: `II`/`MM` with magic 42 or 85. |
| **PNG** | `tEXt`, `iTXt`, `zTXt` and `eXIf` chunks |
| **PDF** | A new Info-dictionary revision appended to the file |

Every other format returns `UnsupportedFormat`. That includes BigTIFF, ORF,
RAF, MRW, X3F, CR3, CRW, MP4/QuickTime, HEIC and XMP sidecars.

Every write goes through a temporary file, an `fsync` and a rename
(`src/writers/atomic_writer.rs`). A failed write therefore leaves the
original file intact.

## Proven against ExifTool

A catalog entry counts as **write-proven** only if it passes both checks of
`tools/exiftool-tables/generated_tiff_write_matrix.py --route public-api`:

1. **Same bytes as ExifTool's own write.** The same seed file is written
   twice: once by OxiDex's public `modify_tag`/`remove_tag`, and once by
   the pinned ExifTool 13.59 (`SetNewValue` + `WriteInfo`). The two outputs
   are compared at the TIFF entry level (type, count and value bytes), and
   the rest of the file is compared too.
2. **ExifTool reads it back.** The pinned ExifTool reads both outputs. The
   new value must be present and identical in both, and must differ from
   the seed.

Credit needs an exact `Group1:Name` match with the catalog row. Nineteen
entries pass today, all of them in `Exif::Main` and written as `IFD0`:

`ProcessingSoftware`, `DocumentName`, `MinSampleValue`, `MaxSampleValue`,
`XResolution`, `YResolution`, `PageName`, `XPosition`, `YPosition`,
`Artist`, `HostComputer`, `TargetPrinter`, `SEMInfo`, `GDALMetadata`,
`GDALNoData`, `UniqueCameraModel`, `CameraSerialNumber`, `ReelName`,
`CameraLabel`.

That is **19 of the 14,169** entries ExifTool marks writable (0.13%). The
list is committed as `tools/exiftool-tables/tiff_scalar_final_ledger.json`,
and the observations as `docs/public/measurements/catalog-hydrated-observed-13.59.json`.
The [parity ratchet](/contributing/#what-ci-enforces) holds the count at a
floor of 19 or more.

**The public-API scalar write matrix** exercises those 19 tags through the
public API. Each tag is written under three spellings (`EXIF:`, `IFD0:`,
`IFD1:`) into three carriers (little-endian TIFF, big-endian TIFF, JPEG).
Each string tag runs 8 cases: insert, update, growth, shrinkage, delete,
empty, UTF-8 and embedded NUL. Each numeric tag runs 11. Together that
comes to **1,530 operations**. In the most recent recorded run, the 13.59
control of the upgrade rehearsal at `66e48654` (#826), all **1,530 of
1,530** matched ExifTool. See the
[rehearsal record](/reference/upgrade-rehearsal-11.78-12.64).

Writing any other tag goes through the same writers but is **not proven**.
It may work, but no instrument has yet compared it with ExifTool.

## From the command line

```bash
oxidex -EXIF:Artist="Jane Doe" photo.jpg          # set a tag (edits the file in place)
oxidex -EXIF:Artist="Jane Doe" -EXIF:Copyright="2026 Jane Doe" photo.jpg
oxidex -EXIF:Artist= photo.jpg                    # delete a tag
oxidex -TagsFromFile src.jpg dest.jpg             # copy all tags
oxidex -TagsFromFile src.jpg -EXIF:Artist dest.jpg
```

- **No `_original` backup by default.** Unlike ExifTool, OxiDex replaces the
  file atomically and keeps no copy. Pass `--backup` to copy `photo.jpg` to
  `photo.jpg.bak` first. `-overwrite_original` is not an OxiDex option.
- `--readonly` refuses every write, and `--preserve-file-times` restores
  the modification time afterwards.
- Each `-TAG=VALUE` is applied as its own read and write.
- `-all=` writes an empty tag set through the same format writer. What that
  removes depends on the writer, and it has not been compared with
  ExifTool's `-all=`.

## From Rust

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

For the builder-style `Metadata` API and batch writes, see the
[library guide](/guide/library-api#writing).
