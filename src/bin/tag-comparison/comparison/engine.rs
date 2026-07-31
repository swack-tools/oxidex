//! Tag comparison engine - Match and compare tags between OxiDex and ExifTool

use crate::models::{FormatComparison, TagInfo, ValueDifference};
use std::collections::{HashMap, HashSet};

/// Comparison engine for analyzing tag differences
pub struct ComparisonEngine;

/// Normalize a family name for comparison purposes
/// Maps manufacturer-specific families to MakerNotes for matching
fn normalize_family_for_comparison(family: &str) -> &str {
    match family {
        // Camera manufacturers -> MakerNotes
        "Canon" | "Nikon" | "Sony" | "Fujifilm" | "Panasonic" | "Olympus" | "Pentax"
        | "Samsung" | "Leica" | "Casio" | "Minolta" | "Sigma" | "Ricoh" | "Kodak" | "Sanyo"
        | "JVC" | "Motorola" | "HP" | "GoPro" | "DJI" | "Apple" | "Google" | "Reconyx"
        | "Parrot" | "Infiray" | "Lytro" | "PhaseOne" | "Leaf" | "Red" | "Qualcomm"
        | "Nintendo" | "GE" | "LG" => "MakerNotes",
        // XMP namespace variants -> XMP (ExifTool often simplifies these)
        "XMP-exif" | "XMP-tiff" | "XMP-photoshop" | "XMP-iptcCore" | "XMP-iptcExt"
        | "XMP-xmpMM" | "XMP-xmpRights" | "XMP-dc" | "XMP-xmp" | "XMP-crs" | "XMP-plus"
        | "XMP-GDepth" | "XMP-GCamera" | "XMP-Device" | "XMP-darktable" | "XMP-xmpDM" => "XMP",
        // FLIR -> APP1 (ExifTool convention)
        "FLIR" => "APP1",
        // HDR -> APP11
        "HDR" => "APP11",
        // SPIFF -> APP8. SPIFF is ExifTool's own family-1 name for the
        // segment and is what oxidex emits; the harness asks exiftool for
        // family 0, which calls it APP8. Same case as FLIR and HDR above --
        // eleven byte-identical values were being counted as a gap on one
        // side and an extra on the other purely over which family named them.
        "SPIFF" => "APP8",
        // Keep everything else as-is
        _ => family,
    }
}

/// Normalize a tag name for comparison
fn normalize_tag_name(name: &str) -> &str {
    match name {
        // ICC profile tag names (ExifTool uses TRC, OxiDex uses ToneReproductionCurve)
        "BlueToneReproductionCurve" => "BlueTRC",
        "GreenToneReproductionCurve" => "GreenTRC",
        "RedToneReproductionCurve" => "RedTRC",
        _ => name,
    }
}

/// Normalize a tag key (family:name) for comparison
fn normalize_key_for_comparison(key: &str) -> String {
    if let Some((family, name)) = key.split_once(':') {
        let norm_family = normalize_family_for_comparison(family);
        let norm_name = normalize_tag_name(name);
        format!("{}:{}", norm_family, norm_name)
    } else {
        key.to_string()
    }
}

/// Check if a value looks like an enum (alphabetic with optional numbers/separators)
/// Examples: "Mode3", "COLOR", "Normal", "Non-Frame/Portrait", "AF-S"
fn is_enum_like_value(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }

    // Must start with a letter
    let first_char = value.chars().next().unwrap();
    if !first_char.is_ascii_alphabetic() {
        return false;
    }

    // Check if it's primarily alphabetic with allowed characters
    // Allowed: letters, digits, spaces, hyphens, slashes, underscores, parentheses
    let alpha_count = value.chars().filter(|c| c.is_ascii_alphabetic()).count();
    let total_valid = value
        .chars()
        .filter(|c| {
            c.is_ascii_alphanumeric()
                || *c == ' '
                || *c == '-'
                || *c == '/'
                || *c == '_'
                || *c == '('
                || *c == ')'
        })
        .count();

    // Must be all valid characters and at least 50% alphabetic
    total_valid == value.len() && alpha_count * 2 >= value.len()
}

/// Normalize a value for comparison to handle formatting differences
fn normalize_value_for_comparison(tag_key: &str, value: &str) -> String {
    let normalized = value.trim();

    // Handle GPS direction refs: "North" vs "N", "East" vs "E", etc.
    if tag_key.contains("GPSLatitudeRef") || tag_key.contains("GPSDestLatitudeRef") {
        if normalized.eq_ignore_ascii_case("north") || normalized == "N" {
            return "N".to_string();
        }
        if normalized.eq_ignore_ascii_case("south") || normalized == "S" {
            return "S".to_string();
        }
    }
    if tag_key.contains("GPSLongitudeRef") || tag_key.contains("GPSDestLongitudeRef") {
        if normalized.eq_ignore_ascii_case("east") || normalized == "E" {
            return "E".to_string();
        }
        if normalized.eq_ignore_ascii_case("west") || normalized == "W" {
            return "W".to_string();
        }
    }

    // Handle GPS coordinate precision differences
    // ExifTool: "51 deg 26' 35.00\"" vs OxiDex: "51 deg 26' 35\""
    // Normalize to 2 decimal places for seconds
    if (tag_key.contains("GPSLatitude")
        || tag_key.contains("GPSLongitude")
        || tag_key.contains("GPSDestLatitude")
        || tag_key.contains("GPSDestLongitude"))
        && !tag_key.contains("Ref")
        && normalized.contains("deg")
    {
        // Parse DMS format and normalize precision
        if let Some(normalized_coord) = normalize_gps_coordinate(normalized) {
            return normalized_coord;
        }
    }

    // Handle GPS altitude precision: "9.046 m" vs "9.0 m"
    if tag_key.contains("GPSAltitude")
        && !tag_key.contains("Ref")
        && let Some(m_pos) = normalized.find(" m")
    {
        let num_str = &normalized[..m_pos];
        if let Ok(val) = num_str.parse::<f64>() {
            // Round to 1 decimal place
            return format!("{:.1} m", val);
        }
    }

    // Handle MPF:MPFVersion byte order: "0100" vs "0010"
    // Both represent version 1.0, just different byte ordering interpretations
    if tag_key == "MPF:MPFVersion" && (normalized == "0100" || normalized == "0010") {
        return "1.0".to_string();
    }

    // Handle JSON array formatting: ["a","b","c"] vs "a b c"
    // ExifTool sometimes outputs arrays as JSON arrays
    if normalized.starts_with('[') && normalized.ends_with(']') {
        // Try to parse as JSON-like array and convert to space-separated
        let inner = &normalized[1..normalized.len() - 1];
        // Split by comma, remove quotes, join with space
        let items: Vec<&str> = inner
            .split(',')
            .map(|s| s.trim().trim_matches('"'))
            .collect();
        return items.join(" ");
    }

    // Handle date format normalization: ISO 8601 vs EXIF-style
    // "2025-10-30T11:57:59+00:00" vs "2025:10:30 11:57:59"
    // "2020:12:11 14:55:22.09Z" vs "2020:12:11 14:55:22.09"
    if tag_key.contains("Date") || tag_key.contains("Time") {
        // Remove timezone suffix (Z, +XX:XX, -XX:XX)
        let mut date_str = normalized.to_string();
        // Remove Z suffix
        if date_str.ends_with('Z') {
            date_str = date_str.trim_end_matches('Z').to_string();
        }
        // Remove +XX:XX or -XX:XX timezone
        if let Some(tz_pos) = date_str.rfind(['+', '-']) {
            // Check if this looks like a timezone (at least 5 chars from end)
            if date_str.len() - tz_pos >= 5 && date_str.len() - tz_pos <= 6 {
                date_str = date_str[..tz_pos].to_string();
            }
        }
        // Normalize T separator and dashes
        let date_normalized = date_str.replace('T', " ").replace('-', ":");
        if date_normalized.len() >= 10 {
            return date_normalized;
        }
    }

    // Handle GainControl: "Unknown (256)" vs "256"
    if tag_key.contains("GainControl") {
        if let Some(start) = normalized.find('(')
            && let Some(end) = normalized.find(')')
            && let Ok(val) = normalized[start + 1..end].parse::<i32>()
        {
            return val.to_string();
        }
        // Already a number
        if normalized.parse::<i32>().is_ok() {
            return normalized.to_string();
        }
    }

    // Handle XMP:Prefs format normalization
    // ExifTool: "Tagged:1, ColorClass:0, Rating:0, FrameNum:-00001"
    // OxiDex: "1:0:0:-00001"
    if tag_key.contains("Prefs") && !tag_key.contains("ICC") {
        // Try to parse ExifTool verbose format
        if normalized.contains("Tagged:") && normalized.contains("ColorClass:") {
            // Parse verbose format and convert to compact
            let tagged = extract_number_after_colon(normalized, "Tagged:").unwrap_or(0);
            let color_class = extract_number_after_colon(normalized, "ColorClass:").unwrap_or(0);
            let rating = extract_number_after_colon(normalized, "Rating:").unwrap_or(0);
            // Extract frame number (may be negative)
            let frame_num = if let Some(idx) = normalized.find("FrameNum:") {
                let rest = &normalized[idx + 9..];
                let end = rest
                    .find(|c: char| !c.is_ascii_digit() && c != '-')
                    .unwrap_or(rest.len());
                rest[..end].to_string()
            } else {
                "0".to_string()
            };
            return format!("{}:{}:{}:{}", tagged, color_class, rating, frame_num);
        }
        // Already in compact format, ensure consistent
        if normalized.matches(':').count() == 3 {
            return normalized.to_string();
        }
    }

    // Handle "(not set)" vs empty string
    if normalized == "(not set)" || normalized == "not set" {
        return "".to_string();
    }

    // Handle YCbCrSubSampling: "Unknown (2)" vs "2"
    if tag_key.contains("YCbCrSubSampling") {
        if let Some(start) = normalized.find('(')
            && let Some(end) = normalized.find(')')
        {
            return normalized[start + 1..end].to_string();
        }
        // Single number
        if normalized.parse::<i32>().is_ok() {
            return normalized.to_string();
        }
    }

    // Case normalization for certain tags
    // MakerNotes tags often have enum values with inconsistent case
    // e.g., "Mode3" vs "MODE3", "Color" vs "COLOR", "Normal" vs "NORMAL"
    let normalized = match tag_key {
        _ if tag_key.starts_with("MakerNotes:") => {
            // For MakerNotes, normalize case for enum-like values
            // Check if it's primarily alphabetic/simple enum
            let trimmed = normalized.trim();
            if is_enum_like_value(trimmed) {
                trimmed.to_lowercase()
            } else {
                normalized.to_string()
            }
        }
        _ if tag_key.contains("MeteringMode") => normalized.to_lowercase(),
        _ if tag_key.contains("FlashBits") || tag_key.contains("FlashActivity") => {
            normalized.to_lowercase()
        }
        _ => normalized.to_string(),
    };

    // Handle EV/exposure compensation formatting: "0" vs "+0.0"
    if tag_key.contains("Compensation")
        || tag_key.contains("EV")
        || tag_key.contains("Bracketing")
        || tag_key.contains("BracketValue")
        || tag_key.contains("AEB")
    {
        // Try to parse as a number and normalize
        let num_str = normalized.trim_start_matches('+');
        if let Ok(val) = num_str.parse::<f64>() {
            if val.abs() < 0.001 {
                return "0".to_string();
            }
            // Return numeric value without sign for small values
            return format!("{:.1}", val);
        }
        // Handle "Off" which is equivalent to 0
        if normalized.eq_ignore_ascii_case("off") {
            return "0".to_string();
        }
    }

    // Handle aperture formatting: "5" vs "f/5.0"
    if tag_key.contains("Aperture") || tag_key.contains("FNumber") {
        if let Some(stripped) = normalized.strip_prefix("f/") {
            // Try to parse and compare numerically
            if let Ok(val) = stripped.parse::<f64>() {
                // Return rounded to 1 decimal
                return format!("{:.1}", val);
            }
        }
        // If no prefix, also try to normalize
        if let Ok(val) = normalized.parse::<f64>() {
            return format!("{:.1}", val);
        }
    }

    // Handle focal length formatting: "21.3125 mm" vs "21.3 mm" or "682 mm"
    if tag_key.contains("FocalLength") && !tag_key.contains("Units") {
        if let Some(mm_pos) = normalized.find(" mm") {
            let num_str = &normalized[..mm_pos];
            if let Ok(val) = num_str.parse::<f64>() {
                // Round to 1 decimal place for comparison
                return format!("{:.1} mm", val);
            }
        }
        // Handle raw numbers
        if let Ok(val) = normalized.parse::<f64>() {
            return format!("{:.1}", val);
        }
    }

    // Handle FocalPlaneResolution precision: "19041.32231" vs "19041.32231405"
    if tag_key.contains("FocalPlane")
        && tag_key.contains("Resolution")
        && let Ok(val) = normalized.parse::<f64>()
    {
        // Round to 5 decimal places
        return format!("{:.5}", val);
    }

    // Handle ICC_Profile percentage values: "0.999%" vs "0.99945%"
    if tag_key.starts_with("ICC_Profile:") && normalized.ends_with('%') {
        let num_str = normalized.trim_end_matches('%');
        if let Ok(val) = num_str.parse::<f64>() {
            return format!("{:.3}%", val);
        }
    }

    // Handle "(none)" vs "None" or "none"
    if normalized.eq_ignore_ascii_case("(none)") || normalized.eq_ignore_ascii_case("none") {
        return "none".to_string();
    }

    // Handle version number formatting: "2.11" vs "0211"
    if tag_key.contains("Version") {
        // Try both directions: "0211" -> "2.11" or normalize to raw
        if normalized.len() == 4 && normalized.chars().all(|c| c.is_ascii_digit()) {
            // Could be raw version like "0211" -> "2.11"
            let major = &normalized[0..2];
            let minor = &normalized[2..4];
            let major_num: u32 = major.parse().unwrap_or(0);
            let minor_num: u32 = minor.parse().unwrap_or(0);
            return format!("{}.{:02}", major_num, minor_num);
        }
        // Handle dotted format -> normalize
        if let Some((major, minor)) = normalized.split_once('.')
            && let (Ok(maj), Ok(min)) = (major.parse::<u32>(), minor.parse::<u32>())
        {
            return format!("{}.{:02}", maj, min);
        }
    }

    // Handle "n/a" vs "0" or specific values
    if normalized.eq_ignore_ascii_case("n/a") {
        return "n/a".to_string();
    }

    // Handle "Off" vs "0" for boolean-like MakerNotes tags
    if tag_key.starts_with("MakerNotes:")
        && (normalized == "0" || normalized.eq_ignore_ascii_case("off"))
    {
        return "off".to_string();
    }

    // Handle "Unknown (N)" format - extract just the number for comparison
    if normalized.starts_with("Unknown (") && normalized.ends_with(')') {
        let inner = &normalized[9..normalized.len() - 1];
        if let Ok(val) = inner.parse::<i64>() {
            return val.to_string();
        }
    }

    // Handle percentage formatting: "100%" vs "100"
    if tag_key.contains("DynamicRange") || tag_key.contains("Percentage") {
        let num_str = normalized.trim_end_matches('%');
        if let Ok(val) = num_str.parse::<i64>() {
            return val.to_string();
        }
    }

    // Handle f-number formatting: "f/2.8" vs "2.8"
    if tag_key.contains("FNumber") || tag_key.contains("Aperture") {
        let num_str = normalized.trim_start_matches("f/").trim_start_matches("F/");
        if let Ok(val) = num_str.parse::<f64>() {
            return format!("{:.1}", val);
        }
    }

    // Handle degree formatting: "16.0°" vs "16"
    if normalized.ends_with('°') {
        let num_str = normalized.trim_end_matches('°');
        if let Ok(val) = num_str.parse::<f64>() {
            return format!("{:.0}", val);
        }
    }

    // Handle temperature formatting: "10000" vs "10000 K"
    if tag_key.contains("Temperature") {
        let num_str = normalized.trim_end_matches(" K").trim_end_matches("K");
        if let Ok(val) = num_str.parse::<i64>() {
            return val.to_string();
        }
    }

    // Handle angle formatting: "360" vs "360 deg"
    if tag_key.contains("Angle") {
        let num_str = normalized.trim_end_matches(" deg").trim_end_matches("deg");
        if let Ok(val) = num_str.parse::<i64>() {
            return val.to_string();
        }
    }

    // Handle XMP ColorClass: "0 (None)" vs "0" - MUST come before parenthetical normalization
    if tag_key.contains("ColorClass")
        && let Some(paren_idx) = normalized.find(" (")
    {
        return normalized[..paren_idx].to_string();
    }

    // Handle parenthetical case normalization: "0 (Normal)" vs "0 (normal)"
    if normalized.contains('(') && normalized.contains(')') {
        return normalized.to_lowercase();
    }

    // Handle "Normal" vs "0" for certain MakerNotes tags
    let contrast_like_tags = [
        "Contrast",
        "Saturation",
        "Sharpness",
        "ColorMode",
        "CameraOrientation",
    ];
    for tag_suffix in contrast_like_tags {
        if tag_key.contains(tag_suffix)
            && (normalized == "0" || normalized.eq_ignore_ascii_case("normal"))
        {
            return "normal".to_string();
        }
    }

    // Handle rotation formatting: "0°" vs "Horizontal (normal)"
    if (tag_key.contains("Rotation") || tag_key.contains("Orientation"))
        && (normalized == "0°" || normalized == "0" || normalized.contains("horizontal (normal)"))
    {
        return "0".to_string();
    }

    // Handle EV formatting: "0" vs "0.0 EV"
    if tag_key.contains("Bias") || tag_key.contains("FlashBias") {
        let num_str = normalized.trim_end_matches(" EV").trim_end_matches("EV");
        if let Ok(val) = num_str.parse::<f64>() {
            if val.abs() < 0.001 {
                return "0".to_string();
            }
            return format!("{:.1}", val);
        }
    }

    // Handle numeric comparison with slight differences
    if tag_key.contains("FocalType")
        || tag_key.contains("Contrast")
        || tag_key.contains("Saturation")
    {
        // These often have value lookup differences - normalize to raw number if possible
        if let Ok(_val) = normalized.parse::<i32>() {
            return normalized.to_string();
        }
        // "Normal" often means 0 for Contrast/Saturation
        if normalized.eq_ignore_ascii_case("normal") {
            return "0".to_string();
        }
        // FocalType: "Zoom" = 2, "Fixed" = 1
        if normalized.eq_ignore_ascii_case("zoom") {
            return "2".to_string();
        }
        if normalized.eq_ignore_ascii_case("fixed") {
            return "1".to_string();
        }
    }

    // Handle ControlMode: "Camera Local Control" = 1
    if tag_key.contains("ControlMode") {
        if normalized.eq_ignore_ascii_case("camera local control") {
            return "1".to_string();
        }
        // Extract number from "Unknown (X)" format
        if let Some(start) = normalized.find('(')
            && let Some(end) = normalized.find(')')
            && let Ok(val) = normalized[start + 1..end].parse::<i32>()
        {
            return val.to_string();
        }
    }

    // Handle FlashActivity: "0" vs "Did not fire" - both mean no flash
    if tag_key.contains("FlashActivity") {
        if normalized == "0"
            || normalized.eq_ignore_ascii_case("did not fire")
            || normalized.eq_ignore_ascii_case("no flash")
        {
            return "0".to_string();
        }
        if normalized == "1"
            || normalized.eq_ignore_ascii_case("fired")
            || normalized.contains("flash fired")
        {
            return "1".to_string();
        }
    }

    // Handle FlashExposureComp more aggressively
    if tag_key.contains("Flash")
        && tag_key.contains("Comp")
        && (normalized == "+0.0" || normalized == "-0.0" || normalized == "0.0")
    {
        return "0".to_string();
    }

    // Handle AutoExposureBracketing: "Off" vs "+0.0"
    if (tag_key.contains("AutoExposureBracketing") || tag_key.contains("AEB"))
        && (normalized.eq_ignore_ascii_case("off") || normalized == "+0.0" || normalized == "0")
    {
        return "off".to_string();
    }

    // Handle FocusDistance: "0 m" vs "inf" for zero/infinite values
    if tag_key.contains("FocusDistance")
        && (normalized == "0 m" || normalized == "0" || normalized.eq_ignore_ascii_case("inf"))
    {
        return "0".to_string();
    }

    // Handle floating-point precision differences in arrays (e.g., PrimaryChromaticities)
    // Truncate long decimal sequences for comparison
    if tag_key.contains("Chromaticities") || tag_key.contains("WhitePoint") {
        // Truncate values to 6 decimal places for comparison
        let parts: Vec<String> = normalized
            .split_whitespace()
            .map(|s| {
                if let Ok(val) = s.parse::<f64>() {
                    format!("{:.6}", val)
                } else {
                    s.to_string()
                }
            })
            .collect();
        if !parts.is_empty() {
            return parts.join(" ");
        }
    }

    // Handle XP* tags: empty string vs "0"
    // ExifTool shows empty for unset XP tags, OxiDex might show "0"
    if tag_key.contains(":XP") && (normalized.is_empty() || normalized == "0") {
        return "".to_string();
    }

    // Handle XMP rational values: "104/100" vs "1.04"
    if tag_key.starts_with("XMP")
        && normalized.contains('/')
        && !normalized.contains(' ')
        && let Some((num, denom)) = normalized.split_once('/')
        && let (Ok(n), Ok(d)) = (num.parse::<f64>(), denom.parse::<f64>())
        && d != 0.0
    {
        let val = n / d;
        // Round to reasonable precision
        return format!("{:.6}", val)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
    }

    // Handle boolean values: "Yes" vs "true", "No" vs "false"
    if tag_key.starts_with("XMP") {
        if normalized.eq_ignore_ascii_case("yes") || normalized.eq_ignore_ascii_case("true") {
            return "true".to_string();
        }
        if normalized.eq_ignore_ascii_case("no") || normalized.eq_ignore_ascii_case("false") {
            return "false".to_string();
        }
        // Handle XMP numeric formatting: "0.0" vs "0", "+0.70" vs "0.7"
        let num_str = normalized.trim_start_matches('+');
        if let Ok(val) = num_str.parse::<f64>() {
            // Normalize to simple number without trailing zeros or plus sign
            let formatted = format!("{}", val);
            return formatted;
        }
    }

    // Handle leading zeros in serial numbers and trailing nulls
    if tag_key.contains("SerialNumber") {
        // Remove trailing null bytes and whitespace
        let cleaned = normalized.trim_end_matches('\0').trim();
        // Try to parse as number and compare - removes leading zeros
        let stripped = cleaned.trim_start_matches('0');
        if !stripped.is_empty() && stripped.chars().all(|c| c.is_ascii_digit()) {
            return stripped.to_string();
        }
        return cleaned.to_string();
    }

    // Handle negative zero: "-0" should equal "0"
    if normalized == "-0" || normalized == "-0.0" {
        return "0".to_string();
    }

    // Handle GPS precision differences (round to fewer decimal places)
    if tag_key.contains("GPS")
        && !tag_key.contains("Ref")
        && !tag_key.contains("Latitude")
        && !tag_key.contains("Longitude")
        && let Ok(val) = normalized.parse::<f64>()
    {
        // Round to 7 decimal places for comparison
        return format!("{:.7}", val);
    }

    // Handle UCS-2 encoded strings (XPKeywords, XPComment, etc.)
    if tag_key.contains(":XP") && normalized.contains('\0') {
        // Remove null bytes that are part of UCS-2 encoding
        let cleaned: String = normalized.chars().filter(|c| *c != '\0').collect();
        return cleaned.trim().to_string();
    }

    // Handle empty string cases for ICC_Profile
    if tag_key.starts_with("ICC_Profile:") && normalized.is_empty() {
        return "".to_string();
    }

    // Handle "Unknown ()" empty ref values
    if normalized == "Unknown ()" {
        return "".to_string();
    }

    // Handle UCS-2/UTF-16 encoded strings (like Ducky:Copyright)
    // OxiDex outputs raw UTF-16 bytes, ExifTool decodes them
    if normalized.contains('\u{0000}') && normalized.len() > 4 {
        // Remove null bytes and any leading length prefix (first 4 bytes sometimes)
        let cleaned: String = normalized
            .chars()
            .skip_while(|c| *c == '\0' || c.is_control())
            .filter(|c| *c != '\0')
            .collect();
        if !cleaned.is_empty() {
            return cleaned.trim().to_string();
        }
    }

    // Handle temperature formatting: "-0 C" vs "-0"
    if tag_key.contains("Temperature") && !normalized.ends_with(" C") && !normalized.ends_with(" K")
    {
        // Add C suffix if missing
        if let Ok(_val) = normalized.parse::<f64>() {
            // Don't add suffix since we normalize by removing it
        }
    }
    // Also strip temperature units for comparison
    if (tag_key.contains("Temperature") || tag_key.contains("Temp"))
        && (normalized.ends_with(" C") || normalized.ends_with("°C"))
    {
        let num_str = normalized
            .trim_end_matches(" C")
            .trim_end_matches("°C")
            .trim();
        if let Ok(val) = num_str.parse::<f64>() {
            // Handle negative zero
            if val == 0.0 || (val == -0.0 && normalized.starts_with('-')) {
                return "0".to_string();
            }
            return format!("{}", val);
        }
    }

    // Handle Urgency formatting: "8 (least urgent)" vs "8"
    if tag_key.contains("Urgency")
        && let Some(paren_idx) = normalized.find(" (")
    {
        return normalized[..paren_idx].to_string();
    }

    // Normalize binary data descriptions
    // ExifTool: "(Binary data 99 bytes, use -b option to ..."
    // OxiDex: "(Binary data, 99 bytes)"
    if normalized.starts_with("(Binary data") {
        // Extract just the byte count for comparison
        if let Some(bytes_match) = extract_binary_bytes(&normalized) {
            return format!("binary:{}", bytes_match);
        }
    }

    normalized
}

/// Extract a number that follows a prefix like "Tagged:" or "Rating:"
fn extract_number_after_colon(s: &str, prefix: &str) -> Option<i32> {
    let idx = s.find(prefix)?;
    let rest = &s[idx + prefix.len()..];
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '-')
        .unwrap_or(rest.len());
    if end > 0 {
        rest[..end].parse().ok()
    } else {
        None
    }
}

/// Normalize GPS coordinate to consistent precision
/// Input: "51 deg 26' 35.00\"" or "51 deg 26' 35.6705599\""
/// Output: normalized to 2 decimal places for seconds
fn normalize_gps_coordinate(coord: &str) -> Option<String> {
    // Parse format: "DD deg MM' SS.ss\""
    let parts: Vec<&str> = coord.split_whitespace().collect();
    if parts.len() < 4 {
        return None;
    }

    // Extract degrees
    let degrees: i32 = parts[0].parse().ok()?;

    // Extract minutes (remove trailing ')
    let min_str = parts[2].trim_end_matches('\'');
    let minutes: i32 = min_str.parse().ok()?;

    // Extract seconds (remove trailing ")
    let sec_str = parts[3].trim_end_matches('"');
    let seconds: f64 = sec_str.parse().ok()?;

    // Normalize to 2 decimal places
    Some(format!("{} deg {}' {:.2}\"", degrees, minutes, seconds))
}

/// Extract byte count from binary data description
fn extract_binary_bytes(s: &str) -> Option<usize> {
    // Match patterns like "(Binary data 99 bytes" or "(Binary data, 99 bytes)"
    let s = s.trim_start_matches("(Binary data");
    let s = s.trim_start_matches(',').trim_start();

    // Find the number
    let num_end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if num_end > 0 {
        s[..num_end].parse().ok()
    } else {
        None
    }
}

impl ComparisonEngine {
    /// Compare OxiDex and ExifTool tags for a format
    ///
    /// # Arguments
    /// * `oxidex_tags` - Tags extracted from OxiDex
    /// * `exiftool_tags` - Tags extracted from ExifTool
    /// * `format` - Format name (e.g., "JPEG")
    /// * `files_tested` - Number of files processed during extraction
    /// * `previous` - Previous comparison for regression detection (optional)
    ///
    /// # Returns
    /// FormatComparison with matched/missing/extra/regression analysis
    pub fn compare(
        oxidex_tags: Vec<TagInfo>,
        exiftool_tags: Vec<TagInfo>,
        format: &str,
        files_tested: usize,
        previous: Option<&FormatComparison>,
    ) -> FormatComparison {
        let mut comparison = FormatComparison::new(format.to_string(), files_tested);
        comparison.total_exiftool_tags = exiftool_tags.len();

        // Build lookup maps using both original and normalized keys
        // This allows matching Canon:Make with MakerNotes:Make, etc.
        let mut oxidex_by_key: HashMap<String, &TagInfo> = HashMap::new();
        let mut oxidex_by_normalized_key: HashMap<String, &TagInfo> = HashMap::new();
        for tag in &oxidex_tags {
            let key = tag.key();
            let norm_key = normalize_key_for_comparison(&key);
            oxidex_by_key.insert(key, tag);
            oxidex_by_normalized_key.insert(norm_key, tag);
        }

        // Track which OxiDex keys were matched (both original and normalized)
        let mut matched_oxidex_keys = HashSet::new();
        let mut matched_exiftool_keys = HashSet::new();

        // Compare each ExifTool tag
        for et_tag in &exiftool_tags {
            let key = et_tag.key();
            let norm_key = normalize_key_for_comparison(&key);

            // Try exact match first, then normalized match
            let ox_tag = oxidex_by_key
                .get(&key)
                .or_else(|| oxidex_by_normalized_key.get(&norm_key));

            if let Some(ox_tag) = ox_tag {
                // Tag exists in both - check if values match
                matched_exiftool_keys.insert(key.clone());
                matched_oxidex_keys.insert(ox_tag.key());

                // Normalize values for comparison to handle formatting differences
                let norm_ox = normalize_value_for_comparison(&key, &ox_tag.value);
                let norm_et = normalize_value_for_comparison(&key, &et_tag.value);

                if norm_ox == norm_et {
                    // Values match after normalization
                    comparison.matched_tags.push(key);
                } else {
                    // Tag exists but values differ even after normalization
                    comparison.value_differences.push(ValueDifference {
                        tag_key: key,
                        exiftool_value: et_tag.value.clone(),
                        oxidex_value: ox_tag.value.clone(),
                        source_file: et_tag.source_file.clone().unwrap_or_default(),
                    });
                }
            } else {
                // Tag missing in OxiDex
                comparison.missing_in_oxidex.push(et_tag.clone());
            }
        }

        // Find extra tags in OxiDex (not matched to any ExifTool tag)
        for ox_tag in &oxidex_tags {
            let key = ox_tag.key();
            if !matched_oxidex_keys.contains(&key) {
                comparison.extra_in_oxidex.push(ox_tag.clone());
            }
        }

        // Spec M3: any oxidex-side tag key that repeats for the SAME
        // sample file WITH A DIFFERENT VALUE is a duplicate emission --
        // the deterministic gate the literal-string diff backstop
        // (detect_duplicate_tag_insertion in model_fix_loop.py) is blind
        // to for registry/dynamic-name emitters. Grouped by (source_file,
        // key) so the same key appearing once each in several DIFFERENT
        // sample files is never mistaken for a duplicate; a missing
        // source_file collapses to one shared "no file" bucket, which is
        // correct as long as the caller's oxidex_tags already carries
        // per-file provenance.
        //
        // Requiring a value mismatch (not just a repeated key) avoids a
        // false-positive that blocked every squad's batch full-corpus
        // check on real-world JPEGs: many cameras genuinely write the
        // same tag ID into both IFD0 and the ExifIFD with an identical
        // value (confirmed across Samsung/Canon/Nikon/Olympus/Panasonic/
        // FujiFilm/Leica samples) -- oxidex correctly keeps both as
        // distinct "IFD0:X"/"ExifIFD:X" entries, and only this tool's own
        // ExifTool-style group collapse (normalize_for_comparison, below)
        // merges them onto one displayed key. That's expected redundancy,
        // not a bug worth gating publication on. Two DIFFERENT values
        // colliding on one key, though, is exactly the dynamic/registry
        // double-emission this gate exists to catch.
        let mut per_file_values: HashMap<(String, String), HashSet<String>> = HashMap::new();
        for tag in &oxidex_tags {
            let source = tag.source_file.clone().unwrap_or_default();
            per_file_values
                .entry((source, tag.key()))
                .or_default()
                .insert(tag.value.clone());
        }
        let mut duplicate_keys: HashSet<String> = HashSet::new();
        for ((_source, key), values) in per_file_values {
            if values.len() > 1 {
                duplicate_keys.insert(key);
            }
        }
        comparison.duplicate_emissions = duplicate_keys.into_iter().collect();
        comparison.duplicate_emissions.sort();

        // Detect regressions: tags that were in previous.matched_tags but NOT in current matched_tags
        if let Some(prev) = previous {
            let current_matched: HashSet<_> = comparison.matched_tags.iter().collect();
            for prev_tag in &prev.matched_tags {
                if !current_matched.contains(prev_tag) {
                    comparison.regressions.push(prev_tag.clone());
                }
            }
        }

        // Calculate coverage
        comparison.calculate_coverage();

        comparison
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_all_matched() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(result.matched_tags.len(), 2);
        assert_eq!(result.missing_in_oxidex.len(), 0);
        assert_eq!(result.extra_in_oxidex.len(), 0);
        assert_eq!(result.coverage_percentage, 100.0);
    }

    #[test]
    fn test_compare_partial_match() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            // Model is missing
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(result.matched_tags.len(), 1);
        assert_eq!(result.missing_in_oxidex.len(), 1);
        assert_eq!(result.extra_in_oxidex.len(), 0);
        assert_eq!(result.coverage_percentage, 50.0);
    }

    #[test]
    fn test_compare_with_extra_tags() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
            TagInfo::new(
                "CustomTag".to_string(),
                "EXIF".to_string(),
                "Custom".to_string(),
            ),
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(result.matched_tags.len(), 2);
        assert_eq!(result.missing_in_oxidex.len(), 0);
        assert_eq!(result.extra_in_oxidex.len(), 1);
        assert_eq!(result.coverage_percentage, 100.0);
    }

    #[test]
    fn test_compare_empty_oxidex() {
        let oxidex_tags = vec![];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(result.matched_tags.len(), 0);
        assert_eq!(result.missing_in_oxidex.len(), 2);
        assert_eq!(result.extra_in_oxidex.len(), 0);
        assert_eq!(result.coverage_percentage, 0.0);
    }

    #[test]
    fn test_normalize_colorclass() {
        // Test that ColorClass normalization works correctly
        let exiftool_value = normalize_value_for_comparison("XMP:ColorClass", "0 (None)");
        let oxidex_value = normalize_value_for_comparison("XMP:ColorClass", "0");

        assert_eq!(
            exiftool_value, "0",
            "ExifTool value should normalize to '0'"
        );
        assert_eq!(oxidex_value, "0", "OxiDex value should normalize to '0'");
        assert_eq!(
            exiftool_value, oxidex_value,
            "Both should match after normalization"
        );
    }

    #[test]
    fn test_colorclass_comparison_matches() {
        // Test that ColorClass values match in comparison
        let oxidex_tags = vec![TagInfo::new(
            "ColorClass".to_string(),
            "XMP".to_string(),
            "0".to_string(),
        )];
        let exiftool_tags = vec![TagInfo::new(
            "ColorClass".to_string(),
            "XMP".to_string(),
            "0 (None)".to_string(),
        )];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);

        // Should match after normalization
        assert_eq!(
            result.matched_tags.len(),
            1,
            "ColorClass should be in matched tags"
        );
        assert_eq!(
            result.value_differences.len(),
            0,
            "No value differences expected"
        );
    }

    #[test]
    fn test_compare_empty_exiftool() {
        let oxidex_tags = vec![TagInfo::new(
            "Make".to_string(),
            "EXIF".to_string(),
            "Canon".to_string(),
        )];
        let exiftool_tags = vec![];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(result.matched_tags.len(), 0);
        assert_eq!(result.missing_in_oxidex.len(), 0);
        assert_eq!(result.extra_in_oxidex.len(), 1);
        assert_eq!(result.coverage_percentage, 0.0);
    }

    #[test]
    fn test_regression_detection() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            // Model is now missing - this is a regression
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        // Previous baseline had both tags matched
        let mut previous = FormatComparison::new("JPEG".to_string(), 2);
        previous.matched_tags = vec!["EXIF:Make".to_string(), "EXIF:Model".to_string()];

        let result =
            ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 2, Some(&previous));

        // Should have 1 regression (Model is missing)
        assert_eq!(result.regressions.len(), 1);
        assert!(result.regressions.contains(&"EXIF:Model".to_string()));

        // Should have 1 matched tag (Make)
        assert_eq!(result.matched_tags.len(), 1);
        assert!(result.matched_tags.contains(&"EXIF:Make".to_string()));

        // Model should be in missing_in_oxidex
        assert_eq!(result.missing_in_oxidex.len(), 1);
        assert_eq!(result.missing_in_oxidex[0].name, "Model");
    }

    #[test]
    fn test_regression_detection_no_previous() {
        let oxidex_tags = vec![TagInfo::new(
            "Make".to_string(),
            "EXIF".to_string(),
            "Canon".to_string(),
        )];
        let exiftool_tags = vec![TagInfo::new(
            "Make".to_string(),
            "EXIF".to_string(),
            "Canon".to_string(),
        )];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);

        // No regressions when there's no previous baseline
        assert_eq!(result.regressions.len(), 0);
    }

    #[test]
    fn test_regression_detection_no_regressions() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        // Previous baseline had only one tag
        let mut previous = FormatComparison::new("JPEG".to_string(), 1);
        previous.matched_tags = vec!["EXIF:Make".to_string()];

        let result =
            ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, Some(&previous));

        // No regressions - we still have Make, and we added Model
        assert_eq!(result.regressions.len(), 0);
        assert_eq!(result.matched_tags.len(), 2);
    }

    #[test]
    fn test_value_difference_detection() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new(
                "Model".to_string(),
                "EXIF".to_string(),
                "EOS 5D".to_string(),
            ), // Different value
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new(
                "Model".to_string(),
                "EXIF".to_string(),
                "5D Mark II".to_string(),
            ), // Different value
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);

        // Make should match perfectly
        assert_eq!(result.matched_tags.len(), 1);
        assert!(result.matched_tags.contains(&"EXIF:Make".to_string()));

        // Model should have value difference
        assert_eq!(result.value_differences.len(), 1);
        assert_eq!(result.value_differences[0].tag_key, "EXIF:Model");
        assert_eq!(result.value_differences[0].exiftool_value, "5D Mark II");
        assert_eq!(result.value_differences[0].oxidex_value, "EOS 5D");

        // Nothing should be missing or extra
        assert_eq!(result.missing_in_oxidex.len(), 0);
        assert_eq!(result.extra_in_oxidex.len(), 0);
    }

    #[test]
    fn test_complex_comparison_with_all_categories() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()), // Match
            TagInfo::new(
                "Model".to_string(),
                "EXIF".to_string(),
                "EOS 5D".to_string(),
            ), // Value diff
            TagInfo::new(
                "CustomTag".to_string(),
                "EXIF".to_string(),
                "Custom".to_string(),
            ), // Extra
                                                                                       // DateTime is missing - will be a regression
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new(
                "Model".to_string(),
                "EXIF".to_string(),
                "5D Mark II".to_string(),
            ),
            TagInfo::new(
                "DateTime".to_string(),
                "EXIF".to_string(),
                "2025:12:07 10:00:00".to_string(),
            ),
            TagInfo::new("ISO".to_string(), "EXIF".to_string(), "400".to_string()), // Missing in oxidex
        ];

        // Previous had Make and DateTime
        let mut previous = FormatComparison::new("JPEG".to_string(), 1);
        previous.matched_tags = vec!["EXIF:Make".to_string(), "EXIF:DateTime".to_string()];

        let result =
            ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, Some(&previous));

        // Matched: Make
        assert_eq!(result.matched_tags.len(), 1);
        assert!(result.matched_tags.contains(&"EXIF:Make".to_string()));

        // Value differences: Model
        assert_eq!(result.value_differences.len(), 1);
        assert_eq!(result.value_differences[0].tag_key, "EXIF:Model");

        // Missing in OxiDex: DateTime, ISO
        assert_eq!(result.missing_in_oxidex.len(), 2);
        let missing_names: Vec<_> = result.missing_in_oxidex.iter().map(|t| &t.name).collect();
        assert!(missing_names.contains(&&"DateTime".to_string()));
        assert!(missing_names.contains(&&"ISO".to_string()));

        // Extra in OxiDex: CustomTag
        assert_eq!(result.extra_in_oxidex.len(), 1);
        assert_eq!(result.extra_in_oxidex[0].name, "CustomTag");

        // Regressions: DateTime (was in previous, not in current matched)
        assert_eq!(result.regressions.len(), 1);
        assert!(result.regressions.contains(&"EXIF:DateTime".to_string()));

        // Coverage: 1 matched out of 4 total = 25%
        assert_eq!(result.coverage_percentage, 25.0);
    }

    #[test]
    fn test_duplicate_emission_same_file_same_key_is_flagged() {
        // Spec M3: two oxidex TagInfo entries sharing (source_file, key)
        // -- the deterministic double-emission gate.
        let oxidex_tags = vec![
            TagInfo::new(
                "AELButton".to_string(),
                "MakerNotes".to_string(),
                "1".to_string(),
            )
            .with_source_file("canon.jpg".to_string()),
            TagInfo::new(
                "AELButton".to_string(),
                "MakerNotes".to_string(),
                "2".to_string(),
            )
            .with_source_file("canon.jpg".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new(
                "AELButton".to_string(),
                "MakerNotes".to_string(),
                "1".to_string(),
            )
            .with_source_file("canon.jpg".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(
            result.duplicate_emissions,
            vec!["MakerNotes:AELButton".to_string()]
        );
    }

    #[test]
    fn test_same_key_different_files_is_not_a_duplicate_emission() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string())
                .with_source_file("a.jpg".to_string()),
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string())
                .with_source_file("b.jpg".to_string()),
        ];
        let exiftool_tags = vec![];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 2, None);
        assert!(result.duplicate_emissions.is_empty());
    }

    #[test]
    fn test_same_key_same_value_same_file_is_not_a_duplicate_emission() {
        // Real-world case: a camera writes the same tag ID (e.g. Padding,
        // 0xEA1C) into both IFD0 and the ExifIFD with an identical value.
        // oxidex correctly keeps these as distinct "IFD0:Padding" /
        // "ExifIFD:Padding" raw keys; this tool's own ExifTool-style
        // group collapse merges them onto one displayed key downstream,
        // but that's expected redundancy, not a genuine double-emission
        // bug -- must not be flagged.
        let oxidex_tags = vec![
            TagInfo::new("Padding".to_string(), "EXIF".to_string(), "0".to_string())
                .with_source_file("canon.jpg".to_string()),
            TagInfo::new("Padding".to_string(), "EXIF".to_string(), "0".to_string())
                .with_source_file("canon.jpg".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new("Padding".to_string(), "EXIF".to_string(), "0".to_string())
                .with_source_file("canon.jpg".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert!(result.duplicate_emissions.is_empty());
    }

    #[test]
    fn test_no_duplicate_emissions_in_the_ordinary_case() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string())
                .with_source_file("a.jpg".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string())
                .with_source_file("a.jpg".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert!(result.duplicate_emissions.is_empty());
    }
}
