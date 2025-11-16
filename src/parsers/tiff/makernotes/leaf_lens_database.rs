//! Leaf Lens Database
//!
//! Database of Leaf medium format lens IDs and their corresponding names.
//! Leaf Digital Backs were used with various medium format camera systems
//! primarily from Mamiya and Contax.
//!
//! ## Coverage
//! - Mamiya 645 AF/AFD lenses
//! - Contax 645 lenses
//! - Popular medium format lenses (~20 entries)

#![allow(dead_code)]

/// Lookup Leaf lens name by lens ID
///
/// # Arguments
/// * `lens_id` - Leaf lens ID from MakerNote
///
/// # Returns
/// Lens name if found in database, None otherwise
pub fn lookup_leaf_lens(lens_id: u16) -> Option<String> {
    let lens_name = match lens_id {
        // Mamiya 645 AF/AFD Lenses
        0x0100 => "Mamiya AF 35mm f/3.5",
        0x0101 => "Mamiya AF 45mm f/2.8",
        0x0102 => "Mamiya AF 55mm f/2.8",
        0x0103 => "Mamiya AF 80mm f/2.8",
        0x0104 => "Mamiya AF 110mm f/2.8",
        0x0105 => "Mamiya AF 120mm f/4 Macro",
        0x0106 => "Mamiya AF 150mm f/2.8",
        0x0107 => "Mamiya AF 210mm f/4",
        0x0108 => "Mamiya AF 300mm f/2.8 APO",

        // Mamiya 645 AF/AFD Zoom Lenses
        0x0200 => "Mamiya AF 35-70mm f/4.5",
        0x0201 => "Mamiya AF 55-110mm f/4.5",
        0x0202 => "Mamiya AF 70-210mm f/4.5",

        // Contax 645 Lenses
        0x0300 => "Contax 645 35mm f/3.5",
        0x0301 => "Contax 645 45mm f/2.8",
        0x0302 => "Contax 645 80mm f/2.0",
        0x0303 => "Contax 645 120mm f/4 Macro",
        0x0304 => "Contax 645 140mm f/2.8",
        0x0305 => "Contax 645 210mm f/4",

        // Schneider Leaf Shutter Lenses
        0x0400 => "Schneider 80mm f/2.8 LS",
        0x0401 => "Schneider 110mm f/2.8 LS",
        0x0402 => "Schneider 150mm f/2.8 LS",

        _ => return None,
    };

    Some(lens_name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mamiya_prime_lenses() {
        assert_eq!(
            lookup_leaf_lens(0x0103),
            Some("Mamiya AF 80mm f/2.8".to_string())
        );
        assert_eq!(
            lookup_leaf_lens(0x0106),
            Some("Mamiya AF 150mm f/2.8".to_string())
        );
    }

    #[test]
    fn test_mamiya_zoom_lenses() {
        assert_eq!(
            lookup_leaf_lens(0x0200),
            Some("Mamiya AF 35-70mm f/4.5".to_string())
        );
    }

    #[test]
    fn test_contax_lenses() {
        assert_eq!(
            lookup_leaf_lens(0x0302),
            Some("Contax 645 80mm f/2.0".to_string())
        );
    }

    #[test]
    fn test_schneider_lenses() {
        assert_eq!(
            lookup_leaf_lens(0x0400),
            Some("Schneider 80mm f/2.8 LS".to_string())
        );
    }

    #[test]
    fn test_unknown_lens() {
        assert_eq!(lookup_leaf_lens(0xFFFF), None);
    }
}
