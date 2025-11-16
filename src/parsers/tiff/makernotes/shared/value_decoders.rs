/// Decode APEX exposure time value to human-readable string
///
/// Used by: Canon, Nikon, Sony, Pentax
/// Formula: exposure_time = 1 / (2^value)
pub fn decode_exposure_time(apex_value: i32) -> String {
    if apex_value == 0 {
        return "1 s".to_string();
    }

    let divisor = 2_f64.powi(apex_value);
    format!("1/{} s", divisor.round() as i32)
}

/// Decode APEX aperture value to f-number
///
/// Used by: Canon, Nikon, Sony, Olympus, Fuji
/// Formula: f_number = sqrt(2^value)
pub fn decode_aperture(apex_value: i32) -> String {
    let f_number = 2_f64.powf(apex_value as f64 / 2.0);
    format!("f/{:.1}", f_number)
}

/// Decode ISO value from various encoding schemes
///
/// Used by: All manufacturers (different encodings)
pub fn decode_iso(value: i32) -> String {
    // Common encodings:
    // - Direct value (100, 200, 400, etc.)
    // - Log2 encoding (value = log2(ISO))
    // - Manufacturer-specific offsets

    if value < 16 {
        // Likely log2 encoding
        let iso = 2_i32.pow(value as u32);
        format!("ISO {}", iso)
    } else {
        // Direct value
        format!("ISO {}", value)
    }
}

/// Decode focal length from numerator/denominator
///
/// Used by: All manufacturers
pub fn decode_focal_length(numerator: i32, denominator: i32) -> String {
    if denominator == 0 {
        return "Unknown".to_string();
    }

    let focal_length = numerator as f64 / denominator as f64;
    format!("{:.1} mm", focal_length)
}

/// Decode color temperature in Kelvin
///
/// Used by: Canon, Nikon, Sony (white balance)
pub fn decode_temperature_kelvin(kelvin: i32) -> String {
    format!("{} K", kelvin)
}

/// Decode GPS coordinate from degrees, minutes, seconds
///
/// Used by: All manufacturers with GPS
pub fn decode_gps_coord(degrees: u32, minutes: u32, seconds: u32) -> f64 {
    degrees as f64 + (minutes as f64 / 60.0) + (seconds as f64 / 3600.0)
}

/// Decode Unix timestamp to ISO 8601 string
///
/// Used by: Apple, Google, Samsung (smartphone metadata)
pub fn decode_timestamp(unix_timestamp: u32) -> String {
    // Convert Unix timestamp to human-readable format
    // For now, return as-is; can enhance with chrono crate later
    format!("Timestamp: {}", unix_timestamp)
}

/// Decode flash mode from common values
///
/// Used by: Most manufacturers
pub fn decode_flash_mode(value: u16) -> &'static str {
    match value {
        0 => "No Flash",
        1 => "Flash Fired",
        5 => "Flash Fired, Return not detected",
        7 => "Flash Fired, Return detected",
        9 => "Flash Fired, Compulsory",
        13 => "Flash Fired, Compulsory, Return not detected",
        15 => "Flash Fired, Compulsory, Return detected",
        16 => "No Flash, Compulsory",
        24 => "No Flash, Auto",
        25 => "Flash Fired, Auto",
        29 => "Flash Fired, Auto, Return not detected",
        31 => "Flash Fired, Auto, Return detected",
        32 => "No Flash Available",
        _ => "Unknown Flash Mode",
    }
}

/// Decode white balance from common values
///
/// Used by: Most manufacturers
pub fn decode_white_balance(value: u16) -> &'static str {
    match value {
        0 => "Auto",
        1 => "Daylight",
        2 => "Cloudy",
        3 => "Tungsten",
        4 => "Fluorescent",
        5 => "Flash",
        6 => "Custom",
        7 => "Shade",
        8 => "Kelvin",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_exposure_time() {
        assert_eq!(decode_exposure_time(0), "1 s");
        assert_eq!(decode_exposure_time(3), "1/8 s");
        assert_eq!(decode_exposure_time(10), "1/1024 s");
    }

    #[test]
    fn test_decode_aperture() {
        assert_eq!(decode_aperture(1), "f/1.4");
        assert_eq!(decode_aperture(2), "f/2.0");
        assert_eq!(decode_aperture(4), "f/4.0");
    }

    #[test]
    fn test_decode_flash_mode() {
        assert_eq!(decode_flash_mode(0), "No Flash");
        assert_eq!(decode_flash_mode(1), "Flash Fired");
        assert_eq!(decode_flash_mode(25), "Flash Fired, Auto");
    }
}
