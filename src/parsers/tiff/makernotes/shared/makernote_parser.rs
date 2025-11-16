use std::collections::HashMap;
use crate::parsers::tiff::ifd_parser::ByteOrder;

/// Common trait for all MakerNotes parsers
///
/// Each manufacturer implements this trait to provide consistent
/// parsing interface across all brands.
pub trait MakerNoteParser {
    /// Returns the manufacturer identifier (e.g., "Canon", "Nikon", "Apple")
    fn manufacturer_name(&self) -> &'static str;

    /// Returns the tag namespace prefix (e.g., "Canon:", "Nikon:", "Apple:")
    fn tag_prefix(&self) -> &'static str;

    /// Parse MakerNote data and extract tags
    ///
    /// # Arguments
    /// * `data` - Raw MakerNote data bytes
    /// * `byte_order` - Byte order for multi-byte values
    /// * `tags` - HashMap to insert extracted tags into
    ///
    /// # Returns
    /// Ok(()) on success, Err(message) on failure
    fn parse(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        tags: &mut HashMap<String, String>
    ) -> Result<(), String>;

    /// Optional: Validate that this data belongs to this manufacturer
    ///
    /// Some manufacturers have header signatures (e.g., "Nikon\0\0")
    /// Default implementation accepts all data.
    fn validate_header(&self, data: &[u8]) -> bool {
        let _ = data; // Suppress unused parameter warning
        true
    }

    /// Optional: Lens database lookup (if manufacturer has lens IDs)
    ///
    /// Returns lens name for given lens ID, or None if:
    /// - Manufacturer doesn't use lens IDs
    /// - Lens ID not found in database
    fn lookup_lens(&self, lens_id: u16) -> Option<String> {
        let _ = lens_id;
        None
    }
}
