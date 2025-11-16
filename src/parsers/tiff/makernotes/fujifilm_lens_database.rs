//! Fujifilm Lens Database
//!
//! This module provides lens name lookups for Fujifilm cameras.
//! Supports both X-mount (X-series mirrorless) and GFX-mount (medium format) lenses.
//!
//! The lens ID values are based on ExifTool's Fujifilm.pm module.

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Looks up a Fujifilm lens name by its lens ID.
///
/// # Parameters
/// - `lens_id`: The lens ID value from Fujifilm MakerNotes
///
/// # Returns
/// - `Some(String)`: The lens name if found in the database
/// - `None`: If the lens ID is not recognized
///
/// # Example
/// ```
/// use exiftool_rs::parsers::tiff::makernotes::fujifilm_lens_database::lookup_lens_name;
///
/// let lens_name = lookup_lens_name(35);
/// assert_eq!(lens_name, Some("XF 56mm f/1.2 R".to_string()));
/// ```
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    FUJIFILM_LENS_DATABASE.get(&lens_id).map(|s| s.to_string())
}

/// Database mapping Fujifilm lens IDs to lens model names
///
/// This database includes:
/// - XF-mount lenses (X-series mirrorless cameras)
/// - XC-mount lenses (budget X-series lenses)
/// - GF-mount lenses (GFX medium format cameras)
///
/// Lens IDs are manufacturer-specific values stored in Fujifilm MakerNotes.
static FUJIFILM_LENS_DATABASE: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // ===== XF Lenses (X-Mount Prime Lenses) =====

    // Fast primes
    db.insert(23, "XF 14mm f/2.8 R");
    db.insert(33, "XF 16mm f/1.4 R WR");
    db.insert(26, "XF 18mm f/2 R");
    db.insert(147, "XF 23mm f/1.4 R");
    db.insert(256, "XF 23mm f/1.4 R LM WR");
    db.insert(27, "XF 27mm f/2.8");
    db.insert(269, "XF 27mm f/2.8 R WR");
    db.insert(35, "XF 35mm f/1.4 R");
    db.insert(163, "XF 35mm f/2 R WR");
    db.insert(189, "XF 50mm f/1.0 R WR");
    db.insert(148, "XF 56mm f/1.2 R");
    db.insert(235, "XF 56mm f/1.2 R APD");
    db.insert(60, "XF 60mm f/2.4 R Macro");
    db.insert(80, "XF 80mm f/2.8 R LM OIS WR Macro");
    db.insert(270, "XF 90mm f/2 R LM WR");

    // Wide angle primes
    db.insert(257, "XF 16mm f/2.8 R WR");
    db.insert(258, "XF 23mm f/2 R WR");
    db.insert(259, "XF 33mm f/1.4 R LM WR");

    // Telephoto primes
    db.insert(260, "XF 50mm f/2 R WR");

    // ===== XF Lenses (X-Mount Zoom Lenses) =====

    // Standard zooms
    db.insert(1, "XF 18-55mm f/2.8-4 R LM OIS");
    db.insert(29, "XF 16-55mm f/2.8 R LM WR");
    db.insert(4095, "XF 18-120mm f/4 R LM OIS WR");
    db.insert(271, "XF 16-80mm f/4 R OIS WR");

    // Telephoto zooms
    db.insert(6, "XF 55-200mm f/3.5-4.8 R LM OIS");
    db.insert(20, "XF 50-140mm f/2.8 R LM OIS WR");
    db.insert(261, "XF 70-300mm f/4-5.6 R LM OIS WR");
    db.insert(272, "XF 100-400mm f/4.5-5.6 R LM OIS WR");
    db.insert(273, "XF 150-600mm f/5.6-8 R LM OIS WR");

    // Wide angle zooms
    db.insert(17, "XF 10-24mm f/4 R OIS");
    db.insert(274, "XF 8-16mm f/2.8 R LM WR");

    // ===== XC Lenses (Budget X-Mount Lenses) =====

    db.insert(11, "XC 16-50mm f/3.5-5.6 OIS");
    db.insert(275, "XC 16-50mm f/3.5-5.6 OIS II");
    db.insert(12, "XC 50-230mm f/4.5-6.7 OIS");
    db.insert(276, "XC 50-230mm f/4.5-6.7 OIS II");
    db.insert(277, "XC 15-45mm f/3.5-5.6 OIS PZ");
    db.insert(278, "XC 35mm f/2");

    // ===== GF Lenses (GFX Medium Format Lenses) =====

    // GF primes
    db.insert(279, "GF 23mm f/4 R LM WR");
    db.insert(280, "GF 32-64mm f/4 R LM WR");
    db.insert(45, "GF 45mm f/2.8 R WR");
    db.insert(281, "GF 50mm f/3.5 R LM WR");
    db.insert(63, "GF 63mm f/2.8 R WR");
    db.insert(110, "GF 110mm f/2 R LM WR");
    db.insert(282, "GF 120mm f/4 Macro R LM OIS WR");
    db.insert(250, "GF 250mm f/4 R LM OIS WR");

    // GF zooms
    db.insert(283, "GF 20-35mm f/4 R WR");
    db.insert(284, "GF 35-70mm f/4.5-5.6 WR");
    db.insert(100, "GF 100-200mm f/5.6 R LM OIS WR");
    db.insert(285, "GF 45-100mm f/4 R LM OIS WR");

    // ===== Teleconverters =====

    db.insert(286, "GF 1.4X TC WR");
    db.insert(287, "GF 2X TC WR");
    db.insert(288, "XF 1.4X TC WR");
    db.insert(289, "XF 2X TC WR");

    // ===== Additional Recent Lenses =====

    db.insert(290, "XF 18mm f/1.4 R LM WR");
    db.insert(291, "XF 33mm f/1.4 R LM WR");
    db.insert(292, "XF 30mm f/2.8 R LM WR Macro");
    db.insert(293, "GF 80mm f/1.7 R WR");
    db.insert(294, "GF 30mm f/3.5 R WR");
    db.insert(295, "GF 30mm f/5.6 T/S");
    db.insert(296, "XF 70-300mm f/4-5.6 R LM OIS WR");
    db.insert(297, "GF 55mm f/1.7 R WR");

    db
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xf_prime_lens_lookup() {
        // Test popular XF primes
        assert_eq!(lookup_lens_name(35), Some("XF 35mm f/1.4 R".to_string()));
        assert_eq!(lookup_lens_name(148), Some("XF 56mm f/1.2 R".to_string()));
        assert_eq!(
            lookup_lens_name(270),
            Some("XF 90mm f/2 R LM WR".to_string())
        );
    }

    #[test]
    fn test_xf_zoom_lens_lookup() {
        // Test popular XF zooms
        assert_eq!(
            lookup_lens_name(1),
            Some("XF 18-55mm f/2.8-4 R LM OIS".to_string())
        );
        assert_eq!(
            lookup_lens_name(20),
            Some("XF 50-140mm f/2.8 R LM OIS WR".to_string())
        );
        assert_eq!(
            lookup_lens_name(272),
            Some("XF 100-400mm f/4.5-5.6 R LM OIS WR".to_string())
        );
    }

    #[test]
    fn test_xc_lens_lookup() {
        // Test budget XC lenses
        assert_eq!(
            lookup_lens_name(11),
            Some("XC 16-50mm f/3.5-5.6 OIS".to_string())
        );
        assert_eq!(
            lookup_lens_name(277),
            Some("XC 15-45mm f/3.5-5.6 OIS PZ".to_string())
        );
    }

    #[test]
    fn test_gf_lens_lookup() {
        // Test GFX medium format lenses
        assert_eq!(lookup_lens_name(63), Some("GF 63mm f/2.8 R WR".to_string()));
        assert_eq!(
            lookup_lens_name(110),
            Some("GF 110mm f/2 R LM WR".to_string())
        );
        assert_eq!(
            lookup_lens_name(100),
            Some("GF 100-200mm f/5.6 R LM OIS WR".to_string())
        );
    }

    #[test]
    fn test_teleconverter_lookup() {
        // Test teleconverters
        assert_eq!(lookup_lens_name(286), Some("GF 1.4X TC WR".to_string()));
        assert_eq!(lookup_lens_name(288), Some("XF 1.4X TC WR".to_string()));
    }

    #[test]
    fn test_unknown_lens() {
        // Test unknown lens ID
        assert_eq!(lookup_lens_name(65000), None);
        assert_eq!(lookup_lens_name(0), None);
    }

    #[test]
    fn test_database_not_empty() {
        // Verify database has reasonable size
        assert!(
            FUJIFILM_LENS_DATABASE.len() >= 60,
            "Database should contain at least 60 lenses"
        );
    }
}
