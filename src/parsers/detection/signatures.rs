//! Format signature definitions
//!
//! This module contains the signature structure and static table for
//! simple format detection via magic byte matching.

use crate::core::FileFormat;
use crate::parsers::raw;

/// A signature definition for format detection
///
/// This structure describes a file format signature including:
/// - The byte pattern to match
/// - The offset where the pattern should be found
/// - The format to return if matched
#[derive(Debug)]
pub struct Signature {
    /// Magic bytes to match against
    pub bytes: &'static [u8],
    /// Offset in the file where signature should be found (0 = file start)
    pub offset: u64,
    /// Format to return when this signature matches
    pub format: FileFormat,
}

/// Macro to simplify signature table creation
///
/// Usage: signature!(bytes, offset, format)
/// Example: signature!(b"PNG", 0, FileFormat::PNG)
macro_rules! signature {
    ($bytes:expr, $offset:expr, $format:expr) => {
        Signature {
            bytes: $bytes,
            offset: $offset,
            format: $format,
        }
    };
}

/// Static signature table for simple format detection
///
/// This table contains signatures that can be checked with simple byte matching
/// at fixed offsets. More complex formats requiring additional logic are handled
/// separately in the detect_format function.
///
/// Signatures are ordered from most specific to least specific to ensure
/// correct detection when multiple formats share similar patterns.
pub static SIMPLE_SIGNATURES: &[Signature] = &[
    // Camera Raw formats with unique signatures
    signature!(
        b"FUJIFILMCCD-RAW ",
        0,
        FileFormat::CameraRaw(raw::RawFormat::FujifilmRAF)
    ),
    signature!(b"FOVb", 0, FileFormat::CameraRaw(raw::RawFormat::SigmaX3F)),
    signature!(
        b"\x00MRM",
        0,
        FileFormat::CameraRaw(raw::RawFormat::MinoltaMRW)
    ),
    // Image formats
    signature!(b"\x89PNG", 0, FileFormat::PNG),
    signature!(b"GIF87a", 0, FileFormat::GIF),
    signature!(b"GIF89a", 0, FileFormat::GIF),
    signature!(b"BM", 0, FileFormat::BMP),
    signature!(b"8BPS", 0, FileFormat::PSD),
    signature!(b"\x00\x00\x01\x00", 0, FileFormat::ICO),
    signature!(b"FLIF", 0, FileFormat::FLIF),
    signature!(b"\x76\x2F\x31\x01", 0, FileFormat::EXR),
    signature!(b"\x42\x50\x47\xFB", 0, FileFormat::BPG),
    signature!(b"\xFF\x0A", 0, FileFormat::JXL),
    // Audio/Video formats
    signature!(b"fLaC", 0, FileFormat::FLAC),
    signature!(b"ID3", 0, FileFormat::MP3),
    signature!(b"FLV", 0, FileFormat::FLV),
    signature!(b"MAC ", 0, FileFormat::APE),
    signature!(b"\x1A\x45\xDF\xA3", 0, FileFormat::MKV),
    signature!(b"OggS", 0, FileFormat::OGG),
    // ASF/WMV - Header Object GUID
    signature!(
        b"\x30\x26\xB2\x75\x8E\x66\xCF\x11\xA6\xD9\x00\xAA\x00\x62\xCE\x6C",
        0,
        FileFormat::ASF
    ),
    // MXF - Universal Label prefix (06.0E.2B.34)
    signature!(b"\x06\x0E\x2B\x34", 0, FileFormat::MXF),
    // Document formats
    signature!(b"%PDF", 0, FileFormat::PDF),
    // Archive formats
    signature!(b"Rar!", 0, FileFormat::RAR),
    signature!(b"\x37\x7A\xBC\xAF\x27\x1C", 0, FileFormat::SevenZ),
    signature!(b"\x1F\x8B", 0, FileFormat::GZ),
    // Font formats
    signature!(b"OTTO", 0, FileFormat::OTF),
    signature!(b"wOFF", 0, FileFormat::WOFF),
    signature!(b"wOF2", 0, FileFormat::WOFF2),
    signature!(b"\x00\x01\x00\x00", 0, FileFormat::TTF),
    signature!(b"true", 0, FileFormat::TTF),
    // Binary formats
    signature!(b"\x7FELF", 0, FileFormat::ELF),
    signature!(b"\x89HDF\x0D\x0A\x1A\x0A", 0, FileFormat::HDF5),
    signature!(b"SIMPLE", 0, FileFormat::FITS),
    signature!(b"BEGIN:VCARD", 0, FileFormat::VCF),
    signature!(b"\x4C\x00\x00\x00", 0, FileFormat::LNK),
    signature!(b"SQLite format 3\0", 0, FileFormat::SQLite),
    signature!(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1", 0, FileFormat::OLE),
    // Windows forensic formats
    signature!(b"MAM\x04", 0, FileFormat::Prefetch), // Compressed prefetch (Win10+)
    signature!(b"SCCA", 4, FileFormat::Prefetch),    // Uncompressed prefetch
    signature!(b"regf", 0, FileFormat::Registry),
    signature!(b"ElfFile\0", 0, FileFormat::EVTX),
    // macOS formats
    signature!(b"bplist00", 0, FileFormat::Plist), // Binary plist v0
    signature!(b"bplist01", 0, FileFormat::Plist), // Binary plist v1
    // Network packet capture formats
    signature!(b"\x0a\x0d\x0d\x0a", 0, FileFormat::PCAPNG), // PCAP-NG Section Header Block
    signature!(b"\xa1\xb2\xc3\xd4", 0, FileFormat::PCAP),   // PCAP big-endian
    signature!(b"\xd4\xc3\xb2\xa1", 0, FileFormat::PCAP),   // PCAP little-endian
    signature!(b"\xa1\xb2\x3c\x4d", 0, FileFormat::PCAP),   // PCAP nanosecond big-endian
    signature!(b"\x4d\x3c\xb2\xa1", 0, FileFormat::PCAP),   // PCAP nanosecond little-endian
    // X.509 Certificates
    signature!(b"-----BEGIN CERTIFICATE-----", 0, FileFormat::X509),
    // ICC Profile (signature "acsp" at offset 36)
    signature!(b"acsp", 36, FileFormat::ICC),
    // XMP Sidecar (<?xpacket or <x:xmpmeta)
    signature!(b"<?xpacket", 0, FileFormat::XMP),
    signature!(b"<x:xmpmeta", 0, FileFormat::XMP),
    // Archive formats with offset signatures
    signature!(b"ustar", 257, FileFormat::TAR),
    signature!(b"CD001", 32769, FileFormat::ISO),
];
