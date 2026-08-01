//! Raw format detection based on magic bytes and file extension
//!
//! Camera raw files use various magic byte sequences to identify the format.
//! This module implements detection logic for all supported raw formats.
//!
//! ## Detection Strategy
//!
//! 1. **Magic Bytes First**: Check file header for format-specific signatures
//! 2. **Extension Fallback**: Use file extension when magic bytes are ambiguous
//!
//! Most raw formats are TIFF-based and share common magic bytes (II or MM),
//! so we combine magic byte detection with extension checking for accurate identification.

use std::path::Path;

/// Camera raw format families
///
/// This enum represents all supported camera raw file formats organized by manufacturer.
/// Each variant corresponds to a specific raw format with its own file structure and metadata layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RawFormat {
    // Canon
    /// Canon Raw version 2 - TIFF-based raw format used by Canon DSLRs
    CanonCR2,
    /// Canon Raw version 3 - ISO Base Media Format (similar to MP4) used by newer Canon cameras
    CanonCR3,
    /// Canon Raw (legacy) - Proprietary format used by older Canon cameras
    CanonCRW,

    // Nikon
    /// Nikon Electronic Format - TIFF-based raw format used by Nikon cameras
    NikonNEF,
    /// Nikon Raw (compressed) - Compressed variant of NEF
    NikonNRW,

    // Sony
    /// Sony Alpha Raw - TIFF-based raw format used by Sony Alpha cameras
    SonyARW,
    /// Sony Raw version 2 - Used by older Sony cameras
    SonySR2,
    /// Sony Raw Format - Alternative Sony raw format
    SonySRF,
    /// Samsung/Sony Raw - Used by Samsung cameras (Sony-compatible)
    SonySRW,
    /// Sony Alpha Raw Quad - High-resolution Sony format
    SonyARQ,
    /// ARRI Raw Image - Used by ARRI cinema cameras (Sony-compatible)
    SonyARI,

    // Fujifilm
    /// Fujifilm Raw - Proprietary raw format used by Fujifilm cameras
    FujifilmRAF,

    // Olympus
    /// Olympus Raw Format - TIFF-based raw format used by Olympus cameras
    OlympusORF,
    /// Olympus Raw Image - Alternative Olympus format
    OlympusORI,

    // Pentax
    /// Pentax Electronic Format - TIFF-based raw format used by Pentax cameras
    PentaxPEF,

    // Panasonic
    /// Panasonic Raw version 2 - Raw format used by Panasonic Lumix cameras
    PanasonicRW2,
    /// Panasonic Raw (legacy) - Older Panasonic raw format
    PanasonicRWL,

    // Hasselblad
    /// Hasselblad 3F Raw - Raw format used by Hasselblad medium format cameras
    Hasselblad3FR,
    /// Hasselblad Flexible File Format - Alternative Hasselblad format
    HasselbladFFF,

    // Phase One
    /// Phase One Intelligent Image Quality - Raw format used by Phase One cameras
    PhaseOneIIQ,

    // Mamiya
    /// Mamiya Electronic Format - Raw format used by Mamiya cameras
    MamiyaMEF,

    // Leaf
    /// Leaf Mosaic - Raw format used by Leaf digital backs
    LeafMOS,

    // Kodak
    /// Kodak Digital Camera Raw - Raw format used by Kodak cameras
    KodakDCR,
    /// Kodak Digital Camera - Alternative Kodak format
    KodakKDC,

    // Minolta
    /// Minolta Digital Camera - Raw format used by Minolta cameras
    MinoltaMDC,
    /// Minolta Raw - Proprietary Minolta format
    MinoltaMRW,

    // Epson
    /// Epson Raw Format - Raw format used by Epson cameras
    EpsonERF,

    // Sigma
    /// Sigma X3F - Proprietary format used by Sigma cameras with Foveon sensor
    SigmaX3F,

    // GoPro
    /// GoPro Raw - Raw format used by GoPro cameras
    GoProGPR,

    // Adobe
    /// Adobe Digital Negative - Open raw format standard based on TIFF
    AdobeDNG,

    // Other
    /// HEIF-based raw format
    HEIFHIF,
    /// Light L16 Raw Image - Raw format used by Light cameras
    LightLRI,
    /// Sinar Tag Image - Raw format used by Sinar cameras
    SinarSTI,
    /// Generic RAW format (manufacturer-agnostic)
    GenericRAW,
    /// Generic CAM format
    GenericCAM,
    /// Generic REV format
    GenericREV,
}

impl RawFormat {
    /// ExifTool's `FileType` for this format.
    ///
    /// The variant names carry the manufacturer for readability at the call
    /// site, so `format!("{:?}", format)` -- which is what the raw parsers used
    /// to write into `File:FileType` -- put `CanonCR2`, `NikonNEF`,
    /// `AdobeDNG`, `SigmaX3F`, `MinoltaMRW`, `FujifilmRAF`, `PanasonicRW2`,
    /// `PhaseOneIIQ` and `CanonCRW` on those files. ExifTool reports the bare
    /// format name: `CR2`, `NEF`, `DNG`, ...
    ///
    /// Every name below is the key ExifTool's `%fileTypeLookup` resolves that
    /// extension to, after chasing scalar aliases the way `GetFileType` does --
    /// which is why `OlympusORI` reports `ORF` (`ORI => 'ORF'`) and `HEIFHIF`
    /// reports `HEIF` (`HIF => 'HEIF'`) rather than echoing their own
    /// extension.
    #[must_use]
    pub fn file_type(self) -> &'static str {
        match self {
            Self::CanonCR2 => "CR2",
            Self::CanonCR3 => "CR3",
            Self::CanonCRW => "CRW",
            Self::NikonNEF => "NEF",
            Self::NikonNRW => "NRW",
            Self::SonyARW => "ARW",
            Self::SonySR2 => "SR2",
            Self::SonySRF => "SRF",
            Self::SonySRW => "SRW",
            Self::SonyARQ => "ARQ",
            Self::FujifilmRAF => "RAF",
            Self::OlympusORF => "ORF",
            // `ORI => 'ORF'` in %fileTypeLookup: an .ori reports as ORF.
            Self::OlympusORI => "ORF",
            Self::PentaxPEF => "PEF",
            Self::PanasonicRW2 => "RW2",
            Self::PanasonicRWL => "RWL",
            Self::Hasselblad3FR => "3FR",
            Self::HasselbladFFF => "FFF",
            Self::PhaseOneIIQ => "IIQ",
            Self::MamiyaMEF => "MEF",
            Self::LeafMOS => "MOS",
            Self::KodakDCR => "DCR",
            Self::KodakKDC => "KDC",
            Self::MinoltaMRW => "MRW",
            Self::EpsonERF => "ERF",
            Self::SigmaX3F => "X3F",
            Self::GoProGPR => "GPR",
            Self::AdobeDNG => "DNG",
            // `HIF => 'HEIF'` in %fileTypeLookup.
            Self::HEIFHIF => "HEIF",
            Self::LightLRI => "LRI",
            Self::GenericRAW => "RAW",
            // ExifTool 13.30's %fileTypeLookup has no entry for these five
            // extensions -- `exiftool` reports "Unknown file type" for them --
            // so there is no upstream name to match. They keep the bare
            // extension, which is at least the shape of a FileType rather than
            // a Rust identifier.
            Self::SonyARI => "ARI",
            Self::MinoltaMDC => "MDC",
            Self::SinarSTI => "STI",
            Self::GenericCAM => "CAM",
            Self::GenericREV => "REV",
        }
    }

    /// ExifTool's `File:MIMEType` for this format, when `%mimeType` in
    /// `Image/ExifTool.pm` defines one.
    ///
    /// `None` means ExifTool has no entry, and callers should leave whatever
    /// the generic extension table produced rather than invent a value.
    pub fn exiftool_mime_type(self) -> Option<&'static str> {
        Some(match self {
            RawFormat::CanonCR2 => "image/x-canon-cr2", // ExifTool.pm:634
            RawFormat::CanonCR3 => "image/x-canon-cr3", // ExifTool.pm:637
            RawFormat::CanonCRW => "image/x-canon-crw", // ExifTool.pm:639
            RawFormat::NikonNEF => "image/x-nikon-nef",
            RawFormat::NikonNRW => "image/x-nikon-nrw",
            RawFormat::SonyARW => "image/x-sony-arw",
            RawFormat::SonySR2 => "image/x-sony-sr2",
            RawFormat::SonySRF => "image/x-sony-srf",
            RawFormat::SonySRW => "image/x-samsung-srw",
            RawFormat::FujifilmRAF => "image/x-fujifilm-raf",
            RawFormat::OlympusORF | RawFormat::OlympusORI => "image/x-olympus-orf",
            RawFormat::PentaxPEF => "image/x-pentax-pef",
            RawFormat::PanasonicRW2 => "image/x-panasonic-rw2",
            RawFormat::PanasonicRWL => "image/x-leica-rwl",
            RawFormat::Hasselblad3FR => "image/x-hasselblad-3fr",
            RawFormat::HasselbladFFF => "image/x-hasselblad-fff",
            RawFormat::MamiyaMEF => "image/x-mamiya-mef",
            RawFormat::KodakDCR => "image/x-kodak-dcr",
            RawFormat::KodakKDC => "image/x-kodak-kdc",
            RawFormat::MinoltaMRW => "image/x-minolta-mrw",
            RawFormat::EpsonERF => "image/x-epson-erf",
            RawFormat::SigmaX3F => "image/x-sigma-x3f",
            RawFormat::GoProGPR => "image/x-gopro-gpr",
            RawFormat::AdobeDNG => "image/x-adobe-dng",
            RawFormat::LightLRI => "image/x-light-lri",
            // ExifTool.pm maps IIQ, MOS and RAW to the generic raw MIME type.
            RawFormat::PhaseOneIIQ | RawFormat::LeafMOS | RawFormat::GenericRAW => "image/x-raw",
            RawFormat::SonyARQ
            | RawFormat::SonyARI
            | RawFormat::MinoltaMDC
            | RawFormat::SinarSTI
            | RawFormat::HEIFHIF
            | RawFormat::GenericCAM
            | RawFormat::GenericREV => return None,
        })
    }
}

/// Detect raw format from magic bytes and file extension
///
/// This function analyzes the first bytes of a file and its extension to determine
/// the camera raw format. It prioritizes magic byte detection but falls back to
/// extension-based detection when magic bytes are ambiguous or insufficient.
///
/// # Arguments
/// * `data` - First 16-32 bytes of the file (minimum 4 bytes recommended)
/// * `filename` - File name including extension (for extension fallback)
///
/// # Returns
/// * `Some(RawFormat)` - Successfully detected format
/// * `None` - Not a recognized raw format
///
/// # Examples
/// ```
/// use oxidex::parsers::raw::{detect_raw_format, RawFormat};
///
/// // Canon CR2 has distinctive magic bytes
/// let cr2_data = b"II\x2a\x00\x10\x00\x00\x00CR\x02\x00";
/// assert_eq!(detect_raw_format(cr2_data, "photo.cr2"), Some(RawFormat::CanonCR2));
///
/// // Extension fallback for TIFF-based formats
/// let tiff_data = b"II\x2a\x00\x08\x00\x00\x00";
/// assert_eq!(detect_raw_format(tiff_data, "photo.arw"), Some(RawFormat::SonyARW));
/// ```
pub fn detect_raw_format(data: &[u8], filename: &str) -> Option<RawFormat> {
    // Extract file extension for fallback detection
    let ext = Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())?;

    // Check magic bytes first, fall back to extension for TIFF-based formats

    // TIFF-based formats use either:
    // - II (0x49 0x49) = little-endian
    // - MM (0x4d 0x4d) = big-endian
    // Followed by 0x002a (magic number 42)
    if data.len() >= 8 {
        match &data[0..4] {
            // Canon CR2 has a distinctive marker at offset 8: "CR\x02\x00"
            // This distinguishes it from other TIFF-based formats
            [0x49, 0x49, 0x2a, 0x00] if data.len() >= 12 && &data[8..12] == b"CR\x02\x00" => {
                return Some(RawFormat::CanonCR2);
            }

            // Nikon NEF (TIFF big-endian) - must check extension to distinguish from other TIFF formats
            [0x4d, 0x4d, 0x00, 0x2a] if ext == "nef" => {
                return Some(RawFormat::NikonNEF);
            }

            // Sony ARW (TIFF little-endian) - must check extension
            [0x49, 0x49, 0x2a, 0x00] if ext == "arw" => {
                return Some(RawFormat::SonyARW);
            }

            // DNG (TIFF with DNG version tag) - must check extension
            // DNG files are TIFF-based but include a DNGVersion tag (0xC612)
            [0x49, 0x49, 0x2a, 0x00] if ext == "dng" => {
                return Some(RawFormat::AdobeDNG);
            }

            _ => {}
        }
    }

    // Canon CR3 uses ISO Base Media File Format (similar to MP4/QuickTime)
    // Magic bytes: 4 bytes size + "ftypcrx " (file type box)
    if data.len() >= 12 && &data[4..12] == b"ftypcrx " {
        return Some(RawFormat::CanonCR3);
    }

    // Fujifilm RAF has a distinctive 16-byte signature
    // "FUJIFILMCCD-RAW " (note the trailing space)
    if data.len() >= 16 && &data[0..16] == b"FUJIFILMCCD-RAW " {
        return Some(RawFormat::FujifilmRAF);
    }

    // Sigma X3F uses "FOVb" signature
    // FOV = Field of View, proprietary Foveon sensor format
    if data.len() >= 4 && &data[0..4] == b"FOVb" {
        return Some(RawFormat::SigmaX3F);
    }

    // Minolta MRW uses "\x00MRM" signature
    // MRM = Minolta Raw format Marker
    if data.len() >= 4 && &data[0..4] == b"\x00MRM" {
        return Some(RawFormat::MinoltaMRW);
    }

    // Extension-based detection for formats where magic bytes don't provide enough info
    // Many raw formats are TIFF-based and share the same magic bytes,
    // so extension is the primary differentiator
    match ext.as_str() {
        // Canon formats
        "cr2" => Some(RawFormat::CanonCR2),
        "cr3" => Some(RawFormat::CanonCR3),
        "crw" => Some(RawFormat::CanonCRW),

        // Nikon formats
        "nef" => Some(RawFormat::NikonNEF),
        "nrw" => Some(RawFormat::NikonNRW),

        // Sony formats
        "arw" => Some(RawFormat::SonyARW),
        "sr2" => Some(RawFormat::SonySR2),
        "srf" => Some(RawFormat::SonySRF),
        "srw" => Some(RawFormat::SonySRW),
        "arq" => Some(RawFormat::SonyARQ),
        "ari" => Some(RawFormat::SonyARI),

        // Fujifilm
        "raf" => Some(RawFormat::FujifilmRAF),

        // Olympus
        "orf" => Some(RawFormat::OlympusORF),
        "ori" => Some(RawFormat::OlympusORI),

        // Pentax
        "pef" => Some(RawFormat::PentaxPEF),

        // Panasonic
        "rw2" => Some(RawFormat::PanasonicRW2),
        "rwl" => Some(RawFormat::PanasonicRWL),

        // Hasselblad
        "3fr" => Some(RawFormat::Hasselblad3FR),
        "fff" => Some(RawFormat::HasselbladFFF),

        // Phase One
        "iiq" => Some(RawFormat::PhaseOneIIQ),

        // Mamiya
        "mef" => Some(RawFormat::MamiyaMEF),

        // Leaf
        "mos" => Some(RawFormat::LeafMOS),

        // Kodak
        "dcr" => Some(RawFormat::KodakDCR),
        "kdc" => Some(RawFormat::KodakKDC),

        // Minolta
        "mdc" => Some(RawFormat::MinoltaMDC),
        "mrw" => Some(RawFormat::MinoltaMRW),

        // Epson
        "erf" => Some(RawFormat::EpsonERF),

        // Sigma
        "x3f" => Some(RawFormat::SigmaX3F),

        // GoPro
        "gpr" => Some(RawFormat::GoProGPR),

        // Adobe
        "dng" => Some(RawFormat::AdobeDNG),

        // Other formats
        "hif" => Some(RawFormat::HEIFHIF),
        "lri" => Some(RawFormat::LightLRI),
        "sti" => Some(RawFormat::SinarSTI),
        "raw" => Some(RawFormat::GenericRAW),
        "cam" => Some(RawFormat::GenericCAM),
        "rev" => Some(RawFormat::GenericREV),

        // Not a recognized raw format
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canon_cr2_magic() {
        // Canon CR2 has TIFF header + CR\x02\x00 marker at offset 8
        let data = b"II\x2a\x00\x10\x00\x00\x00CR\x02\x00\x00\x00\x00\x00";
        assert_eq!(
            detect_raw_format(data, "test.cr2"),
            Some(RawFormat::CanonCR2),
            "Should detect CR2 from magic bytes"
        );
    }

    #[test]
    fn test_canon_cr3_magic() {
        // CR3 uses ISO Base Media format with "ftypcrx " marker
        let data = b"\x00\x00\x00\x18ftypcrx \x00\x00\x00\x00";
        assert_eq!(
            detect_raw_format(data, "test.cr3"),
            Some(RawFormat::CanonCR3),
            "Should detect CR3 from magic bytes"
        );
    }

    #[test]
    fn test_fujifilm_raf_magic() {
        // Fujifilm RAF has distinctive 16-byte signature
        let data = b"FUJIFILMCCD-RAW \x00\x00\x00\x00";
        assert_eq!(
            detect_raw_format(data, "test.raf"),
            Some(RawFormat::FujifilmRAF),
            "Should detect RAF from magic bytes"
        );
    }

    #[test]
    fn test_sigma_x3f_magic() {
        // Sigma X3F has "FOVb" signature
        let data = b"FOVb\x00\x00\x00\x00";
        assert_eq!(
            detect_raw_format(data, "test.x3f"),
            Some(RawFormat::SigmaX3F),
            "Should detect X3F from magic bytes"
        );
    }

    #[test]
    fn test_minolta_mrw_magic() {
        // Minolta MRW has "\x00MRM" signature
        let data = b"\x00MRM\x00\x00\x00\x00";
        assert_eq!(
            detect_raw_format(data, "test.mrw"),
            Some(RawFormat::MinoltaMRW),
            "Should detect MRW from magic bytes"
        );
    }

    #[test]
    fn test_extension_fallback() {
        // Generic TIFF header without distinctive markers - should use extension
        let data = b"\x00\x00\x00\x00";
        assert_eq!(
            detect_raw_format(data, "test.nef"),
            Some(RawFormat::NikonNEF),
            "Should fall back to extension for NEF"
        );
        assert_eq!(
            detect_raw_format(data, "test.arw"),
            Some(RawFormat::SonyARW),
            "Should fall back to extension for ARW"
        );
        assert_eq!(
            detect_raw_format(data, "test.dng"),
            Some(RawFormat::AdobeDNG),
            "Should fall back to extension for DNG"
        );
    }

    #[test]
    fn test_all_extensions() {
        // Verify all extensions are mapped
        let test_cases = vec![
            ("test.cr2", RawFormat::CanonCR2),
            ("test.cr3", RawFormat::CanonCR3),
            ("test.crw", RawFormat::CanonCRW),
            ("test.nef", RawFormat::NikonNEF),
            ("test.nrw", RawFormat::NikonNRW),
            ("test.orf", RawFormat::OlympusORF),
            ("test.pef", RawFormat::PentaxPEF),
            ("test.rw2", RawFormat::PanasonicRW2),
            ("test.3fr", RawFormat::Hasselblad3FR),
            ("test.iiq", RawFormat::PhaseOneIIQ),
        ];

        let minimal_data = b"\x00\x00\x00\x00";
        for (filename, expected) in test_cases {
            assert_eq!(
                detect_raw_format(minimal_data, filename),
                Some(expected),
                "Failed for {}",
                filename
            );
        }
    }

    #[test]
    fn test_case_insensitive_extension() {
        // Extension matching should be case-insensitive
        let data = b"\x00\x00\x00\x00";
        assert_eq!(
            detect_raw_format(data, "test.NEF"),
            Some(RawFormat::NikonNEF),
            "Should handle uppercase extension"
        );
        assert_eq!(
            detect_raw_format(data, "test.Nef"),
            Some(RawFormat::NikonNEF),
            "Should handle mixed case extension"
        );
    }

    #[test]
    fn test_no_extension() {
        // Should return None when no extension is present
        let data = b"II\x2a\x00\x08\x00\x00\x00";
        assert_eq!(
            detect_raw_format(data, "test"),
            None,
            "Should return None for file without extension"
        );
    }

    #[test]
    fn test_unknown_extension() {
        // Should return None for unknown extensions
        let data = b"\x00\x00\x00\x00";
        assert_eq!(
            detect_raw_format(data, "test.xyz"),
            None,
            "Should return None for unknown extension"
        );
    }

    #[test]
    fn test_insufficient_data() {
        // Should handle small data buffers gracefully
        let data = b"II";
        // Should fall back to extension when data is insufficient
        assert_eq!(
            detect_raw_format(data, "test.nef"),
            Some(RawFormat::NikonNEF),
            "Should use extension when data is insufficient"
        );
    }
}

#[cfg(test)]
mod exiftool_file_type_tests {
    use super::*;

    /// Every raw format must report an ExifTool file type, never the Rust
    /// variant name. Before this mapping existed all ten raw formats in the
    /// sample corpus were wrong:
    ///
    /// ```text
    /// exiftool=MRW oxidex=MinoltaMRW     exiftool=X3F oxidex=SigmaX3F
    /// exiftool=RAF oxidex=FujifilmRAF    exiftool=DNG oxidex=AdobeDNG
    /// exiftool=IIQ oxidex=PhaseOneIIQ    exiftool=RW2 oxidex=PanasonicRW2
    /// exiftool=NEF oxidex=NikonNEF       exiftool=CR2 oxidex=CanonCR2
    /// exiftool=CRW oxidex=CanonCRW
    /// ```
    #[test]
    fn file_type_is_never_the_rust_variant_name() {
        for format in ALL_RAW_FORMATS {
            let ft = format.file_type();
            let debug = format!("{format:?}");
            assert_ne!(
                ft, debug,
                "{debug} reports its Rust variant name as File:FileType"
            );
            assert!(
                ft.len() <= 4
                    && ft
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()),
                "{debug} -> {ft:?} is not an ExifTool file type code"
            );
        }
    }

    /// Spot-checks quoted from `%fileTypeLookup` in ExifTool 13.55's
    /// `Image/ExifTool.pm`, each confirmed against `exiftool -s -S -FileType`
    /// on the matching sample-corpus file.
    #[test]
    fn file_types_match_exiftool_lookup() {
        assert_eq!(RawFormat::SigmaX3F.file_type(), "X3F");
        assert_eq!(RawFormat::MinoltaMRW.file_type(), "MRW");
        assert_eq!(RawFormat::FujifilmRAF.file_type(), "RAF");
        assert_eq!(RawFormat::AdobeDNG.file_type(), "DNG");
        assert_eq!(RawFormat::PhaseOneIIQ.file_type(), "IIQ");
        assert_eq!(RawFormat::PanasonicRW2.file_type(), "RW2");
        assert_eq!(RawFormat::NikonNEF.file_type(), "NEF");
        assert_eq!(RawFormat::CanonCR2.file_type(), "CR2");
        assert_eq!(RawFormat::CanonCRW.file_type(), "CRW");
        assert_eq!(RawFormat::CanonCR3.file_type(), "CR3");
        assert_eq!(RawFormat::Hasselblad3FR.file_type(), "3FR");
        // ExifTool.pm:496 - RWL is Leica RAW, not Panasonic, despite the
        // Rust variant being named PanasonicRWL.
        assert_eq!(RawFormat::PanasonicRWL.file_type(), "RWL");
        assert_eq!(
            RawFormat::PanasonicRWL.exiftool_mime_type(),
            Some("image/x-leica-rwl")
        );
        // ExifTool.pm:519 - SRW is Samsung RAW, again unlike the variant name.
        assert_eq!(
            RawFormat::SonySRW.exiftool_mime_type(),
            Some("image/x-samsung-srw")
        );
    }

    /// Formats ExifTool 13.55 has no `%mimeType` entry for must return `None`
    /// rather than an invented MIME string.
    #[test]
    fn absent_mime_types_are_none() {
        assert_eq!(RawFormat::SonyARI.exiftool_mime_type(), None);
        assert_eq!(RawFormat::MinoltaMDC.exiftool_mime_type(), None);
        assert_eq!(RawFormat::SinarSTI.exiftool_mime_type(), None);
    }

    const ALL_RAW_FORMATS: [RawFormat; 36] = [
        RawFormat::CanonCR2,
        RawFormat::CanonCR3,
        RawFormat::CanonCRW,
        RawFormat::NikonNEF,
        RawFormat::NikonNRW,
        RawFormat::SonyARW,
        RawFormat::SonySR2,
        RawFormat::SonySRF,
        RawFormat::SonySRW,
        RawFormat::SonyARQ,
        RawFormat::SonyARI,
        RawFormat::FujifilmRAF,
        RawFormat::OlympusORF,
        RawFormat::OlympusORI,
        RawFormat::PentaxPEF,
        RawFormat::PanasonicRW2,
        RawFormat::PanasonicRWL,
        RawFormat::Hasselblad3FR,
        RawFormat::HasselbladFFF,
        RawFormat::PhaseOneIIQ,
        RawFormat::MamiyaMEF,
        RawFormat::LeafMOS,
        RawFormat::KodakDCR,
        RawFormat::KodakKDC,
        RawFormat::MinoltaMDC,
        RawFormat::MinoltaMRW,
        RawFormat::EpsonERF,
        RawFormat::SigmaX3F,
        RawFormat::GoProGPR,
        RawFormat::AdobeDNG,
        RawFormat::HEIFHIF,
        RawFormat::LightLRI,
        RawFormat::SinarSTI,
        RawFormat::GenericRAW,
        RawFormat::GenericCAM,
        RawFormat::GenericREV,
    ];
}
