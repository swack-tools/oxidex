//! Value formatters for ExifTool-compatible output
//!
//! This module contains formatters that transform raw metadata values into
//! human-readable strings matching ExifTool's output format.

pub mod cfa_pattern;
pub mod exif_enums;
pub mod exposure_program;
pub mod gps_altitude_ref;
pub mod gps_direction_ref;
pub mod gps_lat_lon_ref;
pub mod gps_processing_method;
pub mod gps_speed_ref;
pub mod gps_status;
pub mod interop_version;
pub mod numeric_precision;
pub mod scene_type;
pub mod unit_suffixes;
pub mod ycbcr_subsampling;

// Re-export main formatting functions for convenience
pub use cfa_pattern::decode_cfa_pattern;
pub use exif_enums::{
    format_color_space, format_components_configuration, format_compression, format_contrast,
    format_custom_rendered, format_digital_zoom_ratio, format_exposure_mode, format_file_source,
    format_flash, format_gain_control, format_interop_index, format_light_source,
    format_metering_mode, format_orientation, format_resolution_unit, format_saturation,
    format_scene_capture_type, format_sensing_method, format_sharpness,
    format_subject_distance_range, format_white_balance, format_ycbcr_positioning,
};
pub use exposure_program::format_exposure_program;
pub use gps_altitude_ref::{format_gps_altitude_ref, format_gps_altitude_ref_byte};
pub use gps_direction_ref::format_gps_direction_ref;
pub use gps_lat_lon_ref::{format_gps_lat_ref, format_gps_lon_ref};
pub use gps_processing_method::decode_gps_processing_method;
pub use gps_speed_ref::format_gps_speed_ref;
pub use gps_status::format_gps_status;
pub use interop_version::decode_version_bytes;
pub use numeric_precision::{
    format_exif_rational, format_icc_value, format_integer_precision_values,
    format_three_decimal_values, is_icc_matrix_tag, is_integer_precision_tag, is_three_decimal_tag,
};
pub use scene_type::decode_scene_type;
pub use unit_suffixes::format_with_unit;
pub use ycbcr_subsampling::{format_ycbcr_subsampling, format_ycbcr_subsampling_string};
