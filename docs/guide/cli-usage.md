# Command line

```text
oxidex [OPTIONS] [-TAG[=VALUE] ...] FILE|DIRECTORY ...
```

`oxidex` takes ExifTool-style arguments: `-TAG` to select a tag,
`-TAG=VALUE` to write one, and single-dash long options such as `-json`.
Its **values** aim to match the pinned ExifTool 13.59. Several **output
layouts** still differ from ExifTool's; they are listed under
[Differences from ExifTool](#differences-from-exiftool) below.

::: warning Beta: v2.0.0-beta.1
The command line and its output may still change before 2.0.0. The examples
on this page were checked against the current reviewed source snapshot, a
development/pre-tag state. Neither a final signed tag nor a release-qualified
SHA has been established.
:::

## Reading

```bash
oxidex photo.jpg                          # every tag
oxidex -Make -Model -DateTimeOriginal photo.jpg
oxidex a.jpg b.nef c.png                  # several files
oxidex -r /path/to/photos/                # a directory, recursively, in parallel
```

Text output names the file, then prints one `Group:Tag: value` line per
tag, sorted by name:

```text
$ oxidex -Make -Model -DateTimeOriginal photo.jpg
ExifIFD:DateTimeOriginal: 2003:12:04 06:46:52
IFD0:Make: Canon
IFD0:Model: Canon EOS DIGITAL REBEL
```

The group is ExifTool's family 1 group (`IFD0`, `ExifIFD`, `Canon`, `GPS`,
…), so the same tag name from two places stays distinguishable. `-s` drops
the group. `-G` prints ExifTool's family 0 group in brackets (`[EXIF] Make`),
and `-G1` prints family 1 (`[IFD0] Make`).

### JSON

```bash
oxidex -j photo.jpg
oxidex -j -a -G1 photo.jpg     # every occurrence, family-1 keys: compare with `exiftool -j -a -G1`
```

JSON keys are always group-qualified. Numbers are emitted as JSON numbers
and text as JSON strings, as ExifTool does. With `-j -a -G1`, keys and
values are formatted for comparison with `exiftool -j -a -G1`. This page does
not make a per-file or corpus parity claim; see [ExifTool parity](/guide/exiftool-parity)
for the named instrument and receipt behind each measured result. `-a` keeps
every occurrence of a tag rather than only the priority winner.

With more than one file, the output is an array with a `SourceFile` per
entry.

### Other output options

| Option | Effect |
| --- | --- |
| `--no-print-conv` | Raw stored values, without ExifTool's print conversion (ExifTool's `-n`). For example, `FNumber: 14` instead of `14.0`, and `Flash: 0` instead of `No Flash`. |
| `--csv` | Two columns, `Tag,Value`, one row per tag |
| `--extended-output` | Also show three classes of OxiDex's own diagnostic tags, which are hidden by default: JPEG SOF details, undecoded MakerNote hex, and per-entry ZIP forensics. Other OxiDex-only tags are still printed by default, among them executable hardening and import summaries, `OOXML:` document properties, and `EXE:Rich*`. Comparisons with ExifTool count them as EXTRA. |
| `--strict` | Fail a damaged or unidentifiable read instead of returning the partial result |
| `--detector magika` | Use the Magika model for file-type detection. Needs a build with `--features magika`. |
| `-e` | Accepted and ignored. ExifTool formatting is now the default. |

### Files OxiDex identifies but does not parse

Some file types are identified, with a correct `File:FileType` and
`MIMEType`, but not parsed. RTF, JSON and URL files, for example, are read
as plain text: you get line and word counts, not ExifTool's tags for those
formats. [Supported formats](/reference/formats/) lists them.

## Writing

```bash
oxidex -EXIF:Artist="Jane Doe" photo.jpg                # set
oxidex -EXIF:Artist="Jane Doe" -EXIF:Copyright="2026" photo.jpg
oxidex -EXIF:Artist= photo.jpg                          # delete
oxidex -all= photo.jpg                                  # remove all metadata the format writer handles
oxidex -TagsFromFile src.jpg dest.jpg                   # copy everything
oxidex -TagsFromFile src.jpg -IFD0:Make dest.jpg        # copy selected tags
```

A successful write prints `1 image files updated`. Writes are atomic and in
place. **No `_original` copy is kept** unless you pass `--backup`, which
first copies `photo.jpg` to `photo.jpg.bak`. `--readonly` makes every write
fail, and `--preserve-file-times` restores the modification time afterwards.

Only JPEG (EXIF), TIFF and TIFF-based RAW, PNG and PDF can be written, and
only some tags are proven against ExifTool. See
[Writing metadata](/guide/writing).

### Shifting dates

```bash
oxidex "-AllDates+=1:0:0 0:0:0" photo.jpg            # add one year
oxidex "-EXIF:DateTimeOriginal-=0:0:0 1:0:0" photo.jpg
oxidex "-EXIF:DateTimeOriginal=2026:01:15 10:30:00" photo.jpg
```

`AllDates` covers `DateTimeOriginal`, `CreateDate` and `ModifyDate`.

## Renaming files from metadata

```bash
oxidex -n '-FileName<DateTimeOriginal' -d %Y%m%d_%H%M%S photo.jpg   # dry run
oxidex '-FileName<DateTimeOriginal' -d %Y%m%d_%H%M%S photo.jpg
oxidex '-FileName<${IFD0:Make}_${IFD0:Model}' photo.jpg
```

`-d` takes a chrono format string. **`-n` is a dry run** here. It prints
`old -> new` and changes nothing.

## Differences from ExifTool

| | ExifTool 13.59 | OxiDex |
| --- | --- | --- |
| `-n` | raw values | dry run for renames. Use `--no-print-conv` for raw values. |
| Writing | keeps `file_original` unless `-overwrite_original` | replaces the file atomically. `--backup` keeps `file.bak`. |
| Default text output | `Tag Description : value`, in file order | `Group:Tag: value`, sorted by name |
| Plain `-j` | bare tag names, in file order | group-qualified keys, sorted by name |
| `SourceFile` in JSON | always | only when more than one file is read |
| `--csv` / `-csv` | one row per file, one column per tag | `Tag,Value`, one row per tag |
| `-g` (grouped listing) | `---- Group ----` headings | accepted, not implemented |
| `-s` | short tag names, repeatable | drops the group; otherwise partial |
| Tags | every tag ExifTool reads | see [ExifTool parity](/guide/exiftool-parity) |

The exit status is 1 when a file cannot be opened or written. For a
**single file**, a damaged or unparsed read still exits 0, and its JSON
carries a `Status` key; `--strict` turns that into exit 1. With **several
files or `-r`**, a damaged or unidentifiable file is reported on stderr as
`Error reading <file>: …` and the command exits 1. See
[Troubleshooting](/guide/troubleshooting).
