//! Machine-readable parse status and diagnostics for a metadata read.
//!
//! ExifTool's read path does not hard-fail on a damaged file if it can
//! still say *something*. `ProcessJPEG` clears its `$success` flag when a
//! segment can't be parsed but keeps walking the file, and only at the end
//! does `$success or $self->Warn('JPEG format error')`
//! (`ExifTool.pm:8483`) turn that into a `Warning` tag instead of an
//! exception. `Warn` itself (`ExifTool.pm:5616-5643`) resolves to
//! `$self->FoundTag('Warning', $str)` -- a warning is just another
//! extracted tag, not a separate channel that can silently go missing.
//!
//! `ReadReport` is OxiDex's equivalent: a read always returns whatever tags
//! it could get, plus a `status` that records how far it got, plus the
//! diagnostics that would previously have been dropped by a `let _ =`, an
//! `if let Ok`, or an `eprintln!` inside a parser.

use crate::core::metadata_map::MetadataMap;
use crate::exiftool_tables::RefusalCounts;

/// How far a read got.
///
/// This is a statement about the *read*, not about how many tags came out
/// of it -- a `Partial` read of a one-byte-truncated RAW can still produce
/// hundreds of tags, and a `Parsed` read of a mostly-empty file can produce
/// almost none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParseStatus {
    /// The format has a parser and it ran to completion with nothing
    /// pushed to the diagnostic sink.
    Parsed,
    /// The format has a parser, but the read hit at least one recoverable
    /// problem: a sub-block a parser could not decode (diagnostics
    /// non-empty, real tags still present), or the parser failed outright
    /// and the read fell back to filesystem + identity tags. Either way,
    /// `diagnostics` says what went wrong, and a `File:Warning` /
    /// `File:Error` tag mirrors it into the output the way ExifTool's
    /// `Warn`/`Error` do.
    Partial,
    /// No parser exists for this format, or the dispatcher declined the
    /// file as a format it does not implement -- but `crate::filetype::
    /// identify` could still name it. Only `FileType`/`FileTypeExtension`/
    /// `MIMEType` plus filesystem tags are present; see `add_identity_tags`
    /// in `core::operations`. AGENTS.md calls this "detected is not
    /// parsed": a file in this state can report a perfectly correct
    /// `FileType` while 100% of its real tags are missing.
    IdentifiedOnly,
    /// Neither a parser nor the identification tables could say anything
    /// about this file. Only filesystem tags are present.
    Unsupported,
}

impl ParseStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ParseStatus::Parsed => "Parsed",
            ParseStatus::Partial => "Partial",
            ParseStatus::IdentifiedOnly => "IdentifiedOnly",
            ParseStatus::Unsupported => "Unsupported",
        }
    }
}

impl std::fmt::Display for ParseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What kind of problem a [`Diagnostic`] reports.
///
/// `Warning` and `Error` mirror the two tags ExifTool's own `Warn`/`Error`
/// (`ExifTool.pm:5616`, `:5654`) produce. `Refusal` is a third kind neither
/// of those Perl subs has: it is the seam Step 10's runtime refusals
/// (a maintainer policy decision to decline walking a structure at all,
/// rather than a parse failure) report into -- see
/// [`Diagnostic::refusals`], which builds one from a
/// [`RefusalCounts`][crate::exiftool_tables::RefusalCounts].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    /// Recoverable: some tags may be missing or a sub-block was skipped,
    /// but the read continued.
    Warning,
    /// The read (or a sub-parse within it) could not continue.
    Error,
    /// A deliberate runtime refusal to process a structure. Reserved for
    /// Step 10; never constructed by anything in this codebase yet.
    Refusal,
}

/// One diagnostic that would previously have been dropped on the floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub message: String,
}

impl Diagnostic {
    pub fn warning<S: Into<String>>(message: S) -> Self {
        Diagnostic {
            kind: DiagnosticKind::Warning,
            message: message.into(),
        }
    }

    pub fn error<S: Into<String>>(message: S) -> Self {
        Diagnostic {
            kind: DiagnosticKind::Error,
            message: message.into(),
        }
    }

    /// Step 10's runtime refusals: a low-level constructor for a caller that
    /// already has a rendered message. Most callers want
    /// [`Diagnostic::refusals`] instead, which formats a
    /// [`RefusalCounts`][crate::exiftool_tables::RefusalCounts] consistently.
    pub fn refusal<S: Into<String>>(message: S) -> Self {
        Diagnostic {
            kind: DiagnosticKind::Refusal,
            message: message.into(),
        }
    }

    /// Build a `Refusal` diagnostic from one table's [`RefusalCounts`], or
    /// `None` when nothing was withheld -- so a call site can
    /// `.and_then(|d| sink.push(d))` straight off a `decode_binary_table`
    /// result without a separate `is_empty` check.
    ///
    /// `module`/`table` identify the generated table
    /// (`exiftool_tables::BinaryTable::module`/`::table`), matching how
    /// [`crate::exiftool_tables::PerlCitation`] names one, so a diagnostic
    /// and the `RawAccess` citations a parser used against the same table
    /// read the same way.
    #[must_use]
    pub fn refusals(module: &str, table: &str, counts: RefusalCounts) -> Option<Self> {
        if counts.total() == 0 {
            return None;
        }
        Some(Self::refusal(format!(
            "{module}::{table}: {total} field{plural} withheld \
             (value_conv={value_conv} raw_conv={raw_conv} condition={condition} \
             hook={hook} subdirectory={subdirectory} print_conv={print_conv} \
             offset_unsound={offset_unsound})",
            total = counts.total(),
            plural = if counts.total() == 1 { "" } else { "s" },
            value_conv = counts.value_conv,
            raw_conv = counts.raw_conv,
            condition = counts.condition,
            hook = counts.hook,
            subdirectory = counts.subdirectory,
            print_conv = counts.print_conv,
            offset_unsound = counts.offset_unsound,
        )))
    }
}

/// Where a parser reports a problem instead of swallowing it.
///
/// A plain `Vec<Diagnostic>` today, kept as a named alias so call sites
/// don't repeat the element type and so a richer sink (per-message dedup
/// like ExifTool's `WAS_WARNED`, severity filtering) can replace the `Vec`
/// later without changing every function signature that threads it.
pub type DiagnosticSink = Vec<Diagnostic>;

/// The result of a metadata read: what was extracted, how far the read
/// got, and anything that went wrong along the way.
#[derive(Debug, Clone)]
pub struct ReadReport {
    pub metadata: MetadataMap,
    pub status: ParseStatus,
    pub diagnostics: Vec<Diagnostic>,
}

impl ReadReport {
    /// Discards `status` and `diagnostics`, keeping only the metadata.
    pub fn into_metadata(self) -> MetadataMap {
        self.metadata
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_as_str_matches_the_json_wire_names() {
        assert_eq!(ParseStatus::Parsed.as_str(), "Parsed");
        assert_eq!(ParseStatus::Partial.as_str(), "Partial");
        assert_eq!(ParseStatus::IdentifiedOnly.as_str(), "IdentifiedOnly");
        assert_eq!(ParseStatus::Unsupported.as_str(), "Unsupported");
    }

    #[test]
    fn display_matches_as_str() {
        for status in [
            ParseStatus::Parsed,
            ParseStatus::Partial,
            ParseStatus::IdentifiedOnly,
            ParseStatus::Unsupported,
        ] {
            assert_eq!(status.to_string(), status.as_str());
        }
    }

    #[test]
    fn diagnostic_constructors_set_the_right_kind() {
        assert_eq!(Diagnostic::warning("w").kind, DiagnosticKind::Warning);
        assert_eq!(Diagnostic::error("e").kind, DiagnosticKind::Error);
        assert_eq!(Diagnostic::refusal("r").kind, DiagnosticKind::Refusal);
    }

    #[test]
    fn refusals_is_none_when_nothing_was_withheld() {
        assert_eq!(
            Diagnostic::refusals("PhotoCD", "Main", RefusalCounts::default()),
            None
        );
    }

    #[test]
    fn refusals_summarizes_every_reason_by_name() {
        let counts = RefusalCounts {
            value_conv: 9,
            raw_conv: 3,
            condition: 6,
            hook: 0,
            subdirectory: 0,
            print_conv: 2,
            offset_unsound: 0,
        };
        let diagnostic =
            Diagnostic::refusals("PhotoCD", "Main", counts).expect("nonzero counts produce Some");
        assert_eq!(diagnostic.kind, DiagnosticKind::Refusal);
        assert!(diagnostic.message.contains("PhotoCD::Main"));
        assert!(diagnostic.message.contains("20 fields withheld"));
        assert!(diagnostic.message.contains("value_conv=9"));
        assert!(diagnostic.message.contains("raw_conv=3"));
        assert!(diagnostic.message.contains("condition=6"));
        // The newest reason has to appear by name too: a refusal reported
        // only in the total is a refusal a reader cannot act on.
        assert!(diagnostic.message.contains("print_conv=2"));
    }

    #[test]
    fn into_metadata_drops_status_and_diagnostics() {
        let mut metadata = MetadataMap::new();
        metadata.insert("File:FileName", crate::core::TagValue::new_string("x.jpg"));
        let report = ReadReport {
            metadata,
            status: ParseStatus::Parsed,
            diagnostics: vec![Diagnostic::warning("unused")],
        };
        let metadata = report.into_metadata();
        assert_eq!(metadata.get_string("File:FileName"), Some("x.jpg"));
    }
}
