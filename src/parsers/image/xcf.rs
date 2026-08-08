//! XCF (GIMP native image) parser
//!
//! XCF structure (per ExifTool's `GIMP.pm` and
//! <https://gitlab.gnome.org/GNOME/gimp/blob/master/devel-docs/xcf.txt>).
//! All multi-byte fields are big-endian.
//!
//! - Magic: 9 bytes `"gimp xcf "`
//! - Version: 5-byte NUL-terminated string at offset 9 (`"file"`, `"v001"`, ...)
//! - Width, height, color mode: three `int32u` filling the 26-byte header
//! - Precision: an extra `int32u` present only in version 4 and later
//! - Property list: repeating `int32u` type, `int32u` length, then that many
//!   payload bytes, terminated by a property of type 0 (`PROP_END`)
//!
//! `PROP_PARASITES` (21) carries GIMP's "parasites" -- named blobs attached to
//! the image. Each is an `int32u` name length, the name bytes, an `int32u`
//! flags word, an `int32u` data length, and the data. The parasite named
//! `"icc-profile"` holds a bare ICC profile, which is what this parser routes
//! to the ICC decoder.
//!
//! Only the ICC profile is extracted here. The `exif-data`, `gimp-metadata`
//! (XMP) and `gimp-comment` parasites that ExifTool also decodes are walked
//! past but not parsed.

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap};
use crate::error::{ExifToolError, Result};

const XCF_SIGNATURE: &[u8] = b"gimp xcf ";

/// Bytes in the fixed XCF header: magic, version string, width, height and
/// color mode.
const XCF_HEADER_LEN: usize = 26;

/// Property type holding the image's parasites.
const PROP_PARASITES: u32 = 21;

/// Largest property payload we will read, so a corrupt length cannot make us
/// allocate without bound.
const MAX_PROPERTY_SIZE: u64 = 64 * 1024 * 1024;

/// Parser for XCF (GIMP) files.
///
/// Extracts the ICC profile that GIMP stores in the `icc-profile` parasite.
pub struct XCFParser;

impl XCFParser {
    /// Verifies the XCF file signature (`"gimp xcf "`).
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < XCF_HEADER_LEN as u64 {
            return Ok(false);
        }
        let header = reader.read(0, XCF_SIGNATURE.len())?;
        Ok(header == XCF_SIGNATURE)
    }
}

impl FormatParser for XCFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid XCF signature"));
        }

        let mut metadata = MetadataMap::new();
        parse_xcf_properties(reader, &mut metadata)?;
        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::XCF)
    }
}

/// Reads a big-endian `int32u` at `offset`.
fn read_u32(reader: &dyn FileReader, offset: u64) -> Result<u32> {
    let bytes = reader.read(offset, 4)?;
    Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Returns the XCF version number encoded in the 5-byte header string.
///
/// ExifTool matches `/^v0*(\d+)/`, so `"file"` (version 0) yields no match and
/// `"v011"` yields 11. Only the "4 or later" test matters to us: those files
/// carry an extra precision word before the property list.
fn version_number(version: &[u8]) -> Option<u32> {
    // Trim at the NUL terminator, the way ExifTool's `string[5]` format does.
    let version = match version.iter().position(|&b| b == 0) {
        Some(nul) => &version[..nul],
        None => version,
    };
    let rest = version.strip_prefix(b"v")?;
    // The regex is a prefix match, so take the leading digit run. Leading
    // zeros are absorbed by `parse`, matching the `0*` in the pattern.
    let digit_len = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digit_len == 0 {
        return None;
    }
    std::str::from_utf8(&rest[..digit_len]).ok()?.parse().ok()
}

/// Walks the XCF property list, decoding the parasites property.
fn parse_xcf_properties(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
    let file_size = reader.size();

    let version = reader.read(9, 5)?.to_vec();

    // Version 4 and later insert a precision word between the header and the
    // property list.
    let mut offset = XCF_HEADER_LEN as u64;
    if version_number(&version).is_some_and(|v| v >= 4) {
        offset += 4;
    }

    loop {
        if offset + 8 > file_size {
            break;
        }
        let prop_type = read_u32(reader, offset)?;
        // PROP_END terminates the list.
        if prop_type == 0 {
            break;
        }
        let prop_size = u64::from(read_u32(reader, offset + 4)?);
        offset += 8;

        // A property running past EOF means the list is malformed; keep what we
        // have, the way ExifTool stops on a short read.
        if offset + prop_size > file_size {
            break;
        }

        // Only the parasites property is read into memory, and only up to a
        // bound, so a corrupt length cannot make us allocate without limit.
        // Anything else is walked past.
        if prop_type == PROP_PARASITES && prop_size <= MAX_PROPERTY_SIZE {
            match reader.read(offset, prop_size as usize) {
                Ok(payload) => {
                    let payload = payload.to_vec();
                    parse_parasites(&payload, metadata);
                }
                Err(_) => break,
            }
        }

        offset += prop_size;
    }

    Ok(())
}

/// Decodes the parasite records packed into a `PROP_PARASITES` payload.
///
/// Mirrors ExifTool's `ProcessParasites`: it stops at the first record that
/// would run past the end of the payload rather than treating it as an error.
fn parse_parasites(payload: &[u8], metadata: &mut MetadataMap) {
    let end = payload.len();
    let mut pos = 0usize;

    loop {
        let Some(name_end) = pos.checked_add(4) else {
            break;
        };
        if name_end > end {
            break;
        }
        let name_len = u32::from_be_bytes([
            payload[pos],
            payload[pos + 1],
            payload[pos + 2],
            payload[pos + 3],
        ]) as usize;
        pos = name_end;

        // The name must be followed by the 8-byte flags/length pair.
        match pos.checked_add(name_len).and_then(|p| p.checked_add(8)) {
            Some(needed) if needed <= end => {}
            _ => break,
        }

        let name = &payload[pos..pos + name_len];
        pos += name_len;

        // Trim at the NUL terminator; the stored length includes it.
        let name = match name.iter().position(|&b| b == 0) {
            Some(nul) => &name[..nul],
            None => name,
        };

        // payload[pos..pos + 4] is the flags word, which ExifTool ignores.
        let data_len = u32::from_be_bytes([
            payload[pos + 4],
            payload[pos + 5],
            payload[pos + 6],
            payload[pos + 7],
        ]) as usize;
        pos += 8;

        match pos.checked_add(data_len) {
            Some(data_end) if data_end <= end => {
                if name == b"icc-profile" {
                    insert_icc_profile(&payload[pos..data_end], metadata);
                }
                pos = data_end;
            }
            _ => break,
        }
    }
}

/// Decodes an embedded ICC profile and files its tags under `ICC_Profile:`.
fn insert_icc_profile(icc_data: &[u8], metadata: &mut MetadataMap) {
    match crate::parsers::icc::parse_icc_profile_data(icc_data) {
        Ok(icc_tags) => {
            // `parse_icc_profile_data` returns bare names; the `ICC_Profile:`
            // family is added by whoever embeds the profile, the same way
            // `flif.rs` and `gif.rs` do for their containers.
            for (tag_name, value) in icc_tags {
                metadata.insert(format!("ICC_Profile:{}", tag_name), value);
            }
        }
        Err(e) => {
            eprintln!("Warning: Failed to parse ICC profile in XCF: {}", e);
        }
    }
}

/// Parses metadata from XCF files.
///
/// This is a convenience wrapper around `XCFParser` that provides a functional
/// API.
pub fn parse_xcf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = XCFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::TagValue;
    use crate::test_support::TestReader;

    /// Builds a parasite record: name length, name, flags, data length, data.
    fn parasite(name: &str, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let name_bytes = format!("{}\0", name).into_bytes();
        out.extend_from_slice(&(name_bytes.len() as u32).to_be_bytes());
        out.extend_from_slice(&name_bytes);
        out.extend_from_slice(&1u32.to_be_bytes()); // flags
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(data);
        out
    }

    /// Builds a minimal XCF file carrying the given parasite payloads.
    fn xcf_with_parasites(version: &[u8; 5], parasites: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(XCF_SIGNATURE);
        out.extend_from_slice(version);
        out.extend_from_slice(&8u32.to_be_bytes()); // width
        out.extend_from_slice(&8u32.to_be_bytes()); // height
        out.extend_from_slice(&0u32.to_be_bytes()); // color mode
        if version_number(version).is_some_and(|v| v >= 4) {
            out.extend_from_slice(&150u32.to_be_bytes()); // precision
        }
        out.extend_from_slice(&PROP_PARASITES.to_be_bytes());
        out.extend_from_slice(&(parasites.len() as u32).to_be_bytes());
        out.extend_from_slice(parasites);
        out.extend_from_slice(&0u32.to_be_bytes()); // PROP_END
        out.extend_from_slice(&0u32.to_be_bytes());
        out
    }

    /// A minimal but structurally valid ICC profile: a 128-byte header
    /// declaring one `desc` tag.
    fn minimal_icc() -> Vec<u8> {
        let desc = b"desc";
        let text = b"Test Profile\0";
        let tag_offset = 128 + 4 + 12;
        let tag_size = 8 + 4 + text.len();
        let total = tag_offset + tag_size;

        let mut icc = vec![0u8; 128];
        icc[0..4].copy_from_slice(&(total as u32).to_be_bytes()); // profile size
        icc[12..16].copy_from_slice(b"mntr"); // profile class
        icc[16..20].copy_from_slice(b"RGB "); // color space
        icc[20..24].copy_from_slice(b"XYZ "); // connection space
        icc[36..40].copy_from_slice(b"acsp"); // file signature

        icc.extend_from_slice(&1u32.to_be_bytes()); // tag count
        icc.extend_from_slice(desc);
        icc.extend_from_slice(&(tag_offset as u32).to_be_bytes());
        icc.extend_from_slice(&(tag_size as u32).to_be_bytes());

        icc.extend_from_slice(b"desc");
        icc.extend_from_slice(&0u32.to_be_bytes());
        icc.extend_from_slice(&(text.len() as u32).to_be_bytes());
        icc.extend_from_slice(text);
        icc
    }

    #[test]
    fn test_version_number() {
        assert_eq!(version_number(b"file\0"), None);
        assert_eq!(version_number(b"v001\0"), Some(1));
        assert_eq!(version_number(b"v003\0"), Some(3));
        assert_eq!(version_number(b"v004\0"), Some(4));
        assert_eq!(version_number(b"v011\0"), Some(11));
        assert_eq!(version_number(b"v013\0"), Some(13));
    }

    #[test]
    fn test_rejects_non_xcf() {
        let data = b"not an xcf file at all....".to_vec();
        assert!(parse_xcf_metadata(&TestReader::new(data)).is_err());
    }

    #[test]
    fn test_extracts_icc_profile_from_parasite() {
        let data = xcf_with_parasites(b"file\0", &parasite("icc-profile", &minimal_icc()));
        let metadata = parse_xcf_metadata(&TestReader::new(data)).unwrap();

        assert_eq!(
            metadata.get("ICC_Profile:ProfileDescription"),
            Some(&TagValue::String("Test Profile".to_string())),
        );
    }

    /// Version 4 and later carry a precision word the property walk must skip.
    #[test]
    fn test_extracts_icc_profile_from_v013_file() {
        let data = xcf_with_parasites(b"v013\0", &parasite("icc-profile", &minimal_icc()));
        let metadata = parse_xcf_metadata(&TestReader::new(data)).unwrap();

        assert_eq!(
            metadata.get("ICC_Profile:ProfileDescription"),
            Some(&TagValue::String("Test Profile".to_string())),
        );
    }

    /// The ICC parasite is reached even when other parasites precede it.
    #[test]
    fn test_skips_preceding_parasites() {
        let mut parasites = parasite("gimp-comment", b"hello\0");
        parasites.extend_from_slice(&parasite("icc-profile", &minimal_icc()));
        parasites.extend_from_slice(&parasite("gimp-image-grid", &[0u8; 16]));

        let data = xcf_with_parasites(b"file\0", &parasites);
        let metadata = parse_xcf_metadata(&TestReader::new(data)).unwrap();

        assert_eq!(
            metadata.get("ICC_Profile:ProfileDescription"),
            Some(&TagValue::String("Test Profile".to_string())),
        );
        // Parasites other than the ICC profile are walked past, not decoded.
        assert!(metadata.get("Comment").is_none());
    }

    /// A truncated parasite record must stop the walk instead of panicking.
    #[test]
    fn test_truncated_parasite_is_not_fatal() {
        let mut parasites = parasite("icc-profile", &minimal_icc());
        parasites.truncate(parasites.len() - 32);

        let data = xcf_with_parasites(b"file\0", &parasites);
        let metadata = parse_xcf_metadata(&TestReader::new(data)).unwrap();
        assert!(metadata.get("ICC_Profile:ProfileDescription").is_none());
    }

    /// A parasite whose declared name length runs past the payload must not panic.
    #[test]
    fn test_absurd_name_length_is_not_fatal() {
        let mut parasites = Vec::new();
        parasites.extend_from_slice(&u32::MAX.to_be_bytes());
        parasites.extend_from_slice(b"icc-profile\0");

        let data = xcf_with_parasites(b"file\0", &parasites);
        assert!(parse_xcf_metadata(&TestReader::new(data)).is_ok());
    }
}
