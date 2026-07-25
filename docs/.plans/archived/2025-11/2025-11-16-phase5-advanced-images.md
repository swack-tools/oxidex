# Phase 5: Advanced Image Formats Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add metadata extraction for next-gen and professional image formats (AVIF, JXL, BPG, EXR, FLIF, SVG, ICO, expanded PSD)

**Architecture:** Modern image format parsers with HDR/color space metadata support.

**Tech Stack:** Rust, image-rs ecosystem, nom (binary parsing), quick-xml (SVG)

**Timeline:** 2-3 months

---

## Parser List

1. **AVIF** - AV1 Image Format (.avif) - Based on ISO BMFF
2. **JXL** - JPEG XL (.jxl) - Magic: `FF 0A` or `00 00 00 0C 4A 58 4C 20`
3. **BPG** - Better Portable Graphics (.bpg) - Magic: `42 50 47 FB`
4. **EXR** - OpenEXR (.exr) - Magic: `76 2F 31 01`
5. **FLIF** - Free Lossless Image Format (.flif) - Magic: `46 4C 49 46`
6. **SVG** - Scalable Vector Graphics (.svg) - XML-based
7. **ICO** - Windows Icon (.ico) - Magic: `00 00 01 00`
8. **PSD** - Photoshop (expanded) (.psd) - Extend existing parser

Focus on:
- HDR metadata
- Color space information
- Layer count (PSD)
- Animation frames (if applicable)

**Success Criteria:**
- [ ] 8 parsers implemented
- [ ] HDR metadata extracted
- [ ] Tests passing

**Est. Tasks:** ~35-40 tasks
