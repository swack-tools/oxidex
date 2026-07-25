//! Photo Mechanic tag registry
//!
//! This module provides TagRegistry definitions for Photo Mechanic MakerNotes.
//! Photo Mechanic is a professional photo browser and workflow tool with
//! comprehensive IPTC-compatible metadata support.

use super::super::shared::tag_registry::TagRegistry;
use crate::const_decoder;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

// Decodes color class
const_decoder!(
    DECODE_COLOR_CLASS,
    i16,
    [
        (0, "None"),
        (1, "Red"),
        (2, "Yellow"),
        (3, "Green"),
        (4, "Blue"),
        (5, "Purple"),
        (6, "Orange"),
        (7, "Gray"),
        (8, "White"),
    ]
);

// Decodes urgency level
const_decoder!(
    DECODE_URGENCY,
    i16,
    [
        (1, "High (1)"),
        (2, "2"),
        (3, "3"),
        (4, "4"),
        (5, "Normal (5)"),
        (6, "6"),
        (7, "7"),
        (8, "Low (8)"),
    ]
);

// Decodes edit status
const_decoder!(
    DECODE_EDIT_STATUS,
    i16,
    [
        (0, "Original"),
        (1, "Edited"),
        (2, "Selected"),
        (3, "Rejected"),
        (4, "For Review"),
        (5, "Approved"),
    ]
);

// ============================================================================
// Tag Registry Factory Function
// ============================================================================

/// Create Photo Mechanic tag registry with all tag definitions
///
/// This registry provides declarative definitions of all Photo Mechanic MakerNote tags
/// including IPTC workflow, ratings, keywords, location, contact, and usage terms.
///
/// # Returns
/// A fully configured TagRegistry ready for Photo Mechanic MakerNote parsing
pub fn photomechanic_registry() -> TagRegistry {
    TagRegistry::new()
        // Version
        .register_raw(0x0001, "Version")
        // Rating and tagging
        .register_raw(0x0010, "Rating")
        .register_simple_i16(0x0011, "ColorClass", &DECODE_COLOR_CLASS)
        .register_raw(0x0012, "Tagged")
        // IPTC content
        .register_raw(0x0020, "Caption")
        .register_raw(0x0021, "Headline")
        .register_raw(0x0022, "Keywords")
        .register_raw(0x0023, "Category")
        .register_raw(0x0024, "SupplementalCategories")
        // Copyright and credit
        .register_raw(0x0030, "CopyrightNotice")
        .register_raw(0x0031, "Credit")
        .register_raw(0x0032, "ByLine")
        .register_raw(0x0033, "ByLineTitle")
        .register_raw(0x0034, "Source")
        .register_raw(0x0035, "ObjectName")
        // Location information
        .register_raw(0x0040, "City")
        .register_raw(0x0041, "ProvinceState")
        .register_raw(0x0042, "CountryName")
        .register_raw(0x0043, "CountryCode")
        .register_raw(0x0044, "SubLocation")
        // Subject information
        .register_raw(0x0050, "PersonShown")
        .register_raw(0x0051, "Event")
        .register_raw(0x0052, "SubjectCode")
        // Transmission information
        .register_raw(0x0060, "SpecialInstructions")
        .register_raw(0x0061, "TransmissionReference")
        .register_simple_i16(0x0062, "Urgency", &DECODE_URGENCY)
        // Job information
        .register_raw(0x0070, "JobID")
        .register_simple_i16(0x0071, "EditStatus", &DECODE_EDIT_STATUS)
        .register_raw(0x0072, "FixtureID")
        // Contact information
        .register_raw(0x0080, "Contact")
        .register_raw(0x0081, "CreatorWebsite")
        .register_raw(0x0082, "CreatorEmail")
        .register_raw(0x0083, "CreatorPhone")
        // Rights and usage
        .register_raw(0x0090, "UsageTerms")
        .register_raw(0x0091, "WebStatementURL")
        // Ingestion and stationery
        .register_raw(0x00A0, "IngestionTime")
        .register_raw(0x00B0, "CodeReplacementApplied")
        .register_raw(0x00B1, "StructuredKeywordCount")
        .register_raw(0x00C0, "StationeryApplied")
        .register_raw(0x00C1, "StationeryName")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = photomechanic_registry();
        assert!(registry.has_tag(0x0001)); // Version
        assert!(registry.has_tag(0x0010)); // Rating
        assert!(registry.has_tag(0x0011)); // ColorClass
    }

    #[test]
    fn test_tag_names() {
        let registry = photomechanic_registry();
        assert_eq!(registry.get_tag_name(0x0001), Some("Version"));
        assert_eq!(registry.get_tag_name(0x0010), Some("Rating"));
        assert_eq!(registry.get_tag_name(0x0011), Some("ColorClass"));
    }

    #[test]
    fn test_decoders() {
        assert_eq!(DECODE_COLOR_CLASS.decode(1), "Red");
        assert_eq!(DECODE_COLOR_CLASS.decode(3), "Green");
        assert_eq!(DECODE_URGENCY.decode(5), "Normal (5)");
        assert_eq!(DECODE_EDIT_STATUS.decode(5), "Approved");
    }
}
