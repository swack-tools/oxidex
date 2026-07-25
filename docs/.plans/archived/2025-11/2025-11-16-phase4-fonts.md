# Phase 4: Font Files Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add metadata extraction for font formats (TTF, OTF, WOFF, WOFF2)

**Architecture:** Binary font table parsing, extract name tables and font metrics.

**Tech Stack:** Rust, nom (binary parsing), ttf-parser crate (optional)

**Timeline:** 1 month

---

## Parser List

1. **TTF** - TrueType Font (.ttf) - Magic: `00 01 00 00` or `74 72 75 65` ("true")
2. **OTF** - OpenType Font (.otf) - Magic: `4F 54 54 4F` ("OTTO")
3. **WOFF** - Web Open Font Format (.woff) - Magic: `77 4F 46 46` ("wOFF")
4. **WOFF2** - WOFF2 (.woff2) - Magic: `77 4F 46 32` ("wOF2")

Each parser extracts:
- Font family name
- Font subfamily (Regular, Bold, etc.)
- Version
- Copyright
- Designer
- Glyph count

**Reference:** ExifTool `lib/Image/ExifTool/Font.pm`

**Success Criteria:**
- [ ] 4 parsers implemented
- [ ] Tests passing
- [ ] ExifTool parity

**Est. Tasks:** ~20 tasks
