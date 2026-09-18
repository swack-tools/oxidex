# Troubleshooting

## A file shows only `File:` tags

```text
File:FileType: RTF
File:MIMEType: text/rtf
File:LineCount: 25
…
```

The file was **identified but not parsed**. OxiDex names many more file
types than it has parsers for. Some formats, such as RTF, JSON and URL
files, are read as plain text, which gives line and word counts rather than
ExifTool's tags for the format. Other formats get only their identity and
filesystem tags. [Supported formats](/reference/formats/) lists which is
which.

In JSON output, a read that did not complete carries a top-level `Status`:

- `"Status": "Partial"`: the parser hit a problem, for example
  `File:Warning: JPEG format error` on a truncated JPEG.
- `IdentifiedOnly`: no parser exists for the identified type.
- `Unsupported`: the file was not identified at all.

A fully parsed file has no `Status` key. The library reports the same
through `read_metadata_report` (see the [Rust library guide](/guide/library-api#knowing-how-far-a-read-got)).
To make an incomplete read an error, pass `--strict`.

## A tag I expected is missing, or its value differs from ExifTool

1. **Check the name OxiDex uses.** Output keys are group-qualified
   (`IFD0:Make`, `ExifIFD:ISO`). `oxidex -j -a -G1 file` lists every key and
   every occurrence, for comparison with `exiftool -j -a -G1 file`.
2. **Compare with the right ExifTool.** OxiDex targets ExifTool **13.59**
   (`.exiftool-version`). Another release can legitimately print something
   different. From a checkout, `just compare-file path/to/file` diffs one
   file against the pinned release and names both tools in its output.
3. **Raw or converted?** ExifTool's `-n` is `--no-print-conv` in OxiDex. The
   OxiDex `-n` flag is a dry run for renames.
4. If it is still different, it is a real gap or defect. Please report it
   (see below). [ExifTool parity](/guide/exiftool-parity) explains how gaps
   are measured.

## Writing fails

| Message | Cause |
| --- | --- |
| `Unsupported format: Write operations for format TXT are not supported` | Only JPEG (EXIF), TIFF and TIFF-based RAW, PNG and PDF can be written. See [Writing metadata](/guide/writing). |
| `Invalid value for tag 'EXIF:FNumber': Not a floating point number` | The value does not fit the tag's type |
| `Cannot modify file in read-only mode (--readonly flag set)` | Remove `--readonly` |

OxiDex keeps **no `_original` backup**. Pass `--backup` for a `.bak` copy.
Only 19 tags are proven to produce the same bytes as ExifTool's own write.
Anything else may differ, and a report with the file is welcome.

## `-n` did not print raw values

In OxiDex, `-n` means "dry run" (for `-FileName<…` renames). Use
`--no-print-conv`.

## Warnings on stderr

Messages such as `Warning: Found SubIFD1 which is unusual` are written to
stderr, and they do not change the exit status. Structured problems with a
file are also reported as `File:Warning` / `File:Error` tags, as ExifTool
reports them.

## Build problems

- **`cargo install oxidex` installs something else.** The crates.io name
  belongs to an unrelated stub crate. Build from source; see
  [Installation](/guide/getting-started).
- **`cargo test --workspace --release` fails with bogus `panic strategy` or
  duplicate `chrono` errors.** This is an output filename collision after
  `cargo clippy --all-features` has used the same target directory, and it
  happens on a clean checkout too. Run
  `cargo clean --release -p chrono -p oxidex`, or use `cargo test --workspace`.
- **A slow first build.** The generated tables and the six tag crates are
  large. Later builds are incremental.

## Reporting a problem

Open an issue at
[github.com/swack-tools/oxidex/issues](https://github.com/swack-tools/oxidex/issues)
and include:

- `oxidex --version`, and the commit if you built from source;
- the exact command and its output;
- for a parity problem, the output of ExifTool **13.59** for the same
  command (`exiftool -ver` should print `13.59`);
- the file, or a small file that reproduces the problem, if you can share it.
