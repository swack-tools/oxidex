//! AutoCAD DXF (Drawing Exchange Format) parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use std::collections::HashMap;

/// Parser for AutoCAD DXF (Drawing Exchange Format) files
///
/// Extracts metadata from DXF text-based drawing interchange files.
pub struct DXFParser;

impl DXFParser {
    /// Verifies the DXF file signature by checking for the characteristic "0\nSECTION" header
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 20 {
            return Ok(false);
        }
        let header = reader.read(0, 20)?;
        let text = std::str::from_utf8(header).unwrap_or("");
        Ok(text.starts_with("0\n") && text.contains("SECTION"))
    }

    fn parse_content(reader: &dyn FileReader) -> Result<DXFContent> {
        let read_size = (256 * 1024).min(reader.size() as usize);
        let data = reader.read(0, read_size)?;
        let text = std::str::from_utf8(data)
            .map_err(|_| ExifToolError::parse_error("Invalid UTF-8 in DXF file"))?;

        let mut content = DXFContent::default();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;

        while i < lines.len() && i + 1 < lines.len() {
            let group_code = lines[i].trim();
            let value = lines[i + 1].trim();

            if group_code == "0" && value == "SECTION" && i + 3 < lines.len() {
                i = Self::parse_section(&lines, i + 4, lines[i + 3].trim(), &mut content);
            } else if group_code == "0" && content.in_entities {
                Self::count_entity(value, &mut content);
                i += 2;
            } else {
                i += 2;
            }
        }
        Ok(content)
    }

    fn parse_section(
        lines: &[&str],
        start: usize,
        section_name: &str,
        content: &mut DXFContent,
    ) -> usize {
        let mut i = start;

        if section_name == "HEADER" {
            content.in_header = true;
        } else if section_name == "TABLES" {
            content.in_tables = true;
        } else if section_name == "ENTITIES" {
            content.in_entities = true;
        }

        while i < lines.len() {
            let group_code = lines[i].trim();
            if i + 1 >= lines.len() {
                break;
            }
            let value = lines[i + 1].trim();

            if group_code == "0" {
                if value == "ENDSEC" {
                    content.in_header = false;
                    content.in_tables = false;
                    content.in_entities = false;
                    return i + 2;
                }
                if content.in_tables && value == "LAYER" {
                    content.layer_count += 1;
                } else if content.in_entities {
                    Self::count_entity(value, content);
                }
            } else if group_code == "9" && content.in_header && i + 3 < lines.len() {
                let var_name = value;
                let next_group = lines[i + 2].trim();
                let var_value = lines[i + 3].trim();

                if (var_name == "$EXTMIN" || var_name == "$EXTMAX")
                    && next_group == "10"
                    && i + 9 < lines.len()
                {
                    let prefix = if var_name == "$EXTMIN" {
                        "EXTMIN"
                    } else {
                        "EXTMAX"
                    };
                    content
                        .header_vars
                        .insert(format!("${}_X", prefix), var_value.to_string());
                    content
                        .header_vars
                        .insert(format!("${}_Y", prefix), lines[i + 5].trim().to_string());
                    content
                        .header_vars
                        .insert(format!("${}_Z", prefix), lines[i + 7].trim().to_string());
                    i += 8;
                    continue;
                }
                content
                    .header_vars
                    .insert(var_name.to_string(), var_value.to_string());
                i += 4;
                continue;
            }
            i += 2;
        }
        i
    }

    fn count_entity(entity_type: &str, content: &mut DXFContent) {
        match entity_type {
            "LINE" => content.line_count += 1,
            "CIRCLE" => content.circle_count += 1,
            "ARC" => content.arc_count += 1,
            "TEXT" | "MTEXT" => content.text_count += 1,
            "LWPOLYLINE" | "POLYLINE" => content.polyline_count += 1,
            "POINT" => content.point_count += 1,
            "ELLIPSE" => content.ellipse_count += 1,
            "SPLINE" => content.spline_count += 1,
            _ => {}
        }
    }

    fn map_version(version: &str) -> &str {
        match version {
            "AC1009" => "AutoCAD R12",
            "AC1012" => "AutoCAD R13",
            "AC1014" => "AutoCAD R14",
            "AC1015" => "AutoCAD 2000",
            "AC1018" => "AutoCAD 2004",
            "AC1021" => "AutoCAD 2007",
            "AC1024" => "AutoCAD 2010",
            "AC1027" => "AutoCAD 2013",
            "AC1032" => "AutoCAD 2018",
            _ => version,
        }
    }

    fn map_units(code: &str) -> &str {
        match code {
            "0" => "Unitless",
            "1" => "Inches",
            "2" => "Feet",
            "3" => "Miles",
            "4" => "Millimeters",
            "5" => "Centimeters",
            "6" => "Meters",
            "7" => "Kilometers",
            "8" => "Microinches",
            "9" => "Mils",
            "10" => "Yards",
            "11" => "Angstroms",
            "12" => "Nanometers",
            "13" => "Microns",
            "14" => "Decimeters",
            _ => code,
        }
    }
}

impl FormatParser for DXFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid DXF signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("DXF".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        let content = Self::parse_content(reader)?;

        // Extract AutoCAD version
        if let Some(version) = content.header_vars.get("$ACADVER") {
            metadata.insert(
                "AutoCADVersion".to_string(),
                TagValue::String(Self::map_version(version).to_string()),
            );
        }

        // Extract drawing units
        if let Some(units) = content.header_vars.get("$INSUNITS") {
            metadata.insert(
                "DrawingUnits".to_string(),
                TagValue::String(Self::map_units(units).to_string()),
            );
        }

        // Extract drawing extents (bounding box)
        if let Some(x) = content.header_vars.get("$EXTMIN_X")
            && let Some(y) = content.header_vars.get("$EXTMIN_Y")
        {
            metadata.insert(
                "ExtentMin".to_string(),
                TagValue::String(format!("{}, {}", x, y)),
            );
        }
        if let Some(x) = content.header_vars.get("$EXTMAX_X")
            && let Some(y) = content.header_vars.get("$EXTMAX_Y")
        {
            metadata.insert(
                "ExtentMax".to_string(),
                TagValue::String(format!("{}, {}", x, y)),
            );
        }

        // Entity and layer counts
        let counts = [
            ("LineCount", content.line_count),
            ("CircleCount", content.circle_count),
            ("ArcCount", content.arc_count),
            ("TextCount", content.text_count),
            ("PolylineCount", content.polyline_count),
            ("PointCount", content.point_count),
            ("EllipseCount", content.ellipse_count),
            ("SplineCount", content.spline_count),
            ("LayerCount", content.layer_count),
        ];
        for (name, count) in counts {
            if count > 0 {
                metadata.insert(name.to_string(), TagValue::Integer(count as i64));
            }
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::DXF)
    }
}

/// Internal structure for tracking parsed DXF content
#[derive(Default)]
struct DXFContent {
    header_vars: HashMap<String, String>,
    in_header: bool,
    in_tables: bool,
    in_entities: bool,
    layer_count: u64,
    line_count: u64,
    circle_count: u64,
    arc_count: u64,
    text_count: u64,
    polyline_count: u64,
    point_count: u64,
    ellipse_count: u64,
    spline_count: u64,
}

/// Parses metadata from DXF files.
///
/// This is a convenience wrapper around DXFParser that provides a functional API.
pub fn parse_dxf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = DXFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
