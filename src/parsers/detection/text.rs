//! Text-based format detection
//!
//! Handles detection of text-based 3D and interchange formats including
//! DXF, OBJ, GLTF, STL, and EPS.

use crate::core::FileFormat;
use chrono::{DateTime, FixedOffset};

fn looks_like_ics(text: &str) -> bool {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text).trim_start();
    let mut lines = text.lines();
    let Some(first_line) = lines.next() else {
        return false;
    };

    if !first_line.trim_end_matches('\r').eq("BEGIN:VCALENDAR") {
        return false;
    }

    lines.any(|line| {
        let line = line.trim_end_matches('\r');
        line.get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("VERSION:"))
    })
}

fn looks_like_eml(text: &str) -> bool {
    let header_end = text
        .find("\r\n\r\n")
        .or_else(|| text.find("\n\n"))
        .unwrap_or(text.len());
    let headers = &text[..header_end];
    let mut has_from = false;
    let mut has_valid_date = false;
    let mut has_address_like_from = false;
    let mut has_address_like_recipient = false;
    let mut has_mail_specific_header = false;
    let mut has_subject = false;
    let mut saw_header = false;

    for line in headers.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }

        if line.starts_with(' ') || line.starts_with('\t') {
            if !saw_header {
                return false;
            }
            continue;
        }

        let Some((name, value)) = line.split_once(':') else {
            return false;
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return false;
        }

        saw_header = true;
        has_from |= name.eq_ignore_ascii_case("from");
        has_address_like_from |=
            name.eq_ignore_ascii_case("from") && looks_like_email_address(value.trim());
        has_address_like_recipient |=
            matches!(name.to_ascii_lowercase().as_str(), "to" | "cc" | "bcc")
                && looks_like_email_address(value.trim());
        has_valid_date |=
            name.eq_ignore_ascii_case("date") && looks_like_rfc_email_date(value.trim());
        has_subject |= name.eq_ignore_ascii_case("subject");
        has_mail_specific_header |= matches!(
            name.to_ascii_lowercase().as_str(),
            "message-id"
                | "mime-version"
                | "received"
                | "content-type"
                | "content-transfer-encoding"
                | "content-disposition"
                | "return-path"
        );
    }

    has_from
        && ((has_valid_date
            && ((has_address_like_from && has_address_like_recipient) || has_mail_specific_header))
            || (has_address_like_from && has_address_like_recipient && has_subject))
}

fn eml_header_bytes(data: &[u8]) -> &[u8] {
    if let Some(index) = data.windows(4).position(|window| window == b"\r\n\r\n") {
        return &data[..index];
    }
    if let Some(index) = data.windows(2).position(|window| window == b"\n\n") {
        return &data[..index];
    }
    data
}

fn looks_like_rfc_email_date(value: &str) -> bool {
    DateTime::<FixedOffset>::parse_from_rfc2822(value).is_ok()
}

fn looks_like_email_address(value: &str) -> bool {
    value
        .split(|character: char| {
            character.is_ascii_whitespace()
                || matches!(character, '<' | '>' | '(' | ')' | ',' | ';' | '"')
        })
        .any(|candidate| {
            let Some((local, domain)) = candidate.rsplit_once('@') else {
                return false;
            };

            !local.is_empty()
                && !domain.is_empty()
                && !local.contains('@')
                && domain
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        })
}

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

    // The bounded probe may cut a multibyte character; judge the valid prefix.
    if looks_like_ics(super::helpers::utf8_prefix(data)) {
        return Some(FileFormat::ICS);
    }

    let eml_headers = String::from_utf8_lossy(eml_header_bytes(data));
    if looks_like_eml(&eml_headers) {
        return Some(FileFormat::EML);
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
