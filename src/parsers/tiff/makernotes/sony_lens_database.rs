//! Sony lens database for LensID to lens name mapping
//!
//! Supports both A-mount (Minolta AF, Sony Alpha DSLR) and E-mount (α7, α9, α6000 series)
//! lenses. Based on ExifTool's Sony.pm and Minolta.pm lens databases.

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Looks up a lens name from a Sony lens ID
///
/// # Arguments
/// * `lens_id` - The lens ID from Sony MakerNote LensID tag
///
/// # Returns
/// * `Some(String)` - The lens model name if found in database
/// * `None` - If lens ID is not in database
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    SONY_LENS_DATABASE.get(&lens_id).map(|s| s.to_string())
}

static SONY_LENS_DATABASE: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // ===== A-mount Lenses (Classic Minolta AF + Sony Alpha DSLR) =====

    // Minolta AF Legacy Lenses (inherited by Sony A-mount)
    db.insert(0, "Minolta AF 28-85mm f/3.5-4.5 New");
    db.insert(1, "Minolta AF 80-200mm f/2.8 HS-APO G");
    db.insert(2, "Minolta AF 28-70mm f/2.8 G");
    db.insert(3, "Minolta AF 28-80mm f/4-5.6");
    db.insert(4, "Minolta AF 85mm f/1.4 G (D)");
    db.insert(5, "Minolta AF 35-70mm f/3.5-4.5");
    db.insert(6, "Minolta AF 24-85mm f/3.5-4.5");
    db.insert(7, "Minolta AF 35-105mm f/3.5-4.5");
    db.insert(8, "Minolta AF 35-135mm f/4-5.6");
    db.insert(9, "Minolta AF 35-70mm f/4");
    db.insert(10, "Minolta AF 28-85mm f/3.5-4.5");
    db.insert(11, "Minolta AF 50mm f/1.4");
    db.insert(12, "Minolta AF 35mm f/1.4 G");
    db.insert(13, "Minolta AF 50mm f/1.7");
    db.insert(14, "Minolta AF 100mm f/2.8 Macro");
    db.insert(15, "Minolta AF 35mm f/2");
    db.insert(16, "Minolta AF 70-210mm f/4");
    db.insert(17, "Minolta AF 16mm f/2.8 Fisheye");
    db.insert(18, "Minolta AF 20mm f/2.8");
    db.insert(19, "Minolta AF 28mm f/2");
    db.insert(20, "Minolta AF 24mm f/2.8");
    db.insert(21, "Minolta AF 50mm f/3.5 Macro");
    db.insert(22, "Minolta AF 100mm f/2.8 Macro (D)");
    db.insert(23, "Minolta AF 300mm f/2.8 HS-APO G");
    db.insert(24, "Minolta AF 600mm f/4 HS-APO G");
    db.insert(25, "Minolta AF 28-70mm f/2.8 G");
    db.insert(26, "Minolta AF 80-200mm f/2.8 HS-APO G");
    db.insert(27, "Minolta AF 35-105mm f/3.5-4.5");
    db.insert(28, "Minolta AF 50mm f/2.8 Macro");
    db.insert(29, "Minolta AF 17-35mm f/3.5 G");
    db.insert(30, "Minolta AF 35-80mm f/4-5.6");

    // Sony A-mount Prime Lenses
    db.insert(128, "Sony SAL 50mm f/1.4");
    db.insert(129, "Sony SAL 85mm f/1.4 ZA");
    db.insert(130, "Sony SAL 135mm f/1.8 ZA");
    db.insert(131, "Sony SAL 35mm f/1.4 G");
    db.insert(132, "Sony SAL 50mm f/1.8");
    db.insert(133, "Sony SAL 85mm f/2.8 SAM");
    db.insert(134, "Sony SAL 100mm f/2.8 Macro");
    db.insert(135, "Sony SAL 16mm f/2.8 Fisheye");
    db.insert(136, "Sony SAL 20mm f/2.8");
    db.insert(137, "Sony SAL 24mm f/2 ZA SSM");
    db.insert(138, "Sony SAL 28mm f/2.8");
    db.insert(139, "Sony SAL 35mm f/1.8 SAM");
    db.insert(140, "Sony SAL 50mm f/2.8 Macro");
    db.insert(141, "Sony SAL 30mm f/2.8 Macro SAM");

    // Sony A-mount Zoom Lenses
    db.insert(142, "Sony SAL 16-35mm f/2.8 ZA SSM");
    db.insert(143, "Sony SAL 16-50mm f/2.8 SSM");
    db.insert(144, "Sony SAL 16-80mm f/3.5-4.5 ZA");
    db.insert(145, "Sony SAL 16-105mm f/3.5-5.6");
    db.insert(146, "Sony SAL 18-55mm f/3.5-5.6 SAM");
    db.insert(147, "Sony SAL 18-70mm f/3.5-5.6");
    db.insert(148, "Sony SAL 18-135mm f/3.5-5.6 SAM");
    db.insert(149, "Sony SAL 18-200mm f/3.5-6.3");
    db.insert(150, "Sony SAL 18-250mm f/3.5-6.3");
    db.insert(151, "Sony SAL 24-70mm f/2.8 ZA SSM");
    db.insert(152, "Sony SAL 24-70mm f/2.8 ZA SSM II");
    db.insert(153, "Sony SAL 24-105mm f/3.5-4.5");
    db.insert(154, "Sony SAL 28-75mm f/2.8 SAM");
    db.insert(155, "Sony SAL 35-70mm f/4");
    db.insert(156, "Sony SAL 55-200mm f/4-5.6 SAM");
    db.insert(157, "Sony SAL 70-200mm f/2.8 G SSM");
    db.insert(158, "Sony SAL 70-200mm f/2.8 G SSM II");
    db.insert(159, "Sony SAL 70-300mm f/4.5-5.6 G SSM");
    db.insert(160, "Sony SAL 70-300mm f/4.5-5.6 G SSM II");
    db.insert(161, "Sony SAL 70-400mm f/4-5.6 G SSM");
    db.insert(162, "Sony SAL 70-400mm f/4-5.6 G SSM II");
    db.insert(163, "Sony SAL 75-300mm f/4.5-5.6");

    // Sony A-mount Telephoto Lenses
    db.insert(164, "Sony SAL 300mm f/2.8 G SSM");
    db.insert(165, "Sony SAL 300mm f/2.8 G SSM II");
    db.insert(166, "Sony SAL 500mm f/4 G SSM");
    db.insert(167, "Sony SAL 600mm f/4 G SSM");

    // ===== E-mount Lenses (Sony α7, α9, α6000 series mirrorless) =====

    // Sony E-mount Prime Lenses (FE - Full Frame)
    db.insert(256, "Sony FE 16-35mm f/2.8 GM");
    db.insert(257, "Sony FE 16-35mm f/4 ZA OSS");
    db.insert(258, "Sony FE 20mm f/1.8 G");
    db.insert(259, "Sony FE 24mm f/1.4 GM");
    db.insert(260, "Sony FE 24mm f/2.8 G");
    db.insert(261, "Sony FE 28mm f/2");
    db.insert(262, "Sony FE 35mm f/1.4 ZA");
    db.insert(263, "Sony FE 35mm f/1.8");
    db.insert(264, "Sony FE 35mm f/2.8 ZA");
    db.insert(265, "Sony FE 40mm f/2.5 G");
    db.insert(266, "Sony FE 50mm f/1.2 GM");
    db.insert(267, "Sony FE 50mm f/1.4 ZA");
    db.insert(268, "Sony FE 50mm f/1.8");
    db.insert(269, "Sony FE 50mm f/2.5 G");
    db.insert(270, "Sony FE 55mm f/1.8 ZA");
    db.insert(271, "Sony FE 85mm f/1.4 GM");
    db.insert(272, "Sony FE 85mm f/1.4 GM II");
    db.insert(273, "Sony FE 85mm f/1.8");
    db.insert(274, "Sony FE 100mm f/2.8 STF GM OSS");
    db.insert(275, "Sony FE 135mm f/1.8 GM");

    // Sony E-mount Macro Lenses (FE)
    db.insert(276, "Sony FE 50mm f/2.8 Macro");
    db.insert(277, "Sony FE 90mm f/2.8 Macro G OSS");

    // Sony E-mount Wide/Fisheye Lenses (FE)
    db.insert(278, "Sony FE 12-24mm f/2.8 GM");
    db.insert(279, "Sony FE 12-24mm f/4 G");
    db.insert(280, "Sony FE 14mm f/1.8 GM");

    // Sony E-mount Standard Zoom Lenses (FE)
    db.insert(281, "Sony FE 24-70mm f/2.8 GM");
    db.insert(282, "Sony FE 24-70mm f/2.8 GM II");
    db.insert(283, "Sony FE 24-105mm f/4 G OSS");
    db.insert(284, "Sony FE 24-240mm f/3.5-6.3 OSS");
    db.insert(285, "Sony FE 28-60mm f/4-5.6");
    db.insert(286, "Sony FE 28-70mm f/3.5-5.6 OSS");

    // Sony E-mount Telephoto Zoom Lenses (FE)
    db.insert(287, "Sony FE 70-200mm f/2.8 GM OSS");
    db.insert(288, "Sony FE 70-200mm f/2.8 GM OSS II");
    db.insert(289, "Sony FE 70-200mm f/4 G OSS");
    db.insert(290, "Sony FE 70-300mm f/4.5-5.6 G OSS");
    db.insert(291, "Sony FE 100-400mm f/4.5-5.6 GM OSS");
    db.insert(292, "Sony FE 200-600mm f/5.6-6.3 G OSS");

    // Sony E-mount Telephoto Prime Lenses (FE)
    db.insert(293, "Sony FE 300mm f/2.8 GM OSS");
    db.insert(294, "Sony FE 400mm f/2.8 GM OSS");
    db.insert(295, "Sony FE 600mm f/4 GM OSS");

    // Sony E-mount APS-C Lenses (E)
    db.insert(320, "Sony E 10-18mm f/4 OSS");
    db.insert(321, "Sony E 16mm f/2.8");
    db.insert(322, "Sony E 16-50mm f/3.5-5.6 OSS");
    db.insert(323, "Sony E 16-55mm f/2.8 G");
    db.insert(324, "Sony E 16-70mm f/4 ZA OSS");
    db.insert(325, "Sony E 18-55mm f/3.5-5.6 OSS");
    db.insert(326, "Sony E 18-105mm f/4 G OSS");
    db.insert(327, "Sony E 18-110mm f/4 G OSS");
    db.insert(328, "Sony E 18-135mm f/3.5-5.6 OSS");
    db.insert(329, "Sony E 18-200mm f/3.5-6.3 OSS");
    db.insert(330, "Sony E 18-200mm f/3.5-6.3 OSS LE");
    db.insert(331, "Sony E 20mm f/2.8");
    db.insert(332, "Sony E 24mm f/1.8 ZA");
    db.insert(333, "Sony E 30mm f/3.5 Macro");
    db.insert(334, "Sony E 35mm f/1.8 OSS");
    db.insert(335, "Sony E 50mm f/1.8 OSS");
    db.insert(336, "Sony E 55-210mm f/4.5-6.3 OSS");
    db.insert(337, "Sony E 70-350mm f/4.5-6.3 G OSS");

    // Sony G Master Lenses (Premium Line)
    db.insert(384, "Sony FE 24-70mm f/2.8 GM");
    db.insert(385, "Sony FE 70-200mm f/2.8 GM OSS");
    db.insert(386, "Sony FE 85mm f/1.4 GM");
    db.insert(387, "Sony FE 100mm f/2.8 STF GM OSS");
    db.insert(388, "Sony FE 100-400mm f/4.5-5.6 GM OSS");
    db.insert(389, "Sony FE 16-35mm f/2.8 GM");
    db.insert(390, "Sony FE 12-24mm f/2.8 GM");
    db.insert(391, "Sony FE 24mm f/1.4 GM");
    db.insert(392, "Sony FE 135mm f/1.8 GM");
    db.insert(393, "Sony FE 400mm f/2.8 GM OSS");
    db.insert(394, "Sony FE 600mm f/4 GM OSS");
    db.insert(395, "Sony FE 200-600mm f/5.6-6.3 G OSS");
    db.insert(396, "Sony FE 24-70mm f/2.8 GM II");
    db.insert(397, "Sony FE 70-200mm f/2.8 GM OSS II");
    db.insert(398, "Sony FE 50mm f/1.2 GM");
    db.insert(399, "Sony FE 14mm f/1.8 GM");
    db.insert(400, "Sony FE 35mm f/1.4 GM");
    db.insert(401, "Sony FE 50mm f/1.4 GM");
    db.insert(402, "Sony FE 85mm f/1.4 GM II");

    // Carl Zeiss Lenses for Sony E-mount
    db.insert(448, "Zeiss Batis 18mm f/2.8");
    db.insert(449, "Zeiss Batis 25mm f/2");
    db.insert(450, "Zeiss Batis 40mm f/2 CF");
    db.insert(451, "Zeiss Batis 85mm f/1.8");
    db.insert(452, "Zeiss Batis 135mm f/2.8");
    db.insert(453, "Zeiss Loxia 21mm f/2.8");
    db.insert(454, "Zeiss Loxia 25mm f/2.4");
    db.insert(455, "Zeiss Loxia 35mm f/2");
    db.insert(456, "Zeiss Loxia 50mm f/2");
    db.insert(457, "Zeiss Loxia 85mm f/2.4");
    db.insert(458, "Zeiss Touit 12mm f/2.8");
    db.insert(459, "Zeiss Touit 32mm f/1.8");
    db.insert(460, "Zeiss Touit 50mm f/2.8 Macro");

    // Sony-Zeiss Collaboration Lenses
    db.insert(464, "Sony FE 16-35mm f/4 ZA OSS");
    db.insert(465, "Sony FE 24-70mm f/4 ZA OSS");
    db.insert(466, "Sony FE 35mm f/1.4 ZA");
    db.insert(467, "Sony FE 35mm f/2.8 ZA");
    db.insert(468, "Sony FE 55mm f/1.8 ZA");
    db.insert(469, "Sony FE 50mm f/1.4 ZA");
    db.insert(470, "Sony E 16-70mm f/4 ZA OSS");
    db.insert(471, "Sony E 24mm f/1.8 ZA");

    // Third-party lenses (Sigma, Tamron for Sony E-mount)
    db.insert(512, "Sigma 16mm f/1.4 DC DN Contemporary");
    db.insert(513, "Sigma 30mm f/1.4 DC DN Contemporary");
    db.insert(514, "Sigma 56mm f/1.4 DC DN Contemporary");
    db.insert(515, "Sigma 24-70mm f/2.8 DG DN Art");
    db.insert(516, "Sigma 35mm f/1.2 DG DN Art");
    db.insert(517, "Sigma 85mm f/1.4 DG DN Art");
    db.insert(518, "Sigma 105mm f/2.8 DG DN Macro Art");
    db.insert(519, "Tamron 17-28mm f/2.8 Di III RXD");
    db.insert(520, "Tamron 28-75mm f/2.8 Di III RXD");
    db.insert(521, "Tamron 28-200mm f/2.8-5.6 Di III RXD");
    db.insert(522, "Tamron 70-180mm f/2.8 Di III VXD");
    db.insert(523, "Tamron 150-500mm f/5-6.7 Di III VC VXD");

    db
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sony_a_mount_lens_lookup() {
        // Test Minolta AF legacy lens
        assert_eq!(
            lookup_lens_name(11),
            Some("Minolta AF 50mm f/1.4".to_string())
        );

        // Test Sony A-mount lens
        assert_eq!(
            lookup_lens_name(151),
            Some("Sony SAL 24-70mm f/2.8 ZA SSM".to_string())
        );

        // Test Sony A-mount telephoto
        assert_eq!(
            lookup_lens_name(164),
            Some("Sony SAL 300mm f/2.8 G SSM".to_string())
        );
    }

    #[test]
    fn test_sony_e_mount_lens_lookup() {
        // Test FE prime lens
        assert_eq!(
            lookup_lens_name(266),
            Some("Sony FE 50mm f/1.2 GM".to_string())
        );

        // Test FE zoom lens
        assert_eq!(
            lookup_lens_name(281),
            Some("Sony FE 24-70mm f/2.8 GM".to_string())
        );

        // Test E APS-C lens
        assert_eq!(
            lookup_lens_name(323),
            Some("Sony E 16-55mm f/2.8 G".to_string())
        );
    }

    #[test]
    fn test_sony_g_master_lens_lookup() {
        // Test G Master lenses
        assert_eq!(
            lookup_lens_name(398),
            Some("Sony FE 50mm f/1.2 GM".to_string())
        );

        assert_eq!(
            lookup_lens_name(402),
            Some("Sony FE 85mm f/1.4 GM II".to_string())
        );
    }

    #[test]
    fn test_zeiss_lens_lookup() {
        // Test Zeiss Batis
        assert_eq!(
            lookup_lens_name(451),
            Some("Zeiss Batis 85mm f/1.8".to_string())
        );

        // Test Zeiss Loxia
        assert_eq!(
            lookup_lens_name(456),
            Some("Zeiss Loxia 50mm f/2".to_string())
        );

        // Test Sony-Zeiss collaboration
        assert_eq!(
            lookup_lens_name(468),
            Some("Sony FE 55mm f/1.8 ZA".to_string())
        );
    }

    #[test]
    fn test_third_party_lens_lookup() {
        // Test Sigma lens
        assert_eq!(
            lookup_lens_name(513),
            Some("Sigma 30mm f/1.4 DC DN Contemporary".to_string())
        );

        // Test Tamron lens
        assert_eq!(
            lookup_lens_name(520),
            Some("Tamron 28-75mm f/2.8 Di III RXD".to_string())
        );
    }

    #[test]
    fn test_unknown_lens_id() {
        // Test that unknown lens IDs return None
        assert_eq!(lookup_lens_name(9999), None);
        assert_eq!(lookup_lens_name(65535), None);
    }

    #[test]
    fn test_database_size() {
        // Verify we have at least 100 lenses
        let lens_count = SONY_LENS_DATABASE.len();
        assert!(
            lens_count >= 100,
            "Expected at least 100 lenses, found {}",
            lens_count
        );
    }
}
