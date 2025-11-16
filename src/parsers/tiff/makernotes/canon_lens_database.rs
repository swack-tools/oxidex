//! Canon lens database for LensType/LensID to lens name mapping
//!
//! Based on ExifTool's Canon.pm %canonLensTypes hash

/// Looks up a lens name from a Canon lens ID
///
/// # Arguments
/// * `lens_id` - The lens ID from CameraInfo or LensInfo arrays
///
/// # Returns
/// * `Some(String)` - The lens model name if found
/// * `None` - If lens ID is not in database
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    CANON_LENS_DATABASE.get(&lens_id).map(|s| s.to_string())
}

use once_cell::sync::Lazy;
use std::collections::HashMap;

static CANON_LENS_DATABASE: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // Most common Canon EF lenses (sorted by ID)
    db.insert(1, "Canon EF 50mm f/1.8");
    db.insert(2, "Canon EF 28mm f/2.8");
    db.insert(3, "Canon EF 135mm f/2.8 Soft-Focus");
    db.insert(4, "Canon EF 35-70mm f/3.5-4.5");
    db.insert(5, "Canon EF 35-105mm f/3.5-4.5");
    db.insert(6, "Canon EF 75-300mm f/4-5.6");
    db.insert(7, "Canon EF 100-300mm f/5.6L");
    db.insert(8, "Canon EF 100mm f/2.8 Macro");
    db.insert(9, "Canon EF 35mm f/2");
    db.insert(10, "Canon EF 15mm f/2.8 Fisheye");

    db.insert(11, "Canon EF 50-200mm f/3.5-4.5L");
    db.insert(13, "Canon EF 50mm f/1.4");
    db.insert(14, "Canon EF 300mm f/2.8L");
    db.insert(15, "Canon EF 50-200mm f/3.5-4.5");
    db.insert(16, "Canon EF 35-135mm f/3.5-4.5");
    db.insert(17, "Canon EF 35-70mm f/3.5-4.5A");
    db.insert(18, "Canon EF 28-70mm f/3.5-4.5");
    db.insert(20, "Canon EF 100-200mm f/4.5A");
    db.insert(21, "Canon EF 35-135mm f/4-5.6 USM");
    db.insert(22, "Canon EF 80-200mm f/2.8L");

    db.insert(23, "Canon EF 35-105mm f/3.5-4.5 USM");
    db.insert(24, "Canon EF 35-80mm f/4-5.6 Power Zoom");
    db.insert(26, "Canon EF 100-300mm f/5.6L");
    db.insert(27, "Canon EF 100mm f/2");
    db.insert(
        28,
        "Canon EF 14mm f/2.8L or Sigma 14mm f/2.8 EX Aspherical HSM",
    );
    db.insert(29, "Canon EF 200mm f/2.8L");
    db.insert(30, "Canon EF 300mm f/2.8L");
    db.insert(31, "Canon EF 400mm f/2.8L");
    db.insert(32, "Canon EF 500mm f/4.5L");
    db.insert(35, "Canon EF 135mm f/2L");

    db.insert(36, "Canon EF 600mm f/4L");
    db.insert(37, "Canon EF 24-85mm f/3.5-4.5 USM");
    db.insert(38, "Canon EF 300mm f/4L");
    db.insert(39, "Canon EF 400mm f/5.6L");
    db.insert(40, "Canon EF 500mm f/4.5L USM");
    db.insert(41, "Canon EF 100-400mm f/4.5-5.6L IS USM");
    db.insert(42, "Canon EF 70-210mm f/3.5-4.5 USM");
    db.insert(43, "Canon EF 80-200mm f/4.5-5.6 USM");
    db.insert(44, "Canon EF 35-80mm f/4-5.6 USM");
    db.insert(45, "Canon EF 50mm f/1.0L");

    db.insert(48, "Canon EF 50mm f/1.8 II");
    db.insert(49, "Canon EF 28-105mm f/3.5-4.5 USM");
    db.insert(50, "Canon EF 17-40mm f/4L USM");
    db.insert(51, "Canon EF 10-22mm f/3.5-4.5 USM");
    db.insert(124, "Canon MP-E 65mm f/2.8 1-5x Macro Photo");
    db.insert(125, "Canon TS-E 24mm f/3.5L");
    db.insert(126, "Canon TS-E 45mm f/2.8");
    db.insert(127, "Canon TS-E 90mm f/2.8");
    db.insert(129, "Canon EF 300mm f/2.8L");
    db.insert(130, "Canon EF 50mm f/1.0L");

    db.insert(131, "Canon EF 28-80mm f/2.8-4L or Sigma 24-70mm f/2.8 EX");
    db.insert(132, "Canon EF 1200mm f/5.6L");
    db.insert(134, "Canon EF 600mm f/4L IS");
    db.insert(135, "Canon EF 200mm f/1.8L");
    db.insert(136, "Canon EF 300mm f/2.8L");
    db.insert(137, "Canon EF 85mm f/1.2L or Sigma 15mm f/2.8 EX Fisheye");
    db.insert(138, "Canon EF 28-80mm f/2.8-4L");
    db.insert(139, "Canon EF 400mm f/2.8L");
    db.insert(140, "Canon EF 500mm f/4L IS");
    db.insert(
        141,
        "Canon EF 500mm f/4L IS or Sigma 17-35mm f/2.8-4 EX Aspherical",
    );

    db.insert(142, "Canon EF 300mm f/2.8L IS");
    db.insert(143, "Canon EF 500mm f/4L");
    db.insert(149, "Canon EF 100mm f/2");
    db.insert(
        150,
        "Canon EF 14mm f/2.8L or Sigma 20mm f/1.8 EX Aspherical",
    );
    db.insert(151, "Canon EF 200mm f/2.8L");
    db.insert(152, "Canon EF 300mm f/4L IS or Sigma 55-200mm f/4-5.6 DC");
    db.insert(
        153,
        "Canon EF 35-350mm f/3.5-5.6L or Sigma 28-300mm f/3.5-6.3 Macro",
    );
    db.insert(
        154,
        "Canon EF 20mm f/2.8 USM or Tamron AF 28-300mm f/3.5-6.3 XR Di VC",
    );
    db.insert(155, "Canon EF 85mm f/1.8 USM or Sigma 30mm f/1.4 EX DC HSM");
    db.insert(
        156,
        "Canon EF 28-105mm f/3.5-4.5 USM or Tamron AF 90mm f/2.8 Di Macro",
    );

    db.insert(
        160,
        "Canon EF 20-35mm f/3.5-4.5 USM or Tamron AF 19-35mm f/3.5-4.5",
    );
    db.insert(161, "Canon EF 28-70mm f/2.8L or Sigma 24-70mm f/2.8 EX");
    db.insert(162, "Canon EF 200mm f/2.8L");
    db.insert(163, "Canon EF 300mm f/4L");
    db.insert(164, "Canon EF 400mm f/5.6L");
    db.insert(165, "Canon EF 70-200mm f/2.8L");
    db.insert(166, "Canon EF 70-200mm f/2.8L + 1.4x");
    db.insert(167, "Canon EF 70-200mm f/2.8L + 2x");
    db.insert(
        168,
        "Canon EF 28mm f/1.8 USM or Sigma 50-500mm f/4-6.3 APO HSM EX",
    );
    db.insert(
        169,
        "Canon EF 17-35mm f/2.8L or Sigma 18-200mm f/3.5-6.3 DC OS",
    );

    db.insert(170, "Canon EF 200mm f/2.8L II");
    db.insert(171, "Canon EF 300mm f/4L");
    db.insert(
        172,
        "Canon EF 400mm f/5.6L or Sigma 150-600mm f/5-6.3 DG OS HSM | S",
    );
    db.insert(
        173,
        "Canon EF 180mm Macro f/3.5L or Sigma 180mm EX HSM Macro f/3.5",
    );
    db.insert(174, "Canon EF 135mm f/2L or Sigma 28mm f/1.8 DG Macro EX");
    db.insert(175, "Canon EF 400mm f/2.8L");
    db.insert(176, "Canon EF 24-85mm f/3.5-4.5 USM");
    db.insert(177, "Canon EF 300mm f/4L IS");
    db.insert(178, "Canon EF 28-135mm f/3.5-5.6 IS");
    db.insert(179, "Canon EF 24mm f/1.4L");

    // Professional L-series lenses
    db.insert(180, "Canon EF 35mm f/1.4L or Sigma 50mm f/1.4 EX DG HSM");
    db.insert(181, "Canon EF 100-400mm f/4.5-5.6L IS");
    db.insert(182, "Canon EF 70-200mm f/4L");
    db.insert(183, "Canon EF 70-200mm f/4L + 1.4x");
    db.insert(184, "Canon EF 70-200mm f/4L + 2x");
    db.insert(185, "Canon EF 70-200mm f/4L + 2.8x");
    db.insert(186, "Canon EF 70-200mm f/2.8L IS");
    db.insert(187, "Canon EF 70-200mm f/2.8L IS + 1.4x");
    db.insert(188, "Canon EF 70-200mm f/2.8L IS + 2x");
    db.insert(189, "Canon EF 70-200mm f/2.8L IS + 2.8x");

    db.insert(190, "Canon EF 100mm f/2.8 Macro");
    db.insert(191, "Canon EF 400mm f/4 DO IS");
    db.insert(193, "Canon EF 35-80mm f/4-5.6 USM");
    db.insert(194, "Canon EF 80-200mm f/4.5-5.6 USM");
    db.insert(195, "Canon EF 35-105mm f/4.5-5.6 USM");
    db.insert(196, "Canon EF 75-300mm f/4-5.6 IS USM");
    db.insert(197, "Canon EF 75-300mm f/4-5.6 USM");
    db.insert(198, "Canon EF 50mm f/1.4 USM");
    db.insert(199, "Canon EF 28-80mm f/3.5-5.6 USM");
    db.insert(200, "Canon EF 75-300mm f/4-5.6 USM");

    // Modern EF lenses
    db.insert(224, "Canon EF 70-200mm f/2.8L IS II");
    db.insert(225, "Canon EF 70-200mm f/2.8L IS II + 1.4x");
    db.insert(226, "Canon EF 70-200mm f/2.8L IS II + 2x");
    db.insert(
        234,
        "Canon EF 200mm f/2L IS or Sigma 24-105mm f/4 DG OS HSM | A",
    );
    db.insert(235, "Canon EF 800mm f/5.6L IS");
    db.insert(236, "Canon EF 24mm f/1.4L II or Sigma 35mm f/1.4 DG HSM");
    db.insert(237, "Canon EF 70-300mm f/4-5.6L IS USM");
    db.insert(248, "Canon EF 16-35mm f/2.8L II");
    db.insert(251, "Canon EF 300mm f/2.8L IS II");
    db.insert(252, "Canon EF 400mm f/2.8L IS II");

    db.insert(254, "Canon EF 500mm f/4L IS II or EF 24-105mm f/4L IS USM");
    db.insert(255, "Canon EF 600mm f/4L IS II");
    db.insert(368, "Canon EF 24-70mm f/2.8L II USM");
    db.insert(488, "Canon EF 16-35mm f/4L IS USM");
    db.insert(489, "Canon EF 24-105mm f/3.5-5.6 IS STM");

    // STM lenses (budget/consumer)
    db.insert(4142, "Canon EF 24mm f/2.8 IS USM");
    db.insert(4143, "Canon EF 28mm f/2.8 IS USM");
    db.insert(4144, "Canon EF-S 24mm f/2.8 STM");
    db.insert(4145, "Canon EF-M 28mm f/3.5 Macro IS STM");
    db.insert(4146, "Canon EF 24-105mm f/4L IS II USM");
    db.insert(4147, "Canon EF 16-35mm f/2.8L III USM");
    db.insert(4150, "Canon EF 24-70mm f/2.8L III USM");
    db.insert(4152, "Canon EF 100-400mm f/4.5-5.6L IS II USM");
    db.insert(4156, "Canon EF 50mm f/1.8 STM");

    // Canon RF lenses (mirrorless)
    db.insert(61182, "Canon RF 24-105mm f/4L IS USM");
    db.insert(61183, "Canon RF 28-70mm f/2L USM");
    db.insert(61184, "Canon RF 50mm f/1.2L USM");
    db.insert(61185, "Canon RF 24-70mm f/2.8L IS USM");
    db.insert(61186, "Canon RF 15-35mm f/2.8L IS USM");
    db.insert(61187, "Canon RF 70-200mm f/2.8L IS USM");
    db.insert(61188, "Canon RF 85mm f/1.2L USM");
    db.insert(61189, "Canon RF 100-500mm f/4.5-7.1L IS USM");
    db.insert(61190, "Canon RF 600mm f/11 IS STM");
    db.insert(61191, "Canon RF 800mm f/11 IS STM");
    db.insert(61192, "Canon RF 24-240mm f/4-6.3 IS USM");
    db.insert(61193, "Canon RF 35mm f/1.8 IS STM Macro");

    db
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_common_lens() {
        // Canon EF 50mm f/1.8 STM
        let result = lookup_lens_name(4156);
        assert_eq!(result, Some("Canon EF 50mm f/1.8 STM".to_string()));
    }

    #[test]
    fn test_lookup_l_series_lens() {
        // Canon EF 24-70mm f/2.8L II USM
        let result = lookup_lens_name(368);
        assert_eq!(result, Some("Canon EF 24-70mm f/2.8L II USM".to_string()));
    }

    #[test]
    fn test_lookup_rf_lens() {
        // Canon RF 24-105mm f/4L IS USM
        let result = lookup_lens_name(61182);
        assert_eq!(result, Some("Canon RF 24-105mm f/4L IS USM".to_string()));
    }

    #[test]
    fn test_lookup_unknown_lens() {
        let result = lookup_lens_name(65000);
        assert_eq!(result, None);
    }

    #[test]
    fn test_database_size() {
        // Should have 100+ lens entries minimum
        assert!(
            CANON_LENS_DATABASE.len() >= 100,
            "Expected at least 100 lens entries, found {}",
            CANON_LENS_DATABASE.len()
        );
    }
}
