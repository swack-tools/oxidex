//! Sigma lens database for LensID to lens name mapping
//!
//! Based on ExifTool's Sigma.pm lens database, covering:
//! - Sigma Art series (high-performance prime and zoom lenses)
//! - Sigma Contemporary series (compact, versatile lenses)
//! - Sigma Sports series (telephoto lenses for action photography)
//! - Legacy SA-mount lenses
//! - DG DN mirrorless lenses (Sony E, Leica L-mount)

/// Looks up a lens name from a Sigma lens ID
///
/// # Arguments
/// * `lens_id` - The lens ID from LensType tag
///
/// # Returns
/// * `Some(String)` - The lens model name if found
/// * `None` - If lens ID is not in database
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    SIGMA_LENS_DATABASE.get(&lens_id).map(|s| s.to_string())
}

use once_cell::sync::Lazy;
use std::collections::HashMap;

static SIGMA_LENS_DATABASE: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // ===== Sigma Art Series Primes (High-Performance Fixed Focal Length) =====
    // The Art series represents Sigma's premium optics with exceptional sharpness

    // Art series wide-angle primes
    db.insert(1, "Sigma 14mm f/1.8 DG HSM Art");
    db.insert(2, "Sigma 20mm f/1.4 DG HSM Art");
    db.insert(3, "Sigma 24mm f/1.4 DG HSM Art");
    db.insert(4, "Sigma 28mm f/1.4 DG HSM Art");
    db.insert(5, "Sigma 35mm f/1.2 DG DN Art");
    db.insert(6, "Sigma 35mm f/1.4 DG HSM Art");
    db.insert(7, "Sigma 40mm f/1.4 DG HSM Art");

    // Art series standard and portrait primes
    db.insert(10, "Sigma 50mm f/1.4 DG HSM Art");
    db.insert(11, "Sigma 50mm f/1.4 DG DN Art");
    db.insert(12, "Sigma 65mm f/2.0 DG DN Contemporary");
    db.insert(13, "Sigma 85mm f/1.4 DG HSM Art");
    db.insert(14, "Sigma 85mm f/1.4 DG DN Art");
    db.insert(15, "Sigma 105mm f/1.4 DG HSM Art");
    db.insert(16, "Sigma 135mm f/1.8 DG HSM Art");

    // Art series macro lenses
    db.insert(20, "Sigma 70mm f/2.8 DG Macro Art");
    db.insert(21, "Sigma 105mm f/2.8 DG DN Macro Art");

    // ===== Sigma Art Series Zooms =====
    // Professional zoom lenses with constant aperture

    db.insert(30, "Sigma 14-24mm f/2.8 DG HSM Art");
    db.insert(31, "Sigma 18-35mm f/1.8 DC HSM Art");
    db.insert(32, "Sigma 24-35mm f/2.0 DG HSM Art");
    db.insert(33, "Sigma 24-70mm f/2.8 DG OS HSM Art");
    db.insert(34, "Sigma 24-70mm f/2.8 DG DN Art");
    db.insert(35, "Sigma 50-100mm f/1.8 DC HSM Art");
    db.insert(36, "Sigma 60-600mm f/4.5-6.3 DG OS HSM Sports");
    db.insert(37, "Sigma 70-200mm f/2.8 DG OS HSM Sports");

    // ===== Sigma Contemporary Series =====
    // Compact, lightweight lenses balancing performance and portability

    db.insert(50, "Sigma 16mm f/1.4 DC DN Contemporary");
    db.insert(51, "Sigma 23mm f/1.4 DC DN Contemporary");
    db.insert(52, "Sigma 30mm f/1.4 DC DN Contemporary");
    db.insert(53, "Sigma 56mm f/1.4 DC DN Contemporary");
    db.insert(54, "Sigma 17-70mm f/2.8-4.0 DC Macro OS HSM Contemporary");
    db.insert(55, "Sigma 18-50mm f/2.8 DC DN Contemporary");
    db.insert(56, "Sigma 28-70mm f/2.8 DG DN Contemporary");
    db.insert(57, "Sigma 100-400mm f/5.0-6.3 DG OS HSM Contemporary");
    db.insert(58, "Sigma 150-600mm f/5.0-6.3 DG OS HSM Contemporary");

    // ===== Sigma Sports Series =====
    // Telephoto lenses optimized for action and wildlife photography

    db.insert(70, "Sigma 120-300mm f/2.8 DG OS HSM Sports");
    db.insert(71, "Sigma 150-600mm f/5.0-6.3 DG OS HSM Sports");
    db.insert(72, "Sigma 500mm f/4.0 DG OS HSM Sports");
    db.insert(73, "Sigma 60-600mm f/4.5-6.3 DG DN OS Sports");

    // ===== Legacy SA-Mount Lenses (for Sigma SD cameras) =====
    // Classic Sigma lenses for the SA mount system

    db.insert(100, "Sigma 8-16mm f/4.5-5.6 DC HSM");
    db.insert(101, "Sigma 10-20mm f/3.5 EX DC HSM");
    db.insert(102, "Sigma 17-50mm f/2.8 EX DC OS HSM");
    db.insert(103, "Sigma 17-70mm f/2.8-4.0 DC Macro OS HSM");
    db.insert(104, "Sigma 18-125mm f/3.8-5.6 DC OS HSM");
    db.insert(105, "Sigma 18-200mm f/3.5-6.3 DC Macro OS HSM");
    db.insert(106, "Sigma 18-250mm f/3.5-6.3 DC Macro OS HSM");
    db.insert(107, "Sigma 50-500mm f/4.5-6.3 APO DG OS HSM");
    db.insert(108, "Sigma 70-300mm f/4.0-5.6 DG Macro");

    // Legacy SA-mount primes
    db.insert(120, "Sigma 8mm f/3.5 EX DG Circular Fisheye");
    db.insert(121, "Sigma 10mm f/2.8 EX DC Fisheye HSM");
    db.insert(122, "Sigma 15mm f/2.8 EX DG Diagonal Fisheye");
    db.insert(123, "Sigma 30mm f/1.4 EX DC HSM");
    db.insert(124, "Sigma 50mm f/2.8 EX DG Macro");
    db.insert(125, "Sigma 180mm f/2.8 EX DG OS HSM APO Macro");
    db.insert(126, "Sigma 300mm f/2.8 EX DG HSM");

    // ===== Sigma DG DN Series (Mirrorless Full Frame) =====
    // Modern mirrorless mount lenses (Sony E, Leica L)

    db.insert(150, "Sigma 14-24mm f/2.8 DG DN Art");
    db.insert(151, "Sigma 20mm f/2.0 DG DN Contemporary");
    db.insert(152, "Sigma 24mm f/2.0 DG DN Contemporary");
    db.insert(153, "Sigma 24mm f/3.5 DG DN Contemporary");
    db.insert(154, "Sigma 35mm f/2.0 DG DN Contemporary");
    db.insert(155, "Sigma 45mm f/2.8 DG DN Contemporary");
    db.insert(156, "Sigma 65mm f/2.0 DG DN Contemporary");
    db.insert(157, "Sigma 90mm f/2.8 DG DN Contemporary");

    db
});
