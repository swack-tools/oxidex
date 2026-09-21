---
layout: home

hero:
  name: OxiDex
  text: ExifTool's output, in Rust
  tagline: A reimplementation of ExifTool whose tag tables are generated from ExifTool's own Perl source, and whose output is measured against the pinned ExifTool 13.59.
  actions:
    - theme: brand
      text: Get started
      link: /guide/getting-started
    - theme: alt
      text: ExifTool parity
      link: /guide/exiftool-parity
    - theme: alt
      text: Project status
      link: /status/

features:
  - icon: 🧬
    title: Generated from ExifTool's source
    details: Tag tables, byte layouts, conditions and conversions are dumped from the pinned Perl modules and generated into Rust. CI regenerates them and verifies them against live Perl. Whatever cannot be modelled is refused and counted, never approximated.
    link: /architecture/
    linkText: Architecture
  - icon: 🎯
    title: 16,684 Tag Definitions
    details: 77.5% measured extraction conformance against pinned ExifTool, across 126 file types, in the last published coverage report. It is historical evidence, not a v2.0.0-beta.1 parity receipt; a definition says a tag exists, not that OxiDex extracts it.
    link: /guide/exiftool-parity
    linkText: How parity is measured
  - icon: 🛡️
    title: Proven reads cannot regress
    details: Release parity evidence remains pending. The regression gate and its ratchets are safeguards, not a v2.0.0-beta.1 parity receipt.
    link: /guide/exiftool-parity#guarantees-that-stop-regressions
    linkText: The guarantees
  - icon: ⚡
    title: Historical benchmark record
    details: The linked figures were measured at commit 8f04e288 for OxiDex 1.2.1. They are historical context, explicitly not v2.0.0-beta.1 performance evidence.
    link: /performance/
    linkText: The measurements
  - icon: ✍️
    title: Careful writes
    details: Atomic writes for JPEG EXIF, TIFF and TIFF-based RAW, PNG and PDF. 19 tags are proven byte-for-byte against ExifTool's own writes; the rest is labelled as not yet proven.
    link: /guide/writing
    linkText: Write support
  - icon: 🦀
    title: Library, C API and CLI
    details: An ExifTool-style command line, a Rust library, and a C API with a generated header. A separate MCP server (oxidex-mcp) exposes it to AI assistants.
    link: /guide/library-api
    linkText: Library guide
---

::: warning v2.0.0-beta.1: a pre-release
This site documents the pre-tag 2.0 development line on
`refactor/tag-machinery`; it is not a published beta release. The final
reviewed `main` SHA, signed tag, release date, assets, and release receipts are
pending. Output and API may still change before 2.0.0.
2.0 changes tag keys, value formatting and parts of the API compared with
1.x. Read [Migrating from 1.x to 2.0](/guide/migrating-from-1x) before
upgrading. The previous stable release is
[v1.2.1](https://github.com/swack-tools/oxidex/releases/tag/v1.2.1)
([its docs](https://github.com/swack-tools/oxidex/tree/v1.2.1/docs)).
:::

## Quick example

```bash
# Everything ExifTool would print for this file
oxidex photo.jpg

# Selected tags
oxidex -Make -Model -DateTimeOriginal photo.jpg

# JSON with every occurrence and its family-1 group, as `exiftool -j -a -G1`
oxidex -j -a -G1 photo.jpg

# Write a tag (atomic, in place; add --backup to keep a .bak copy)
oxidex -EXIF:Artist="Jane Doe" photo.jpg

# A whole directory, read in parallel
oxidex -r -j /path/to/photos/
```

## What OxiDex is today

- **A reader first.** Development-state source inspection maps 131 formats in
  detection and 129 to a parser, including camera RAW with 36 RAW sub-formats.
  This is not a v2.0.0-beta.1 parity receipt. Many more file types are
  *identified*, from ExifTool's own type tables, but not parsed.
  [Supported formats](/reference/formats/) lists which is which.
- **Parity is measured, not claimed.** Every number on this site names the
  instrument and commit that produced it. The pinned ExifTool is always
  checked for both its version and its capabilities first.
  [ExifTool parity](/guide/exiftool-parity) explains the terms.
- **Writing is narrower.** JPEG EXIF, TIFF and TIFF-based RAW, PNG and PDF
  can be written. What is proven, and what is only implemented, is on
  [Writing metadata](/guide/writing).
- **Moving toward full generation.** The goal is that a new ExifTool release
  flows in by regeneration, with no one retyping a tag rule. The
  [v2 design](/AUTOGENERATION-V2-DESIGN) and the
  [plan](/AUTOGENERATION-PLAN) describe how, and the [status page](/status/)
  shows how far along it is.

## Where to go next

- [Install OxiDex](/guide/getting-started)
- [Use the command line](/guide/cli-usage) or [the Rust library](/guide/library-api)
- [How the architecture works](/architecture/)
- [How to contribute](/contributing/)
