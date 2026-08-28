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
mod generated_compute;
mod lens_alternatives;
mod lens_id;
pub mod tables;

pub use tables::{COMPOSITES, Composite};

use std::collections::HashSet;

use crate::core::{Instance, MetadataMap, TagOccurrence, TagValue};

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
        // A one-byte `Binary` is how oxidex's EXIF parser stores an `int8u`
        // tag whose PrintConv the CLI applies later -- `GPS:GPSAltitudeRef`
        // is `Binary([0])`, not `Integer(0)`. ExifTool's ValueConv for such a
        // tag is simply that byte, so rendering it as the integer is the
        // ValueConv, not an interpretation of it; without this,
        // `Composite:GPSAltitude` starves on a `GPSAltitudeRef` that is
        // demonstrably present (`map.contains_key` is true and the CLI prints
        // "Above Sea Level" for it) and silently never fires.
        //
        // Longer `Binary` runs are still refused: a multi-byte blob has no
        // single ValueConv number, and guessing an encoding for one is the
        // approximation this layer exists to avoid.
        TagValue::Binary(bytes) if bytes.len() == 1 => Some(bytes[0].to_string()),
        // Longer Binary, Struct and Array are not inputs to any implemented
        // Composite.
        _ => None,
    }
}

/// The value to feed downstream composites for `occurrence`: its `value`
/// (ValueConv) form when one is attached -- Step 22 folded the old
/// `value_forms` sidecar into [`TagOccurrence::value`], so this is where a
/// full-precision Nikon `FocusDistance` or an XMP `n/d` focal-plane
/// resolution (or another composite's own unrounded result -- see
/// [`apply`]'s own insertion, below) is read back -- or, failing that,
/// [`crate::core::exiftool_compat::apex_value_conv`]'s ValueConv for an
/// APEX-encoded rational, or the occurrence's raw stored form otherwise.
///
/// This mirrors ExifTool's Composite table reading `$val[N]` *post-ValueConv*
/// (Exif.pm:4678): [`crate::core::exiftool_compat::format_tag_value`] (the
/// PrintConv step) only runs at CLI output time, after composites have
/// already been derived, so a bare stored value is already the ValueConv
/// form for every tag that has no separate PrintConv step -- which is every
/// case not covered by `value`/`apex_value_conv` here.
fn occurrence_value_string(occurrence: &TagOccurrence) -> Option<String> {
    if let Some(v) = &occurrence.value {
        return value_string(v);
    }
    if let Some(converted) =
        crate::core::exiftool_compat::apex_value_conv(&occurrence.name, &occurrence.raw)
    {
        return value_string(&converted);
    }
    value_string(&occurrence.raw)
}

/// Resolves one Composite dependency key (already normalized to `Group:Tag`
/// form, or bare) against every occurrence [`crate::core::tag_sink::TagSink`]
/// has ever recorded -- using
/// [`crate::cli::tag_resolution::resolve_requested_tags`]'s own
/// priority/order arbitration, the exact rule a CLI `-TAG` request resolves
/// through.
///
/// This is Step 22's replacement for the old `lookup_rank`/`lookup_ranked`
/// hard-coded group-rank table plus suffix scan
/// (`OVERHAUL_STEP18_DESIGN.md` Phase C): that rank table was a *guess* at
/// group precedence, disconnected from what `TagSink` actually recorded, and
/// a suffix scan over `map.iter()` (the winner-only projection) could not
/// see a Composite's own declared priority at all. The Casio fix this
/// session's evidence cites is the sharp case: a single wrong tag *name*
/// (`CCDSensitivity` where ExifTool says `ISO`) silently cost two tags,
/// because `Composite:LightValue` resolved its `Require: ISO` by scanning
/// for any `*:ISO` key rather than asking who actually wins the name.
/// Routing through the same function the CLI uses for `-TAG` means
/// composites and the CLI can no longer disagree about which occurrence a
/// bare or group-qualified dependency name means -- including a
/// `Priority => 0` demotion like Canon's:
///
/// ```text
/// Canon.pm:9781-9782
///     ISO => {
///         Priority => 0,  # let EXIF:ISO take priority
/// ```
///
/// [`apply`] now inserts every computed Composite via `insert_occurrence_with_raw`
/// carrying its own real `Composite::priority` (clamped at 0), so
/// `Composite:ISO`'s occurrence genuinely competes at priority 0 against
/// `ExifIFD:ISO`'s ordinary priority 1 through this same arbitration --
/// `0 >= 1` is false, so the extracted tag wins the bare `ISO` key and
/// `Composite:LightValue` (`Require => { 2 => 'ISO' }`, Exif.pm:4687-4691)
/// binds to it, not to `Canon:BaseISO * Canon:AutoISO / 100`. No separate
/// demoted-composite special case is needed for this anymore: it is the
/// same rule as every other name.
fn resolve_dependency(map: &MetadataMap, key: &str) -> Option<String> {
    let requested = [key.to_string()];
    let resolved = crate::cli::tag_resolution::resolve_requested_tags(map, &requested, false);
    let occurrence = resolved.into_iter().next()?.occurrence;
    occurrence_value_string(occurrence)
}

/// The family-0 group of whichever occurrence wins the bare tag name `key`,
/// under the same arbitration [`resolve_dependency`] uses for its value.
///
/// Exactly one Composite needs this: `Exif`'s primary `LensID`, whose ExifTool
/// `PrintConv` is handed `$self` and reads `$$self{TAG_INFO}{LensType}
/// {PrintConv}` (Exif.pm:5326) -- i.e. *which manufacturer's* LensType lookup
/// produced the string, which no positional `$val[N]` carries. See
/// [`lens_id`]'s module doc for why `Make` is not a substitute (a Samsung body
/// writing `Pentax:LensType`).
fn resolve_group0(map: &MetadataMap, key: &str) -> Option<String> {
    let requested = [key.to_string()];
    let resolved = crate::cli::tag_resolution::resolve_requested_tags(map, &requested, false);
    let occurrence = resolved.into_iter().next()?.occurrence;
    let group = occurrence.group0.as_ref().to_string();
    (!group.is_empty()).then_some(group)
}

/// Resolve a composite input.
///
/// An unqualified dependency is not a search across groups in ExifTool: it
/// reads exactly one entry, the *bare* tag key, whichever occurrence
/// currently wins it (`ExifTool.pm:4008`, `BuildCompositeTags`:
/// `if (defined $$rawValue{$reqTag})`). A group-qualified dependency
/// (`EXIF:Make`, `GPS:GPSLongitude`, `Composite:ScaleFactor35efl`) is a
/// namespace constraint, not decoration -- in particular, GPS's own
/// `Composite:GPSPosition` requires the *explicit* `GPS:GPSLongitude`, so
/// that a later pass's freshly-derived `Composite:GPSLongitude` (which the
/// same bare-name arbitration this function delegates to would otherwise
/// prefer, since a composite normally displaces a same-priority extracted
/// tag on its own later `order`) is never rebound into its own input --
/// which would flip a western longitude east. `[`resolve_dependency`] itself
/// enforces every qualifier exactly this way, so this function only needs
/// to normalize the two dependency-name notations ExifTool's generated
/// tables use (`Module::Tag` for QuickTime, `Group:Tag` for everything
/// parsed) onto one separator before delegating.
fn resolve(map: &MetadataMap, name: &str) -> Option<String> {
    let key = name.replacen("::", ":", 1);
    resolve_dependency(map, &key)
}

/// Iteration order for one [`apply`] pass: every non-`Inhibit` Composite
/// first, in `COMPOSITES` order, then every `Inhibit`-bearing one.
///
/// ExifTool's own `BuildCompositeTags` defers an `Inhibit`-bearing tag until
/// everything else has had its chance, specifically so it does not race a
/// same-named primary that has not been attempted yet in this same pass
/// (ExifTool.pm:4049-4052's `unless ($$inhibit{$index} and $allBuilt) { push
/// @deferredTags, ... }`, which holds a tag like `LensID-2` back until the
/// *last* internal iteration, "ignoring Composite Inhibit tags" only once
/// nothing else remains buildable -- ExifTool.pm:4157). Two ordering passes
/// (this one, plus the outer `MAX_PASSES` fixpoint loop) is the whole of
/// that mechanism this crate needs: only two ExifTool composites declare
/// `Inhibit` at all (`Exif::LensID-2`, `XMP::LensID`), both against the same
/// target (`Composite:LensID`), and that target is never itself
/// `Inhibit`-bearing, so one non-inhibit-first ordering per pass is already
/// enough to guarantee the primary is attempted before either fallback in
/// the very first pass it could fire in.
fn pass_order() -> Vec<usize> {
    let mut order: Vec<usize> = (0..COMPOSITES.len())
        .filter(|&i| COMPOSITES[i].inhibit.is_empty())
        .collect();
    order.extend((0..COMPOSITES.len()).filter(|&i| !COMPOSITES[i].inhibit.is_empty()));
    order
}

/// Compute every Composite tag whose inputs are available, and insert them.
///
/// Returns the number of tags added. Existing tags are never overwritten: a
/// value the parser actually read from the file always beats a derived one.
pub fn apply(map: &mut MetadataMap) -> usize {
    let mut added = 0;
    // ExifTool branches on manufacturer for Canon sensor geometry, so resolve
    // it once up front rather than per composite.
    let make = resolve(map, "Make");
    let file_type = resolve(map, "FileType");
    // Which manufacturer's `LensType` lookup won the bare name -- the one piece
    // of context `Composite:LensID` needs that a positional input cannot carry.
    // Resolved once here rather than per pass; no Composite in this table
    // produces a `LensType`, so it cannot change between passes.
    let lens_type_group = resolve_group0(map, "LensType");
    // `%Image::ExifTool::Olympus::Composite{LensType}` is
    // `Require => {0 => 'LensTypeMake', 1 => 'LensTypeModel'}`,
    // `ValueConv => '"$val[0] $val[1]"'`, `PrintConv => \%olympusLensTypes`.
    // Both present is therefore exactly the condition under which ExifTool
    // answers the bare `LensType` name from that table -- `'2 20 10' =>
    // 'Lumix G Vario 12-32mm F3.5-5.6 Asph. Mega OIS'` (Olympus.pm:175) --
    // rather than from Panasonic's own plain string. A zero *make* does not
    // exempt a body: `PanasonicDC-GH7.jpg` is `0 20 10`. The table is not
    // transcribed here, so `lens_id` refuses those bodies; see
    // `lens_id::OMITTED`.
    let olympus_lens_type_pair =
        resolve(map, "LensTypeMake").is_some() && resolve(map, "LensTypeModel").is_some();
    // Composites this run produced, keyed by each definition's own index
    // into COMPOSITES -- NOT by `comp.name`. Two distinct table rows can
    // share one output Name (`Exif::LensID` and `Exif::LensID-2` both
    // produce `Composite:LensID`; see `Composite::inhibit`'s doc comment),
    // and keying this set by name alone would let firing the first make
    // `already_ours` true for the *second* too, letting it re-fire later in
    // the very same pass and overwrite -- via the `map.remove` a genuine
    // same-definition refinement is allowed below -- a value that was never
    // its own to refine. Indexing by position keeps "I am revisiting my own
    // prior guess" and "someone else already claimed this name" distinct.
    let mut ours: HashSet<usize> = HashSet::new();
    let order = pass_order();

    for _pass in 0..MAX_PASSES {
        let mut added_this_pass = 0;

        for &idx in &order {
            let comp = &COMPOSITES[idx];
            let key = format!("Composite:{}", comp.name);
            let already_ours = ours.contains(&idx);
            // Exif.pm guards this join with
            // `not defined $$self{VALUE}{DateTimeOriginal}`. An extracted
            // DateTimeOriginal in any source group wins over the synthesized
            // date/time join even though its fully-qualified key differs from
            // the Composite output key.
            if comp.module == "Exif"
                && comp.name == "DateTimeOriginal"
                && resolve(map, "DateTimeOriginal").is_some()
            {
                continue;
            }
            // A composite computed on an earlier pass is revisited, because a
            // `Desire` input may only have appeared since -- FocalLength35efl
            // needs ScaleFactor35efl, which is itself derived. Without this it
            // would be frozen at "34.0 mm" instead of gaining its 35 mm
            // equivalent. Values read from the file are still never replaced.
            //
            // This ALSO blocks any other Composite definition that shares
            // this output name once one of them has claimed the key -- see
            // `ours`'s own doc comment just above.
            if !already_ours && (map.contains_key(&key) || map.contains_key(comp.name)) {
                continue;
            }

            // ExifTool.pm's `Inhibit` (ExifTool.pm:4080-4089's `if
            // ($$inhibit{$index}) { $found = 0; last; }`): the mirror image
            // of `Require` -- if ANY of these indexed dependencies resolves
            // to a value, this definition must not fire, full stop, even if
            // its own Require/Desire are otherwise satisfied. `LensID-2`
            // (Exif.pm:5362-5385) and XMP's own `LensID` (XMP.pm:2789-2801)
            // both inhibit on `Composite:LensID`, so neither ever computes
            // while the LensType-based primary already has -- see
            // `pass_order`'s doc comment for why the primary is guaranteed
            // to have been attempted first within this same pass.
            if comp
                .inhibit
                .iter()
                .any(|&(_, dep)| resolve(map, dep).is_some())
            {
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
                match resolve(map, dep) {
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
                owned[index] = resolve(map, dep);
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
            // `Composite:LensID` is the one definition whose ExifTool
            // conversion is not a function of its positional inputs alone --
            // it is handed `$self` and reads the winning LensType's own
            // PrintConv identity back out of it (Exif.pm:5326). Both Exif rows
            // that produce this Name are routed here rather than through
            // `compute::compute`, which has no such context; see [`lens_id`].
            let computed = if comp.module == "Exif" && comp.name == "LensID" {
                if comp.require.is_empty() {
                    // `LensID-2` (Exif.pm:5362-5385): the LensModel/Lens text
                    // fallback, whose ValueConv and PrintConv genuinely differ.
                    //
                    // `Inhibit => {4 => 'Composite:LensID'}` (Exif.pm:5371-5373)
                    // -- this row is suppressed outright whenever *any* module
                    // already produced a `Composite:LensID`. A maker `LensType`
                    // is exactly the condition under which one is: either the
                    // Exif primary above fires on it, or a manufacturer's own
                    // Composite does (`%Image::ExifTool::Nikon::Composite`
                    // {LensID}, Nikon.pm:13222-13238, built from LensIDNumber +
                    // 7 more raw tags against %nikonLensIDs; likewise Ricoh.pm
                    // and XMP.pm).
                    //
                    // oxidex implements only the Exif rows, so when the primary
                    // refuses -- a maker whose lookup tables are not transcribed
                    // (see `lens_id::OMITTED`) -- ExifTool would still have
                    // emitted a real lens name here while this fallback emits
                    // the camera's raw `LensModel`/`Lens` text. That is the
                    // plausible-but-wrong value AGENTS.md forbids: on
                    // Nikon.nef the oracle says `AF-S DX Zoom-Nikkor 18-70mm
                    // f/3.5-4.5G IF-ED` and the fallback says `18-70mm
                    // f/3.5-4.5`. Omit and count it instead.
                    if lens_type_group.is_some() {
                        None
                    } else {
                        lens_id::compute_fallback(&inputs)
                            .and_then(|(print, value)| compute::Computed::new(value, print))
                    }
                } else {
                    // The primary (Exif.pm:5303-5360): PrintConv only, so the
                    // value and print forms are the same string.
                    lens_id::compute_primary(
                        &inputs,
                        lens_type_group.as_deref(),
                        make.as_deref(),
                        olympus_lens_type_pair,
                    )
                    .and_then(compute::Computed::same)
                }
            } else {
                compute::compute(comp.module, comp.name, &inputs, make.as_deref())
            };
            if let Some(c) = computed {
                // Count only genuine changes, so the fixpoint still terminates.
                let changed = map.get_string(&key) != Some(c.print.as_str());
                // Carrying the composite's own declared `Composite::priority`
                // (ExifTool.pm:9442's `Priority`, clamped at 0 -- the lowest
                // this crate's u8 `TagOccurrence::priority` can represent,
                // which is also FoundTag's own floor: nothing beneath the
                // "yield to any ordinarily-extracted tag" case at priority 0
                // has a different effect) is what lets `resolve_dependency`
                // arbitrate a same-named extracted tag against this
                // Composite the same way FoundTag would -- see
                // `resolve_dependency`'s own doc comment for the Canon `ISO`
                // worked example this replaces the old `values`/`Derived`
                // special case with. Attaching `c.value` (the full-precision
                // ValueConv form) via `insert_occurrence_with_raw` is what
                // lets a later composite in this same pass (or a later pass)
                // read this one back at full precision instead of the
                // rounded `c.print` string -- the same job the old
                // in-memory `values` cache did, now folded into the
                // occurrence itself.
                let priority = comp.priority.max(0) as u8;
                // A composite revisited on a later pass (`already_ours`) is
                // refining its *own* prior guess, not contending with a
                // second source for the name -- `FocalLength35efl` upgrading
                // from "34.0 mm" to "34.0 mm (35 mm equivalent: 54.0 mm)" is
                // the same logical tag, not two tags racing for one key. But
                // `TagSink::record`'s priority-0 promotion rule (matching
                // `ExifTool.pm:9541-9551`) treats a same-priority arrival as
                // *losing* to an existing priority-0 winner, precisely to
                // make a genuine `Priority => 0` tie (JPEG COM's two comment
                // sources) resolve to the first arrival. Applied naively
                // here, that rule would freeze every demoted composite
                // (`Composite:GPSPosition`, `Canon:ISO`, ...) at whatever
                // its first, least-informed pass produced and block every
                // later pass's better answer from ever landing. Removing the
                // key first makes the re-insertion land in the sink's vacant
                // branch, which records unconditionally -- refining a
                // composite's own value is not the same operation as a
                // second source contending for its name, and this is what
                // keeps the two from being conflated.
                if already_ours {
                    map.remove(&key);
                }
                map.insert_occurrence_with_raw(
                    key,
                    TagValue::new_string(c.print),
                    TagValue::new_string(c.value),
                    priority,
                    "",
                    Instance::default(),
                );
                if !already_ours {
                    added += 1;
                }
                ours.insert(idx);
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
    fn unqualified_lookup_is_deterministic_and_favors_the_last_recorded_occurrence() {
        // `TagSink`'s own winner rule (`ExifTool.pm:9564`, `$priority >=
        // $oldPriority`) is what `resolve_dependency` now delegates to via
        // `cli::tag_resolution::resolve_requested_tags`: every occurrence
        // minted through the `insert()` shim ties at
        // `SHIM_DEFAULT_PRIORITY`, so the *last-recorded* one wins,
        // regardless of its group name -- unlike the old hard-coded
        // File-before-EXIF-before-MakerNotes rank table this replaced. This
        // is deterministic because `TagSink` stores occurrences in a `Vec`
        // (file order is intrinsic to it), not because of anything about
        // group names.
        for _ in 0..1_000 {
            let mut m = map_of(&[
                ("MakerNotes:ImageWidth", "1624"),
                ("EXIF:ImageWidth", "6000"),
                ("File:ImageWidth", "4000"),
            ]);
            assert_eq!(resolve(&m, "ImageWidth").as_deref(), Some("4000"));

            // An actual unqualified key remains authoritative: `insert()`
            // records it after the three above, so it wins on `order` too.
            m.insert("ImageWidth", TagValue::new_string("8000"));
            assert_eq!(resolve(&m, "ImageWidth").as_deref(), Some("8000"));
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
    fn unqualified_lookup_ignores_group_name_and_uses_recording_order_alone() {
        // Regression pin against the old rank table's `(rank, key)` tiebreak,
        // which fell back to *alphabetical key order* for two groups it did
        // not otherwise rank -- "Alpha:Thing" beat "Zulu:Thing" purely by
        // spelling. The new rule has no such fallback: whichever occurrence
        // was recorded last wins, so reversing the insertion order must
        // reverse the winner too.
        for _ in 0..1_000 {
            let forward = map_of(&[("Zulu:Thing", "first"), ("Alpha:Thing", "second")]);
            assert_eq!(resolve(&forward, "Thing").as_deref(), Some("second"));

            let reversed = map_of(&[("Alpha:Thing", "first"), ("Zulu:Thing", "second")]);
            assert_eq!(resolve(&reversed, "Thing").as_deref(), Some("second"));
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
            resolve(&m, "ExifIFD:ShutterSpeedValue").expect("APEX shutter ValueConv");
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
    fn exif_rational_beats_xmp_print_form_for_composite_inputs() {
        // Canon/CanonPowerShotS110-new.jpg, reduced to the two FocalLength
        // occurrences and the DOF inputs the real file resolves. The EXIF
        // sub-IFD stores 0x920a as the full-precision rational 11109/1000
        // (ValueConv 11.109); the XMP packet repeats the tag as
        // exif:FocalLength="11109/1000", which the XMP parser stores already
        // PrintConv-formatted ("11.1 mm"). XMP.pm declares the whole exif
        // namespace `PRIORITY => 0, # not as reliable as actual EXIF tags`
        // (XMP.pm:1992; XMP-tiff at 1900 and XMP-exifEX at 2462 likewise), so
        // in ExifTool the EXIF occurrence keeps the bare `FocalLength` key and
        // BuildCompositeTags hands DOF `$val[0]` = 11.109, printing
        // `2.19 m (1.45 - 3.64 m)`. When the XMP occurrence (recorded later,
        // minted at the same shim priority) is allowed to win instead, the
        // composite consumes the rounded 11.1 and prints 2.20 -- a value no
        // ExifTool ever emits for this file.
        let mut m = MetadataMap::new();
        m.insert(
            "ExifIFD:FocalLength",
            TagValue::Rational {
                numerator: 11109,
                denominator: 1000,
            },
        );
        // The XMP APP1 segment is recorded after the EXIF one, exactly as
        // `process_xmp_segments` does for this file.
        m.insert("XMP-exif:FocalLength", TagValue::new_string("11.1 mm"));
        m.insert("Composite:Aperture", TagValue::new_string("4.0"));
        m.set_value_form("Composite:Aperture", "4");
        m.insert(
            "Composite:CircleOfConfusion",
            TagValue::new_string("0.006 mm"),
        );
        m.set_value_form("Composite:CircleOfConfusion", "0.00646288985171026");
        // DOF's `$d = $val[4]` (SubjectDistance) is the distance the real
        // file resolves; the Canon pair below is present but unused.
        m.insert("XMP-exif:SubjectDistance", TagValue::new_string("2.07 m"));
        m.insert("Canon:FocusDistanceUpper", TagValue::new_string("2.11 m"));
        m.insert("Canon:FocusDistanceLower", TagValue::new_string("0 m"));

        // The bare key must resolve to the EXIF rational, not the XMP print
        // string (ExifTool.pm:9564's priority arbitration with the XMP table's
        // PRIORITY => 0).
        assert_eq!(resolve(&m, "FocalLength").as_deref(), Some("11109/1000"));

        apply(&mut m);
        assert_eq!(
            m.get_string("Composite:DOF"),
            Some("2.19 m (1.45 - 3.64 m)")
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

    #[test]
    fn pass_order_defers_every_inhibit_bearing_composite() {
        // The whole correctness of Inhibit gating rests on the primary
        // always being attempted before its inhibit-bearing alternates
        // within the same pass (see `pass_order`'s doc comment). Pin that
        // ordering property directly, independent of any one composite's
        // implementation status.
        let order = pass_order();
        assert_eq!(order.len(), COMPOSITES.len());
        let first_inhibit_pos = order
            .iter()
            .position(|&i| !COMPOSITES[i].inhibit.is_empty())
            .expect("at least one Inhibit-bearing composite is generated");
        assert!(
            order[..first_inhibit_pos]
                .iter()
                .all(|&i| COMPOSITES[i].inhibit.is_empty()),
            "an Inhibit-bearing composite was ordered before a non-inhibit one"
        );
        assert!(
            order[first_inhibit_pos..]
                .iter()
                .all(|&i| !COMPOSITES[i].inhibit.is_empty()),
            "a non-inhibit composite was ordered after an Inhibit-bearing one"
        );
    }

    #[test]
    fn exif_lens_id_and_lens_id_2_are_both_generated_with_the_inhibit_wired() {
        // Regression pin for the codegen dedup fix: Exif.pm defines LensID
        // twice under one Name ('LensID' and 'LensID-2', Exif.pm:5362-5385),
        // and the old (module, Name) dedup key silently dropped the second
        // one -- see codegen_composite.py's own comment on this. Both must
        // survive table generation, and only the alternate (`LensID-2`)
        // carries an `Inhibit` on `Composite:LensID`.
        let exif_lens_ids: Vec<&Composite> = COMPOSITES
            .iter()
            .filter(|c| c.module == "Exif" && c.name == "LensID")
            .collect();
        assert_eq!(
            exif_lens_ids.len(),
            2,
            "expected Exif's primary LensID and its LensID-2 fallback"
        );
        let primary = exif_lens_ids
            .iter()
            .find(|c| !c.require.is_empty())
            .expect("the LensType-requiring primary");
        assert!(primary.inhibit.is_empty());
        assert_eq!(primary.require, &[(0, "LensType")]);

        let fallback = exif_lens_ids
            .iter()
            .find(|c| c.require.is_empty())
            .expect("the Desire-only LensID-2 fallback");
        assert_eq!(fallback.inhibit, &[(4, "Composite:LensID")]);
    }

    #[test]
    fn xmp_lens_id_also_carries_its_inhibit_on_the_exif_primary() {
        // XMP.pm's own LensID (XMP.pm:2789-2801) is a *different* module, so
        // it never collided with Exif's under the old dedup key -- but it
        // was still silently dropping its `Inhibit` field before this step,
        // since the generator never read `Inhibit` at all.
        let xmp_lens_id = COMPOSITES
            .iter()
            .find(|c| c.module == "XMP" && c.name == "LensID")
            .expect("generated XMP LensID composite");
        assert_eq!(xmp_lens_id.inhibit, &[(6, "Composite:LensID")]);
    }

    #[test]
    fn inhibited_lens_id_fallback_never_overwrites_the_primarys_answer() {
        // Seed BOTH the primary's inputs (a real `%canonLensTypes` string
        // with fractional alternatives) and LensID-2's own inputs
        // (LensModel/Lens/Make, which would otherwise let it fire too), and
        // confirm the primary's disambiguated answer survives untouched.
        //
        // These are `CanonEOS-1D.jpg`'s own values, and the expected string
        // is what the pinned 13.59 oracle prints for that file -- note it is
        // NOT the `Canon:LensType` text: `PrintLensID` narrows
        // "... or Other Lens" down to the Sigma alternative using
        // FocalLength/MaxAperture, which is the whole reason this composite
        // cannot be an alias of LensType.
        let mut m = map_of(&[
            (
                "Canon:LensType",
                "Canon EF 28-70mm f/2.8L USM or Other Lens",
            ),
            ("ExifIFD:FocalLength", "47.0 mm"),
            ("Canon:MaxAperture", "2.8"),
            ("Canon:MinFocalLength", "28 mm"),
            ("Canon:MaxFocalLength", "70 mm"),
            ("ExifIFD:LensModel", "EF 50mm f/1.8"),
            ("ExifIFD:Lens", "50mm F1.8"),
            ("IFD0:Make", "Canon"),
        ]);
        apply(&mut m);
        assert_eq!(
            m.get_string("Composite:LensID"),
            Some("Canon EF 28-70mm f/2.8L USM or Sigma 28-70mm f/2.8 EX"),
            "the LensType-based primary must win, not the LensModel/Lens fallback"
        );
    }

    /// With no `LensType` at all, nothing inhibits `LensID-2`
    /// (Exif.pm:5371-5373), so the LensModel text fallback is the whole of
    /// ExifTool's answer and oxidex must reproduce it.
    #[test]
    fn lens_id_2_fires_when_no_lens_type_inhibits_it() {
        let mut m = map_of(&[
            ("ExifIFD:LensModel", "EF 50mm f/1.8"),
            ("ExifIFD:Lens", "50mm F1.8"),
            ("IFD0:Make", "Canon"),
        ]);
        apply(&mut m);
        assert_eq!(m.get_string("Composite:LensID"), Some("EF 50mm f/1.8"));
    }

    /// The regression this step exists to prevent: a maker `LensType` whose
    /// lookup tables are NOT transcribed here (Nikon -- its real
    /// `Composite:LensID` is `%Image::ExifTool::Nikon::Composite{LensID}`,
    /// Nikon.pm:13222-13238) must leave the tag ABSENT, never fall through to
    /// LensID-2's raw text. On `Nikon.nef` the oracle says `AF-S DX
    /// Zoom-Nikkor 18-70mm f/3.5-4.5G IF-ED`; the fallback would say
    /// `18-70mm f/3.5-4.5`.
    #[test]
    fn unimplemented_maker_lens_type_omits_rather_than_falling_back() {
        let mut m = map_of(&[
            ("Nikon:LensType", "G"),
            ("Nikon:Lens", "18-70mm f/3.5-4.5"),
            ("IFD0:Make", "NIKON CORPORATION"),
        ]);
        apply(&mut m);
        assert_eq!(
            m.get_string("Composite:LensID"),
            None,
            "a maker LensType with no transcribed lookup must omit, not \
             emit the raw Lens text"
        );
    }
}

#[cfg(test)]
mod step22_bare_name_arbitration_regression {
    //! Pinned-corpus regressions this step's own full-corpus conformance
    //! run found while replacing the hard-coded group-rank table with real
    //! priority+order arbitration (`OVERHAUL_STEP18_DESIGN.md` Phase C):
    //! a same-named tag from a lower-priority source (a JPEG APP12
    //! "Picture Info" segment, `PRIORITY => 0` in ExifTool's own
    //! `%APP12::PictureInfo` table) or a source ExifTool's real file-order
    //! scan visits *before* the one that should win (SPIFF vs SOF) can
    //! otherwise win a bare Composite dependency's priority/order tie.
    //!
    //! `ExifTool.jpg` is deliberately multi-format (it round-trips through
    //! ExifTool's own test suite carrying JPEG, SPIFF, CIFF, EXIF, IPTC and
    //! an APP12 Picture Info segment all in one file), which is exactly why
    //! it is the file that exercises this: it is one of the only corpus
    //! files where a genuinely lower-priority or later-in-scan-order source
    //! carries a same-named tag a Composite also depends on.

    use std::path::Path;

    /// Skip the calling test when its pinned fixture is absent.
    ///
    /// This used to be a sentinel STRING returned from `composite_string`, which
    /// every caller had to remember to check -- and all but one did not, so a
    /// missing corpus turned into `left: Some("<skipped: fixture absent>")` vs
    /// `right: Some("8x8")`. That is indistinguishable from a real wrong value,
    /// and it cost four branches a false RED when a container eviction removed
    /// the corpus symlink: the code was fine, the fixtures were gone.
    ///
    /// A macro that `return`s cannot be silently ignored the way a sentinel can.
    macro_rules! fixture_or_skip {
        ($path:expr) => {
            if !Path::new($path).is_file() {
                eprintln!(
                    "skip: pinned fixture {} not present -- not a failure",
                    $path
                );
                return;
            }
        };
    }

    fn composite_string(path: &str, key: &str) -> Option<String> {
        let path = Path::new(path);
        let report = crate::core::operations::read_metadata_report(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        report
            .metadata
            .get_string(key)
            .map(std::string::ToString::to_string)
    }

    const EXIFTOOL_JPG: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images/ExifTool.jpg";

    /// `Composite:ImageSize`/`Megapixels` bare-resolve `ImageWidth`/
    /// `ImageHeight`, both ordinary (undeclared) priority in ExifTool
    /// (`JPEG.pm`'s `%SPIFF` table sets no `PRIORITY`). Real ExifTool's
    /// SOF marker is always scanned *after* an APP8 SPIFF marker in a real
    /// JPEG byte stream, so `File:ImageWidth` (from SOF) is recorded after
    /// `SPIFF:ImageWidth` and wins the tie by `order`
    /// (`ExifTool.pm:9564`). Pinned oracle (13.59, `-G -s -j -a`):
    /// `Composite:ImageSize` = `"8x8"`, `Composite:Megapixels` =
    /// `6.4e-05`. Before this step's `operations.rs` fix (processing
    /// `process_spiff_segments` before `process_sof_segments_with_options`,
    /// matching that real scan order), oxidex read `"3000x4500"`/`13.5` --
    /// SPIFF's own (much larger, unrelated) declared dimensions.
    #[test]
    fn exiftool_jpg_image_size_prefers_the_later_scanned_sof_dimensions() {
        fixture_or_skip!(EXIFTOOL_JPG);
        assert_eq!(
            composite_string(EXIFTOOL_JPG, "Composite:ImageSize"),
            Some("8x8".to_string())
        );
    }

    #[test]
    fn exiftool_jpg_megapixels_matches_the_sof_dimensions_not_spiffs() {
        fixture_or_skip!(EXIFTOOL_JPG);
        let mp = composite_string(EXIFTOOL_JPG, "Composite:Megapixels");
        let mp: f64 = mp.expect("Composite:Megapixels").parse().expect("numeric");
        assert!(
            (mp - 0.000064).abs() < 1e-9,
            "expected ~6.4e-05 (8x8 px), got {mp}"
        );
    }

    /// `Composite:Aperture`'s unqualified `Desire => {0 => 'FNumber'}`
    /// (Exif.pm:4782) must bind `ExifIFD:FNumber` ("3.5"), not the JPEG
    /// APP12 Picture Info segment's own same-named `APP12:FNumber`
    /// ("11.0") -- `%APP12::PictureInfo` declares `PRIORITY => 0`
    /// (APP12.pm:27), which the JPEG-segment merge point silently dropped
    /// before this step's `jpeg_helpers.rs` fix (looping through
    /// `picture_info.iter()`'s winner-only projection and re-inserting via
    /// the plain `insert()` shim, instead of `MetadataMap::merge`, flattened
    /// every occurrence back to `SHIM_DEFAULT_PRIORITY`). Pinned oracle:
    /// `Composite:Aperture` = `"3.5"`, `Composite:LightValue` = `"10.9"`.
    #[test]
    fn exiftool_jpg_aperture_defers_to_exif_over_the_priority_zero_app12_segment() {
        fixture_or_skip!(EXIFTOOL_JPG);
        assert_eq!(
            composite_string(EXIFTOOL_JPG, "Composite:Aperture"),
            Some("3.5".to_string())
        );
        assert_eq!(
            composite_string(EXIFTOOL_JPG, "Composite:LightValue"),
            Some("10.9".to_string())
        );
    }
}

#[cfg(test)]
mod step29_generated_expression_regression {
    //! Step 29 (R6): pinned-corpus regressions for the three Composites
    //! `codegen_composite.py`'s `$val[N]` grammar compiler
    //! (`tools/exiftool-tables/exprs.py::compile_composite`) auto-derives
    //! with zero hand-written code in `compute.rs` -- see
    //! `src/composite/generated_compute.rs`. Each value below is quoted
    //! from the pinned 13.59 oracle (`exiftool -G1 -s -a`), on a real
    //! sample from `combined-samples` that exercises the composite.

    use std::path::Path;

    /// Skip the calling test when its pinned fixture is absent.
    ///
    /// This used to be a sentinel STRING returned from `composite_string`, which
    /// every caller had to remember to check -- and all but one did not, so a
    /// missing corpus turned into `left: Some("<skipped: fixture absent>")` vs
    /// `right: Some("8x8")`. That is indistinguishable from a real wrong value,
    /// and it cost four branches a false RED when a container eviction removed
    /// the corpus symlink: the code was fine, the fixtures were gone.
    ///
    /// A macro that `return`s cannot be silently ignored the way a sentinel can.
    macro_rules! fixture_or_skip {
        ($path:expr) => {
            if !Path::new($path).is_file() {
                eprintln!(
                    "skip: pinned fixture {} not present -- not a failure",
                    $path
                );
                return;
            }
        };
    }

    fn composite_string(path: &str, key: &str) -> Option<String> {
        let path = Path::new(path);
        let report = crate::core::operations::read_metadata_report(path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        report
            .metadata
            .get_string(key)
            .map(std::string::ToString::to_string)
    }

    const FLIR_JPG: &str = "/tmp/oxidex-exiftool-cache/combined-samples/FLIR.jpg";
    const PANASONIC_RW2: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Panasonic.rw2";

    /// FLIR.pm:1311-1315: `PeakSpectralSensitivity => { Require =>
    /// 'FLIR:PlanckB', ValueConv => '14387.6515/$val', PrintConv =>
    /// 'sprintf("%.1f um", $val)' }`. The bare `$val` here is the single
    /// Require'd input aliased to `$val[0]` (ExifTool.pm:3611-3612), not a
    /// scalar-tag conversion -- see `compile_composite`'s own doc comment.
    /// Pinned oracle on `FLIR.jpg` (`PlanckB` = 1374.5): `"10.5 um"`.
    #[test]
    fn flir_peak_spectral_sensitivity_matches_the_pinned_oracle() {
        fixture_or_skip!(FLIR_JPG);
        assert_eq!(
            composite_string(FLIR_JPG, "Composite:PeakSpectralSensitivity"),
            Some("10.5 um".to_string())
        );
    }

    /// PanasonicRaw.pm's `ImageWidth`/`ImageHeight` (`ValueConv => '$val[1]
    /// - $val[0]'` over `SensorRightBorder`/`SensorLeftBorder` and
    /// `SensorBottomBorder`/`SensorTopBorder`) have no PrintConv, so the
    /// generated arm's `print` is the same `perl_num`-formatted value.
    /// Pinned oracle on `Panasonic.rw2`
    /// (SensorLeftBorder/Right/Top/Bottom = 8/3656/6/2742): `3648`/`2736`.
    #[test]
    fn panasonicraw_image_size_matches_the_pinned_oracle() {
        fixture_or_skip!(PANASONIC_RW2);
        assert_eq!(
            composite_string(PANASONIC_RW2, "Composite:ImageWidth"),
            Some("3648".to_string())
        );
        assert_eq!(
            composite_string(PANASONIC_RW2, "Composite:ImageHeight"),
            Some("2736".to_string())
        );
    }

    const CANON_S110_JPG: &str =
        "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonPowerShotS110-new.jpg";
    const SONY_A100_JPG: &str =
        "/tmp/oxidex-exiftool-cache/combined-samples/Sony/SonyDSLR-A100.jpg";
    const CANON_EOS10D_JPG: &str =
        "/tmp/oxidex-exiftool-cache/combined-samples/Canon/CanonEOS10D.jpg";

    /// The optics composites must consume ValueConv-level inputs, exactly as
    /// `BuildCompositeTags` reads `$$rawValue{...}` (the post-ValueConv
    /// store) for every `@val` element (ExifTool.pm:4008+). This file's
    /// EXIF 0x920a is 11109/1000 (ValueConv 11.109); the XMP packet repeats
    /// the tag PrintConv-rounded, and XMP.pm:1992 demotes the whole exif
    /// namespace to `PRIORITY => 0`, so ExifTool's DOF sees 11.109 and prints
    /// `2.19 m (1.45 - 3.64 m)` (pinned 13.59 oracle, `-G1 -s`). Consuming
    /// the rounded 11.1 instead prints 2.20 -- and FOV flips 38.4 -> 38.5.
    #[test]
    fn canon_s110_dof_and_fov_consume_the_exif_rational_not_the_xmp_print_form() {
        fixture_or_skip!(CANON_S110_JPG);
        assert_eq!(
            composite_string(CANON_S110_JPG, "Composite:DOF"),
            Some("2.19 m (1.45 - 3.64 m)".to_string())
        );
        assert_eq!(
            composite_string(CANON_S110_JPG, "Composite:FOV"),
            Some("38.4 deg".to_string())
        );
    }

    /// The DSLR-A100's `FocusDistance` (Minolta.pm `%minoltaA100`, ValueConv
    /// `2**(($val-126)/16)` = 10.3747164372081, PrintConv `%.2f m`) feeds
    /// Composite:FOV's distance term. The pinned 13.59 oracle prints
    /// `67.3 deg (13.81 m)`; feeding the PrintConv-rounded 10.37 instead
    /// prints 13.80.
    #[test]
    fn sony_a100_fov_consumes_the_focus_distance_value_conv() {
        fixture_or_skip!(SONY_A100_JPG);
        assert_eq!(
            composite_string(SONY_A100_JPG, "Composite:FOV"),
            Some("67.3 deg (13.81 m)".to_string())
        );
    }

    /// Canon.pm's `%Canon::FocalLength` keys 2/3 (FocalPlaneXSize/YSize,
    /// Canon.pm:2727-2769) are conditioned on `$$self{Model}` -- the IFD0
    /// Model ("Canon EOS 10D"), whose trailing `10D` passes
    /// `/\b(1DS?|5D|D30|D60|10D|20D|30D|K236)$/`. With those sizes present,
    /// `CalcScaleFactor35efl` derives 1.55015938943742 and the pinned 13.59
    /// oracle prints DOF `0.42 m (12.59 - 13.01 m)` and FocalLength35efl
    /// `365.0 mm (35 mm equivalent: 565.8 mm)`. Gating them on
    /// CanonImageType ("IMG:EOS 10D JPEG", which ends in "JPEG") drops both
    /// sizes, and the fallback sensor-size path lands on 1.5886 instead.
    #[test]
    fn canon_eos10d_scale_factor_uses_the_focal_plane_sizes_gated_on_the_exif_model() {
        fixture_or_skip!(CANON_EOS10D_JPG);
        assert_eq!(
            composite_string(CANON_EOS10D_JPG, "Composite:DOF"),
            Some("0.42 m (12.59 - 13.01 m)".to_string())
        );
        assert_eq!(
            composite_string(CANON_EOS10D_JPG, "Composite:FocalLength35efl"),
            Some("365.0 mm (35 mm equivalent: 565.8 mm)".to_string())
        );
    }
}
