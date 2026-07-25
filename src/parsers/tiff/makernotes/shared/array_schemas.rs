//! Array schema system for declarative CameraSettings-style array parsing
//!
//! Many MakerNote parsers extract arrays of i16/u16/i32 values with specific
//! indices mapping to camera settings. This module provides declarative schemas
//! to eliminate repetitive array extraction code.

use super::generic_decoders::SimpleValueDecoder;
use std::collections::HashMap;

/// Definition of a single array index with its name and optional decoder
#[derive(Debug, Clone)]
pub struct ArrayIndexDef {
    /// Array index (0-based)
    pub index: usize,
    /// Tag name for this index (e.g., "MacroMode", "Quality")
    pub name: &'static str,
    /// Optional decoder for i16 values at this index
    pub decoder_i16: Option<&'static SimpleValueDecoder<i16>>,
    /// Optional decoder for u16 values at this index
    pub decoder_u16: Option<&'static SimpleValueDecoder<u16>>,
    /// Optional decoder for i32 values at this index
    pub decoder_i32: Option<&'static SimpleValueDecoder<i32>>,
    /// Optional decoder for u32 values at this index
    pub decoder_u32: Option<&'static SimpleValueDecoder<u32>>,
}

impl ArrayIndexDef {
    /// Create a new index definition with i16 decoder
    pub const fn with_i16_decoder(
        index: usize,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<i16>,
    ) -> Self {
        Self {
            index,
            name,
            decoder_i16: Some(decoder),
            decoder_u16: None,
            decoder_i32: None,
            decoder_u32: None,
        }
    }

    /// Create a new index definition with u16 decoder
    pub const fn with_u16_decoder(
        index: usize,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<u16>,
    ) -> Self {
        Self {
            index,
            name,
            decoder_i16: None,
            decoder_u16: Some(decoder),
            decoder_i32: None,
            decoder_u32: None,
        }
    }

    /// Create a new index definition with i32 decoder
    pub const fn with_i32_decoder(
        index: usize,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<i32>,
    ) -> Self {
        Self {
            index,
            name,
            decoder_i16: None,
            decoder_u16: None,
            decoder_i32: Some(decoder),
            decoder_u32: None,
        }
    }

    /// Create a new index definition with u32 decoder
    pub const fn with_u32_decoder(
        index: usize,
        name: &'static str,
        decoder: &'static SimpleValueDecoder<u32>,
    ) -> Self {
        Self {
            index,
            name,
            decoder_i16: None,
            decoder_u16: None,
            decoder_i32: None,
            decoder_u32: Some(decoder),
        }
    }

    /// Create a new index definition without decoder (raw value)
    pub const fn raw(index: usize, name: &'static str) -> Self {
        Self {
            index,
            name,
            decoder_i16: None,
            decoder_u16: None,
            decoder_i32: None,
            decoder_u32: None,
        }
    }
}

/// Schema defining how to parse an array of values
///
/// Example:
/// ```ignore
/// static CAMERA_SETTINGS: ArraySchema = ArraySchema {
///     name: "CameraSettings",
///     indices: &[
///         ArrayIndexDef::with_i16_decoder(1, "MacroMode", &MACRO_MODE),
///         ArrayIndexDef::with_i16_decoder(2, "SelfTimer", &SELF_TIMER),
///         ArrayIndexDef::raw(3, "Quality"),
///     ],
/// };
/// ```
#[derive(Debug)]
pub struct ArraySchema {
    /// Schema name (e.g., "CameraSettings", "ShotInfo")
    pub name: &'static str,
    /// Index definitions
    pub indices: &'static [ArrayIndexDef],
}

impl ArraySchema {
    /// Process an i16 array using this schema
    pub fn process_i16_array(
        &self,
        array: &[i16],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        for def in self.indices {
            if let Some(value) = array.get(def.index) {
                let decoded = if let Some(decoder) = def.decoder_i16 {
                    decoder.decode(*value)
                } else {
                    value.to_string()
                };
                tags.insert(format!("{}:{}:{}", prefix, self.name, def.name), decoded);
            }
        }
    }

    /// Process a u16 array using this schema
    pub fn process_u16_array(
        &self,
        array: &[u16],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        for def in self.indices {
            if let Some(value) = array.get(def.index) {
                let decoded = if let Some(decoder) = def.decoder_u16 {
                    decoder.decode(*value)
                } else {
                    value.to_string()
                };
                tags.insert(format!("{}:{}:{}", prefix, self.name, def.name), decoded);
            }
        }
    }

    /// Process an i32 array using this schema
    pub fn process_i32_array(
        &self,
        array: &[i32],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        for def in self.indices {
            if let Some(value) = array.get(def.index) {
                let decoded = if let Some(decoder) = def.decoder_i32 {
                    decoder.decode(*value)
                } else {
                    value.to_string()
                };
                tags.insert(format!("{}:{}:{}", prefix, self.name, def.name), decoded);
            }
        }
    }

    /// Process a u32 array using this schema
    pub fn process_u32_array(
        &self,
        array: &[u32],
        prefix: &str,
        tags: &mut HashMap<String, String>,
    ) {
        for def in self.indices {
            if let Some(value) = array.get(def.index) {
                let decoded = if let Some(decoder) = def.decoder_u32 {
                    decoder.decode(*value)
                } else {
                    value.to_string()
                };
                tags.insert(format!("{}:{}:{}", prefix, self.name, def.name), decoded);
            }
        }
    }
}
