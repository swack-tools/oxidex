# Introduction

OxiDex is a Rust reimplementation of [ExifTool](https://exiftool.org/), Phil
Harvey's metadata reader and writer. It is a command-line tool, a Rust
library and a C library. Its target is ExifTool's own output: the same tag
names, groups and values that ExifTool 13.59 prints for the same file.

::: warning v2.0.0-beta.1: a pre-release
These docs describe the pre-tag 2.0 development line on
`refactor/tag-machinery`; they do not document a published beta release. The
final reviewed `main` SHA, signed tag, release date, assets, and release
receipts remain pending. Output and API may still change before 2.0.0. Tag
keys, value formatting and parts of the API changed since 1.x; see
[Migrating from 1.x to 2.0](/guide/migrating-from-1x). The last stable
release is [v1.2.1](https://github.com/swack-tools/oxidex/releases/tag/v1.2.1).
:::

## How it works

ExifTool is mostly a generic engine plus declarative tag tables written in
Perl. OxiDex does not retype those tables. It dumps them from the pinned
ExifTool source and generates Rust from them: tag IDs, byte layouts,
selection conditions and value conversions. CI verifies the generated code
against the live Perl. Procedural code, such as the walkers that find the
tables inside a JPEG, TIFF or QuickTime file, is ported by hand, one named
Perl subroutine at a time. See [Architecture](/architecture/).

## What it can do today

- ✅ 16,684 metadata tag definitions, synced from ExifTool's own tag database
  in this development snapshot. A definition says a tag exists, not that
  OxiDex reads it; it is not a v2.0.0-beta.1 parity receipt.
- 🔶 **Reading.** Development-state source inspection maps 131 formats for
  detection, of which 129 map to a parser, including camera RAW with 36
  sub-formats. Candidate read coverage remains pending the parity receipt.
  Many more types are identified but not parsed. See [Supported
  formats](/reference/formats/).
- 🔶 **Release parity pending.** The v2.0.0-beta.1 parity receipt has not yet
  been recorded. The regression gate is a safeguard, not a release-coverage
  claim; see [ExifTool parity](/guide/exiftool-parity) and the
  [status page](/status/).
- 🔶 **Writing.** JPEG EXIF, TIFF and TIFF-based RAW, PNG and PDF can be
  written, atomically. 19 tags are proven byte-for-byte against ExifTool.
  See [Writing metadata](/guide/writing).
- 🔶 **Performance receipt pending.** The figures on the
  [Performance](/performance/) page are historical measurements at commit
  `8f04e288` for OxiDex 1.2.1, explicitly not v2.0.0-beta.1 evidence.
- ✅ **Interfaces.** An ExifTool-style CLI, a Rust library, a C API, and a
  separate MCP server.

## What it is not (yet)

- **Not a complete ExifTool.** Many tags ExifTool reads are still missing,
  and the parity measurements list them. Where a conversion cannot be
  reproduced exactly, OxiDex omits the tag rather than print a plausible
  wrong value.
- **Not a drop-in replacement for scripts.** The arguments are ExifTool-style,
  but some options differ. Most notably, `-n` is a dry run and there is no
  `_original` backup. Output layout also differs. See
  [Differences from ExifTool](/guide/cli-usage#differences-from-exiftool).
- **Not a general writer.** Most formats and most tags cannot yet be
  written, or are not yet proven.

## Next steps

- [Install OxiDex](/guide/getting-started)
- [Command line](/guide/cli-usage)
- [Rust library](/guide/library-api)
- [Writing metadata](/guide/writing)

## License

GPL-3.0. OxiDex is an independent reimplementation, and is not affiliated
with or endorsed by the ExifTool project.
