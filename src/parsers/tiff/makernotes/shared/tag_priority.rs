//! ExifTool's tag priority, for the case where one file reports the same tag
//! name twice.
//!
//! A MakerNote routinely carries the same tag in two places: once in the
//! vendor's `Main` table and once inside a binary sub-directory. ExifTool keeps
//! both -- they are separate tag keys, `LensType` and `LensType (1)` -- but only
//! one of them is the tag that prints under the plain name, and `Priority`
//! decides which. oxidex reports one value per `Group:Name`, so the same
//! decision has to be made at insert time.
//!
//! # ExifTool's rule
//!
//! `Image::ExifTool::FoundTag` (`ExifTool.pm:9448`) reads the tag's priority at
//! `:9469-9473` -- the tag's own `Priority`, else the table's `PRIORITY`, else
//! `0` when the tag is marked `Avoid`. When the name is already present
//! (`:9515`) it compares against the priority already recorded for it:
//!
//! ```text
//! my $oldPriority = $$self{PRIORITY}{$tag};      # ExifTool.pm:9544
//! unless ($oldPriority) { ... $oldPriority = 1; } #             :9545-9551
//! ...
//! if ($priority >= $oldPriority and ...) {        #             :9564
//!     # the new value takes the plain name, the old one moves to "$tag (1)"
//! } else {
//!     $tag = $nextTag;                            #             :9585
//!     # the new value goes to "$tag (1)"; the old one keeps the plain name
//! }
//! ```
//!
//! Two details make the rule what it is. A value stored with priority 0 records
//! no entry in `PRIORITY` at all (`:9589` stores it "only if exists and is
//! non-zero"), and a missing entry is *promoted to 1* at `:9545-9551` -- the
//! comment there reads "promote existing 0-priority tag so it takes precedence
//! over a new 0-tag". So the comparison at `:9564` resolves the four possible
//! orderings like this:
//!
//! | first | second | winner |
//! |-------|--------|--------|
//! | normal | `Priority => 0` | first — `0 >= 1` is false |
//! | `Priority => 0` | normal | second — old promoted to 1, `1 >= 1` holds |
//! | `Priority => 0` | `Priority => 0` | first — old promoted to 1, `0 >= 1` is false |
//! | normal | normal | second — `1 >= 1` holds |
//!
//! Which is exactly two rules, and they compose to the same outcome for any
//! number of instances in any order:
//!
//! * a `Priority => 0` value never displaces a value already present;
//! * a normal-priority value always displaces whatever is present.
//!
//! The second is what a plain [`HashMap::insert`] already does, so only the
//! first needs a function. Writing a `Priority => 0` tag with `insert` is the
//! bug this exists to prevent: it does not fail, it does not drop a tag, it
//! silently prints the sub-directory's value under a real ExifTool tag name.
//!
//! [`HashMap::insert`]: std::collections::HashMap::insert

use std::collections::HashMap;

/// Records a value ExifTool declares `Priority => 0`.
///
/// Keeps whatever is already under `key` and discards `value`; stores `value`
/// only when the name has not been reported yet. See the module documentation
/// for why this, plus an ordinary `insert` for every normal-priority tag,
/// reproduces `FoundTag`'s comparison exactly.
pub(crate) fn insert_low_priority(tags: &mut HashMap<String, String>, key: String, value: String) {
    tags.entry(key).or_insert(value);
}

/// Like [`insert_low_priority`], but for callers migrated to Step 19's real
/// occurrence retention (`OVERHAUL_STEP18_DESIGN.md` §2.3 Phase B):
/// currently just Pentax's `LensType`/`LensFocalLength`/`PentaxModelID`
/// duplicate pairs (`pentax.rs`).
///
/// A shadowed value is not discarded outright as [`insert_low_priority`]
/// does -- it is stashed under a synthetic `"<key> (N)"` companion key,
/// mirroring `FoundTag`'s own `"$tag ($nextInd)"` duplicate-key convention
/// (`ExifTool.pm:9532`). The `HashMap<String, String>` this trait's callers
/// pass has no way to hold two values under one key, so the companion key is
/// the seam: `tiff_helpers.rs`'s makernote merge recognizes it and records
/// the shadowed value as a real, always-losing `TagOccurrence` under its
/// base key instead of literally inserting a garbage `"Tag (N)"` tag name.
///
/// Every other `MakerNoteParser` -- everyone except Pentax -- keeps calling
/// [`insert_low_priority`] unchanged (`shared/binary_subdir.rs`'s generic
/// dispatch, in particular), so this function's behavior never reaches any
/// other manufacturer's output.
pub(crate) fn insert_low_priority_retained(
    tags: &mut HashMap<String, String>,
    key: String,
    value: String,
) {
    if !tags.contains_key(&key) {
        tags.insert(key, value);
        return;
    }
    let mut n = 1u32;
    while tags.contains_key(&format!("{key} ({n})")) {
        n += 1;
    }
    tags.insert(format!("{key} ({n})"), value);
}

/// The other half of [`insert_low_priority_retained`]: records one
/// `MakerNoteParser`-produced `(tag_name, value)` pair into a real
/// `MetadataMap`, recognizing the `"<key> (N)"` synthetic duplicate marker
/// and routing it to [`crate::core::MetadataMap::insert_occurrence`] as a
/// real, always-losing `TagOccurrence` under its base key -- instead of what
/// every makernote-merge call site used to do unconditionally, which would
/// otherwise literally insert a tag named e.g. `"Pentax:LensType (1)"` that
/// no ExifTool output ever produces.
///
/// Every non-Pentax manufacturer's tags pass straight through unaffected:
/// only [`insert_low_priority_retained`] ever mints a `"(N)"`-suffixed key,
/// and only Pentax's four call sites (`pentax.rs`) use it, so
/// [`strip_duplicate_marker`] never matches anything else's tag name.
///
/// Every makernote-merge call site in the tree (JPEG/TIFF's
/// `tiff_helpers.rs`, `avi.rs`'s Pentax `hymn`/`mknt` chunks, and RAW's
/// `DNGPrivateData` MakN record) goes through this rather than a bare
/// `metadata.insert()`, because Pentax MakerNotes can reach any of them.
pub(crate) fn record_makernote_tag(
    metadata: &mut crate::core::MetadataMap,
    tag_name: String,
    tag_value: crate::core::TagValue,
) {
    if let Some(base) = strip_duplicate_marker(&tag_name) {
        metadata.insert_occurrence(base, tag_value, 0, "", crate::core::Instance::default());
        return;
    }
    if let Some(group1) = priority_zero_duplicate_group1(&tag_name) {
        // ExifTool declares `Priority => 0` on each of these -- see
        // `PRIORITY_ZERO_DUPLICATES`' own doc comment for the exact
        // per-manufacturer `.pm` citations. Every one is a MakerNote-side
        // duplicate of a standard EXIF tag (an APEX-derived FNumber/
        // ExposureTime, a log-scale-derived ISO, a binary-record
        // FocalLength, ...) that ExifTool itself trusts less than the
        // ordinary EXIF copy.
        //
        // Step 20's composite/CLI arbitration (`cli::tag_resolution::
        // resolve_requested_tags`) resolves an unqualified `-TAG` request or
        // Composite `Desire`/`Require` by priority-then-order, so at the
        // ordinary default priority these MakerNote duplicates -- extracted
        // *after* the main EXIF IFD on every file where MakerNote parsing
        // runs later in the same IFD pass -- would otherwise win the tie on
        // `order` alone and silently feed a wrong shutter speed/aperture/
        // ISO/focal length into every composite chained from them
        // (`Aperture`, `ShutterSpeed`, `LightValue`, `HyperfocalDistance`,
        // `CircleOfConfusion`, `DOF`, `FocalLength35efl`, `ScaleFactor35efl`).
        // Caught on 500+ corpus JPEGs (Canon's `ExposureTime`/`FNumber`) and
        // Nikon's own `ISO` duplicate, plus the equivalent CR2/CR3/DNG/MRW/
        // NEF/RW2 RAW containers, when Step 22's full-corpus conformance run
        // first exercised this arbitration end to end -- see the
        // `ExposureTime` example this function's own regression test pins.
        metadata.insert_occurrence(
            tag_name,
            tag_value,
            0,
            group1,
            crate::core::Instance::default(),
        );
        return;
    }
    metadata.insert(tag_name, tag_value);
}

/// MakerNote tags ExifTool itself declares `Priority => 0` for, mapped to
/// the family-1 group their manufacturer prefix implies -- a small, explicit
/// allowlist rather than a manufacturer-wide rule, because MakerNote tags
/// are not uniformly lower priority than EXIF in ExifTool (`CIFF:Make`, for
/// one, legitimately outranks `IFD0:Make` on `ExifTool.jpg` -- confirmed
/// against the pinned oracle and pinned by `cli::tag_resolution`'s own test
/// suite), so [`priority_zero_duplicate_group1`] only demotes the specific
/// tags actually declared low priority in the pinned source:
///
/// * `Canon:FocalLength` -- `%Canon::FocalLength` key 1 explicitly
///   (Canon.pm:2723-2724, "the EXIF FocalLength is more reliable").
/// * `Canon:FNumber`/`Canon:ExposureTime` -- `%Canon::ShotInfo` keys 21 and
///   22 explicitly (Canon.pm:2956-2994).
/// * `Canon:ISO`/`Canon:CameraTemperature`/`Canon:MacroMagnification`/
///   `Canon:FocalLength`/`Canon:MinFocal`/`Canon:MaxFocal` -- the
///   *table-level* `PRIORITY => 0` every `%Canon::CameraInfo*` sub-table
///   declares (Canon.pm:3162 and 20+ other CameraInfo tables, "these tags
///   are not reliable since they change with firmware version"). These six
///   are ExifTool's own shared `%ci*` Perl hashes (`my %ciFNumber`,
///   `%ciExposureTime`, `%ciISO`, `%ciCameraTemperature`,
///   `%ciMacroMagnification`, `%ciFocalLength`, `%ciMinFocal`, `%ciMaxFocal`,
///   Canon.pm:3087-3149), each embedded verbatim into every CameraInfo table
///   via `0x04 => { %ciExposureTime }`-style splices -- which is exactly why
///   `src/parsers/tiff/makernotes/canon/camera_info.rs` already keeps its
///   own decode of them in a `camera_info_tags` map merged with
///   `PRIORITY => 0` semantics (`merge_priority0`), but only *within*
///   Canon's own tag set; nothing carried that semantics out to this
///   cross-manufacturer arbitration point until this fix.
/// * `Nikon:ISO` -- Nikon.pm:1803 explicitly, `Priority => 0, # the EXIF ISO
///   is more reliable` (MakerNote tag 0x0002). `Nikon:FocalLength` --
///   Nikon.pm:5908 explicitly (the `NewLensData` `%Nikon::LensData0403`-style
///   entry, tag 0x3c). `Nikon:FNumber` -- Nikon.pm:5898 explicitly, the
///   same `NewLensData` table's tag 0x38 (confirmed on `NikonZ5.jpg`:
///   `ExifIFD:FNumber` "8.0" vs `Nikon:FNumber` "6.2", the encrypted
///   `LensData` mirrorless-Z-series bodies carry).
/// * `Minolta:ISO`/`Minolta:ExposureTime`/`Minolta:FNumber`/
///   `Minolta:FocalLength`/`Minolta:MaxAperture` -- the table-level
///   `PRIORITY => 0` on `%Minolta::CameraSettings`/`%CameraSettingsOld`
///   (Minolta.pm:974, "not as reliable as other tags") and, for
///   `Minolta:FNumber`/`Minolta:ExposureTime` specifically on the Sony
///   DSLR-A100, `%Minolta::CameraSettingsA100` (Minolta.pm:1874, "may not be
///   as reliable as other information") -- both tables `src/parsers/tiff/
///   makernotes/minolta.rs` already decodes into its own `sub_dir` output
///   with a comment noting "All four tables are PRIORITY => 0, the same
///   tier CameraSettings reports under", same shape as Canon's
///   `camera_info_tags`.
/// * `Sony:FNumber`/`Sony:ExposureTime`/`Sony:ISO`/`Sony:FocalLength` --
///   the table-level `PRIORITY => 0` several `%Sony::*` sub-tables declare
///   (Sony.pm:3531 `%MoreSettings` and 20+ other tables across Sony's many
///   camera-generation-specific binary records), same shape as Canon's
///   `%Canon::CameraInfo*` and Minolta's `%CameraSettings`. Confirmed on
///   `SonyDSLR-A900.jpg` (`ExifIFD:FNumber` "4.5" vs `Sony:FNumber` "4.8")
///   and `SonyILCE-3500.jpg` (`ExifIFD:FocalLength` "125.0 mm" vs
///   `Sony:FocalLength` "158.0 mm").
/// * `MakerNotes:ISO`/`MakerNotes:ExposureTime`/`MakerNotes:FNumber`/
///   `MakerNotes:FocalLength`/`MakerNotes:MaxAperture` -- the *other*
///   Minolta `%CameraSettings` decoder, `src/parsers/raw/
///   minolta_makernote.rs` (used for standalone `.mrw` files' `TTW` block,
///   a separate code path from `minolta.rs`'s `MinoltaParser` because MRW's
///   MakerNote value offsets are TIFF-base-relative rather than note-
///   relative). This decoder inserts under the literal `MakerNotes:`
///   prefix -- the same cross-manufacturer convention several other
///   MakerNote parsers use for a handful of tags (`PreviewImage`/
///   `PreviewImageStart` in particular) -- rather than `Minolta:`, so it is
///   listed separately from the entries above even though it decodes the
///   identical ExifTool table. Left at `group1 = ""` (falls back to the
///   shim's own `MakerNotes` group0, matching this decoder's existing
///   display) rather than corrected to `Minolta` here, since fixing that
///   display mismatch would mean auditing every other tag this convention
///   is intentionally shared with (`src/parsers/tiff/makernotes/{sigma,
///   sony,minolta,nikon,olympus,casio}.rs` all key `PreviewImage` the same
///   way) -- out of scope for the value-correctness fix this table exists
///   for.
/// * `Pentax:ExposureTime`/`Pentax:FNumber` -- Pentax.pm:1472-1483
///   explicitly (MakerNote tags 0x0012/0x0013). `Pentax:ISO` --
///   Pentax.pm:2686/6264 explicitly (two different tag IDs across Pentax's
///   several MakerNote sub-formats, both `Priority => 0`).
///   `Pentax:FocalLength` -- Pentax.pm:1746/1759 explicitly (the
///   model-conditional `%Pentax::Main` tag 0x0006 entries).
/// * `Leica:FocalLength` -- Panasonic.pm:2103 explicitly, the
///   `%Panasonic::FocusInfo` entry 1 reached from `Leica5`/`Leica8` 0x040a.
///   Confirmed on `LeicaQ2MONO.jpg`, where the Q2 Monochrom's cropped-frame
///   FocusInfo reads `50.0 mm` against `ExifIFD:FocalLength`'s `28.0 mm`:
///   left at the default priority the MakerNote copy won the composite
///   arbitration on order alone and drove `Composite:FOV` to `35.5 deg`
///   where the oracle prints `37.3 deg`. Every other Leica body in the
///   corpus stores the same number in both places, so this one file is the
///   whole observable difference -- and the reason the demotion is needed at
///   all rather than assumed harmless.
///
/// Not exhaustive: other manufacturers declare the same `Priority => 0` "let
/// EXIF take priority" convention for their own ISO/FNumber/FocalLength-
/// shaped MakerNote fields (Casio.pm does too, confirmed against the pinned
/// source), but this list only covers the tag names this crate's own
/// MakerNote parsers currently produce under a name that also collides with
/// a standard EXIF/Composite dependency name -- covering a name that never
/// collides would be dead code with no observable effect.
const PRIORITY_ZERO_DUPLICATES: &[(&str, &str)] = &[
    ("Canon:FocalLength", "Canon"),
    ("Canon:FNumber", "Canon"),
    ("Canon:ExposureTime", "Canon"),
    ("Canon:ISO", "Canon"),
    ("Canon:CameraTemperature", "Canon"),
    ("Canon:MacroMagnification", "Canon"),
    ("Canon:MinFocal", "Canon"),
    ("Canon:MaxFocal", "Canon"),
    ("Nikon:ISO", "Nikon"),
    ("Nikon:FocalLength", "Nikon"),
    ("Nikon:FNumber", "Nikon"),
    ("Pentax:ExposureTime", "Pentax"),
    ("Pentax:FNumber", "Pentax"),
    ("Pentax:ISO", "Pentax"),
    ("Pentax:FocalLength", "Pentax"),
    ("Minolta:ISO", "Minolta"),
    ("Minolta:ExposureTime", "Minolta"),
    ("Minolta:FNumber", "Minolta"),
    ("Minolta:FocalLength", "Minolta"),
    ("Minolta:MaxAperture", "Minolta"),
    ("MakerNotes:ISO", ""),
    ("MakerNotes:ExposureTime", ""),
    ("MakerNotes:FNumber", ""),
    ("MakerNotes:FocalLength", ""),
    ("MakerNotes:MaxAperture", ""),
    ("Sony:FNumber", "Sony"),
    ("Sony:ExposureTime", "Sony"),
    ("Sony:ISO", "Sony"),
    ("Sony:FocalLength", "Sony"),
    ("Leica:FocalLength", "Leica"),
];

fn priority_zero_duplicate_group1(tag_name: &str) -> Option<&'static str> {
    PRIORITY_ZERO_DUPLICATES
        .iter()
        .find(|(name, _)| *name == tag_name)
        .map(|(_, group1)| *group1)
}

/// Recognizes [`insert_low_priority_retained`]'s `"<key> (N)"` synthetic
/// duplicate-marker convention and returns the base key, or `None` if
/// `tag_name` carries no such marker.
fn strip_duplicate_marker(tag_name: &str) -> Option<&str> {
    let rest = tag_name.strip_suffix(')')?;
    let paren = rest.rfind(" (")?;
    let (base, digits) = rest.split_at(paren);
    let digits = &digits[2..];
    if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
        Some(base)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    /// `normal` then `Priority => 0`: `0 >= 1` is false, so the first value
    /// keeps the plain tag name (ExifTool.pm:9564, :9585).
    #[test]
    fn low_priority_does_not_displace_a_present_value() {
        let mut tags = map(&[("Pentax:LensType", "smc PENTAX-DA 21mm F3.2 AL Limited")]);
        insert_low_priority(
            &mut tags,
            "Pentax:LensType".to_string(),
            "M-42 or No Lens".to_string(),
        );
        assert_eq!(
            tags["Pentax:LensType"],
            "smc PENTAX-DA 21mm F3.2 AL Limited"
        );
    }

    /// Two `Priority => 0` instances: the first is promoted to 1 when the
    /// second arrives, so the first still wins (ExifTool.pm:9541-9551).
    #[test]
    fn first_of_two_low_priority_values_wins() {
        let mut tags = HashMap::new();
        insert_low_priority(
            &mut tags,
            "Pentax:LensType".to_string(),
            "first".to_string(),
        );
        insert_low_priority(
            &mut tags,
            "Pentax:LensType".to_string(),
            "second".to_string(),
        );
        assert_eq!(tags["Pentax:LensType"], "first");
    }

    /// A `Priority => 0` value is still the reported one when nothing else
    /// reports the tag -- the rule suppresses a clobber, not the tag.
    #[test]
    fn low_priority_value_is_kept_when_it_is_the_only_one() {
        let mut tags = HashMap::new();
        insert_low_priority(
            &mut tags,
            "Pentax:PentaxModelID".to_string(),
            "Optio SV".to_string(),
        );
        assert_eq!(tags["Pentax:PentaxModelID"], "Optio SV");
    }

    /// `Priority => 0` then normal: the stored value recorded no priority, so
    /// it is promoted to 1 and the normal value's `1 >= 1` displaces it
    /// (ExifTool.pm:9545-9551, :9564). That is a plain `insert`, and this test
    /// pins that the helper does not turn it into an `or_insert` too.
    #[test]
    fn a_normal_priority_value_still_displaces_a_low_priority_one() {
        let mut tags = HashMap::new();
        insert_low_priority(&mut tags, "Pentax:LensType".to_string(), "low".to_string());
        tags.insert("Pentax:LensType".to_string(), "normal".to_string());
        assert_eq!(tags["Pentax:LensType"], "normal");
    }

    #[test]
    fn insert_low_priority_retained_keeps_the_winner_at_the_bare_key() {
        let mut tags = HashMap::new();
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "first".to_string(),
        );
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "second".to_string(),
        );
        assert_eq!(tags["Pentax:LensType"], "first");
    }

    #[test]
    fn insert_low_priority_retained_stashes_the_shadowed_value_under_a_companion_key() {
        let mut tags = HashMap::new();
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "first".to_string(),
        );
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "second".to_string(),
        );
        assert_eq!(
            tags.get("Pentax:LensType (1)").map(String::as_str),
            Some("second")
        );
    }

    #[test]
    fn insert_low_priority_retained_numbers_a_third_shadowed_value_distinctly() {
        let mut tags = HashMap::new();
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "first".to_string(),
        );
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "second".to_string(),
        );
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "third".to_string(),
        );
        assert_eq!(tags["Pentax:LensType"], "first");
        assert_eq!(
            tags.get("Pentax:LensType (1)").map(String::as_str),
            Some("second")
        );
        assert_eq!(
            tags.get("Pentax:LensType (2)").map(String::as_str),
            Some("third")
        );
    }

    #[test]
    fn insert_low_priority_retained_shadows_a_value_an_earlier_plain_insert_established() {
        // PentaxModelID's shape: the normal (0x0005) copy is a plain
        // `tags.insert`, and the low-priority (0x0215 CameraInfo) copy
        // arrives after it and must still be retained, not silently
        // dropped, even though it never displaces the winner.
        let mut tags = HashMap::new();
        tags.insert("Pentax:PentaxModelID".to_string(), "K10D".to_string());
        insert_low_priority_retained(
            &mut tags,
            "Pentax:PentaxModelID".to_string(),
            "K10D".to_string(),
        );
        assert_eq!(tags["Pentax:PentaxModelID"], "K10D");
        assert_eq!(
            tags.get("Pentax:PentaxModelID (1)").map(String::as_str),
            Some("K10D")
        );
    }

    #[test]
    fn strip_duplicate_marker_recognizes_the_synthetic_key() {
        assert_eq!(
            strip_duplicate_marker("Pentax:LensType (1)"),
            Some("Pentax:LensType")
        );
        assert_eq!(
            strip_duplicate_marker("Pentax:LensType (12)"),
            Some("Pentax:LensType")
        );
    }

    #[test]
    fn strip_duplicate_marker_ignores_ordinary_tag_names() {
        assert_eq!(strip_duplicate_marker("Pentax:LensType"), None);
        assert_eq!(strip_duplicate_marker("Pentax:LensType ()"), None);
        assert_eq!(strip_duplicate_marker("Pentax:LensType (x)"), None);
    }

    #[test]
    fn record_makernote_tag_ends_a_full_pentax_duplicate_pair_at_the_real_key_only() {
        let mut metadata = crate::core::MetadataMap::new();
        let mut tags = HashMap::new();
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "first".to_string(),
        );
        insert_low_priority_retained(
            &mut tags,
            "Pentax:LensType".to_string(),
            "second".to_string(),
        );

        for (tag_name, value) in tags {
            record_makernote_tag(
                &mut metadata,
                tag_name,
                crate::core::TagValue::String(value),
            );
        }

        assert_eq!(metadata.get_string("Pentax:LensType"), Some("first"));
        assert!(
            metadata.get_string("Pentax:LensType (1)").is_none(),
            "the synthetic marker key must never become a real tag name"
        );
        assert_eq!(metadata.occurrences_for("Pentax:LensType").len(), 2);
    }

    /// Step 22 regression pin: `Canon:ExposureTime` (Canon.pm:2965-2994,
    /// `Priority => 0`) must not win the bare `ExposureTime` composite
    /// dependency over a normal-priority `ExifIFD:ExposureTime`, even when
    /// it is recorded *after* the EXIF one -- the ordinary case, since
    /// MakerNotes parsing runs later in the same IFD pass. Before this fix,
    /// both occurrences tied at `SHIM_DEFAULT_PRIORITY` and the tie went to
    /// whichever was recorded last (`TagSink::record`'s own rule), which
    /// is backwards: real ExifTool's `$priority >= $oldPriority` keeps the
    /// higher-priority EXIF tag under the bare name regardless of arrival
    /// order. Found on `CanonDIGITAL_IXUS100IS.jpg`
    /// (`ExifIFD:ExposureTime` "1/200" vs `Canon:ExposureTime` "1/193")
    /// when Step 22's full-corpus conformance run first exercised
    /// `cli::tag_resolution::resolve_requested_tags`'s bare-name
    /// arbitration against real MakerNote data end to end.
    #[test]
    fn canon_exposure_time_defers_to_the_normal_priority_exif_tag() {
        let mut metadata = crate::core::MetadataMap::new();
        // Recorded in file order: EXIF's ExposureTime first, Canon's
        // MakerNote copy second -- the shape every affected corpus file has.
        metadata.insert(
            "ExifIFD:ExposureTime",
            crate::core::TagValue::new_string("1/200"),
        );
        record_makernote_tag(
            &mut metadata,
            "Canon:ExposureTime".to_string(),
            crate::core::TagValue::new_string("1/193"),
        );

        let resolved = crate::cli::tag_resolution::resolve_requested_tags(
            &metadata,
            &["ExposureTime".to_string()],
            false,
        );
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].occurrence.raw,
            crate::core::TagValue::new_string("1/200"),
            "the bare ExposureTime dependency must bind the normal-priority EXIF tag"
        );
        // Both occurrences are still retained -- `-a`/`-G1` still reaches
        // Canon's own copy under its own key.
        assert_eq!(metadata.get_string("Canon:ExposureTime"), Some("1/193"));
    }

    /// Same shape, for `Nikon:ISO` (Nikon.pm:1803, `Priority => 0, # the
    /// EXIF ISO is more reliable`).
    #[test]
    fn nikon_iso_defers_to_the_normal_priority_exif_tag() {
        let mut metadata = crate::core::MetadataMap::new();
        metadata.insert("ExifIFD:ISO", crate::core::TagValue::new_integer(50));
        record_makernote_tag(
            &mut metadata,
            "Nikon:ISO".to_string(),
            crate::core::TagValue::new_integer(0),
        );

        let resolved = crate::cli::tag_resolution::resolve_requested_tags(
            &metadata,
            &["ISO".to_string()],
            false,
        );
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved[0].occurrence.raw,
            crate::core::TagValue::new_integer(50)
        );
    }

    #[test]
    fn priority_zero_duplicate_group1_is_exact_and_narrow() {
        assert_eq!(
            priority_zero_duplicate_group1("Canon:ExposureTime"),
            Some("Canon")
        );
        assert_eq!(priority_zero_duplicate_group1("Nikon:ISO"), Some("Nikon"));
        // Every other same-named MakerNote tag is unaffected -- in
        // particular CIFF:Make, which legitimately outranks IFD0:Make on
        // `ExifTool.jpg` (see `cli::tag_resolution`'s own pinned test).
        assert_eq!(priority_zero_duplicate_group1("CIFF:Make"), None);
        assert_eq!(priority_zero_duplicate_group1("Canon:LensModel"), None);
    }
}
