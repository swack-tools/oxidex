//! STL (Stereolithography) 3D model parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// Known software signatures in binary STL headers
const SOFTWARE_SIGNATURES: &[(&str, &str)] = &[
    ("SolidWorks", "SolidWorks"),
    ("Materialise", "Materialise"),
    ("AutoCAD", "AutoCAD"),
    ("Blender", "Blender"),
    ("OpenSCAD", "OpenSCAD"),
];

#[derive(Debug)]
struct BoundingBox {
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
    min_z: f32,
    max_z: f32,
}

impl BoundingBox {
    fn new() -> Self {
        Self {
            min_x: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            min_y: f32::INFINITY,
            max_y: f32::NEG_INFINITY,
            min_z: f32::INFINITY,
            max_z: f32::NEG_INFINITY,
        }
    }

    fn update(&mut self, x: f32, y: f32, z: f32) {
        self.min_x = self.min_x.min(x);
        self.max_x = self.max_x.max(x);
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);
        self.min_z = self.min_z.min(z);
        self.max_z = self.max_z.max(z);
    }
}

/// Parser for STL (Stereolithography) 3D model files
///
/// Extracts metadata from STL files used in 3D printing and CAD applications.
pub struct STLParser;

impl STLParser {
    /// Verifies the STL file signature (supports both ASCII and binary formats)
    ///
    /// The binary half of this used to be `reader.size() >= 84`, a pure
    /// length test with no content bearing at all: every file of 84+ bytes
    /// passed as binary STL, and `parse_binary` then read arbitrary bytes as
    /// a solid name, a triangle count up to 4.29 billion, and `f32` vertex
    /// coordinates (NaN/Inf included) for whatever the file actually was.
    /// The real invariant was already being computed one function away, into
    /// a `FileSizeValid` *tag* rather than used as a gate -- promoted here.
    ///
    /// The ASCII half now shares the detector's `looks_like_stl`, which is
    /// the anchored, facet-corroborated version of the same
    /// `starts_with(b"solid")` check this parser had by itself before -- see
    /// that function's docs for why an unadorned "solid ..." is not enough.
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 6 {
            return Ok(false);
        }
        let text_probe_len = reader.size().min(64 * 1024) as usize;
        let text_probe = reader.read(0, text_probe_len)?;
        if crate::parsers::detection::text::looks_like_stl(&String::from_utf8_lossy(text_probe)) {
            return Ok(true);
        }
        Self::looks_like_binary(reader)
    }

    /// Whether the file's length matches the triangle count its own binary
    /// STL header declares: `84 + 50 * count`. STL has no ExifTool magic
    /// number to defer to (ExifTool does not support the format at all), so
    /// this is the strongest content check available -- it is what
    /// `parse_binary` already computed as `FileSizeValid`.
    fn looks_like_binary(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 84 {
            return Ok(false);
        }
        let count_bytes = reader.read(80, 4)?;
        let count_reader = EndianReader::little_endian(count_bytes);
        let Some(triangle_count) = count_reader.u32_at(0) else {
            return Ok(false);
        };
        let expected_size = 84 + (u64::from(triangle_count) * 50);
        Ok(reader.size() == expected_size)
    }

    fn detect_format(reader: &dyn FileReader) -> Result<bool> {
        Ok(reader.size() >= 6 && &reader.read(0, 6)?[0..5] == b"solid")
    }

    fn insert_bbox(metadata: &mut MetadataMap, bbox: &BoundingBox) {
        if bbox.min_x.is_finite() {
            metadata.insert(
                "BoundingBoxMinX".to_string(),
                TagValue::Float(bbox.min_x as f64),
            );
            metadata.insert(
                "BoundingBoxMaxX".to_string(),
                TagValue::Float(bbox.max_x as f64),
            );
            metadata.insert(
                "BoundingBoxMinY".to_string(),
                TagValue::Float(bbox.min_y as f64),
            );
            metadata.insert(
                "BoundingBoxMaxY".to_string(),
                TagValue::Float(bbox.max_y as f64),
            );
            metadata.insert(
                "BoundingBoxMinZ".to_string(),
                TagValue::Float(bbox.min_z as f64),
            );
            metadata.insert(
                "BoundingBoxMaxZ".to_string(),
                TagValue::Float(bbox.max_z as f64),
            );
        }
    }

    fn parse_ascii(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let read_size = (reader.size().min(1024 * 1024)) as usize;
        let data = reader.read(0, read_size)?;
        let content = String::from_utf8_lossy(data);

        // Extract solid name from first line
        if let Some(name) = content
            .lines()
            .next()
            .and_then(|l| l.strip_prefix("solid "))
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
        {
            metadata.insert("SolidName".to_string(), TagValue::String(name.to_string()));
        }

        // Count triangles and calculate bounding box
        let mut triangle_count = 0u32;
        let mut bbox = BoundingBox::new();
        let mut lines = content.lines();

        while let Some(line) = lines.next() {
            if line.trim().starts_with("facet") {
                triangle_count += 1;
                for _ in 0..2 {
                    if lines.next().is_some_and(|l| l.trim() == "outer loop") {
                        break;
                    }
                }
                for _ in 0..3 {
                    if let Some(coords) =
                        lines.next().and_then(|l| l.trim().strip_prefix("vertex "))
                    {
                        let parts: Vec<&str> = coords.split_whitespace().collect();
                        if parts.len() >= 3
                            && let (Ok(x), Ok(y), Ok(z)) =
                                (parts[0].parse(), parts[1].parse(), parts[2].parse())
                        {
                            bbox.update(x, y, z);
                        }
                    }
                }
            }
        }

        metadata.insert(
            "TriangleCount".to_string(),
            TagValue::Integer(triangle_count as i64),
        );
        Self::insert_bbox(&mut metadata, &bbox);

        Ok(metadata)
    }

    fn parse_binary(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        if reader.size() < 84 {
            return Err(ExifToolError::parse_error("Binary STL too small"));
        }

        let header = reader.read(0, 80)?;
        let header_str = String::from_utf8_lossy(header);

        // Check for software signatures and extract solid name
        for (signature, software) in SOFTWARE_SIGNATURES {
            if header_str.contains(signature) {
                metadata.insert(
                    "Software".to_string(),
                    TagValue::String(software.to_string()),
                );
                break;
            }
        }
        let header_trimmed = header_str.trim_matches('\0').trim();
        if !header_trimmed.is_empty()
            && header_trimmed
                .chars()
                .all(|c| c.is_ascii_graphic() || c.is_whitespace())
        {
            metadata.insert(
                "SolidName".to_string(),
                TagValue::String(header_trimmed.to_string()),
            );
        }

        // Binary STL uses little-endian byte order
        let count_bytes = reader.read(80, 4)?;
        let count_reader = EndianReader::little_endian(count_bytes);
        let triangle_count = count_reader.u32_at(0).unwrap_or(0);
        metadata.insert(
            "TriangleCount".to_string(),
            TagValue::Integer(triangle_count as i64),
        );

        let expected_size = 84 + (triangle_count as u64 * 50);
        metadata.insert(
            "FileSizeValid".to_string(),
            TagValue::String(
                if reader.size() == expected_size {
                    "Yes"
                } else {
                    "No"
                }
                .to_string(),
            ),
        );

        let mut bbox = BoundingBox::new();
        for i in 0..triangle_count {
            let offset = 84 + (i as u64 * 50);
            if offset + 50 > reader.size() {
                break;
            }
            let triangle_data = reader.read(offset, 50)?;
            let triangle_reader = EndianReader::little_endian(triangle_data);
            for j in 0..3 {
                let base = 12 + j * 12;
                let x = triangle_reader.f32_at(base).unwrap_or(0.0);
                let y = triangle_reader.f32_at(base + 4).unwrap_or(0.0);
                let z = triangle_reader.f32_at(base + 8).unwrap_or(0.0);
                bbox.update(x, y, z);
            }
        }
        Self::insert_bbox(&mut metadata, &bbox);

        Ok(metadata)
    }
}

impl FormatParser for STLParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid STL signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("STL".to_string()));

        let is_ascii = Self::detect_format(reader)?;
        metadata.insert(
            "STLFormat".to_string(),
            TagValue::String(if is_ascii { "ASCII" } else { "Binary" }.to_string()),
        );

        let format_metadata = if is_ascii {
            self.parse_ascii(reader)?
        } else {
            self.parse_binary(reader)?
        };
        for (key, value) in format_metadata {
            metadata.insert(key, value);
        }
        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::STL)
    }
}

/// Parses metadata from STL files.
///
/// This is a convenience wrapper around STLParser that provides a functional API.
pub fn parse_stl_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = STLParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    fn binary_stl(triangle_count: u32) -> Vec<u8> {
        let mut data = vec![0u8; 80];
        data.extend_from_slice(&triangle_count.to_le_bytes());
        data.extend(std::iter::repeat_n(0u8, triangle_count as usize * 50));
        data
    }

    /// The old gate was `reader.size() >= 84`, a pure length test: every file
    /// of 84+ bytes -- including an arbitrary binary blob with no relation to
    /// STL at all -- passed as binary STL and had its bytes reinterpreted as
    /// a triangle count and vertex floats.
    #[test]
    fn arbitrary_bytes_past_84_are_not_stl() {
        let mut data = vec![0xABu8; 200];
        // Whatever u32 happens to sit at offset 80..84 in this junk, it will
        // essentially never satisfy `84 + 50*count == size`.
        data[80..84].copy_from_slice(&0x41414141u32.to_le_bytes());
        let reader = TestReader::new(data);
        assert!(!STLParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn a_binary_stl_whose_size_matches_its_triangle_count_is_stl() {
        let reader = TestReader::new(binary_stl(3));
        assert!(STLParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn a_truncated_binary_stl_is_rejected() {
        let mut data = binary_stl(3);
        data.truncate(data.len() - 10); // one triangle short
        let reader = TestReader::new(data);
        assert!(!STLParser::verify_signature(&reader).unwrap());
    }

    /// The ASCII half now shares the detector's `looks_like_stl`, so prose
    /// merely opening "solid " -- previously accepted by `header[0..5] ==
    /// b"solid"` -- needs the same facet/endsolid corroboration to pass.
    #[test]
    fn prose_opening_solid_without_a_facet_is_not_ascii_stl() {
        let reader = TestReader::new(
            b"solid state drives were bought in bulk this year for the new fleet.\n".to_vec(),
        );
        assert!(!STLParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn a_real_ascii_stl_is_still_stl() {
        let data = concat!(
            "solid cube\n",
            "  facet normal 0.0 0.0 1.0\n",
            "    outer loop\n",
            "      vertex 0.0 0.0 0.0\n",
            "    endloop\n",
            "  endfacet\n",
            "endsolid cube\n",
        );
        let reader = TestReader::new(data.as_bytes().to_vec());
        assert!(STLParser::verify_signature(&reader).unwrap());
    }
}
