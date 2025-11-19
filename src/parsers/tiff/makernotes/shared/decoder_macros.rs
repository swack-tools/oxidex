//! Declarative macros for creating decoders with minimal boilerplate
//!
//! This module provides powerful macros that eliminate the need for repetitive
//! decoder function declarations. Instead of writing dozens of similar match
//! statements, you can declare decoders using simple, readable syntax.
//!
//! ## Design Philosophy
//! Macros reduce visual noise and make decoder definitions more maintainable.
//! They enforce consistent patterns and reduce the chance of bugs from
//! copy-paste errors.
//!
//! ## Before & After Example
//!
//! **Before** (manual decoder function):
//! ```ignore
//! fn decode_scene_type(value: i16) -> String {
//!     match value {
//!         0 => "None".to_string(),
//!         1 => "Food".to_string(),
//!         2 => "Sunset".to_string(),
//!         3 => "Blue Sky".to_string(),
//!         _ => format!("Unknown ({})", value),
//!     }
//! }
//! ```
//!
//! **After** (using simple_decoder! macro):
//! ```ignore
//! simple_decoder!(decode_scene_type, i16, {
//!     0 => "None",
//!     1 => "Food",
//!     2 => "Sunset",
//!     3 => "Blue Sky",
//! });
//! ```
//!
//! ## Macro Inventory
//! - `simple_decoder!` - Creates a match-based decoder function
//! - `const_decoder!` - Creates a const SimpleValueDecoder
//! - `bitfield_decoder!` - Creates a const BitfieldDecoder
//! - `decoder_fn!` - Creates a decoder function with custom unknown formatting

/// Creates a simple match-based decoder function
///
/// This macro generates a complete decoder function with automatic
/// unknown value handling.
///
/// # Syntax
/// ```ignore
/// simple_decoder!(function_name, input_type, {
///     value1 => "string1",
///     value2 => "string2",
///     ...
/// });
/// ```
///
/// # Example
/// ```ignore
/// use oxidex::simple_decoder;
///
/// simple_decoder!(decode_quality, i16, {
///     1 => "Low",
///     2 => "Medium",
///     3 => "High",
/// });
///
/// assert_eq!(decode_quality(2), "Medium");
/// assert_eq!(decode_quality(99), "Unknown (99)");
/// ```
///
/// # Generated Code
/// The macro expands to:
/// ```rust
/// fn decode_quality(value: i16) -> String {
///     match value {
///         1 => "Low".to_string(),
///         2 => "Medium".to_string(),
///         3 => "High".to_string(),
///         _ => format!("Unknown ({})", value),
///     }
/// }
/// ```
#[macro_export]
macro_rules! simple_decoder {
    ($name:ident, $type:ty, { $($val:expr => $str:expr),* $(,)? }) => {
        fn $name(value: $type) -> String {
            match value {
                $(
                    $val => $str.to_string(),
                )*
                _ => format!("Unknown ({})", value),
            }
        }
    };
}

/// Creates a simple match-based decoder function with custom unknown format
///
/// This variant allows you to customize the unknown value message.
///
/// # Syntax
/// ```ignore
/// simple_decoder_custom!(function_name, input_type, "Custom Unknown", {
///     value1 => "string1",
///     value2 => "string2",
/// });
/// ```
///
/// # Example
/// ```ignore
/// use oxidex::simple_decoder_custom;
///
/// simple_decoder_custom!(decode_mode, i16, "Invalid Mode", {
///     0 => "Normal",
///     1 => "Sport",
/// });
///
/// assert_eq!(decode_mode(0), "Normal");
/// assert_eq!(decode_mode(99), "Invalid Mode (99)");
/// ```
#[macro_export]
macro_rules! simple_decoder_custom {
    ($name:ident, $type:ty, $unknown_prefix:expr, { $($val:expr => $str:expr),* $(,)? }) => {
        fn $name(value: $type) -> String {
            match value {
                $(
                    $val => $str.to_string(),
                )*
                _ => format!("{} ({})", $unknown_prefix, value),
            }
        }
    };
}

/// Creates a const SimpleValueDecoder for compile-time decoder definitions
///
/// This macro creates a constant decoder that can be used at compile-time,
/// offering better performance than function-based decoders.
///
/// # Syntax
/// ```ignore
/// const_decoder!(DECODER_NAME, input_type, [
///     (value1, "string1"),
///     (value2, "string2"),
/// ]);
/// ```
///
/// # Example
/// ```ignore
/// use oxidex::const_decoder;
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::SimpleValueDecoder;
///
/// const_decoder!(WHITE_BALANCE, i16, [
///     (0, "Auto"),
///     (1, "Daylight"),
///     (2, "Cloudy"),
///     (3, "Tungsten"),
/// ]);
///
/// assert_eq!(WHITE_BALANCE.decode(1), "Daylight");
/// ```
///
/// # Generated Code
/// ```ignore
/// const WHITE_BALANCE: SimpleValueDecoder<i16> = SimpleValueDecoder::new(&[
///     (0, "Auto"),
///     (1, "Daylight"),
///     (2, "Cloudy"),
///     (3, "Tungsten"),
/// ]);
/// ```
#[macro_export]
macro_rules! const_decoder {
    ($name:ident, $type:ty, [ $( ($val:expr, $str:expr) ),* $(,)? ]) => {
        const $name: $crate::parsers::tiff::makernotes::shared::generic_decoders::SimpleValueDecoder<$type> =
            $crate::parsers::tiff::makernotes::shared::generic_decoders::SimpleValueDecoder::new(&[
                $(
                    ($val, $str),
                )*
            ]);
    };
}

/// Creates a const BitfieldDecoder for multi-flag value decoding
///
/// # Syntax
/// ```ignore
/// bitfield_decoder!(DECODER_NAME, [
///     (bit_mask1, "name1"),
///     (bit_mask2, "name2"),
/// ]);
/// ```
///
/// # Example
/// ```ignore
/// use oxidex::bitfield_decoder;
/// use oxidex::parsers::tiff::makernotes::shared::generic_decoders::BitfieldDecoder;
///
/// bitfield_decoder!(CAMERA_FEATURES, [
///     (0x01, "HDR"),
///     (0x02, "Panorama"),
///     (0x04, "Night Mode"),
/// ]);
///
/// assert_eq!(CAMERA_FEATURES.decode(0x03), "HDR, Panorama");
/// ```
#[macro_export]
macro_rules! bitfield_decoder {
    ($name:ident, [ $( ($mask:expr, $str:expr) ),* $(,)? ]) => {
        const $name: $crate::parsers::tiff::makernotes::shared::generic_decoders::BitfieldDecoder =
            $crate::parsers::tiff::makernotes::shared::generic_decoders::BitfieldDecoder::new(&[
                $(
                    ($mask, $str),
                )*
            ]);
    };
}

/// Creates a decoder function with optional value handling
///
/// This macro generates a decoder that returns Option<String>, allowing
/// for explicit handling of invalid values.
///
/// # Syntax
/// ```ignore
/// decoder_fn!(function_name, input_type, {
///     value1 => "string1",
///     value2 => "string2",
/// });
/// ```
///
/// # Example
/// ```ignore
/// use oxidex::decoder_fn;
///
/// decoder_fn!(decode_optional, i16, {
///     0 => "Zero",
///     1 => "One",
/// });
///
/// assert_eq!(decode_optional(0), Some("Zero".to_string()));
/// assert_eq!(decode_optional(99), None);
/// ```
#[macro_export]
macro_rules! decoder_fn {
    ($name:ident, $type:ty, { $($val:expr => $str:expr),* $(,)? }) => {
        fn $name(value: $type) -> Option<String> {
            match value {
                $(
                    $val => Some($str.to_string()),
                )*
                _ => None,
            }
        }
    };
}

/// Creates a decoder function with documentation
///
/// This macro adds documentation to the generated decoder function,
/// making the code more maintainable and self-documenting.
///
/// # Syntax
/// ```ignore
/// documented_decoder!(
///     /// Your documentation here
///     function_name, input_type, {
///         value1 => "string1",
///         value2 => "string2",
///     }
/// );
/// ```
///
/// # Example
/// ```ignore
/// use oxidex::documented_decoder;
///
/// documented_decoder!(
///     /// Decodes the camera's shooting mode
///     ///
///     /// # Arguments
///     /// * `value` - The mode value from the MakerNote
///     decode_shooting_mode, i16, {
///         0 => "Auto",
///         1 => "Manual",
///         2 => "Aperture Priority",
///         3 => "Shutter Priority",
///     }
/// );
/// ```
#[macro_export]
macro_rules! documented_decoder {
    (
        $(#[$meta:meta])*
        $name:ident, $type:ty, { $($val:expr => $str:expr),* $(,)? }
    ) => {
        $(#[$meta])*
        fn $name(value: $type) -> String {
            match value {
                $(
                    $val => $str.to_string(),
                )*
                _ => format!("Unknown ({})", value),
            }
        }
    };
}

/// Creates multiple related decoder functions at once
///
/// This macro is useful when you have several related decoders that
/// share similar patterns.
///
/// # Syntax
/// ```ignore
/// decoder_group! {
///     decoder1_name, type1, { ... },
///     decoder2_name, type2, { ... },
/// }
/// ```
///
/// # Example
/// ```ignore
/// use oxidex::decoder_group;
///
/// decoder_group! {
///     decode_on_off, i16, {
///         0 => "Off",
///         1 => "On",
///     },
///     decode_yes_no, i16, {
///         0 => "No",
///         1 => "Yes",
///     },
/// }
/// ```
#[macro_export]
macro_rules! decoder_group {
    ($($name:ident, $type:ty, { $($val:expr => $str:expr),* $(,)? }),* $(,)?) => {
        $(
            fn $name(value: $type) -> String {
                match value {
                    $(
                        $val => $str.to_string(),
                    )*
                    _ => format!("Unknown ({})", value),
                }
            }
        )*
    };
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {

    #[test]
    fn test_simple_decoder_macro() {
        simple_decoder!(test_quality, i16, {
            1 => "Low",
            2 => "Medium",
            3 => "High",
        });

        assert_eq!(test_quality(1), "Low");
        assert_eq!(test_quality(2), "Medium");
        assert_eq!(test_quality(3), "High");
        assert_eq!(test_quality(99), "Unknown (99)");
    }

    #[test]
    fn test_simple_decoder_custom_macro() {
        simple_decoder_custom!(test_mode, i16, "Invalid Mode", {
            0 => "Normal",
            1 => "Sport",
            2 => "Portrait",
        });

        assert_eq!(test_mode(0), "Normal");
        assert_eq!(test_mode(1), "Sport");
        assert_eq!(test_mode(99), "Invalid Mode (99)");
    }

    #[test]
    fn test_const_decoder_macro() {
        const_decoder!(TEST_WB, i16, [(0, "Auto"), (1, "Daylight"), (2, "Cloudy"),]);

        assert_eq!(TEST_WB.decode(0), "Auto");
        assert_eq!(TEST_WB.decode(1), "Daylight");
        assert_eq!(TEST_WB.decode(2), "Cloudy");
        assert_eq!(TEST_WB.decode(99), "Unknown (99)");
    }

    #[test]
    fn test_bitfield_decoder_macro() {
        bitfield_decoder!(
            TEST_FEATURES,
            [(0x01, "HDR"), (0x02, "Panorama"), (0x04, "Night Mode"),]
        );

        assert_eq!(TEST_FEATURES.decode(0x00), "None");
        assert_eq!(TEST_FEATURES.decode(0x01), "HDR");
        assert_eq!(TEST_FEATURES.decode(0x03), "HDR, Panorama");
    }

    #[test]
    fn test_decoder_fn_macro() {
        decoder_fn!(test_optional, i16, {
            0 => "Zero",
            1 => "One",
            2 => "Two",
        });

        assert_eq!(test_optional(0), Some("Zero".to_string()));
        assert_eq!(test_optional(1), Some("One".to_string()));
        assert_eq!(test_optional(99), None);
    }

    #[test]
    fn test_documented_decoder_macro() {
        documented_decoder!(
            /// Test decoder with documentation
            test_documented, i16, {
                0 => "First",
                1 => "Second",
            }
        );

        assert_eq!(test_documented(0), "First");
        assert_eq!(test_documented(1), "Second");
        assert_eq!(test_documented(5), "Unknown (5)");
    }

    #[test]
    fn test_decoder_group_macro() {
        decoder_group! {
            test_group_a, i16, {
                0 => "A0",
                1 => "A1",
            },
            test_group_b, i16, {
                0 => "B0",
                1 => "B1",
            },
        }

        assert_eq!(test_group_a(0), "A0");
        assert_eq!(test_group_a(1), "A1");
        assert_eq!(test_group_b(0), "B0");
        assert_eq!(test_group_b(1), "B1");
    }

    #[test]
    fn test_trailing_commas() {
        // Test that trailing commas are handled correctly
        simple_decoder!(test_trailing, i16, {
            0 => "Zero",
            1 => "One",
        });

        const_decoder!(TEST_TRAILING_CONST, i16, [(0, "Zero"), (1, "One"),]);

        assert_eq!(test_trailing(0), "Zero");
        assert_eq!(TEST_TRAILING_CONST.decode(1), "One");
    }
}
