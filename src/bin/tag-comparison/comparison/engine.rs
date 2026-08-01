//! Tag comparison engine - Match and compare tags between OxiDex and ExifTool

use crate::models::{FormatComparison, TagInfo, ValueDifference};
use std::collections::{HashMap, HashSet};

/// Comparison engine for analyzing tag differences
pub struct ComparisonEngine;

/// Normalize a family name for comparison purposes
/// Maps manufacturer-specific families to MakerNotes for matching
pub(crate) fn normalize_family_for_comparison(family: &str) -> &str {
    // MPF -> the three family-1 groups MPF.pm files its tags under. This
    // harness asks ExifTool for family 0 (`-G`), where all of them are "MPF";
    // oxidex emits the family-1 spelling `exiftool -G1 -s` prints. The MP
    // Entry groups carry a 1-based image index (MPF.pm:247,
    // `$$et{SET_GROUP1} = '+' . ($i + 1);`) so they cannot be match arms.
    //
    //   MPF.pm:24        GROUPS => { 0 => 'MPF', 1 => 'MPF0', 2 => 'Image'}
    //   ExifTool.pm:7959 $dirInfo{Multi} = 1;  # the MP Attribute IFD will be MPF1
    //   MPF.pm:96        GROUPS => { 0 => 'MPF', 1 => 'MPImage', 2 => 'Image'}
    //
    // Same case as FLIR/AROT/SPIFF/GoPro below: without this, oxidex's correct
    // family-1 keys would count as oxidex-only extras on 737 files while
    // ExifTool's byte-identical `MPF:*` keys counted as gaps.
    if matches!(family, "MPF0" | "MPF1") || is_mp_image_group(family) {
        return "MPF";
    }
    match family {
        // Camera manufacturers -> MakerNotes
        //
        // NOTE: "GoPro" deliberately is NOT in this list -- see the GoPro arm
        // below. ExifTool never files a GoPro tag under family-0 MakerNotes.
        "Canon" | "CanonCustom" | "Nikon" | "Sony" | "Fujifilm" | "Panasonic" | "Olympus"
        | "Pentax" | "Samsung" | "Leica" | "Casio" | "Minolta" | "Sigma" | "Ricoh" | "Kodak"
        | "Sanyo" | "JVC" | "Motorola" | "HP" | "DJI" | "Apple" | "Google" | "Reconyx"
        | "Parrot" | "Infiray" | "Lytro" | "PhaseOne" | "Leaf" | "Red" | "Qualcomm"
        | "Nintendo" | "GE" | "LG" => "MakerNotes",
        // XMP namespace variants -> XMP (ExifTool often simplifies these)
        "XMP-exif" | "XMP-tiff" | "XMP-photoshop" | "XMP-iptcCore" | "XMP-iptcExt"
        | "XMP-xmpMM" | "XMP-xmpRights" | "XMP-dc" | "XMP-xmp" | "XMP-crs" | "XMP-plus"
        | "XMP-GDepth" | "XMP-GCamera" | "XMP-Device" | "XMP-darktable" | "XMP-xmpDM" => "XMP",
        // FLIR -> APP1 (ExifTool convention)
        "FLIR" => "APP1",
        // AROT is ExifTool's family-1 name for the HDR gain table stored in
        // JPEG APP10 (JPEG.pm HDRGainInfo). The harness asks ExifTool for
        // family 0 with `-G`, so reconcile OxiDex's canonical family-1 keys
        // with ExifTool's APP10 keys. OxiDex also exposes legacy `HDR:*`
        // aliases for this parser; the extractor deliberately excludes those
        // redundant aliases instead of pretending they belong to APP11.
        "AROT" => "APP10",
        // SPIFF -> APP8. SPIFF is ExifTool's own family-1 name for the
        // segment and is what oxidex emits; the harness asks exiftool for
        // family 0, which calls it APP8. Same case as FLIR and AROT above --
        // eleven byte-identical values were being counted as a gap on one
        // side and an extra on the other purely over which family named them.
        "SPIFF" => "APP8",
        // GoPro -> APP6. GoPro's JPEG metadata lives in the APP6 segment, and
        // "GoPro" is ExifTool's own family-1 name for it (GoPro.pm's GPMF
        // table declares GROUPS => { 0 => 'APP6', 1 => 'GoPro' }), which is
        // exactly what oxidex emits. Verified on every GoPro sample in the
        // corpus, e.g. GoPro/GoProHERO12Black.jpg:
        //     exiftool -G0 -s => [APP6]  MetadataVersion : 8.2.2
        //     exiftool -G1 -s => [GoPro] MetadataVersion : 8.2.2
        // and across the whole corpus family-1 "GoPro" only ever appears under
        // family-0 "APP6" (160 tags), never under MakerNotes -- which is why
        // GoPro was removed from the manufacturer list above. Mapping it there
        // sent oxidex's APP6 tags to "MakerNotes:X" while the harness compared
        // against exiftool's "APP6:X", splitting 42 same-tag pairs across a
        // gap on one side and an extra on the other; 34 of them are
        // byte-identical and match outright once reconciled, and the other 8
        // become visible value differences instead of vanishing. Note the
        // mapping is applied to BOTH sides, so GoPro-as-family-0 (which
        // ExifTool does use for GoPro MP4/GPMF tracks) still matches oxidex's
        // "GoPro:" tags.
        "GoPro" => "APP6",
        // Keep everything else as-is
        _ => family,
    }
}

/// True for `MPImage1`, `MPImage2`, ... -- ExifTool's per-MP-Entry family-1
/// groups. Not `MPImage` bare (no such group is ever emitted) and not
/// `MPImageList` (a tag name in `MPF::Main`, never a group).
fn is_mp_image_group(family: &str) -> bool {
    family
        .strip_prefix("MPImage")
        .is_some_and(|idx| !idx.is_empty() && idx.bytes().all(|b| b.is_ascii_digit()))
}

/// Normalize a tag name for comparison
fn normalize_tag_name(name: &str) -> &str {
    match name {
        // ICC profile tag names (ExifTool uses TRC, OxiDex uses ToneReproductionCurve)
        "BlueToneReproductionCurve" => "BlueTRC",
        "GreenToneReproductionCurve" => "GreenTRC",
        "RedToneReproductionCurve" => "RedTRC",
        _ => name,
    }
}

/// Normalize a tag key (family:name) for comparison
fn normalize_key_for_comparison(key: &str) -> String {
    if let Some((family, name)) = key.split_once(':') {
        let norm_family = normalize_family_for_comparison(family);
        let norm_name = normalize_tag_name(name);
        format!("{}:{}", norm_family, norm_name)
    } else {
        key.to_string()
    }
}

/// Normalize a value for comparison.
///
/// This function exists for exactly one reason: `-json`, which is how this
/// harness reads ExifTool, does not always print a value the same way
/// `exiftool -G1 -s` does -- and `-G1 -s` is the human-readable output oxidex
/// is actually trying to reproduce. Where the two ExifTool outputs disagree,
/// comparing against `-json` verbatim would report a difference that does not
/// exist in the product being measured. Those, and only those, are corrected
/// here. Each is justified against the exiftool script's own source.
///
/// Deliberately NOT keyed on the tag: this takes only the value. Until
/// 2026-07-31 it took `tag_key` as well and carried 44 further rules gated on
/// `tag_key.contains(..)` substring tests, none of which described any
/// difference between ExifTool's two output modes. They existed to make
/// numbers and enums agree -- rounding both sides to a fixed number of
/// decimals, stripping units, folding case, mapping `"Off"` to `"0"`,
/// re-rendering through `f64`. That is not measurement, it is the harness
/// answering its own question, and PR #242 identified it as what hid most of
/// the 833 destroyed number literals it had just recovered.
///
/// Measured over the full 4,238-file corpus (46 formats, ExifTool 13.59)
/// before removing them, those rules changed the reported verdict on 119
/// tag comparisons -- every one of them in the direction of scoring a
/// difference as a match. Hand-checked against `exiftool -G1 -s` and
/// `./target/release/oxidex` one at a time: 113 are real oxidex differences
/// (XMP rationals emitted unconverted as `59/10` where ExifTool prints `5.9`;
/// XMP timestamps emitted as ISO-8601 `2005-11-21T17:07:14+01:00` where
/// ExifTool prints `2005:11:21 17:07:14+01:00`; `FocalLength` missing its
/// `%.1f mm` PrintConv; `MPFVersion` read byte-swapped as `0010`; XP* tags
/// emitting `0` for an empty value; `MakerNotes:FujiFlashMode` capitalized
/// `Red-eye Reduction` where ExifTool writes `Red-eye reduction`), and 6 are
/// a separate harness defect in `oxidex_extractor.rs` -- the mirror of #242
/// on the oxidex side, where the extractor re-renders oxidex's own decimal
/// text through a float (`-4.10` -> `-4.1`) even though `oxidex -e -s` prints
/// ExifTool's exact string. That one is left for its own measured pass rather
/// than smuggled into a normalization change.
///
/// The substring tests were also wildly over-broad in their own right, which
/// is why they could not be fixed in place: `contains("Time")` matched all 82
/// `QuickTime:*` tags through the group name alone; `contains("EV")` matched
/// `QuickTime:HEVCConfigurationVersion` and `APP12:REV`; `contains("Temp")`
/// matched `QuickTime:NumTemporalLayers`; `contains("Flash") &&
/// contains("Comp")` matched `FlashPix:Company`. Any future rule must
/// therefore be value-shaped like the ones below, or be gated on an explicit
/// list of full `group:name` keys -- never on a substring of the concatenated
/// key. Removing the `tag_key` parameter is what makes that a compile-time
/// checkpoint rather than a convention.
fn normalize_value_for_comparison(value: &str) -> String {
    // 1. Trailing whitespace. ExifTool's `Printable()` strips it before
    //    printing, `-json` keeps the padding as written in the file, so
    //    `ICC_Profile:ColorSpaceData` arrives as "RGB " here but prints as
    //    "RGB" under `-G1 -s`. Applied to both sides.
    let normalized = value.trim();

    // 2. Booleans. ExifTool's JSON writer turns a PrintConv result of
    //    "True"/"False" into a bare JSON boolean and lowercases it on the
    //    way out -- from the exiftool script's EscapeJSON (13.59, line 3806):
    //
    //        return lc($str) if $str =~ /^(true|false)$/i and $json < 2;
    //
    //    So Photoshop:CopyrightFlag, whose PrintConv is { 0 => 'False',
    //    1 => 'True' } (Photoshop.pm:171-181), prints as `False` under
    //    `exiftool -G1 -s` and arrives here as `false` through `-j`.
    //
    //    Only those four spellings fold. The old rule folded any casing,
    //    which meant a value oxidex rendered `TRUE` could never be reported
    //    as differing from ExifTool's `True` -- a casing bug the harness was
    //    structurally unable to see. ExifTool's own tables spell it only
    //    `True`/`False` (nine occurrences of each across lib/Image/ExifTool,
    //    zero of `TRUE`/`FALSE`), so restricting the fold to the spellings
    //    ExifTool can actually produce costs nothing and closes the blind
    //    spot: all 33 boolean-valued instances in the corpus still match.
    if matches!(normalized, "true" | "false" | "True" | "False") {
        return normalized.to_ascii_lowercase();
    }

    // 3. List-valued tags. `-json` serializes them as a JSON array where
    //    `-G1 -s` prints the joined string, so the brackets are ExifTool's
    //    transport, not a value oxidex failed to produce.
    //
    //    NOTE: the joining here (space-separated) does not match what
    //    `-G1 -s` prints (comma-separated: `a, b, c`), so this is only half
    //    a fix. That half belongs to the XMP list-rendering work in
    //    src/parsers/xmp/rdf_parser.rs and its engine-side counterpart, and
    //    is deliberately left exactly as it was here -- changing the join
    //    would move 15 more comparisons underneath that work in flight.
    if normalized.starts_with('[') && normalized.ends_with(']') {
        let inner = &normalized[1..normalized.len() - 1];
        let items: Vec<&str> = inner
            .split(',')
            .map(|s| s.trim().trim_matches('"'))
            .collect();
        return items.join(" ");
    }

    normalized.to_string()
}

impl ComparisonEngine {
    /// Compare OxiDex and ExifTool tags for a format
    ///
    /// # Arguments
    /// * `oxidex_tags` - Tags extracted from OxiDex
    /// * `exiftool_tags` - Tags extracted from ExifTool
    /// * `format` - Format name (e.g., "JPEG")
    /// * `files_tested` - Number of files processed during extraction
    /// * `previous` - Previous comparison for regression detection (optional)
    ///
    /// # Returns
    /// FormatComparison with matched/missing/extra/regression analysis
    pub fn compare(
        oxidex_tags: Vec<TagInfo>,
        exiftool_tags: Vec<TagInfo>,
        format: &str,
        files_tested: usize,
        previous: Option<&FormatComparison>,
    ) -> FormatComparison {
        let mut comparison = FormatComparison::new(format.to_string(), files_tested);
        comparison.total_exiftool_tags = exiftool_tags.len();

        // Build lookup maps using both original and normalized keys
        // This allows matching Canon:Make with MakerNotes:Make, etc.
        let mut oxidex_by_key: HashMap<String, &TagInfo> = HashMap::new();
        let mut oxidex_by_normalized_key: HashMap<String, &TagInfo> = HashMap::new();
        for tag in &oxidex_tags {
            let key = tag.key();
            let norm_key = normalize_key_for_comparison(&key);
            oxidex_by_key.insert(key, tag);
            oxidex_by_normalized_key.insert(norm_key, tag);
        }

        // Track which OxiDex keys were matched (both original and normalized)
        let mut matched_oxidex_keys = HashSet::new();
        let mut matched_exiftool_keys = HashSet::new();

        // Compare each ExifTool tag
        for et_tag in &exiftool_tags {
            let key = et_tag.key();
            let norm_key = normalize_key_for_comparison(&key);

            // Try exact match first, then normalized match
            let ox_tag = oxidex_by_key
                .get(&key)
                .or_else(|| oxidex_by_normalized_key.get(&norm_key));

            if let Some(ox_tag) = ox_tag {
                // Tag exists in both - check if values match
                matched_exiftool_keys.insert(key.clone());
                matched_oxidex_keys.insert(ox_tag.key());

                // Normalize values for comparison to handle formatting differences
                let norm_ox = normalize_value_for_comparison(&ox_tag.value);
                let norm_et = normalize_value_for_comparison(&et_tag.value);

                if norm_ox == norm_et {
                    // Values match after normalization
                    comparison.matched_tags.push(key);
                } else {
                    // Tag exists but values differ even after normalization
                    comparison.value_differences.push(ValueDifference {
                        tag_key: key,
                        exiftool_value: et_tag.value.clone(),
                        oxidex_value: ox_tag.value.clone(),
                        source_file: et_tag.source_file.clone().unwrap_or_default(),
                    });
                }
            } else {
                // Tag missing in OxiDex
                comparison.missing_in_oxidex.push(et_tag.clone());
            }
        }

        // Find extra tags in OxiDex (not matched to any ExifTool tag)
        for ox_tag in &oxidex_tags {
            let key = ox_tag.key();
            if !matched_oxidex_keys.contains(&key) {
                comparison.extra_in_oxidex.push(ox_tag.clone());
            }
        }

        // Spec M3: any oxidex-side tag key that repeats for the SAME
        // sample file WITH A DIFFERENT VALUE is a duplicate emission --
        // the deterministic gate the literal-string diff backstop
        // (detect_duplicate_tag_insertion in model_fix_loop.py) is blind
        // to for registry/dynamic-name emitters. Grouped by (source_file,
        // key) so the same key appearing once each in several DIFFERENT
        // sample files is never mistaken for a duplicate; a missing
        // source_file collapses to one shared "no file" bucket, which is
        // correct as long as the caller's oxidex_tags already carries
        // per-file provenance.
        //
        // Requiring a value mismatch (not just a repeated key) avoids a
        // false-positive that blocked every squad's batch full-corpus
        // check on real-world JPEGs: many cameras genuinely write the
        // same tag ID into both IFD0 and the ExifIFD with an identical
        // value (confirmed across Samsung/Canon/Nikon/Olympus/Panasonic/
        // FujiFilm/Leica samples) -- oxidex correctly keeps both as
        // distinct "IFD0:X"/"ExifIFD:X" entries, and only this tool's own
        // ExifTool-style group collapse (normalize_for_comparison, below)
        // merges them onto one displayed key. That's expected redundancy,
        // not a bug worth gating publication on. Two DIFFERENT values
        // colliding on one key, though, is exactly the dynamic/registry
        // double-emission this gate exists to catch.
        let mut per_file_values: HashMap<(String, String), HashSet<String>> = HashMap::new();
        for tag in &oxidex_tags {
            let source = tag.source_file.clone().unwrap_or_default();
            per_file_values
                .entry((source, tag.key()))
                .or_default()
                .insert(tag.value.clone());
        }
        let mut duplicate_keys: HashSet<String> = HashSet::new();
        for ((_source, key), values) in per_file_values {
            if values.len() > 1 {
                duplicate_keys.insert(key);
            }
        }
        comparison.duplicate_emissions = duplicate_keys.into_iter().collect();
        comparison.duplicate_emissions.sort();

        // Detect regressions: tags that were in previous.matched_tags but NOT in current matched_tags
        if let Some(prev) = previous {
            let current_matched: HashSet<_> = comparison.matched_tags.iter().collect();
            for prev_tag in &prev.matched_tags {
                if !current_matched.contains(prev_tag) {
                    comparison.regressions.push(prev_tag.clone());
                }
            }
        }

        // Calculate coverage
        comparison.calculate_coverage();

        comparison
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_all_matched() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(result.matched_tags.len(), 2);
        assert_eq!(result.missing_in_oxidex.len(), 0);
        assert_eq!(result.extra_in_oxidex.len(), 0);
        assert_eq!(result.coverage_percentage, 100.0);
    }

    #[test]
    fn test_compare_partial_match() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            // Model is missing
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(result.matched_tags.len(), 1);
        assert_eq!(result.missing_in_oxidex.len(), 1);
        assert_eq!(result.extra_in_oxidex.len(), 0);
        assert_eq!(result.coverage_percentage, 50.0);
    }

    #[test]
    fn test_compare_with_extra_tags() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
            TagInfo::new(
                "CustomTag".to_string(),
                "EXIF".to_string(),
                "Custom".to_string(),
            ),
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(result.matched_tags.len(), 2);
        assert_eq!(result.missing_in_oxidex.len(), 0);
        assert_eq!(result.extra_in_oxidex.len(), 1);
        assert_eq!(result.coverage_percentage, 100.0);
    }

    #[test]
    fn test_compare_empty_oxidex() {
        let oxidex_tags = vec![];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(result.matched_tags.len(), 0);
        assert_eq!(result.missing_in_oxidex.len(), 2);
        assert_eq!(result.extra_in_oxidex.len(), 0);
        assert_eq!(result.coverage_percentage, 0.0);
    }

    #[test]
    fn test_colorclass_parenthetical_is_a_real_difference() {
        // Reproduces Olympus/OlympusOM-3.jpg. Both ExifTool output modes
        // agree, so there is nothing for this harness to compensate for:
        //     exiftool -G1 -s => [XMP-photomech] ColorClass : 0 (None)
        //     exiftool -j -G  => "XMP:ColorClass": "0 (None)"
        //     oxidex -e -s    => ColorClass: 0
        // oxidex is not applying the PrintConv; that is an oxidex difference
        // and must be reported. The rule that used to truncate ExifTool's
        // value at " (" made it invisible.
        assert_eq!(normalize_value_for_comparison("0 (None)"), "0 (None)");
        assert_eq!(normalize_value_for_comparison("0"), "0");

        let result = ComparisonEngine::compare(
            vec![TagInfo::new(
                "ColorClass".to_string(),
                "XMP".to_string(),
                "0".to_string(),
            )],
            vec![TagInfo::new(
                "ColorClass".to_string(),
                "XMP".to_string(),
                "0 (None)".to_string(),
            )],
            "JPEG",
            1,
            None,
        );
        assert!(result.matched_tags.is_empty());
        assert_eq!(result.value_differences.len(), 1);
        assert_eq!(result.value_differences[0].exiftool_value, "0 (None)");
        assert_eq!(result.value_differences[0].oxidex_value, "0");
    }

    #[test]
    fn test_trailing_whitespace_is_the_json_transport_not_a_difference() {
        // ExifTool's Printable() strips trailing padding before printing;
        // `-json` keeps it. ICC_Profile:ColorSpaceData is "RGB " in the file
        // and "RGB" under `-G1 -s`, so both sides must fold to one string.
        assert_eq!(normalize_value_for_comparison("RGB "), "RGB");
        assert_eq!(normalize_value_for_comparison("RGB"), "RGB");
    }

    #[test]
    fn test_only_exiftools_own_boolean_spellings_fold() {
        // The transport artifact this compensates for: EscapeJSON lowercases
        // a "True"/"False" PrintConv result on the way into `-j`, so the two
        // spellings are one value (Photoshop:CopyrightFlag).
        assert_eq!(normalize_value_for_comparison("False"), "false");
        assert_eq!(normalize_value_for_comparison("false"), "false");
        assert_eq!(normalize_value_for_comparison("True"), "true");
        assert_eq!(normalize_value_for_comparison("true"), "true");

        // ...but nothing else folds. ExifTool's tables never spell it
        // "TRUE"/"FALSE", so an oxidex value in that casing is a genuine
        // rendering difference and has to stay visible. The old rule folded
        // any casing and made it permanently undetectable.
        assert_eq!(normalize_value_for_comparison("TRUE"), "TRUE");
        assert_eq!(normalize_value_for_comparison("FALSE"), "FALSE");
        assert_ne!(
            normalize_value_for_comparison("TRUE"),
            normalize_value_for_comparison("True")
        );

        // "Yes"/"No" are not booleans in the JSON transport -- ExifTool
        // prints them as strings in both modes (XMP:Tagged : No), so oxidex
        // answering "false" is a difference, not a spelling.
        assert_ne!(
            normalize_value_for_comparison("No"),
            normalize_value_for_comparison("false")
        );
    }

    /// Distinct number texts must stay distinct -- the property PR #242
    /// restored on the extractor side and that the old normalization then
    /// destroyed again at compare time. Each pair below is a real corpus
    /// case where `exiftool -G1 -s` and `oxidex -e -s` print different
    /// strings and the harness scored them as equal.
    #[test]
    fn test_distinct_number_texts_stay_distinct() {
        for (exiftool, oxidex) in [
            // EXIF:FocalLength -- ExifTool's PrintConv is "%.1f mm"
            ("15.0 mm", "15 mm"),
            // XMP:FNumber -- oxidex emits the rational unconverted
            ("5.9", "59/10"),
            // XMP:AbsoluteAltitude -- DJI writes a signed, padded decimal
            ("+42.90", "42.9"),
            // XMP:PoseHeadingDegrees
            ("0.000000", "0"),
            // EXIF:FocalPlaneXResolution
            ("6514.65798", "6514.657980456"),
            // EXIF:GPSAltitude
            ("27.99831776 m", "28.0 m"),
            // ICC_Profile:MeasurementFlare
            ("0.999%", "0.99945%"),
            // MPF:MPFVersion -- oxidex reads the version bytes byte-swapped
            ("0100", "0010"),
            // EXIF:Acceleration -- oxidex renders a negative zero
            ("0", "-0"),
            // XMP:MetadataDate -- oxidex drops the offset and uses ISO-8601
            ("2016:05:18 12:54:01-05:00", "2016:05:18 12:54:01"),
            ("2004:02:26", "2004-02-26"),
            // MakerNotes:FaceDetect -- PrintConv not applied
            ("Off", "0"),
            // EXIF:XPTitle -- empty XP tag emitted as "0"
            ("", "0"),
            // MakerNotes:FujiFlashMode -- casing
            ("Red-eye reduction", "Red-eye Reduction"),
            // EXIF:GPSDestLatitudeRef
            ("North", "N"),
            // MakerNotes:PanoramaAngle -- oxidex adds a unit ExifTool omits
            ("360", "360 deg"),
            // X3F SigmaRaw:SensorTemperature -- oxidex omits the unit
            ("20 C", "20"),
        ] {
            assert_ne!(
                normalize_value_for_comparison(exiftool),
                normalize_value_for_comparison(oxidex),
                "{exiftool:?} and {oxidex:?} are different strings in \
                 ExifTool's and oxidex's own output and must be reported as \
                 a difference"
            );
        }
    }

    /// The one thing that legitimately collapses: `-json` arrays. Left
    /// exactly as-is; the join style is owned by the XMP list-rendering work.
    #[test]
    fn test_json_array_transport_still_collapses() {
        assert_eq!(
            normalize_value_for_comparison(r#"["ExifTool","Test","XMP"]"#),
            "ExifTool Test XMP"
        );
    }

    #[test]
    fn test_compare_empty_exiftool() {
        let oxidex_tags = vec![TagInfo::new(
            "Make".to_string(),
            "EXIF".to_string(),
            "Canon".to_string(),
        )];
        let exiftool_tags = vec![];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(result.matched_tags.len(), 0);
        assert_eq!(result.missing_in_oxidex.len(), 0);
        assert_eq!(result.extra_in_oxidex.len(), 1);
        assert_eq!(result.coverage_percentage, 0.0);
    }

    #[test]
    fn test_regression_detection() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            // Model is now missing - this is a regression
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        // Previous baseline had both tags matched
        let mut previous = FormatComparison::new("JPEG".to_string(), 2);
        previous.matched_tags = vec!["EXIF:Make".to_string(), "EXIF:Model".to_string()];

        let result =
            ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 2, Some(&previous));

        // Should have 1 regression (Model is missing)
        assert_eq!(result.regressions.len(), 1);
        assert!(result.regressions.contains(&"EXIF:Model".to_string()));

        // Should have 1 matched tag (Make)
        assert_eq!(result.matched_tags.len(), 1);
        assert!(result.matched_tags.contains(&"EXIF:Make".to_string()));

        // Model should be in missing_in_oxidex
        assert_eq!(result.missing_in_oxidex.len(), 1);
        assert_eq!(result.missing_in_oxidex[0].name, "Model");
    }

    #[test]
    fn test_regression_detection_no_previous() {
        let oxidex_tags = vec![TagInfo::new(
            "Make".to_string(),
            "EXIF".to_string(),
            "Canon".to_string(),
        )];
        let exiftool_tags = vec![TagInfo::new(
            "Make".to_string(),
            "EXIF".to_string(),
            "Canon".to_string(),
        )];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);

        // No regressions when there's no previous baseline
        assert_eq!(result.regressions.len(), 0);
    }

    #[test]
    fn test_regression_detection_no_regressions() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
        ];

        // Previous baseline had only one tag
        let mut previous = FormatComparison::new("JPEG".to_string(), 1);
        previous.matched_tags = vec!["EXIF:Make".to_string()];

        let result =
            ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, Some(&previous));

        // No regressions - we still have Make, and we added Model
        assert_eq!(result.regressions.len(), 0);
        assert_eq!(result.matched_tags.len(), 2);
    }

    #[test]
    fn test_value_difference_detection() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new(
                "Model".to_string(),
                "EXIF".to_string(),
                "EOS 5D".to_string(),
            ), // Different value
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new(
                "Model".to_string(),
                "EXIF".to_string(),
                "5D Mark II".to_string(),
            ), // Different value
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);

        // Make should match perfectly
        assert_eq!(result.matched_tags.len(), 1);
        assert!(result.matched_tags.contains(&"EXIF:Make".to_string()));

        // Model should have value difference
        assert_eq!(result.value_differences.len(), 1);
        assert_eq!(result.value_differences[0].tag_key, "EXIF:Model");
        assert_eq!(result.value_differences[0].exiftool_value, "5D Mark II");
        assert_eq!(result.value_differences[0].oxidex_value, "EOS 5D");

        // Nothing should be missing or extra
        assert_eq!(result.missing_in_oxidex.len(), 0);
        assert_eq!(result.extra_in_oxidex.len(), 0);
    }

    #[test]
    fn test_complex_comparison_with_all_categories() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()), // Match
            TagInfo::new(
                "Model".to_string(),
                "EXIF".to_string(),
                "EOS 5D".to_string(),
            ), // Value diff
            TagInfo::new(
                "CustomTag".to_string(),
                "EXIF".to_string(),
                "Custom".to_string(),
            ), // Extra
                                                                                       // DateTime is missing - will be a regression
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
            TagInfo::new(
                "Model".to_string(),
                "EXIF".to_string(),
                "5D Mark II".to_string(),
            ),
            TagInfo::new(
                "DateTime".to_string(),
                "EXIF".to_string(),
                "2025:12:07 10:00:00".to_string(),
            ),
            TagInfo::new("ISO".to_string(), "EXIF".to_string(), "400".to_string()), // Missing in oxidex
        ];

        // Previous had Make and DateTime
        let mut previous = FormatComparison::new("JPEG".to_string(), 1);
        previous.matched_tags = vec!["EXIF:Make".to_string(), "EXIF:DateTime".to_string()];

        let result =
            ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, Some(&previous));

        // Matched: Make
        assert_eq!(result.matched_tags.len(), 1);
        assert!(result.matched_tags.contains(&"EXIF:Make".to_string()));

        // Value differences: Model
        assert_eq!(result.value_differences.len(), 1);
        assert_eq!(result.value_differences[0].tag_key, "EXIF:Model");

        // Missing in OxiDex: DateTime, ISO
        assert_eq!(result.missing_in_oxidex.len(), 2);
        let missing_names: Vec<_> = result.missing_in_oxidex.iter().map(|t| &t.name).collect();
        assert!(missing_names.contains(&&"DateTime".to_string()));
        assert!(missing_names.contains(&&"ISO".to_string()));

        // Extra in OxiDex: CustomTag
        assert_eq!(result.extra_in_oxidex.len(), 1);
        assert_eq!(result.extra_in_oxidex[0].name, "CustomTag");

        // Regressions: DateTime (was in previous, not in current matched)
        assert_eq!(result.regressions.len(), 1);
        assert!(result.regressions.contains(&"EXIF:DateTime".to_string()));

        // Coverage: 1 matched out of 4 total = 25%
        assert_eq!(result.coverage_percentage, 25.0);
    }

    #[test]
    fn test_duplicate_emission_same_file_same_key_is_flagged() {
        // Spec M3: two oxidex TagInfo entries sharing (source_file, key)
        // -- the deterministic double-emission gate.
        let oxidex_tags = vec![
            TagInfo::new(
                "AELButton".to_string(),
                "MakerNotes".to_string(),
                "1".to_string(),
            )
            .with_source_file("canon.jpg".to_string()),
            TagInfo::new(
                "AELButton".to_string(),
                "MakerNotes".to_string(),
                "2".to_string(),
            )
            .with_source_file("canon.jpg".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new(
                "AELButton".to_string(),
                "MakerNotes".to_string(),
                "1".to_string(),
            )
            .with_source_file("canon.jpg".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(
            result.duplicate_emissions,
            vec!["MakerNotes:AELButton".to_string()]
        );
    }

    #[test]
    fn test_canon_custom_family_normalizes_to_makernotes() {
        assert_eq!(normalize_family_for_comparison("CanonCustom"), "MakerNotes");
    }

    #[test]
    fn test_same_key_different_files_is_not_a_duplicate_emission() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string())
                .with_source_file("a.jpg".to_string()),
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string())
                .with_source_file("b.jpg".to_string()),
        ];
        let exiftool_tags = vec![];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 2, None);
        assert!(result.duplicate_emissions.is_empty());
    }

    #[test]
    fn test_same_key_same_value_same_file_is_not_a_duplicate_emission() {
        // Real-world case: a camera writes the same tag ID (e.g. Padding,
        // 0xEA1C) into both IFD0 and the ExifIFD with an identical value.
        // oxidex correctly keeps these as distinct "IFD0:Padding" /
        // "ExifIFD:Padding" raw keys; this tool's own ExifTool-style
        // group collapse merges them onto one displayed key downstream,
        // but that's expected redundancy, not a genuine double-emission
        // bug -- must not be flagged.
        let oxidex_tags = vec![
            TagInfo::new("Padding".to_string(), "EXIF".to_string(), "0".to_string())
                .with_source_file("canon.jpg".to_string()),
            TagInfo::new("Padding".to_string(), "EXIF".to_string(), "0".to_string())
                .with_source_file("canon.jpg".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new("Padding".to_string(), "EXIF".to_string(), "0".to_string())
                .with_source_file("canon.jpg".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert!(result.duplicate_emissions.is_empty());
    }

    #[test]
    fn test_no_duplicate_emissions_in_the_ordinary_case() {
        let oxidex_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string())
                .with_source_file("a.jpg".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string())
                .with_source_file("a.jpg".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert!(result.duplicate_emissions.is_empty());
    }

    #[test]
    fn test_gopro_family1_matches_exiftool_app6_family0() {
        // ExifTool's GoPro APP6 segment: family 0 is "APP6", family 1 is
        // "GoPro". The harness runs `exiftool -G` (family 0) while oxidex
        // emits the family-1 name, so without the mapping these
        // byte-identical values were a gap on one side and an extra on the
        // other. Reproduces GoPro/GoProHERO12Black.jpg.
        let oxidex_tags = vec![
            TagInfo::new(
                "MetadataVersion".to_string(),
                "GoPro".to_string(),
                "8.2.2".to_string(),
            )
            .with_source_file("GoProHERO12Black.jpg".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new(
                "MetadataVersion".to_string(),
                "APP6".to_string(),
                "8.2.2".to_string(),
            )
            .with_source_file("GoProHERO12Black.jpg".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(
            result.matched_tags,
            vec!["APP6:MetadataVersion".to_string()]
        );
        assert!(result.missing_in_oxidex.is_empty());
        assert!(result.extra_in_oxidex.is_empty());
    }

    #[test]
    fn test_arot_family1_matches_exiftool_app10_family0() {
        let oxidex_tags = vec![
            TagInfo::new(
                "HDRGainCurveSize".to_string(),
                "AROT".to_string(),
                "152".to_string(),
            )
            .with_source_file("Apple_iPadAir_3rd_generation.jpg".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new(
                "HDRGainCurveSize".to_string(),
                "APP10".to_string(),
                "152".to_string(),
            )
            .with_source_file("Apple_iPadAir_3rd_generation.jpg".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 1, None);
        assert_eq!(
            result.matched_tags,
            vec!["APP10:HDRGainCurveSize".to_string()]
        );
        assert!(result.missing_in_oxidex.is_empty());
        assert!(result.extra_in_oxidex.is_empty());
    }

    #[test]
    fn test_gopro_is_not_folded_into_makernotes() {
        // GoPro must NOT normalize to MakerNotes: ExifTool never files a
        // GoPro tag under family-0 MakerNotes, so folding it there both
        // missed the real APP6 match and risked matching an unrelated
        // camera's MakerNotes tag of the same name.
        let oxidex_tags = vec![
            TagInfo::new(
                "ColorMode".to_string(),
                "GoPro".to_string(),
                "GoPro Color".to_string(),
            )
            .with_source_file("GoProHERO12Black.jpg".to_string()),
        ];
        let exiftool_tags = vec![
            TagInfo::new(
                "ColorMode".to_string(),
                "MakerNotes".to_string(),
                "Standard".to_string(),
            )
            .with_source_file("Casio.jpg".to_string()),
        ];

        let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", 2, None);
        assert!(result.matched_tags.is_empty());
        assert!(result.value_differences.is_empty());
        assert_eq!(result.missing_in_oxidex.len(), 1);
        assert_eq!(result.extra_in_oxidex.len(), 1);
    }
}
