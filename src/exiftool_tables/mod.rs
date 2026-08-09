//! ExifTool tag tables transcribed mechanically from ExifTool itself.
//!
//! # Why this exists
//!
//! OxiDex already knows most ExifTool tag *names*: `src/tag_sync` parses
//! `exiftool -f -listx`, which lists every documented tag. Knowing a name is
//! not enough to read one. `-listx` is the documentation view, and it omits
//! the things a reader needs:
//!
//! * `SubDirectory`/`TagTable` -- the edges between tables
//! * `FORMAT` / `FIRST_ENTRY` -- the byte layout of binary records
//! * `Format` overrides, `Mask`, `DataMember`, `Condition`
//! * `ValueConv` / `RawConv`
//!
//! That missing layout is precisely what MakerNote extraction depends on, and
//! it is why tag *coverage* has trailed tag *knowledge*. This module closes
//! that gap by reading ExifTool's tables out of the Perl interpreter's symbol
//! table, where the real structures live, and generating Rust from them.
//!
//! # Guarantees
//!
//! The generator refuses to approximate. A `PrintConv` it cannot reproduce
//! exactly is dropped, not guessed, and the drop is counted and reported. This
//! is a deliberate bias toward under-claiming: a wrong conversion does not
//! crash, it emits a confident wrong number under a real ExifTool tag name,
//! and an archival pipeline downstream cannot tell. A missing tag is loud and
//! recoverable; a wrong one is neither.
//!
//! `tools/exiftool-tables/verify.py` checks every emitted field and enum entry
//! back against ExifTool through an independent code path, and is wired up as
//! `just verify-tables`.
//!
//! # Regenerating
//!
//! ```sh
//! just regen-tables            # extract + generate + verify
//! ```

pub mod binary_tables;
pub mod runtime;

pub use binary_tables::{
    ALL_BINARY_TABLES, BinaryTable, EXIFTOOL_VERSION, ExprId, Field, Fmt, PrintConv,
};
pub use runtime::{DecodedField, DecodedValue, decode_binary_table};

/// Look up a generated table by ExifTool module and table name,
/// e.g. `("Canon", "CameraSettings")`.
#[must_use]
pub fn find_table(module: &str, table: &str) -> Option<&'static BinaryTable> {
    ALL_BINARY_TABLES
        .iter()
        .copied()
        .find(|t| t.module == module && t.table == table)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tables are only meaningful relative to a specific ExifTool release,
    /// so the stamp has to survive regeneration. Without it `verify.py` cannot
    /// tell a transcription error from a mismatched oracle, which is the one
    /// failure mode that makes every other check in this module unreadable.
    #[test]
    fn tables_record_their_exiftool_release() {
        assert!(
            EXIFTOOL_VERSION
                .split('.')
                .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())),
            "expected a dotted ExifTool version, found {EXIFTOOL_VERSION:?}"
        );
    }

    #[test]
    fn tables_are_present() {
        assert!(
            ALL_BINARY_TABLES.len() > 200,
            "expected the generated table set, found {}",
            ALL_BINARY_TABLES.len()
        );
    }

    #[test]
    fn canon_camera_settings_matches_exiftool() {
        // Spot-check against Canon.pm: MacroMode sits at index 1 of a table
        // whose FORMAT is int16s, so ProcessBinaryData reads it at byte
        // offset 1 * 2 = 2 (the int16u at offset 0 is the record length,
        // which the table deliberately defines no tag for). FIRST_ENTRY is
        // 1, but ExifTool only uses that to bound the Unknown>1 auto-scan
        // -- it never shifts the index-to-offset arithmetic.
        let t = find_table("Canon", "CameraSettings").expect("Canon::CameraSettings");
        assert_eq!(t.default_format, Fmt::Int16s);
        assert_eq!(t.first_entry, 1);

        let f = t
            .fields
            .iter()
            .find(|f| f.name == "MacroMode")
            .expect("MacroMode");
        assert_eq!(f.index, 1);
        assert_eq!(t.byte_offset(f), 2);
        assert_eq!(f.print_conv.apply(1).as_deref(), Some("Macro"));
        assert_eq!(f.print_conv.apply(2).as_deref(), Some("Normal"));
        // A value absent from the enum must not invent a rendering.
        assert_eq!(f.print_conv.apply(99), None);
    }

    #[test]
    fn byte_offsets_scale_with_format_width() {
        let t = find_table("Canon", "CameraSettings").expect("Canon::CameraSettings");
        for f in t.fields {
            // int16s table: every field lands on an even byte boundary.
            assert_eq!(t.byte_offset(f) % 2, 0, "field {} misaligned", f.name);
        }
    }

    #[test]
    fn int_enums_are_sorted_for_binary_search() {
        // `PrintConv::apply` uses `binary_search_by_key`; an unsorted table
        // would silently return wrong or missing values rather than fail.
        for t in ALL_BINARY_TABLES {
            for f in t.fields {
                if let PrintConv::IntEnum(m) = f.print_conv {
                    assert!(
                        m.windows(2).all(|w| w[0].0 < w[1].0),
                        "{}::{} field {} enum is not strictly sorted",
                        t.module,
                        t.table,
                        f.name
                    );
                }
            }
        }
    }

    #[test]
    fn no_empty_names() {
        for t in ALL_BINARY_TABLES {
            for f in t.fields {
                assert!(
                    !f.name.is_empty(),
                    "{}::{} has an unnamed field",
                    t.module,
                    t.table
                );
            }
        }
    }
}
