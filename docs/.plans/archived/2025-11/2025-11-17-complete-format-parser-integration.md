# Complete Format Parser Integration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Connect all existing format parsers (video, audio, document, archive, font, advanced image, specialized) to the operations.rs dispatcher to match ExifTool's metadata extraction capabilities.

**Architecture:** The codebase already has parsers implemented in src/parsers/* for Phases 1-6 formats. These parsers need to be imported and connected in src/core/operations.rs to handle metadata extraction. Each format follows the pattern: detect format → dispatch to parser → extract metadata → return MetadataMap.

**Tech Stack:** Rust, existing parser modules (video/, audio/, document/, archive/, font/, image/, specialized/)

---

## Task 1: Connect Video Format Parsers (Phase 1)

**Files:**
- Modify: `src/core/operations.rs:1-20` (add imports)
- Modify: `src/core/operations.rs:109-136` (add parser dispatch cases)
- Test: Run batch processing on ../examples/

**Step 1: Add video parser imports**

Add after line 16 in `src/core/operations.rs`:

```rust
use crate::parsers::video::mkv::parse_mkv_metadata;
use crate::parsers::video::webm::parse_webm_metadata;
use crate::parsers::video::flv::parse_flv_metadata;
use crate::parsers::video::avi::parse_avi_metadata;
use crate::parsers::video::mts::parse_mts_metadata;
```

**Step 2: Add video format parser dispatch cases**

In the match statement at line 109, add before the `_` wildcard case:

```rust
        FileFormat::MKV => parse_mkv_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("MKV parse error: {}", e))),
        FileFormat::WEBM => parse_webm_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("WebM parse error: {}", e))),
        FileFormat::FLV => parse_flv_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("FLV parse error: {}", e))),
        FileFormat::AVI => parse_avi_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("AVI parse error: {}", e))),
        FileFormat::MTS => parse_mts_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("MTS parse error: {}", e))),
```

**Step 3: Test video format parsing**

Run: `cargo build --release && just run -r ../examples/ 2>&1 | grep -E "(mkv|webm|flv|avi|mts|MKV|WEBM|FLV|AVI|MTS)" | head -20`

Expected: Files with these extensions should now be parsed successfully instead of showing "Unknown format"

**Step 4: Compare with ExifTool output**

Run: `find ../examples -name "*.mkv" -o -name "*.webm" -o -name "*.flv" -o -name "*.avi" -o -name "*.mts" | head -3 | xargs -I {} sh -c 'echo "=== {} ==="; exiftool "{}"; ./target/release/oxidex "{}"' | head -100`

Expected: OxiDex output should match ExifTool's extracted metadata tags

**Step 5: Commit**

```bash
git add src/core/operations.rs
git commit -m "feat: connect video format parsers (MKV, WebM, FLV, AVI, MTS)"
```

---

## Task 2: Connect Audio Format Parsers (Phase 1 continued)

**Files:**
- Modify: `src/core/operations.rs:1-20` (add imports)
- Modify: `src/core/operations.rs:109-136` (add parser dispatch cases)
- Test: Run batch processing on ../examples/

**Step 1: Add audio parser imports**

Add after the video parser imports in `src/core/operations.rs`:

```rust
use crate::parsers::audio::mp3::parse_mp3_metadata;
use crate::parsers::audio::flac::parse_flac_metadata;
use crate::parsers::audio::aac::parse_aac_metadata;
use crate::parsers::audio::wav::parse_wav_metadata;
use crate::parsers::audio::ogg::parse_ogg_metadata;
use crate::parsers::audio::opus::parse_opus_metadata;
use crate::parsers::audio::ape::parse_ape_metadata;
```

**Step 2: Add audio format parser dispatch cases**

In the match statement, add after the video cases:

```rust
        FileFormat::MP3 => parse_mp3_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("MP3 parse error: {}", e))),
        FileFormat::FLAC => parse_flac_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("FLAC parse error: {}", e))),
        FileFormat::AAC => parse_aac_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("AAC parse error: {}", e))),
        FileFormat::WAV => parse_wav_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("WAV parse error: {}", e))),
        FileFormat::OGG => parse_ogg_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("OGG parse error: {}", e))),
        FileFormat::OPUS => parse_opus_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("Opus parse error: {}", e))),
        FileFormat::APE => parse_ape_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("APE parse error: {}", e))),
```

**Step 3: Test audio format parsing**

Run: `cargo build --release && just run -r ../examples/ 2>&1 | grep -E "(mp3|flac|aac|wav|ogg|opus|ape|MP3|FLAC|AAC|WAV|OGG|OPUS|APE)" | head -20`

Expected: Audio files should now be parsed successfully

**Step 4: Compare with ExifTool**

Run: `find ../examples -name "*.mp3" -o -name "*.flac" -o -name "*.aac" | head -3 | xargs -I {} sh -c 'echo "=== {} ==="; exiftool "{}"; ./target/release/oxidex "{}"' | head -100`

Expected: Metadata tags should match ExifTool's output

**Step 5: Commit**

```bash
git add src/core/operations.rs
git commit -m "feat: connect audio format parsers (MP3, FLAC, AAC, WAV, OGG, Opus, APE)"
```

---

## Task 3: Connect Document Format Parsers (Phase 2)

**Files:**
- Modify: `src/core/operations.rs:1-20` (add imports)
- Modify: `src/core/operations.rs:109-136` (add parser dispatch cases)
- Test: Run batch processing on ../examples/

**Step 1: Add document parser imports**

Add after the audio parser imports:

```rust
use crate::parsers::document::zip::parse_zip_metadata;
use crate::parsers::document::docx::parse_docx_metadata;
use crate::parsers::document::xlsx::parse_xlsx_metadata;
use crate::parsers::document::pptx::parse_pptx_metadata;
use crate::parsers::document::epub::parse_epub_metadata;
```

**Step 2: Add document format parser dispatch cases**

In the match statement, add after audio cases:

```rust
        FileFormat::ZIP => parse_zip_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("ZIP parse error: {}", e))),
        FileFormat::DOCX => parse_docx_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("DOCX parse error: {}", e))),
        FileFormat::XLSX => parse_xlsx_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("XLSX parse error: {}", e))),
        FileFormat::PPTX => parse_pptx_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("PPTX parse error: {}", e))),
        FileFormat::Pages => parse_docx_metadata(&reader) // Pages uses same format as DOCX
            .map_err(|e| ExifToolError::parse_error(format!("Pages parse error: {}", e))),
        FileFormat::Numbers => parse_xlsx_metadata(&reader) // Numbers uses same format as XLSX
            .map_err(|e| ExifToolError::parse_error(format!("Numbers parse error: {}", e))),
        FileFormat::Keynote => parse_pptx_metadata(&reader) // Keynote uses same format as PPTX
            .map_err(|e| ExifToolError::parse_error(format!("Keynote parse error: {}", e))),
        FileFormat::EPUB => parse_epub_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("EPUB parse error: {}", e))),
```

**Step 3: Test document format parsing**

Run: `cargo build --release && just run -r ../examples/ 2>&1 | grep -E "(docx|xlsx|pptx|epub|zip|DOCX|XLSX|PPTX|EPUB|ZIP)" | head -20`

Expected: Document files should be parsed successfully

**Step 4: Compare with ExifTool**

Run: `find ../examples -name "*.docx" -o -name "*.xlsx" -o -name "*.epub" | head -3 | xargs -I {} sh -c 'echo "=== {} ==="; exiftool "{}"; ./target/release/oxidex "{}"' | head -100`

Expected: Metadata should match ExifTool

**Step 5: Commit**

```bash
git add src/core/operations.rs
git commit -m "feat: connect document format parsers (DOCX, XLSX, PPTX, Pages, Numbers, Keynote, EPUB, ZIP)"
```

---

## Task 4: Connect Archive Format Parsers (Phase 3)

**Files:**
- Modify: `src/core/operations.rs:1-20` (add imports)
- Modify: `src/core/operations.rs:109-136` (add parser dispatch cases)

**Step 1: Add archive parser imports**

Add after document parser imports:

```rust
use crate::parsers::archive::rar::parse_rar_metadata;
use crate::parsers::archive::sevenz::parse_7z_metadata;
use crate::parsers::archive::iso::parse_iso_metadata;
use crate::parsers::archive::tar::parse_tar_metadata;
use crate::parsers::archive::gz::parse_gz_metadata;
```

**Step 2: Add archive format parser dispatch cases**

In the match statement:

```rust
        FileFormat::RAR => parse_rar_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("RAR parse error: {}", e))),
        FileFormat::SevenZ => parse_7z_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("7z parse error: {}", e))),
        FileFormat::ISO => parse_iso_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("ISO parse error: {}", e))),
        FileFormat::TAR => parse_tar_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("TAR parse error: {}", e))),
        FileFormat::GZ => parse_gz_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("GZ parse error: {}", e))),
```

**Step 3: Test and commit**

```bash
cargo build --release
find ../examples -name "*.rar" -o -name "*.7z" -o -name "*.iso" | head -3 | xargs -I {} sh -c 'exiftool "{}"; ./target/release/oxidex "{}"'
git add src/core/operations.rs
git commit -m "feat: connect archive format parsers (RAR, 7z, ISO, TAR, GZ)"
```

---

## Task 5: Connect Font Format Parsers (Phase 4)

**Files:**
- Modify: `src/core/operations.rs:1-20` (add imports)
- Modify: `src/core/operations.rs:109-136` (add parser dispatch cases)

**Step 1: Add font parser imports**

```rust
use crate::parsers::font::ttf::parse_ttf_metadata;
use crate::parsers::font::otf::parse_otf_metadata;
use crate::parsers::font::woff::parse_woff_metadata;
use crate::parsers::font::woff2::parse_woff2_metadata;
```

**Step 2: Add font format parser dispatch cases**

```rust
        FileFormat::TTF => parse_ttf_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("TTF parse error: {}", e))),
        FileFormat::OTF => parse_otf_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("OTF parse error: {}", e))),
        FileFormat::WOFF => parse_woff_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("WOFF parse error: {}", e))),
        FileFormat::WOFF2 => parse_woff2_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("WOFF2 parse error: {}", e))),
```

**Step 3: Test and commit**

```bash
cargo build --release
git add src/core/operations.rs
git commit -m "feat: connect font format parsers (TTF, OTF, WOFF, WOFF2)"
```

---

## Task 6: Connect Advanced Image Format Parsers (Phase 5)

**Files:**
- Modify: `src/core/operations.rs:1-20` (add imports)
- Modify: `src/core/operations.rs:109-136` (add parser dispatch cases)

**Step 1: Add advanced image parser imports**

```rust
use crate::parsers::image::avif::parse_avif_metadata;
use crate::parsers::image::jxl::parse_jxl_metadata;
use crate::parsers::image::bpg::parse_bpg_metadata;
use crate::parsers::image::exr::parse_exr_metadata;
use crate::parsers::image::flif::parse_flif_metadata;
use crate::parsers::image::svg::parse_svg_metadata;
use crate::parsers::image::ico::parse_ico_metadata;
use crate::parsers::image::psd::parse_psd_metadata;
```

**Step 2: Add image format parser dispatch cases**

```rust
        FileFormat::AVIF => parse_avif_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("AVIF parse error: {}", e))),
        FileFormat::JXL => parse_jxl_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("JXL parse error: {}", e))),
        FileFormat::BPG => parse_bpg_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("BPG parse error: {}", e))),
        FileFormat::EXR => parse_exr_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("EXR parse error: {}", e))),
        FileFormat::FLIF => parse_flif_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("FLIF parse error: {}", e))),
        FileFormat::SVG => parse_svg_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("SVG parse error: {}", e))),
        FileFormat::ICO => parse_ico_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("ICO parse error: {}", e))),
        FileFormat::PSD => parse_psd_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("PSD parse error: {}", e))),
```

**Step 3: Test and commit**

```bash
cargo build --release
git add src/core/operations.rs
git commit -m "feat: connect advanced image format parsers (AVIF, JXL, BPG, EXR, FLIF, SVG, ICO, PSD)"
```

---

## Task 7: Connect Specialized Format Parsers (Phase 6)

**Files:**
- Modify: `src/core/operations.rs:1-20` (add imports)
- Modify: `src/core/operations.rs:109-136` (add parser dispatch cases)

**Step 1: Add specialized parser imports**

```rust
use crate::parsers::specialized::elf::parse_elf_metadata;
use crate::parsers::specialized::macho::parse_macho_metadata;
use crate::parsers::specialized::dwg::parse_dwg_metadata;
use crate::parsers::specialized::dxf::parse_dxf_metadata;
use crate::parsers::specialized::stl::parse_stl_metadata;
use crate::parsers::specialized::obj::parse_obj_metadata;
use crate::parsers::specialized::gltf::parse_gltf_metadata;
use crate::parsers::specialized::fits::parse_fits_metadata;
use crate::parsers::specialized::hdf5::parse_hdf5_metadata;
```

**Step 2: Add specialized format parser dispatch cases**

```rust
        FileFormat::ELF => parse_elf_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("ELF parse error: {}", e))),
        FileFormat::MachO => parse_macho_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("Mach-O parse error: {}", e))),
        FileFormat::DWG => parse_dwg_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("DWG parse error: {}", e))),
        FileFormat::DXF => parse_dxf_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("DXF parse error: {}", e))),
        FileFormat::STL => parse_stl_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("STL parse error: {}", e))),
        FileFormat::OBJ => parse_obj_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("OBJ parse error: {}", e))),
        FileFormat::GLTF => parse_gltf_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("glTF parse error: {}", e))),
        FileFormat::FITS => parse_fits_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("FITS parse error: {}", e))),
        FileFormat::HDF5 => parse_hdf5_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("HDF5 parse error: {}", e))),
```

**Step 3: Test and commit**

```bash
cargo build --release
git add src/core/operations.rs
git commit -m "feat: connect specialized format parsers (ELF, Mach-O, DWG, DXF, STL, OBJ, glTF, FITS, HDF5)"
```

---

## Task 8: Run Full Test Suite and Verify CI/CD

**Files:**
- Verify: All tests pass
- Verify: CI/CD pipeline succeeds

**Step 1: Run full batch processing test**

```bash
cargo build --release
just run -r ../examples/ 2>&1 | tee test_results.log
```

Expected: Significantly fewer "Unknown format" errors

**Step 2: Run cargo fmt and clippy**

```bash
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: No warnings or errors

**Step 3: Run tests**

```bash
cargo test
```

Expected: All tests pass

**Step 4: Push and verify CI/CD**

```bash
git push
sleep 60
gh run list --limit 1
gh run view $(gh run list --limit 1 --json databaseId -q '.[0].databaseId') --log-failed
```

Expected: All CI/CD checks pass

**Step 5: If CI/CD fails, fix issues**

- Check error logs
- Fix compilation errors
- Adjust imports if modules don't exist
- Re-run tests
- Push fix

---

## Success Criteria

1. ✅ All Phase 1-6 format parsers connected to operations.rs
2. ✅ Batch processing shows reduced "Unknown format" errors
3. ✅ Metadata extraction matches ExifTool output for supported formats
4. ✅ All tests pass
5. ✅ Clippy shows no warnings
6. ✅ CI/CD pipeline succeeds
7. ✅ Code is formatted with cargo fmt

## Notes

- Some parsers may not exist yet - in that case, skip that format and note in commit message
- Apple formats (Pages, Numbers, Keynote) use ZIP-based formats similar to Office Open XML
- Focus on getting parsers connected first, then verify metadata accuracy
- If a parser doesn't exist, check if there's a stub module or if it needs to be created
