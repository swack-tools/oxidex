//! Text-based format detection
//!
//! Handles detection of text-based 3D and interchange formats including
//! DXF, OBJ, GLTF, STL, and EPS.

use crate::core::FileFormat;

/// Detect text-based 3D and interchange formats
///
/// Several formats use text-based representations with distinctive patterns:
/// - DXF: AutoCAD exchange format
/// - OBJ: Wavefront 3D object
/// - GLTF: GL Transmission Format (JSON)
/// - STL: Stereolithography (ASCII variant)
/// - EPS: Encapsulated PostScript
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 100 bytes recommended)
///
/// # Returns
///
/// `Some(FileFormat)` if text format detected, `None` otherwise
pub fn detect_text_formats(data: &[u8]) -> Option<FileFormat> {
    // EPS detection first (can be shorter than 100 bytes)
    // ASCII EPS: %!PS-Adobe
    if data.starts_with(b"%!PS-Adobe") {
        return Some(FileFormat::EPS);
    }

    // Binary EPS (DOS EPS): 0xC5D0D3C6 magic
    if data.len() >= 4 && data[0] == 0xC5 && data[1] == 0xD0 && data[2] == 0xD3 && data[3] == 0xC6 {
        return Some(FileFormat::EPS);
    }

    // ICS (iCalendar) and EML (RFC 822 email) are line-based text formats whose
    // distinctive headers may appear in very short files (below the 100-byte
    // threshold used for the 3D/JSON formats below). Check them first so they are
    // routed to the dedicated parsers instead of falling through to plain TXT.
    if looks_like_ics(data) {
        return Some(FileFormat::ICS);
    }

    if looks_like_eml(data) {
        return Some(FileFormat::EML);
    }

    // DER-encoded X.509 certificates that use short-form length encoding (e.g.
    // 0x30 0x10 ...) are not covered by the 0x30 0x82 / 0x30 0x81 signatures in
    // the signature table. Detect the nested ASN.1 SEQUENCE structure here.
    if looks_like_der_x509(data) {
        return Some(FileFormat::X509);
    }

    if data.len() < 100 {
        return None;
    }

    let text = std::str::from_utf8(&data[0..100]).ok()?;

    // DXF: starts with "0\n" and contains "SECTION"
    if text.starts_with("0\n") && text.contains("SECTION") {
        return Some(FileFormat::DXF);
    }

    // OBJ: contains vertex definitions
    if text.contains("v ") || text.contains("vn ") || text.contains("vt ") {
        return Some(FileFormat::OBJ);
    }

    // GLTF: JSON with "asset" field
    if text.contains("\"asset\"") && text.contains("{") {
        return Some(FileFormat::GLTF);
    }

    // STL ASCII: starts with "solid"
    if text.starts_with("solid") {
        return Some(FileFormat::STL);
    }

    None
}

/// Returns true if the buffer looks like an iCalendar (.ics) document.
///
/// iCalendar streams begin with the `BEGIN:VCALENDAR` component, optionally
/// after leading whitespace or other unfolded lines.
fn looks_like_ics(data: &[u8]) -> bool {
    let prefix = &data[..data.len().min(256)];
    let text = String::from_utf8_lossy(prefix);
    text.trim_start().starts_with("BEGIN:VCALENDAR") || text.contains("\nBEGIN:VCALENDAR")
}

/// Returns true if the buffer looks like an RFC 822 email message (.eml).
///
/// Email messages start with header lines such as `From:`, `To:`, or
/// `Subject:`. We require a recognizable header at the start of the buffer or
/// on its own line to avoid misclassifying arbitrary text.
fn looks_like_eml(data: &[u8]) -> bool {
    let prefix = &data[..data.len().min(256)];
    let text = String::from_utf8_lossy(prefix);
    let lower = text.to_ascii_lowercase();
    lower.starts_with("from:")
        || lower.starts_with("subject:")
        || lower.contains("\nsubject:")
        || lower.contains("\nfrom:")
}

/// Returns true if the buffer looks like a DER-encoded X.509 certificate.
///
/// A DER certificate is an ASN.1 SEQUENCE (tag `0x30`) whose first element is
/// the TBSCertificate, itself a nested SEQUENCE. We validate the outer tag, a
/// plausible length, and the nested SEQUENCE tag. This complements the
/// `0x30 0x82` / `0x30 0x81` long-form signatures by also matching short-form
/// length encodings (`0x30 0x10`, etc.).
fn looks_like_der_x509(data: &[u8]) -> bool {
    // Need at least: outer tag, outer length octet, inner tag, inner length.
    if data.len() < 4 {
        return false;
    }

    // Outer ASN.1 SEQUENCE.
    if data[0] != 0x30 {
        return false;
    }

    // Determine where the outer content begins based on the length encoding.
    let content_start = match data[1] {
        // Long form: low 7 bits give the number of subsequent length octets.
        len if len & 0x80 != 0 => 2 + (len & 0x7F) as usize,
        // Short form: a single length octet.
        _ => 2,
    };

    // The first element of the certificate must itself be a SEQUENCE
    // (the TBSCertificate), distinguishing real certificates from text such as
    // DXF files that also begin with the ASCII '0' byte (0x30).
    data.get(content_start) == Some(&0x30)
}
