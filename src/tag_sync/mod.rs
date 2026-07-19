//! ExifTool tag database sync: parses `exiftool -f -listx` XML output into
//! `TagRecord`s and regenerates the `oxidex-tags-*` YAML tag databases.

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

/// A single ExifTool tag as reported by `exiftool -f -listx`.
#[derive(Debug, Clone, PartialEq)]
pub struct TagRecord {
    pub table: String,
    pub id: String,
    pub name: String,
    pub writable: bool,
    pub type_name: Option<String>,
    pub description: Option<String>,
}

fn attr_value(e: &BytesStart, key: &str) -> Option<String> {
    e.attributes().flatten().find_map(|attr| {
        if attr.key.as_ref() == key.as_bytes() {
            std::str::from_utf8(&attr.value).ok().map(|s| s.to_string())
        } else {
            None
        }
    })
}

/// Parses `exiftool -f -listx` XML output into a flat list of `TagRecord`s.
///
/// ExifTool has already resolved table-level `WRITABLE` inheritance by the
/// time it emits this XML, so no inheritance logic is needed here — every
/// `<tag>` element carries its own fully-resolved `writable` attribute.
pub fn parse_listx(xml: &str) -> Result<Vec<TagRecord>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut tags = Vec::new();
    let mut current_table = String::new();
    let mut in_tag = false;
    let mut capturing_en_desc = false;
    let mut buf = Vec::new();

    loop {
        match reader
            .read_event_into(&mut buf)
            .context("failed to read XML event from exiftool -listx output")?
        {
            Event::Start(e) | Event::Empty(e) if e.name().as_ref() == b"table" => {
                current_table = attr_value(&e, "name").unwrap_or_default();
            }
            Event::Start(e) if e.name().as_ref() == b"tag" => {
                let id = attr_value(&e, "id").unwrap_or_default();
                let name = attr_value(&e, "name").unwrap_or_default();
                let writable = matches!(attr_value(&e, "writable").as_deref(), Some("true"));
                let type_name = attr_value(&e, "type");

                tags.push(TagRecord {
                    table: current_table.clone(),
                    id,
                    name,
                    writable,
                    type_name,
                    description: None,
                });
                in_tag = true;
            }
            Event::Empty(e) if e.name().as_ref() == b"tag" => {
                let id = attr_value(&e, "id").unwrap_or_default();
                let name = attr_value(&e, "name").unwrap_or_default();
                let writable = matches!(attr_value(&e, "writable").as_deref(), Some("true"));
                let type_name = attr_value(&e, "type");

                tags.push(TagRecord {
                    table: current_table.clone(),
                    id,
                    name,
                    writable,
                    type_name,
                    description: None,
                });
            }
            Event::Start(e) if in_tag && e.name().as_ref() == b"desc" => {
                capturing_en_desc = attr_value(&e, "lang").as_deref() == Some("en");
            }
            Event::Text(t) if capturing_en_desc => {
                if let Some(last) = tags.last_mut() {
                    // `decode()` only handles byte-encoding, not XML entities
                    // (e.g. `&#39;`, `&amp;`) — `escape::unescape` does that,
                    // matching the pattern already used in
                    // src/parsers/xmp/rdf_parser.rs.
                    let decoded = t
                        .decode()
                        .context("invalid text content in <desc> element")?;
                    let text = quick_xml::escape::unescape(&decoded)
                        .unwrap_or_else(|_| decoded.clone())
                        .into_owned();
                    last.description = Some(text);
                }
                capturing_en_desc = false;
            }
            Event::End(e) if e.name().as_ref() == b"tag" => {
                in_tag = false;
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(tags)
}

/// Routes an ExifTool table name (e.g. `Canon::AFConfig`, `Exif::Main`) to
/// the `oxidex-tags-*` domain crate that should own it. Matching is
/// case-insensitive: `-listx` table names use ExifTool's own mixed casing
/// (`Exif`, `Jpeg2000`), which does not consistently match any single case
/// convention.
pub fn get_domain_for_table(table_name: &str) -> &'static str {
    let base_name = table_name.split("::").next().unwrap_or(table_name);
    match base_name.to_ascii_uppercase().as_str() {
        "EXIF" | "XMP" | "IPTC" | "GPS" | "ICC_PROFILE" | "MWG" | "PHOTOSHOP" | "FLASHPIX"
        | "GEOTIFF" | "COMPOSITE" | "TRAILER" | "MAKERNOTES" => "core",

        "CANON" | "CANONCUSTOM" | "CANONRAW" | "NIKON" | "NIKONCAPTURE" | "NIKONCUSTOM"
        | "NIKONSETTINGS" | "SONY" | "SONYIDC" | "PANASONIC" | "PANASONICRAW" | "OLYMPUS"
        | "FUJIFILM" | "PENTAX" | "CASIO" | "MINOLTA" | "MINOLTARAW" | "RICOH" | "SIGMA"
        | "SIGMARAW" | "PHASEONE" | "KODAK" | "KYOCERARAW" | "SAMSUNG" | "SANYO" | "HP" | "GE"
        | "RECONYX" | "JVC" | "MOTOROLA" | "APPLE" | "DJI" | "GOPRO" | "PARROT" | "INFIRAY"
        | "FLIR" => "camera",

        "QUICKTIME" | "MATROSKA" | "MPEG" | "M2TS" | "MXF" | "FLAC" | "AAC" | "AIFF" | "VORBIS"
        | "OPUS" | "ID3" | "APE" | "ASF" | "FLASH" | "REAL" | "THEORA" | "H264" | "WAVPACK"
        | "MPC" | "DSF" | "WTV" => "media",

        "PNG" | "GIF" | "JPEG" | "JPEG2000" | "BMP" | "TIFF" | "DNG" | "MNG" | "PGF" | "PICT"
        | "OPENEXR" | "FLIF" | "BPG" | "WEBP" | "DPX" | "PSP" | "PCX" | "MIFF" | "PHOTOCD"
        | "ICO" | "PALM" => "image",

        "PDF" | "POSTSCRIPT" | "FONT" | "PLIST" | "HTML" | "TORRENT" | "ZIP" | "TNEF" | "VCARD"
        | "MICROSOFT" | "MACOS" | "EXE" | "LNK" | "RSRC" | "FOTOSTATION" | "PHOTOMECHANIC"
        | "ITC" | "GIMP" | "GM" | "GOOGLE" => "document",

        "DICOM" | "FITS" | "MRC" | "STIM" | "PCAP" | "XISF" | "MISB" | "DJVU" | "ISO"
        | "NINTENDO" => "specialty",

        _ => "core",
    }
}

/// The six `oxidex-tags-*` domain crates, in the order YAML files are
/// written.
pub const DOMAINS: [&str; 6] = ["core", "camera", "media", "image", "document", "specialty"];

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_LISTX: &str = r#"<?xml version='1.0' encoding='UTF-8'?>
<taginfo>
<table name='Exif::Main' g0='EXIF' g1='IFD0' g2='Image'>
 <desc lang='en'>Exif</desc>
 <tag id='271' name='Make' type='string' writable='true' g1='IFD0'>
  <desc lang='en'>Manufacturer</desc>
  <desc lang='de'>Hersteller</desc>
 </tag>
 <tag id='37500' name='MakerNotes' type='undef' writable='false' g1='ExifIFD'>
  <desc lang='en'>Maker Notes</desc>
 </tag>
</table>
<table name='Composite' g0='Composite' g1='Composite' g2='Other'>
 <tag id='Exif-ThumbnailImage' name='ThumbnailImage' type='?' writable='true' g0='EXIF' g1='All' g2='Preview'>
  <desc lang='en'>Thumbnail Image</desc>
 </tag>
</table>
</taginfo>
"#;

    #[test]
    fn parses_hash_form_tags_with_type_and_description() {
        let tags = parse_listx(SAMPLE_LISTX).expect("valid listx XML must parse");
        assert_eq!(tags.len(), 3);

        let make = tags
            .iter()
            .find(|t| t.name == "Make")
            .expect("Make tag must be present");
        assert_eq!(make.table, "Exif::Main");
        assert_eq!(make.id, "271");
        assert!(make.writable);
        assert_eq!(make.type_name.as_deref(), Some("string"));
        assert_eq!(make.description.as_deref(), Some("Manufacturer"));
    }

    #[test]
    fn parses_non_writable_tags_and_non_numeric_ids() {
        let tags = parse_listx(SAMPLE_LISTX).expect("valid listx XML must parse");

        let maker_notes = tags
            .iter()
            .find(|t| t.name == "MakerNotes")
            .expect("MakerNotes tag must be present");
        assert!(!maker_notes.writable);

        let thumb = tags
            .iter()
            .find(|t| t.name == "ThumbnailImage")
            .expect("ThumbnailImage tag must be present");
        assert_eq!(thumb.id, "Exif-ThumbnailImage");
        assert_eq!(thumb.table, "Composite");
        assert_eq!(thumb.type_name.as_deref(), Some("?"));
    }

    #[test]
    fn rejects_malformed_xml() {
        let result = parse_listx("<taginfo><table name='X'><tag id='1'");
        assert!(result.is_err(), "truncated XML must return an error, not panic");
    }

    #[test]
    fn routes_core_standards_case_insensitively() {
        assert_eq!(get_domain_for_table("Exif::Main"), "core");
        assert_eq!(get_domain_for_table("GPS::Main"), "core");
        assert_eq!(get_domain_for_table("Composite"), "core");
        assert_eq!(get_domain_for_table("ICC_Profile::Main"), "core");
    }

    #[test]
    fn routes_camera_makernotes() {
        assert_eq!(get_domain_for_table("Canon::AFConfig"), "camera");
        assert_eq!(get_domain_for_table("Nikon::Main"), "camera");
    }

    #[test]
    fn routes_media_and_image_and_document_and_specialty() {
        assert_eq!(get_domain_for_table("QuickTime::Main"), "media");
        assert_eq!(get_domain_for_table("Jpeg2000::Main"), "image");
        assert_eq!(get_domain_for_table("PDF::Main"), "document");
        assert_eq!(get_domain_for_table("DICOM::Main"), "specialty");
    }

    #[test]
    fn routes_unknown_tables_to_core_by_default() {
        assert_eq!(get_domain_for_table("SomeBrandNewVendor::Main"), "core");
    }
}
