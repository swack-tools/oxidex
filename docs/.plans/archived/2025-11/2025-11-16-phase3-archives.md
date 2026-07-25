# Phase 3: Archive Formats Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add metadata extraction for archive formats (RAR, 7z, ISO, TAR, GZ)

**Note:** ZIP parser already implemented in Phase 2.

**Architecture:** Binary archive parsers, extract archive metadata (file counts, compression ratios, dates).

**Tech Stack:** Rust, existing crates for each format (tar, flate2, etc.)

**Timeline:** 1-2 months

---

## Parser List

1. **RAR** - WinRAR archive (.rar) - Magic: `52 61 72 21` ("Rar!")
2. **7z** - 7-Zip archive (.7z) - Magic: `37 7A BC AF` ("7z")
3. **ISO** - ISO 9660 (.iso) - Magic: `43 44 30 30 31` at offset 32769
4. **TAR** - Tape archive (.tar) - Magic: `75 73 74 61 72` at offset 257 ("ustar")
5. **GZ** - Gzip (.gz) - Magic: `1F 8B`

Each parser extracts:
- File count
- File list
- Compression ratio
- Archive creation date

**Success Criteria:**
- [ ] 5 parsers implemented
- [ ] Tests passing
- [ ] ExifTool parity
- [ ] Benchmarks <20ms

**Est. Tasks:** ~25-30
