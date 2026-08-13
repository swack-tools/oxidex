//! ExifTool's Composite (derived) tag layer.
//!
//! Composite tags are not read from the file. `ImageSize` comes from
//! `ImageWidth`/`ImageHeight`; `Megapixels` comes from `ImageSize`; `DOF` comes
//! from `FocalLength`, `Aperture` and `CircleOfConfusion`, two of which are
//! themselves composites.
//!
//! This layer was the single largest source of missing tags in the comparison
//! corpus -- the ten most-missed tag names are all composites, and every input
//! they need was already being extracted correctly. It is pure derivation, so
//! one engine closes the gap across every format at once rather than per
//! format.
//!
//! [`tables`] is generated from ExifTool; [`compute`] is hand-written. A
//! composite whose computation is not implemented simply never fires.

pub mod compute;
pub mod tables;

pub use tables::{COMPOSITES, Composite};

use std::collections::{HashMap, HashSet};

use crate::core::{MetadataMap, TagValue};

/// Maximum resolution passes.
///
/// Composites form a shallow DAG (`DOF` -> `CircleOfConfusion` ->
/// `ScaleFactor35efl`), so this converges in two or three rounds. The cap is a
/// backstop against a cyclic definition rather than a real limit; the loop also
/// exits as soon as a pass adds nothing.
const MAX_PASSES: usize = 8;

/// Render a tag value as the string a composite conversion expects.
///
/// Composite inputs arrive as whatever variant the parser produced, and the
/// numeric ones matter: `ExposureTime` and `FNumber` are usually `Rational`.
/// A stringifier that only handled `String` would silently starve most
/// composites of their inputs and they would quietly never fire.
///
/// `Rational` is kept in `n/d` form rather than pre-divided because
/// [`compute`] parses that form, and because ExifTool's own shutter-speed
/// handling is sensitive to the distinction.
fn value_string(v: &TagValue) -> Option<String> {
    match v {
        TagValue::String(s) => Some(s.clone()),
        TagValue::Integer(i) => Some(i.to_string()),
        TagValue::Float(f) => Some(f.to_string()),
        TagValue::Rational {
            numerator,
            denominator,
        } => Some(format!("{numerator}/{denominator}")),
        // EXIF date/time tags are stored as strings today, but retain support
        // for a typed UTC value so the SubSec composites do not silently starve
        // if a parser upgrades its representation.
        TagValue::DateTime(dt) => Some(dt.format("%Y:%m:%d %H:%M:%S").to_string()),
        // Binary, Struct and Array are not inputs to any implemented Composite.
        _ => None,
    }
}

/// Rank the groups that can supply an unqualified Composite dependency.
///
/// `MetadataMap` is backed by a randomized `HashMap`, so its iteration order
/// must never decide which of several same-named tags wins. Structural/file
/// groups precede embedded metadata, followed by EXIF directories, maker notes,
/// and XMP.  The key itself is the final tie-breaker, making unknown groups
/// deterministic too.
fn lookup_rank(key: &str) -> (u8, &str) {
    let group = key.split_once(':').map_or("", |(group, _)| group);
    let rank = match group {
        "Composite" => 0,
        "File" => 1,
        // Primary container/image groups. These provide the displayed file
        // dimensions when a format parser does not also publish a bare key.
        "JPEG" | "PNG" | "GIF" | "BMP" | "WebP" | "JXL" | "HEIF" | "QuickTime" | "RIFF" | "AVI"
        | "Matroska" | "ASF" | "Flash" | "H264" | "Photoshop" | "PDF" | "SVG" | "EXR" | "BPG"
        | "FLIF" | "FITS" => 2,
        // Standard TIFF/EXIF directories.
        "EXIF" | "IFD0" | "ExifIFD" | "InteropIFD" | "GPS" => 3,
        group if group.starts_with("SubIFD") => 3,
        "MakerNotes" => 4,
        group if group.starts_with("XMP") => 5,
        _ => 6,
    };
    (rank, key)
}

/// Look up one fully-qualified key, applying APEX ValueConv but not PrintConv.
///
/// ExifTool's Composite table (Exif.pm:4678) reads `$val[N]` post-ValueConv:
/// for ShutterSpeedValue/ApertureValue/MaxApertureValue that means seconds and
/// f-stops, not the raw APEX-encoded rational still sitting in the map at
/// this point in the pipeline -- [`crate::core::exiftool_compat::format_tag_value`]
/// only runs at CLI output time, after composites have already been derived.
/// Reusing [`crate::core::exiftool_compat::apex_value_conv`] here keeps the
/// conversion in one place rather than re-deriving `2**(-$val)` a second time.
/// The helper returns a raw numeric value, so downstream composites retain the
/// precision that an emitted `1/152` or `4.7` PrintConv string would discard.
fn lookup_key(map: &MetadataMap, key: &str) -> Option<String> {
    if let Some(v) = map.value_form(key) {
        return Some(v.to_string());
    }
    let raw = map.get(key)?;
    let base_name = crate::core::exiftool_compat::strip_family_prefix(key);
    // TIFF's GPSAltitudeRef is a one-byte field. It remains Binary until the
    // CLI output formatter runs, but GPS.pm's Composite is evaluated before
    // that formatter. Supply the same PrintConv form here so the Composite
    // sees the reference ExifTool gives it (GPS.pm:406-431).
    if base_name == "GPSAltitudeRef" {
        if let TagValue::Binary(bytes) = raw {
            if let Some(rendered) =
                crate::core::formatters::gps_altitude_ref::format_gps_altitude_ref_bytes(bytes)
            {
                return Some(rendered);
            }
        }
    }
    if let Some(converted) = crate::core::exiftool_compat::apex_value_conv(base_name, raw) {
        return value_string(&converted);
    }
    value_string(raw)
}

/// Look up a tag by bare name, ignoring any `Group:` prefix.
///
/// ExifTool resolves composite inputs by name across all groups, so
/// `EXIF:FocalLength` satisfies a dependency written as `FocalLength`. An exact
/// match wins over a suffix match so an explicitly-grouped tag is preferred;
/// otherwise [`lookup_rank`] supplies a stable group preference.
fn lookup(map: &MetadataMap, name: &str) -> Option<String> {
    lookup_ranked(map, name, true)
}

/// [`lookup`] restricted to tags that were read from the file.
///
/// Used for the one case where ExifTool's priority rule says a derived value
/// must not shadow an extracted one; see [`resolve`].
fn lookup_extracted(map: &MetadataMap, name: &str) -> Option<String> {
    lookup_ranked(map, name, false)
}

fn lookup_ranked(map: &MetadataMap, name: &str, composites: bool) -> Option<String> {
    if let Some(v) = lookup_key(map, name) {
        return Some(v);
    }
    // ExifTool's `EXIF:` dependency prefix is a family-0 group. OxiDex emits
    // the family-1 IFD name (`ExifIFD:` or `IFD0:`), so bridge that one
    // generated namespace deliberately. Other explicit groups remain exact:
    // `GPS:GPSLatitude` must not silently bind an unrelated suffix match.
    if let Some(bare) = name.strip_prefix("EXIF:") {
        for family in ["ExifIFD", "IFD0", "EXIF"] {
            let key = format!("{family}:{bare}");
            if let Some(v) = lookup_key(map, &key) {
                return Some(v);
            }
        }
        return None;
    }
    if name.contains(':') {
        return None;
    }

    // Unqualified dependencies resolve through [`lookup_rank`] alone. An
    // earlier hardcoded family list did the same job, but the two orders
    // disagreed -- the list tried EXIF before File, while `lookup_rank` ranks
    // File first so that a container's own dimensions win for ImageWidth. Two
    // precedence tables that contradict each other cannot both be right, and
    // the ranking is the one with a test pinning it, so it is the only one
    // kept. It still resolves standard EXIF ahead of same-named MakerNote
    // values, and still makes the choice independent of the randomized
    // HashMap iteration order.
    let suffix = format!(":{name}");
    map.iter()
        .filter(|(k, _)| k.ends_with(&suffix))
        .filter(|(k, _)| composites || !k.starts_with("Composite:"))
        .min_by_key(|(k, _)| lookup_rank(k))
        .and_then(|(k, _)| lookup_key(map, k))
}

/// A composite value produced by this run, with the priority of the definition
/// that produced it.
struct Derived {
    /// The `ValueConv` form -- full precision, what dependents consume.
    value: String,
    /// See [`Composite::priority`].
    priority: i8,
}

/// Resolve a composite input, preferring an already-computed unrounded value.
///
/// `values` holds the `ValueConv` form of composites computed earlier in this
/// run. Consulting it first is what stops precision loss from compounding down
/// a chain: `HyperfocalDistance` needs `CircleOfConfusion` to full precision,
/// not the `0.019 mm` that gets printed.
///
/// An unqualified dependency is not a search across groups in ExifTool: it
/// reads exactly one entry, the *bare* tag key. All line numbers below are from
/// the release named by `.exiftool-version`, which these tables are generated
/// from and verified against.
///
/// ```text
/// ExifTool.pm:4008 (BuildCompositeTags)
///     if (defined $$rawValue{$reqTag}) {
/// ```
///
/// Which tag holds that key is decided by `FoundTag`'s priority arbitration,
/// and a Composite competes for it like any other tag:
///
/// ```text
/// ExifTool.pm:9442
///     if ($priority >= $oldPriority and ...)
///         # move existing tag out of the way since this tag is higher priority
/// ```
///
/// `$oldPriority` defaults to 1 for an ordinarily-extracted main-document tag
/// (ExifTool.pm:9422-9429), and a Composite defaults to 1 as well -- the
/// `Composite` table declares no `PRIORITY` (ExifTool.pm:2256-2262), so an
/// undeclared one falls through to "the normal default" at ExifTool.pm:9440.
/// So a Composite normally *does* take the name: `Composite:LensID` and
/// `Composite:GPSLatitude` really are what a bare `LensID` or `GPSLatitude`
/// dependency binds, which the corpus confirms. The exception is a definition
/// that demotes itself, and `Canon:ISO` is the one that matters here:
///
/// ```text
/// Canon.pm:9781-9782
///     ISO => {
///         Priority => 0,  # let EXIF:ISO take priority
/// ```
///
/// `0 >= 1` is false, so `EXIF:ISO` keeps the bare key and `Composite:LightValue`
/// -- whose `Require` is the unqualified `ISO` (Exif.pm:4687-4691) -- is computed
/// from the camera's recorded ISO, not from Canon's `BaseISO * AutoISO / 100`.
fn resolve(map: &MetadataMap, values: &HashMap<&str, Derived>, name: &str) -> Option<String> {
    // An explicit group is a namespace constraint, not merely decoration.
    // In particular, GPS::Composite requires `GPS:GPSLongitude`: after the
    // first pass has produced Composite:GPSLongitude, rebinding that generated
    // value here would feed the signed composite back into itself and flip a
    // western longitude east on the next fixpoint pass.  The one explicit
    // generated namespace is `Composite:` itself.
    if let Some(bare) = name.strip_prefix("Composite:") {
        return values
            .get(bare)
            .map(|d| d.value.clone())
            .or_else(|| lookup(map, name));
    }
    // Generated QuickTime Composite dependencies retain ExifTool's
    // `Module::Tag` table notation, while parsed values use the emitted
    // `Group:Tag` key. Resolve that notation at the composite boundary.
    if let Some((group, tag)) = name.split_once("::") {
        return lookup(map, &format!("{group}:{tag}"));
    }
    if name.contains(':') {
        return lookup(map, name);
    }
    match values.get(name) {
        // Priority >= 1: the composite would have claimed the bare key.
        Some(d) if d.priority >= 1 => Some(d.value.clone()),
        // Demoted: an extracted tag of this name keeps the bare key. With no
        // such tag there is nothing to lose to, and the composite holds it.
        Some(d) => lookup_extracted(map, name).or_else(|| Some(d.value.clone())),
        None => lookup(map, name),
    }
}

/// Compute every Composite tag whose inputs are available, and insert them.
///
/// Returns the number of tags added. Existing tags are never overwritten: a
/// value the parser actually read from the file always beats a derived one.
pub fn apply(map: &mut MetadataMap) -> usize {
    let mut added = 0;
    // ExifTool branches on manufacturer for Canon sensor geometry, so resolve
    // it once up front rather than per composite.
    let make = lookup(map, "Make");
    let file_type = lookup(map, "FileType");
    // ValueConv forms of composites computed so far, keyed by bare tag name.
    let mut values: HashMap<&str, Derived> = HashMap::new();
    // Composites this run produced. They may be recomputed on a later pass
    // once more of their optional inputs exist; tags that came from the file
    // are never touched.
    let mut ours: HashSet<&str> = HashSet::new();

    for _pass in 0..MAX_PASSES {
        let mut added_this_pass = 0;

        for comp in COMPOSITES {
            let key = format!("Composite:{}", comp.name);
            let already_ours = ours.contains(comp.name);
            // Exif.pm guards this join with
            // `not defined $$self{VALUE}{DateTimeOriginal}`. An extracted
            // DateTimeOriginal in any source group wins over the synthesized
            // date/time join even though its fully-qualified key differs from
            // the Composite output key.
            if comp.module == "Exif"
                && comp.name == "DateTimeOriginal"
                && lookup(map, "DateTimeOriginal").is_some()
            {
                continue;
            }
            // A composite computed on an earlier pass is revisited, because a
            // `Desire` input may only have appeared since -- FocalLength35efl
            // needs ScaleFactor35efl, which is itself derived. Without this it
            // would be frozen at "34.0 mm" instead of gaining its 35 mm
            // equivalent. Values read from the file are still never replaced.
            if !already_ours && (map.contains_key(&key) || map.contains_key(comp.name)) {
                continue;
            }

            // Required inputs must all be present; desired ones may be absent.
            // Both are passed positionally so indices line up with ExifTool's
            // $val[N].
            let input_len = comp
                .require
                .iter()
                .chain(comp.desire.iter())
                .map(|(index, _)| index + 1)
                .max()
                .unwrap_or(0);
            let mut owned: Vec<Option<String>> = vec![None; input_len];
            let mut satisfied = true;
            for &(index, dep) in comp.require {
                match resolve(map, &values, dep) {
                    Some(v) => owned[index] = Some(v),
                    None => {
                        satisfied = false;
                        break;
                    }
                }
            }
            if !satisfied {
                continue;
            }
            for &(index, dep) in comp.desire {
                owned[index] = resolve(map, &values, dep);
            }
            // Exif.pm ImageSize ValueConv (Exif.pm:4384-4390) prefers
            // ExifImageWidth/Height over the required IFD0 ImageWidth/Height
            // pair, but only for these four TIFF-based RAW types:
            // `$$self{TIFF_TYPE} =~ /^(CR2|Canon 1D RAW|IIQ|EIP)$/`. CanonRaw.cr2
            // carries both pairs as 3456x2304 and 384x256; PhaseOne.iiq's IFD0
            // pair is a 1x1 placeholder next to a real 7320x5484 ExifIFD pair.
            // "Canon 1D RAW" is a Model string, not a FileType, and is not
            // reachable from `file_type` here.
            if comp.module == "Exif"
                && comp.name == "ImageSize"
                && matches!(file_type.as_deref(), Some("CR2" | "IIQ" | "EIP"))
                && owned.get(2).and_then(Option::as_ref).is_some()
                && owned.get(3).and_then(Option::as_ref).is_some()
            {
                owned[0] = owned[2].clone();
                owned[1] = owned[3].clone();
            }
            // A composite with only optional inputs still needs at least one.
            if comp.require.is_empty() && owned.iter().all(Option::is_none) {
                continue;
            }

            let inputs: Vec<Option<&str>> = owned.iter().map(|o| o.as_deref()).collect();
            if let Some(c) = compute::compute(comp.module, comp.name, &inputs, make.as_deref()) {
                // Count only genuine changes, so the fixpoint still terminates.
                let changed = map.get_string(&key) != Some(c.print.as_str());
                values.insert(
                    comp.name,
                    Derived {
                        value: c.value,
                        priority: comp.priority,
                    },
                );
                map.insert(key, TagValue::new_string(c.print));
                if !already_ours {
                    added += 1;
                }
                ours.insert(comp.name);
                if changed {
                    added_this_pass += 1;
                }
            }
        }

        if added_this_pass == 0 {
            break;
        }
    }

    added
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_of(pairs: &[(&str, &str)]) -> MetadataMap {
        let mut m = MetadataMap::new();
        for (k, v) in pairs {
            m.insert(*k, TagValue::new_string((*v).to_string()));
        }
        m
    }

    #[test]
    fn definitions_are_generated() {
        assert!(COMPOSITES.len() > 90, "got {}", COMPOSITES.len());
        assert!(COMPOSITES.iter().any(|c| c.name == "Megapixels"));
    }

    #[test]
    fn derives_image_size_and_megapixels() {
        let mut m = map_of(&[("File:ImageWidth", "4000"), ("File:ImageHeight", "3000")]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:ImageSize"), Some("4000x3000"));
        // Megapixels depends on ImageSize, which is itself derived -- this only
        // works because resolution runs to a fixpoint.
        assert_eq!(m.get_string("Composite:Megapixels"), Some("12.0"));
    }

    #[test]
    fn gps_altitude_uses_gps_values_when_reference_is_present() {
        let mut m = map_of(&[
            ("GPS:GPSAltitude", "27.99831776 m"),
            ("GPS:GPSAltitudeRef", "Above Sea Level"),
        ]);
        apply(&mut m);
        assert_eq!(
            m.get_string("Composite:GPSAltitude"),
            Some("27.9 m Above Sea Level")
        );
    }

    #[test]
    fn cr2_image_size_prefers_exif_dimensions() {
        let mut m = map_of(&[
            ("File:FileType", "CR2"),
            ("IFD0:ImageWidth", "384"),
            ("IFD0:ImageHeight", "256"),
            ("ExifIFD:ExifImageWidth", "3456"),
            ("ExifIFD:ExifImageHeight", "2304"),
        ]);

        apply(&mut m);

        assert_eq!(m.get_string("Composite:ImageSize"), Some("3456x2304"));
    }

    #[test]
    fn iiq_and_eip_image_size_also_prefer_exif_dimensions() {
        // Exif.pm:4384-4390's ValueConv checks
        // `$$self{TIFF_TYPE} =~ /^(CR2|Canon 1D RAW|IIQ|EIP)$/`, not just CR2.
        // PhaseOne.iiq's IFD0 pair is a 1x1 placeholder next to the real
        // 7320x5484 ExifIFD pair -- exactly the shape CR2's placeholder-IFD0
        // case already covered, just under a different FileType.
        for file_type in ["IIQ", "EIP"] {
            let mut m = map_of(&[
                ("File:FileType", file_type),
                ("IFD0:ImageWidth", "1"),
                ("IFD0:ImageHeight", "1"),
                ("ExifIFD:ExifImageWidth", "7320"),
                ("ExifIFD:ExifImageHeight", "5484"),
            ]);
            apply(&mut m);
            assert_eq!(
                m.get_string("Composite:ImageSize"),
                Some("7320x5484"),
                "{file_type} should prefer the ExifIFD pair"
            );
        }
    }

    #[test]
    fn resolves_inputs_across_group_prefixes() {
        let mut m = map_of(&[("EXIF:FNumber", "2.8")]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:Aperture"), Some("2.8"));
    }

    #[test]
    fn unqualified_lookup_has_deterministic_group_precedence() {
        // Each MetadataMap gets an independently-randomized HashMap seed. A
        // plain `.find()` therefore selected every one of these values across
        // repeated runs, making ImageSize change nondeterministically.
        for _ in 0..1_000 {
            let mut m = map_of(&[
                ("MakerNotes:ImageWidth", "1624"),
                ("EXIF:ImageWidth", "6000"),
                ("File:ImageWidth", "4000"),
            ]);
            assert_eq!(lookup(&m, "ImageWidth").as_deref(), Some("4000"));

            // An actual unqualified key remains authoritative.
            m.insert("ImageWidth", TagValue::new_string("8000"));
            assert_eq!(lookup(&m, "ImageWidth").as_deref(), Some("8000"));
        }

        let mut m = map_of(&[
            ("MakerNotes:ImageWidth", "1624"),
            ("MakerNotes:ImageHeight", "1080"),
            ("File:ImageWidth", "4000"),
            ("File:ImageHeight", "3000"),
        ]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:ImageSize"), Some("4000x3000"));
    }

    #[test]
    fn unknown_groups_use_the_key_as_a_stable_tiebreaker() {
        for _ in 0..1_000 {
            let m = map_of(&[("Zulu:Thing", "last"), ("Alpha:Thing", "first")]);
            assert_eq!(lookup(&m, "Thing").as_deref(), Some("first"));
        }
    }

    #[test]
    fn resolves_exif_family_dependencies_to_their_ifd_groups() {
        let mut m = map_of(&[
            ("ExifIFD:DateTimeOriginal", "2005:01:14 08:57:59"),
            ("ExifIFD:SubSecTimeOriginal", "20"),
        ]);
        apply(&mut m);
        assert_eq!(
            m.get_string("Composite:SubSecDateTimeOriginal"),
            Some("2005:01:14 08:57:59.20")
        );
    }

    #[test]
    fn explicit_gps_dependencies_do_not_rebind_to_generated_composites() {
        let mut m = map_of(&[
            ("GPS:GPSLatitude", "54 deg 59' 22.80\""),
            ("GPS:GPSLatitudeRef", "North"),
            ("GPS:GPSLongitude", "1 deg 54' 51.00\""),
            ("GPS:GPSLongitudeRef", "West"),
        ]);
        apply(&mut m);
        assert_eq!(
            m.get_string("Composite:GPSLongitude"),
            Some("1 deg 54' 51.00\" W")
        );
        assert_eq!(
            m.get_string("Composite:GPSPosition"),
            Some("54 deg 59' 22.80\" N, 1 deg 54' 51.00\" W")
        );
    }

    #[test]
    fn extracted_date_time_original_suppresses_the_synthesized_join() {
        let mut m = map_of(&[
            ("ExifIFD:DateTimeOriginal", "2001:01:01 01:11:11"),
            ("IPTC:DateCreated", "1992:01:01"),
            ("IPTC:TimeCreated", "02:11:11+01:00"),
        ]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:DateTimeOriginal"), None);
    }

    #[test]
    fn preserves_generated_dependency_positions() {
        let canon = COMPOSITES
            .iter()
            .find(|c| c.module == "Canon" && c.name == "WB_RGGBLevels")
            .expect("generated Canon white-balance composite");
        assert_eq!(canon.require, &[(0, "Canon:WhiteBalance")]);
        assert!(canon.desire.contains(&(10, "WB_RGGBLevelsShade")));
        assert!(canon.desire.contains(&(11, "WB_RGGBLevelsKelvin")));
        assert!(!canon.desire.iter().any(|(index, _)| *index == 9));
    }

    #[test]
    fn bare_dependencies_prefer_standard_exif_without_mixing_groups() {
        let mut m = map_of(&[
            ("Panasonic:WBRedLevel", "2283"),
            ("Panasonic:WBGreenLevel", "1054"),
            ("IFD0:WBRedLevel", "570"),
            ("IFD0:WBGreenLevel", "263"),
        ]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:RedBalance"), Some("2.1673"));
    }

    #[test]
    fn chains_three_levels_deep() {
        // ScaleFactor35efl -> CircleOfConfusion -> HyperfocalDistance
        let mut m = map_of(&[
            ("EXIF:FocalLength", "50.0 mm"),
            ("EXIF:FNumber", "2.8"),
            ("Composite:ScaleFactor35efl", "1.0"),
        ]);
        apply(&mut m);
        assert_eq!(
            m.get_string("Composite:CircleOfConfusion"),
            Some("0.030 mm")
        );
        // 29.72, not 29.76: HyperfocalDistance divides by the *unrounded*
        // CircleOfConfusion (0.0300463), matching ExifTool. Getting 29.76 here
        // would mean the printed "0.030 mm" had been fed back into the chain.
        assert_eq!(
            m.get_string("Composite:HyperfocalDistance"),
            Some("29.72 m")
        );
    }

    #[test]
    fn derives_depth_of_field_through_the_generated_graph() {
        let mut m = map_of(&[
            ("EXIF:FocalLength", "34"),
            ("EXIF:FNumber", "14"),
            ("Composite:CircleOfConfusion", "0.018913043114871"),
            ("MakerNotes:FocusDistanceLower", "5.46"),
            ("MakerNotes:FocusDistanceUpper", "655.35"),
        ]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:Aperture"), Some("14.0"));
        assert_eq!(m.get_string("Composite:DOF"), Some("inf (4.31 m - inf)"));
    }

    #[test]
    fn shutter_and_aperture_composites_read_apex_values_value_conv_not_raw() {
        // SamsungDigimax340.jpg: ShutterSpeedValue = 58/8 (APEX 7.25),
        // ApertureValue = 44658/10000 (APEX 4.4658). Composite inputs must see
        // ExifTool's ValueConv (seconds / f-stop), not the raw APEX rational,
        // or ShutterSpeed prints "7.2" and Aperture prints "4.5" instead of
        // matching ExifTool's "1/152" and "4.7".
        let mut m = MetadataMap::new();
        m.insert(
            "ExifIFD:ShutterSpeedValue",
            TagValue::Rational {
                numerator: 58,
                denominator: 8,
            },
        );
        m.insert(
            "ExifIFD:ApertureValue",
            TagValue::Rational {
                numerator: 44658,
                denominator: 10000,
            },
        );
        m.insert("ExifIFD:ISO", TagValue::Integer(100));
        let shutter_value_conv =
            lookup_key(&m, "ExifIFD:ShutterSpeedValue").expect("APEX shutter ValueConv");
        let shutter_seconds: f64 = shutter_value_conv.parse().expect("numeric ValueConv");
        assert!((shutter_seconds - 2f64.powf(-7.25)).abs() < f64::EPSILON);
        assert_ne!(shutter_value_conv, "1/152");
        apply(&mut m);
        assert_eq!(m.get_string("Composite:ShutterSpeed"), Some("1/152"));
        assert_eq!(m.get_string("Composite:Aperture"), Some("4.7"));
        assert_eq!(m.get_string("Composite:LightValue"), Some("11.7"));
    }

    #[test]
    fn depth_of_field_uses_value_conv_precision_not_printed_distance() {
        let mut m = map_of(&[
            ("EXIF:FocalLength", "50.0 mm"),
            ("EXIF:FNumber", "4.0"),
            ("Composite:ScaleFactor35efl", "1.5"),
            ("Nikon:FocusDistance", "0.71 m"),
        ]);
        m.set_value_form("Nikon:FocusDistance", "0.707945784384138");

        apply(&mut m);

        // ExifTool keeps the unrounded Nikon ValueConv form private while the
        // visible tag remains its two-decimal PrintConv form.
        assert_eq!(m.get_string("Nikon:FocusDistance"), Some("0.71 m"));
        assert_eq!(
            m.get_string("Composite:DOF"),
            Some("0.03 m (0.69 - 0.72 m)")
        );
    }

    #[test]
    fn upgrades_a_composite_once_a_derived_input_appears() {
        // FocalLength35efl can be computed from FocalLength alone, but gains
        // its 35 mm equivalent once ScaleFactor35efl is derived. Whichever
        // order the two are visited in, the final answer must be the full one.
        let mut m = map_of(&[
            ("EXIF:FocalLength", "34.0 mm"),
            ("EXIF:FocalPlaneResolutionUnit", "2"),
            ("EXIF:FocalPlaneXResolution", "3072000/892"),
            ("EXIF:FocalPlaneYResolution", "2048000/595"),
            ("IFD0:Make", "Canon"),
        ]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:ScaleFactor35efl"), Some("1.6"));
        assert_eq!(
            m.get_string("Composite:FocalLength35efl"),
            Some("34.0 mm (35 mm equivalent: 54.0 mm)")
        );
    }

    #[test]
    fn a_demoted_composite_does_not_shadow_the_extracted_tag_it_defers_to() {
        // Canon's Composite:ISO carries `Priority => 0, # let EXIF:ISO take
        // priority` (Canon.pm:9781), so EXIF:ISO keeps the bare `ISO` key and
        // LightValue's unqualified `2 => 'ISO'` binds that, not the
        // BaseISO * AutoISO / 100 estimate.
        //
        // These are the real tags of Canon/CanonDIGITAL_IXUS120IS.jpg, on which
        // `exiftool -a -G1 -s` reports Composite:ISO 75 and LightValue 10.9 --
        // 10.9 being the value computed from the extracted 80. Binding the
        // composite's own 75 instead gives 11.0, which is what oxidex printed.
        let mut m = map_of(&[
            ("ExifIFD:ISO", "80"),
            ("ExifIFD:FNumber", "2.8"),
            ("ExifIFD:ExposureTime", "1/200"),
            ("Canon:CameraISO", "Auto"),
            ("Canon:BaseISO", "100"),
            ("Canon:AutoISO", "75"),
        ]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:ISO"), Some("75"));
        assert_eq!(m.get_string("Composite:LightValue"), Some("10.9"));

        // With no extracted ISO there is nothing for the demoted composite to
        // lose the bare key to, so it supplies the dependency itself and the
        // same file's numbers give 11.0.
        let mut m = map_of(&[
            ("ExifIFD:FNumber", "2.8"),
            ("ExifIFD:ExposureTime", "1/200"),
            ("Canon:CameraISO", "Auto"),
            ("Canon:BaseISO", "100"),
            ("Canon:AutoISO", "75"),
        ]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:LightValue"), Some("11.0"));
    }

    #[test]
    fn a_default_priority_composite_still_wins_the_bare_name() {
        // Only a Composite that demotes itself yields. Every other one takes
        // the bare tag key from a same-named extracted tag
        // (ExifTool.pm:9542, `$priority >= $oldPriority`), which is why
        // ExifTool reports Composite:GPSLatitude/GPSAltitude/LensID as the
        // meaning of those names on the corpus.
        assert!(
            COMPOSITES.iter().all(|c| c.priority >= 1
                || (c.module == "Canon" && c.name == "ISO")
                || (c.module == "Exif" && c.name == "GPSPosition")
                || (c.module == "ID3" && c.name == "DateTimeOriginal")
                || (c.module == "MPEG" && c.name == "Duration")
                || (c.module == "QuickTime"
                    && matches!(c.name, "AvgBitrate" | "GPSAltitude" | "GPSAltitudeRef"))),
            "an unreviewed Composite demoted itself; check its ExifTool Priority"
        );

        // GPS:GPSLatitude is `Priority => 1, Avoid => 1` (GPS.pm): the explicit
        // Priority wins over Avoid, so it does claim the name.
        let gps = COMPOSITES
            .iter()
            .find(|c| c.module == "GPS" && c.name == "GPSLatitude")
            .expect("generated GPS latitude composite");
        assert_eq!(gps.priority, 1);
    }

    #[test]
    fn auto_focus_needs_a_nikon_focus_mode_specifically() {
        // Nikon.pm's Composite::AutoFocus writes its dependency group-qualified
        // (`Require => { 0 => 'Nikon:FocusMode' }`), and that qualification is
        // the whole reason ExifTool stays silent on the 3900-odd corpus files
        // that are not Nikons. Twelve other makers publish a `FocusMode` of
        // their own -- Canon on 610 corpus files, FujiFilm on 366, Panasonic
        // on 312, Sony on 253 -- and `Composite:AutoFocus` appears on exactly
        // the 298 that carry `Nikon:FocusMode`, on none of the rest.
        let mut m = map_of(&[("Nikon:FocusMode", "Manual")]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:AutoFocus"), Some("Off"));

        let mut m = map_of(&[("Nikon:FocusMode", "AF-S")]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:AutoFocus"), Some("On"));

        // `exiftool -a -G1 -s -FocusMode -AutoFocus` on the pinned 13.59:
        //
        //   ======== Canon.jpg
        //   [Canon]     FocusMode  : Manual Focus (3)
        //   ======== Olympus/OlympusAIR-A01.jpg
        //   [Olympus]   FocusMode  : Single AF; S-AF, Imager AF
        //   ======== FujiFilm.jpg
        //   [FujiFilm]  FocusMode  : Auto
        //
        // No AutoFocus line on any of the three. Canon.jpg is the sharp case:
        // its FocusMode starts with "Manual", so a dependency that fell back
        // to a bare-name search would not merely over-emit, it would over-emit
        // the minority value.
        for focus_mode in [
            ("Canon:FocusMode", "Manual Focus (3)"),
            ("Olympus:FocusMode", "Single AF; S-AF, Imager AF"),
            ("FujiFilm:FocusMode", "Auto"),
        ] {
            let mut m = map_of(&[focus_mode]);
            apply(&mut m);
            assert_eq!(
                m.get_string("Composite:AutoFocus"),
                None,
                "{} must not derive AutoFocus",
                focus_mode.0
            );
        }
    }

    #[test]
    fn never_overwrites_a_parsed_value() {
        // A value read from the file must win over a derived one.
        let mut m = map_of(&[
            ("File:ImageWidth", "4000"),
            ("File:ImageHeight", "3000"),
            ("Composite:ImageSize", "from-file"),
        ]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:ImageSize"), Some("from-file"));
    }

    #[test]
    fn adds_nothing_without_inputs() {
        let mut m = map_of(&[("File:FileName", "x.jpg")]);
        assert_eq!(apply(&mut m), 0);
    }

    #[test]
    fn terminates_on_an_empty_map() {
        let mut m = MetadataMap::new();
        assert_eq!(apply(&mut m), 0);
    }
}
