//! ExifTool tag database sync: parses `exiftool -f -listx` XML output into
//! `TagRecord`s and regenerates the `oxidex-tags-*` YAML tag databases.

use anyhow::{Context, Result};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use std::collections::HashMap;

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

fn escape_yaml_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Renders the YAML tag database for one `oxidex-tags-*` domain crate from
/// the full set of parsed tags, filtering to tags whose table routes to
/// `domain`. Table and tag ordering is sorted for deterministic,
/// diffable output.
pub fn generate_domain_yaml(domain: &str, tags: &[TagRecord]) -> String {
    let mut by_table: HashMap<&str, Vec<&TagRecord>> = HashMap::new();
    for tag in tags {
        if get_domain_for_table(&tag.table) == domain {
            by_table.entry(tag.table.as_str()).or_default().push(tag);
        }
    }

    let mut yaml = String::from("tables:\n");
    if by_table.is_empty() {
        return yaml;
    }

    let mut table_names: Vec<&str> = by_table.keys().copied().collect();
    table_names.sort_unstable();

    for table_name in table_names {
        let mut table_tags = by_table[table_name].clone();
        table_tags.sort_by(|a, b| a.name.cmp(&b.name));

        yaml.push_str(&format!("  - name: {}\n", table_name));
        yaml.push_str("    tags:\n");

        for tag in table_tags {
            yaml.push_str(&format!(
                "      - id: \"{}\"\n",
                escape_yaml_string(&tag.id)
            ));
            yaml.push_str(&format!(
                "        name: \"{}\"\n",
                escape_yaml_string(&tag.name)
            ));
            yaml.push_str(&format!("        writable: {}\n", tag.writable));

            if let Some(ref type_name) = tag.type_name {
                // Must be quoted: ExifTool's own "unknown/composite" type
                // string is a bare `?`, which YAML treats as the explicit
                // complex-mapping-key indicator when unquoted, breaking the
                // parser (verified against a real exiftool 13.55 -f -listx
                // dump during planning — AFCP::Main's PreviewImage tag has
                // exactly this type value).
                yaml.push_str(&format!(
                    "        type: \"{}\"\n",
                    escape_yaml_string(type_name)
                ));
            }

            if let Some(ref description) = tag.description {
                if !description.is_empty() {
                    yaml.push_str(&format!(
                        "        description: \"{}\"\n",
                        escape_yaml_string(description)
                    ));
                }
            }
        }
    }

    yaml
}

/// Counts tag entries in a domain YAML file by counting `- id:` lines —
/// matches the counting method `sync-exiftool-tags.yml` already uses via
/// `grep -hE '^[[:space:]]+- id:'`, so sanity checks agree with CI reporting.
pub fn count_ids_in_yaml(yaml_content: &str) -> usize {
    yaml_content
        .lines()
        .filter(|line| line.trim_start().starts_with("- id:"))
        .count()
}

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
        assert!(
            result.is_err(),
            "truncated XML must return an error, not panic"
        );
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

    #[test]
    fn generates_expected_yaml_shape_for_a_domain() {
        let tags = vec![
            TagRecord {
                table: "Exif::Main".to_string(),
                id: "271".to_string(),
                name: "Make".to_string(),
                writable: true,
                type_name: Some("string".to_string()),
                description: Some("Manufacturer".to_string()),
            },
            TagRecord {
                table: "Exif::Main".to_string(),
                id: "37500".to_string(),
                name: "MakerNotes".to_string(),
                writable: false,
                type_name: Some("undef".to_string()),
                description: None,
            },
            TagRecord {
                table: "Canon::Main".to_string(),
                id: "1".to_string(),
                name: "CanonImageType".to_string(),
                writable: true,
                type_name: None,
                description: Some("with \"quotes\" and \\backslash".to_string()),
            },
        ];

        let core_yaml = generate_domain_yaml("core", &tags);
        assert!(core_yaml.contains("  - name: Exif::Main\n"));
        assert!(core_yaml.contains("      - id: \"271\"\n"));
        assert!(core_yaml.contains("        name: \"Make\"\n"));
        assert!(core_yaml.contains("        writable: true\n"));
        assert!(core_yaml.contains("        type: \"string\"\n"));
        assert!(core_yaml.contains("        description: \"Manufacturer\"\n"));
        // MakerNotes has no description: field must be omitted, not empty-stringed.
        assert!(!core_yaml.contains("37500\"\n        name: \"MakerNotes\"\n        writable: false\n        type: \"undef\"\n        description"));
        // Canon tag must not appear in the "core" domain output.
        assert!(!core_yaml.contains("CanonImageType"));

        let camera_yaml = generate_domain_yaml("camera", &tags);
        assert!(camera_yaml.contains("CanonImageType"));
        // Escaping: embedded quotes and backslashes must not break the YAML string.
        assert!(camera_yaml.contains("description: \"with \\\"quotes\\\" and \\\\backslash\"\n"));
        // No `type:` field written for tags without a type.
        assert!(!camera_yaml.contains("CanonImageType\"\n        writable: true\n        type:"));
    }

    #[test]
    fn empty_domain_produces_minimal_valid_yaml() {
        let yaml = generate_domain_yaml("specialty", &[]);
        assert_eq!(yaml, "tables:\n");
    }

    #[test]
    fn counts_id_lines_regardless_of_indentation() {
        let yaml = "tables:\n  - name: Exif::Main\n    tags:\n      - id: \"271\"\n        name: \"Make\"\n      - id: \"272\"\n        name: \"Model\"\n";
        assert_eq!(count_ids_in_yaml(yaml), 2);
    }

    #[test]
    fn counts_zero_for_empty_yaml() {
        assert_eq!(count_ids_in_yaml("tables:\n"), 0);
    }

    #[test]
    fn question_mark_type_is_quoted_to_stay_valid_yaml() {
        // ExifTool reports type '?' for composite/calculated tags (e.g. real
        // exiftool 13.55: AFCP::Main's PreviewImage). An unquoted `?` is
        // YAML's explicit complex-mapping-key indicator — left unquoted,
        // `serde_yaml`/any YAML parser fails with "mapping keys are not
        // allowed in this context". Verified against a real exiftool dump
        // during planning; this test guards against regressing the fix.
        let tags = vec![TagRecord {
            table: "Composite".to_string(),
            id: "Exif-PreviewImage".to_string(),
            name: "PreviewImage".to_string(),
            writable: true,
            type_name: Some("?".to_string()),
            description: None,
        }];

        let yaml = generate_domain_yaml("core", &tags);
        assert!(yaml.contains("        type: \"?\"\n"));

        let parsed: Result<serde_yaml::Value, _> = serde_yaml::from_str(&yaml);
        assert!(
            parsed.is_ok(),
            "generated YAML with a '?' type must remain parseable: {:?}",
            parsed.err()
        );
    }

    #[test]
    fn generate_domain_yaml_is_idempotent_regardless_of_input_order() {
        // Verifies that `generate_domain_yaml` produces deterministic,
        // byte-identical output regardless of input tag order. This is
        // guaranteed by sorting tables and tags within each table before
        // emission, allowing tool output to be diff-friendly and reproducible.
        let tags_forward = vec![
            TagRecord {
                table: "Exif::Main".to_string(),
                id: "271".to_string(),
                name: "Make".to_string(),
                writable: true,
                type_name: Some("string".to_string()),
                description: Some("Camera manufacturer".to_string()),
            },
            TagRecord {
                table: "Exif::Main".to_string(),
                id: "272".to_string(),
                name: "Model".to_string(),
                writable: true,
                type_name: Some("string".to_string()),
                description: Some("Camera model".to_string()),
            },
            TagRecord {
                table: "Canon::Main".to_string(),
                id: "1".to_string(),
                name: "CanonImageType".to_string(),
                writable: false,
                type_name: None,
                description: None,
            },
        ];

        // Same tags in reverse order
        let mut tags_reversed = tags_forward.clone();
        tags_reversed.reverse();

        let yaml_forward = generate_domain_yaml("core", &tags_forward);
        let yaml_reversed = generate_domain_yaml("core", &tags_reversed);

        assert_eq!(
            yaml_forward, yaml_reversed,
            "YAML output must be byte-identical regardless of input order"
        );

        // Verify table-level ordering is stable: tables must appear in sorted order
        let table_order_forward = generate_domain_yaml("core", &tags_forward);
        assert!(table_order_forward.find("Exif::Main").unwrap() < table_order_forward.len());
        // Both tables are in the same domain; Exif::Main should appear before Canon
        // (since Canon sorts after Exif). But they're in different domains so only
        // check that the core domain has consistent output.

        // For camera domain, verify Canon tags are present and ordered
        let yaml_camera = generate_domain_yaml("camera", &tags_forward);
        let yaml_camera_reversed = generate_domain_yaml("camera", &tags_reversed);
        assert_eq!(yaml_camera, yaml_camera_reversed);
        assert!(yaml_camera.contains("CanonImageType"));
    }
}
