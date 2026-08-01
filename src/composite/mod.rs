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
        // Binary, DateTime, Struct and Array are never composite inputs in the
        // definitions we implement; treating them as absent is correct here and
        // keeps a nonsensical coercion from reaching the arithmetic.
        _ => None,
    }
}

/// Look up a tag by bare name, ignoring any `Group:` prefix.
///
/// ExifTool resolves composite inputs by name across all groups, so
/// `EXIF:FocalLength` satisfies a dependency written as `FocalLength`. An exact
/// match wins over a suffix match so an explicitly-grouped tag is preferred.
fn lookup(map: &MetadataMap, name: &str) -> Option<String> {
    if let Some(v) = map.value_form(name) {
        return Some(v.to_string());
    }
    if let Some(v) = map.get(name).and_then(value_string) {
        return Some(v);
    }
    let suffix = format!(":{name}");
    map.iter()
        .find(|(k, _)| k.ends_with(&suffix))
        .and_then(|(k, v)| {
            map.value_form(k)
                .map(str::to_string)
                .or_else(|| value_string(v))
        })
}

/// Resolve a composite input, preferring an already-computed unrounded value.
///
/// `values` holds the `ValueConv` form of composites computed earlier in this
/// run. Consulting it first is what stops precision loss from compounding down
/// a chain: `HyperfocalDistance` needs `CircleOfConfusion` to full precision,
/// not the `0.019 mm` that gets printed.
fn resolve(map: &MetadataMap, values: &HashMap<&str, String>, name: &str) -> Option<String> {
    let bare = name.rsplit(':').next().unwrap_or(name);
    if let Some(v) = values.get(bare) {
        return Some(v.clone());
    }
    lookup(map, name)
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
    // ValueConv forms of composites computed so far, keyed by bare tag name.
    let mut values: HashMap<&str, String> = HashMap::new();
    // Composites this run produced. They may be recomputed on a later pass
    // once more of their optional inputs exist; tags that came from the file
    // are never touched.
    let mut ours: HashSet<&str> = HashSet::new();

    for _pass in 0..MAX_PASSES {
        let mut added_this_pass = 0;

        for comp in COMPOSITES {
            let key = format!("Composite:{}", comp.name);
            let already_ours = ours.contains(comp.name);
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
            let mut owned: Vec<Option<String>> =
                Vec::with_capacity(comp.require.len() + comp.desire.len());
            let mut satisfied = true;
            for dep in comp.require {
                match resolve(map, &values, dep) {
                    Some(v) => owned.push(Some(v)),
                    None => {
                        satisfied = false;
                        break;
                    }
                }
            }
            if !satisfied {
                continue;
            }
            for dep in comp.desire {
                owned.push(resolve(map, &values, dep));
            }
            // A composite with only optional inputs still needs at least one.
            if comp.require.is_empty() && owned.iter().all(Option::is_none) {
                continue;
            }

            let inputs: Vec<Option<&str>> = owned.iter().map(|o| o.as_deref()).collect();
            if let Some(c) = compute::compute(comp.name, &inputs, make.as_deref()) {
                // Count only genuine changes, so the fixpoint still terminates.
                let changed = map.get_string(&key) != Some(c.print.as_str());
                values.insert(comp.name, c.value);
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
    fn resolves_inputs_across_group_prefixes() {
        let mut m = map_of(&[("EXIF:FNumber", "2.8")]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:Aperture"), Some("2.8"));
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
