//! Canon MakerNote parser
//!
//! Parses Canon-specific EXIF MakerNote tags containing camera settings,
//! lens information, focus data, and other proprietary metadata.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::error::Result;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

// Canon MakerNote Tag IDs
const CANON_CAMERA_SETTINGS: u16 = 0x0001;
const CANON_FOCAL_LENGTH: u16 = 0x0002;
const CANON_SHOT_INFO: u16 = 0x0004;
const CANON_PANORAMA: u16 = 0x0005;
const CANON_IMAGE_TYPE: u16 = 0x0006;
const CANON_FIRMWARE_VERSION: u16 = 0x0007;
const CANON_FILE_NUMBER: u16 = 0x0008;
const CANON_OWNER_NAME: u16 = 0x0009;
const CANON_SERIAL_NUMBER: u16 = 0x000C;
const CANON_CAMERA_INFO: u16 = 0x000D;
const CANON_CUSTOM_FUNCTIONS: u16 = 0x000F;
const CANON_MODEL_ID: u16 = 0x0010;

// Canon signature (not always present)
const CANON_SIGNATURE: &[u8] = b"Canon";

/// Represents a Canon MakerNote tag value
#[derive(Debug, Clone, PartialEq)]
pub enum CanonTagValue {
    /// Single integer value
    Integer(i32),
    /// String value (model name, firmware, etc.)
    String(String),
    /// Array of integers (camera settings, shot info)
    IntArray(Vec<i16>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_canon_tag_ids() {
        assert_eq!(CANON_CAMERA_SETTINGS, 0x0001);
        assert_eq!(CANON_FOCAL_LENGTH, 0x0002);
        assert_eq!(CANON_SHOT_INFO, 0x0004);
        assert_eq!(CANON_MODEL_ID, 0x0010);
    }

    #[test]
    fn test_canon_signature() {
        assert_eq!(CANON_SIGNATURE, b"Canon");
    }
}
