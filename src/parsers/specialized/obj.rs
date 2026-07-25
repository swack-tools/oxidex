//! Wavefront OBJ 3D model parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};

/// Parser for Wavefront OBJ 3D model files
///
/// Extracts metadata from OBJ text-based 3D geometry description files.
pub struct OBJParser;

impl OBJParser {
    /// Verifies the OBJ file by checking for vertex/normal/texture coordinate definitions
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 10 {
            return Ok(false);
        }
        let header = reader.read(0, 100.min(reader.size() as usize))?;
        let text = std::str::from_utf8(header).unwrap_or("");
        Ok(text.contains("v ") || text.contains("vn ") || text.contains("vt "))
    }
}

impl FormatParser for OBJParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid OBJ signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("OBJ".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Read entire file (up to 10MB) for comprehensive parsing
        const MAX_SIZE: usize = 10 * 1024 * 1024;
        let size = reader.size() as usize;
        if size > MAX_SIZE {
            return Ok(metadata); // Return basic metadata if file too large
        }

        let content = reader.read(0, size)?;
        let text = std::str::from_utf8(content)
            .map_err(|_| ExifToolError::parse_error("Invalid UTF-8 in OBJ file"))?;

        // Counters for various elements
        let mut vertex_count = 0u32;
        let mut face_count = 0u32;
        let mut normal_count = 0u32;
        let mut texture_coord_count = 0u32;

        // Collections for unique values
        let mut object_names = Vec::new();
        let mut group_names = Vec::new();
        let mut materials = Vec::new();
        let mut material_library: Option<String> = None;

        // Parse line by line
        for line in text.lines() {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                continue;
            }

            // Check line prefix
            if trimmed.starts_with("v ") {
                vertex_count += 1;
            } else if trimmed.starts_with("f ") {
                face_count += 1;
            } else if trimmed.starts_with("vn ") {
                normal_count += 1;
            } else if trimmed.starts_with("vt ") {
                texture_coord_count += 1;
            } else if trimmed.starts_with("o ") {
                if let Some(name) = trimmed.strip_prefix("o ") {
                    let name = name.trim();
                    if !name.is_empty() {
                        object_names.push(name.to_string());
                    }
                }
            } else if trimmed.starts_with("g ") {
                if let Some(name) = trimmed.strip_prefix("g ") {
                    let name = name.trim();
                    if !name.is_empty() {
                        group_names.push(name.to_string());
                    }
                }
            } else if trimmed.starts_with("mtllib ") {
                if let Some(lib) = trimmed.strip_prefix("mtllib ") {
                    material_library = Some(lib.trim().to_string());
                }
            } else if trimmed.starts_with("usemtl ")
                && let Some(mat) = trimmed.strip_prefix("usemtl ")
            {
                let mat = mat.trim().to_string();
                if !mat.is_empty() && !materials.contains(&mat) {
                    materials.push(mat);
                }
            }
        }

        // Insert counts
        if vertex_count > 0 {
            metadata.insert(
                "VertexCount".to_string(),
                TagValue::Integer(vertex_count as i64),
            );
        }
        if face_count > 0 {
            metadata.insert(
                "FaceCount".to_string(),
                TagValue::Integer(face_count as i64),
            );
        }
        if normal_count > 0 {
            metadata.insert(
                "NormalCount".to_string(),
                TagValue::Integer(normal_count as i64),
            );
        }
        if texture_coord_count > 0 {
            metadata.insert(
                "TextureCoordCount".to_string(),
                TagValue::Integer(texture_coord_count as i64),
            );
        }

        // Insert boolean flags
        metadata.insert(
            "HasNormals".to_string(),
            TagValue::String(if normal_count > 0 { "Yes" } else { "No" }.to_string()),
        );
        metadata.insert(
            "HasTextureCoords".to_string(),
            TagValue::String(if texture_coord_count > 0 { "Yes" } else { "No" }.to_string()),
        );

        // Insert collections
        if !object_names.is_empty() {
            metadata.insert(
                "ObjectNames".to_string(),
                TagValue::String(object_names.join(", ")),
            );
        }
        if !group_names.is_empty() {
            metadata.insert(
                "GroupNames".to_string(),
                TagValue::String(group_names.join(", ")),
            );
        }
        if let Some(lib) = material_library {
            metadata.insert("MaterialLibrary".to_string(), TagValue::String(lib));
        }
        if !materials.is_empty() {
            metadata.insert(
                "Materials".to_string(),
                TagValue::String(materials.join(", ")),
            );
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::OBJ)
    }
}

/// Parses metadata from OBJ files.
///
/// This is a convenience wrapper around OBJParser that provides a functional API.
pub fn parse_obj_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = OBJParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
