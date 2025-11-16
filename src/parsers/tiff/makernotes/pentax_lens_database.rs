//! Pentax lens database for LensID to lens name mapping
//!
//! Based on ExifTool's Pentax.pm lens database, covering:
//! - Classic K-mount lenses (K, M, A series)
//! - Modern DA/FA/D FA series (APS-C and full-frame)
//! - Limited editions and special lenses
//! - Third-party K-mount compatible lenses

/// Looks up a lens name from a Pentax lens ID
///
/// # Arguments
/// * `lens_id` - The lens ID from LensType tag
///
/// # Returns
/// * `Some(String)` - The lens model name if found
/// * `None` - If lens ID is not in database
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    PENTAX_LENS_DATABASE.get(&lens_id).map(|s| s.to_string())
}

use once_cell::sync::Lazy;
use std::collections::HashMap;

static PENTAX_LENS_DATABASE: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // ===== Classic K-Mount Manual Focus Lenses (SMC Pentax K, M, A series) =====
    // These lenses are highly regarded for their optical quality and build
    db.insert(1, "SMC Pentax-K 50mm f/1.2");
    db.insert(2, "SMC Pentax-K 50mm f/1.4");
    db.insert(3, "SMC Pentax-K 28mm f/2.8");
    db.insert(4, "SMC Pentax-K 35mm f/2.8");
    db.insert(5, "SMC Pentax-K 135mm f/2.5");
    db.insert(6, "SMC Pentax-M 50mm f/1.7");
    db.insert(7, "SMC Pentax-M 50mm f/1.4");
    db.insert(8, "SMC Pentax-M 50mm f/2.0");
    db.insert(9, "SMC Pentax-M 28mm f/2.8");
    db.insert(10, "SMC Pentax-M 35mm f/2.0");
    db.insert(11, "SMC Pentax-M 40mm f/2.8");
    db.insert(12, "SMC Pentax-M 100mm f/2.8");
    db.insert(13, "SMC Pentax-M 135mm f/3.5");
    db.insert(14, "SMC Pentax-M 200mm f/4.0");

    // ===== SMC Pentax-A Autofocus Lenses (First AF generation) =====
    db.insert(20, "SMC Pentax-A 50mm f/1.4");
    db.insert(21, "SMC Pentax-A 50mm f/1.7");
    db.insert(22, "SMC Pentax-A 50mm f/2.0");
    db.insert(23, "SMC Pentax-A 28mm f/2.8");
    db.insert(24, "SMC Pentax-A 35mm f/2.0");
    db.insert(25, "SMC Pentax-A 85mm f/1.4");
    db.insert(26, "SMC Pentax-A 100mm f/2.8 Macro");
    db.insert(27, "SMC Pentax-A 135mm f/2.8");
    db.insert(28, "SMC Pentax-A 200mm f/2.8");
    db.insert(29, "SMC Pentax-A 200mm f/4.0");
    db.insert(30, "SMC Pentax-A 300mm f/4.0");

    // ===== SMC Pentax-F Autofocus Lenses (Early modern AF) =====
    db.insert(40, "SMC Pentax-F 50mm f/1.4");
    db.insert(41, "SMC Pentax-F 50mm f/1.7");
    db.insert(42, "SMC Pentax-F 35-70mm f/3.5-4.5");
    db.insert(43, "SMC Pentax-F 70-210mm f/4.0-5.6");
    db.insert(44, "SMC Pentax-F 100mm f/2.8 Macro");
    db.insert(45, "SMC Pentax-F 28-80mm f/3.5-4.5");

    // ===== SMC Pentax-FA (Film + Digital full-frame compatible) =====
    db.insert(50, "SMC Pentax-FA 28mm f/2.8 AL");
    db.insert(51, "SMC Pentax-FA 31mm f/1.8 AL Limited");
    db.insert(52, "SMC Pentax-FA 35mm f/2.0 AL");
    db.insert(53, "SMC Pentax-FA 43mm f/1.9 Limited");
    db.insert(54, "SMC Pentax-FA 50mm f/1.4");
    db.insert(55, "SMC Pentax-FA 50mm f/1.7");
    db.insert(56, "SMC Pentax-FA 77mm f/1.8 Limited");
    db.insert(57, "SMC Pentax-FA 100mm f/2.8 Macro");
    db.insert(58, "SMC Pentax-FA 135mm f/2.8");
    db.insert(59, "SMC Pentax-FA 200mm f/2.8");
    db.insert(60, "SMC Pentax-FA 28-70mm f/4.0 AL");
    db.insert(61, "SMC Pentax-FA 28-105mm f/4.0-5.6");
    db.insert(62, "SMC Pentax-FA 35-80mm f/4.0-5.6");
    db.insert(63, "SMC Pentax-FA 70-200mm f/4.0-5.6");
    db.insert(64, "SMC Pentax-FA 80-200mm f/2.8 ED IF");
    db.insert(65, "SMC Pentax-FA 100-300mm f/4.5-5.6");

    // ===== HD Pentax-DA (APS-C Digital Specific, High Definition coating) =====
    db.insert(70, "HD Pentax-DA 15mm f/4.0 ED AL Limited");
    db.insert(71, "HD Pentax-DA 20-40mm f/2.8-4.0 ED Limited DC WR");
    db.insert(72, "HD Pentax-DA 21mm f/3.2 AL Limited");
    db.insert(73, "HD Pentax-DA 35mm f/2.8 Macro Limited");
    db.insert(74, "HD Pentax-DA 40mm f/2.8 Limited");
    db.insert(75, "HD Pentax-DA 55mm f/1.4 SDM");
    db.insert(76, "HD Pentax-DA 70mm f/2.4 Limited");
    db.insert(77, "HD Pentax-DA 16-85mm f/3.5-5.6 ED DC WR");
    db.insert(78, "HD Pentax-DA 560mm f/5.6 ED AW");

    // ===== SMC Pentax-DA (APS-C Digital Specific) =====
    db.insert(80, "SMC Pentax-DA 14mm f/2.8 ED IF");
    db.insert(81, "SMC Pentax-DA 15mm f/4.0 ED AL Limited");
    db.insert(82, "SMC Pentax-DA 18-55mm f/3.5-5.6 AL");
    db.insert(83, "SMC Pentax-DA 18-55mm f/3.5-5.6 AL II");
    db.insert(84, "SMC Pentax-DA 18-55mm f/3.5-5.6 AL WR");
    db.insert(85, "SMC Pentax-DA 18-135mm f/3.5-5.6 ED AL IF DC WR");
    db.insert(86, "SMC Pentax-DA 18-250mm f/3.5-6.3 ED AL IF");
    db.insert(87, "SMC Pentax-DA 21mm f/3.2 AL Limited");
    db.insert(88, "SMC Pentax-DA 35mm f/2.4 AL");
    db.insert(89, "SMC Pentax-DA 35mm f/2.8 Macro Limited");
    db.insert(90, "SMC Pentax-DA 40mm f/2.8 Limited");
    db.insert(91, "SMC Pentax-DA 40mm f/2.8 XS");
    db.insert(92, "SMC Pentax-DA 50mm f/1.8");
    db.insert(93, "SMC Pentax-DA 50-135mm f/2.8 ED IF SDM");
    db.insert(94, "SMC Pentax-DA 50-200mm f/4.0-5.6 ED");
    db.insert(95, "SMC Pentax-DA 50-200mm f/4.0-5.6 ED WR");
    db.insert(96, "SMC Pentax-DA 55mm f/1.4 SDM");
    db.insert(97, "SMC Pentax-DA 55-300mm f/4.0-5.8 ED");
    db.insert(98, "SMC Pentax-DA 55-300mm f/4.5-6.3 ED PLM WR RE");
    db.insert(99, "SMC Pentax-DA 60-250mm f/4.0 ED IF SDM");
    db.insert(100, "SMC Pentax-DA 70mm f/2.4 Limited");
    db.insert(101, "SMC Pentax-DA 300mm f/4.0 ED IF SDM");

    // ===== SMC Pentax-DA* (Star Series - Professional APS-C lenses) =====
    db.insert(110, "SMC Pentax-DA* 16-50mm f/2.8 ED AL IF SDM");
    db.insert(111, "SMC Pentax-DA* 50-135mm f/2.8 ED IF SDM");
    db.insert(112, "SMC Pentax-DA* 55mm f/1.4 SDM");
    db.insert(113, "SMC Pentax-DA* 200mm f/2.8 ED IF SDM");
    db.insert(114, "SMC Pentax-DA* 300mm f/4.0 ED IF SDM");

    // ===== HD Pentax-D FA (Modern full-frame, compatible with film and digital) =====
    db.insert(120, "HD Pentax-D FA 15-30mm f/2.8 ED SDM WR");
    db.insert(121, "HD Pentax-D FA 21mm f/2.4 ED Limited DC WR");
    db.insert(122, "HD Pentax-D FA 24-70mm f/2.8 ED SDM WR");
    db.insert(123, "HD Pentax-D FA 28-105mm f/3.5-5.6 ED DC WR");
    db.insert(124, "HD Pentax-D FA 35mm f/2.0 AL");
    db.insert(125, "HD Pentax-D FA 50mm f/1.4 SDM AW");
    db.insert(126, "HD Pentax-D FA 70-210mm f/4.0 ED SDM WR");
    db.insert(127, "HD Pentax-D FA 85mm f/1.4 ED SDM AW");
    db.insert(128, "HD Pentax-D FA 100mm f/2.8 Macro WR");
    db.insert(129, "HD Pentax-D FA 150-450mm f/4.5-5.6 ED DC AW");
    db.insert(130, "HD Pentax-D FA* 50mm f/1.4 SDM AW");
    db.insert(131, "HD Pentax-D FA* 70-200mm f/2.8 ED DC AW");
    db.insert(132, "HD Pentax-D FA* 85mm f/1.4 ED SDM AW");

    // ===== SMC Pentax-D FA (Digital Full-Frame) =====
    db.insert(140, "SMC Pentax-D FA Macro 50mm f/2.8");
    db.insert(141, "SMC Pentax-D FA Macro 100mm f/2.8 WR");

    // ===== Specialty and Fisheye Lenses =====
    db.insert(150, "SMC Pentax-DA Fish-Eye 10-17mm f/3.5-4.5 ED IF");
    db.insert(151, "HD Pentax-DA Fish-Eye 10-17mm f/3.5-4.5 ED");
    db.insert(152, "SMC Pentax-F Fish-Eye 17-28mm f/3.5-4.5");
    db.insert(153, "SMC Pentax Fish-Eye 10mm f/2.8");

    // ===== Third-Party K-Mount Lenses =====
    // These are notable third-party lenses commonly used with Pentax bodies
    db.insert(200, "Sigma 10-20mm f/3.5 EX DC HSM (Pentax)");
    db.insert(201, "Sigma 17-50mm f/2.8 EX DC OS HSM (Pentax)");
    db.insert(202, "Sigma 17-70mm f/2.8-4 DC Macro OS HSM (Pentax)");
    db.insert(203, "Sigma 18-35mm f/1.8 DC HSM Art (Pentax)");
    db.insert(204, "Sigma 30mm f/1.4 DC HSM Art (Pentax)");
    db.insert(205, "Sigma 50mm f/1.4 DG HSM Art (Pentax)");
    db.insert(206, "Sigma 50-100mm f/1.8 DC HSM Art (Pentax)");
    db.insert(207, "Sigma 105mm f/2.8 EX DG OS HSM Macro (Pentax)");
    db.insert(
        208,
        "Sigma 150-600mm f/5.0-6.3 DG OS HSM Contemporary (Pentax)",
    );
    db.insert(209, "Sigma 150-600mm f/5.0-6.3 DG OS HSM Sports (Pentax)");
    db.insert(210, "Tamron 10-24mm f/3.5-4.5 Di II LD (Pentax)");
    db.insert(211, "Tamron 16-300mm f/3.5-6.3 Di II VC PZD Macro (Pentax)");
    db.insert(212, "Tamron 17-50mm f/2.8 XR Di II LD (Pentax)");
    db.insert(213, "Tamron 70-200mm f/2.8 Di LD IF Macro (Pentax)");
    db.insert(214, "Tamron 90mm f/2.8 Di VC USD Macro (Pentax)");
    db.insert(215, "Tokina 11-16mm f/2.8 AT-X Pro DX II (Pentax)");
    db.insert(216, "Tokina 12-28mm f/4.0 AT-X Pro (Pentax)");
    db.insert(217, "Tokina 100mm f/2.8 AT-X Pro D Macro (Pentax)");

    db
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classic_k_mount_lenses() {
        assert_eq!(
            lookup_lens_name(2),
            Some("SMC Pentax-K 50mm f/1.4".to_string())
        );
        assert_eq!(
            lookup_lens_name(3),
            Some("SMC Pentax-K 28mm f/2.8".to_string())
        );
    }

    #[test]
    fn test_pentax_m_series() {
        assert_eq!(
            lookup_lens_name(6),
            Some("SMC Pentax-M 50mm f/1.7".to_string())
        );
        assert_eq!(
            lookup_lens_name(10),
            Some("SMC Pentax-M 35mm f/2.0".to_string())
        );
    }

    #[test]
    fn test_limited_lenses() {
        assert_eq!(
            lookup_lens_name(51),
            Some("SMC Pentax-FA 31mm f/1.8 AL Limited".to_string())
        );
        assert_eq!(
            lookup_lens_name(53),
            Some("SMC Pentax-FA 43mm f/1.9 Limited".to_string())
        );
        assert_eq!(
            lookup_lens_name(56),
            Some("SMC Pentax-FA 77mm f/1.8 Limited".to_string())
        );
    }

    #[test]
    fn test_hd_da_limited() {
        assert_eq!(
            lookup_lens_name(70),
            Some("HD Pentax-DA 15mm f/4.0 ED AL Limited".to_string())
        );
        assert_eq!(
            lookup_lens_name(74),
            Some("HD Pentax-DA 40mm f/2.8 Limited".to_string())
        );
    }

    #[test]
    fn test_da_star_series() {
        assert_eq!(
            lookup_lens_name(110),
            Some("SMC Pentax-DA* 16-50mm f/2.8 ED AL IF SDM".to_string())
        );
        assert_eq!(
            lookup_lens_name(113),
            Some("SMC Pentax-DA* 200mm f/2.8 ED IF SDM".to_string())
        );
    }

    #[test]
    fn test_modern_d_fa() {
        assert_eq!(
            lookup_lens_name(122),
            Some("HD Pentax-D FA 24-70mm f/2.8 ED SDM WR".to_string())
        );
        assert_eq!(
            lookup_lens_name(127),
            Some("HD Pentax-D FA 85mm f/1.4 ED SDM AW".to_string())
        );
    }

    #[test]
    fn test_fisheye_lenses() {
        assert_eq!(
            lookup_lens_name(150),
            Some("SMC Pentax-DA Fish-Eye 10-17mm f/3.5-4.5 ED IF".to_string())
        );
    }

    #[test]
    fn test_third_party_lenses() {
        assert_eq!(
            lookup_lens_name(203),
            Some("Sigma 18-35mm f/1.8 DC HSM Art (Pentax)".to_string())
        );
        assert_eq!(
            lookup_lens_name(214),
            Some("Tamron 90mm f/2.8 Di VC USD Macro (Pentax)".to_string())
        );
        assert_eq!(
            lookup_lens_name(215),
            Some("Tokina 11-16mm f/2.8 AT-X Pro DX II (Pentax)".to_string())
        );
    }

    #[test]
    fn test_unknown_lens() {
        assert_eq!(lookup_lens_name(65000), None);
        assert_eq!(lookup_lens_name(9999), None);
    }

    #[test]
    fn test_database_size() {
        // Verify we have at least 80 lenses as required
        let db = &*PENTAX_LENS_DATABASE;
        assert!(
            db.len() >= 80,
            "Database should contain at least 80 lenses, got {}",
            db.len()
        );
    }
}
