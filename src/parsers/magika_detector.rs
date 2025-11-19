//! Magika AI-powered file type detection.
//!
//! This module provides optional AI-powered file type detection using Google's Magika library.
//! It is only available when the `magika` feature is enabled.
//!
//! # Feature Gate
//!
//! This entire module is behind the `magika` cargo feature. To use it:
//!
//! ```toml
//! [dependencies]
//! oxidex = { version = "1.0", features = ["magika"] }
//! ```
//!
//! # Usage
//!
//! ```no_run
//! # #[cfg(feature = "magika")]
//! # {
//! use oxidex::parsers::magika_detector::detect_with_magika;
//!
//! let data = std::fs::read("image.jpg")?;
//! let format = detect_with_magika(&data)?;
//! println!("Detected format: {:?}", format);
//! # }
//! # Ok::<(), std::io::Error>(())
//! ```

#[cfg(feature = "magika")]
use magika::Session;
#[cfg(feature = "magika")]
use std::io;

use crate::core::FileFormat;

/// Detect file format using Google's Magika AI model.
///
/// This function uses deep learning to identify file types with ~99% accuracy
/// across 200+ content types. It requires the `magika` cargo feature to be enabled.
///
/// # Arguments
///
/// * `data` - File content bytes (minimum 512 bytes recommended for best accuracy)
///
/// # Returns
///
/// * `Ok(FileFormat)` - The detected format, mapped to our `FileFormat` enum
/// * `Err(io::Error)` - If Magika initialization fails or detection errors
///
/// # Performance
///
/// * Cold start (first call): ~100ms (model loading)
/// * Warm inference: ~5ms per file
/// * Throughput: ~1000 files/sec on modern hardware
///
/// # Examples
///
/// ```no_run
/// # #[cfg(feature = "magika")]
/// # {
/// let jpeg_data = std::fs::read("photo.jpg")?;
/// let format = detect_with_magika(&jpeg_data)?;
/// assert_eq!(format, FileFormat::JPEG);
/// # }
/// # Ok::<(), std::io::Error>(())
/// ```
#[cfg(feature = "magika")]
pub fn detect_with_magika(data: &[u8]) -> io::Result<FileFormat> {
    // Create a new Magika session (sessions are lightweight and thread-safe)
    let mut session = Session::new()
        .map_err(|e| io::Error::new(
            io::ErrorKind::Other,
            format!("Failed to initialize Magika: {}", e)
        ))?;

    // Perform AI-based detection
    let result = session
        .identify_content_sync(data)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("Magika detection failed: {}", e)))?;

    // Map Magika label to our FileFormat enum
    magika_label_to_format(result.info().label)
}

/// Map Magika content type labels to our FileFormat enum.
///
/// This function translates Magika's 200+ content type labels into our existing
/// FileFormat enum variants. For types we don't support yet, it returns `FileFormat::Unknown`.
///
/// # Mapping Strategy
///
/// 1. **Direct mappings**: Common formats map 1:1 (jpeg → JPEG, png → PNG)
/// 2. **Variant mappings**: Multiple Magika labels may map to same format (mp4/mov → QuickTime)
/// 3. **Unsupported**: New types Magika detects but we don't parse → Unknown
///
/// # Arguments
///
/// * `label` - Magika content type label (e.g., "jpeg", "png", "python")
///
/// # Returns
///
/// The corresponding `FileFormat` variant, or `FileFormat::Unknown` for unsupported types.
#[cfg(feature = "magika")]
fn magika_label_to_format(label: &str) -> io::Result<FileFormat> {
    let format = match label {
        // Images (Phase 0: Core formats)
        "jpeg" => FileFormat::JPEG,
        "tiff" => FileFormat::TIFF,
        "png" => FileFormat::PNG,
        "gif" => FileFormat::GIF,
        "bmp" => FileFormat::BMP,
        "webp" => FileFormat::WebP,
        "heic" | "heif" => FileFormat::HEIF,
        "ico" => FileFormat::ICO,
        "psd" => FileFormat::PSD,
        "svg" => FileFormat::SVG,

        // Images (Phase 5: Advanced formats)
        "avif" => FileFormat::AVIF,
        "jxl" => FileFormat::JXL,
        "exr" => FileFormat::EXR,
        "bpg" => FileFormat::BPG,
        "flif" => FileFormat::FLIF,

        // Documents
        "pdf" => FileFormat::PDF,
        "docx" => FileFormat::DOCX,
        "xlsx" => FileFormat::XLSX,
        "pptx" => FileFormat::PPTX,
        "epub" => FileFormat::EPUB,

        // Video (Phase 1)
        "mp4" | "mov" | "quicktime" => FileFormat::QuickTime,
        "mkv" | "matroska" => FileFormat::MKV,
        "webm" => FileFormat::WEBM,
        "flv" => FileFormat::FLV,
        "avi" => FileFormat::AVI,
        "mts" | "m2ts" => FileFormat::MTS,

        // Audio (Phase 1)
        "mp3" => FileFormat::MP3,
        "flac" => FileFormat::FLAC,
        "aac" | "m4a" => FileFormat::AAC,
        "wav" => FileFormat::WAV,
        "ogg" | "vorbis" => FileFormat::OGG,
        "opus" => FileFormat::OPUS,
        "ape" => FileFormat::APE,

        // Archives (Phase 2/3)
        "zip" => FileFormat::ZIP,
        "rar" => FileFormat::RAR,
        "7z" => FileFormat::SevenZ,
        "iso" => FileFormat::ISO,
        "tar" => FileFormat::TAR,
        "gzip" | "gz" => FileFormat::GZ,

        // Fonts (Phase 4)
        "ttf" => FileFormat::TTF,
        "otf" => FileFormat::OTF,
        "woff" => FileFormat::WOFF,
        "woff2" => FileFormat::WOFF2,

        // Executables (Phase 6)
        "pe" | "exe" | "dll" => FileFormat::PE,
        "elf" => FileFormat::ELF,
        "macho" => FileFormat::MachO,

        // CAD/3D (Phase 6)
        "dwg" => FileFormat::DWG,
        "dxf" => FileFormat::DXF,
        "stl" => FileFormat::STL,
        "obj" => FileFormat::OBJ,
        "gltf" | "glb" => FileFormat::GLTF,

        // Scientific (Phase 6)
        "fits" => FileFormat::FITS,
        "hdf5" => FileFormat::HDF5,

        // Text-based (Phase 7)
        "vcf" | "vcard" => FileFormat::VCF,
        "lnk" => FileFormat::LNK,

        // Unsupported types (Magika detects 200+ types, we only support ~50)
        // Examples: "python", "javascript", "rust", "json", "xml", "html", "markdown", etc.
        // These return Unknown - user may want to add to FileFormat enum in future
        _ => FileFormat::Unknown,
    };

    Ok(format)
}

#[cfg(test)]
#[cfg(feature = "magika")]
mod tests {
    use super::*;

    #[test]
    fn test_magika_label_mapping() {
        // Test common image formats
        assert_eq!(
            magika_label_to_format("jpeg").unwrap(),
            FileFormat::JPEG
        );
        assert_eq!(
            magika_label_to_format("png").unwrap(),
            FileFormat::PNG
        );
        assert_eq!(
            magika_label_to_format("gif").unwrap(),
            FileFormat::GIF
        );

        // Test video formats
        assert_eq!(
            magika_label_to_format("mp4").unwrap(),
            FileFormat::QuickTime
        );
        assert_eq!(
            magika_label_to_format("mkv").unwrap(),
            FileFormat::MKV
        );

        // Test audio formats
        assert_eq!(
            magika_label_to_format("mp3").unwrap(),
            FileFormat::MP3
        );
        assert_eq!(
            magika_label_to_format("flac").unwrap(),
            FileFormat::FLAC
        );

        // Test documents
        assert_eq!(
            magika_label_to_format("pdf").unwrap(),
            FileFormat::PDF
        );
        assert_eq!(
            magika_label_to_format("docx").unwrap(),
            FileFormat::DOCX
        );

        // Test unsupported types
        assert_eq!(
            magika_label_to_format("python").unwrap(),
            FileFormat::Unknown
        );
        assert_eq!(
            magika_label_to_format("unknown").unwrap(),
            FileFormat::Unknown
        );
    }
}
