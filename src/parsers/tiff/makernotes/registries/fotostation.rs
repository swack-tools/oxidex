//! FotoStation/FotoWare tag registry
//!
//! This module provides TagRegistry definitions for FotoStation MakerNotes.
//! FotoStation is a professional digital asset management (DAM) system
//! with comprehensive workflow and categorization metadata.

use super::super::shared::tag_registry::TagRegistry;
use crate::const_decoder;

// ============================================================================
// Declarative Decoder Definitions
// ============================================================================

// Decodes workflow status
const_decoder!(
    DECODE_WORKFLOW_STATUS,
    i16,
    [
        (0, "New"),
        (1, "In Progress"),
        (2, "Pending Review"),
        (3, "Approved"),
        (4, "Rejected"),
        (5, "Published"),
        (6, "Archived"),
        (7, "Expired"),
    ]
);

// Decodes approval status
const_decoder!(
    DECODE_APPROVAL_STATUS,
    i16,
    [
        (0, "Pending"),
        (1, "Approved"),
        (2, "Rejected"),
        (3, "Needs Revision"),
        (4, "Final"),
    ]
);

// Decodes publication status
const_decoder!(
    DECODE_PUBLICATION_STATUS,
    i16,
    [
        (0, "Unpublished"),
        (1, "Published"),
        (2, "Scheduled"),
        (3, "Retracted"),
    ]
);

// Decodes rights status
const_decoder!(
    DECODE_RIGHTS_STATUS,
    i16,
    [
        (0, "Unknown"),
        (1, "Rights Managed"),
        (2, "Royalty Free"),
        (3, "Rights Reserved"),
        (4, "Public Domain"),
        (5, "Creative Commons"),
    ]
);

// Decodes usage rights level
const_decoder!(
    DECODE_USAGE_RIGHTS,
    i16,
    [
        (0, "No Restrictions"),
        (1, "Internal Use Only"),
        (2, "Editorial Use"),
        (3, "Commercial Use"),
        (4, "Limited Use"),
        (5, "Restricted"),
    ]
);

// Decodes release status
const_decoder!(
    DECODE_RELEASE_STATUS,
    i16,
    [
        (0, "Not Required"),
        (1, "Not Available"),
        (2, "On File"),
        (3, "Pending"),
    ]
);

// ============================================================================
// Tag Registry Factory Function
// ============================================================================

/// Create FotoStation tag registry with all tag definitions
///
/// This registry provides declarative definitions of all FotoStation MakerNote tags
/// including workflow status, approval, publication, archiving, rights management,
/// taxonomy, versioning, and batch processing metadata.
///
/// # Returns
/// A fully configured TagRegistry ready for FotoStation MakerNote parsing
pub fn fotostation_registry() -> TagRegistry {
    TagRegistry::new()
        // Version
        .register_raw(0x0001, "Version")
        // Workflow and approval
        .register_simple_i16(0x0010, "WorkflowStatus", &DECODE_WORKFLOW_STATUS)
        .register_simple_i16(0x0011, "ApprovalStatus", &DECODE_APPROVAL_STATUS)
        .register_simple_i16(0x0012, "PublicationStatus", &DECODE_PUBLICATION_STATUS)
        // Archive and location
        .register_raw(0x0020, "ArchiveLocation")
        .register_raw(0x0021, "Category")
        .register_raw(0x0022, "Subcategory")
        .register_raw(0x0023, "CollectionName")
        .register_raw(0x0024, "ArchiveID")
        // Rights management
        .register_simple_i16(0x0030, "RightsStatus", &DECODE_RIGHTS_STATUS)
        .register_simple_i16(0x0031, "UsageRights", &DECODE_USAGE_RIGHTS)
        .register_raw(0x0032, "ExpirationDate")
        .register_simple_i16(0x0033, "ReleaseStatus", &DECODE_RELEASE_STATUS)
        // Taxonomy and controlled vocabularies
        .register_raw(0x0040, "TaxonomyLevel1")
        .register_raw(0x0041, "TaxonomyLevel2")
        .register_raw(0x0042, "TaxonomyLevel3")
        .register_raw(0x0043, "ControlledVocabulary")
        // Custom fields
        .register_raw(0x0050, "CustomField1")
        .register_raw(0x0051, "CustomField2")
        .register_raw(0x0052, "CustomField3")
        // Versioning
        .register_raw(0x0060, "VersionNumber")
        .register_raw(0x0061, "VersionComment")
        .register_raw(0x0062, "CheckedOutBy")
        .register_raw(0x0063, "CheckedOutDate")
        // Batch processing
        .register_raw(0x0070, "BatchID")
        .register_raw(0x0071, "Operator")
        .register_raw(0x0072, "StationName")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_creation() {
        let registry = fotostation_registry();
        assert!(registry.has_tag(0x0001)); // Version
        assert!(registry.has_tag(0x0010)); // WorkflowStatus
        assert!(registry.has_tag(0x0030)); // RightsStatus
    }

    #[test]
    fn test_tag_names() {
        let registry = fotostation_registry();
        assert_eq!(registry.get_tag_name(0x0001), Some("Version"));
        assert_eq!(registry.get_tag_name(0x0010), Some("WorkflowStatus"));
        assert_eq!(registry.get_tag_name(0x0030), Some("RightsStatus"));
    }

    #[test]
    fn test_decoders() {
        assert_eq!(DECODE_WORKFLOW_STATUS.decode(0), "New");
        assert_eq!(DECODE_WORKFLOW_STATUS.decode(3), "Approved");
        assert_eq!(DECODE_APPROVAL_STATUS.decode(1), "Approved");
        assert_eq!(DECODE_RIGHTS_STATUS.decode(2), "Royalty Free");
    }
}
