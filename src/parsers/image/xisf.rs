//! Extensible Image Serialization Format (XISF) reader.
//!
//! `XISF::ProcessXISF` (XISF.pm:118-145) is short and entirely declarative:
//! validate `XISF0100`, read the little-endian `int32u` header length at byte
//! 8, report the header bytes themselves as the `XML` tag, then hand the
//! header to `XMP::ProcessXMP` against `XISF::Main` with two `dirInfo` knobs
//! set --
//!
//! ```perl
//!     IgnoreProp   => { xisf => 1, Metadata => 1, Property => 1 },
//!     XMPParseOpts => { AttrProc => \&HandleXISFAttrs },
//! ```
//!
//! -- and finally split `ImageGeometry` on `:` into `ImageWidth`,
//! `ImageHeight` and `NumPlanes` (XISF.pm:137-143).
//!
//! # Why this is not a new XML parser
//!
//! Every tag name here is a *property path*, minted by `GetXMPTagID` from the
//! element/attribute hierarchy, exactly as for a plain XML file. That walk
//! already lives in [`crate::parsers::xmp::generic_xml`], including the
//! `ucfirst`-per-segment concatenation, the shorthand-attribute loop and
//! `XMPAutoConv`. This module supplies the two knobs above and the
//! `XISF::Main` name table (XISF.pm:21-83) on top of it, rather than
//! reimplementing the naming rule a second time and letting the two drift.
//!
//! So `<Image geometry="256:256:1">` becomes `Image` + `Geometry` ->
//! `ImageGeometry`; `<Data compression="zlib:65536">` nested inside it becomes
//! `ImageDataCompression`, a name that appears in no table at all; and
//! `<Metadata><Property id="XISF:CreatorOS" type="String">Linux</Property>` is
//! rewritten by `HandleXISFAttrs` to the bare property `CreatorOS` with
//! `Metadata` and `Property` both dropped from the path.
//!
//! # What is deliberately absent
//!
//! **`ImageICCProfile` -> `ICC_Profile`** (XISF.pm:38-42). Its
//! `ValueConv => 'Image::ExifTool::XMP::DecodeBase64($val)'` yields a raw ICC
//! profile that ExifTool then re-enters as an `ICC_Profile` directory, not a
//! scalar; emitting the base64 text, or a `Binary data` placeholder over it,
//! under the real tag name `ICC_Profile` would be a plausible wrong value of
//! exactly the kind AGENTS.md rules out. The pinned `t/images/XISF.xisf`
//! carries no `iccProfile` attribute, so nothing is lost on the fixture.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/XISF.pm`

use crate::core::tag_occurrence::Instance;
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::parsers::xmp::generic_xml::{AttrHook, XmlWalkOptions, extract_xml_properties_with};

/// XISF.pm:124, `$raf->Read($buff, 16) == 16 and $buff =~ /^XISF0100/`.
const SIGNATURE: &[u8] = b"XISF0100";
/// The fixed 16-byte file header the `int32u` length at offset 8 measures from.
const FILE_HEADER_LEN: usize = 16;

/// XISF.pm:133, `IgnoreProp => { xisf => 1, Metadata => 1, Property => 1 }`.
const IGNORE_PROP: &[&str] = &["xisf", "Metadata", "Property"];

/// `%Image::ExifTool::XISF::Main`'s renaming entries (XISF.pm:21-83). Only the
/// names that differ from the property path are listed; every other entry in
/// that table (`ImageGeometry`, `CreatorApplication`, ...) reports under the
/// path name itself, and a path with no entry at all is minted verbatim
/// ("ExifTool will extract any other tags found", XISF.pm:25-27).
const RENAMES: &[(&str, &str)] = &[
    ("ImageImageType", "ImageType"),                   // XISF.pm:32
    ("ImageColorSpace", "ColorSpace"),                 // XISF.pm:33
    ("ImageResolutionHorizontal", "XResolution"),      // XISF.pm:35
    ("ImageResolutionVertical", "YResolution"),        // XISF.pm:36
    ("ImageResolutionUnit", "ResolutionUnit"),         // XISF.pm:37
    ("ImageICCProfileLocation", "ICCProfileLocation"), // XISF.pm:43
    ("ImageOffset", "ImagePixelOffset"),               // XISF.pm:45
    ("ImageOrientation", "Orientation"),               // XISF.pm:46
    ("ImageId", "ImageID"),                            // XISF.pm:47
    ("ImageUuid", "UUID"),                             // XISF.pm:48
    ("CreationTime", "CreateDate"),                    // XISF.pm:50-56
    ("OriginalCreationTime", "DateTimeOriginal"),      // XISF.pm:73-80
];

/// The two `XISF::Main` entries whose `ValueConv` is
/// `Image::ExifTool::XMP::ConvertXMPDate($val)` (XISF.pm:54, :78). Keyed by
/// property path, since the rename happens after this lookup would.
const XMP_DATE_TAGS: &[&str] = &["CreationTime", "OriginalCreationTime"];

/// `ImageICCProfile` (XISF.pm:38-42) -- see the module header for why its
/// value is omitted rather than approximated.
const OMITTED_PATHS: &[&str] = &["ImageICCProfile"];

/// `Binary => 1` entries in `XISF::Main`, reported by ExifTool's ordinary
/// (non-`-b`) output as a byte-count placeholder. `ImageData` is XISF.pm:49.
const BINARY_TAGS: &[&str] = &["ImageData"];

/// XISF.pm:90-112, `HandleXISFAttrs`: when an element carries an `id`
/// attribute, that id (minus an `XISF:` namespace) *replaces* the property
/// name, an accompanying `value` attribute replaces the element's content, and
/// `id`/`value`/`type` are struck from the attribute list so none of them
/// becomes a tag.
fn handle_xisf_attrs(attrs: &[(String, String)]) -> Option<AttrHook> {
    let id = attrs
        .iter()
        .find(|(name, _)| name == "id")
        .map(|(_, value)| value.as_str())?;
    Some(AttrHook {
        prop: Some(id.strip_prefix("XISF:").unwrap_or(id).to_string()),
        value: attrs
            .iter()
            .find(|(name, _)| name == "value")
            .map(|(_, value)| value.clone()),
        consumed: attrs
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|name| matches!(name.as_str(), "id" | "value" | "type"))
            .collect(),
    })
}

/// `Image::ExifTool::XMP::ConvertXMPDate` with `$unsure` unset (XMP.pm:3382-3393).
fn convert_xmp_date(value: &str) -> String {
    let bytes = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes.len() >= range.end && bytes[range].iter().all(u8::is_ascii_digit)
    };
    // `^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}:\d{2})(:\d{2})?\s*(\S*)$`
    if bytes.len() >= 16
        && digits(0..4)
        && bytes[4] == b'-'
        && digits(5..7)
        && bytes[7] == b'-'
        && digits(8..10)
        && matches!(bytes[10], b'T' | b' ')
        && digits(11..13)
        && bytes[13] == b':'
        && digits(14..16)
    {
        let mut rest = &value[16..];
        let mut seconds = "";
        if rest.len() >= 3 && rest.as_bytes()[0] == b':' && digits(17..19) {
            seconds = &rest[..3];
            rest = &rest[3..];
        }
        let trailing = rest.trim_start();
        if !trailing.chars().any(char::is_whitespace) {
            return format!(
                "{}:{}:{} {}{seconds}{trailing}",
                &value[0..4],
                &value[5..7],
                &value[8..10],
                &value[11..16],
            );
        }
    }
    // The `elsif (not $unsure ...)` branch: a leading `YYYY[-MM[-DD]]` has its
    // hyphens transliterated to colons, and nothing else changes.
    let leading_date = digits(0..4)
        && (bytes.len() < 5
            || (bytes[4] == b'-' && digits(5..7) && (bytes.len() < 8 || bytes[7] == b'-')));
    if leading_date {
        return value.replace('-', ":");
    }
    value.to_string()
}

/// Extract XISF metadata (`Image::ExifTool::XISF::ProcessXISF`).
pub fn parse_xisf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < FILE_HEADER_LEN as u64 {
        return Err("XISF file is too short for the 16-byte header".to_string());
    }
    let head = reader
        .read(0, FILE_HEADER_LEN)
        .map_err(|error| error.to_string())?;
    if !head.starts_with(SIGNATURE) {
        return Err("invalid XISF signature".to_string());
    }
    // XISF.pm:126-128: `SetByteOrder('II')` then `Get32u(\$buff, 8)`.
    let header_len = u32::from_le_bytes([head[8], head[9], head[10], head[11]]) as usize;
    let header = reader
        .read(FILE_HEADER_LEN as u64, header_len)
        .map_err(|error| error.to_string())?;
    if header.len() != header_len {
        // XISF.pm:129 warns and returns with only the file type set.
        return Err("error reading XISF header".to_string());
    }

    let mut metadata = MetadataMap::new();
    // XISF.pm:130, `$et->FoundTag(XML => $buff)` -- the whole header block,
    // reported by ordinary output as a byte count.
    metadata.insert(
        "XML:XML",
        TagValue::new_string(format!(
            "(Binary data {header_len} bytes, use -b option to extract)"
        )),
    );

    let options = XmlWalkOptions {
        // `%Image::ExifTool::XISF::Main`'s `GROUPS => { 0 => 'XML', 1 => 'XML' }`
        // (XISF.pm:22).
        group0: "XML",
        ignore_prop: IGNORE_PROP,
        attr_proc: Some(handle_xisf_attrs),
        ..XmlWalkOptions::default()
    };
    let properties = extract_xml_properties_with(header, &options).unwrap_or_default();

    let mut geometry: Option<String> = None;
    for property in properties {
        if OMITTED_PATHS.contains(&property.name.as_str()) {
            continue;
        }
        if property.name == "ImageGeometry" && geometry.is_none() {
            geometry = Some(property.value.clone());
        }
        let name = RENAMES
            .iter()
            .find(|(path, _)| *path == property.name)
            .map_or(property.name.as_str(), |(_, renamed)| *renamed);
        let value = if XMP_DATE_TAGS.contains(&property.name.as_str()) {
            convert_xmp_date(&property.raw)
        } else if BINARY_TAGS.contains(&name) {
            format!(
                "(Binary data {} bytes, use -b option to extract)",
                property.raw.len()
            )
        } else {
            property.value.clone()
        };
        metadata.insert_occurrence(
            format!("{}:{name}", property.group1),
            TagValue::new_string(value),
            0,
            &property.group1,
            Instance::default(),
        );
    }

    // XISF.pm:137-143: `my ($w, $h, $n) = split /:/, $geo;` -- these three are
    // `FoundTag`'d with no table, so they land in the default `File` group.
    if let Some(geometry) = geometry {
        let mut parts = geometry.split(':');
        for name in ["ImageWidth", "ImageHeight", "NumPlanes"] {
            let Some(part) = parts.next() else { break };
            metadata.insert(format!("File:{name}"), TagValue::new_string(part));
        }
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xisf_date_conversion_matches_convert_xmp_date() {
        // The pinned `t/images/XISF.xisf` value: ExifTool reports
        // `CreateDate: 2019:09:18 22:57:08Z`.
        assert_eq!(
            convert_xmp_date("2019-09-18T22:57:08Z"),
            "2019:09:18 22:57:08Z"
        );
        // `$unsure` unset, so the second branch fires on a bare date.
        assert_eq!(convert_xmp_date("2019-09-18"), "2019:09:18");
        assert_eq!(convert_xmp_date("not a date"), "not a date");
    }

    #[test]
    fn handle_xisf_attrs_renames_and_consumes() {
        let attrs = [
            ("id".to_string(), "XISF:CompressionLevel".to_string()),
            ("type".to_string(), "Int32".to_string()),
            ("value".to_string(), "0".to_string()),
        ];
        let hook = handle_xisf_attrs(&attrs).expect("id present");
        assert_eq!(hook.prop.as_deref(), Some("CompressionLevel"));
        assert_eq!(hook.value.as_deref(), Some("0"));
        assert_eq!(hook.consumed.len(), 3);

        // XISF.pm:93, `return 0 unless defined $$attrs{id}`.
        let plain = [("geometry".to_string(), "256:256:1".to_string())];
        assert!(handle_xisf_attrs(&plain).is_none());
    }
}
