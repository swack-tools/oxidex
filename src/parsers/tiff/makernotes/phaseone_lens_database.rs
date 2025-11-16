//! Phase One lens database for LensID to lens name mapping
//!
//! Based on ExifTool's PhaseOne.pm lens database, covering:
//! - Schneider Kreuznach lenses (Phase One's premium lens partner)
//! - Mamiya lenses (medium format heritage)
//! - Rodenstock lenses (large format technical photography)
//! - Phase One Blue Ring series (latest generation)

/// Looks up a lens name from a Phase One lens ID
///
/// # Arguments
/// * `lens_id` - The lens ID from LensType tag
///
/// # Returns
/// * `Some(String)` - The lens model name if found
/// * `None` - If lens ID is not in database
pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    PHASEONE_LENS_DATABASE.get(&lens_id).map(|s| s.to_string())
}

use once_cell::sync::Lazy;
use std::collections::HashMap;

static PHASEONE_LENS_DATABASE: Lazy<HashMap<u16, &'static str>> = Lazy::new(|| {
    let mut db = HashMap::new();

    // ===== Schneider Kreuznach Lenses (Phase One Partnership) =====
    // Premium German optics designed specifically for Phase One medium format systems

    // Schneider Kreuznach wide-angle lenses
    db.insert(1, "Schneider Kreuznach 28mm f/4.5 LS");
    db.insert(2, "Schneider Kreuznach 35mm f/3.5 LS");
    db.insert(3, "Schneider Kreuznach 40mm f/4.0 LS");
    db.insert(4, "Schneider Kreuznach 45mm f/3.5 LS");

    // Schneider Kreuznach standard and portrait lenses
    db.insert(10, "Schneider Kreuznach 55mm f/2.8 LS");
    db.insert(11, "Schneider Kreuznach 80mm f/2.8 LS");
    db.insert(12, "Schneider Kreuznach 110mm f/2.8 LS");
    db.insert(13, "Schneider Kreuznach 120mm f/4.0 Macro LS");
    db.insert(14, "Schneider Kreuznach 150mm f/2.8 LS");
    db.insert(15, "Schneider Kreuznach 150mm f/3.5 LS");

    // Schneider Kreuznach telephoto lenses
    db.insert(20, "Schneider Kreuznach 240mm f/4.5 LS");

    // ===== Mamiya Medium Format Lenses =====
    // Classic Mamiya 645 and RZ67 lenses compatible with Phase One digital backs

    db.insert(30, "Mamiya Sekor 35mm f/3.5");
    db.insert(31, "Mamiya Sekor 45mm f/2.8");
    db.insert(32, "Mamiya Sekor 55mm f/2.8");
    db.insert(33, "Mamiya Sekor 80mm f/1.9");
    db.insert(34, "Mamiya Sekor 80mm f/2.8 LS D");
    db.insert(35, "Mamiya Sekor 110mm f/2.8");
    db.insert(36, "Mamiya Sekor 120mm f/4.0 Macro D");
    db.insert(37, "Mamiya Sekor 150mm f/2.8");
    db.insert(38, "Mamiya Sekor 210mm f/4.0");
    db.insert(39, "Mamiya Sekor 300mm f/2.8 APO");

    // Mamiya zoom lenses
    db.insert(45, "Mamiya Sekor 55-110mm f/4.5");
    db.insert(46, "Mamiya Sekor 75-150mm f/4.5");

    // ===== Rodenstock Lenses (Technical Photography) =====
    // High-end view camera lenses adapted for Phase One

    db.insert(50, "Rodenstock HR Digaron 23mm f/5.6");
    db.insert(51, "Rodenstock HR Digaron 32mm f/4.0");
    db.insert(52, "Rodenstock HR Digaron 40mm f/4.0");
    db.insert(53, "Rodenstock HR Digaron 50mm f/4.0");
    db.insert(54, "Rodenstock HR Digaron 60mm f/4.0");
    db.insert(55, "Rodenstock HR Digaron 70mm f/5.6");

    // ===== Phase One Blue Ring Series =====
    // Latest generation lenses with distinctive blue ring marking

    db.insert(60, "Phase One Blue Ring 23mm");
    db.insert(61, "Phase One Blue Ring 28mm");
    db.insert(62, "Phase One Blue Ring 35mm LS");
    db.insert(63, "Phase One Blue Ring 45mm");
    db.insert(64, "Phase One Blue Ring 55mm LS");
    db.insert(65, "Phase One Blue Ring 80mm LS");
    db.insert(66, "Phase One Blue Ring 110mm Macro LS");
    db.insert(67, "Phase One Blue Ring 150mm LS");

    // ===== Leaf Shutter (LS) variants =====
    // Special versions with built-in leaf shutters for flash sync at all speeds

    db.insert(70, "Phase One 80mm f/2.8 AF LS");
    db.insert(71, "Phase One 110mm f/2.8 AF LS");

    db
});
