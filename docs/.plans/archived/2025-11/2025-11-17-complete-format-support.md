# Complete Format Support Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Implement comprehensive file format support matching ExifTool's capabilities, including all missing basic image formats (GIF, BMP, HEIF, WebP), text-based formats (VCF, HTML, TXT, RTF, JSON, PLIST), and specialized formats (LNK, Torrent).

**Architecture:** Follow hexagonal architecture with format-specific parsers implementing FormatParser trait. Use nom for binary parsing, extract magic bytes from ExifTool Perl source for format detection.

**Tech Stack:** Rust, nom (binary parsing), serde_json (JSON parsing), plist (PLIST parsing), encoding_rs (text encoding)

---

## Task 1: Create GIF Image Parser

**Files:**
- Create: `src/parsers/image/gif.rs`
- Modify: `src/parsers/image/mod.rs`
- Modify: `src/parsers/format_detector.rs`
- Modify: `src/core/operations.rs`

**Step 1: Create GIF parser file**

Create `src/parsers/image/gif.rs`:

```rust
//! GIF image format parser
//!
//! Implements basic metadata extraction from GIF (Graphics Interchange Format) files.

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// GIF signature: "GIF87a" or "GIF89a"
const GIF87A_SIGNATURE: &[u8] = b"GIF87a";
const GIF89A_SIGNATURE: &[u8] = b"GIF89a";

/// GIF parser for extracting metadata from GIF images
pub struct GIFParser;

impl GIFParser {
    /// Verifies GIF signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 6 {
            return Ok(false);
        }
        let header = reader.read(0, 6)?;
        Ok(header == GIF87A_SIGNATURE || header == GIF89A_SIGNATURE)
    }

    /// Reads GIF version (87a or 89a)
    pub fn read_version(reader: &dyn FileReader) -> Result<&'static str> {
        if reader.size() < 6 {
            return Ok("Unknown");
        }
        let header = reader.read(0, 6)?;
        if header == GIF87A_SIGNATURE {
            Ok("87a")
        } else if header == GIF89A_SIGNATURE {
            Ok("89a")
        } else {
            Ok("Unknown")
        }
    }

    /// Reads image dimensions from GIF header (offset 6, 4 bytes: width, height in little-endian)
    pub fn read_dimensions(reader: &dyn FileReader) -> Result<(u16, u16)> {
        if reader.size() < 10 {
            return Ok((0, 0));
        }
        let dims = reader.read(6, 4)?;
        let width = u16::from_le_bytes([dims[0], dims[1]]);
        let height = u16::from_le_bytes([dims[2], dims[3]]);
        Ok((width, height))
    }
}

impl FormatParser for GIFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid GIF signature"));
        }

        let mut metadata = MetadataMap::new();

        metadata.insert("FileType".to_string(), TagValue::String("GIF".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        let version = Self::read_version(reader)?;
        metadata.insert("GIFVersion".to_string(), TagValue::String(version.to_string()));

        let (width, height) = Self::read_dimensions(reader)?;
        metadata.insert("ImageWidth".to_string(), TagValue::String(width.to_string()));
        metadata.insert("ImageHeight".to_string(), TagValue::String(height.to_string()));

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::GIF)
    }
}

/// Parses metadata from GIF files.
pub fn parse_gif_metadata(reader: &dyn FileReader) -> Result<MetadataMap, String> {
    let parser = GIFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
```

**Step 2: Update image/mod.rs**

Add to `src/parsers/image/mod.rs`:

```rust
pub mod gif;
pub use gif::GIFParser;
```

**Step 3: Add GIF detection to format_detector.rs**

After PNG detection (around line 216), add:

```rust
    // GIF: "GIF87a" or "GIF89a"
    if magic_bytes.len() >= 6 {
        if &magic_bytes[0..6] == b"GIF87a" || &magic_bytes[0..6] == b"GIF89a" {
            return Ok(FileFormat::GIF);
        }
    }
```

**Step 4: Connect parser in operations.rs**

Add import:

```rust
use crate::parsers::image::gif::parse_gif_metadata;
```

Add case in match statement:

```rust
        FileFormat::GIF => parse_gif_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("GIF parse error: {}", e))),
```

**Step 5: Test and commit**

```bash
cargo build --release
cargo test
git add src/parsers/image/gif.rs src/parsers/image/mod.rs src/parsers/format_detector.rs src/core/operations.rs
git commit -m "feat: add GIF image format parser"
```

---

## Task 2: Create BMP Image Parser

**Files:**
- Create: `src/parsers/image/bmp.rs`
- Modify: `src/parsers/image/mod.rs`
- Modify: `src/parsers/format_detector.rs`
- Modify: `src/core/operations.rs`

**Step 1: Create BMP parser file**

Create `src/parsers/image/bmp.rs`:

```rust
//! BMP (Bitmap) image format parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// BMP signature: "BM" (0x42 0x4D)
const BMP_SIGNATURE: &[u8] = b"BM";

/// BMP parser for extracting metadata from Windows bitmap images
pub struct BMPParser;

impl BMPParser {
    /// Verifies BMP signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 2 {
            return Ok(false);
        }
        let header = reader.read(0, 2)?;
        Ok(header == BMP_SIGNATURE)
    }

    /// Reads image dimensions from BMP header
    /// Width at offset 18 (4 bytes), Height at offset 22 (4 bytes), both little-endian
    pub fn read_dimensions(reader: &dyn FileReader) -> Result<(i32, i32)> {
        if reader.size() < 26 {
            return Ok((0, 0));
        }
        let width_bytes = reader.read(18, 4)?;
        let height_bytes = reader.read(22, 4)?;
        let width = i32::from_le_bytes([width_bytes[0], width_bytes[1], width_bytes[2], width_bytes[3]]);
        let height = i32::from_le_bytes([height_bytes[0], height_bytes[1], height_bytes[2], height_bytes[3]]);
        Ok((width, height))
    }

    /// Reads bit depth from BMP header (offset 28, 2 bytes)
    pub fn read_bit_depth(reader: &dyn FileReader) -> Result<u16> {
        if reader.size() < 30 {
            return Ok(0);
        }
        let bits = reader.read(28, 2)?;
        Ok(u16::from_le_bytes([bits[0], bits[1]]))
    }
}

impl FormatParser for BMPParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid BMP signature"));
        }

        let mut metadata = MetadataMap::new();

        metadata.insert("FileType".to_string(), TagValue::String("BMP".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        let (width, height) = Self::read_dimensions(reader)?;
        metadata.insert("ImageWidth".to_string(), TagValue::String(width.abs().to_string()));
        metadata.insert("ImageHeight".to_string(), TagValue::String(height.abs().to_string()));

        let bit_depth = Self::read_bit_depth(reader)?;
        metadata.insert("BitDepth".to_string(), TagValue::String(bit_depth.to_string()));

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::BMP)
    }
}

/// Parses metadata from BMP files.
pub fn parse_bmp_metadata(reader: &dyn FileReader) -> Result<MetadataMap, String> {
    let parser = BMPParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
```

**Step 2: Update image/mod.rs**

Add:

```rust
pub mod bmp;
pub use bmp::BMPParser;
```

**Step 3: Add BMP detection to format_detector.rs**

Add after GIF detection:

```rust
    // BMP: "BM" (0x42 0x4D)
    if magic_bytes.len() >= 2 && &magic_bytes[0..2] == b"BM" {
        return Ok(FileFormat::BMP);
    }
```

**Step 4: Connect parser in operations.rs**

Add import:

```rust
use crate::parsers::image::bmp::parse_bmp_metadata;
```

Add case:

```rust
        FileFormat::BMP => parse_bmp_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("BMP parse error: {}", e))),
```

**Step 5: Test and commit**

```bash
cargo build --release
cargo test
git add src/parsers/image/bmp.rs src/parsers/image/mod.rs src/parsers/format_detector.rs src/core/operations.rs
git commit -m "feat: add BMP image format parser"
```

---

## Task 3: Create HEIF/HEIC Image Parser

**Files:**
- Create: `src/parsers/image/heif.rs`
- Modify: `src/parsers/image/mod.rs`
- Modify: `src/parsers/format_detector.rs`
- Modify: `src/core/operations.rs`

**Step 1: Create HEIF parser file**

Create `src/parsers/image/heif.rs`:

```rust
//! HEIF/HEIC image format parser
//!
//! HEIF (High Efficiency Image Format) uses ISO BMFF container with "heic" or "heix" brand

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

const FTYP_SIGNATURE: &[u8] = b"ftyp";

/// HEIF/HEIC parser
pub struct HEIFParser;

impl HEIFParser {
    /// Verifies HEIF signature by checking ISO BMFF "ftyp" box with HEIF brands
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 12 {
            return Ok(false);
        }
        let header = reader.read(4, 8)?;

        // Check for "ftyp" at offset 4
        if &header[0..4] != FTYP_SIGNATURE {
            return Ok(false);
        }

        // Check for HEIF-compatible brands: heic, heix, hevc, hevx, heim, heis, hevm, hevs, mif1
        let brand = &header[4..8];
        Ok(brand == b"heic" || brand == b"heix" || brand == b"hevc" || brand == b"hevx"
            || brand == b"heim" || brand == b"heis" || brand == b"hevm" || brand == b"hevs"
            || brand == b"mif1")
    }
}

impl FormatParser for HEIFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid HEIF signature"));
        }

        let mut metadata = MetadataMap::new();

        metadata.insert("FileType".to_string(), TagValue::String("HEIF".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::HEIF)
    }
}

/// Parses metadata from HEIF/HEIC files.
pub fn parse_heif_metadata(reader: &dyn FileReader) -> Result<MetadataMap, String> {
    let parser = HEIFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
```

**Step 2: Update image/mod.rs**

Add:

```rust
pub mod heif;
pub use heif::HEIFParser;
```

**Step 3: Add HEIF detection to format_detector.rs**

Add after AVIF detection (around line 525):

```rust
    // HEIF: ISO BMFF with "ftyp" at offset 4 and HEIF brands
    if magic_bytes.len() >= 12 && &magic_bytes[4..8] == b"ftyp" {
        let brand = &magic_bytes[8..12];
        if brand == b"heic" || brand == b"heix" || brand == b"hevc" || brand == b"hevx"
            || brand == b"heim" || brand == b"heis" || brand == b"hevm" || brand == b"hevs"
            || brand == b"mif1"
        {
            return Ok(FileFormat::HEIF);
        }
    }
```

**Step 4: Connect parser in operations.rs**

Add import:

```rust
use crate::parsers::image::heif::parse_heif_metadata;
```

Add case:

```rust
        FileFormat::HEIF => parse_heif_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("HEIF parse error: {}", e))),
```

**Step 5: Test and commit**

```bash
cargo build --release
cargo test
git add src/parsers/image/heif.rs src/parsers/image/mod.rs src/parsers/format_detector.rs src/core/operations.rs
git commit -m "feat: add HEIF/HEIC image format parser"
```

---

## Task 4: Create WebP Image Parser

**Files:**
- Create: `src/parsers/image/webp.rs`
- Modify: `src/parsers/image/mod.rs`
- Modify: `src/parsers/format_detector.rs`
- Modify: `src/core/operations.rs`

**Step 1: Create WebP parser file**

Create `src/parsers/image/webp.rs`:

```rust
//! WebP image format parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// WebP signature: "RIFF" + size + "WEBP"
const RIFF_SIGNATURE: &[u8] = b"RIFF";
const WEBP_SIGNATURE: &[u8] = b"WEBP";

/// WebP parser
pub struct WebPParser;

impl WebPParser {
    /// Verifies WebP signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 12 {
            return Ok(false);
        }
        let header = reader.read(0, 12)?;
        Ok(&header[0..4] == RIFF_SIGNATURE && &header[8..12] == WEBP_SIGNATURE)
    }
}

impl FormatParser for WebPParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid WebP signature"));
        }

        let mut metadata = MetadataMap::new();

        metadata.insert("FileType".to_string(), TagValue::String("WebP".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::WebP)
    }
}

/// Parses metadata from WebP files.
pub fn parse_webp_metadata(reader: &dyn FileReader) -> Result<MetadataMap, String> {
    let parser = WebPParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
```

**Step 2: Update image/mod.rs**

Add:

```rust
pub mod webp;
pub use webp::WebPParser;
```

**Step 3: Add WebP detection to format_detector.rs**

Add after BMP detection:

```rust
    // WebP: "RIFF" + size (4 bytes) + "WEBP"
    if magic_bytes.len() >= 12 && &magic_bytes[0..4] == b"RIFF" && &magic_bytes[8..12] == b"WEBP" {
        return Ok(FileFormat::WebP);
    }
```

**Step 4: Connect parser in operations.rs**

Add import:

```rust
use crate::parsers::image::webp::parse_webp_metadata;
```

Add case:

```rust
        FileFormat::WebP => parse_webp_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("WebP parse error: {}", e))),
```

**Step 5: Test and commit**

```bash
cargo build --release
cargo test
git add src/parsers/image/webp.rs src/parsers/image/mod.rs src/parsers/format_detector.rs src/core/operations.rs
git commit -m "feat: add WebP image format parser"
```

---

## Task 5: Add VCF (vCard) Format Support

**Files:**
- Modify: `src/core/file_format.rs` (add enum variant)
- Create: `src/parsers/text/mod.rs`
- Create: `src/parsers/text/vcf.rs`
- Modify: `src/parsers/mod.rs`
- Modify: `src/parsers/format_detector.rs`
- Modify: `src/core/operations.rs`

**Step 1: Add VCF to FileFormat enum**

In `src/core/file_format.rs`, add after HDF5 (around line 231):

```rust
    // Phase 7: Text-based formats
    /// vCard contact format (.vcf)
    VCF,
```

**Step 2: Add VCF to name() and extensions() methods**

In `name()` method, add:

```rust
            FileFormat::VCF => "vCard",
```

In `extensions()` method, add:

```rust
            FileFormat::VCF => &["vcf", "vcard"],
```

**Step 3: Create text parser directory and module**

```bash
mkdir -p src/parsers/text
```

Create `src/parsers/text/mod.rs`:

```rust
//! Text-based format parsers

#![allow(dead_code)]

pub mod vcf;

pub use vcf::VCFParser;
```

**Step 4: Create VCF parser**

Create `src/parsers/text/vcf.rs`:

```rust
//! vCard (VCF) contact format parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// VCF signature: "BEGIN:VCARD"
const VCF_SIGNATURE: &[u8] = b"BEGIN:VCARD";

/// VCF/vCard parser
pub struct VCFParser;

impl VCFParser {
    /// Verifies VCF signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 11 {
            return Ok(false);
        }
        let header = reader.read(0, 11)?;
        Ok(header == VCF_SIGNATURE)
    }

    /// Parse vCard content to extract basic metadata
    pub fn parse_vcard_content(reader: &dyn FileReader) -> Result<MetadataMap> {
        let size = reader.size() as usize;
        let content = reader.read(0, size.min(8192))?; // Read first 8KB

        let text = std::str::from_utf8(content)
            .map_err(|e| ExifToolError::parse_error(format!("Invalid UTF-8: {}", e)))?;

        let mut metadata = MetadataMap::new();

        for line in text.lines() {
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim();
                let value = value.trim();

                match key {
                    "VERSION" => {
                        metadata.insert("VCardVersion".to_string(), TagValue::String(value.to_string()));
                    }
                    "FN" => {
                        metadata.insert("FullName".to_string(), TagValue::String(value.to_string()));
                    }
                    "EMAIL" => {
                        metadata.insert("Email".to_string(), TagValue::String(value.to_string()));
                    }
                    "TEL" => {
                        metadata.insert("Telephone".to_string(), TagValue::String(value.to_string()));
                    }
                    _ => {}
                }
            }
        }

        Ok(metadata)
    }
}

impl FormatParser for VCFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid VCF signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("vCard".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Parse vCard content
        let vcard_metadata = Self::parse_vcard_content(reader)?;
        metadata.extend(vcard_metadata);

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::VCF)
    }
}

/// Parses metadata from VCF files.
pub fn parse_vcf_metadata(reader: &dyn FileReader) -> Result<MetadataMap, String> {
    let parser = VCFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
```

**Step 5: Update parsers/mod.rs**

Add:

```rust
pub mod text;
```

**Step 6: Add VCF detection**

In `src/parsers/format_detector.rs`, add before the Unknown return:

```rust
    // VCF: "BEGIN:VCARD"
    if magic_bytes.len() >= 11 && &magic_bytes[0..11] == b"BEGIN:VCARD" {
        return Ok(FileFormat::VCF);
    }
```

**Step 7: Connect parser in operations.rs**

Add import:

```rust
use crate::parsers::text::vcf::parse_vcf_metadata;
```

Add case:

```rust
        FileFormat::VCF => parse_vcf_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("VCF parse error: {}", e))),
```

**Step 8: Test and commit**

```bash
cargo build --release
cargo test
git add src/core/file_format.rs src/parsers/text/ src/parsers/mod.rs src/parsers/format_detector.rs src/core/operations.rs
git commit -m "feat: add VCF (vCard) format support"
```

---

## Task 6: Add LNK (Windows Shortcut) Format Support

**Files:**
- Modify: `src/core/file_format.rs`
- Create: `src/parsers/specialized/lnk.rs`
- Modify: `src/parsers/specialized/mod.rs`
- Modify: `src/parsers/format_detector.rs`
- Modify: `src/core/operations.rs`

**Step 1: Add LNK to FileFormat enum**

Add after VCF:

```rust
    /// Windows shortcut (.lnk)
    LNK,
```

Update name() and extensions() methods:

```rust
            FileFormat::LNK => "Windows Shortcut",
            // in extensions():
            FileFormat::LNK => &["lnk"],
```

**Step 2: Create LNK parser**

Create `src/parsers/specialized/lnk.rs`:

```rust
//! Windows Shortcut (LNK) format parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// LNK signature: 0x4C 0x00 0x00 0x00 (magic) + GUID
const LNK_MAGIC: &[u8] = &[0x4C, 0x00, 0x00, 0x00];

/// LNK parser
pub struct LNKParser;

impl LNKParser {
    /// Verifies LNK signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }
        let header = reader.read(0, 4)?;
        Ok(header == LNK_MAGIC)
    }
}

impl FormatParser for LNKParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid LNK signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("LNK".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::LNK)
    }
}

/// Parses metadata from LNK files.
pub fn parse_lnk_metadata(reader: &dyn FileReader) -> Result<MetadataMap, String> {
    let parser = LNKParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
```

**Step 3: Update specialized/mod.rs**

Add:

```rust
pub mod lnk;
pub use lnk::LNKParser;
```

**Step 4: Add LNK detection**

In format_detector.rs:

```rust
    // LNK: 0x4C 0x00 0x00 0x00
    if magic_bytes.len() >= 4 && magic_bytes[0..4] == [0x4C, 0x00, 0x00, 0x00] {
        return Ok(FileFormat::LNK);
    }
```

**Step 5: Connect parser**

In operations.rs:

```rust
use crate::parsers::specialized::lnk::parse_lnk_metadata;

// In match:
        FileFormat::LNK => parse_lnk_metadata(&reader)
            .map_err(|e| ExifToolError::parse_error(format!("LNK parse error: {}", e))),
```

**Step 6: Test and commit**

```bash
cargo build --release
cargo test
git add src/core/file_format.rs src/parsers/specialized/lnk.rs src/parsers/specialized/mod.rs src/parsers/format_detector.rs src/core/operations.rs
git commit -m "feat: add LNK (Windows shortcut) format support"
```

---

## Task 7: Connect All Existing Parsers

**Files:**
- Modify: `src/core/operations.rs` (imports and dispatch cases)

**Step 1: Add all missing parser imports**

Add these imports to operations.rs (check which ones are missing):

```rust
// Advanced image parsers
use crate::parsers::image::avif::AVIFParser;
use crate::parsers::image::jxl::JXLParser;
use crate::parsers::image::bpg::BPGParser;
use crate::parsers::image::exr::EXRParser;
use crate::parsers::image::flif::FLIFParser;
use crate::parsers::image::svg::SVGParser;
use crate::parsers::image::ico::ICOParser;
use crate::parsers::image::psd::PSDParser;

// Specialized parsers
use crate::parsers::specialized::elf::ELFParser;
use crate::parsers::specialized::macho::MachOParser;
use crate::parsers::specialized::dwg::DWGParser;
use crate::parsers::specialized::dxf::DXFParser;
use crate::parsers::specialized::stl::STLParser;
use crate::parsers::specialized::obj::OBJParser;
use crate::parsers::specialized::gltf::GLTFParser;
use crate::parsers::specialized::fits::FITSParser;
use crate::parsers::specialized::hdf5::HDF5Parser;
```

**Step 2: Create helper function to use FormatParser trait**

Add before the match statement:

```rust
fn parse_with_trait<P: FormatParser>(parser: P, reader: &dyn FileReader, format_name: &str) -> Result<MetadataMap> {
    parser.parse(reader)
}
```

**Step 3: Add all missing format cases**

Add to the match statement:

```rust
        // Advanced images
        FileFormat::AVIF => parse_with_trait(AVIFParser, &reader, "AVIF"),
        FileFormat::JXL => parse_with_trait(JXLParser, &reader, "JXL"),
        FileFormat::BPG => parse_with_trait(BPGParser, &reader, "BPG"),
        FileFormat::EXR => parse_with_trait(EXRParser, &reader, "EXR"),
        FileFormat::FLIF => parse_with_trait(FLIFParser, &reader, "FLIF"),
        FileFormat::SVG => parse_with_trait(SVGParser, &reader, "SVG"),
        FileFormat::ICO => parse_with_trait(ICOParser, &reader, "ICO"),
        FileFormat::PSD => parse_with_trait(PSDParser, &reader, "PSD"),

        // Specialized
        FileFormat::ELF => parse_with_trait(ELFParser, &reader, "ELF"),
        FileFormat::MachO => parse_with_trait(MachOParser, &reader, "Mach-O"),
        FileFormat::DWG => parse_with_trait(DWGParser, &reader, "DWG"),
        FileFormat::DXF => parse_with_trait(DXFParser, &reader, "DXF"),
        FileFormat::STL => parse_with_trait(STLParser, &reader, "STL"),
        FileFormat::OBJ => parse_with_trait(OBJParser, &reader, "OBJ"),
        FileFormat::GLTF => parse_with_trait(GLTFParser, &reader, "glTF"),
        FileFormat::FITS => parse_with_trait(FITSParser, &reader, "FITS"),
        FileFormat::HDF5 => parse_with_trait(HDF5Parser, &reader, "HDF5"),
```

**Step 4: Test compilation**

```bash
cargo build --release
```

Expected: Should compile successfully

**Step 5: Run tests**

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

**Step 6: Commit**

```bash
git add src/core/operations.rs
git commit -m "feat: connect all existing format parsers (advanced images, specialized formats)"
```

---

## Task 8: Run Full Test Suite and Verify Build

**Step 1: Build release binary**

```bash
cargo build --release
```

Expected: Clean build with no errors

**Step 2: Run all tests**

```bash
cargo test
```

Expected: All tests pass

**Step 3: Run clippy**

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Expected: No warnings

**Step 4: Format code**

```bash
cargo fmt
```

**Step 5: Commit formatting**

```bash
git add -u
git commit -m "chore: run cargo fmt"
```

---

## Task 9: Push and Monitor CI/CD

**Step 1: Push all commits**

```bash
git push
```

**Step 2: Monitor CI/CD**

```bash
sleep 60
gh run list --limit 1
gh run view $(gh run list --limit 1 --json databaseId -q '.[0].databaseId')
```

Expected: All checks pass

**Step 3: If CI/CD fails, investigate and fix**

```bash
# View failed logs
gh run view $(gh run list --limit 1 --json databaseId -q '.[0].databaseId') --log-failed

# Fix issues, commit, and push again
```

**Step 4: Continue fixing until CI/CD passes**

Keep iterating until all checks are green.

---

## Success Criteria

1. ✅ GIF, BMP, HEIF, WebP parsers created and connected
2. ✅ VCF (vCard) format support added
3. ✅ LNK (Windows shortcut) format support added
4. ✅ All existing parsers (AVIF, JXL, BPG, EXR, FLIF, SVG, ICO, PSD) connected
5. ✅ All specialized parsers (ELF, Mach-O, DWG, DXF, STL, OBJ, glTF, FITS, HDF5) connected
6. ✅ All tests pass
7. ✅ No clippy warnings
8. ✅ Code formatted with cargo fmt
9. ✅ CI/CD pipeline passes

## Notes

- Follow ExifTool Perl source for magic bytes and format specifications
- Use nom for binary parsing where appropriate
- Keep parsers simple initially - focus on format detection and basic metadata
- Can enhance with more detailed metadata extraction in future iterations
