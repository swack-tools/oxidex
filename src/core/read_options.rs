//! Step 21: request-aware read gating.
//!
//! Models ExifTool's own request machinery: `REQ_TAG_LOOKUP`
//! (`ExifTool.pm:5157-5170`, built from `REQUESTED_TAGS`/`RequestTags`) is
//! consulted before an expensive or diagnostic-only tag is computed at all,
//! not filtered out after the fact. The clearest example is JPEG's DQT
//! handler (`ExifTool.pm:7682-7692`):
//!
//! ```perl
//! } elsif ($marker == 0xdb and length($$segDataPt) and    # DQT
//!     # save the DQT data only if JPEGDigest has been requested
//!     # (Note: since we aren't checking the API RequestAll option here, the
//!     #  application must use the RequestTags option to generate these tags
//!     #  if they have not been specifically requested. The reason is that
//!     #  there is too much overhead involved in the calculation of this tag
//!     #  to make this worth the CPU time.)
//!     ($$req{jpegdigest} or $$req{jpegqualityestimate}
//!     or ($$options{RequestAll} and $$options{RequestAll} > 2)))
//! {
//!     my $num = unpack('C',$$segDataPt) & 0x0f;   # get table index
//!     $dqt[$num] = $$segDataPt if $num < 4;       # save for hash calculation
//! }
//! ```
//!
//! `$$req{jpegqualityestimate}` is exactly `REQ_TAG_LOOKUP{jpegqualityestimate}`,
//! populated from `lc($2)` on every requested tag's bare name
//! (`ExifTool.pm:5164-5166`). oxidex had no equivalent at all: `File:
//! JPEGQualityEstimate` was computed and inserted on every JPEG read,
//! unconditionally -- see the (now superseded) note in
//! `tests/integration/KNOWN_DISCREPANCIES.md` and AGENTS.md's
//! output-contract/8.4. [`ReadOptions`] is that missing lookup.
//!
//! # Two different gates, not one
//!
//! This step closes two different defects that both manifest as "oxidex
//! shows a tag ExifTool's default output does not", and they are gated
//! differently on purpose:
//!
//! * **`JPEGQualityEstimate`** is a real ExifTool tag that a real
//!   `-JPEGQualityEstimate` request reveals (confirmed against the pinned
//!   oracle: `-s -JPEGQualityEstimate` on `t/images/Canon.jpg` returns `61`,
//!   while plain `-j`/`-a` returns none of it). This is computed
//!   unconditionally today; [`ReadOptions::should_emit`] is the same
//!   `$$req{...} or extended` check ExifTool.pm makes above, applied at the
//!   same point -- before `process_dqt_segments` even collects the DQT
//!   payload, in `src/core/jpeg_helpers.rs`.
//! * The other three categories this step moves out of default output --
//!   ten JPEG SOF-derived diagnostic tags (`src/parsers/jpeg/app_parsers.rs`),
//!   the undecoded MakerNote hex-fallback (`ExifIFD:0x927C` and its kin
//!   across every IFD, `src/core/tiff_helpers.rs`/`tag_db::lookup_tag_name`),
//!   and ZIP's per-entry forensic tags (`ZIP:File1:...`,
//!   `src/parsers/archive/zip.rs`) -- are not real ExifTool tags at all
//!   (there is no `-ComponentID_1` or `-ZIP:File1:CRC32` to request against
//!   real ExifTool; `-u` does not reveal them either). They are oxidex's own
//!   diagnostic/forensic namespace, gated purely on
//!   [`ReadOptions::extended`] (`--extended-output`), never on a specific
//!   request.
//!
//! # Two different gate *sites*, also on purpose
//!
//! `JPEGQualityEstimate` and the ten SOF tags are gated at the source --
//! never computed/inserted unless wanted -- because nothing downstream
//! depends on their absence from the internal `MetadataMap`.
//!
//! The hex-fallback tags are different: `src/writers/exif_surgical.rs`'s
//! surgical EXIF writer depends on `ExifIFD:0x927C` being present in the
//! `MetadataMap` it diffs against, in order to detect and reject an attempt
//! to edit an unsurfaced/raw-carried tag (`plan_changed_makernote_key_errors_
//! instead_of_silently_dropping`, confirmed still passing after this step).
//! Gating that insert at the source would have silently broken MakerNote
//! round-trip fidelity on write. So the hex-fallback and ZIP-forensic
//! categories stay computed exactly as before, and are instead filtered by
//! [`ReadOptions::strip_extended_only`] at the CLI *display* boundary
//! (`cli::tag_resolution::resolve_file_output`, used by both the single-file
//! and batch read paths) -- never by `read_metadata`/`write_metadata`/
//! `modify_tag`, which still see the full internal map.

use super::metadata_map::MetadataMap;
use std::collections::HashSet;

/// Per-read request-awareness, threaded from the CLI down to the small
/// number of parse sites whose emission should depend on what was actually
/// asked for. See the module doc comment for the ExifTool citations this
/// models and why two different categories of gated tag use two different
/// gate sites.
#[derive(Debug, Clone, Default)]
pub struct ReadOptions {
    /// Lowercased short tag names explicitly requested via `-TAG`, group
    /// qualifier stripped -- matches `REQ_TAG_LOOKUP`'s keys
    /// (`lc($2)` in ExifTool.pm:5164-5166: `/^(.*:)?([-\w?*]*)#?$/`, group 2
    /// is the bare name). A group-qualified request like
    /// `-EXIF:JPEGQualityEstimate` still populates this with
    /// `"jpegqualityestimate"`.
    requested: HashSet<String>,

    /// OxiDex's own opt-in (`--extended-output`) for its non-ExifTool
    /// diagnostic/forensic namespace: tags no real ExifTool option (not even
    /// `-u`) reveals, kept available for debugging rather than parity. See
    /// the module doc comment's second bullet list.
    extended: bool,
}

impl ReadOptions {
    /// Builds options from the CLI's requested-tag tokens (as returned by
    /// `CliArgs::specific_tags()`, so possibly group-qualified, e.g.
    /// `"EXIF:Make"`) and the `--extended-output` flag.
    pub fn new(requested_tags: &[String], extended: bool) -> Self {
        let requested = requested_tags
            .iter()
            .map(|t| {
                t.rsplit_once(':')
                    .map_or(t.as_str(), |(_, name)| name)
                    .to_ascii_lowercase()
            })
            .collect();
        Self {
            requested,
            extended,
        }
    }

    /// Today's default full-listing read: nothing specifically requested,
    /// extended namespace off. Every pre-existing call site (`read_metadata`,
    /// `parse_jpeg_metadata`, every test fixture that does not opt in) keeps
    /// using this via `Default`/this constructor, so none of them observe a
    /// behavior change from this step.
    pub fn default_full_listing() -> Self {
        Self::default()
    }

    /// Whether the CLI's `--extended-output` opted into OxiDex's
    /// diagnostic/forensic namespace.
    pub fn extended(&self) -> bool {
        self.extended
    }

    /// Whether `short_name` (case-insensitive, no group qualifier) was
    /// explicitly requested.
    pub fn is_requested(&self, short_name: &str) -> bool {
        self.requested.contains(&short_name.to_ascii_lowercase())
    }

    /// The DQT gate's shape (`ExifTool.pm:7688-7689`): emit a
    /// specifically-requested-or-extended tag. Used only by
    /// `JPEGQualityEstimate`, the one tag in this step's scope that is a
    /// real, individually-requestable ExifTool tag; see the module doc
    /// comment for why the other three categories use `extended()` alone
    /// instead.
    pub fn should_emit(&self, short_name: &str) -> bool {
        self.extended || self.is_requested(short_name)
    }

    /// Strips OxiDex's own diagnostic/forensic-only tags from a full,
    /// unfiltered listing unless `extended()` opted in: `lookup_tag_name`'s
    /// bare-hex fallback for a tag id with no name in the generated
    /// database (`ExifIFD:0x927C`'s undecoded MakerNote chief among them --
    /// ExifTool itself never surfaces a raw hex-numbered tag by default,
    /// only under `-u`) and ZIP's per-entry forensic tags
    /// (`ZIP:File1:CRC32`, ...), which have no ExifTool counterpart at all.
    ///
    /// Deliberately a display-only filter, not a source-level gate -- see
    /// the module doc comment's "Two different gate *sites*" section for
    /// why (`src/writers/exif_surgical.rs`'s dependency on
    /// `ExifIFD:0x927C` staying in the underlying `MetadataMap`). Callers
    /// that need the untouched internal map (`read_metadata`,
    /// `write_metadata`, `modify_tag`, the surgical writer) never call this;
    /// it is applied only by `cli::tag_resolution::resolve_file_output`'s
    /// unfiltered-listing branch, shared by the single-file and batch CLI
    /// paths.
    ///
    /// A tag matched by [`is_requested`](Self::is_requested) is never
    /// filtered by this function regardless of `extended()` -- in practice
    /// this is moot for callers of this function specifically (it only ever
    /// runs on the *unfiltered* listing, where `requested` is always empty),
    /// but keeping the check here rather than assuming it makes the
    /// invariant explicit instead of implicit in caller order.
    pub fn strip_extended_only(&self, metadata: &MetadataMap) -> MetadataMap {
        if self.extended {
            return metadata.clone();
        }
        let mut out = MetadataMap::with_capacity(metadata.len());
        for (key, value) in metadata.iter() {
            let short_name = key.rsplit_once(':').map_or(key.as_str(), |(_, n)| n);
            if self.is_requested(short_name) {
                out.insert(key.clone(), value.clone());
                continue;
            }
            if is_hex_fallback_name(short_name) || is_zip_forensic_entry_key(key) {
                continue;
            }
            out.insert(key.clone(), value.clone());
        }
        out
    }
}

/// Whether `short_name` is `tag_db::lookup_tag_name`'s hex fallback shape
/// (`"0x" + hex digits`, minted identically wherever an IFD tag id has no
/// name in the generated database) rather than re-deriving the fallback
/// from every call site that can produce one.
fn is_hex_fallback_name(short_name: &str) -> bool {
    let Some(hex) = short_name
        .strip_prefix("0x")
        .or_else(|| short_name.strip_prefix("0X"))
    else {
        return false;
    };
    !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit())
}

/// Whether `key` is one of ZIP's per-entry forensic tags
/// (`ZIP:File<N>:<Field>`, `src/parsers/archive/zip.rs`), which real
/// ExifTool's `ZIP.pm` has no counterpart for at all: it reports the
/// unnumbered `Zip*` fields for the archive's first local file header only,
/// never a per-entry breakdown.
fn is_zip_forensic_entry_key(key: &str) -> bool {
    let Some(rest) = key.strip_prefix("ZIP:File") else {
        return false;
    };
    let digits_end = rest.find(':').unwrap_or(0);
    digits_end > 0
        && rest.as_bytes()[..digits_end]
            .iter()
            .all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::TagValue;

    #[test]
    fn default_requests_nothing_and_extended_is_off() {
        let options = ReadOptions::default_full_listing();
        assert!(!options.extended());
        assert!(!options.is_requested("jpegqualityestimate"));
        assert!(!options.should_emit("jpegqualityestimate"));
    }

    #[test]
    fn requested_tag_names_are_lowercased_and_group_qualifiers_stripped() {
        let options = ReadOptions::new(&["EXIF:JPEGQualityEstimate".to_string()], false);
        assert!(options.is_requested("jpegqualityestimate"));
        assert!(options.is_requested("JPEGQualityEstimate"));
        assert!(options.should_emit("jpegqualityestimate"));
        assert!(!options.extended());
    }

    #[test]
    fn extended_mode_emits_everything_regardless_of_request() {
        let options = ReadOptions::new(&[], true);
        assert!(options.should_emit("anything"));
    }

    #[test]
    fn strip_extended_only_removes_hex_fallback_and_zip_entry_keys() {
        let mut metadata = MetadataMap::new();
        metadata.insert("ExifIFD:0x927C", TagValue::new_binary(vec![1, 2, 3]));
        metadata.insert("IFD0:Make", TagValue::new_string("Canon"));
        metadata.insert("ZIP:File1:CRC32", TagValue::new_string("0xDEADBEEF"));
        metadata.insert("ZIP:FileCount", TagValue::new_integer(1));

        let options = ReadOptions::default_full_listing();
        let filtered = options.strip_extended_only(&metadata);

        assert_eq!(filtered.get_string("IFD0:Make"), Some("Canon"));
        assert_eq!(filtered.get_integer("ZIP:FileCount"), Some(1));
        assert!(filtered.get("ExifIFD:0x927C").is_none());
        assert!(filtered.get("ZIP:File1:CRC32").is_none());
    }

    #[test]
    fn extended_mode_keeps_everything() {
        let mut metadata = MetadataMap::new();
        metadata.insert("ExifIFD:0x927C", TagValue::new_binary(vec![1, 2, 3]));
        metadata.insert("ZIP:File1:CRC32", TagValue::new_string("0xDEADBEEF"));

        let options = ReadOptions::new(&[], true);
        let filtered = options.strip_extended_only(&metadata);

        assert!(filtered.get("ExifIFD:0x927C").is_some());
        assert!(filtered.get("ZIP:File1:CRC32").is_some());
    }

    #[test]
    fn non_hex_fallback_and_non_zip_entry_names_survive() {
        assert!(!is_hex_fallback_name("Make"));
        assert!(!is_hex_fallback_name("0x"));
        assert!(!is_hex_fallback_name("0xZZZZ"));
        assert!(is_hex_fallback_name("0x927C"));
        assert!(is_hex_fallback_name("0xa"));

        assert!(!is_zip_forensic_entry_key("ZIP:FileCount"));
        assert!(!is_zip_forensic_entry_key("ZIP:Files"));
        assert!(!is_zip_forensic_entry_key("ZIP:FileVersion"));
        assert!(is_zip_forensic_entry_key("ZIP:File1:CRC32"));
        assert!(is_zip_forensic_entry_key("ZIP:File12:Filename"));
    }
}
