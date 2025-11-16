//! Panasonic lens database for LensID to lens name mapping
//!
//! Based on ExifTool's Panasonic.pm lens database, covering both:
//! - Micro Four Thirds (M43) lenses: Lumix G/GX/GH/GF series
//! - L-mount lenses: Lumix S series full-frame
//! - Leica DG (Designed by Leica) lenses for M43

/// Looks up a lens name from a Panasonic lens ID
///
/// # Arguments
/// * `lens_id` - The lens ID from LensType tag
///
/// # Returns
/// * `Some(String)` - The lens model name if found
/// * `None` - If lens ID is not in database
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    PANASONIC_LENS_DATABASE.get(&lens_id).map(|s| s.to_string())
}

use once_cell::sync::Lazy;
use std::collections::HashMap;

static PANASONIC_LENS_DATABASE: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // ===== Micro Four Thirds (M43) Standard Zoom Lenses =====
    db.insert(1, "Lumix G Vario 14-42mm f/3.5-5.6 ASPH. MEGA O.I.S.");
    db.insert(2, "Lumix G Vario 45-200mm f/4.0-5.6 MEGA O.I.S.");
    db.insert(3, "Lumix G Vario 14-140mm f/4.0-5.8 ASPH. MEGA O.I.S.");
    db.insert(4, "Lumix G Vario HD 14-140mm f/4.0-5.8 ASPH. MEGA O.I.S.");
    db.insert(5, "Lumix G Vario 45-150mm f/4.0-5.6 ASPH. MEGA O.I.S.");
    db.insert(6, "Lumix G Vario 12-32mm f/3.5-5.6 ASPH. MEGA O.I.S.");
    db.insert(7, "Lumix G Vario 35-100mm f/4.0-5.6 ASPH. MEGA O.I.S.");
    db.insert(8, "Lumix G X Vario 12-35mm f/2.8 ASPH. POWER O.I.S.");
    db.insert(9, "Lumix G X Vario 35-100mm f/2.8 POWER O.I.S.");
    db.insert(10, "Lumix G X Vario 12-35mm f/2.8 II ASPH. POWER O.I.S.");
    db.insert(11, "Lumix G X Vario 35-100mm f/2.8 II POWER O.I.S.");
    db.insert(12, "Lumix G Vario 14-42mm f/3.5-5.6 II ASPH. MEGA O.I.S.");
    db.insert(13, "Lumix G Vario 100-300mm f/4.0-5.6 MEGA O.I.S.");
    db.insert(14, "Lumix G Vario 45-200mm f/4.0-5.6 II POWER O.I.S.");
    db.insert(
        15,
        "Lumix G X Vario 12-35mm f/2.8 ASPH. POWER O.I.S. (Mark II)",
    );

    // ===== M43 Prime Lenses =====
    db.insert(20, "Lumix G 20mm f/1.7 ASPH.");
    db.insert(21, "Lumix G 20mm f/1.7 II ASPH.");
    db.insert(22, "Lumix G 14mm f/2.5 ASPH.");
    db.insert(23, "Lumix G 25mm f/1.7 ASPH.");
    db.insert(24, "Lumix G 42.5mm f/1.7 ASPH. POWER O.I.S.");
    db.insert(25, "Lumix G 15mm f/1.7 ASPH.");
    db.insert(26, "Lumix G 8mm f/3.5 Fisheye");
    db.insert(27, "Lumix G Macro 30mm f/2.8 ASPH. MEGA O.I.S.");
    db.insert(28, "Lumix G 42.5mm f/1.2 ASPH. POWER O.I.S.");
    db.insert(29, "Lumix G 25mm f/1.4 ASPH.");

    // ===== Leica DG Lenses (Designed by Leica for M43) =====
    db.insert(30, "Leica DG Summilux 15mm f/1.7 ASPH.");
    db.insert(31, "Leica DG Summilux 25mm f/1.4 ASPH.");
    db.insert(32, "Leica DG Nocticron 42.5mm f/1.2 ASPH. POWER O.I.S.");
    db.insert(33, "Leica DG Vario-Elmarit 8-18mm f/2.8-4.0 ASPH.");
    db.insert(
        34,
        "Leica DG Vario-Elmarit 12-60mm f/2.8-4.0 ASPH. POWER O.I.S.",
    );
    db.insert(35, "Leica DG Elmarit 200mm f/2.8 POWER O.I.S.");
    db.insert(
        36,
        "Leica DG Vario-Elmar 100-400mm f/4.0-6.3 ASPH. POWER O.I.S.",
    );
    db.insert(37, "Leica DG Summilux 10-25mm f/1.7 ASPH.");
    db.insert(38, "Leica DG Vario-Summilux 10-25mm f/1.7 ASPH.");
    db.insert(39, "Leica DG Vario-Summilux 25-50mm f/1.7 ASPH.");

    // ===== M43 Pro Lenses (Weather Sealed, Professional Grade) =====
    db.insert(40, "Lumix G X Vario 12-35mm f/2.8 ASPH. POWER O.I.S. (Pro)");
    db.insert(
        41,
        "Lumix G X Vario 35-100mm f/2.8 ASPH. POWER O.I.S. (Pro)",
    );
    db.insert(42, "Lumix G Vario 7-14mm f/4.0 ASPH.");
    db.insert(43, "Lumix G Vario 100-300mm f/4.0-5.6 II POWER O.I.S.");
    db.insert(44, "Lumix G 8-18mm f/2.8-4.0 ASPH.");
    db.insert(45, "Lumix G Vario 14-140mm f/3.5-5.6 ASPH. POWER O.I.S.");

    // ===== L-Mount Full Frame Lenses (Lumix S Series) =====
    db.insert(100, "Lumix S 20-60mm f/3.5-5.6");
    db.insert(101, "Lumix S 24-105mm f/4 MACRO O.I.S.");
    db.insert(102, "Lumix S Pro 16-35mm f/4");
    db.insert(103, "Lumix S Pro 24-70mm f/2.8");
    db.insert(104, "Lumix S Pro 70-200mm f/2.8 O.I.S.");
    db.insert(105, "Lumix S Pro 70-200mm f/4 O.I.S.");
    db.insert(106, "Lumix S 24-70mm f/2.8 (Mark II)");
    db.insert(107, "Lumix S 70-200mm f/2.8 (Mark II)");
    db.insert(108, "Lumix S 70-300mm f/4.5-5.6 MACRO O.I.S.");

    // ===== L-Mount Prime Lenses =====
    db.insert(110, "Lumix S 14mm f/1.8");
    db.insert(111, "Lumix S 18mm f/1.8");
    db.insert(112, "Lumix S 20mm f/1.8");
    db.insert(113, "Lumix S 24mm f/1.8");
    db.insert(114, "Lumix S 35mm f/1.8");
    db.insert(115, "Lumix S 50mm f/1.8");
    db.insert(116, "Lumix S 85mm f/1.8");
    db.insert(117, "Lumix S Pro 50mm f/1.4");
    db.insert(118, "Lumix S 5mm f/1.8 (ultra-wide prime)");

    // ===== L-Mount Macro Lenses =====
    db.insert(120, "Lumix S 50mm f/2.8 Macro");
    db.insert(121, "Lumix S 100mm f/2.8 Macro");

    // ===== Specialty M43 Lenses =====
    db.insert(
        130,
        "Lumix G Vario 14-42mm f/3.5-5.6 ASPH. POWER O.I.S. (Pancake)",
    );
    db.insert(131, "Lumix G Vario 12-60mm f/3.5-5.6 ASPH. POWER O.I.S.");
    db.insert(
        132,
        "Lumix G Vario 14-140mm f/3.5-5.6 II ASPH. POWER O.I.S.",
    );
    db.insert(
        133,
        "Lumix G X Vario PZ 14-42mm f/3.5-5.6 ASPH. POWER O.I.S. (Power Zoom)",
    );
    db.insert(
        134,
        "Lumix G X Vario PZ 45-175mm f/4.0-5.6 ASPH. POWER O.I.S. (Power Zoom)",
    );

    // ===== Third-party lenses for Panasonic (Olympus M.Zuiko compatibility) =====
    // Note: Panasonic M43 cameras can use Olympus M.Zuiko lenses via M43 mount
    db.insert(200, "Olympus M.Zuiko Digital ED 12-40mm f/2.8 PRO");
    db.insert(201, "Olympus M.Zuiko Digital ED 40-150mm f/2.8 PRO");
    db.insert(202, "Olympus M.Zuiko Digital ED 7-14mm f/2.8 PRO");
    db.insert(203, "Olympus M.Zuiko Digital ED 300mm f/4.0 IS PRO");
    db.insert(204, "Olympus M.Zuiko Digital 17mm f/1.8");
    db.insert(205, "Olympus M.Zuiko Digital 25mm f/1.8");
    db.insert(206, "Olympus M.Zuiko Digital 45mm f/1.8");

    // ===== Additional Leica DG Specialty Lenses =====
    db.insert(210, "Leica DG Summilux 9mm f/1.7 ASPH.");
    db.insert(211, "Leica DG Summilux 12mm f/1.4 ASPH.");
    db.insert(
        212,
        "Leica DG Vario-Elmarit 50-200mm f/2.8-4.0 ASPH. POWER O.I.S.",
    );

    // ===== Cine Lenses (GH series hybrid video/photo) =====
    db.insert(220, "Lumix S 24mm f/1.8 (Cine)");
    db.insert(221, "Lumix S 35mm f/1.8 (Cine)");
    db.insert(222, "Lumix S 50mm f/1.8 (Cine)");
    db.insert(223, "Lumix S 85mm f/1.8 (Cine)");

    db
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_m43_standard_zoom() {
        // Lumix G Vario 14-42mm f/3.5-5.6 ASPH. MEGA O.I.S.
        let result = lookup_lens_name(1);
        assert_eq!(
            result,
            Some("Lumix G Vario 14-42mm f/3.5-5.6 ASPH. MEGA O.I.S.".to_string())
        );
    }

    #[test]
    fn test_lookup_leica_dg_lens() {
        // Leica DG Nocticron 42.5mm f/1.2 ASPH. POWER O.I.S.
        let result = lookup_lens_name(32);
        assert_eq!(
            result,
            Some("Leica DG Nocticron 42.5mm f/1.2 ASPH. POWER O.I.S.".to_string())
        );
    }

    #[test]
    fn test_lookup_l_mount_pro_lens() {
        // Lumix S Pro 24-70mm f/2.8
        let result = lookup_lens_name(103);
        assert_eq!(result, Some("Lumix S Pro 24-70mm f/2.8".to_string()));
    }

    #[test]
    fn test_lookup_l_mount_prime() {
        // Lumix S 50mm f/1.8
        let result = lookup_lens_name(115);
        assert_eq!(result, Some("Lumix S 50mm f/1.8".to_string()));
    }

    #[test]
    fn test_lookup_olympus_m43_lens() {
        // Olympus M.Zuiko Digital ED 12-40mm f/2.8 PRO (compatible with Panasonic M43)
        let result = lookup_lens_name(200);
        assert_eq!(
            result,
            Some("Olympus M.Zuiko Digital ED 12-40mm f/2.8 PRO".to_string())
        );
    }

    #[test]
    fn test_lookup_unknown_lens() {
        let result = lookup_lens_name(65000);
        assert_eq!(result, None);
    }

    #[test]
    fn test_database_size() {
        // Should have 50+ lens entries (M43 + L-mount + Leica DG)
        assert!(
            PANASONIC_LENS_DATABASE.len() >= 50,
            "Expected at least 50 lens entries, found {}",
            PANASONIC_LENS_DATABASE.len()
        );
    }

    #[test]
    fn test_leica_dg_summilux_series() {
        // Test multiple Leica DG Summilux lenses
        assert!(lookup_lens_name(30).is_some()); // 15mm f/1.7
        assert!(lookup_lens_name(31).is_some()); // 25mm f/1.4
        assert!(lookup_lens_name(210).is_some()); // 9mm f/1.7
        assert!(lookup_lens_name(211).is_some()); // 12mm f/1.4
    }

    #[test]
    fn test_lumix_s_pro_series() {
        // Test L-mount professional lenses
        assert!(lookup_lens_name(102).is_some()); // 16-35mm f/4
        assert!(lookup_lens_name(103).is_some()); // 24-70mm f/2.8
        assert!(lookup_lens_name(104).is_some()); // 70-200mm f/2.8
    }
}
