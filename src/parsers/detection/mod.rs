//! Format detection via magic byte analysis
//!
//! This module provides format detection capabilities for determining file types
//! by examining magic bytes (file signatures) at the beginning of files.
//!
//! # Architectural Role
//!
//! The format detector is part of the **infrastructure layer** and serves as the
//! entry point for the parsing pipeline. It uses the `FileReader` port to read
//! magic bytes and returns a `FileFormat` enum variant that routes to the
//! appropriate format parser.
//!
//! # Supported Formats
//!
//! The detector currently identifies:
//! - JPEG: 0xFF 0xD8 0xFF
//! - TIFF (Little-Endian): 0x49 0x49 0x2A 0x00
//! - TIFF (Big-Endian): 0x4D 0x4D 0x00 0x2A
//! - PNG: 0x89 0x50 0x4E 0x47
//! - FLAC: 0x66 0x4C 0x61 0x43 ("fLaC")
//! - PDF: 0x25 0x50 0x44 0x46
//! - QuickTime/MP4: "ftyp" at bytes 4-7
//!
//! Unknown formats return `FileFormat::Unknown`.
//!
//! # Examples
//!
//! ```no_run
//! use oxidex::parsers::detection::detect_format;
//! use oxidex::io::MMapReader;
//! use std::path::Path;
//!
//! # fn example() -> std::io::Result<()> {
//! let reader = MMapReader::new(Path::new("image.jpg"))?;
//! let format = detect_format(&reader)?;
//! println!("Detected format: {}", format);
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

mod archive;
mod audio;
mod binary;
mod bmff;
mod camera;
mod helpers;
mod riff;
mod signatures;
pub(crate) mod text;
mod tiff;
mod video;
pub(crate) mod x509_der;

use crate::core::{FileFormat, FileReader};
use std::io;

// Re-export detection functions for internal use
use archive::detect_zip_variant;
use audio::{detect_ogg_variant, is_aac_adts, is_mp3_sync};
use binary::{detect_pe_format, is_dwg, is_macho};
use bmff::detect_bmff_variants;
use camera::detect_casio_cam;
use helpers::{matches_at_offset, utf8_prefix};
use riff::detect_riff_formats;
use signatures::SIMPLE_SIGNATURES;
use text::detect_text_formats;
use tiff::detect_tiff_variants;
use video::is_mts_stream;
use x509_der::{looks_like_der_x509, top_level_der_object_len};

pub(crate) const DER_X509_MAX_PROBE_SIZE: usize = 1024 * 1024;
pub(crate) const TEXT_FORMAT_PROBE_SIZE: usize = 64 * 1024;

/// Detects the file format by examining magic bytes.
///
/// This function reads the first 1024 bytes of the file (or fewer if the file is smaller)
/// and matches them against known format signatures using a combination of:
/// 1. Simple signature table lookup
/// 2. Specialized detection functions for complex formats
/// 3. Text-based format detection
///
/// Format detection is performed by checking byte sequences in order from most
/// specific to least specific to avoid false positives.
///
/// # Arguments
///
/// * `reader` - A file reader providing access to file contents via the FileReader port
///
/// # Returns
///
/// * `Ok(FileFormat)` - The detected format, or `FileFormat::Unknown` if unrecognized
/// * `Err(io::Error)` - An I/O error occurred while reading the file
///
/// # Error Handling
///
/// This function gracefully handles files smaller than 1024 bytes by reading only the
/// available bytes and attempting format detection with the partial data. Empty files
/// return `Ok(FileFormat::Unknown)`.
///
/// # Examples
///
/// ```no_run
/// use oxidex::parsers::detection::detect_format;
/// use oxidex::io::MMapReader;
/// use oxidex::core::FileFormat;
/// use std::path::Path;
///
/// # fn example() -> std::io::Result<()> {
/// let reader = MMapReader::new(Path::new("photo.jpg"))?;
/// let format = detect_format(&reader)?;
///
/// match format {
///     FileFormat::JPEG => println!("JPEG image detected"),
///     FileFormat::PNG => println!("PNG image detected"),
///     FileFormat::TIFF => println!("TIFF image detected"),
///     FileFormat::PDF => println!("PDF document detected"),
///     FileFormat::Unknown => println!("Unknown or unsupported format"),
///     _ => println!("Other format detected"),
/// }
/// # Ok(())
/// # }
/// ```
pub fn detect_format(reader: &dyn FileReader) -> io::Result<FileFormat> {
    // Read 1 KiB to align with EML validation while covering MTS's 3-packet probe.
    let magic_bytes = match reader.read(0, 1024) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            // File is smaller than 1024 bytes, read what's available
            let size = reader.size() as usize;
            if size == 0 {
                return Ok(FileFormat::Unknown);
            }
            reader.read(0, size)?
        }
        Err(e) => return Err(e),
    };

    // Empty file check
    if magic_bytes.is_empty() {
        return Ok(FileFormat::Unknown);
    }

    // Phase 1: Check complex formats that need special handling
    // These must be checked before simple signatures to ensure correct priority

    // TIFF and raw camera formats (many share similar signatures)
    if let Some(format) = detect_tiff_variants(magic_bytes) {
        return Ok(format);
    }

    // ISO Base Media File Format variants (ftyp-based)
    if let Some(format) = detect_bmff_variants(magic_bytes) {
        return Ok(format);
    }

    // RIFF-based formats (WAV, AVI, WebP)
    if let Some(format) = detect_riff_formats(magic_bytes) {
        return Ok(format);
    }

    // DjVu is an IFF container with a distinctive `AT&TFORM` prefix.  Its
    // root form must be a single-page image or multi-page document, matching
    // AIFF.pm's DjVu gate before it starts walking chunks.
    if magic_bytes.starts_with(b"AT&TFORM")
        && matches!(magic_bytes.get(12..16), Some(b"DJVU" | b"DJVM"))
    {
        return Ok(FileFormat::DJVU);
    }

    // AIFF/AIFC is the other half of AIFF.pm's magic number,
    // `^(FORM....AIF[FC]|AT&TFORM)`. The two alternatives cannot collide: DjVu
    // puts its `FORM` at offset 4 behind the `AT&T` prefix, AIFF at offset 0.
    // Testing it after the DjVu gate keeps that ordering explicit rather than
    // resting on the offset difference alone.
    if magic_bytes.starts_with(b"FORM") && matches!(magic_bytes.get(8..12), Some(b"AIFF" | b"AIFC"))
    {
        return Ok(FileFormat::AIFF);
    }

    // ZIP variants require archive inspection before offset-based signatures can claim
    // bytes that happen to appear inside ZIP headers, names, or payloads.
    if magic_bytes.starts_with(&[0x50, 0x4B]) {
        return Ok(detect_zip_variant(reader));
    }

    // FITS: `^SIMPLE  = {20}T`, the full 30-byte keyword record. This used to
    // be a `SIMPLE_SIGNATURES` entry testing the bare word "SIMPLE" (six
    // bytes) -- a plain-text file opening "SIMPLE ANSWER:" satisfied it and
    // was dispatched to `FITSParser` ahead of every other rule, since Phase 2
    // runs first. `matches_magic` reads the real 30-byte pattern, so it has
    // to run as its own check rather than live in the byte-literal table.
    if crate::filetype::matches_magic("FITS", magic_bytes) {
        return Ok(FileFormat::FITS);
    }

    // MIE: `~[\x10\x18]\x04.0MIE` (`ExifTool.pm:993`) mixes a two-byte
    // alternation with a wildcard byte, which `signature!`'s literal-bytes
    // table can't express (the same reason FITS is checked here rather than
    // there). `filetype::matches_magic` already carries this pattern
    // (generated from the same `%magicNumber` table `add_identity_tags`
    // reads for the ~40 formats with no parser), so MIE was already
    // correctly named `File:FileType` before this step -- just never routed
    // to `mie.rs`'s own parser.
    if crate::filetype::matches_magic("MIE", magic_bytes) {
        return Ok(FileFormat::MIE);
    }

    // PCX: `^\x0a[\x00-\x05]\x01[\x01\x02\x04\x08].{64}[\x00-\x02]`
    // (PCX.pm's own inline check, `ProcessPCX`). The two byte-range
    // alternations at offsets 1 and 3 are outside `signature!`'s literal-bytes
    // grammar, same reason FITS and MIE are checked here.
    if crate::filetype::matches_magic("PCX", magic_bytes) {
        return Ok(FileFormat::PCX);
    }

    // MRC: `^.{64}[\x01-\x03]\x00\x00\x00...MAP[\x00 ](\x44\x44|\x44\x41|\x11\x11)\x00\x00`
    // (ExifTool.pm's `%magicNumber`, generated from MRC.pm's header layout).
    // The alternation on the machine-stamp bytes at offset ~212 is outside
    // the literal-bytes grammar.
    if crate::filetype::matches_magic("MRC", magic_bytes) {
        return Ok(FileFormat::MRC);
    }

    // SWF: `^[FC]WS[^\x00]` (Flash.pm:599, `$buff =~ /^(F|C)WS([^\0])/`) --
    // the fourth byte (the version) must be present and non-zero, which is
    // outside `signature!`'s literal-bytes grammar the same way PCX's and
    // MRC's byte-range alternations are. Before this existed, a `.swf` file
    // had no route past `add_identity_tags`: a correct `File:FileType: SWF`
    // over zero of the header/stage/XMP tags `swf.rs`'s parser now reads.
    if crate::filetype::matches_magic("SWF", magic_bytes) {
        return Ok(FileFormat::SWF);
    }

    // PPM/PGM/PBM: `^P[1-6]\s+` (Other.pm's `ProcessPPM`, one shared
    // `FileType: PPM` for all three NetPBM ASCII/binary variants).
    if crate::filetype::matches_magic("PPM", magic_bytes) {
        return Ok(FileFormat::PPM);
    }

    // RealAudio (.ra) binary format: `^\.ra\xfd` (Real.pm:523's
    // `.ra\xfd` alternative -- the other three, `.RMF` (RealMedia) and the
    // URL-metafile prefixes (RAM/RPM), are handled elsewhere: `.RMF` is out
    // of this pass's scope, and the URL forms are matched by `ram_url` below.
    // Real.pm:565 reads the big-endian `u16` version right after this
    // 4-byte signature to select `AudioV3`/`AudioV4`/`AudioV5`.
    if magic_bytes.starts_with(b".ra\xfd") {
        return Ok(FileFormat::RA);
    }

    // MOI: `^V6` (MOI.pm's `ProcessMOI`, `$buff =~ /^V6/`). A two-byte magic
    // is weak on its own; `moi.rs`'s parser re-validates the 256-byte header
    // length and (when known) the embedded file-size field before accepting.
    if magic_bytes.starts_with(b"V6") {
        return Ok(FileFormat::MOI);
    }

    // Kyocera Contax N Digital RAW: `.{25}ARECOYK` -- the reversed ASCII
    // literal "KYOCERA" at byte offset 0x19 (`KyoceraRaw.pm:121`,
    // `substr($buff, 0x19, 7) eq 'ARECOYK'`). ExifTool's own magic number for
    // the shared `RAW` extension is `(.{25}ARECOYK|II|MM)`
    // (`ExifTool.pm`'s `%magicNumber`): this is the non-TIFF half of that
    // alternation, so it must run ahead of the plain-text/binary fallback the
    // same way MOI's `V6` does. Before this existed, a Kyocera `.raw` file
    // had no TIFF magic to trip the extension-gated `CameraRaw` override in
    // `core::operations`, fell through to `add_identity_tags`, and reported a
    // correct `File:FileType: RAW` over zero of its eleven real tags.
    if crate::parsers::raw::looks_like_kyocera_raw(magic_bytes) {
        return Ok(FileFormat::CameraRaw(
            crate::parsers::raw::RawFormat::GenericRAW,
        ));
    }

    // ITC: `^.{4}itch` -- iTunes Cover Flow's first block is always an
    // `itch` header box (ITC.pm's `ProcessITC`).
    if magic_bytes.len() >= 8 && &magic_bytes[4..8] == b"itch" {
        return Ok(FileFormat::ITC);
    }

    // PGF: `^PGF` (PGF.pm's `ProcessPGF`, `$buff =~ /^PGF(.)/`).
    if magic_bytes.starts_with(b"PGF") {
        return Ok(FileFormat::PGF);
    }

    // AA: `^.{4}\x57\x90\x75\x36` (Audible.pm's `ProcessAA`) -- a 4-byte
    // magic number sitting after the file's own big-endian size field.
    if magic_bytes.len() >= 8 && &magic_bytes[4..8] == b"\x57\x90\x75\x36" {
        return Ok(FileFormat::AA);
    }

    // R3D: `^\x00\x00..RED(1|2)` (Red.pm:225's own `ProcessR3D` check). The
    // two free size bytes at offsets 2-3 and the version alternation are
    // outside `signature!`'s literal-bytes grammar, same reason MRC and PCX
    // are checked here.
    if magic_bytes.len() >= 8
        && magic_bytes[0] == 0
        && magic_bytes[1] == 0
        && &magic_bytes[4..7] == b"RED"
        && matches!(magic_bytes[7], b'1' | b'2')
    {
        return Ok(FileFormat::R3D);
    }

    // PMP: `^.{8}\x00{3}\x7c.{112}\xff\xd8\xff\xdb` (Sony.pm:11374's own
    // `ProcessPMP` check). Two fixed byte groups at offsets 8 and 124 with
    // free bytes between them -- outside `signature!`'s single-run grammar,
    // same reason MRC and PCX are checked here.
    if magic_bytes.len() >= 128
        && magic_bytes[8..12] == [0, 0, 0, 0x7c]
        && magic_bytes[124..128] == [0xff, 0xd8, 0xff, 0xdb]
    {
        return Ok(FileFormat::PMP);
    }

    // DV: `^\x1f\x07\x00[\x3f\xbf]` (ExifTool.pm's `%magicNumber`, the same
    // pattern DV.pm:158 scans for). The alternation on byte 3 is outside
    // `signature!`'s literal-bytes grammar.
    if magic_bytes.len() >= 4
        && magic_bytes[0] == 0x1f
        && magic_bytes[1] == 0x07
        && magic_bytes[2] == 0x00
        && matches!(magic_bytes[3], 0x3f | 0xbf)
    {
        return Ok(FileFormat::DV);
    // MacOS `._` sidecar: `\0\x05\x16\x07\0.\0\0Mac OS X        `
    // (ExifTool.pm:992's `%magicNumber`) -- the AppleDouble magic with a
    // wildcard version byte at offset 5, which `signature!`'s literal-bytes
    // table cannot express. MacOS.pm:706 makes the same test.
    if crate::filetype::matches_magic("MacOS", magic_bytes) {
        return Ok(FileFormat::MacOSSidecar);
    }

    // INDD/IND: the 16-byte master-page GUID at offset 0 (InDesign.pm:25,
    // and ExifTool.pm's `%magicNumber` `IND` entry). `indesign.rs` repeats
    // the test and goes on to validate the second master page the way
    // InDesign.pm:55 does.
    if magic_bytes.starts_with(b"\x06\x06\xed\xf5\xd8\x1d\x46\xe5\xbd\x31\xef\xe7\xfe\x74\xb7\x1d")
    {
        return Ok(FileFormat::INDD);
    }

    // PFB/PFA: `^(.{6})?%!(PS-(AdobeFont-|Bitstream )|FontType1-)`
    // (Font.pm:840) -- a PostScript Type 1 font program, optionally behind a
    // six-byte PFB segment header. Tested before the `%!PS` signature that
    // would otherwise claim it for the EPS/PS parser, which is the order
    // ExifTool uses: `Font::ProcessFont` reaches this arm and only then hands
    // off to `PostScript::ProcessPS` with `Font::PSInfo` bound as a second
    // table (PostScript.pm:452-457).
    if crate::parsers::font::pfb::is_type1_font_program(magic_bytes) {
        return Ok(FileFormat::PFB);
    }

    // PDB/MOBI: `^.{60}(\.pdfADBE|TEXtREAd|...)` (ExifTool.pm's
    // `%magicNumber`, generated from Palm.pm:23-52's `%palmTypes`) -- a
    // 28-way alternation on the type/creator pair at offset 60, outside
    // `signature!`'s literal-bytes grammar. Palm.pm:294-295 makes the same
    // test the file's accept/reject gate, and `palm.rs` repeats it.
    if crate::filetype::matches_magic("PDB", magic_bytes) {
        return Ok(FileFormat::PalmDB);
    }

    // Torrent: `^d\d+:\w+` (ExifTool.pm's `%magicNumber`) -- a bencoded
    // dictionary whose first key is a byte string. The `\d+`/`\w+` runs are
    // outside `signature!`'s literal-bytes grammar, same reason FITS, MIE,
    // PCX and MRC are checked here. `torrent.rs` re-validates by requiring
    // the decoded root dictionary to carry `announce`, `created by` or
    // `info` (Torrent.pm:286) before accepting the file.
    if crate::filetype::matches_magic("Torrent", magic_bytes) {
        return Ok(FileFormat::Torrent);
    }

    // Phase 2: Check simple signatures from lookup table
    for sig in SIMPLE_SIGNATURES {
        let end = sig.offset as usize + sig.bytes.len();
        if sig.offset == 0 {
            // Optimization: most signatures are at offset 0
            if magic_bytes.starts_with(sig.bytes) {
                return Ok(sig.format);
            }
        } else if end <= magic_bytes.len() {
            if matches_at_offset(magic_bytes, sig.bytes, sig.offset as usize) {
                return Ok(sig.format);
            }
        } else if reader.size() >= end as u64 {
            // The signature lives past the 1 KiB probe, so the probe cannot
            // decide it: `matches_at_offset` returns false whenever the
            // pattern would run off the end of the buffer, which reads
            // exactly like "no match" and silently retired ISO 9660's
            // `CD001` at 32769 -- declared in the table, never reachable,
            // so ISO files resolved to Unknown and never met their parser.
            //
            // Read only the signature's own bytes rather than widening the
            // probe for every file: this costs one short seek, and only for
            // signatures the probe could not have covered anyway.
            if let Ok(probe) = reader.read(sig.offset as u64, sig.bytes.len())
                && probe == sig.bytes
            {
                return Ok(sig.format);
            }
        }
    }

    // DER X.509 certificates share ASN.1 SEQUENCE prefixes with many formats, so inspect the
    // declared top-level object only. Cap the object size to avoid unbounded reads while allowing
    // large but ordinary certificates.
    if magic_bytes.first() == Some(&0x30) {
        if let Some(der_object_len) = top_level_der_object_len(magic_bytes)
            && der_object_len <= DER_X509_MAX_PROBE_SIZE
            && der_object_len as u64 == reader.size()
        {
            let der_probe = if der_object_len > magic_bytes.len() {
                reader.read(0, der_object_len)?
            } else {
                &magic_bytes[..der_object_len]
            };

            if looks_like_der_x509(der_probe) {
                return Ok(FileFormat::X509);
            }
        }
    }

    // Phase 3: Check formats with special detection logic

    // OGG/Opus (already checked in table, but need variant detection)
    if magic_bytes.starts_with(b"OggS")
        && let Some(format) = detect_ogg_variant(magic_bytes)
    {
        return Ok(format);
    }

    // MP3 (MPEG sync pattern, not in simple table due to bit masking)
    if is_mp3_sync(magic_bytes) {
        return Ok(FileFormat::MP3);
    }

    // AAC (ADTS sync pattern)
    if is_aac_adts(magic_bytes) {
        return Ok(FileFormat::AAC);
    }

    // MTS/M2TS (transport stream sync pattern)
    if is_mts_stream(magic_bytes) {
        return Ok(FileFormat::MTS);
    }

    // PE format (requires DOS stub validation)
    if let Some(format) = detect_pe_format(magic_bytes, reader) {
        return Ok(format);
    }

    // Mach-O (multiple magic numbers)
    if is_macho(magic_bytes) {
        return Ok(FileFormat::MachO);
    }

    // DWG (version-based signature)
    if is_dwg(magic_bytes) {
        return Ok(FileFormat::DWG);
    }

    // Portable FloatMap (PFM): "P" + F/f + LF + "<width> <height>" + LF +
    // "<scale>" + LF. ExifTool's magic regex (Other.pm):
    // ^P[Ff]\x0a\d+ \d+\x0a[-+0-9.]+\x0a
    if crate::parsers::image::pfm::looks_like_pfm(magic_bytes) {
        return Ok(FileFormat::PFM);
    }

    // Windows Printer Font Metrics, the *other* format behind `FileType: PFM`
    // (Font.pm:844-853). Disjoint from the FloatMap magic above -- a FloatMap
    // file opens with `P`, this one with `\0\x01`/`\0\x02` -- so the order of
    // the two is ExifTool's (`%fileTypeLookup{pfm} => [['Font','PFM2'], ...]`
    // tries Font first) rather than a tie-break.
    //
    // Content detection is required, not merely preferred: `detect_format`
    // never sees a filename, so before this a Printer Font Metrics file
    // resolved to `FileFormat::Unknown` and bottomed out in
    // `add_identity_tags` -- a correct `File:FileType: PFM` over zero real
    // tags. ExifTool's own acceptance test is content-based too, and every
    // clause of it is reproduced in `verify_signature`: the self-describing
    // size at offset 2 and the `"PostScript\0"` string at the offset named by
    // offset 101 are what make this specific enough to run this early.
    if crate::parsers::font::pfm::PrinterFontMetricsParser::verify_signature(reader) {
        return Ok(FileFormat::PFM);
    }

    // Radiance RGBE (HDR): `#?RADIANCE` or `#?RGBE` on the first line. It has
    // to outrank the text rules below and the plain-text fallback -- the
    // header is ASCII, so `is_likely_text` accepts it, and the file reported
    // TEXT statistics ExifTool never reports for an image.
    if crate::parsers::image::radiance::looks_like_radiance(magic_bytes) {
        return Ok(FileFormat::HDR);
    }

    // Transport Neutral Encapsulation Format (TNEF / winmail.dat).  The
    // signature is the little-endian 0x223e9f78 key, followed by the TNEF
    // version attribute header at byte 6 (TNEF.pm:406-409).
    if magic_bytes.len() >= 10
        && magic_bytes.starts_with(b"\x78\x9f\x3e\x22")
        && magic_bytes[6..10] == [0x01, 0x06, 0x90, 0x08]
    {
        return Ok(FileFormat::TNEF);
    }

    // JPEG 2000's two wire forms: the JP2 signature box and a bare J2C
    // codestream beginning with SOC then SIZ.  `Jpeg2000.pm:1538-1557`
    // makes exactly this distinction before dispatching the box/marker walk.
    if magic_bytes.starts_with(b"\x00\x00\x00\x0cjP  \r\n\x87\n")
        || magic_bytes.starts_with(b"\x00\x00\x00\x0cjP\x1a\x1a\r\n\x87\n")
        || magic_bytes.starts_with(b"\xff\x4f\xff\x51")
    {
        return Ok(FileFormat::Jpeg2000);
    }

    // SVG must outrank ICS/EML text heuristics, but only when SVG is the XML
    // root element. Email bodies may legitimately embed SVG markup.
    if looks_like_svg_root(magic_bytes) {
        return Ok(FileFormat::SVG);
    }

    if looks_like_xml_plist_root(magic_bytes) {
        return Ok(FileFormat::Plist);
    }

    // HTML and XHTML, using ExifTool's own gate from `HTML.pm`'s ProcessHTML.
    // It runs after the SVG and plist roots because those three share the
    // `<?xml` opening: ExifTool requires an actual HTML element in the first
    // 256 bytes before it will accept an XML declaration as HTML, which is
    // what keeps SVG, XMP sidecars and XML plists out. Before this existed
    // every .html file fell through to the plain-text fallback and was parsed
    // as TXT, reporting five TEXT:* statistics ExifTool never reports and none
    // of the 57 HTML/Dublin-Core/Office tags it does.
    if crate::parsers::text::html::looks_like_html(magic_bytes) {
        return Ok(FileFormat::HTML);
    }

    // An XMP sidecar written without the `x:xmpmeta` wrapper. The signature
    // table catches `<?xpacket` and `<x:xmpmeta` roots, but not this one, so
    // `XMP.xml` fell through to the plain-text fallback: 14 tags where
    // ExifTool reports 82, under `FileType: TXT`.
    if looks_like_rdf_root(magic_bytes) {
        return Ok(FileFormat::XMP);
    }

    // A plain XML document -- not XMP, RDF, SVG, PLIST, INX or RMD, all of
    // which the gates above have already claimed. ExifTool routes it to
    // `XMP::ProcessXMP` all the same and walks it as schema-less XMP
    // (XMP.pm:4425-4427); `filetype::is_plain_xml` is that same decision,
    // already transcribed for the identification layer, so this reuses it
    // rather than writing a second copy that could disagree.
    //
    // Like the SVG/plist/HTML gates, this must outrank the plain-text
    // fallback below: an XML document is printable text, so `is_likely_text`
    // accepts it, and Geotag.gpx/.kml/.xml were parsed as TXT -- four TEXT
    // statistics ExifTool never reports, and none of the up-to-42 track-point
    // tags it does.
    if crate::filetype::is_plain_xml(magic_bytes) {
        return Ok(FileFormat::XML);
    }

    // Text-based formats need a wider bounded probe for long ICS bodies and EML header blocks.
    let text_probe_len = reader
        .size()
        .min(TEXT_FORMAT_PROBE_SIZE as u64)
        .try_into()
        .unwrap_or(TEXT_FORMAT_PROBE_SIZE);
    let text_probe = if text_probe_len > magic_bytes.len() {
        reader.read(0, text_probe_len)?
    } else {
        &magic_bytes[..text_probe_len]
    };
    if let Some(format) = detect_text_formats(text_probe) {
        return Ok(format);
    }

    // Casio CAM (JPEG at offset 70)
    if let Some(format) = detect_casio_cam(magic_bytes, reader) {
        return Ok(format);
    }

    // JPEG (checked late due to Casio CAM sharing similar pattern)
    if magic_bytes.len() >= 3 && magic_bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok(FileFormat::JPEG);
    }

    // JXL (second variant with longer signature)
    if magic_bytes.len() >= 12
        && matches_at_offset(
            magic_bytes,
            &[0x00, 0x00, 0x00, 0x0C, 0x4A, 0x58, 0x4C, 0x20],
            0,
        )
    {
        return Ok(FileFormat::JXL);
    }

    // RealAudio metafiles identify themselves by a streaming protocol in the
    // first record. This must precede the generic text fallback so a RAM URL
    // reaches its Real-specific extractor rather than the TXT parser.
    if crate::parsers::audio::ram::ram_url(magic_bytes).is_some() {
        return Ok(FileFormat::RAM);
    }

    // PICT: `^(.{10}|.{522})(\x11\x01|\x00\x11)` (PICT.pm's own magic,
    // ExifTool.pm's `%magicNumber`) -- a version opcode at one of two
    // possible offsets depending on whether the file carries the older
    // 512-byte all-zero header. Unlike every check above, this one is not
    // anchored at offset 0: it is only two non-wildcard bytes, ten or 522
    // bytes into the file, which is weak enough to false-positive against
    // arbitrary binary content at that offset (a `.jpg`'s EXIF payload
    // tripped it when this ran early, misrouting the whole file to the PICT
    // parser and losing every real JPEG tag). Real ExifTool never has this
    // collision because it only tries a type's magic number among the
    // candidates its own extension names; `detect_format` has no filename to
    // narrow with, so the next-best mitigation is running this dead last,
    // after every stronger, offset-0-anchored signature above has had its
    // chance -- and the `w > 0 && h > 0` bounding-rect check inside
    // `pict::parse_pict_metadata` itself is a second gate past this one.
    if crate::filetype::matches_magic("PICT", magic_bytes)
        && crate::parsers::image::pict::parse_pict_metadata(reader).is_ok()
    {
        return Ok(FileFormat::PICT);
    }

    // Plain text detection (fallback for files that look like text)
    // Check if most bytes are printable ASCII or valid UTF-8
    if is_likely_text(magic_bytes) {
        return Ok(FileFormat::TXT);
    }

    // No known format matched
    Ok(FileFormat::Unknown)
}

/// Advance past the XML declaration, comments and DOCTYPE to the root element.
///
/// `None` when one of those is unterminated inside the probe. That is not the
/// same as "no root element": it means the root is not visible from here, and
/// the callers all treat it as a decline.
fn xml_root_element(data: &[u8]) -> Option<&str> {
    // The probe may cut a trailing multibyte character; judge the valid prefix.
    let mut text = utf8_prefix(data);
    text = text.strip_prefix('\u{feff}').unwrap_or(text);

    loop {
        text = text.trim_start();

        if let Some(rest) = text.strip_prefix("<?xml") {
            let end = rest.find("?>")?;
            text = &rest[end + 2..];
            continue;
        }

        if let Some(rest) = text.strip_prefix("<!--") {
            let end = rest.find("-->")?;
            text = &rest[end + 3..];
            continue;
        }

        if text.starts_with("<!DOCTYPE") {
            let end = xml_doctype_end(text)?;
            text = &text[end + 1..];
            continue;
        }

        return Some(text);
    }
}

/// Whether `name` opens the root element, followed by a real name terminator.
fn root_element_is(data: &[u8], name: &str) -> bool {
    let Some(text) = xml_root_element(data) else {
        return false;
    };
    let Some(rest) = text.strip_prefix(name) else {
        return false;
    };
    rest.chars()
        .next()
        .is_none_or(|character| character.is_whitespace() || matches!(character, '>' | '/'))
}

pub(crate) fn looks_like_svg_root(data: &[u8]) -> bool {
    root_element_is(data, "<svg")
}

/// Whether the document's root element is `<rdf:RDF>`.
///
/// XMP.pm recognises this as XMP in its own right (XMP.pm:4393-4394):
///
/// ```text
///     } elsif ($buf2 =~ /<rdf:RDF/) {
///         $isRDF = 1;     # recognize XMP without x:xmpmeta element
/// ```
///
/// ExifTool searches its whole read-ahead buffer, while this requires the RDF
/// element to be the document root. The difference is a deliberate
/// under-claim: an arbitrary XML document that merely *contains* an RDF island
/// deeper down stays XML here rather than being renamed XMP on the strength of
/// a substring. `XMP.xml` -- the file this exists for -- opens `<?xml ...?>`
/// then `<rdf:RDF`, and is XMP by either reading.
///
/// Runs after the SVG, plist and HTML roots, which is ExifTool's order too:
/// all four share the `<?xml` opening, and the RDF test is the last `elsif` in
/// XMP.pm's chain.
fn looks_like_rdf_root(data: &[u8]) -> bool {
    root_element_is(data, "<rdf:RDF")
}

fn looks_like_xml_plist_root(data: &[u8]) -> bool {
    let data = &data[..data.len().min(512)];
    // The 512-byte cut may split a multibyte character; judge the valid prefix.
    let text = utf8_prefix(data);

    let Some(rest) = text.strip_prefix("<?xml") else {
        return false;
    };
    let Some(end) = rest.find("?>") else {
        return false;
    };

    let mut text = &rest[end + 2..];
    loop {
        text = text.trim_start();

        if let Some(rest) = text.strip_prefix("<!--") {
            let Some(end) = rest.find("-->") else {
                return false;
            };
            text = &rest[end + 3..];
            continue;
        }

        if text.starts_with("<!DOCTYPE") {
            let Some(end) = xml_doctype_end(text) else {
                return false;
            };
            text = &text[end + 1..];
            continue;
        }

        break;
    }

    let Some(after_plist) = text.strip_prefix("<plist") else {
        return false;
    };
    after_plist
        .chars()
        .next()
        .is_none_or(|character| character.is_whitespace() || matches!(character, '>' | '/'))
}

fn xml_doctype_end(text: &str) -> Option<usize> {
    let mut quote = None;
    let mut subset_depth = 0u32;

    for (index, character) in text.char_indices() {
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
            continue;
        }

        match character {
            '"' | '\'' => quote = Some(character),
            '[' => subset_depth += 1,
            ']' => subset_depth = subset_depth.saturating_sub(1),
            '>' if subset_depth == 0 => return Some(index),
            _ => {}
        }
    }

    None
}

/// Checks if data is likely to be plain text
///
/// Uses heuristics to determine if the data consists primarily of
/// printable characters and valid text encodings.
///
/// # Arguments
///
/// * `data` - Data to check
///
/// # Returns
///
/// `true` if data appears to be text, `false` otherwise
fn is_likely_text(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }

    // Check for UTF-8 BOM
    if data.len() >= 3 && &data[0..3] == b"\xEF\xBB\xBF" {
        return true;
    }

    // Check for UTF-16 BOM
    if data.len() >= 2 && (&data[0..2] == b"\xFF\xFE" || &data[0..2] == b"\xFE\xFF") {
        return true;
    }

    // Judge the valid UTF-8 prefix so a probe cut through a multibyte character
    // does not disqualify the buffer, and count multibyte characters as
    // printable text. A UTF-8 character is at most 4 bytes, so a probe cut can
    // strand at most 3 bytes; a longer invalid tail means genuinely non-UTF-8
    // data, which keeps the pre-existing strictness for mixed binary content.
    let text = utf8_prefix(data);
    if text.is_empty() || data.len() - text.len() > 3 {
        // Not UTF-8. It may still be single-byte 8-bit text (Latin-1,
        // MacRoman, any legacy code page), which the UTF-8 gate above can
        // never admit: one 0xE9 in an 18-byte Latin-1 file strands 13 bytes
        // outside the valid prefix and reads exactly like binary. Every such
        // file resolved to Unknown, so the dispatched TXT parser -- which
        // reports MIMEEncoding, Newlines, LineCount and WordCount -- never
        // ran on any non-UTF-8 text file at all.
        return is_likely_eight_bit_text(data);
    }

    let printable_count = text
        .chars()
        .filter(|&character| !character.is_control() || matches!(character, '\t' | '\n' | '\r'))
        .count();
    let total_count = text.chars().count() + (data.len() - text.len());

    // If at least 95% of characters are printable, consider it text
    let ratio = printable_count as f64 / total_count as f64;
    ratio >= 0.95
}

/// Control bytes that disqualify a buffer from being single-byte text.
///
/// Mirrors ExifTool's `Text.pm` `ProcessTXT` gate
/// (`/([\0-\x06\x0e-\x1a\x1c-\x1f\x7f])/`): a buffer holding any of these is
/// binary, or multi-byte Unicode that is only recognised by its BOM.
fn is_binary_control_byte(byte: u8) -> bool {
    matches!(byte, 0x00..=0x06 | 0x0E..=0x1A | 0x1C..=0x1F | 0x7F)
}

/// Checks whether a non-UTF-8 buffer looks like single-byte 8-bit text.
///
/// Two conditions, both needed:
///
/// 1. No binary control bytes, per ExifTool's rule above.
/// 2. A majority of the bytes are printable ASCII. High bytes alone satisfy
///    condition 1 -- a run of 0xFF passes it -- so this is what separates
///    8-bit *text* from high-entropy binary that merely happens to avoid the
///    control range.
fn is_likely_eight_bit_text(data: &[u8]) -> bool {
    if data.is_empty() || data.iter().copied().any(is_binary_control_byte) {
        return false;
    }

    let printable_ascii = data
        .iter()
        .filter(|&&byte| byte.is_ascii_graphic() || matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
        .count();

    printable_ascii * 2 >= data.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// ISO 9660 declares `CD001` at 32769, far past the 1 KiB probe. The
    /// table entry existed all along; nothing could reach it, so every ISO
    /// resolved to Unknown and its parser was dead code.
    #[test]
    fn test_detect_signature_beyond_the_probe_window() {
        let mut data = vec![0u8; 40960];
        data[32769..32774].copy_from_slice(b"CD001");
        let reader = TestReader::new(data);
        assert_eq!(detect_format(&reader).unwrap(), FileFormat::ISO);
    }

    /// The deep read must not invent a match for a file too short to hold
    /// the signature -- a truncated image is Unknown, not ISO.
    #[test]
    fn test_deep_signature_not_claimed_when_file_is_too_short() {
        let mut data = vec![0u8; 2048];
        data[0..5].copy_from_slice(b"CD001"); // right bytes, wrong offset
        let reader = TestReader::new(data);
        assert_ne!(detect_format(&reader).unwrap(), FileFormat::ISO);
    }

    /// `is_dwg`'s old range check (`header[2] >= b'1' && header[3] >= b'0'`)
    /// accepted any byte at or past those values, so plain text opening
    /// "ACTION" or "ACCESS" was dispatched to `DWGParser` ahead of every text
    /// rule -- and `File:FileType`, read from the same magic table
    /// `is_dwg` now asks, never agreed it was DWG.
    #[test]
    fn prose_opening_action_is_not_dwg() {
        let mut data = b"ACTION_ITEMS: buy milk, walk the dog, review the PR by Friday.\n".to_vec();
        data.resize(64, b'\n');
        let reader = TestReader::new(data);
        assert_ne!(detect_format(&reader).unwrap(), FileFormat::DWG);
    }

    #[test]
    fn a_real_dwg_version_string_is_still_dwg() {
        let mut data = b"AC1015".to_vec();
        data.push(0);
        data.resize(64, 0);
        let reader = TestReader::new(data);
        assert_eq!(detect_format(&reader).unwrap(), FileFormat::DWG);
    }

    /// The old `SIMPLE_SIGNATURES` entry tested the bare word "SIMPLE" (six
    /// bytes) rather than ExifTool's full 30-byte keyword record, so a
    /// plain-text file opening "SIMPLE ANSWER:" was dispatched to
    /// `FITSParser` -- and Phase 2's simple-signature scan runs ahead of
    /// every text rule, so nothing downstream could correct it.
    #[test]
    fn prose_opening_simple_is_not_fits() {
        let mut data = b"SIMPLE ANSWER: always test your assumptions before shipping.\n".to_vec();
        data.resize(64, b'\n');
        let reader = TestReader::new(data);
        assert_ne!(detect_format(&reader).unwrap(), FileFormat::FITS);
    }

    #[test]
    fn a_real_fits_keyword_record_is_still_fits() {
        // ExifTool's magic: `^SIMPLE  = {20}T` -- "SIMPLE", two spaces, "=",
        // twenty spaces, "T". `matches_magic` reads this straight from the
        // pattern rather than the record's 80-column card layout, so a
        // shorter buffer that merely opens with it is enough.
        let mut data = b"SIMPLE  =                    T".to_vec();
        data.resize(80, b' ');
        let reader = TestReader::new(data);
        assert_eq!(detect_format(&reader).unwrap(), FileFormat::FITS);
    }

    /// HTML shares the `<?xml` opening with SVG, XMP and XML plists, so the
    /// order of these four checks is load-bearing: every one of them must keep
    /// resolving to its own format once HTML joins the list.
    #[test]
    fn test_xml_rooted_formats_keep_their_own_detection() {
        let cases: &[(&[u8], FileFormat)] = &[
            (
                b"<?xml version=\"1.0\" encoding=\"iso-8859-1\"?>\n\
                  <!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.0 Transitional//EN\">\n\
                  <html><head><title>t</title></head></html>\n",
                FileFormat::HTML,
            ),
            (
                b"<html>\n<head><meta name=\"author\" content=\"x\"></head>\n</html>\n",
                FileFormat::HTML,
            ),
            (
                b"<?xml version=\"1.0\" standalone=\"yes\"?>\n<svg width=\"4in\"></svg>\n",
                FileFormat::SVG,
            ),
            (
                b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                  <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"x\">\n\
                  <plist version=\"1.0\"><dict/></plist>\n",
                FileFormat::Plist,
            ),
            (
                b"<?xpacket begin=\"\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
                  <x:xmpmeta xmlns:x=\"adobe:ns:meta/\"></x:xmpmeta>\n",
                FileFormat::XMP,
            ),
            (
                // Names no HTML element, so it must not be claimed as HTML --
                // and an RDF root is XMP written without the `x:xmpmeta`
                // wrapper, which XMP.pm recognises in its own right. This
                // shape is `XMP.xml`, which used to land on the plain-text
                // path and report 14 tags where ExifTool reports 82.
                b"<?xml version='1.0' encoding='UTF-8'?>\n\
                  <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n\
                  </rdf:RDF>\n",
                FileFormat::XMP,
            ),
            (
                // An RDF island *inside* another document is not an XMP
                // sidecar: the root element decides.
                b"<?xml version='1.0'?>\n<catalog>\n\
                  <rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'/>\n\
                  </catalog>\n",
                FileFormat::TXT,
            ),
        ];
        for (data, expected) in cases {
            let reader = TestReader::new(data.to_vec());
            assert_eq!(
                detect_format(&reader).unwrap(),
                *expected,
                "misdetected: {}",
                String::from_utf8_lossy(&data[..data.len().min(60)])
            );
        }
    }

    #[test]
    fn test_detect_svg_with_multibyte_char_straddling_probe_boundary() {
        // An SVG whose 1 KiB probe cut splits a multibyte character must still
        // be detected from its valid UTF-8 prefix.
        let mut data = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\"><text>");
        while data.len() < 1023 {
            data.push('x');
        }
        data.truncate(1023);
        data.push('\u{e9}'); // two-byte char at bytes 1023..1025
        data.push_str("</text></svg>");

        let reader = TestReader::new(data.into_bytes());
        assert_eq!(detect_format(&reader).unwrap(), FileFormat::SVG);
    }

    /// Single-byte 8-bit text is not valid UTF-8, so the UTF-8 gate rejected
    /// it and every Latin-1 or MacRoman .txt resolved to Unknown -- the TXT
    /// parser was dispatched but never reached by any of them.
    #[test]
    fn test_detect_latin1_text() {
        let reader = TestReader::new(b"this \xe9 is Latin1\r\n".to_vec());
        assert_eq!(detect_format(&reader).unwrap(), FileFormat::TXT);
    }

    #[test]
    fn test_detect_macroman_text() {
        // 0x8E lands in the C1 range, which is what makes this
        // unknown-8bit rather than iso-8859-1.
        let reader = TestReader::new(b"this \x8e is MacRoman\r".to_vec());
        assert_eq!(detect_format(&reader).unwrap(), FileFormat::TXT);
    }

    /// A web URL is ordinary text unless it names one of the Real media
    /// resources accepted by ExifTool's `Real.pm` metafile gate.
    #[test]
    fn http_text_without_real_media_suffix_stays_txt() {
        let reader = TestReader::new(b"http://example.test/index.html\n".to_vec());
        assert_eq!(detect_format(&reader).unwrap(), FileFormat::TXT);
    }

    /// High bytes alone clear ExifTool's control-character gate, so the
    /// 8-bit path must not claim binary that merely avoids that range.
    #[test]
    fn test_high_byte_run_is_not_claimed_as_text() {
        let reader = TestReader::new(vec![0xFF; 512]);
        assert_eq!(detect_format(&reader).unwrap(), FileFormat::Unknown);
    }

    #[test]
    fn test_detect_txt_with_non_ascii_tail_in_probe() {
        // Multibyte UTF-8 content within the probe window must count as text.
        let mut data = String::new();
        while data.len() < 600 {
            data.push_str("plain english text ");
        }
        while data.len() < 1100 {
            data.push_str("\u{43f}\u{440}\u{438}\u{432}\u{435}\u{442} "); // Cyrillic
        }

        let reader = TestReader::new(data.into_bytes());
        assert_eq!(detect_format(&reader).unwrap(), FileFormat::TXT);
    }

    #[test]
    fn test_detect_ics_larger_than_probe_with_multibyte_at_cut() {
        // Calendars larger than the text probe window must still be detected
        // when the probe cut splits a multibyte character.
        let mut data = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nDESCRIPTION:");
        while data.len() < TEXT_FORMAT_PROBE_SIZE - 1 {
            data.push('x');
        }
        // Two-byte char spanning the probe cut at TEXT_FORMAT_PROBE_SIZE.
        data.push('\u{e9}');
        assert_eq!(data.len(), TEXT_FORMAT_PROBE_SIZE + 1);
        data.push_str("\r\nEND:VCALENDAR\r\n");

        let reader = TestReader::new(data.into_bytes());
        assert_eq!(detect_format(&reader).unwrap(), FileFormat::ICS);
    }

    #[test]
    fn test_detect_jpeg() {
        let data = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::JPEG);
    }

    #[test]
    fn test_detect_tiff_little_endian() {
        let data = vec![0x49, 0x49, 0x2A, 0x00, 0x08, 0x00, 0x00, 0x00];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_detect_tiff_big_endian() {
        let data = vec![0x4D, 0x4D, 0x00, 0x2A, 0x00, 0x00, 0x00, 0x08];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_detect_png() {
        let data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PNG);
    }

    #[test]
    fn test_detect_pdf() {
        let data = vec![0x25, 0x50, 0x44, 0x46, 0x2D, 0x31, 0x2E, 0x34];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PDF);
    }

    #[test]
    fn test_detect_unknown() {
        let data = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_empty_file() {
        let data = vec![];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_file_too_small_one_byte() {
        let data = vec![0xFF];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_file_too_small_two_bytes() {
        let data = vec![0xFF, 0xD8];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_short_file_matches_jpeg() {
        let data = vec![0xFF, 0xD8, 0xFF];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::JPEG);
    }

    #[test]
    fn test_short_file_matches_pdf() {
        let data = vec![0x25, 0x50, 0x44, 0x46];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PDF);
    }

    #[test]
    fn test_jpeg_with_padding() {
        let mut data = vec![0xFF, 0xD8, 0xFF, 0xE1];
        data.extend_from_slice(&[0x00; 20]);
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::JPEG);
    }

    #[test]
    fn test_tiff_little_endian_minimal() {
        let data = vec![0x49, 0x49, 0x2A, 0x00];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_tiff_big_endian_minimal() {
        let data = vec![0x4D, 0x4D, 0x00, 0x2A];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::TIFF);
    }

    #[test]
    fn test_png_full_signature() {
        let data = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PNG);
    }

    #[test]
    fn test_partial_match_not_detected() {
        let data = vec![0xFF, 0xD8, 0x00, 0x00];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }

    #[test]
    fn test_pdf_with_version() {
        let data = vec![0x25, 0x50, 0x44, 0x46, 0x2D, 0x31, 0x2E, 0x37, 0x0A];
        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PDF);
    }

    #[test]
    fn test_detect_pe_mz_signature() {
        let mut data = vec![0x4D, 0x5A];
        data.extend_from_slice(&[0x90, 0x00]);
        data.extend_from_slice(&[0x03, 0x00]);
        data.resize(0x3C, 0x00);
        data.extend_from_slice(&[0x80, 0x00, 0x00, 0x00]);
        data.resize(0x80, 0x00);
        data.extend_from_slice(&[0x50, 0x45, 0x00, 0x00]);

        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PE);
    }

    #[test]
    fn test_detect_pe_with_nt_signature() {
        let mut data = vec![0x4D, 0x5A];
        data.resize(0x3C, 0x00);
        data.extend_from_slice(&[0x40, 0x00, 0x00, 0x00]);
        data.resize(0x40, 0x00);
        data.extend_from_slice(&[0x50, 0x45, 0x00, 0x00]);

        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::PE);
    }

    #[test]
    fn test_detect_non_pe_mz_file() {
        let mut data = vec![0x4D, 0x5A];
        data.resize(64, 0x00);

        let reader = TestReader::new(data);
        let format = detect_format(&reader).unwrap();
        assert_eq!(format, FileFormat::Unknown);
    }
}
