//! Generic decoders for common MakerNote value patterns
//!
//! This module provides reusable decoder functions and types that eliminate the need
//! for repetitive decoder implementations across different manufacturer parsers.
//!
//! ## Design Philosophy
//! Instead of writing individual decoder functions for every tag, this module provides:
//! 1. **Pre-built common decoders** for frequently-used patterns (On/Off, Yes/No, etc.)
//! 2. **Generic decoder types** that can be configured with mappings
//! 3. **Formatter helpers** for consistent unknown value handling
//!
//! ## Before & After Example
//!
//! **Before** (in samsung.rs):
//! ```ignore
//! fn decode_scene_optimizer(value: i16) -> String {
//!     match value {
//!         0 => "Off".to_string(),
//!         1 => "On".to_string(),
//!         2 => "Auto".to_string(),
//!         _ => format!("Unknown ({})", value),
//!     }
//! }
//! ```
//!
//! **After** (in samsung.rs using this module):
//! ```ignore
//! use super::shared::generic_decoders::SimpleValueDecoder;
//!
//! const SCENE_OPTIMIZER: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
//!     (0, "Off"),
//!     (1, "On"),
//!     (2, "Auto"),
//! ]);
//!
//! // Usage:
//! let result = SCENE_OPTIMIZER.decode(value);
//! ```
//!
//! ## Usage Statistics Impact
//! Using these generic decoders can reduce code duplication from ~1300% to <50%
//! by eliminating hundreds of nearly-identical decoder functions.

use std::collections::HashMap;
use std::fmt::Display;
use std::hash::Hash;

/// A simple value-to-string decoder that maps input values to output strings
///
/// This is the most common decoder type, used for simple enum-like mappings.
/// It uses a static slice for zero-cost abstraction and compile-time validation.
///
/// # Type Parameters
/// * `T` - The input value type (typically i16, u16, i32, u32)
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::SimpleValueDecoder;
///
/// const QUALITY: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
///     (1, "Low"),
///     (2, "Medium"),
///     (3, "High"),
/// ]);
///
/// assert_eq!(QUALITY.decode(2), "Medium");
/// assert_eq!(QUALITY.decode(99), "Unknown (99)");
/// ```
#[derive(Debug, Clone)]
pub struct SimpleValueDecoder<T: 'static> {
    /// Static mapping of values to strings
    /// Using a slice instead of HashMap for const compatibility
    mappings: &'static [(T, &'static str)],
}

impl<T> SimpleValueDecoder<T>
where
    T: PartialEq + Display + Copy,
{
    /// Creates a new decoder with the given value mappings
    ///
    /// # Arguments
    /// * `mappings` - Static slice of (value, string) tuples
    ///
    /// # Returns
    /// A new SimpleValueDecoder instance
    pub const fn new(mappings: &'static [(T, &'static str)]) -> Self {
        Self { mappings }
    }

    /// Decodes a value to its string representation
    ///
    /// # Arguments
    /// * `value` - The value to decode
    ///
    /// # Returns
    /// The mapped string, or "Unknown (value)" if not found
    pub fn decode(&self, value: T) -> String {
        for (mapped_val, mapped_str) in self.mappings {
            if *mapped_val == value {
                return mapped_str.to_string();
            }
        }
        format!("Unknown ({})", value)
    }

    /// Decodes a value with a custom unknown formatter
    ///
    /// # Arguments
    /// * `value` - The value to decode
    /// * `unknown_formatter` - Function to format unknown values
    ///
    /// # Returns
    /// The mapped string, or the result of unknown_formatter
    pub fn decode_with<F>(&self, value: T, unknown_formatter: F) -> String
    where
        F: FnOnce(T) -> String,
    {
        for (mapped_val, mapped_str) in self.mappings {
            if *mapped_val == value {
                return mapped_str.to_string();
            }
        }
        unknown_formatter(value)
    }
}

/// A runtime-configurable value decoder using HashMap
///
/// Use this when you need dynamic mapping construction or need to share
/// mappings across multiple decoders. For static mappings, prefer SimpleValueDecoder.
///
/// # Type Parameters
/// * `T` - The input value type
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::DynamicValueDecoder;
/// use std::collections::HashMap;
///
/// let mut mappings = HashMap::new();
/// mappings.insert(0, "Off");
/// mappings.insert(1, "On");
///
/// let decoder = DynamicValueDecoder::new(mappings);
/// assert_eq!(decoder.decode(&1), "On");
/// ```
#[derive(Debug, Clone)]
pub struct DynamicValueDecoder<T> {
    /// Runtime mapping of values to strings
    mappings: HashMap<T, &'static str>,
}

impl<T> DynamicValueDecoder<T>
where
    T: Eq + Hash + Display,
{
    /// Creates a new dynamic decoder with the given mappings
    pub fn new(mappings: HashMap<T, &'static str>) -> Self {
        Self { mappings }
    }

    /// Decodes a value to its string representation
    pub fn decode(&self, value: &T) -> String {
        self.mappings
            .get(value)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("Unknown ({})", value))
    }
}

/// A bitfield decoder for values that represent multiple flags
///
/// Many MakerNote tags use bitfields where each bit represents a different
/// feature or setting. This decoder handles such cases.
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::BitfieldDecoder;
///
/// const FEATURES: BitfieldDecoder = BitfieldDecoder::new(&[
///     (0x01, "HDR"),
///     (0x02, "Panorama"),
///     (0x04, "Night Mode"),
///     (0x08, "Portrait"),
/// ]);
///
/// assert_eq!(FEATURES.decode(0x05), "HDR, Night Mode");
/// assert_eq!(FEATURES.decode(0x00), "None");
/// ```
#[derive(Debug, Clone)]
pub struct BitfieldDecoder {
    /// Mapping of bit positions to feature names
    bit_mappings: &'static [(u32, &'static str)],
}

impl BitfieldDecoder {
    /// Creates a new bitfield decoder
    ///
    /// # Arguments
    /// * `bit_mappings` - Slice of (bit_mask, name) tuples
    pub const fn new(bit_mappings: &'static [(u32, &'static str)]) -> Self {
        Self { bit_mappings }
    }

    /// Decodes a bitfield value to a comma-separated list of set flags
    ///
    /// # Arguments
    /// * `value` - The bitfield value to decode
    ///
    /// # Returns
    /// Comma-separated list of set flags, or "None" if no flags are set
    pub fn decode(&self, value: u32) -> String {
        let mut parts = Vec::new();

        for (mask, name) in self.bit_mappings {
            if value & mask != 0 {
                parts.push(*name);
            }
        }

        if parts.is_empty() {
            "None".to_string()
        } else {
            parts.join(", ")
        }
    }
}

/// A range-based decoder that maps value ranges to strings
///
/// Useful for categorizing continuous values into discrete buckets.
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::RangeDecoder;
///
/// const BRIGHTNESS: RangeDecoder<i16> = RangeDecoder::new(&[
///     (0..30, "Dark"),
///     (30..70, "Normal"),
///     (70..100, "Bright"),
/// ], "Very Bright");
///
/// assert_eq!(BRIGHTNESS.decode(25), "Dark");
/// assert_eq!(BRIGHTNESS.decode(150), "Very Bright");
/// ```
#[derive(Debug, Clone)]
pub struct RangeDecoder<T: 'static> {
    /// Ranges mapped to their string representations
    /// Ranges are checked in order, first match wins
    ranges: &'static [(std::ops::Range<T>, &'static str)],
    /// Fallback string for values outside all ranges
    fallback: &'static str,
}

impl<T> RangeDecoder<T>
where
    T: PartialOrd + Display + Copy,
{
    /// Creates a new range decoder
    ///
    /// # Arguments
    /// * `ranges` - Slice of (range, string) tuples
    /// * `fallback` - String to use for values outside all ranges
    pub const fn new(
        ranges: &'static [(std::ops::Range<T>, &'static str)],
        fallback: &'static str,
    ) -> Self {
        Self { ranges, fallback }
    }

    /// Decodes a value based on which range it falls into
    pub fn decode(&self, value: T) -> String {
        for (range, name) in self.ranges {
            if range.contains(&value) {
                return name.to_string();
            }
        }
        self.fallback.to_string()
    }
}

// ============================================================================
// Pre-built Common Decoders
// ============================================================================

/// Pre-built decoder for binary On/Off values (0=Off, 1=On)
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::ON_OFF;
///
/// assert_eq!(ON_OFF.decode(0), "Off");
/// assert_eq!(ON_OFF.decode(1), "On");
/// ```
pub const ON_OFF: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[(0, "Off"), (1, "On")]);

/// Pre-built decoder for binary On/Off values (0=Off, 1=On) - i32 version
///
/// Used by parsers that process i32 arrays (e.g., Olympus CameraSettings)
pub const ON_OFF_I32: SimpleValueDecoder<i32> = SimpleValueDecoder::new(&[(0, "Off"), (1, "On")]);

/// Pre-built decoder for binary Yes/No values (0=No, 1=Yes)
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::YES_NO;
///
/// assert_eq!(YES_NO.decode(0), "No");
/// assert_eq!(YES_NO.decode(1), "Yes");
/// ```
pub const YES_NO: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[(0, "No"), (1, "Yes")]);

/// Pre-built decoder for Enabled/Disabled values (0=Disabled, 1=Enabled)
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::ENABLED_DISABLED;
///
/// assert_eq!(ENABLED_DISABLED.decode(0), "Disabled");
/// assert_eq!(ENABLED_DISABLED.decode(1), "Enabled");
/// ```
pub const ENABLED_DISABLED: SimpleValueDecoder<i16> =
    SimpleValueDecoder::new(&[(0, "Disabled"), (1, "Enabled")]);

/// Pre-built decoder for Auto/Manual modes (0=Auto, 1=Manual)
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::AUTO_MANUAL;
///
/// assert_eq!(AUTO_MANUAL.decode(0), "Auto");
/// assert_eq!(AUTO_MANUAL.decode(1), "Manual");
/// ```
pub const AUTO_MANUAL: SimpleValueDecoder<i16> =
    SimpleValueDecoder::new(&[(0, "Auto"), (1, "Manual")]);

/// Pre-built decoder for common quality levels (Low/Medium/High)
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::QUALITY_LMH;
///
/// assert_eq!(QUALITY_LMH.decode(1), "Low");
/// assert_eq!(QUALITY_LMH.decode(2), "Medium");
/// assert_eq!(QUALITY_LMH.decode(3), "High");
/// ```
pub const QUALITY_LMH: SimpleValueDecoder<i16> =
    SimpleValueDecoder::new(&[(1, "Low"), (2, "Medium"), (3, "High")]);

/// Pre-built decoder for Normal/Fine/Extra Fine quality
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::QUALITY_NORMAL_FINE;
///
/// assert_eq!(QUALITY_NORMAL_FINE.decode(0), "Normal");
/// assert_eq!(QUALITY_NORMAL_FINE.decode(1), "Fine");
/// assert_eq!(QUALITY_NORMAL_FINE.decode(2), "Extra Fine");
/// ```
pub const QUALITY_NORMAL_FINE: SimpleValueDecoder<i16> =
    SimpleValueDecoder::new(&[(0, "Normal"), (1, "Fine"), (2, "Extra Fine")]);

// ============================================================================
// Formatter Helpers
// ============================================================================

/// Formats an unknown value with standard "Unknown (value)" pattern
///
/// # Arguments
/// * `value` - The unknown value to format
///
/// # Returns
/// Formatted string "Unknown (value)"
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::format_unknown;
///
/// assert_eq!(format_unknown(42), "Unknown (42)");
/// ```
pub fn format_unknown<T: Display>(value: T) -> String {
    format!("Unknown ({})", value)
}

/// Formats an unknown value with a custom prefix
///
/// # Arguments
/// * `prefix` - The prefix to use (e.g., "Unknown Mode")
/// * `value` - The unknown value to format
///
/// # Returns
/// Formatted string "prefix (value)"
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::format_unknown_with_prefix;
///
/// assert_eq!(
///     format_unknown_with_prefix("Unknown Mode", 5),
///     "Unknown Mode (5)"
/// );
/// ```
pub fn format_unknown_with_prefix<T: Display>(prefix: &str, value: T) -> String {
    format!("{} ({})", prefix, value)
}

/// Formats a value as hexadecimal
///
/// # Arguments
/// * `value` - The value to format
///
/// # Returns
/// Formatted string "0xHH"
///
/// # Example
/// ```rust
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::format_hex;
///
/// assert_eq!(format_hex(255u8), "0xFF");
/// ```
pub fn format_hex<T: std::fmt::UpperHex>(value: T) -> String {
    format!("0x{:X}", value)
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_value_decoder() {
        const TEST_DECODER: SimpleValueDecoder<i16> =
            SimpleValueDecoder::new(&[(0, "Zero"), (1, "One"), (2, "Two")]);

        assert_eq!(TEST_DECODER.decode(0), "Zero");
        assert_eq!(TEST_DECODER.decode(1), "One");
        assert_eq!(TEST_DECODER.decode(2), "Two");
        assert_eq!(TEST_DECODER.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_simple_value_decoder_with_custom_formatter() {
        const TEST_DECODER: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[(0, "Known")]);

        let result = TEST_DECODER.decode_with(99, |v| format!("Invalid: {}", v));
        assert_eq!(result, "Invalid: 99");
    }

    #[test]
    fn test_dynamic_value_decoder() {
        let mut mappings = HashMap::new();
        mappings.insert(0, "Off");
        mappings.insert(1, "On");

        let decoder = DynamicValueDecoder::new(mappings);
        assert_eq!(decoder.decode(&0), "Off");
        assert_eq!(decoder.decode(&1), "On");
        assert_eq!(decoder.decode(&2), "Unknown (2)");
    }

    #[test]
    fn test_bitfield_decoder() {
        const FEATURES: BitfieldDecoder = BitfieldDecoder::new(&[
            (0x01, "HDR"),
            (0x02, "Panorama"),
            (0x04, "Night Mode"),
            (0x08, "Portrait"),
        ]);

        assert_eq!(FEATURES.decode(0x00), "None");
        assert_eq!(FEATURES.decode(0x01), "HDR");
        assert_eq!(FEATURES.decode(0x03), "HDR, Panorama");
        assert_eq!(FEATURES.decode(0x05), "HDR, Night Mode");
        assert_eq!(FEATURES.decode(0x0F), "HDR, Panorama, Night Mode, Portrait");
    }

    #[test]
    fn test_range_decoder() {
        const BRIGHTNESS: RangeDecoder<i16> = RangeDecoder::new(
            &[(0..30, "Dark"), (30..70, "Normal"), (70..100, "Bright")],
            "Very Bright",
        );

        assert_eq!(BRIGHTNESS.decode(0), "Dark");
        assert_eq!(BRIGHTNESS.decode(25), "Dark");
        assert_eq!(BRIGHTNESS.decode(30), "Normal");
        assert_eq!(BRIGHTNESS.decode(50), "Normal");
        assert_eq!(BRIGHTNESS.decode(70), "Bright");
        assert_eq!(BRIGHTNESS.decode(99), "Bright");
        assert_eq!(BRIGHTNESS.decode(100), "Very Bright");
        assert_eq!(BRIGHTNESS.decode(200), "Very Bright");
    }

    #[test]
    fn test_prebuilt_on_off() {
        assert_eq!(ON_OFF.decode(0), "Off");
        assert_eq!(ON_OFF.decode(1), "On");
        assert_eq!(ON_OFF.decode(2), "Unknown (2)");
    }

    #[test]
    fn test_prebuilt_yes_no() {
        assert_eq!(YES_NO.decode(0), "No");
        assert_eq!(YES_NO.decode(1), "Yes");
    }

    #[test]
    fn test_prebuilt_enabled_disabled() {
        assert_eq!(ENABLED_DISABLED.decode(0), "Disabled");
        assert_eq!(ENABLED_DISABLED.decode(1), "Enabled");
    }

    #[test]
    fn test_prebuilt_auto_manual() {
        assert_eq!(AUTO_MANUAL.decode(0), "Auto");
        assert_eq!(AUTO_MANUAL.decode(1), "Manual");
    }

    #[test]
    fn test_prebuilt_quality_lmh() {
        assert_eq!(QUALITY_LMH.decode(1), "Low");
        assert_eq!(QUALITY_LMH.decode(2), "Medium");
        assert_eq!(QUALITY_LMH.decode(3), "High");
    }

    #[test]
    fn test_prebuilt_quality_normal_fine() {
        assert_eq!(QUALITY_NORMAL_FINE.decode(0), "Normal");
        assert_eq!(QUALITY_NORMAL_FINE.decode(1), "Fine");
        assert_eq!(QUALITY_NORMAL_FINE.decode(2), "Extra Fine");
    }

    #[test]
    fn test_format_unknown() {
        assert_eq!(format_unknown(42), "Unknown (42)");
        assert_eq!(format_unknown(-10), "Unknown (-10)");
    }

    #[test]
    fn test_format_unknown_with_prefix() {
        assert_eq!(
            format_unknown_with_prefix("Invalid Mode", 7),
            "Invalid Mode (7)"
        );
    }

    #[test]
    fn test_format_hex() {
        assert_eq!(format_hex(0u8), "0x0");
        assert_eq!(format_hex(255u8), "0xFF");
        assert_eq!(format_hex(0x1234u16), "0x1234");
    }
}
