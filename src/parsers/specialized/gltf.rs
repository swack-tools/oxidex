//! glTF (GL Transmission Format) 3D model parser

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// Parser for glTF (GL Transmission Format) 3D model files
///
/// Extracts metadata from glTF JSON-based 3D scene description files
/// and GLB (binary glTF) files.
pub struct GLTFParser;

impl GLTFParser {
    /// Verifies the glTF file by checking for JSON structure with "asset" field
    /// or GLB binary signature
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 12 {
            return Ok(false);
        }

        // Check for GLB binary format (magic: "glTF")
        let header = reader.read(0, 12)?;
        if header.len() >= 4 && &header[0..4] == b"glTF" {
            return Ok(true);
        }

        // Check for JSON-based glTF
        let preview = reader.read(0, 100.min(reader.size() as usize))?;
        let text = std::str::from_utf8(preview).unwrap_or("");
        Ok(text.contains("\"asset\"") && text.contains("{"))
    }

    /// Detects whether the file is JSON-based glTF or binary GLB
    fn detect_format(reader: &dyn FileReader) -> Result<&'static str> {
        if reader.size() < 4 {
            return Ok("Unknown");
        }
        let header = reader.read(0, 4)?;
        if &header[0..4] == b"glTF" {
            Ok("GLB")
        } else {
            Ok("glTF")
        }
    }

    /// Extracts the JSON content from either glTF or GLB format
    fn extract_json_content(reader: &dyn FileReader) -> Result<String> {
        let format = Self::detect_format(reader)?;

        if format == "GLB" {
            // GLB format: 12-byte header + chunks
            // Header: magic(4) + version(4) + length(4)
            // First chunk is typically JSON
            if reader.size() < 20 {
                return Err(ExifToolError::parse_error("GLB file too small"));
            }

            // Read chunk header: length(4) + type(4)
            // GLB uses little-endian byte order
            let chunk_header = reader.read(12, 8)?;
            let chunk_reader = EndianReader::little_endian(chunk_header);
            let chunk_length = chunk_reader.u32_at(0).unwrap_or(0) as usize;

            // Read JSON chunk data
            let json_data = reader.read(20, chunk_length)?;
            String::from_utf8(json_data.to_vec())
                .map_err(|_| ExifToolError::parse_error("Invalid UTF-8 in GLB JSON chunk"))
        } else {
            // Regular glTF JSON file
            let size = reader.size() as usize;
            let data = reader.read(0, size)?;
            String::from_utf8(data.to_vec())
                .map_err(|_| ExifToolError::parse_error("Invalid UTF-8 in glTF file"))
        }
    }

    /// Extracts a string value from JSON using simple string parsing
    fn extract_json_string(json: &str, key: &str) -> Option<String> {
        let search_key = format!("\"{}\"", key);
        let start = json.find(&search_key)?;
        let after_key = &json[start + search_key.len()..];
        let colon_pos = after_key.find(':')?;
        let after_colon = &after_key[colon_pos + 1..].trim_start();

        if let Some(rest) = after_colon.strip_prefix('"') {
            let end_quote = rest.find('"')?;
            Some(rest[..end_quote].to_string())
        } else {
            None
        }
    }

    /// Counts array elements in JSON using simple parsing
    fn count_json_array(json: &str, key: &str) -> Option<usize> {
        let search_key = format!("\"{}\"", key);
        let start = json.find(&search_key)?;
        let after_key = &json[start + search_key.len()..];
        let colon_pos = after_key.find(':')?;
        let after_colon = &after_key[colon_pos + 1..].trim_start();

        if let Some(array_content) = after_colon.strip_prefix('[') {
            // Find matching closing bracket
            let mut depth = 1; // Already inside the first '['
            let mut count = 0;
            let mut in_string = false;
            let mut escape_next = false;

            // Check if array is non-empty
            if !array_content.trim_start().starts_with(']') {
                count = 1;
            }

            for ch in array_content.chars() {
                if escape_next {
                    escape_next = false;
                    continue;
                }

                match ch {
                    '\\' if in_string => escape_next = true,
                    '"' => in_string = !in_string,
                    '[' if !in_string => {
                        depth += 1;
                    }
                    ']' if !in_string => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    ',' if !in_string && depth == 1 => count += 1,
                    _ => {}
                }
            }

            Some(count)
        } else {
            None
        }
    }

    /// Extracts version from GLB binary header
    /// GLB uses little-endian byte order
    fn extract_glb_version(reader: &dyn FileReader) -> Option<u32> {
        if reader.size() < 8 {
            return None;
        }
        let header = reader.read(4, 4).ok()?;
        let version_reader = EndianReader::little_endian(header);
        version_reader.u32_at(0)
    }
}

impl FormatParser for GLTFParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid GLTF signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("GLTF".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Detect and add format
        let format = Self::detect_format(reader)?;
        metadata.insert("Format".to_string(), TagValue::String(format.to_string()));

        // Extract JSON content
        let json_content = match Self::extract_json_content(reader) {
            Ok(content) => content,
            Err(_) => return Ok(metadata), // Return basic metadata if JSON extraction fails
        };

        // Extract asset information
        // Version is required in glTF asset, so try both locations
        if let Some(version) = Self::extract_json_string(&json_content, "version") {
            metadata.insert(
                "AssetVersion".to_string(),
                TagValue::String(version.clone()),
            );
            // Add GLTF:Version for Worker 25 compatibility
            metadata.insert("GLTF:Version".to_string(), TagValue::new_string(version));
        }

        if let Some(generator) = Self::extract_json_string(&json_content, "generator") {
            metadata.insert(
                "AssetGenerator".to_string(),
                TagValue::String(generator.clone()),
            );
            // Add GLTF:Generator for Worker 25 compatibility
            metadata.insert(
                "GLTF:Generator".to_string(),
                TagValue::new_string(generator),
            );
        }

        if let Some(copyright) = Self::extract_json_string(&json_content, "copyright") {
            metadata.insert(
                "AssetCopyright".to_string(),
                TagValue::String(copyright.clone()),
            );
            // Add GLTF:Copyright for Worker 25 compatibility
            metadata.insert(
                "GLTF:Copyright".to_string(),
                TagValue::new_string(copyright),
            );
        }

        // Count array elements for both old and new tag names
        let arrays = [
            ("scenes", "SceneCount", "GLTF:SceneCount"),
            ("nodes", "NodeCount", "GLTF:NodeCount"),
            ("meshes", "MeshCount", "GLTF:MeshCount"),
            ("materials", "MaterialCount", "GLTF:MaterialCount"),
            ("textures", "TextureCount", "GLTF:TextureCount"),
            ("animations", "AnimationCount", "GLTF:AnimationCount"),
        ];

        for (json_key, meta_key, gltf_key) in &arrays {
            if let Some(count) = Self::count_json_array(&json_content, json_key) {
                let count_i64 = count as i64;
                // Add both old and new tag names for compatibility
                metadata.insert(meta_key.to_string(), TagValue::Integer(count_i64));
                metadata.insert(gltf_key.to_string(), TagValue::new_integer(count_i64));
            }
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::GLTF)
    }
}

/// Parses metadata from glTF files.
///
/// This is a convenience wrapper around GLTFParser that provides a functional API.
pub fn parse_gltf_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = GLTFParser;
    parser.parse(reader).map_err(|e| e.to_string())
}
