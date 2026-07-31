use crate::parsers::tiff::ifd_parser::ByteOrder;
use std::collections::HashMap;

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
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String>;

    /// Parse MakerNote data when the camera model is known.
    ///
    /// A few manufacturers key structural decisions off the model rather than
    /// off anything inside the MakerNote itself -- Nikon, for instance, reads
    /// `AFInfo`'s 16-bit field big-endian on `NIKON D*` bodies and
    /// little-endian everywhere else. Parsers that do not care ignore `model`,
    /// which is what this default does.
    ///
    /// # Arguments
    /// * `data` - Raw MakerNote data bytes
    /// * `byte_order` - Byte order for multi-byte values
    /// * `model` - Camera model string (EXIF `Model`), if it was available
    /// * `tags` - HashMap to insert extracted tags into
    fn parse_with_model(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        let _ = model;
        self.parse(data, byte_order, tags)
    }

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
