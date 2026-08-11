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
    ALL_BINARY_TABLES, BinaryTable, EXIFTOOL_VERSION, ExprId, Field, Fmt, Mask, Omitted, PrintConv,
};
pub use runtime::{
    Acknowledged, DecodedField, DecodedValue, FractionalCensus, PerlCitation, RawAccess,
    RefusalCounts, TableDecode, all_fractional_census, decode_binary_table, fractional_census,
};

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

    /// The stamp must be the release the rest of the repo grades against.
    ///
    /// This is the cheap layer of a three-part guard, and the only one that
    /// needs neither Perl nor Python: `tools/exiftool-tables/verify.py` refuses
    /// a stamp that isn't the pin, and `regen.sh` refuses to transcribe any
    /// other release. Before those existed the check was circular -- verify.py
    /// took its expected release from this very stamp, and both the justfile
    /// recipe and CI chose which ExifTool to fetch the same way -- so these
    /// tables sat at 13.30 for 29 releases while every published coverage
    /// number was measured against the pinned 13.59, and every check passed.
    #[test]
    fn tables_are_transcribed_from_the_pinned_release() {
        let pinned = crate::exiftool_oracle::repo_pin();
        assert_eq!(
            EXIFTOOL_VERSION, pinned,
            "binary_tables.rs was transcribed from ExifTool {EXIFTOOL_VERSION}, but \
             .exiftool-version pins {pinned}; regenerate with `just regen-tables`"
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

    /// `Omitted::any()` is the single gate a caller is meant to check before
    /// trusting a decoded value; if it stops noticing a flag, that flag's
    /// semantic starts leaking through as if it were reproduced.
    #[test]
    fn omitted_any_covers_hook_and_subdirectory() {
        assert!(
            Omitted {
                hook: true,
                ..Omitted::NONE
            }
            .any()
        );
        assert!(
            Omitted {
                subdirectory: true,
                ..Omitted::NONE
            }
            .any()
        );
        assert!(!Omitted::NONE.any());
    }

    /// Step 9's accounting identity: the count of fields the generator flags
    /// `hook`/`subdirectory` must equal what a census of the ExifTool 13.59
    /// dump found (measured independently with `tools/exiftool-tables/
    /// codegen.py`'s own report, and cross-checked again by `verify.py`
    /// against the live Perl hashes). Pinning the numbers here means a future
    /// regen that silently stops reading `Hook`/`SubDirectory` -- reopening
    /// the exact silent-drop class this step closed -- fails a test instead
    /// of only showing up as a diff nobody reads.
    #[test]
    fn hook_and_subdirectory_census_matches_the_13_59_dump() {
        let (mut hooks, mut subdirs) = (0usize, 0usize);
        for t in ALL_BINARY_TABLES {
            for f in t.fields {
                hooks += usize::from(f.omitted.hook);
                subdirs += usize::from(f.omitted.subdirectory);
            }
        }
        assert_eq!(hooks, 35, "Hook-carrying emitted fields");
        assert_eq!(subdirs, 63, "SubDirectory-carrying emitted fields");
    }

    /// `offsets_sound_until` must be set on exactly the tables where a
    /// refused `var_*` field actually sits before an emitted one -- not on
    /// every table that merely contains a `var_*` field ExifTool declares.
    /// At 13.59 that is 4 tables covering 81 already-emitted fields whose
    /// static `index * increment` offset is no longer trustworthy.
    #[test]
    fn offsets_sound_until_marks_exactly_the_tables_with_a_live_hazard() {
        let mut affected_tables = 0usize;
        let mut affected_fields = 0usize;
        for t in ALL_BINARY_TABLES {
            let Some(bound) = t.offsets_sound_until else {
                continue;
            };
            affected_tables += 1;
            let hit = t.fields.iter().filter(|f| f.index > bound).count();
            assert!(
                hit > 0,
                "{}::{} sets offsets_sound_until but no field sits past it",
                t.module,
                t.table
            );
            affected_fields += hit;
        }
        assert_eq!(affected_tables, 4, "tables with a live var_* offset hazard");
        assert_eq!(affected_fields, 81, "fields past the hazard boundary");

        let expect = [
            ("CanonVRD", "Ver2", 88),
            ("DNG", "ImageSeq", 0),
            ("FLAC", "Picture", 1),
            ("Photoshop", "SliceInfo", 20),
        ];
        for (module, table, bound) in expect {
            let t = find_table(module, table).unwrap_or_else(|| panic!("{module}::{table}"));
            assert_eq!(
                t.offsets_sound_until,
                Some(bound),
                "{module}::{table} offsets_sound_until"
            );
        }
    }
}
