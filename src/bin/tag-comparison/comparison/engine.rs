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
        "Canon" | "CanonCustom" | "Nikon" | "Sony" | "FujiFilm" | "Panasonic" | "Olympus"
        | "Pentax" | "Samsung" | "Leica" | "Casio" | "Minolta" | "Sigma" | "Ricoh" | "Kodak"
        | "Sanyo" | "JVC" | "Motorola" | "HP" | "DJI" | "Apple" | "Google" | "Reconyx"
        | "Parrot" | "InfiRay" | "Lytro" | "PhaseOne" | "Leaf" | "Red" | "Qualcomm"
        | "Nintendo" | "GE" | "LG" | "CIFF" => "MakerNotes",
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
        // Vivo's JPEG trailer table declares `GROUPS => { 0 => 'Trailer',
        // 1 => 'Vivo' }`.  OxiDex keeps the product-facing family-1 key,
        // while this comparison uses ExifTool family 0, so align the two.
        "Vivo" => "Trailer",
        // CanonDR4 -> CanonVRD. Same case as FLIR, AROT and SPIFF above:
        // `%CanonVRD::DR4` declares GROUPS => { 1 => 'CanonDR4' } with no
        // family-0 override, so ExifTool files a DPP 4 recipe tag under
        // family-0 CanonVRD and family-1 CanonDR4:
        //     exiftool -G0:1 -s combined-samples/CanonVRD.dr4
        //         => [CanonVRD:CanonDR4] Rotation : 0
        // oxidex emits the family-1 name, so without this the 93 tags of
        // CanonVRD.dr4 counted as 93 missing on one side and 93 extra on the
        // other despite being byte-identical.
        "CanonDR4" => "CanonVRD",
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
        // Legacy ICC spellings. The ICC registry now emits ExifTool's real tag
        // Names (`RedTRC`, ...) rather than their `Description`s, so these arms
        // no longer fire for oxidex output; they remain so an older baseline
        // still lines up against a current run.
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

/// The first (deterministic: scanned in `ox_instances`' stored order) pair
/// of per-file instances for `key` that share a `source_file` on both
/// sides, or `None` if the key's oxidex and exiftool occurrences never
/// land on the same file.
fn find_same_file_pair<'a>(
    key: &str,
    oxidex_instances: &'a HashMap<String, Vec<TagInfo>>,
    exiftool_instances: &'a HashMap<String, Vec<TagInfo>>,
) -> Option<(&'a TagInfo, &'a TagInfo)> {
    let ox_list = oxidex_instances.get(key)?;
    let et_list = exiftool_instances.get(key)?;
    let mut et_by_file: HashMap<&str, &TagInfo> = HashMap::new();
    for t in et_list {
        if let Some(sf) = t.source_file.as_deref() {
            et_by_file.entry(sf).or_insert(t);
        }
    }
    for ox in ox_list {
        if let Some(sf) = ox.source_file.as_deref()
            && let Some(et) = et_by_file.get(sf)
        {
            return Some((ox, et));
        }
    }
    None
}

/// Per-(file, tag) coverage: `(matched_instances, total_exiftool_instances)`.
///
/// Returns raw counts rather than a percentage so the report can sum them
/// across formats before dividing — averaging per-format percentages would let
/// a 3-tag format outweigh a 3,000-tag one.
///
/// This is the counterpart to the distinct-key ratio the harness has always
/// reported, and it exists because that ratio is not a coverage measurement.
/// Both sides are deduplicated to a set of `family:name` keys before it is
/// taken, which drops two things at once: the 4,085 JPEGs in the corpus
/// collapse to ~3,700 keys, and a key counts as "matched" when oxidex emits it
/// on *any* file — not necessarily the file ExifTool read it from. Measured on
/// ExifTool's own `t/images` (194 files, ExifTool 13.59), the two disagree by
/// 22 points: 97.1% by distinct key, 75.4% per (file, tag).
///
/// An ExifTool instance counts as matched only when oxidex emits the same
/// normalized key **on the same source file** and the two values agree under
/// [`normalize_value_for_comparison`] — the same key and value rules the
/// distinct-key path uses, so the two numbers differ in what they count, not
/// in how they decide a match.
fn count_instance_coverage(
    oxidex_instances: &HashMap<String, Vec<TagInfo>>,
    exiftool_instances: &HashMap<String, Vec<TagInfo>>,
) -> (usize, usize) {
    // Two indexes, mirroring the exact-then-normalized rule the distinct-key
    // path uses: `Canon:Make` and `MakerNotes:Make` must meet, but an exact
    // key match is preferred over one that only survives normalization.
    let mut ox_exact: HashMap<(&str, String), &TagInfo> = HashMap::new();
    let mut ox_normalized: HashMap<(&str, String), &TagInfo> = HashMap::new();
    for list in oxidex_instances.values() {
        for tag in list {
            let Some(sf) = tag.source_file.as_deref() else {
                continue;
            };
            let key = tag.key();

            // Deterministic tie-break, NOT `or_insert`. These maps are built by
            // iterating a HashMap, so "whichever arrived first" is whatever
            // order the allocator and hash seed produced that run. Distinct
            // keys can collide after normalization (two XMP namespaces both
            // become `XMP:`), and if their values differ, `or_insert` would let
            // the published percentage flicker between runs over nothing.
            // Smallest original key wins, which is stable across processes.
            match ox_normalized.entry((sf, normalize_key_for_comparison(&key))) {
                std::collections::hash_map::Entry::Occupied(mut e) => {
                    if key < e.get().key() {
                        e.insert(tag);
                    }
                }
                std::collections::hash_map::Entry::Vacant(e) => {
                    e.insert(tag);
                }
            }

            // Exact keys cannot collide within a file except via a genuine
            // double emission, which `duplicate_emissions` reports separately;
            // tie-break on value so the choice is still order-independent.
            ox_exact
                .entry((sf, key))
                .and_modify(|existing| {
                    if tag.value < existing.value {
                        *existing = tag;
                    }
                })
                .or_insert(tag);
        }
    }

    let mut matched = 0usize;
    let mut total = 0usize;
    for list in exiftool_instances.values() {
        for et in list {
            // An instance with no source file cannot be attributed to a file,
            // so it cannot be scored per file either. Skipped from BOTH sides
            // of the ratio rather than counted as a miss.
            let Some(sf) = et.source_file.as_deref() else {
                continue;
            };
            total += 1;
            let key = et.key();
            let ox = ox_exact
                .get(&(sf, key.clone()))
                .or_else(|| ox_normalized.get(&(sf, normalize_key_for_comparison(&key))));
            if let Some(ox) = ox
                && normalize_value_for_comparison(&ox.value)
                    == normalize_value_for_comparison(&et.value)
            {
                matched += 1;
            }
        }
    }
    (matched, total)
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
    ///
    /// Production code goes through [`Self::compare_with_instances`]; this
    /// wrapper exists so the tests keep pinning the documented equivalence:
    /// empty instance maps reproduce the old canonical-value comparison
    /// exactly.
    #[cfg(test)]
    pub fn compare(
        oxidex_tags: Vec<TagInfo>,
        exiftool_tags: Vec<TagInfo>,
        format: &str,
        files_tested: usize,
        previous: Option<&FormatComparison>,
    ) -> FormatComparison {
        let empty = HashMap::new();
        Self::compare_with_instances(
            oxidex_tags,
            exiftool_tags,
            format,
            files_tested,
            previous,
            &empty,
            &empty,
        )
    }

    /// Same as [`Self::compare`], but additionally takes each side's
    /// per-(file, value) instances (`ExtractionResult::all_instances`) so
    /// `value_differences` can require both sides' values come from the
    /// SAME source file.
    ///
    /// `oxidex_tags`/`exiftool_tags` are already collapsed to one
    /// "canonical" `TagInfo` per `family:name` key across the whole
    /// corpus (first file found, in whatever order the extractor visited
    /// files) -- fine for `matched_tags`/`missing_in_oxidex`/
    /// `extra_in_oxidex`, which only test key presence, but wrong for a
    /// same-file value comparison whenever a tag name recurs across
    /// different files with legitimately different values: the two
    /// canonical `TagInfo`s can come from two DIFFERENT files (different
    /// camera bodies, for a MakerNotes binary-data tag), so comparing
    /// them reports two unrelated real values as one file's mismatch.
    /// Concrete case that motivated this: Sony's `AFStatus*` tags
    /// compared `SonyDSLR-A580.jpg`'s real ExifTool value against
    /// `SonySLT-A65.jpg`'s real (and, for A65, itself correct) OxiDex
    /// value, because A65 sorted before A580 in the corpus walk and won
    /// the OxiDex-side "first file wins" slot for that key -- appearing
    /// as if OxiDex fabricated a garbage value for A580 specifically,
    /// batch-size dependent purely because a large-enough Sony sample set
    /// was needed to include another camera with the same tag names.
    ///
    /// When the instance maps are empty (as `compare` passes), this is
    /// exactly the old canonical-value comparison -- existing callers and
    /// tests are unaffected.
    #[allow(clippy::too_many_arguments)]
    pub fn compare_with_instances(
        oxidex_tags: Vec<TagInfo>,
        exiftool_tags: Vec<TagInfo>,
        format: &str,
        files_tested: usize,
        previous: Option<&FormatComparison>,
        oxidex_instances: &HashMap<String, Vec<TagInfo>>,
        exiftool_instances: &HashMap<String, Vec<TagInfo>>,
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

        let have_instances = !oxidex_instances.is_empty() || !exiftool_instances.is_empty();

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

                // Prefer values that both come from the SAME source file
                // over the (possibly unrelated) corpus-wide canonical
                // pair -- see the doc comment above.
                let same_file_pair = if have_instances {
                    find_same_file_pair(&key, oxidex_instances, exiftool_instances).or_else(|| {
                        find_same_file_pair(&norm_key, oxidex_instances, exiftool_instances)
                    })
                } else {
                    None
                };

                let (ox_value, et_value, diff_source) = match same_file_pair {
                    Some((ox_inst, et_inst)) => (
                        &ox_inst.value,
                        &et_inst.value,
                        et_inst.source_file.clone().unwrap_or_default(),
                    ),
                    None if have_instances => {
                        // Both sides have this key SOMEWHERE in the
                        // corpus, but never on the same file -- there is
                        // no evidence a same-file mismatch exists, and
                        // reporting the two (necessarily different)
                        // files' canonical values as if they were one
                        // file's before/after would be exactly the
                        // fabrication AGENTS.md rules out. Count the key
                        // as matched (unchanged from the presence-only
                        // semantics `matched_tags` always used) and move
                        // on without a value_differences entry.
                        comparison.matched_tags.push(key);
                        continue;
                    }
                    None => (
                        &ox_tag.value,
                        &et_tag.value,
                        et_tag.source_file.clone().unwrap_or_default(),
                    ),
                };

                // Normalize values for comparison to handle formatting differences
                let norm_ox = normalize_value_for_comparison(ox_value);
                let norm_et = normalize_value_for_comparison(et_value);

                if norm_ox == norm_et {
                    // Values match after normalization
                    comparison.matched_tags.push(key);
                } else {
                    // Tag exists but values differ even after normalization
                    comparison.value_differences.push(ValueDifference {
                        tag_key: key,
                        exiftool_value: et_value.clone(),
                        oxidex_value: ox_value.clone(),
                        source_file: diff_source,
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

        // Per-(file, tag) coverage, from the same instance maps the value
        // comparison above already uses. The `#[cfg(test)]` `compare` wrapper
        // passes empty maps, which correctly yields 0/0 -> not measurable.
        let (matched_instances, total_instances) =
            count_instance_coverage(oxidex_instances, exiftool_instances);
        comparison.matched_instances = matched_instances;
        comparison.total_exiftool_instances = total_instances;
        comparison.calculate_instance_coverage();

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
    fn test_vivo_family1_normalizes_to_exiftool_trailer_family0() {
        assert_eq!(normalize_family_for_comparison("Vivo"), "Trailer");
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

    /// Build an `all_instances` map from `(file, family, name, value)` rows.
    fn instances(rows: &[(&str, &str, &str, &str)]) -> HashMap<String, Vec<TagInfo>> {
        let mut map: HashMap<String, Vec<TagInfo>> = HashMap::new();
        for (file, family, name, value) in rows {
            let tag = TagInfo::new(name.to_string(), family.to_string(), value.to_string())
                .with_source_file(file.to_string());
            map.entry(format!("{}:{}", family, name))
                .or_default()
                .push(tag);
        }
        map
    }

    /// The whole reason the instance metric exists: a tag ExifTool reads from
    /// three files that oxidex reads from only one is 100% by distinct key and
    /// 33% per (file, tag). The published report used to headline the former.
    #[test]
    fn instance_coverage_does_not_credit_a_key_matched_on_one_file_only() {
        let et = instances(&[
            ("a.jpg", "EXIF", "Make", "Canon"),
            ("b.jpg", "EXIF", "Make", "Canon"),
            ("c.jpg", "EXIF", "Make", "Canon"),
        ]);
        let ox = instances(&[("a.jpg", "EXIF", "Make", "Canon")]);

        let (matched, total) = count_instance_coverage(&ox, &et);
        assert_eq!((matched, total), (1, 3));

        // ...while the distinct-key path calls the same data fully covered.
        let result = ComparisonEngine::compare_with_instances(
            vec![TagInfo::new(
                "Make".to_string(),
                "EXIF".to_string(),
                "Canon".to_string(),
            )],
            vec![TagInfo::new(
                "Make".to_string(),
                "EXIF".to_string(),
                "Canon".to_string(),
            )],
            "JPEG",
            3,
            None,
            &ox,
            &et,
        );
        assert_eq!(result.coverage_percentage, 100.0);
        assert!((result.instance_coverage_percentage - 100.0 / 3.0).abs() < 1e-9);
        assert!(result.is_measurable());
    }

    /// A match must be same-file. Right key on the wrong file is not coverage.
    #[test]
    fn instance_coverage_requires_the_same_source_file() {
        let et = instances(&[("a.jpg", "EXIF", "Make", "Canon")]);
        let ox = instances(&[("b.jpg", "EXIF", "Make", "Canon")]);
        assert_eq!(count_instance_coverage(&ox, &et), (0, 1));
    }

    /// Same file, same key, different value is a miss — not a free match.
    #[test]
    fn instance_coverage_requires_the_value_to_agree() {
        let et = instances(&[("a.jpg", "EXIF", "ISO", "100")]);
        let ox = instances(&[("a.jpg", "EXIF", "ISO", "200")]);
        assert_eq!(count_instance_coverage(&ox, &et), (0, 1));
    }

    /// Family normalization applies to instances too, so `Canon:Make` and
    /// `MakerNotes:Make` are the same tag here exactly as they are elsewhere.
    #[test]
    fn instance_coverage_normalizes_families() {
        let et = instances(&[("a.jpg", "MakerNotes", "LensType", "EF 50mm")]);
        let ox = instances(&[("a.jpg", "Canon", "LensType", "EF 50mm")]);
        assert_eq!(count_instance_coverage(&ox, &et), (1, 1));
    }

    /// An exact key match beats one that only survives normalization, so a
    /// same-named tag in a second namespace cannot displace the real one.
    #[test]
    fn instance_coverage_prefers_an_exact_key_over_a_normalized_one() {
        let et = instances(&[("a.jpg", "XMP", "Title", "right")]);
        let ox = instances(&[
            ("a.jpg", "XMP-dc", "Title", "wrong"),
            ("a.jpg", "XMP", "Title", "right"),
        ]);
        assert_eq!(count_instance_coverage(&ox, &et), (1, 1));
    }

    /// Two distinct keys collapsing onto one normalized key must resolve the
    /// same way on every run. These maps are built by iterating a HashMap, so
    /// an `or_insert` "first wins" would pick whichever the hash seed happened
    /// to yield and let the published percentage flicker between deploys.
    #[test]
    fn instance_coverage_is_stable_when_normalized_keys_collide() {
        let et = instances(&[("a.jpg", "XMP", "Rights", "b-value")]);
        // Both oxidex keys normalize to XMP:Rights with different values, and
        // neither matches ExifTool's key exactly.
        let colliding = [
            ("a.jpg", "XMP-dc", "Rights", "a-value"),
            ("a.jpg", "XMP-xmpRights", "Rights", "b-value"),
        ];
        let forward = instances(&colliding);
        let mut reversed_rows = colliding;
        reversed_rows.reverse();
        let reversed = instances(&reversed_rows);

        // Insertion order must not change the verdict. `XMP-dc:Rights` sorts
        // before `XMP-xmpRights:Rights`, so it wins both ways -- and its value
        // disagrees with ExifTool, so both runs must report a miss.
        assert_eq!(count_instance_coverage(&forward, &et), (0, 1));
        assert_eq!(count_instance_coverage(&reversed, &et), (0, 1));
    }

    /// BMP/ICO in ExifTool's `t/images`: everything ExifTool emits is a
    /// skipped pseudo-family, so nothing is comparable. That must read as
    /// "not measurable", never as a measured 0%.
    #[test]
    fn a_format_with_no_comparable_tags_is_not_measurable() {
        let empty: HashMap<String, Vec<TagInfo>> = HashMap::new();
        let ox = instances(&[("x.bmp", "PNG", "ImageWidth", "16")]);
        assert_eq!(count_instance_coverage(&ox, &empty), (0, 0));

        let result = ComparisonEngine::compare_with_instances(
            vec![TagInfo::new(
                "ImageWidth".to_string(),
                "PNG".to_string(),
                "16".to_string(),
            )],
            vec![],
            "BMP",
            1,
            None,
            &ox,
            &empty,
        );
        assert!(!result.is_measurable());
        assert_eq!(result.instance_coverage_percentage, 0.0);
        assert!(result.summary().contains("not measurable"));
    }

    /// The report must sum raw instance counts, not average per-format
    /// percentages -- otherwise a 1-tag format outweighs a 1,000-tag one.
    #[test]
    fn overall_instance_coverage_weights_by_size_not_by_format() {
        use crate::models::ComparisonReport;

        let mut big = FormatComparison::new("JPEG".to_string(), 100);
        big.matched_instances = 500;
        big.total_exiftool_instances = 1000;
        big.calculate_instance_coverage();

        let mut small = FormatComparison::new("GIF".to_string(), 1);
        small.matched_instances = 1;
        small.total_exiftool_instances = 1;
        small.calculate_instance_coverage();

        // Unmeasurable: contributes 0/0 and must be named, not silently sunk.
        let unmeasurable = FormatComparison::new("BMP".to_string(), 1);

        let mut report = ComparisonReport::new();
        report.add_format("JPEG".to_string(), big);
        report.add_format("GIF".to_string(), small);
        report.add_format("BMP".to_string(), unmeasurable);
        report.calculate_overall_coverage();

        // 501/1001, not the 75% a per-format average would give.
        assert!((report.overall_instance_coverage - 50.05).abs() < 0.01);
        assert_eq!(report.unmeasurable_formats, vec!["BMP".to_string()]);
    }
}
