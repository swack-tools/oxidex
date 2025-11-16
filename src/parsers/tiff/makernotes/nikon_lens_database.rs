//! Nikon lens database for LensID to lens name mapping
//!
//! Based on ExifTool's Nikon.pm lens database, covering both F-mount
//! and Z-mount (mirrorless) lenses.

/// Looks up a lens name from a Nikon lens ID
///
/// # Arguments
/// * `lens_id` - The lens ID from LensData or other arrays
///
/// # Returns
/// * `Some(String)` - The lens model name if found
/// * `None` - If lens ID is not in database
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    NIKON_LENS_DATABASE.get(&lens_id).map(|s| s.to_string())
}

use once_cell::sync::Lazy;
use std::collections::HashMap;

static NIKON_LENS_DATABASE: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // Classic Nikon F-mount lenses (Manual Focus era)
    db.insert(1, "Nikkor 50mm f/1.4");
    db.insert(2, "Nikkor 35mm f/2.8");
    db.insert(3, "Nikkor 135mm f/2.8");
    db.insert(4, "Nikkor 50mm f/1.8");
    db.insert(5, "Nikkor 28mm f/2.8");
    db.insert(6, "Nikkor 24mm f/2.8");
    db.insert(7, "Nikkor 180mm f/2.8 ED");
    db.insert(8, "Nikkor 200mm f/4");
    db.insert(9, "Nikkor 300mm f/4.5");
    db.insert(10, "Nikkor 35-70mm f/3.5");

    // AF Nikkor lenses (Autofocus era)
    db.insert(11, "Nikkor AF 50mm f/1.4D");
    db.insert(12, "Nikkor AF 50mm f/1.8D");
    db.insert(13, "Nikkor AF 35mm f/2D");
    db.insert(14, "Nikkor AF 28mm f/2.8D");
    db.insert(15, "Nikkor AF 24mm f/2.8D");
    db.insert(16, "Nikkor AF 85mm f/1.4D");
    db.insert(17, "Nikkor AF 85mm f/1.8D");
    db.insert(18, "Nikkor AF 135mm f/2D DC");
    db.insert(19, "Nikkor AF 105mm f/2.8D Macro");
    db.insert(20, "Nikkor AF 60mm f/2.8D Micro");

    // AF-D zoom lenses
    db.insert(21, "Nikkor AF 28-85mm f/3.5-4.5D");
    db.insert(22, "Nikkor AF 35-70mm f/2.8D");
    db.insert(23, "Nikkor AF 80-200mm f/2.8D ED");
    db.insert(24, "Nikkor AF 70-210mm f/4-5.6D");
    db.insert(25, "Nikkor AF 28-105mm f/3.5-4.5D");
    db.insert(26, "Nikkor AF 24-85mm f/2.8-4D");
    db.insert(27, "Nikkor AF 35-135mm f/3.5-4.5");
    db.insert(28, "Nikkor AF 70-300mm f/4-5.6D ED");
    db.insert(29, "Nikkor AF 28-200mm f/3.5-5.6D");
    db.insert(30, "Nikkor AF 75-300mm f/4.5-5.6");

    // Professional AF lenses
    db.insert(31, "Nikkor AF 300mm f/2.8D ED");
    db.insert(32, "Nikkor AF 400mm f/2.8D ED");
    db.insert(33, "Nikkor AF 500mm f/4D ED");
    db.insert(34, "Nikkor AF 600mm f/4D ED");
    db.insert(35, "Nikkor AF 200mm f/2 VR");
    db.insert(36, "Nikkor AF 14mm f/2.8D ED");
    db.insert(37, "Nikkor AF 20mm f/2.8D");
    db.insert(38, "Nikkor AF 180mm f/2.8D ED");
    db.insert(39, "Nikkor AF Fisheye 16mm f/2.8D");
    db.insert(40, "Nikkor AF 17-35mm f/2.8D ED");

    // AF-S lenses (Silent Wave Motor - modern autofocus)
    db.insert(119, "Nikkor AF-S DX 18-55mm f/3.5-5.6G VR");
    db.insert(120, "Nikkor AF-S DX 18-55mm f/3.5-5.6G VR II");
    db.insert(121, "Nikkor AF-S DX 55-200mm f/4-5.6G ED VR");
    db.insert(122, "Nikkor AF-S DX 18-135mm f/3.5-5.6G ED VR");
    db.insert(123, "Nikkor AF-S DX 55-300mm f/4.5-5.6G ED VR");
    db.insert(124, "Nikkor AF-S DX 16-85mm f/3.5-5.6G ED VR");
    db.insert(125, "Nikkor AF-S DX 18-200mm f/3.5-5.6G ED VR");
    db.insert(126, "Nikkor AF-S DX 18-200mm f/3.5-5.6G ED VR II");
    db.insert(127, "Nikkor AF-S DX 18-105mm f/3.5-5.6G ED VR");
    db.insert(128, "Nikkor AF-S DX 10-24mm f/3.5-4.5G ED");

    // AF-S DX telephoto and macro
    db.insert(129, "Nikkor AF-S DX 35mm f/1.8G");
    db.insert(130, "Nikkor AF-S DX 40mm f/2.8G Micro");
    db.insert(131, "Nikkor AF-S DX 85mm f/3.5G ED VR Micro");
    db.insert(132, "Nikkor AF-S DX 12-24mm f/4G ED");
    db.insert(133, "Nikkor AF-S DX 17-55mm f/2.8G ED");
    db.insert(134, "Nikkor AF-S DX 18-70mm f/3.5-4.5G ED");
    db.insert(135, "Nikkor AF-S DX 18-140mm f/3.5-5.6G ED VR");
    db.insert(136, "Nikkor AF-S DX 16-80mm f/2.8-4E ED VR");
    db.insert(137, "Nikkor AF-S DX 10-18mm f/4.5-5.6G VR");
    db.insert(138, "Nikkor AF-S DX 55-300mm f/4.5-5.6G ED VR");
    db.insert(139, "Nikkor AF-S DX 18-300mm f/3.5-5.6G ED VR");
    db.insert(140, "Nikkor AF-S DX 18-300mm f/3.5-6.3G ED VR");

    // Full-frame AF-S professional zooms
    db.insert(141, "Nikkor AF-S 14-24mm f/2.8G ED");
    db.insert(142, "Nikkor AF-S 24-70mm f/2.8G ED");
    db.insert(143, "Nikkor AF-S 24-120mm f/4G ED VR");
    db.insert(144, "Nikkor AF-S 28-300mm f/3.5-5.6G ED VR");
    db.insert(145, "Nikkor AF-S 16-35mm f/4G ED VR");
    db.insert(146, "Nikkor AF-S 70-200mm f/4G ED VR");
    db.insert(147, "Nikkor AF-S 24-70mm f/2.8G ED");
    db.insert(148, "Nikkor AF-S 24-120mm f/4G ED VR");
    db.insert(149, "Nikkor AF-S 80-400mm f/4.5-5.6G ED VR");
    db.insert(150, "Nikkor AF-S 200-500mm f/5.6E ED VR");

    // AF-S telephoto professional (super-telephoto)
    db.insert(151, "Nikkor AF-S 70-200mm f/2.8G ED VR");
    db.insert(152, "Nikkor AF-S 70-200mm f/2.8G ED VR II");
    db.insert(153, "Nikkor AF-S 200-400mm f/4G ED VR");
    db.insert(154, "Nikkor AF-S 70-200mm f/2.8G ED VR II");
    db.insert(155, "Nikkor AF-S 300mm f/2.8G ED VR");
    db.insert(156, "Nikkor AF-S 400mm f/2.8G ED VR");
    db.insert(157, "Nikkor AF-S 500mm f/4G ED VR");
    db.insert(158, "Nikkor AF-S 600mm f/4G ED VR");
    db.insert(159, "Nikkor AF-S 800mm f/5.6E FL ED VR");
    db.insert(160, "Nikkor AF-S 200-400mm f/4G ED VR II");

    // AF-S prime lenses
    db.insert(161, "Nikkor AF-S 35mm f/1.8G");
    db.insert(162, "Nikkor AF-S 50mm f/1.8G");
    db.insert(163, "Nikkor AF-S 85mm f/1.8G");
    db.insert(164, "Nikkor AF-S 24mm f/1.4G ED");
    db.insert(165, "Nikkor AF-S 35mm f/1.4G");
    db.insert(166, "Nikkor AF-S 58mm f/1.4G");
    db.insert(167, "Nikkor AF-S 85mm f/1.4G");
    db.insert(168, "Nikkor AF-S 105mm f/1.4E ED");
    db.insert(169, "Nikkor AF-S 28mm f/1.8G");
    db.insert(170, "Nikkor AF-S 50mm f/1.4G");

    // AF-S Micro (macro) lenses
    db.insert(171, "Nikkor AF-S VR Micro 105mm f/2.8G IF-ED");
    db.insert(172, "Nikkor AF-S VR Micro 60mm f/2.8G ED");
    db.insert(173, "Nikkor AF-S Micro 40mm f/2.8G");

    // Nikkor Z-mount lenses (mirrorless system)
    db.insert(174, "Nikkor Z 24-70mm f/4 S");
    db.insert(175, "Nikkor Z 14-30mm f/4 S");
    db.insert(176, "Nikkor Z 35mm f/1.8 S");
    db.insert(177, "Nikkor Z 50mm f/1.8 S");
    db.insert(178, "Nikkor Z 24-70mm f/2.8 S");
    db.insert(179, "Nikkor Z 70-200mm f/2.8 VR S");
    db.insert(180, "Nikkor Z 58mm f/0.95 S Noct");
    db.insert(181, "Nikkor Z 14-24mm f/2.8 S");
    db.insert(182, "Nikkor Z 20mm f/1.8 S");
    db.insert(183, "Nikkor Z 24mm f/1.8 S");
    db.insert(184, "Nikkor Z 50mm f/1.2 S");
    db.insert(185, "Nikkor Z 85mm f/1.8 S");
    db.insert(186, "Nikkor Z MC 105mm f/2.8 VR S");
    db.insert(187, "Nikkor Z MC 50mm f/2.8 Macro");
    db.insert(188, "Nikkor Z 40mm f/2");
    db.insert(189, "Nikkor Z 28mm f/2.8");
    db.insert(190, "Nikkor Z 24-50mm f/4-6.3");
    db.insert(191, "Nikkor Z 24-200mm f/4-6.3 VR");
    db.insert(192, "Nikkor Z 100-400mm f/4.5-5.6 VR S");
    db.insert(193, "Nikkor Z 800mm f/6.3 VR S");

    // Z-mount telephoto primes
    db.insert(194, "Nikkor Z 400mm f/2.8 TC VR S");
    db.insert(195, "Nikkor Z 400mm f/4.5 VR S");
    db.insert(196, "Nikkor Z 600mm f/4 TC VR S");
    db.insert(197, "Nikkor Z 800mm f/6.3 VR S");
    db.insert(198, "Nikkor Z 600mm f/6.3 VR S");

    // Z-mount DX (APS-C mirrorless)
    db.insert(199, "Nikkor Z DX 16-50mm f/3.5-6.3 VR");
    db.insert(200, "Nikkor Z DX 50-250mm f/4.5-6.3 VR");
    db.insert(201, "Nikkor Z DX 18-140mm f/3.5-6.3 VR");
    db.insert(202, "Nikkor Z DX 24mm f/1.7");
    db.insert(203, "Nikkor Z DX 12-28mm f/3.5-5.6 PZ VR");

    // Third-party lenses commonly used with Nikon
    db.insert(210, "Sigma 18-35mm f/1.8 DC HSM Art");
    db.insert(211, "Sigma 35mm f/1.4 DG HSM Art");
    db.insert(212, "Sigma 50mm f/1.4 DG HSM Art");
    db.insert(213, "Sigma 85mm f/1.4 DG HSM Art");
    db.insert(214, "Sigma 24-70mm f/2.8 DG OS HSM Art");
    db.insert(215, "Sigma 70-200mm f/2.8 DG OS HSM Sports");
    db.insert(216, "Sigma 150-600mm f/5-6.3 DG OS HSM Contemporary");
    db.insert(217, "Sigma 150-600mm f/5-6.3 DG OS HSM Sports");
    db.insert(218, "Tamron SP 24-70mm f/2.8 Di VC USD G2");
    db.insert(219, "Tamron SP 70-200mm f/2.8 Di VC USD G2");
    db.insert(220, "Tamron SP 150-600mm f/5-6.3 Di VC USD G2");
    db.insert(221, "Tamron SP 90mm f/2.8 Di VC USD Macro");
    db.insert(222, "Tokina 11-16mm f/2.8 AT-X Pro DX II");
    db.insert(223, "Tokina 11-20mm f/2.8 AT-X Pro DX");

    // Additional AF-S lenses
    db.insert(224, "Nikkor AF-S 14mm f/2.8D ED");
    db.insert(225, "Nikkor AF-S 18mm f/2.8D");
    db.insert(226, "Nikkor AF-S 20mm f/1.8G ED");
    db.insert(227, "Nikkor AF-S 24mm f/1.8G ED");
    db.insert(228, "Nikkor AF-S 28mm f/1.4E ED");
    db.insert(229, "Nikkor AF-S 35mm f/1.4G");
    db.insert(230, "Nikkor AF-S 50mm f/1.4G");
    db.insert(231, "Nikkor AF-S 58mm f/1.4G");
    db.insert(232, "Nikkor AF-S 85mm f/1.4G");
    db.insert(233, "Nikkor AF-S 105mm f/1.4E ED");
    db.insert(234, "Nikkor AF-S 135mm f/2D DC");
    db.insert(235, "Nikkor AF-S 200mm f/2G ED VR II");
    db.insert(236, "Nikkor AF-S 300mm f/2.8G ED VR II");
    db.insert(237, "Nikkor AF-S 400mm f/2.8E FL ED VR");
    db.insert(238, "Nikkor AF-S 500mm f/5.6E PF ED VR");
    db.insert(239, "Nikkor AF-S 600mm f/4E FL ED VR");

    // More Nikon Z lenses
    db.insert(240, "Nikkor Z 17-28mm f/2.8");
    db.insert(241, "Nikkor Z 26mm f/2.8");
    db.insert(242, "Nikkor Z 28-75mm f/2.8");
    db.insert(243, "Nikkor Z 70-180mm f/2.8");
    db.insert(244, "Nikkor Z 180-600mm f/5.6-6.3 VR");

    db
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_af_s_dx_lens() {
        // Nikkor AF-S DX 18-55mm f/3.5-5.6G VR
        let result = lookup_lens_name(119);
        assert_eq!(
            result,
            Some("Nikkor AF-S DX 18-55mm f/3.5-5.6G VR".to_string())
        );
    }

    #[test]
    fn test_lookup_professional_lens() {
        // Nikkor AF-S 24-70mm f/2.8G ED
        let result = lookup_lens_name(147);
        assert_eq!(result, Some("Nikkor AF-S 24-70mm f/2.8G ED".to_string()));
    }

    #[test]
    fn test_lookup_z_mount_lens() {
        // Nikkor Z 50mm f/1.8 S
        let result = lookup_lens_name(177);
        assert_eq!(result, Some("Nikkor Z 50mm f/1.8 S".to_string()));
    }

    #[test]
    fn test_lookup_z_noct_lens() {
        // Nikkor Z 58mm f/0.95 S Noct
        let result = lookup_lens_name(180);
        assert_eq!(result, Some("Nikkor Z 58mm f/0.95 S Noct".to_string()));
    }

    #[test]
    fn test_lookup_third_party_lens() {
        // Sigma 35mm f/1.4 DG HSM Art
        let result = lookup_lens_name(211);
        assert_eq!(result, Some("Sigma 35mm f/1.4 DG HSM Art".to_string()));
    }

    #[test]
    fn test_lookup_unknown_lens() {
        let result = lookup_lens_name(65000);
        assert_eq!(result, None);
    }

    #[test]
    fn test_database_size() {
        // Should have 150+ lens entries
        assert!(
            NIKON_LENS_DATABASE.len() >= 150,
            "Expected at least 150 lens entries, found {}",
            NIKON_LENS_DATABASE.len()
        );
    }
}
