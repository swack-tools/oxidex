//! Minolta Lens Database
//!
//! Database of Minolta lens IDs and their corresponding names.
//! Minolta (later Konica Minolta) used numeric lens IDs in their MakerNote data.
//!
//! ## Coverage
//! - Classic Minolta Maxxum/Dynax AF lenses (1985-2006)
//! - Popular prime and zoom lenses
//! - ~30 commonly used lenses

#![allow(dead_code)]

/// Lookup Minolta lens name by lens ID
///
/// # Arguments
/// * `lens_id` - Minolta lens ID from MakerNote
///
/// # Returns
/// Lens name if found in database, None otherwise
pub fn lookup_minolta_lens(lens_id: u16) -> Option<String> {
    let lens_name = match lens_id {
        // Classic AF Primes
        0x0100 => "AF 50mm f/1.4",
        0x0101 => "AF 50mm f/1.7",
        0x0102 => "AF 50mm f/2.8 Macro",
        0x0110 => "AF 28mm f/2.8",
        0x0111 => "AF 35mm f/1.4 G",
        0x0112 => "AF 35mm f/2.0",
        0x0120 => "AF 85mm f/1.4 G",
        0x0121 => "AF 85mm f/1.4 G(D)",
        0x0122 => "AF 100mm f/2.0",
        0x0123 => "AF 100mm f/2.8 Macro",
        0x0124 => "AF 100mm f/2.8 Macro (D)",

        // Wide Angle Primes
        0x0130 => "AF 20mm f/2.8",
        0x0131 => "AF 24mm f/2.8",

        // Telephoto Primes
        0x0140 => "AF 135mm f/2.8",
        0x0141 => "AF 200mm f/2.8 APO G",
        0x0142 => "AF 300mm f/2.8 APO G",
        0x0143 => "AF 300mm f/4.0 APO G",
        0x0144 => "AF 600mm f/4.0 APO G",

        // Standard Zooms
        0x0200 => "AF 28-70mm f/2.8 G",
        0x0201 => "AF 28-80mm f/3.5-5.6",
        0x0202 => "AF 28-85mm f/3.5-4.5",
        0x0203 => "AF 35-70mm f/4.0",
        0x0204 => "AF 35-105mm f/3.5-4.5",

        // Telephoto Zooms
        0x0210 => "AF 70-210mm f/4.0",
        0x0211 => "AF 75-300mm f/4.5-5.6",
        0x0212 => "AF 80-200mm f/2.8 APO G",
        0x0213 => "AF 100-300mm f/4.5-5.6 APO",

        // Wide Angle Zooms
        0x0220 => "AF 17-35mm f/3.5 G",
        0x0221 => "AF 20-35mm f/3.5-4.5",
        0x0222 => "AF 24-50mm f/4.0",

        // Special Purpose
        0x0300 => "AF 16mm f/2.8 Fisheye",
        0x0301 => "AF 50mm f/3.5 Macro",

        _ => return None,
    };

    Some(lens_name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_prime_lenses() {
        assert_eq!(
            lookup_minolta_lens(0x0100),
            Some("AF 50mm f/1.4".to_string())
        );
        assert_eq!(
            lookup_minolta_lens(0x0101),
            Some("AF 50mm f/1.7".to_string())
        );
    }

    #[test]
    fn test_macro_lenses() {
        assert_eq!(
            lookup_minolta_lens(0x0102),
            Some("AF 50mm f/2.8 Macro".to_string())
        );
        assert_eq!(
            lookup_minolta_lens(0x0123),
            Some("AF 100mm f/2.8 Macro".to_string())
        );
    }

    #[test]
    fn test_g_series_lenses() {
        assert_eq!(
            lookup_minolta_lens(0x0111),
            Some("AF 35mm f/1.4 G".to_string())
        );
        assert_eq!(
            lookup_minolta_lens(0x0120),
            Some("AF 85mm f/1.4 G".to_string())
        );
    }

    #[test]
    fn test_zoom_lenses() {
        assert_eq!(
            lookup_minolta_lens(0x0200),
            Some("AF 28-70mm f/2.8 G".to_string())
        );
        assert_eq!(
            lookup_minolta_lens(0x0212),
            Some("AF 80-200mm f/2.8 APO G".to_string())
        );
    }

    #[test]
    fn test_unknown_lens() {
        assert_eq!(lookup_minolta_lens(0xFFFF), None);
    }
}
