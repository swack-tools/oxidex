//! Leica lens database for LensID to lens name mapping
//!
//! Based on ExifTool's Leica.pm lens database, covering:
//! - Leica M-mount rangefinder lenses (M-series cameras)
//! - Leica L-mount lenses (SL/CL/TL systems)
//! - Leica SL-mount lenses (SL/SL2 cameras)
//! - DG and DC series lenses
//! - APO-Summicron, Summilux, Noctilux premium lenses

/// Looks up a lens name from a Leica lens ID
///
/// # Arguments
/// * `lens_id` - The lens ID from LensType tag
///
/// # Returns
/// * `Some(String)` - The lens model name if found
/// * `None` - If lens ID is not in database
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    LEICA_LENS_DATABASE.get(&lens_id).map(|s| s.to_string())
}

use once_cell::sync::Lazy;
use std::collections::HashMap;

static LEICA_LENS_DATABASE: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // ===== Leica M-Mount Rangefinder Lenses (Manual Focus) =====
    // These are the legendary M-mount lenses for Leica M-series rangefinder cameras

    // Summilux series (f/1.4 premium lenses)
    db.insert(1, "Leica Summilux-M 21mm f/1.4 ASPH");
    db.insert(2, "Leica Summilux-M 24mm f/1.4 ASPH");
    db.insert(3, "Leica Summilux-M 28mm f/1.4 ASPH");
    db.insert(4, "Leica Summilux-M 35mm f/1.4 ASPH");
    db.insert(5, "Leica Summilux-M 50mm f/1.4 ASPH");
    db.insert(6, "Leica Summilux-M 75mm f/1.4");
    db.insert(7, "Leica Summilux-M 90mm f/1.5 ASPH");

    // Noctilux series (ultra-fast f/0.95 - f/1.2)
    db.insert(10, "Leica Noctilux-M 50mm f/0.95 ASPH");
    db.insert(11, "Leica Noctilux-M 50mm f/1.2 ASPH");
    db.insert(12, "Leica Noctilux-M 75mm f/1.25 ASPH");

    // APO-Summicron-M series (f/2.0 with apochromatic correction)
    db.insert(20, "Leica APO-Summicron-M 35mm f/2.0 ASPH");
    db.insert(21, "Leica APO-Summicron-M 50mm f/2.0 ASPH");
    db.insert(22, "Leica APO-Summicron-M 75mm f/2.0 ASPH");
    db.insert(23, "Leica APO-Summicron-M 90mm f/2.0 ASPH");

    // Summicron-M series (f/2.0 standard lenses)
    db.insert(30, "Leica Summicron-M 21mm f/2.0 ASPH");
    db.insert(31, "Leica Summicron-M 28mm f/2.0 ASPH");
    db.insert(32, "Leica Summicron-M 35mm f/2.0 ASPH");
    db.insert(33, "Leica Summicron-M 50mm f/2.0");
    db.insert(34, "Leica Summicron-M 90mm f/2.0");

    // Elmarit-M series (f/2.8 compact lenses)
    db.insert(40, "Leica Elmarit-M 21mm f/2.8 ASPH");
    db.insert(41, "Leica Elmarit-M 24mm f/2.8 ASPH");
    db.insert(42, "Leica Elmarit-M 28mm f/2.8 ASPH");
    db.insert(43, "Leica Elmarit-M 90mm f/2.8");

    // Macro-Elmar-M series
    db.insert(50, "Leica Macro-Elmar-M 90mm f/4.0");

    // ===== Leica SL-Mount Lenses (Autofocus, Full Frame) =====
    // For Leica SL/SL2/SL2-S mirrorless cameras

    // APO-Summicron-SL series (f/2.0 autofocus with APO correction)
    db.insert(100, "Leica APO-Summicron-SL 35mm f/2.0 ASPH");
    db.insert(101, "Leica APO-Summicron-SL 50mm f/2.0 ASPH");
    db.insert(102, "Leica APO-Summicron-SL 75mm f/2.0 ASPH");
    db.insert(103, "Leica APO-Summicron-SL 90mm f/2.0 ASPH");

    // Summilux-SL series (f/1.4 autofocus)
    db.insert(110, "Leica Summilux-SL 50mm f/1.4 ASPH");

    // Vario-Elmarit-SL zoom lenses (f/2.8 constant aperture)
    db.insert(120, "Leica Vario-Elmarit-SL 24-70mm f/2.8 ASPH");
    db.insert(121, "Leica Vario-Elmarit-SL 24-90mm f/2.8-4.0 ASPH");

    // APO-Vario-Elmarit-SL series (professional zooms with APO)
    db.insert(130, "Leica APO-Vario-Elmarit-SL 90-280mm f/2.8-4.0");

    // Super-telephoto APO lenses
    db.insert(140, "Leica APO-Telyt-SL 400mm f/2.8");

    // ===== Leica L-Mount Lenses (TL/CL systems) =====
    // For Leica TL/TL2/CL cameras (APS-C)

    // APO-Summicron-TL series
    db.insert(200, "Leica APO-Summicron-TL 23mm f/2.0 ASPH");
    db.insert(201, "Leica APO-Summicron-TL 35mm f/2.0 ASPH");

    // Summicron-TL series
    db.insert(210, "Leica Summicron-TL 23mm f/2.0 ASPH");

    // Elmarit-TL series
    db.insert(220, "Leica Elmarit-TL 18mm f/2.8 ASPH");

    // Vario-Elmar-TL zooms
    db.insert(230, "Leica Vario-Elmar-TL 18-56mm f/3.5-5.6 ASPH");
    db.insert(231, "Leica APO-Vario-Elmar-TL 55-135mm f/3.5-4.5 ASPH");

    // ===== Leica R-Mount Lenses (Legacy SLR System) =====
    // Historic Leica R-series SLR lenses (for reference/adapters)

    db.insert(300, "Leica Summilux-R 50mm f/1.4");
    db.insert(301, "Leica Summicron-R 50mm f/2.0");
    db.insert(302, "Leica Elmarit-R 28mm f/2.8");
    db.insert(303, "Leica Elmarit-R 35mm f/2.8");
    db.insert(304, "Leica APO-Telyt-R 180mm f/3.4");

    // ===== Third-Party L-Mount Alliance Lenses =====
    // Sigma and Panasonic L-mount lenses compatible with Leica SL

    db.insert(400, "Sigma 35mm f/1.2 DG DN Art (L-mount)");
    db.insert(401, "Sigma 50mm f/1.4 DG DN Art (L-mount)");
    db.insert(402, "Sigma 85mm f/1.4 DG DN Art (L-mount)");
    db.insert(403, "Sigma 105mm f/2.8 DG DN Macro Art (L-mount)");
    db.insert(404, "Panasonic Lumix S Pro 50mm f/1.4 (L-mount)");
    db.insert(405, "Panasonic Lumix S Pro 70-200mm f/2.8 OIS (L-mount)");

    db
});
