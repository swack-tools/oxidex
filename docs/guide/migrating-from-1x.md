# Migrating from 1.x to 2.0

2.0 moves OxiDex's output much closer to ExifTool's. Almost every breaking
change below is a place where 1.x differed from ExifTool 13.59 and 2.0 no
longer does. The full list, with the PR or commit behind each entry, is in
the [changelog](/changelog).

::: warning v2.0.0-beta.1
This guide covers v1.2.1 → v2.0.0-beta.1. It is a beta, so output and API
may still change before 2.0.0.
:::

## If you run the command line

| 1.x | 2.0 | What to do |
| --- | --- | --- |
| Raw values (`Flash: 24`, `FNumber: 9/5`) | ExifTool's display values (`Flash: Auto, Did not fire`, `FNumber: 1.8`) | Pass `--no-print-conv` if you parse raw values. `-n` is still the rename dry run, not ExifTool's `-n`. |
| An unreadable or unparsed file printed `Error:` and exited 1 | For a single file: exits 0 with the identity and filesystem tags, plus a `File:Warning`. Several files or `-r` still report `Error reading …` and exit 1. | Pass `--strict` to fail as before, or check `Status` in JSON |
| Unknown tags printed under hex names (`IFD0:0xF999`) | Hidden, together with OxiDex's own diagnostic tags | Pass `--extended-output` to see them |
| `(Binary, N bytes)` | `(Binary data N bytes, use -b option to extract)` | Update any pattern that matches the placeholder |
| Dates in RFC 3339 form | `YYYY:MM:DD HH:MM:SS` | Parse ExifTool's date form |
| Lists as `[a, b]` | `a, b` | Split on `, ` |
| `-e` did not exist | `-e` is accepted and does nothing | Nothing to do |

## If you read the JSON output

- **Values are typed as ExifTool types them.** Numbers are JSON numbers,
  written as spelled (`2.00` stays `2.00`). `true`/`false` are booleans.
  Floats use Perl's `%.15g` (`2`, not `2.0`). Rationals are a quotient, or
  `"inf"`/`"undef"`, not an `"n/d"` string. Do not assume every value is a
  string.
- **A new top-level key, `"Status"`,** appears when a single-file read did
  not complete: `Partial`, `IdentifiedOnly` or `Unsupported`. A fully parsed
  file has no `Status` key. Multi-file and `-r` output does not carry it.

## If you match on tag names or groups

- `FileType`, `MIMEType` and `FileSize` appear **once, under `File:`**. The
  bare duplicates some parsers wrote are gone.
- The BMP, FLIF, PFM, ICO, OpenEXR, Radiance HDR, PCAP, MP3 and MPC readers
  now put their file-level tags under `File:` (`ImageWidth` →
  `File:ImageWidth`). FITS tags are under `FITS:`.
- `File:FileType` uses ExifTool's names: `CR2`, `NEF` and so on, not
  `CanonCR2` or `NikonNEF`.
- `Fujifilm:*` is now `FujiFilm:*`, and its tag 0x100e is `NoiseReduction`
  (formerly `HighISONoiseReduction`).
- `.fit` files are Garmin FIT (`FileType: FIT`), no longer FITS.
- `File:File*Date` values are local time with the correct offset, and
  `FileSize` is formatted as ExifTool formats it.

EXIF keys did **not** change. `IFD0:Make`, `ExifIFD:ISO` and `GPS:*` were
already the keys in 1.2.1.

## If you use the Rust library

- **`read_metadata` succeeds on files it cannot parse.** 1.x returned
  `Err(UnsupportedFormat)` for a format with no parser. 2.0 returns `Ok`
  with the identity tags. When you need to know, call
  `oxidex::core::read_metadata_report` and check `report.status`
  (`ParseStatus::Parsed`, `Partial`, `IdentifiedOnly`, `Unsupported`).
- **`FileFormat` has 69 more variants** and is not `#[non_exhaustive]`. Add a
  wildcard arm to any exhaustive `match`.
- **Moved modules:**
  - `oxidex::ffi::c_api` → `oxidex::ffi`
  - `oxidex::parsers::format_detector` → `oxidex::parsers::detection`
    (`oxidex::parsers::detect_format` still works)
  - `oxidex::parsers::icc_parser` → `oxidex::parsers::icc`.
    `parse_icc_profile_data` now returns `Vec<IccTag>`, not a `HashMap`.
- **New, not changed:** the `Metadata` builder (`from_path`, `set_tag`,
  `save`, `copy_to`) is new in 2.0. See the [library guide](/guide/library-api).
  `TagValue`, `ExifToolError` and the `MetadataMap` methods that existed in
  1.2.1 keep their signatures.
- **Toolchain:** the crate now uses edition 2024, so build it with a recent
  Rust. The repository pins 1.97.1 in `rust-toolchain.toml`.
- **Depend on Git, not crates.io.** The `oxidex` crate on crates.io is not
  this project. See [Installation](/guide/getting-started).

## If you use the C API

The 15 exported `exiftool_*` functions and the `EXIFTOOL_*` error codes are
unchanged. `include/oxidex.h` now declares those real names. The 1.x header
declared `oxidex_*` names that the library never exported. Code written
against the old header must switch to `exiftool_*` and `ExifToolHandle`.
It could never have linked anyway. See the [C API reference](/reference/ffi-api).

## Not yet confirmed

These may affect you. They have not been checked against a v1.2.1 build:

- whether individual library values changed type, now that most values are
  converted the way ExifTool converts them (a tag that used to be an integer
  may now hold a display string);
- per-format tag renames recorded in PR titles (for example Mach-O, PSD,
  X3F, LNK, HTML and ICS) and the removal of invented tags that ExifTool
  does not emit;
- how `-G`/`-G1` behaved in 1.2.1. In 2.0 they print ExifTool's family 0 and
  family 1 groups.

If you rely on any of these, compare the two versions on your own files.
