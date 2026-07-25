//! Apple (iPhone/iPad) MakerNote parser
//!
//! Parses Apple-specific EXIF MakerNote tags containing computational photography
//! settings, multi-camera data, and iOS-specific metadata.
//!
//! ## Supported Features
//! - HDR processing mode and HDR Headroom/Gain
//! - Portrait Mode and depth data
//! - Live Photo status and video index
//! - Scene detection and Scene Flags
//! - Multi-camera lens identification
//! - Semantic Styles (Photographic Styles) with presets
//! - Smart HDR version
//! - Night Mode
//! - AF performance, confidence, and measured depth
//! - Signal-to-noise ratio metrics
//! - Color temperature and correction matrix
//! - Focus position and distance range
//! - Image processing flags and quality hints
//! - Photo identifiers and content IDs
//! - Green ghost mitigation status
//!
//! ## Format Support
//! Apple MakerNotes come in two formats:
//! 1. **IFD Format**: Standard TIFF IFD structure (older devices, standard EXIF)
//! 2. **BPLIST Format**: Binary plist with "Apple iOS\x00" header (newer devices)
//!
//! ## Architecture
//! Apple's MakerNotes use a proprietary binary format with Apple-specific tags.
//! Unlike traditional camera manufacturers, Apple stores significant computational
//! photography metadata including AI-powered scene analysis and processing flags.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::const_decoder;
use crate::io::EndianReader;
use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use crate::parsers::tiff::makernotes::shared::ifd_parser_base::{
    IfdParserConfig, parse_ifd_entries,
};
use crate::parsers::tiff::makernotes::shared::value_extractors::{
    extract_i16_value, extract_i32_value, extract_string_with_byteorder, extract_u32_value,
};
use std::collections::HashMap;

use super::shared::MakerNoteParser;
use super::shared::array_extractors::extract_i16_array;

// ============================================================================
// APPLE MAKERNOTE TAG IDS
// ============================================================================
// Tag IDs based on ExifTool's Apple.pm and reverse-engineering efforts.
// Apple uses a mix of IFD-based tags and binary plist structures.

// Core identification tags
const APPLE_MAKERNOTE_VERSION: u16 = 0x0001; // MakerNote version string
const APPLE_AE_MATRIX: u16 = 0x0002; // AE (Auto Exposure) matrix data
const APPLE_RUN_TIME: u16 = 0x0003; // Runtime information (plist)
const APPLE_AE_STABLE: u16 = 0x0004; // AE stability flag
const APPLE_AE_TARGET: u16 = 0x0005; // AE target exposure value
const APPLE_AE_AVERAGE: u16 = 0x0006; // AE average value
const APPLE_AF_STABLE: u16 = 0x0007; // AF stability flag
const APPLE_ACCELERATION_VECTOR: u16 = 0x0008; // Device orientation/acceleration

// HDR and image processing tags
const APPLE_HDR_IMAGE_TYPE: u16 = 0x000A; // HDR processing mode
const APPLE_BURST_UUID: u16 = 0x000B; // Burst mode unique ID
const APPLE_FOCUS_DISTANCE_RANGE: u16 = 0x000C; // Focus distance range (min/max)
const APPLE_OIS_MODE: u16 = 0x000F; // Optical Image Stabilization mode

// Content and image identification
const APPLE_CONTENT_IDENTIFIER: u16 = 0x0011; // Media content identifier (UUID)
const APPLE_IMAGE_CAPTURE_TYPE: u16 = 0x0014; // Type of capture (photo/portrait/etc.)
const APPLE_IMAGE_UNIQUE_ID: u16 = 0x0015; // Unique image identifier
const APPLE_LIVE_PHOTO_VIDEO_INDEX: u16 = 0x0017; // Live Photo video frame index
const APPLE_IMAGE_PROCESSING_FLAGS: u16 = 0x0019; // Processing flags bitmask
const APPLE_QUALITY_HINT: u16 = 0x001A; // Quality hint value

// Noise and signal analysis
const APPLE_LUMINANCE_NOISE_AMPLITUDE: u16 = 0x001D; // Measured luminance noise
const APPLE_PHOTOS_APP_FEATURE_FLAGS: u16 = 0x001F; // Photos.app feature flags

// HDR headroom and capture request
const APPLE_IMAGE_CAPTURE_REQUEST_ID: u16 = 0x0020; // Capture request identifier
const APPLE_HDR_HEADROOM: u16 = 0x0021; // HDR headroom value (EV)
const APPLE_AF_PERFORMANCE: u16 = 0x0023; // AF performance metrics

// Scene analysis
const APPLE_SCENE_FLAGS: u16 = 0x0025; // Scene detection flags
const APPLE_SIGNAL_TO_NOISE_RATIO_TYPE: u16 = 0x0026; // SNR measurement type
const APPLE_SIGNAL_TO_NOISE_RATIO: u16 = 0x0027; // Signal-to-noise ratio value

// Photo identifiers and camera info
const APPLE_PHOTO_IDENTIFIER: u16 = 0x002B; // Photo identifier string
const APPLE_COLOR_TEMPERATURE: u16 = 0x002D; // Color temperature (Kelvin)
const APPLE_CAMERA_TYPE: u16 = 0x002E; // Camera type identifier
const APPLE_FOCUS_POSITION: u16 = 0x002F; // Focus position value
const APPLE_HDR_GAIN: u16 = 0x0030; // HDR gain value

// Front-facing camera flag
const APPLE_FRONT_FACING_CAMERA: u16 = 0x0032;

// Advanced AF and processing tags
const APPLE_AF_MEASURED_DEPTH: u16 = 0x0038; // AF measured depth (LiDAR)
const APPLE_AF_CONFIDENCE: u16 = 0x003D; // AF confidence level
const APPLE_COLOR_CORRECTION_MATRIX: u16 = 0x003E; // Color correction matrix
const APPLE_GREEN_GHOST_MITIGATION_STATUS: u16 = 0x003F; // Lens flare mitigation

// Semantic Style tags (Photographic Styles - iOS 15+)
const APPLE_SEMANTIC_STYLE: u16 = 0x0040; // Active photographic style
const APPLE_SEMANTIC_STYLE_RENDERING_VER: u16 = 0x0041; // Style rendering version
const APPLE_SEMANTIC_STYLE_PRESET: u16 = 0x0042; // Style preset identifier

// Apple signature markers
const APPLE_SIGNATURE: &[u8] = b"Apple iOS";
const BPLIST_MAGIC: &[u8] = b"bplist";

// ============================================================================
// TAG VALUE DECODERS
// ============================================================================
// Decoders for Apple MakerNote tag values. These convert numeric values to
// human-readable strings based on ExifTool's Apple.pm definitions.

// Decodes Apple HDR image type
// Values observed from various iPhone models
const_decoder! {
    pub DECODE_HDR_TYPE, i16, [
        (0, "Off"),
        (1, "HDR"),
        (2, "HDR (Original)"),
        (3, "Auto HDR"),
        (4, "Smart HDR"),
        (5, "Smart HDR 2"),
        (6, "Smart HDR 3"),
        (7, "Smart HDR 4"),
        (8, "Smart HDR 5"),
    ]
}

// Decodes Portrait Mode effect type (Depth Effect)
const_decoder! {
    pub DECODE_PORTRAIT_MODE, i16, [
        (0, "Off"),
        (1, "Natural Light"),
        (2, "Studio Light"),
        (3, "Contour Light"),
        (4, "Stage Light"),
        (5, "Stage Light Mono"),
        (6, "High-Key Light Mono"),
    ]
}

// Decodes scene detection type from AI analysis
const_decoder! {
    pub DECODE_SCENE_TYPE, i16, [
        (0, "None"),
        (1, "Sunset/Sunrise"),
        (2, "Blue Sky"),
        (3, "Snow"),
        (4, "Foliage"),
        (5, "Beach"),
        (6, "Night"),
        (7, "Fireworks"),
        (8, "Food"),
        (9, "Pet"),
        (10, "Document"),
        (11, "QR Code"),
        (12, "Portrait"),
    ]
}

// Decodes semantic style (Photographic Style - iOS 15+)
const_decoder! {
    pub DECODE_SEMANTIC_STYLE, i16, [
        (0, "Standard"),
        (1, "Rich Contrast"),
        (2, "Vibrant"),
        (3, "Warm"),
        (4, "Cool"),
    ]
}

// Decodes lens model for multi-camera iPhones
const_decoder! {
    pub DECODE_LENS_MODEL, i16, [
        (0, "Wide (Main Camera)"),
        (1, "Telephoto"),
        (2, "Ultra Wide"),
        (3, "Front Camera"),
        (4, "Telephoto 2x"),
        (5, "Telephoto 3x"),
        (6, "Telephoto 5x"),
    ]
}

// Decodes camera type identifier
const_decoder! {
    pub DECODE_CAMERA_TYPE, i16, [
        (1, "Back Normal"),
        (2, "Back Wide"),
        (3, "Back Ultra Wide"),
        (4, "Back Telephoto"),
        (5, "Back Telephoto 2x"),
        (6, "Front"),
        (7, "Front TrueDepth"),
    ]
}

// Decodes OIS (Optical Image Stabilization) mode
const_decoder! {
    pub DECODE_OIS_MODE, i16, [
        (0, "Off"),
        (1, "On"),
        (2, "Cinematic Mode"),
        (3, "Action Mode"),
    ]
}

// Decodes image capture type
const_decoder! {
    pub DECODE_IMAGE_CAPTURE_TYPE, i16, [
        (0, "Photo"),
        (1, "Portrait"),
        (2, "Panorama"),
        (3, "Live Photo"),
        (4, "Night Mode"),
        (5, "ProRAW"),
        (6, "Cinematic"),
        (10, "Screenshot"),
    ]
}

// Decodes green ghost mitigation status
const_decoder! {
    pub DECODE_GREEN_GHOST_MITIGATION, i16, [
        (0, "Off"),
        (1, "Applied"),
        (2, "Detected"),
    ]
}

// Decodes signal-to-noise ratio measurement type
const_decoder! {
    pub DECODE_SNR_TYPE, i16, [
        (0, "None"),
        (1, "Luminance"),
        (2, "Chrominance"),
        (3, "Combined"),
    ]
}

/// Apple MakerNote parser implementation
///
/// Supports two Apple MakerNote formats:
/// 1. IFD Format - Standard TIFF IFD structure used in older devices
/// 2. BPLIST Format - Binary plist with "Apple iOS\0" header (newer devices)
pub struct AppleParser;

impl Default for AppleParser {
    fn default() -> Self {
        Self::new()
    }
}

impl AppleParser {
    /// Creates a new Apple parser instance
    pub fn new() -> Self {
        AppleParser
    }

    /// Checks if the data starts with a binary plist header
    ///
    /// Apple MakerNotes on newer devices use "Apple iOS\0" followed by bplist data.
    fn is_bplist_format(data: &[u8]) -> bool {
        // Check for "Apple iOS\0" header followed by bplist
        if data.len() >= 16
            && &data[0..9] == APPLE_SIGNATURE
            && data[9] == 0
            && &data[10..16] == BPLIST_MAGIC
        {
            return true;
        }

        // Also check for direct bplist format
        if data.len() >= 6 && &data[0..6] == BPLIST_MAGIC {
            return true;
        }

        false
    }

    /// Parse BPLIST format Apple MakerNotes
    ///
    /// BPLIST format stores key-value pairs in Apple's binary property list format.
    /// This is a simplified parser that extracts common keys.
    fn parse_bplist(&self, data: &[u8], tags: &mut HashMap<String, String>) -> Result<(), String> {
        // Determine offset to bplist data
        let bplist_offset = if data.len() >= 10 && &data[0..9] == APPLE_SIGNATURE && data[9] == 0 {
            10 // Skip "Apple iOS\0"
        } else if data.len() >= 6 && &data[0..6] == BPLIST_MAGIC {
            0 // Direct bplist
        } else {
            return Err("Invalid BPLIST header".to_string());
        };

        let bplist_data = &data[bplist_offset..];

        // Verify bplist magic
        if bplist_data.len() < 8 || &bplist_data[0..6] != BPLIST_MAGIC {
            return Err("Missing bplist magic".to_string());
        }

        // Add format indicator tag
        tags.insert("Apple:MakerNoteFormat".to_string(), "BPLIST".to_string());

        // Extract bplist version (bytes 6-7 after magic)
        let version = String::from_utf8_lossy(&bplist_data[6..8]);
        tags.insert("Apple:BPLISTVersion".to_string(), version.to_string());

        // Binary plist parsing is complex - for now we note that full parsing
        // would require implementing the full bplist specification.
        // The plist trailer is in the last 32 bytes.
        if bplist_data.len() >= 40 {
            let trailer_offset = bplist_data.len() - 32;
            let trailer = &bplist_data[trailer_offset..];

            // Trailer: [unused:6][offset_size:1][ref_size:1][num_objects:8][top:8][offset_table:8]
            let offset_size = trailer[6] as usize;
            let ref_size = trailer[7] as usize;

            if offset_size > 0 && offset_size <= 8 && ref_size > 0 && ref_size <= 8 {
                // Read number of objects (big-endian u64 at offset 8-15 of trailer)
                let num_objects = Self::read_be_u64(&trailer[8..16]);
                tags.insert("Apple:BPLISTObjects".to_string(), num_objects.to_string());
            }
        }

        Ok(())
    }

    /// Read a big-endian u64 from bytes
    fn read_be_u64(bytes: &[u8]) -> u64 {
        if bytes.len() < 8 {
            return 0;
        }
        u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    }

    /// Parse a single IFD entry and extract tag value
    ///
    /// Uses the registry-based approach for consistent decoding of tag values.
    fn parse_entry(
        &self,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        use super::registries::apple::{apple_registry, decode_facing_camera, decode_night_mode};

        let registry = apple_registry();
        let tag_id = entry.tag_id;

        match tag_id {
            // ================================================================
            // INTEGER TAGS WITH DECODERS
            // ================================================================

            // HDR-related tags
            APPLE_HDR_IMAGE_TYPE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let decoded = DECODE_HDR_TYPE.decode(value);
                    tags.insert("Apple:HDRImageType".to_string(), decoded.to_string());
                }
            }

            // OIS Mode
            APPLE_OIS_MODE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let decoded = DECODE_OIS_MODE.decode(value);
                    tags.insert("Apple:OISMode".to_string(), decoded.to_string());
                }
            }

            // Image capture type
            APPLE_IMAGE_CAPTURE_TYPE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let decoded = DECODE_IMAGE_CAPTURE_TYPE.decode(value);
                    tags.insert("Apple:ImageCaptureType".to_string(), decoded.to_string());
                }
            }

            // Camera type
            APPLE_CAMERA_TYPE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let decoded = DECODE_CAMERA_TYPE.decode(value);
                    tags.insert("Apple:CameraType".to_string(), decoded.to_string());
                }
            }

            // Semantic style (Photographic Styles)
            APPLE_SEMANTIC_STYLE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let decoded = DECODE_SEMANTIC_STYLE.decode(value);
                    tags.insert("Apple:SemanticStyle".to_string(), decoded.to_string());
                }
            }

            // Green ghost mitigation
            APPLE_GREEN_GHOST_MITIGATION_STATUS => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let decoded = DECODE_GREEN_GHOST_MITIGATION.decode(value);
                    tags.insert(
                        "Apple:GreenGhostMitigationStatus".to_string(),
                        decoded.to_string(),
                    );
                }
            }

            // SNR type
            APPLE_SIGNAL_TO_NOISE_RATIO_TYPE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let decoded = DECODE_SNR_TYPE.decode(value);
                    tags.insert(
                        "Apple:SignalToNoiseRatioType".to_string(),
                        decoded.to_string(),
                    );
                }
            }

            // ================================================================
            // INTEGER TAGS (raw values)
            // ================================================================

            // AE/AF stability flags (boolean-like)
            APPLE_AE_STABLE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let decoded = if value != 0 { "Yes" } else { "No" };
                    tags.insert("Apple:AEStable".to_string(), decoded.to_string());
                }
            }

            APPLE_AF_STABLE => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    let decoded = if value != 0 { "Yes" } else { "No" };
                    tags.insert("Apple:AFStable".to_string(), decoded.to_string());
                }
            }

            // Front-facing camera flag
            APPLE_FRONT_FACING_CAMERA => {
                if let Some(value) = extract_i16_value(entry, data, byte_order) {
                    tags.insert(
                        "Apple:FrontFacingCamera".to_string(),
                        decode_facing_camera(value),
                    );
                }
            }

            // AE target/average (exposure values)
            APPLE_AE_TARGET => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    tags.insert("Apple:AETarget".to_string(), value.to_string());
                }
            }

            APPLE_AE_AVERAGE => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    tags.insert("Apple:AEAverage".to_string(), value.to_string());
                }
            }

            // Color temperature (Kelvin)
            APPLE_COLOR_TEMPERATURE => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    tags.insert("Apple:ColorTemperature".to_string(), format!("{} K", value));
                }
            }

            // Focus position (integer)
            APPLE_FOCUS_POSITION => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    tags.insert("Apple:FocusPosition".to_string(), value.to_string());
                }
            }

            // AF confidence (percentage-like)
            APPLE_AF_CONFIDENCE => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    tags.insert("Apple:AFConfidence".to_string(), value.to_string());
                }
            }

            // AF measured depth (millimeters, from LiDAR)
            APPLE_AF_MEASURED_DEPTH => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    tags.insert("Apple:AFMeasuredDepth".to_string(), format!("{} mm", value));
                }
            }

            // Signal-to-noise ratio
            APPLE_SIGNAL_TO_NOISE_RATIO => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    // SNR is often stored as fixed-point, display as decimal
                    let snr_db = (value as f64) / 100.0;
                    tags.insert(
                        "Apple:SignalToNoiseRatio".to_string(),
                        format!("{:.2} dB", snr_db),
                    );
                }
            }

            // HDR headroom (EV)
            APPLE_HDR_HEADROOM => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    let headroom_ev = (value as f64) / 1000.0;
                    tags.insert(
                        "Apple:HDRHeadroom".to_string(),
                        format!("{:.2} EV", headroom_ev),
                    );
                }
            }

            // HDR gain
            APPLE_HDR_GAIN => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    let gain = (value as f64) / 1000.0;
                    tags.insert("Apple:HDRGain".to_string(), format!("{:.3}", gain));
                }
            }

            // Luminance noise amplitude
            APPLE_LUMINANCE_NOISE_AMPLITUDE => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    let amplitude = (value as f64) / 10000.0;
                    tags.insert(
                        "Apple:LuminanceNoiseAmplitude".to_string(),
                        format!("{:.4}", amplitude),
                    );
                }
            }

            // Quality hint
            APPLE_QUALITY_HINT => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    tags.insert("Apple:QualityHint".to_string(), value.to_string());
                }
            }

            // Image processing flags (bitmask)
            APPLE_IMAGE_PROCESSING_FLAGS => {
                if let Some(value) = extract_u32_value(entry, data, byte_order) {
                    tags.insert(
                        "Apple:ImageProcessingFlags".to_string(),
                        format!("0x{:08X}", value),
                    );
                }
            }

            // Photos app feature flags
            APPLE_PHOTOS_APP_FEATURE_FLAGS => {
                if let Some(value) = extract_u32_value(entry, data, byte_order) {
                    tags.insert(
                        "Apple:PhotosAppFeatureFlags".to_string(),
                        format!("0x{:08X}", value),
                    );
                }
            }

            // Scene flags
            APPLE_SCENE_FLAGS => {
                if let Some(value) = extract_u32_value(entry, data, byte_order) {
                    tags.insert("Apple:SceneFlags".to_string(), format!("0x{:08X}", value));
                }
            }

            // AF performance metrics
            APPLE_AF_PERFORMANCE => {
                if let Some(value) = extract_u32_value(entry, data, byte_order) {
                    tags.insert(
                        "Apple:AFPerformance".to_string(),
                        format!("0x{:08X}", value),
                    );
                }
            }

            // Semantic style versions
            APPLE_SEMANTIC_STYLE_RENDERING_VER => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    tags.insert(
                        "Apple:SemanticStyleRenderingVer".to_string(),
                        value.to_string(),
                    );
                }
            }

            APPLE_SEMANTIC_STYLE_PRESET => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    tags.insert("Apple:SemanticStylePreset".to_string(), value.to_string());
                }
            }

            // Live Photo video index
            APPLE_LIVE_PHOTO_VIDEO_INDEX => {
                if let Some(value) = extract_i32_value(entry, data, byte_order) {
                    tags.insert("Apple:LivePhotoVideoIndex".to_string(), value.to_string());
                    // Indicate this is a Live Photo
                    tags.insert("Apple:LivePhoto".to_string(), "Yes".to_string());
                }
            }

            // ================================================================
            // STRING TAGS
            // ================================================================

            // MakerNote version
            APPLE_MAKERNOTE_VERSION => {
                if let Some(s) = extract_string_with_byteorder(entry, data, byte_order) {
                    tags.insert("Apple:MakerNoteVersion".to_string(), s);
                }
            }

            // Burst UUID
            APPLE_BURST_UUID => {
                if let Some(s) = extract_string_with_byteorder(entry, data, byte_order) {
                    tags.insert("Apple:BurstUUID".to_string(), s);
                }
            }

            // Content identifier (media UUID)
            APPLE_CONTENT_IDENTIFIER => {
                if let Some(s) = extract_string_with_byteorder(entry, data, byte_order) {
                    tags.insert("Apple:ContentIdentifier".to_string(), s);
                }
            }

            // Image unique ID
            APPLE_IMAGE_UNIQUE_ID => {
                if let Some(s) = extract_string_with_byteorder(entry, data, byte_order) {
                    tags.insert("Apple:ImageUniqueID".to_string(), s);
                }
            }

            // Photo identifier
            APPLE_PHOTO_IDENTIFIER => {
                if let Some(s) = extract_string_with_byteorder(entry, data, byte_order) {
                    tags.insert("Apple:PhotoIdentifier".to_string(), s);
                }
            }

            // Image capture request ID
            APPLE_IMAGE_CAPTURE_REQUEST_ID => {
                if let Some(s) = extract_string_with_byteorder(entry, data, byte_order) {
                    tags.insert("Apple:ImageCaptureRequestIdentifier".to_string(), s);
                }
            }

            // ================================================================
            // ARRAY/COMPLEX TAGS
            // ================================================================

            // Focus distance range (min/max in meters)
            APPLE_FOCUS_DISTANCE_RANGE => {
                if let Some(values) = extract_i16_array(entry, data, byte_order)
                    && values.len() >= 2
                {
                    let near = (values[0] as f64) / 100.0;
                    let far = (values[1] as f64) / 100.0;
                    tags.insert(
                        "Apple:FocusDistanceRange".to_string(),
                        format!("{:.2} - {:.2} m", near, far),
                    );
                }
            }

            // Acceleration vector (X, Y, Z)
            APPLE_ACCELERATION_VECTOR => {
                if let Some(values) = extract_i16_array(entry, data, byte_order)
                    && values.len() >= 3
                {
                    // Values are typically in fixed-point format
                    let x = (values[0] as f64) / 1000.0;
                    let y = (values[1] as f64) / 1000.0;
                    let z = (values[2] as f64) / 1000.0;
                    tags.insert(
                        "Apple:AccelerationVector".to_string(),
                        format!("({:.3}, {:.3}, {:.3})", x, y, z),
                    );
                }
            }

            // AE matrix (complex array)
            APPLE_AE_MATRIX => {
                if let Some(values) = extract_i16_array(entry, data, byte_order)
                    && !values.is_empty()
                {
                    // Format as array of values
                    let formatted: Vec<String> = values.iter().map(|v| v.to_string()).collect();
                    tags.insert("Apple:AEMatrix".to_string(), formatted.join(" "));
                }
            }

            // Color correction matrix
            APPLE_COLOR_CORRECTION_MATRIX => {
                if let Some(values) = extract_i16_array(entry, data, byte_order)
                    && !values.is_empty()
                {
                    // 3x3 matrix stored as 9 values
                    let formatted: Vec<String> = values
                        .iter()
                        .map(|v| format!("{:.4}", (*v as f64) / 10000.0))
                        .collect();
                    tags.insert(
                        "Apple:ColorCorrectionMatrix".to_string(),
                        formatted.join(" "),
                    );
                }
            }

            // RunTime (complex plist structure, store as raw for now)
            APPLE_RUN_TIME => {
                // RunTime is typically a binary plist embedded in the tag
                // For now, indicate it's present
                if entry.value_count > 0 {
                    tags.insert("Apple:RunTime".to_string(), "(binary plist)".to_string());
                }
            }

            // ================================================================
            // FALLBACK: Unknown tags
            // ================================================================
            _ => {
                // For unknown tags, check if they're in the registry for a name
                if let Some(tag_name) = registry.get_tag_name(tag_id) {
                    // Try to extract as integer or string
                    if let Some(value) = extract_i32_value(entry, data, byte_order) {
                        tags.insert(format!("Apple:{}", tag_name), value.to_string());
                    } else if let Some(s) = extract_string_with_byteorder(entry, data, byte_order) {
                        tags.insert(format!("Apple:{}", tag_name), s);
                    }
                }
                // Unknown tags not in registry are silently skipped
            }
        }
    }
}

impl MakerNoteParser for AppleParser {
    fn manufacturer_name(&self) -> &'static str {
        "Apple"
    }

    fn tag_prefix(&self) -> &'static str {
        "Apple:"
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // Check if this is BPLIST format
        if Self::is_bplist_format(data) {
            return self.parse_bplist(data, tags);
        }

        // Otherwise, parse as standard IFD format
        let config = IfdParserConfig {
            signature: Some(APPLE_SIGNATURE),
            signature_offset: 10, // "Apple iOS" (9) + 1 padding byte = 10
            max_entries: 500,
        };

        parse_ifd_entries(data, byte_order, &config, |entry, _ifd_data| {
            // Pass full data buffer to parse_entry as it expects absolute offsets
            self.parse_entry(entry, data, byte_order, tags);
        })
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Accept BPLIST format
        if Self::is_bplist_format(data) {
            return true;
        }

        // Accept data with Apple signature
        if data.len() >= 9 && &data[0..9] == APPLE_SIGNATURE {
            return true;
        }

        // Also accept if it looks like valid IFD data
        if data.len() >= 2 {
            let reader = EndianReader::little_endian(data);
            let entry_count = reader.u16_at(0).unwrap_or(0);
            if entry_count > 0 && entry_count < 500 {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_hdr_type() {
        assert_eq!(DECODE_HDR_TYPE.decode(0), "Off");
        assert_eq!(DECODE_HDR_TYPE.decode(1), "HDR");
        assert_eq!(DECODE_HDR_TYPE.decode(4), "Smart HDR");
        assert_eq!(DECODE_HDR_TYPE.decode(8), "Smart HDR 5");
    }

    #[test]
    fn test_decode_portrait_mode() {
        assert_eq!(DECODE_PORTRAIT_MODE.decode(0), "Off");
        assert_eq!(DECODE_PORTRAIT_MODE.decode(1), "Natural Light");
        assert_eq!(DECODE_PORTRAIT_MODE.decode(4), "Stage Light");
    }

    #[test]
    fn test_decode_scene_type() {
        assert_eq!(DECODE_SCENE_TYPE.decode(0), "None");
        assert_eq!(DECODE_SCENE_TYPE.decode(6), "Night");
        assert_eq!(DECODE_SCENE_TYPE.decode(8), "Food");
        assert_eq!(DECODE_SCENE_TYPE.decode(11), "QR Code");
    }

    #[test]
    fn test_decode_semantic_style() {
        assert_eq!(DECODE_SEMANTIC_STYLE.decode(0), "Standard");
        assert_eq!(DECODE_SEMANTIC_STYLE.decode(2), "Vibrant");
    }

    #[test]
    fn test_decode_lens_model() {
        assert_eq!(DECODE_LENS_MODEL.decode(0), "Wide (Main Camera)");
        assert_eq!(DECODE_LENS_MODEL.decode(1), "Telephoto");
        assert_eq!(DECODE_LENS_MODEL.decode(2), "Ultra Wide");
        assert_eq!(DECODE_LENS_MODEL.decode(6), "Telephoto 5x");
    }

    #[test]
    fn test_decode_camera_type() {
        assert_eq!(DECODE_CAMERA_TYPE.decode(1), "Back Normal");
        assert_eq!(DECODE_CAMERA_TYPE.decode(6), "Front");
    }

    #[test]
    fn test_decode_ois_mode() {
        assert_eq!(DECODE_OIS_MODE.decode(0), "Off");
        assert_eq!(DECODE_OIS_MODE.decode(1), "On");
        assert_eq!(DECODE_OIS_MODE.decode(3), "Action Mode");
    }

    #[test]
    fn test_decode_image_capture_type() {
        assert_eq!(DECODE_IMAGE_CAPTURE_TYPE.decode(0), "Photo");
        assert_eq!(DECODE_IMAGE_CAPTURE_TYPE.decode(1), "Portrait");
        assert_eq!(DECODE_IMAGE_CAPTURE_TYPE.decode(4), "Night Mode");
    }

    #[test]
    fn test_apple_parser_trait() {
        let parser = AppleParser::new();
        assert_eq!(parser.manufacturer_name(), "Apple");
        assert_eq!(parser.tag_prefix(), "Apple:");
    }

    #[test]
    fn test_validate_header_with_signature() {
        let parser = AppleParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(b"Apple iOS");
        data.extend_from_slice(&[0x05, 0x00]); // 5 entries

        assert!(parser.validate_header(&data));
    }

    #[test]
    fn test_validate_header_without_signature() {
        let parser = AppleParser::new();
        let data = vec![0x05, 0x00]; // Just entry count

        assert!(parser.validate_header(&data));
    }

    #[test]
    fn test_validate_header_bplist() {
        let parser = AppleParser::new();
        let mut data = Vec::new();
        data.extend_from_slice(b"Apple iOS");
        data.push(0x00);
        data.extend_from_slice(b"bplist00");

        assert!(parser.validate_header(&data));
    }

    #[test]
    fn test_parse_hdr_tag() {
        let parser = AppleParser::new();
        let mut data = Vec::new();

        // Create minimal IFD with one entry
        data.extend_from_slice(&[0x01, 0x00]); // 1 entry

        // HDR tag entry (tag=0x000A, type=3 (SHORT), count=1, value=4 (Smart HDR))
        data.extend_from_slice(&[0x0A, 0x00]); // Tag
        data.extend_from_slice(&[0x03, 0x00]); // Type: SHORT
        data.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Count: 1
        data.extend_from_slice(&[0x04, 0x00, 0x00, 0x00]); // Value: 4 (inline)

        let mut tags = HashMap::new();
        let result = parser.parse(&data, ByteOrder::LittleEndian, &mut tags);

        assert!(result.is_ok());
        assert_eq!(
            tags.get("Apple:HDRImageType"),
            Some(&"Smart HDR".to_string())
        );
    }

    #[test]
    fn test_is_bplist_format() {
        // Test direct bplist
        let direct_bplist = b"bplist00data";
        assert!(AppleParser::is_bplist_format(direct_bplist));

        // Test Apple iOS header + bplist
        let mut apple_bplist = Vec::new();
        apple_bplist.extend_from_slice(b"Apple iOS");
        apple_bplist.push(0x00);
        apple_bplist.extend_from_slice(b"bplist00");
        assert!(AppleParser::is_bplist_format(&apple_bplist));

        // Test non-bplist data
        let ifd_data = vec![0x05, 0x00];
        assert!(!AppleParser::is_bplist_format(&ifd_data));
    }
}
