use crate::core::TagOccurrence;
use crate::exiftool_tables::Ctx;
use crate::exiftool_tables::session::Session;
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::parsers::tiff::makernotes::makernote_context::MakerNoteContext;
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

    /// Parses with a known model and returns private full-precision value
    /// forms separately from the displayed tag strings.
    fn parse_with_model_and_values(
        &self,
        data: &[u8],
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
        value_forms: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        let _ = value_forms;
        self.parse_with_model(data, byte_order, model, tags)
    }

    /// Parse a MakerNote whose position inside its enclosing TIFF block is
    /// known.
    ///
    /// A MakerNote's value offsets are measured from the enclosing TIFF header,
    /// not from the MakerNote payload, and they routinely address bytes past
    /// the payload's declared end -- `NEFBitDepth`'s eight bytes begin four
    /// short of the end of `NikonCoolpixS8200.jpg`'s 2219-byte MakerNote, and a
    /// Sigma value offset addresses the TIFF header outright. A decoder handed
    /// only the payload cannot reach any of it, however correct the decoder is.
    ///
    /// [`MakerNoteContext`] carries the enclosing block, the payload's position
    /// inside it, and the block's own file offset, so an implementor can decide
    /// what it needs:
    ///
    /// * [`MakerNoteContext::payload`] -- the declared block, which is what
    ///   this default passes on, so overriding nothing changes nothing;
    /// * [`MakerNoteContext::window`] -- the same start extended to the end of
    ///   the enclosing block, which resolves out-of-block values without
    ///   touching a decoder's offset arithmetic; and
    /// * [`MakerNoteContext::tiff`] with
    ///   [`payload_offset`](MakerNoteContext::payload_offset) -- for offsets
    ///   measured from the TIFF header rather than from the payload.
    ///
    /// Reads stay bounded either way: the context can only ever produce slices
    /// inside the enclosing TIFF block, and
    /// [`value_overlaps_directory`](crate::parsers::tiff::makernotes::makernote_context::value_overlaps_directory)
    /// is ExifTool's test for the offsets that must be refused rather than
    /// followed.
    fn parse_with_context(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        self.parse_with_model(ctx.payload(), byte_order, model, tags)
    }

    /// Parses while returning any full-precision `ValueConv` forms separately
    /// from the displayed tag strings.
    ///
    /// Most MakerNote parsers need only their displayed strings and inherit
    /// this default. The explicit output parameter avoids storing parse state
    /// in otherwise stateless parser objects.
    fn parse_with_context_and_values(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        tags: &mut HashMap<String, String>,
        value_forms: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        let _ = value_forms;
        self.parse_with_context(ctx, byte_order, model, tags)
    }

    /// The file-scoped-session form used by the EXIF bridge. Parsers without
    /// generated IFD walks ignore the runtime state and retain their existing
    /// implementation; generated Canon/FujiFilm/Olympus adapters override it
    /// so every directory in the file observes one ExifTool `$self`.
    #[allow(clippy::too_many_arguments)]
    fn parse_with_context_and_values_and_session(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        session: &mut Session,
        cond_ctx: &mut Ctx<'_>,
        tags: &mut HashMap<String, String>,
        value_forms: &mut HashMap<String, String>,
    ) -> Result<(), String> {
        let _ = (session, cond_ctx);
        self.parse_with_context_and_values(ctx, byte_order, model, tags, value_forms)
    }

    /// Session-aware parsing with an optional canonical occurrence channel.
    ///
    /// Existing parsers keep their legacy map behavior through this default.
    /// A parser that can retain source identity and all value forms overrides
    /// it and appends `(public lookup key, occurrence)` rows in traversal
    /// order without also projecting those rows into `tags`.
    #[allow(clippy::too_many_arguments)]
    fn parse_with_context_and_values_and_session_and_occurrences(
        &self,
        ctx: &MakerNoteContext<'_>,
        byte_order: ByteOrder,
        model: Option<&str>,
        session: &mut Session,
        cond_ctx: &mut Ctx<'_>,
        tags: &mut HashMap<String, String>,
        value_forms: &mut HashMap<String, String>,
        occurrences: &mut Vec<(String, TagOccurrence)>,
    ) -> Result<(), String> {
        let _ = occurrences;
        self.parse_with_context_and_values_and_session(
            ctx,
            byte_order,
            model,
            session,
            cond_ctx,
            tags,
            value_forms,
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    struct LegacyParser;

    impl MakerNoteParser for LegacyParser {
        fn manufacturer_name(&self) -> &'static str {
            "Legacy"
        }

        fn tag_prefix(&self) -> &'static str {
            "Legacy:"
        }

        fn parse(
            &self,
            _data: &[u8],
            _byte_order: ByteOrder,
            tags: &mut HashMap<String, String>,
        ) -> Result<(), String> {
            tags.insert("Legacy:Value".to_string(), "unchanged".to_string());
            Ok(())
        }
    }

    #[test]
    fn structured_default_preserves_legacy_map_parser() {
        let mut session = Session::new();
        let mut members = HashMap::new();
        let mut cond_ctx = Ctx::new(&mut members);
        let mut tags = HashMap::new();
        let mut values = HashMap::new();
        let mut occurrences = Vec::new();

        LegacyParser
            .parse_with_context_and_values_and_session_and_occurrences(
                &MakerNoteContext::detached(&[]),
                ByteOrder::LittleEndian,
                None,
                &mut session,
                &mut cond_ctx,
                &mut tags,
                &mut values,
                &mut occurrences,
            )
            .expect("legacy parser still succeeds through the structured default");

        assert_eq!(
            tags.get("Legacy:Value").map(String::as_str),
            Some("unchanged")
        );
        assert!(values.is_empty());
        assert!(occurrences.is_empty());
    }
}
