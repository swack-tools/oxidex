//! File format enumeration for format detection and parser selection.
//!
//! This module defines the `FileFormat` enum used for file-level format detection
//! in the hexagonal architecture. This enum serves a different purpose from
//! `FormatFamily` in tag descriptors:
//!
//! - `FileFormat`: Identifies the container format of a file (JPEG, PNG, etc.)
//! - `FormatFamily`: Categorizes metadata standards within files (EXIF, XMP, etc.)
//!
//! A single file format may contain multiple metadata families. For example,
//! a JPEG file can contain EXIF, XMP, and IPTC metadata.

#![allow(dead_code)]

/// Represents the file format of a media file.
///
/// This enum is used by format parsers to indicate which file formats they support
/// and by the core library to route files to the appropriate parser implementation.
///
/// # Design Notes
///
/// The `Unknown` variant provides graceful degradation when a file format cannot
/// be detected or is not yet supported by the library.
///
/// # Examples
///
/// ```
/// use exiftool_rs::core::FileFormat;
///
/// let format = FileFormat::JPEG;
/// assert_eq!(format, FileFormat::JPEG);
///
/// // Check if format is supported
/// match format {
///     FileFormat::JPEG | FileFormat::TIFF | FileFormat::PNG => {
///         println!("Format is supported");
///     }
///     FileFormat::Unknown => {
///         println!("Format is unknown");
///     }
///     _ => println!("Format may be supported"),
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileFormat {
    /// JPEG/JFIF image format (.jpg, .jpeg)
    JPEG,

    /// TIFF image format (.tif, .tiff)
    TIFF,

    /// PNG image format (.png)
    PNG,

    /// PDF document format (.pdf)
    PDF,

    /// GIF image format (.gif)
    GIF,

    /// BMP bitmap image format (.bmp)
    BMP,

    /// QuickTime/MP4 video format (.mov, .mp4)
    QuickTime,

    /// HEIF/HEIC image format (.heif, .heic)
    HEIF,

    /// WebP image format (.webp)
    WebP,

    /// RAW image formats (generic)
    RAW,

    /// Unknown or unsupported format
    Unknown,
}

impl FileFormat {
    /// Returns a human-readable name for the format.
    ///
    /// # Examples
    ///
    /// ```
    /// use exiftool_rs::core::FileFormat;
    ///
    /// assert_eq!(FileFormat::JPEG.name(), "JPEG");
    /// assert_eq!(FileFormat::PNG.name(), "PNG");
    /// assert_eq!(FileFormat::Unknown.name(), "Unknown");
    /// ```
    pub fn name(&self) -> &'static str {
        match self {
            FileFormat::JPEG => "JPEG",
            FileFormat::TIFF => "TIFF",
            FileFormat::PNG => "PNG",
            FileFormat::PDF => "PDF",
            FileFormat::GIF => "GIF",
            FileFormat::BMP => "BMP",
            FileFormat::QuickTime => "QuickTime",
            FileFormat::HEIF => "HEIF",
            FileFormat::WebP => "WebP",
            FileFormat::RAW => "RAW",
            FileFormat::Unknown => "Unknown",
        }
    }

    /// Returns common file extensions for this format.
    ///
    /// # Examples
    ///
    /// ```
    /// use exiftool_rs::core::FileFormat;
    ///
    /// assert_eq!(FileFormat::JPEG.extensions(), &["jpg", "jpeg"]);
    /// assert_eq!(FileFormat::PNG.extensions(), &["png"]);
    /// ```
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            FileFormat::JPEG => &["jpg", "jpeg"],
            FileFormat::TIFF => &["tif", "tiff"],
            FileFormat::PNG => &["png"],
            FileFormat::PDF => &["pdf"],
            FileFormat::GIF => &["gif"],
            FileFormat::BMP => &["bmp"],
            FileFormat::QuickTime => &["mov", "mp4", "m4v"],
            FileFormat::HEIF => &["heif", "heic"],
            FileFormat::WebP => &["webp"],
            FileFormat::RAW => &["cr2", "nef", "arw", "dng"],
            FileFormat::Unknown => &[],
        }
    }
}

impl std::fmt::Display for FileFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}
