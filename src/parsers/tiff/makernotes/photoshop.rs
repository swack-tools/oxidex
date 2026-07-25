//! Adobe Photoshop MakerNote parser
//!
//! Parses Photoshop-specific editing metadata stored in MakerNotes.
//! Contains layer information, adjustment details, filters applied,
//! edit history, and various processing parameters.
//!
//! ## Supported Versions
//! - Photoshop CC 2015-2024
//! - Photoshop CS6 and earlier
//! - Photoshop Elements
//! - Photoshop Lightroom (when edited with Photoshop)
//!
//! ## Key Features
//! - Layer count and structure information
//! - Adjustment layers (Curves, Levels, Hue/Saturation)
//! - Filters applied (Gaussian Blur, Sharpen, etc.)
//! - Edit history and action count
//! - Smart Object information
//! - Color mode and bit depth
//! - Document resolution settings
//! - Blending modes used
//! - Layer effects (shadows, glows, bevels)
//! - Text layer information
//! - Shape layer data
//! - Mask information
//! - Alpha channel count
//!
//! ## Architecture
//! Photoshop stores extensive editing metadata in proprietary formats.
//! This parser uses the TagRegistry pattern to eliminate code duplication,
//! with all tag definitions centralized in a static registry for O(1) lookup
//! and automatic value decoding.

#![allow(dead_code)]
#![allow(unused_imports)]

use crate::parsers::tiff::ifd_parser::{ByteOrder, IfdEntry};
use std::collections::HashMap;

use super::shared::MakerNoteParser;
use super::shared::array_extractors::{extract_i16_array, extract_string};
use super::shared::generic_decoders::{BitfieldDecoder, SimpleValueDecoder, YES_NO};
use super::shared::ifd_parser_base::{IfdParserConfig, parse_ifd_entries};
use super::shared::tag_registry::TagRegistry;

// Import macros for declarative decoder definitions
use crate::{bitfield_decoder, const_decoder};

// Import once_cell for static lazy initialization of the tag registry
use once_cell::sync::Lazy;

// ============================================================================
// Tag ID Constants
// ============================================================================

const PS_VERSION: u16 = 0x0001; // Photoshop version
const PS_LAYER_COUNT: u16 = 0x0010; // Number of layers
const PS_LAYER_NAMES: u16 = 0x0011; // Layer names (comma-separated)
const PS_ADJUSTMENT_COUNT: u16 = 0x0012; // Number of adjustment layers
const PS_ADJUSTMENT_TYPES: u16 = 0x0013; // Adjustment layer types
const PS_FILTER_COUNT: u16 = 0x0014; // Number of filters applied
const PS_FILTER_NAMES: u16 = 0x0015; // Filter names
const PS_EDIT_COUNT: u16 = 0x0016; // Number of edits in history
const PS_ACTION_COUNT: u16 = 0x0017; // Number of actions executed
const PS_SMART_OBJECT_COUNT: u16 = 0x0018; // Number of smart objects
const PS_COLOR_MODE: u16 = 0x0020; // Color mode (RGB, CMYK, etc.)
const PS_BIT_DEPTH: u16 = 0x0021; // Bit depth (8, 16, 32)
const PS_DPI_HORIZONTAL: u16 = 0x0022; // Horizontal DPI
const PS_DPI_VERTICAL: u16 = 0x0023; // Vertical DPI
const PS_WIDTH_PIXELS: u16 = 0x0024; // Document width in pixels
const PS_HEIGHT_PIXELS: u16 = 0x0025; // Document height in pixels
const PS_BLENDING_MODES: u16 = 0x0030; // Blending modes used (bitmask)
const PS_LAYER_EFFECTS: u16 = 0x0031; // Layer effects used (bitmask)
const PS_TEXT_LAYER_COUNT: u16 = 0x0032; // Number of text layers
const PS_SHAPE_LAYER_COUNT: u16 = 0x0033; // Number of shape layers
const PS_ADJUSTMENT_LAYER_COUNT: u16 = 0x0034; // Number of adjustment layers
const PS_FILL_LAYER_COUNT: u16 = 0x0035; // Number of fill layers
const PS_MASK_COUNT: u16 = 0x0040; // Number of layer masks
const PS_VECTOR_MASK_COUNT: u16 = 0x0041; // Number of vector masks
const PS_CLIPPING_MASK_COUNT: u16 = 0x0042; // Number of clipping masks
const PS_ALPHA_CHANNEL_COUNT: u16 = 0x0043; // Number of alpha channels
const PS_SPOT_CHANNEL_COUNT: u16 = 0x0044; // Number of spot channels
const PS_HAS_CURVES: u16 = 0x0050; // Curves adjustment present
const PS_HAS_LEVELS: u16 = 0x0051; // Levels adjustment present
const PS_HAS_HUE_SAT: u16 = 0x0052; // Hue/Saturation adjustment present
const PS_HAS_COLOR_BALANCE: u16 = 0x0053; // Color Balance adjustment present
const PS_HAS_BRIGHTNESS_CONTRAST: u16 = 0x0054; // Brightness/Contrast present
const PS_HAS_VIBRANCE: u16 = 0x0055; // Vibrance adjustment present
const PS_HAS_EXPOSURE: u16 = 0x0056; // Exposure adjustment present
const PS_HAS_SHADOWS_HIGHLIGHTS: u16 = 0x0057; // Shadows/Highlights present
const PS_GAUSSIAN_BLUR_COUNT: u16 = 0x0060; // Gaussian blur filter applied count
const PS_SHARPEN_COUNT: u16 = 0x0061; // Sharpen filter applied count
const PS_SMART_SHARPEN_COUNT: u16 = 0x0062; // Smart Sharpen applied count
const PS_UNSHARP_MASK_COUNT: u16 = 0x0063; // Unsharp Mask applied count
const PS_NOISE_REDUCTION_COUNT: u16 = 0x0064; // Noise reduction applied count
const PS_LIQUIFY_COUNT: u16 = 0x0065; // Liquify filter applied count
const PS_CAMERA_RAW_COUNT: u16 = 0x0066; // Camera Raw filter applied count
const PS_NEURAL_FILTER_COUNT: u16 = 0x0067; // Neural filters applied count
const PS_LAST_SAVE_TIME: u16 = 0x0070; // Last save timestamp
const PS_CREATION_TIME: u16 = 0x0071; // Document creation timestamp
const PS_TOTAL_EDIT_TIME: u16 = 0x0072; // Total editing time (minutes)
const PS_MODIFIED_FLAG: u16 = 0x0073; // Document modified flag
const PS_BACKUP_COUNT: u16 = 0x0074; // Number of backups created
const PS_LAYER_COMP_COUNT: u16 = 0x0080; // Number of layer comps
const PS_ACTIVE_LAYER_COMP: u16 = 0x0081; // Active layer comp name
const PS_GUIDE_COUNT: u16 = 0x0082; // Number of guides
const PS_GRID_ENABLED: u16 = 0x0083; // Grid visibility
const PS_RULER_UNITS: u16 = 0x0084; // Ruler units (pixels, inches, cm)
const PS_COLOR_PROFILE: u16 = 0x0090; // Embedded color profile name
const PS_PROOF_SETUP: u16 = 0x0091; // Proof setup name
const PS_WORKING_COLOR_SPACE: u16 = 0x0092; // Working color space

// Photoshop signature
const PHOTOSHOP_SIGNATURE: &[u8] = b"Adobe Photoshop";

// ============================================================================
// Shared Decoders - Using const_decoder! macro for compile-time definitions
// ============================================================================

// Decoder for Photoshop color modes
// Maps numeric color mode codes to human-readable strings.
// Used by PS_COLOR_MODE tag.
const_decoder!(
    COLOR_MODE,
    i16,
    [
        (0, "Bitmap"),
        (1, "Grayscale"),
        (2, "Indexed"),
        (3, "RGB"),
        (4, "CMYK"),
        (5, "Multichannel"),
        (6, "Duotone"),
        (7, "Lab"),
    ]
);

// Decoder for bit depth values
// Maps bit depth codes to formatted strings (e.g., "8-bit").
// Used by PS_BIT_DEPTH tag.
const_decoder!(
    BIT_DEPTH,
    i16,
    [(1, "1-bit"), (8, "8-bit"), (16, "16-bit"), (32, "32-bit"),]
);

// Decoder for ruler unit settings
// Maps ruler unit codes to measurement system names.
// Used by PS_RULER_UNITS tag.
const_decoder!(
    RULER_UNITS,
    i16,
    [
        (1, "Inches"),
        (2, "Centimeters"),
        (3, "Points"),
        (4, "Picas"),
        (5, "Pixels"),
    ]
);

// Decoder for blending modes bitmask
// Converts a bitmask into a comma-separated list of active blending modes.
// Each bit represents a different blending mode supported by Photoshop.
// Used by PS_BLENDING_MODES tag.
bitfield_decoder!(
    BLENDING_MODES,
    [
        (0x01, "Normal"),
        (0x02, "Multiply"),
        (0x04, "Screen"),
        (0x08, "Overlay"),
        (0x10, "Soft Light"),
        (0x20, "Hard Light"),
        (0x40, "Color Dodge"),
        (0x80, "Color Burn"),
        (0x100, "Darken"),
        (0x200, "Lighten"),
    ]
);

// Decoder for layer effects bitmask
// Converts a bitmask into a comma-separated list of active layer effects.
// Each bit represents a different layer effect (shadow, glow, bevel, etc.).
// Used by PS_LAYER_EFFECTS tag.
bitfield_decoder!(
    LAYER_EFFECTS,
    [
        (0x01, "Drop Shadow"),
        (0x02, "Inner Shadow"),
        (0x04, "Outer Glow"),
        (0x08, "Inner Glow"),
        (0x10, "Bevel and Emboss"),
        (0x20, "Satin"),
        (0x40, "Color Overlay"),
        (0x80, "Gradient Overlay"),
        (0x100, "Pattern Overlay"),
        (0x200, "Stroke"),
    ]
);

// ============================================================================
// Custom Formatter Functions
// ============================================================================

/// Formats resolution in DPI
///
/// # Arguments
/// * `value` - DPI value
///
/// # Returns
/// Formatted DPI string (e.g., "72 dpi") or "Unknown" for invalid values
fn format_dpi(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    format!("{} dpi", value)
}

/// Formats time duration in minutes to human-readable format
///
/// Converts minutes to hours and minutes when appropriate.
///
/// # Arguments
/// * `minutes` - Duration in minutes
///
/// # Returns
/// Formatted duration string (e.g., "1 hr 30 min", "45 min")
fn format_time_duration(minutes: i16) -> String {
    if minutes < 0 {
        return "Unknown".to_string();
    }
    if minutes < 60 {
        format!("{} min", minutes)
    } else {
        let hours = minutes / 60;
        let mins = minutes % 60;
        if mins == 0 {
            format!("{} hr", hours)
        } else {
            format!("{} hr {} min", hours, mins)
        }
    }
}

/// Formats timestamp value
///
/// Converts numeric timestamp to a formatted string.
/// Note: This is a simplified implementation; production code should
/// convert to proper datetime format using chrono or similar.
///
/// # Arguments
/// * `value` - Unix timestamp or proprietary timestamp value
///
/// # Returns
/// Formatted timestamp string or "Unknown" for invalid values
fn format_timestamp(value: i16) -> String {
    if value <= 0 {
        return "Unknown".to_string();
    }
    // For simplicity, return raw value
    // In production, would convert to human-readable format (ISO 8601, etc.)
    format!("Timestamp: {}", value)
}

/// Decodes blending modes bitfield
///
/// Converts i16 to u32 and uses the BLENDING_MODES decoder.
/// This wrapper function adapts the i16 tag value to the u32 bitfield decoder.
///
/// # Arguments
/// * `value` - Bitfield value as i16
///
/// # Returns
/// Comma-separated list of active blending modes
fn decode_blending_modes(value: i16) -> String {
    BLENDING_MODES.decode(value as u32)
}

/// Decodes layer effects bitfield
///
/// Converts i16 to u32 and uses the LAYER_EFFECTS decoder.
/// This wrapper function adapts the i16 tag value to the u32 bitfield decoder.
///
/// # Arguments
/// * `value` - Bitfield value as i16
///
/// # Returns
/// Comma-separated list of active layer effects
fn decode_layer_effects(value: i16) -> String {
    LAYER_EFFECTS.decode(value as u32)
}

// ============================================================================
// Tag Registry
// ============================================================================

/// Static tag registry for Photoshop MakerNote tags
///
/// This registry centralizes all tag definitions and their decoders,
/// eliminating the need for large match statements and reducing code duplication.
///
/// The registry uses once_cell::sync::Lazy for thread-safe lazy initialization,
/// ensuring the registry is built only once on first access.
///
/// ## Benefits of the Registry Pattern:
/// - **Eliminates duplication**: Single source of truth for tag definitions
/// - **O(1) lookups**: Fast HashMap-based tag name and decoder lookups
/// - **Type safety**: Compile-time validation of decoder types
/// - **Maintainability**: Easy to add/modify tags in one location
/// - **Self-documenting**: Tag definitions serve as documentation
///
/// ## Tag Categories:
/// - String tags: Handled separately (version, names, profiles, etc.)
/// - Raw count tags: Simple numeric values with no decoding
/// - Decoder tags: Use shared decoders (color mode, bit depth, etc.)
/// - Custom formatter tags: DPI, time, timestamps
/// - Boolean tags: Yes/No values using shared YES_NO decoder
/// - Bitfield tags: Blending modes and layer effects
static PHOTOSHOP_TAGS: Lazy<TagRegistry> = Lazy::new(|| {
    TagRegistry::with_capacity(70)
        // Raw count tags - no decoding needed
        .register_raw(PS_LAYER_COUNT, "LayerCount")
        .register_raw(PS_ADJUSTMENT_COUNT, "AdjustmentCount")
        .register_raw(PS_FILTER_COUNT, "FilterCount")
        .register_raw(PS_EDIT_COUNT, "EditCount")
        .register_raw(PS_ACTION_COUNT, "ActionCount")
        .register_raw(PS_SMART_OBJECT_COUNT, "SmartObjectCount")
        .register_raw(PS_TEXT_LAYER_COUNT, "TextLayerCount")
        .register_raw(PS_SHAPE_LAYER_COUNT, "ShapeLayerCount")
        .register_raw(PS_ADJUSTMENT_LAYER_COUNT, "AdjustmentLayerCount")
        .register_raw(PS_FILL_LAYER_COUNT, "FillLayerCount")
        .register_raw(PS_MASK_COUNT, "MaskCount")
        .register_raw(PS_VECTOR_MASK_COUNT, "VectorMaskCount")
        .register_raw(PS_CLIPPING_MASK_COUNT, "ClippingMaskCount")
        .register_raw(PS_ALPHA_CHANNEL_COUNT, "AlphaChannelCount")
        .register_raw(PS_SPOT_CHANNEL_COUNT, "SpotChannelCount")
        .register_raw(PS_GAUSSIAN_BLUR_COUNT, "GaussianBlurCount")
        .register_raw(PS_SHARPEN_COUNT, "SharpenCount")
        .register_raw(PS_SMART_SHARPEN_COUNT, "SmartSharpenCount")
        .register_raw(PS_UNSHARP_MASK_COUNT, "UnsharpMaskCount")
        .register_raw(PS_NOISE_REDUCTION_COUNT, "NoiseReductionCount")
        .register_raw(PS_LIQUIFY_COUNT, "LiquifyCount")
        .register_raw(PS_CAMERA_RAW_COUNT, "CameraRawFilterCount")
        .register_raw(PS_NEURAL_FILTER_COUNT, "NeuralFilterCount")
        .register_raw(PS_BACKUP_COUNT, "BackupCount")
        .register_raw(PS_LAYER_COMP_COUNT, "LayerCompCount")
        .register_raw(PS_GUIDE_COUNT, "GuideCount")
        .register_raw(PS_WIDTH_PIXELS, "WidthPixels")
        .register_raw(PS_HEIGHT_PIXELS, "HeightPixels")
        // Tags using shared decoders
        .register_simple_i16(PS_COLOR_MODE, "ColorMode", &COLOR_MODE)
        .register_simple_i16(PS_BIT_DEPTH, "BitDepth", &BIT_DEPTH)
        .register_simple_i16(PS_RULER_UNITS, "RulerUnits", &RULER_UNITS)
        // Custom formatter tags
        .register_i16(PS_DPI_HORIZONTAL, "HorizontalDPI", format_dpi)
        .register_i16(PS_DPI_VERTICAL, "VerticalDPI", format_dpi)
        .register_i16(PS_LAST_SAVE_TIME, "LastSaveTime", format_timestamp)
        .register_i16(PS_CREATION_TIME, "CreationTime", format_timestamp)
        .register_i16(PS_TOTAL_EDIT_TIME, "TotalEditTime", format_time_duration)
        // Boolean tags using shared YES_NO decoder
        .register_simple_i16(PS_HAS_CURVES, "HasCurves", &YES_NO)
        .register_simple_i16(PS_HAS_LEVELS, "HasLevels", &YES_NO)
        .register_simple_i16(PS_HAS_HUE_SAT, "HasHueSaturation", &YES_NO)
        .register_simple_i16(PS_HAS_COLOR_BALANCE, "HasColorBalance", &YES_NO)
        .register_simple_i16(PS_HAS_BRIGHTNESS_CONTRAST, "HasBrightnessContrast", &YES_NO)
        .register_simple_i16(PS_HAS_VIBRANCE, "HasVibrance", &YES_NO)
        .register_simple_i16(PS_HAS_EXPOSURE, "HasExposure", &YES_NO)
        .register_simple_i16(PS_HAS_SHADOWS_HIGHLIGHTS, "HasShadowsHighlights", &YES_NO)
        .register_simple_i16(PS_MODIFIED_FLAG, "Modified", &YES_NO)
        .register_simple_i16(PS_GRID_ENABLED, "GridEnabled", &YES_NO)
        // Bitfield tags
        .register_i16(PS_BLENDING_MODES, "BlendingModes", decode_blending_modes)
        .register_i16(PS_LAYER_EFFECTS, "LayerEffects", decode_layer_effects)
});

// ============================================================================
// Parser Implementation
// ============================================================================

/// Photoshop MakerNote parser implementing the MakerNoteParser trait
///
/// This parser extracts Photoshop editing metadata from MakerNotes,
/// providing information about layers, adjustments, filters, and document settings.
#[derive(Default)]
pub struct PhotoshopParser;

impl PhotoshopParser {
    /// Creates a new Photoshop parser instance
    ///
    /// # Returns
    /// A new PhotoshopParser ready to parse MakerNote data
    pub fn new() -> Self {
        PhotoshopParser
    }
}

impl MakerNoteParser for PhotoshopParser {
    fn manufacturer_name(&self) -> &'static str {
        "Adobe Photoshop"
    }

    fn tag_prefix(&self) -> &'static str {
        "Photoshop:"
    }

    fn validate_header(&self, data: &[u8]) -> bool {
        // Valid if starts with Photoshop signature or has minimum length for IFD (8 bytes)
        data.starts_with(PHOTOSHOP_SIGNATURE) || data.len() >= 8
    }

    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        // Configure IFD parser with Photoshop-specific settings
        let config = IfdParserConfig {
            signature: Some(PHOTOSHOP_SIGNATURE),
            signature_offset: 15,
            max_entries: 500,
        };

        // Use shared IFD parser to eliminate boilerplate
        parse_ifd_entries(data, byte_order, &config, |entry, parse_data| {
            self.process_tag(entry.tag_id, entry, parse_data, byte_order, tags);
        })?;

        Ok(())
    }
}

impl PhotoshopParser {
    /// Processes a single tag entry and adds it to the tags map
    ///
    /// This method handles both string-based and numeric tags, using the
    /// PHOTOSHOP_TAGS registry for O(1) tag name lookups and automatic decoding.
    ///
    /// # Arguments
    /// * `tag` - Tag ID to process
    /// * `entry` - IFD entry containing tag data
    /// * `data` - Raw MakerNote data buffer
    /// * `byte_order` - Byte order for parsing
    /// * `tags` - Output map to store decoded tag values
    fn process_tag(
        &self,
        tag: u16,
        entry: &IfdEntry,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>,
    ) {
        // Handle string-based tags (not in registry)
        match tag {
            PS_VERSION => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("Photoshop:Version".to_string(), s);
                }
                return;
            }
            PS_LAYER_NAMES => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("Photoshop:LayerNames".to_string(), s);
                }
                return;
            }
            PS_ADJUSTMENT_TYPES => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("Photoshop:AdjustmentTypes".to_string(), s);
                }
                return;
            }
            PS_FILTER_NAMES => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("Photoshop:FiltersApplied".to_string(), s);
                }
                return;
            }
            PS_ACTIVE_LAYER_COMP => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("Photoshop:ActiveLayerComp".to_string(), s);
                }
                return;
            }
            PS_COLOR_PROFILE => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("Photoshop:ColorProfile".to_string(), s);
                }
                return;
            }
            PS_PROOF_SETUP => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("Photoshop:ProofSetup".to_string(), s);
                }
                return;
            }
            PS_WORKING_COLOR_SPACE => {
                if let Some(s) = extract_string(entry, data, byte_order) {
                    tags.insert("Photoshop:WorkingColorSpace".to_string(), s);
                }
                return;
            }
            _ => {}
        }

        // Handle numeric tags using registry for O(1) lookup and automatic decoding
        if let Some(tag_name) = PHOTOSHOP_TAGS.get_tag_name(tag)
            && let Some(array) = extract_i16_array(entry, data, byte_order)
            && let Some(&val) = array.first()
        {
            let decoded = PHOTOSHOP_TAGS.decode_i16(tag, val);
            tags.insert(format!("Photoshop:{}", tag_name), decoded);
        }
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_photoshop_parser_creation() {
        let parser = PhotoshopParser::new();
        assert_eq!(parser.manufacturer_name(), "Adobe Photoshop");
        assert_eq!(parser.tag_prefix(), "Photoshop:");
    }

    #[test]
    fn test_color_mode_decoder() {
        assert_eq!(COLOR_MODE.decode(3), "RGB");
        assert_eq!(COLOR_MODE.decode(4), "CMYK");
        assert_eq!(COLOR_MODE.decode(7), "Lab");
        assert_eq!(COLOR_MODE.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_bit_depth_decoder() {
        assert_eq!(BIT_DEPTH.decode(8), "8-bit");
        assert_eq!(BIT_DEPTH.decode(16), "16-bit");
        assert_eq!(BIT_DEPTH.decode(32), "32-bit");
        assert_eq!(BIT_DEPTH.decode(64), "Unknown (64)");
    }

    #[test]
    fn test_blending_modes_decoder() {
        assert_eq!(BLENDING_MODES.decode(0x01), "Normal");
        assert_eq!(BLENDING_MODES.decode(0x06), "Multiply, Screen");
        assert_eq!(BLENDING_MODES.decode(0x00), "None");
    }

    #[test]
    fn test_layer_effects_decoder() {
        assert_eq!(LAYER_EFFECTS.decode(0x01), "Drop Shadow");
        assert_eq!(LAYER_EFFECTS.decode(0x11), "Drop Shadow, Bevel and Emboss");
        assert_eq!(LAYER_EFFECTS.decode(0x00), "None");
    }

    #[test]
    fn test_ruler_units_decoder() {
        assert_eq!(RULER_UNITS.decode(1), "Inches");
        assert_eq!(RULER_UNITS.decode(5), "Pixels");
        assert_eq!(RULER_UNITS.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_format_dpi() {
        assert_eq!(format_dpi(72), "72 dpi");
        assert_eq!(format_dpi(300), "300 dpi");
        assert_eq!(format_dpi(0), "Unknown");
        assert_eq!(format_dpi(-1), "Unknown");
    }

    #[test]
    fn test_format_time_duration() {
        assert_eq!(format_time_duration(30), "30 min");
        assert_eq!(format_time_duration(90), "1 hr 30 min");
        assert_eq!(format_time_duration(120), "2 hr");
        assert_eq!(format_time_duration(-1), "Unknown");
    }

    #[test]
    fn test_validate_header() {
        let parser = PhotoshopParser::new();
        let valid_header = b"Adobe Photoshop\x00\x01";
        assert!(parser.validate_header(valid_header));

        // Test with minimal valid length
        let minimal_header = b"12345678"; // 8 bytes minimum
        assert!(parser.validate_header(minimal_header));

        // Test with too short data
        let short_header = b"123456";
        assert!(!parser.validate_header(short_header));
    }

    #[test]
    fn test_yes_no_decoder_usage() {
        // Test that the shared YES_NO decoder works correctly
        assert_eq!(YES_NO.decode(0), "No");
        assert_eq!(YES_NO.decode(1), "Yes");
        assert_eq!(YES_NO.decode(2), "Unknown (2)");
    }

    #[test]
    fn test_format_timestamp() {
        assert_eq!(format_timestamp(12345), "Timestamp: 12345");
        assert_eq!(format_timestamp(0), "Unknown");
        assert_eq!(format_timestamp(-1), "Unknown");
    }
}
