//! Olympus lens database for LensID to lens name mapping
//!
//! Based on ExifTool's Olympus.pm lens database, covering both:
//! - Four Thirds (4/3): Zuiko Digital lenses for E-series DSLRs
//! - Micro Four Thirds (M.Zuiko): Current mirrorless system (OM-D, PEN)
//! - Olympus Pro series and Premium primes
//!
//! Olympus uses both legacy lens IDs and newer hex-based IDs for M.Zuiko lenses

/// Looks up a lens name from an Olympus lens ID
///
/// # Arguments
/// * `lens_id` - The lens ID from LensType tag (can be decimal or hex format)
///
/// # Returns
/// * `Some(String)` - The lens model name if found
/// * `None` - If lens ID is not in database
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    OLYMPUS_LENS_DATABASE.get(&lens_id).map(|s| s.to_string())
}

/// Converts hex string lens ID to numeric value for lookup
///
/// Olympus sometimes encodes lens IDs as hex strings (e.g., "0x0201")
pub fn parse_hex_lens_id(hex_str: &str) -> Option<u16> {
    if hex_str.starts_with("0x") || hex_str.starts_with("0X") {
        u16::from_str_radix(&hex_str[2..], 16).ok()
    } else {
        hex_str.parse::<u16>().ok()
    }
}

use once_cell::sync::Lazy;
use std::collections::HashMap;

static OLYMPUS_LENS_DATABASE: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // ===== Four Thirds (4/3) Zuiko Digital Lenses =====
    // Legacy E-series DSLR lenses (E-1, E-3, E-5, E-300, E-500, etc.)

    db.insert(0, "None");
    db.insert(1, "Olympus Zuiko Digital ED 50mm f/2.0 Macro");
    db.insert(2, "Olympus Zuiko Digital 40-150mm f/3.5-4.5");
    db.insert(3, "Olympus Zuiko Digital ED 300mm f/2.8");
    db.insert(4, "Olympus Zuiko Digital 14-54mm f/2.8-3.5");
    db.insert(5, "Olympus Zuiko Digital ED 50-200mm f/2.8-3.5");
    db.insert(6, "Olympus Zuiko Digital ED 7-14mm f/4.0");
    db.insert(7, "Olympus Zuiko Digital 11-22mm f/2.8-3.5");
    db.insert(8, "Olympus Zuiko Digital ED 50-200mm f/2.8-3.5 SWD");
    db.insert(9, "Olympus Zuiko Digital ED 12-60mm f/2.8-4.0 SWD");
    db.insert(10, "Olympus Zuiko Digital ED 14-35mm f/2.0 SWD");

    db.insert(11, "Olympus Zuiko Digital 25mm f/2.8");
    db.insert(12, "Olympus Zuiko Digital ED 9-18mm f/4.0-5.6");
    db.insert(13, "Olympus Zuiko Digital 14-45mm f/3.5-5.6");
    db.insert(14, "Olympus Zuiko Digital 35mm f/3.5 Macro");
    db.insert(15, "Olympus Zuiko Digital ED 14-42mm f/3.5-5.6");
    db.insert(16, "Olympus Zuiko Digital ED 40-150mm f/4.0-5.6");
    db.insert(17, "Olympus Zuiko Digital ED 70-300mm f/4.0-5.6");
    db.insert(18, "Olympus Zuiko Digital ED 18-180mm f/3.5-6.3");
    db.insert(19, "Olympus Zuiko Digital ED 100-400mm f/5.0-6.3");

    // ===== Micro Four Thirds (M.Zuiko) Standard Zoom Lenses =====
    // Modern mirrorless system (OM-D, PEN, OM-1 series)

    db.insert(32, "M.Zuiko Digital ED 14-42mm f/3.5-5.6 L");
    db.insert(33, "M.Zuiko Digital 17mm f/2.8 Pancake");
    db.insert(34, "M.Zuiko Digital ED 14-150mm f/4.0-5.6");
    db.insert(35, "M.Zuiko Digital ED 9-18mm f/4.0-5.6");
    db.insert(36, "M.Zuiko Digital ED 14-42mm f/3.5-5.6");
    db.insert(37, "M.Zuiko Digital ED 40-150mm f/4.0-5.6");
    db.insert(38, "M.Zuiko Digital ED 75-300mm f/4.8-6.7");
    db.insert(39, "M.Zuiko Digital 14-42mm f/3.5-5.6 II");
    db.insert(40, "M.Zuiko Digital ED 12-50mm f/3.5-6.3 EZ");
    db.insert(41, "M.Zuiko Digital 45mm f/1.8");

    db.insert(42, "M.Zuiko Digital ED 60mm f/2.8 Macro");
    db.insert(43, "M.Zuiko Digital ED 14-42mm f/3.5-5.6 IIR");
    db.insert(44, "M.Zuiko Digital ED 40-150mm f/4.0-5.6 R");
    db.insert(45, "M.Zuiko Digital ED 75mm f/1.8");
    db.insert(46, "M.Zuiko Digital 17mm f/1.8");
    db.insert(47, "M.Zuiko Digital 25mm f/1.8");
    db.insert(48, "M.Zuiko Digital ED 12-40mm f/2.8 PRO");
    db.insert(49, "M.Zuiko Digital ED 40-150mm f/2.8 PRO");
    db.insert(50, "M.Zuiko Digital ED 14-42mm f/3.5-5.6 EZ");

    db.insert(51, "M.Zuiko Digital ED 7-14mm f/2.8 PRO");
    db.insert(52, "M.Zuiko Digital ED 300mm f/4.0 IS PRO");
    db.insert(53, "M.Zuiko Digital ED 8mm f/1.8 Fisheye PRO");
    db.insert(54, "M.Zuiko Digital ED 12-50mm f/3.5-6.3 II EZ");
    db.insert(55, "M.Zuiko Digital ED 40-150mm f/4.0-5.6 R II");
    db.insert(56, "M.Zuiko Digital ED 14-150mm f/4.0-5.6 II");
    db.insert(57, "M.Zuiko Digital ED 12-200mm f/3.5-6.3");
    db.insert(58, "M.Zuiko Digital ED 75-300mm f/4.8-6.7 II");

    // ===== M.Zuiko PRO Series (Professional Grade) =====

    db.insert(64, "M.Zuiko Digital ED 12-100mm f/4.0 IS PRO");
    db.insert(65, "M.Zuiko Digital ED 25mm f/1.2 PRO");
    db.insert(66, "M.Zuiko Digital ED 17mm f/1.2 PRO");
    db.insert(67, "M.Zuiko Digital ED 45mm f/1.2 PRO");
    db.insert(68, "M.Zuiko Digital ED 100-400mm f/5.0-6.3 IS");
    db.insert(69, "M.Zuiko Digital ED 8-25mm f/4.0 PRO");
    db.insert(70, "M.Zuiko Digital ED 150-400mm f/4.5 TC1.25x IS PRO");
    db.insert(71, "M.Zuiko Digital ED 12-45mm f/4.0 PRO");
    db.insert(72, "M.Zuiko Digital ED 20mm f/1.4 PRO");
    db.insert(73, "M.Zuiko Digital ED 40-150mm f/4.0 PRO");

    // ===== M.Zuiko Premium Primes =====

    db.insert(80, "M.Zuiko Digital ED 30mm f/3.5 Macro");
    db.insert(81, "M.Zuiko Digital 9mm f/8.0 Fisheye Body Cap Lens");
    db.insert(82, "M.Zuiko Digital 15mm f/8.0 Body Cap Lens");
    db.insert(83, "M.Zuiko Digital ED 12mm f/2.0");
    db.insert(84, "M.Zuiko Digital ED 30mm f/3.5 Macro ED");

    // ===== Hex-encoded M.Zuiko IDs (newer cameras) =====
    // These are the same lenses but with hex-based ID encoding

    db.insert(0x0101, "M.Zuiko Digital 14-42mm f/3.5-5.6 II R");
    db.insert(0x0201, "M.Zuiko Digital ED 12-50mm f/3.5-6.3 EZ");
    db.insert(0x0202, "M.Zuiko Digital ED 40-150mm f/4.0-5.6 R");
    db.insert(0x0203, "M.Zuiko Digital ED 75mm f/1.8");
    db.insert(0x0204, "M.Zuiko Digital 17mm f/1.8");
    db.insert(0x0205, "M.Zuiko Digital 25mm f/1.8");
    db.insert(0x0206, "M.Zuiko Digital ED 12-40mm f/2.8 PRO");
    db.insert(0x0207, "M.Zuiko Digital ED 40-150mm f/2.8 PRO");
    db.insert(0x0208, "M.Zuiko Digital ED 14-42mm f/3.5-5.6 EZ");
    db.insert(0x0209, "M.Zuiko Digital ED 7-14mm f/2.8 PRO");

    db.insert(0x020A, "M.Zuiko Digital ED 300mm f/4.0 IS PRO");
    db.insert(0x020B, "M.Zuiko Digital ED 8mm f/1.8 Fisheye PRO");
    db.insert(0x020C, "M.Zuiko Digital ED 12-50mm f/3.5-6.3 II EZ");
    db.insert(0x020D, "M.Zuiko Digital ED 14-150mm f/4.0-5.6 II");
    db.insert(0x020E, "M.Zuiko Digital ED 12-200mm f/3.5-6.3");
    db.insert(0x020F, "M.Zuiko Digital ED 75-300mm f/4.8-6.7 II");
    db.insert(0x0210, "M.Zuiko Digital ED 12-100mm f/4.0 IS PRO");
    db.insert(0x0211, "M.Zuiko Digital ED 30mm f/3.5 Macro");
    db.insert(0x0212, "M.Zuiko Digital ED 25mm f/1.2 PRO");
    db.insert(0x0213, "M.Zuiko Digital ED 17mm f/1.2 PRO");

    db.insert(0x0214, "M.Zuiko Digital ED 45mm f/1.2 PRO");
    db.insert(0x0215, "M.Zuiko Digital ED 100-400mm f/5.0-6.3 IS");
    db.insert(0x0216, "M.Zuiko Digital ED 8-25mm f/4.0 PRO");
    db.insert(0x0217, "M.Zuiko Digital ED 150-400mm f/4.5 TC1.25x IS PRO");
    db.insert(0x0218, "M.Zuiko Digital ED 12-45mm f/4.0 PRO");
    db.insert(0x0219, "M.Zuiko Digital ED 20mm f/1.4 PRO");
    db.insert(0x021A, "M.Zuiko Digital ED 40-150mm f/4.0 PRO");
    db.insert(0x021B, "M.Zuiko Digital ED 90mm f/3.5 Macro IS PRO");

    // ===== Third-Party Lenses for Micro Four Thirds =====

    db.insert(0x0301, "Sigma 30mm f/2.8 DN Art");
    db.insert(0x0302, "Sigma 19mm f/2.8 DN Art");
    db.insert(0x0303, "Sigma 60mm f/2.8 DN Art");
    db.insert(0x0304, "Panasonic Lumix G 20mm f/1.7 ASPH");
    db.insert(0x0305, "Panasonic Leica DG Summilux 15mm f/1.7 ASPH");

    db
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_four_thirds_lens() {
        // Zuiko Digital ED 12-60mm f/2.8-4.0 SWD
        let result = lookup_lens_name(9);
        assert_eq!(
            result,
            Some("Olympus Zuiko Digital ED 12-60mm f/2.8-4.0 SWD".to_string())
        );
    }

    #[test]
    fn test_lookup_mzuiko_standard_lens() {
        // M.Zuiko Digital 45mm f/1.8
        let result = lookup_lens_name(41);
        assert_eq!(result, Some("M.Zuiko Digital 45mm f/1.8".to_string()));
    }

    #[test]
    fn test_lookup_mzuiko_pro_lens() {
        // M.Zuiko Digital ED 12-40mm f/2.8 PRO
        let result = lookup_lens_name(48);
        assert_eq!(
            result,
            Some("M.Zuiko Digital ED 12-40mm f/2.8 PRO".to_string())
        );
    }

    #[test]
    fn test_lookup_hex_encoded_lens() {
        // M.Zuiko Digital ED 12-100mm f/4.0 IS PRO (hex ID)
        let result = lookup_lens_name(0x0210);
        assert_eq!(
            result,
            Some("M.Zuiko Digital ED 12-100mm f/4.0 IS PRO".to_string())
        );
    }

    #[test]
    fn test_lookup_premium_prime() {
        // M.Zuiko Digital ED 25mm f/1.2 PRO
        let result = lookup_lens_name(65);
        assert_eq!(
            result,
            Some("M.Zuiko Digital ED 25mm f/1.2 PRO".to_string())
        );
    }

    #[test]
    fn test_lookup_unknown_lens() {
        let result = lookup_lens_name(9999);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_hex_lens_id() {
        assert_eq!(parse_hex_lens_id("0x0210"), Some(0x0210));
        assert_eq!(parse_hex_lens_id("0X0210"), Some(0x0210));
        assert_eq!(parse_hex_lens_id("48"), Some(48));
        assert_eq!(parse_hex_lens_id("invalid"), None);
    }

    #[test]
    fn test_database_size() {
        // Should have 70+ lens entries
        assert!(
            OLYMPUS_LENS_DATABASE.len() >= 70,
            "Expected at least 70 lens entries, found {}",
            OLYMPUS_LENS_DATABASE.len()
        );
    }

    #[test]
    fn test_lookup_fisheye() {
        // M.Zuiko Digital ED 8mm f/1.8 Fisheye PRO
        let result = lookup_lens_name(53);
        assert_eq!(
            result,
            Some("M.Zuiko Digital ED 8mm f/1.8 Fisheye PRO".to_string())
        );
    }

    #[test]
    fn test_lookup_macro() {
        // M.Zuiko Digital ED 60mm f/2.8 Macro
        let result = lookup_lens_name(42);
        assert_eq!(
            result,
            Some("M.Zuiko Digital ED 60mm f/2.8 Macro".to_string())
        );
    }
}
