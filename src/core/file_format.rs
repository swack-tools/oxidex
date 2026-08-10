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

use crate::parsers::raw::RawFormat;

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
/// use oxidex::core::FileFormat;
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

    /// Digital Picture Exchange image format (.dpx)
    DPX,

    /// QuickTime/MP4 video format (.mov, .mp4)
    QuickTime,

    /// HEIF/HEIC image format (.heif, .heic)
    HEIF,

    /// WebP image format (.webp)
    WebP,

    /// Casio CAM format - JPEG with 70-byte proprietary header (.cam)
    CasioCAM,

    /// RAW image formats (generic)
    RAW,

    /// Camera raw formats from various manufacturers
    /// This variant wraps a RawFormat enum that identifies the specific camera raw format
    /// (e.g., Canon CR2, Nikon NEF, Sony ARW, etc.)
    CameraRaw(RawFormat),

    /// Portable Executable format (.exe, .dll, .sys)
    PE,

    // Phase 1: Video/Audio formats
    /// MKV (Matroska) video format (.mkv)
    MKV,

    /// WebM video format (.webm)
    WEBM,

    /// FLV (Flash Video) format (.flv)
    FLV,

    /// AVI (Audio Video Interleave) format (.avi)
    AVI,

    /// MTS (MPEG Transport Stream) format (.mts, .m2ts)
    MTS,

    /// ASF/WMV (Advanced Systems Format) video format (.asf, .wmv, .wma)
    ASF,

    /// MXF (Material eXchange Format) video format (.mxf)
    MXF,

    /// MP3 audio format (.mp3)
    MP3,

    /// FLAC audio format (.flac)
    FLAC,

    /// AAC audio format (.aac, .m4a)
    AAC,

    /// WAV audio format (.wav)
    WAV,

    /// AIFF/AIFC audio format (.aif, .aiff, .aifc)
    AIFF,

    /// OGG Vorbis audio format (.ogg)
    OGG,

    /// Opus audio format (.opus)
    OPUS,

    /// APE (Monkey's Audio) format (.ape)
    APE,

    /// RealAudio metafile containing a streaming URL (.ram)
    RAM,

    /// Olympus Digital Speech Standard audio (.dss, .ds2)
    DSS,

    // Phase 2: Document formats
    /// ZIP archive format (.zip)
    ZIP,

    /// DOCX document format (.docx)
    DOCX,

    /// XLSX spreadsheet format (.xlsx)
    XLSX,

    /// PPTX presentation format (.pptx)
    PPTX,

    /// Apple Pages document (.pages)
    Pages,

    /// Apple Numbers spreadsheet (.numbers)
    Numbers,

    /// Apple Keynote presentation (.key)
    Keynote,

    /// EPUB e-book format (.epub)
    EPUB,

    // Phase 3: Archive formats
    /// RAR archive format (.rar)
    RAR,

    /// 7z archive format (.7z)
    SevenZ,

    /// ISO 9660 disc image (.iso)
    ISO,

    /// TAR archive format (.tar)
    TAR,

    /// GZIP compressed file (.gz)
    GZ,

    // Phase 4: Font formats
    /// TrueType font (.ttf)
    TTF,

    /// OpenType font (.otf)
    OTF,

    /// Web Open Font Format (.woff)
    WOFF,

    /// Web Open Font Format 2 (.woff2)
    WOFF2,

    // Phase 5: Advanced image formats
    /// AVIF image format (.avif)
    AVIF,

    /// JPEG XL image format (.jxl)
    JXL,

    /// Better Portable Graphics (.bpg)
    BPG,

    /// OpenEXR image format (.exr)
    EXR,

    /// Portable FloatMap image format (.pfm)
    PFM,

    /// Radiance RGBE high dynamic range image (.hdr)
    HDR,

    /// Free Lossless Image Format (.flif)
    FLIF,

    /// GIMP native image format (.xcf)
    XCF,

    /// Magick Image File Format (.miff, .mif)
    MIFF,

    /// Scalable Vector Graphics (.svg)
    SVG,

    /// Windows Icon format (.ico)
    ICO,

    /// Adobe Photoshop document (.psd)
    PSD,

    /// WordPerfect Graphics image (.wpg)
    WPG,

    /// DjVu image or multi-page document (.djvu, .djv)
    DJVU,

    // Phase 6: Specialized formats
    /// ELF executable format
    ELF,

    /// Mach-O executable format
    MachO,

    /// AutoCAD DWG format
    DWG,

    /// AutoCAD DXF format
    DXF,

    /// STL 3D model format
    STL,

    /// Wavefront OBJ 3D model
    OBJ,

    /// glTF 3D model format
    GLTF,

    /// Garmin FIT activity file format
    FIT,

    /// FITS astronomy format
    FITS,

    /// HDF5 data format
    HDF5,

    // Phase 7: Text-based formats
    /// vCard contact format (.vcf)
    VCF,

    /// iCalendar format (.ics)
    ICS,

    /// Email message format (.eml)
    EML,

    /// Plain text format (.txt)
    TXT,

    /// HTML / XHTML document (.htm, .html, .xhtml)
    HTML,

    /// Windows shortcut (.lnk)
    LNK,

    /// Lytro Light Field Picture (.lfp, .lfr)
    LFP,

    /// SQLite database (.db, .sqlite, .sqlite3)
    SQLite,

    /// Windows Prefetch file (.pf)
    Prefetch,

    /// Windows Registry Hive (.dat, hive files)
    Registry,

    /// Windows Event Log (.evtx)
    EVTX,

    /// macOS Property List (.plist)
    Plist,

    /// OLE (Object Linking and Embedding) / Compound File Binary Format (.doc, .xls, .ppt, .msg)
    OLE,

    // Phase 8: Network formats
    /// PCAP (libpcap) packet capture (.pcap, .cap)
    PCAP,

    /// PCAP-NG (next generation) packet capture (.pcapng)
    PCAPNG,

    /// X.509 Certificate (.crt, .cer, .pem, .der)
    X509,

    /// ICC Color Profile (.icc, .icm)
    ICC,

    /// XMP Sidecar (.xmp)
    XMP,

    /// EPS (Encapsulated PostScript) (.eps, .epsf, .ps)
    EPS,

    /// Canon DPP recipe, standalone rather than appended to an image (.vrd)
    VRD,

    /// Canon DPP version 4 recipe, standalone rather than appended (.dr4)
    DR4,

    /// FLIR Public image Format (.fpf) -- thermal camera raw image
    FPF,

    /// DICOM medical image (.dcm)
    DICOM,

    /// Unknown or unsupported format
    Unknown,
}

impl FileFormat {
    /// Returns a human-readable name for the format.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxidex::core::FileFormat;
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
            FileFormat::DPX => "DPX",
            FileFormat::QuickTime => "QuickTime",
            FileFormat::HEIF => "HEIF",
            FileFormat::WebP => "WebP",
            FileFormat::CasioCAM => "Casio CAM",
            FileFormat::RAW => "RAW",
            FileFormat::CameraRaw(_) => "Camera Raw",
            FileFormat::PE => "PE",
            FileFormat::MKV => "MKV",
            FileFormat::WEBM => "WebM",
            FileFormat::FLV => "FLV",
            FileFormat::AVI => "AVI",
            FileFormat::MTS => "MTS",
            FileFormat::ASF => "ASF",
            FileFormat::MXF => "MXF",
            FileFormat::MP3 => "MP3",
            FileFormat::FLAC => "FLAC",
            FileFormat::AAC => "AAC",
            FileFormat::WAV => "WAV",
            FileFormat::AIFF => "AIFF",
            FileFormat::OGG => "OGG",
            FileFormat::OPUS => "Opus",
            FileFormat::APE => "APE",
            FileFormat::RAM => "RAM",
            FileFormat::DSS => "DSS",
            FileFormat::ZIP => "ZIP",
            FileFormat::DOCX => "DOCX",
            FileFormat::XLSX => "XLSX",
            FileFormat::PPTX => "PPTX",
            FileFormat::Pages => "Pages",
            FileFormat::Numbers => "Numbers",
            FileFormat::Keynote => "Keynote",
            FileFormat::EPUB => "EPUB",
            FileFormat::RAR => "RAR",
            FileFormat::SevenZ => "7z",
            FileFormat::ISO => "ISO",
            FileFormat::TAR => "TAR",
            FileFormat::GZ => "GZIP",
            FileFormat::TTF => "TTF",
            FileFormat::OTF => "OTF",
            FileFormat::WOFF => "WOFF",
            FileFormat::WOFF2 => "WOFF2",
            FileFormat::AVIF => "AVIF",
            FileFormat::JXL => "JXL",
            FileFormat::BPG => "BPG",
            FileFormat::EXR => "EXR",
            FileFormat::PFM => "PFM",
            FileFormat::HDR => "HDR",
            FileFormat::FLIF => "FLIF",
            FileFormat::XCF => "XCF",
            FileFormat::MIFF => "MIFF",
            FileFormat::SVG => "SVG",
            FileFormat::ICO => "ICO",
            FileFormat::PSD => "PSD",
            FileFormat::WPG => "WPG",
            FileFormat::DJVU => "DJVU",
            FileFormat::ELF => "ELF",
            FileFormat::MachO => "Mach-O",
            FileFormat::DWG => "DWG",
            FileFormat::DXF => "DXF",
            FileFormat::STL => "STL",
            FileFormat::OBJ => "OBJ",
            FileFormat::GLTF => "glTF",
            FileFormat::FIT => "FIT",
            FileFormat::FITS => "FITS",
            FileFormat::HDF5 => "HDF5",
            FileFormat::VCF => "vCard",
            FileFormat::ICS => "iCalendar",
            FileFormat::EML => "EML",
            FileFormat::TXT => "TXT",
            FileFormat::LFP => "LFP",
            FileFormat::HTML => "HTML",
            FileFormat::LNK => "Windows Shortcut",
            FileFormat::SQLite => "SQLite",
            FileFormat::Prefetch => "Windows Prefetch",
            FileFormat::Registry => "Windows Registry Hive",
            FileFormat::EVTX => "EVTX",
            FileFormat::Plist => "Plist",
            FileFormat::OLE => "OLE",
            FileFormat::PCAP => "PCAP",
            FileFormat::PCAPNG => "PCAP-NG",
            FileFormat::X509 => "X.509",
            FileFormat::ICC => "ICC",
            FileFormat::XMP => "XMP",
            FileFormat::EPS => "EPS",
            FileFormat::VRD => "VRD",
            FileFormat::DR4 => "DR4",
            FileFormat::FPF => "FPF",
            FileFormat::DICOM => "DICOM",
            FileFormat::Unknown => "Unknown",
        }
    }

    /// Returns common file extensions for this format.
    ///
    /// # Examples
    ///
    /// ```
    /// use oxidex::core::FileFormat;
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
            FileFormat::DPX => &["dpx"],
            FileFormat::QuickTime => &["mov", "mp4", "m4v"],
            FileFormat::HEIF => &["heif", "heic"],
            FileFormat::WebP => &["webp"],
            FileFormat::CasioCAM => &["cam"],
            FileFormat::RAW => &["cr2", "nef", "arw", "dng"],
            FileFormat::CameraRaw(_) => &[
                "cr2", "cr3", "crw", "nef", "nrw", "arw", "sr2", "srf", "srw", "arq", "ari", "raf",
                "orf", "ori", "pef", "rw2", "rwl", "3fr", "fff", "iiq", "mef", "mos", "dcr", "kdc",
                "mdc", "mrw", "erf", "x3f", "gpr", "dng", "hif", "lri", "sti", "raw", "cam", "rev",
            ],
            FileFormat::PE => &["exe", "dll", "sys"],
            FileFormat::MKV => &["mkv"],
            FileFormat::WEBM => &["webm"],
            FileFormat::FLV => &["flv"],
            FileFormat::AVI => &["avi"],
            FileFormat::MTS => &["mts", "m2ts"],
            FileFormat::ASF => &["asf", "wmv", "wma"],
            FileFormat::MXF => &["mxf"],
            FileFormat::MP3 => &["mp3"],
            FileFormat::FLAC => &["flac"],
            FileFormat::AAC => &["aac", "m4a"],
            FileFormat::WAV => &["wav"],
            FileFormat::AIFF => &["aif", "aiff", "aifc"],
            FileFormat::OGG => &["ogg"],
            FileFormat::OPUS => &["opus"],
            FileFormat::APE => &["ape"],
            FileFormat::RAM => &["ram"],
            FileFormat::DSS => &["dss", "ds2"],
            FileFormat::ZIP => &["zip"],
            FileFormat::DOCX => &["docx"],
            FileFormat::XLSX => &["xlsx"],
            FileFormat::PPTX => &["pptx"],
            FileFormat::Pages => &["pages"],
            FileFormat::Numbers => &["numbers"],
            FileFormat::Keynote => &["key"],
            FileFormat::EPUB => &["epub"],
            FileFormat::RAR => &["rar"],
            FileFormat::SevenZ => &["7z"],
            FileFormat::ISO => &["iso"],
            FileFormat::TAR => &["tar"],
            FileFormat::GZ => &["gz"],
            FileFormat::TTF => &["ttf"],
            FileFormat::OTF => &["otf"],
            FileFormat::WOFF => &["woff"],
            FileFormat::WOFF2 => &["woff2"],
            FileFormat::AVIF => &["avif"],
            FileFormat::JXL => &["jxl"],
            FileFormat::BPG => &["bpg"],
            FileFormat::EXR => &["exr"],
            FileFormat::PFM => &["pfm"],
            FileFormat::HDR => &["hdr"],
            FileFormat::FLIF => &["flif"],
            FileFormat::XCF => &["xcf"],
            FileFormat::MIFF => &["miff", "mif"],
            FileFormat::SVG => &["svg"],
            FileFormat::ICO => &["ico"],
            FileFormat::PSD => &["psd"],
            FileFormat::WPG => &["wpg"],
            FileFormat::DJVU => &["djvu", "djv"],
            FileFormat::ELF => &["elf", "so"],
            FileFormat::MachO => &["dylib", "bundle"],
            FileFormat::DWG => &["dwg"],
            FileFormat::DXF => &["dxf"],
            FileFormat::STL => &["stl"],
            FileFormat::OBJ => &["obj"],
            FileFormat::GLTF => &["gltf", "glb"],
            FileFormat::FIT => &["fit"],
            FileFormat::FITS => &["fits"],
            FileFormat::HDF5 => &["h5", "hdf5"],
            FileFormat::VCF => &["vcf", "vcard"],
            FileFormat::ICS => &["ics", "ical"],
            FileFormat::EML => &["eml", "email"],
            FileFormat::TXT => &["txt", "text"],
            FileFormat::LFP => &["lfp", "lfr"],
            FileFormat::HTML => &["htm", "html", "xhtml"],
            FileFormat::LNK => &["lnk"],
            FileFormat::SQLite => &["db", "sqlite", "sqlite3"],
            FileFormat::Prefetch => &["pf"],
            FileFormat::Registry => &["dat"],
            FileFormat::EVTX => &["evtx"],
            FileFormat::Plist => &["plist"],
            FileFormat::OLE => &["doc", "xls", "ppt", "msg", "vsd", "pub"],
            FileFormat::PCAP => &["pcap", "cap"],
            FileFormat::PCAPNG => &["pcapng"],
            FileFormat::X509 => &["crt", "cer", "pem", "der"],
            FileFormat::ICC => &["icc", "icm"],
            FileFormat::XMP => &["xmp"],
            FileFormat::EPS => &["eps", "epsf", "ps"],
            FileFormat::VRD => &["vrd"],
            FileFormat::DR4 => &["dr4"],
            FileFormat::FPF => &["fpf"],
            FileFormat::DICOM => &["dcm", "dicom"],
            FileFormat::Unknown => &[],
        }
    }
}

impl std::fmt::Display for FileFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pe_format_name() {
        assert_eq!(FileFormat::PE.name(), "PE");
    }

    #[test]
    fn test_pe_format_extensions() {
        assert_eq!(FileFormat::PE.extensions(), &["exe", "dll", "sys"]);
    }
}
