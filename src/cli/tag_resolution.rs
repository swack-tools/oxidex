//! Step 20: family-aware `-TAG` request resolution over every retained
//! occurrence, replacing the exact/suffix match `output_formatter::
//! tag_matches_filter` used until now.
//!
//! This is the piece that finally makes Step 18/19's retained occurrences
//! (`TagOccurrence`, `TagSink`) reachable from the CLI. See
//! `OVERHAUL_STEP18_DESIGN.md` §2.3 Phase C and AGENTS.md's tagmodel/1.4 and
//! tagmodel/1.6 findings for the defects this replaces:
//!
//! * a bare `-Make` request matched every occurrence whose stored key ended
//!   in `:Make` with no notion of priority, so which one displayed (when
//!   more than one existed) was whatever `tags.retain`/iteration order
//!   happened to produce -- not ExifTool's actual winner;
//! * a group-qualified request like `-EXIF:Make` matched nothing at all,
//!   because oxidex's stored key prefixes (`IFD0`, `ExifIFD`, `GPS`,
//!   `Canon`, `CIFF`, ...) are ExifTool's *family 1* groups, not family 0 --
//!   the request-side qualifier was never split out or mapped, so an exact
//!   match against `"IFD0:Make"` never fired for a `"EXIF"` qualifier.
//!
//! The core arbitration rule mirrors `TagSink::record`'s own (which mirrors
//! `FoundTag`, `ExifTool.pm:9448`+): among every occurrence sharing a
//! requested tag's short name (and, if a group qualifier was given, matching
//! it), the highest-`priority` occurrence wins; a tie goes to the occurrence
//! with the larger `order` (the more recently recorded one). `-a` skips the
//! arbitration and keeps every match, in file order.

use crate::cli::args::CliArgs;
use crate::core::exiftool_compat::format_tag_value_rules;
use crate::core::read_options::ReadOptions;
use crate::core::tag_occurrence::{Instance, TagOccurrence};
use crate::core::{MetadataMap, TagValue};
use std::collections::HashSet;

/// Maps a stored occurrence's `group0` to ExifTool's real family-0 group.
///
/// For occurrences recorded through [`crate::core::MetadataMap::
/// insert_occurrence`]/`insert_occurrence_with_raw` with an explicit
/// (non-empty) `group1`, `group0` already *is* the true family-0 group (the
/// convention those two constructors document: the literal insert key is
/// `"{family0}:{name}"`, and `group1` carries the real family-1 label
/// separately -- `File:FileSize`'s `group0="File"`, `group1="System"` is the
/// worked example). For the ~4,000 call sites still going through the plain
/// `insert()` shim, `group0` is simply whatever preceded the first `:` in
/// the literal key -- which for the vast majority of this codebase's
/// existing keys (`IFD0:Make`, `ExifIFD:FocalLength`, `GPS:GPSLatitude`,
/// `Canon:FocalLength`, `CIFF:Make`, ...) is actually ExifTool's *family 1*
/// group (AGENTS.md's tagmodel/1.6 finding). This function is what lets a
/// group-qualified request like `-EXIF:Make` resolve against those
/// legacy-shim occurrences anyway, by mapping the family-1-flavored label
/// back to the family-0 group ExifTool itself reports for it.
///
/// Deliberately a small, explicitly-cited allowlist rather than a guess:
/// every arm below is confirmed against the pinned 13.59 oracle's own
/// `GROUPS` declaration for the table that owns it, not inferred. An
/// unrecognized label passes through unchanged -- which is a no-op for
/// labels that already are the true family-0 group (`File`, `MakerNotes`,
/// `IPTC`, `Composite`, `ICC_Profile`, `Photoshop`, ...) and, for anything
/// not yet classified here, is no worse than today's total absence of
/// family-0 resolution.
pub fn resolve_family0(group0: &str) -> &str {
    match group0 {
        // Exif.pm:412 `GROUPS => { 0 => 'EXIF', 1 => 'IFD0', ... }`, and the
        // family-1 overrides for the other IFDs/sub-IFDs in the same file
        // (`Groups => { 1 => 'ExifIFD' }` etc., Exif.pm:2008, :2722) all
        // leave family 0 at the table's own 'EXIF'. GPS.pm:52 is the same
        // shape: `GROUPS => { 0 => 'EXIF', 1 => 'GPS', ... }`.
        "IFD0" | "IFD1" | "IFD2" | "ExifIFD" | "GPS" | "InteropIFD" | "SubIFD" => "EXIF",
        // CanonRaw.pm:50 `%Main = ( GROUPS => { 0 => 'MakerNotes', ... } )`
        // for the CIFF case (`process_ciff_app0_segments`'s `CIFF:` keys);
        // every other manufacturer table in this codebase's existing
        // `Manufacturer:Tag` key convention follows the same
        // `GROUPS => { 0 => 'MakerNotes', 1 => '<Manufacturer>' }` shape
        // (e.g. Canon.pm's own `%Main`).
        "CIFF" | "Canon" | "CanonCustom" | "Nikon" | "Sony" | "Pentax" | "Panasonic"
        | "Olympus" | "FujiFilm" | "Leica" | "SigmaRaw" | "PhaseOne" => "MakerNotes",
        // ID3.pm's per-version tables set only family 1 -- `%ID3::v1` is
        // `GROUPS => { 1 => 'ID3v1', 2 => 'Audio' }` (ID3.pm:335-337) -- so
        // family 0 falls through to `%ID3::Main`'s own 'ID3'. The pinned
        // oracle prints `[ID3] Title` under `-G0` and `[ID3v1] Title` under
        // `-G1` for the same tag. Without this, `Composite:DateTimeOriginal`'s
        // `Desire => 'ID3:Year'` (ID3.pm:841-859) never binds a v1-only file:
        // `t/images/Real.rm` carries an ID3v1 trailer and nothing else, and
        // its `DateTimeOriginal` was the one tag still missing after the
        // RealMedia reader landed.
        "ID3v1" | "ID3v1_Enh" | "ID3v2_2" | "ID3v2_3" | "ID3v2_4" => "ID3",
        // XMP.pm namespaces are all family-0 'XMP' with the namespace
        // prefix (xmp-exif, xmp-dc, ...) as family 1.
        other if other.starts_with("XMP-") => "XMP",
        other => other,
    }
}

/// The family-1 label to show for `-Gn` display: the occurrence's real
/// `group1` when a migrated call site set one explicitly, else `group0`
/// itself (which, per [`resolve_family0`]'s doc comment, already holds a
/// family-1-flavored label for every not-yet-migrated occurrence).
pub fn family1_label(occurrence: &TagOccurrence) -> &str {
    if occurrence.group1.is_empty() {
        &occurrence.group0
    } else {
        &occurrence.group1
    }
}

/// The family-0 label to show for `-Gn` display: `group0` itself when a
/// migrated call site already set a real `group1` (the convention
/// `insert_occurrence`/`insert_occurrence_with_raw` callers follow), else
/// [`resolve_family0`] applied to `group0`.
pub fn family0_label(occurrence: &TagOccurrence) -> &str {
    if occurrence.group1.is_empty() {
        resolve_family0(&occurrence.group0)
    } else {
        &occurrence.group0
    }
}

/// The label for an arbitrary family number, for `-Gn` display.
/// Family 2 and above are always empty in Phase A (`TagOccurrence::group2`
/// is never populated yet -- see `OVERHAUL_STEP18_DESIGN.md`), so only 0
/// and 1 ever resolve to anything.
fn family_label(occurrence: &TagOccurrence, family: u8) -> String {
    match family {
        0 => family0_label(occurrence).to_string(),
        1 => family1_label(occurrence).to_string(),
        2 => occurrence
            .group2
            .as_ref()
            .map(|g| g.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Joins the requested `-Gn:m:...` families for one occurrence into the
/// bracket/colon label ExifTool itself prints (`[MakerNotes:CIFF]`,
/// `[File:System]`), confirmed against the pinned oracle for both the
/// single-family and multi-family cases.
pub fn joined_family_label(occurrence: &TagOccurrence, families: &[u8]) -> String {
    families
        .iter()
        .map(|&family| family_label(occurrence, family))
        .collect::<Vec<_>>()
        .join(":")
}

/// Splits a requested token (`"Make"`, `"EXIF:Make"`, `"XMP-dc:Subject"`)
/// into an optional group qualifier and the short tag name, on the *last*
/// colon -- single-colon requests are the overwhelming case, and this still
/// isolates the tag name correctly for the rare multi-colon XMP-family
/// names.
fn split_request(token: &str) -> (Option<&str>, &str) {
    match token.rsplit_once(':') {
        Some((qualifier, short_name)) => (Some(qualifier), short_name),
        None => (None, token),
    }
}

/// Whether `occurrence` satisfies a request's group qualifier: matched
/// against its family-1 label (its own `group0`/`group1`, whichever is
/// real -- i.e. a request like `-IFD0:Make` or `-CIFF:Make`), or its
/// resolved family-0 label (`-EXIF:Make`, `-MakerNotes:Make`).
fn occurrence_matches_qualifier(occurrence: &TagOccurrence, qualifier: &str) -> bool {
    qualifier.eq_ignore_ascii_case(family1_label(occurrence))
        || qualifier.eq_ignore_ascii_case(family0_label(occurrence))
}

/// One occurrence chosen for CLI display, together with the literal key it
/// should render under.
pub struct ResolvedOccurrence<'a> {
    pub occurrence: &'a TagOccurrence,
    /// `occurrence.lookup_key()` -- kept alongside rather than recomputed at
    /// every call site, since [`crate::core::tag_occurrence::TagOccurrence::
    /// lookup_key`] allocates.
    pub lookup_key: String,
}

/// Resolves every requested token against every occurrence ever recorded in
/// `metadata` (not just each literal key's own winner), applying group
/// qualifiers and, unless `all_occurrences`, ExifTool's own priority/order
/// arbitration (see the module doc comment) to pick exactly one per request.
///
/// A token that matches nothing is silently skipped -- matching
/// `output_formatter::tag_matches_filter`'s existing behavior for an
/// unmatched filter entry.
pub fn resolve_requested_tags<'a>(
    metadata: &'a MetadataMap,
    requested: &[String],
    all_occurrences: bool,
) -> Vec<ResolvedOccurrence<'a>> {
    resolve_requested_tags_labelled(metadata, requested, all_occurrences, None)
}

/// [`resolve_requested_tags`], for output that keys each tag by its group
/// label (`-j -G<n>`): same-named XMP occurrences are arbitrated per
/// displayed label of `label_families`, so differently-labelled XMP tags
/// each keep their own winner, as ExifTool's JSON does (pinned 13.59
/// `-j -G1 -Test XMP6.xmp` returns both `XMP-xxxx:Test` and `XMP-tmp0:Test`).
pub fn resolve_requested_tags_labelled<'a>(
    metadata: &'a MetadataMap,
    requested: &[String],
    all_occurrences: bool,
    label_families: Option<&[u8]>,
) -> Vec<ResolvedOccurrence<'a>> {
    let mut out = Vec::new();
    for token in requested {
        let (qualifier, short_name) = split_request(token);
        let mut matches: Vec<&TagOccurrence> = metadata
            .all_occurrences()
            .map(|(_, occurrence)| occurrence)
            .filter(|occurrence| occurrence.name.eq_ignore_ascii_case(short_name))
            .filter(|occurrence| {
                qualifier.is_none_or(|q| occurrence_matches_qualifier(occurrence, q))
            })
            .collect();
        if matches.is_empty() {
            continue;
        }
        if all_occurrences {
            matches.sort_by_key(|occurrence| occurrence.order);
            out.extend(matches.into_iter().map(|occurrence| ResolvedOccurrence {
                lookup_key: occurrence.lookup_key(),
                occurrence,
            }));
        } else {
            // `FoundTag`'s own tie rule (`ExifTool.pm:9541-9564`), replicated
            // here exactly as `TagSink::record` applies it incrementally
            // rather than as a flat `max_by_key((priority, order))`: fold
            // the matches in file order, and an arrival displaces the
            // running winner only when `new.priority >= effective_old_
            // priority` AND the instance guard below allows it, where a
            // running winner whose own priority is `0` is promoted to `1`
            // for that comparison (`ExifTool.pm:9541-9551`, "promote
            // existing 0-priority tag so it takes precedence over a new
            // 0-tag"). A flat `max_by_key` gets `Priority => 0` families
            // wrong: among several 0-priority arrivals it picks the one
            // with the largest `order` (the LAST), the opposite of the
            // FIRST-wins default JPEG COM's `Comment` and JUMBF's
            // `JUMDType`/`JUMDLabel` both need and that `TagSink::record`'s
            // own winner projection already gives `-j`'s default
            // (non-`-TAG`) output path -- this bug was invisible for
            // `Comment` (an explicit `-Comment` request silently returned
            // the wrong one of two occurrences) until the Stage 4
            // duplicate-loss scan (`tools/exiftool-tables/
            // duplicate_loss_scan.py`) started retaining JUMBF's
            // occurrences too and made the same mis-resolution visible
            // there.
            //
            // Rule 2, the DOC_NUM/`Instance` guard, rides along in the same
            // fold: an occurrence recorded under a non-default
            // sub-document/track `Instance` never displaces a winner
            // recorded under a *different* `Instance`, regardless of
            // priority (`ExifTool.pm:9564`'s `(not $$self{DOC_NUM} or ...)`).
            // Without it, `-TrackID` against `CanonRaw.cr3` returns Track4's
            // `4` here (largest `order` among four equal-priority ties)
            // where the correct answer, matching `TagSink::record`'s own
            // winner projection (what default non-`-TAG` `-j` output uses)
            // and the pinned oracle's `-TrackID` default, is Track1's `1`.
            //
            // Same-named XMP occurrences are first reduced to ExifTool's XMP
            // winner (`xmp_found_tag_winner`, on the priorities the XMP
            // parser stored); only that winner then competes with other
            // groups' occurrences under the existing cross-group fold, whose
            // `TagOccurrence::priority` inputs are unchanged pre-existing
            // behavior. Under `-j -G<n>` the XMP entries printed per label
            // follow the exiftool script (`xmp_label_selection`).
            let (xmp, mut others): (Vec<&TagOccurrence>, Vec<&TagOccurrence>) = matches
                .into_iter()
                .partition(|occurrence| is_xmp_occurrence(occurrence));
            if xmp.is_empty() {
                let winner = found_tag_winner(others);
                out.push(ResolvedOccurrence {
                    lookup_key: winner.lookup_key(),
                    occurrence: winner,
                });
                continue;
            }
            let xmp_winner = xmp_found_tag_winner(xmp.clone());
            others.push(xmp_winner);
            let overall = found_tag_winner(others);
            let mut picked = match label_families {
                None => vec![overall],
                Some(families) => {
                    let mut picked = xmp_label_selection(&xmp, overall, families);
                    if !is_xmp_occurrence(overall) {
                        picked.push(overall);
                        picked.sort_by_key(|occurrence| occurrence.order);
                    }
                    picked
                }
            };
            out.extend(picked.drain(..).map(|occurrence| ResolvedOccurrence {
                lookup_key: occurrence.lookup_key(),
                occurrence,
            }));
        }
    }
    out
}

/// `FoundTag`'s winner among same-named occurrences: fold them in file order,
/// letting an arrival displace the running winner only under
/// ExifTool.pm:9541-9564's rules (see `resolve_requested_tags`).
fn found_tag_winner(mut matches: Vec<&TagOccurrence>) -> &TagOccurrence {
    matches.sort_by_key(|occurrence| occurrence.order);
    let mut remaining = matches.into_iter();
    let mut winner = remaining.next().expect("matches is non-empty");
    for candidate in remaining {
        // ExifTool.pm:9541-9551: promote an existing Priority => 0
        // winner to 1 for the comparison, so a later Priority => 0
        // arrival never displaces the first one.
        let effective_old_priority = if winner.priority == 0 {
            1
        } else {
            winner.priority
        };
        // ExifTool.pm:9564's `(not $$self{DOC_NUM} or ...)`: an
        // occurrence recorded under a non-default sub-document/track
        // Instance never displaces a winner recorded under a
        // *different* Instance, regardless of priority.
        let instance_ok =
            candidate.instance == Instance::default() || candidate.instance == winner.instance;
        if candidate.priority >= effective_old_priority && instance_ok {
            winner = candidate;
        }
    }
    winner
}

/// Whether `occurrence` is an XMP tag (family 0 `XMP`).
fn is_xmp_occurrence(occurrence: &TagOccurrence) -> bool {
    family0_label(occurrence) == "XMP"
}

/// ExifTool's FoundTag priority for an XMP occurrence: what the XMP parser
/// computed from the property's raw tag ID path and resolved namespace
/// ([`crate::parsers::xmp::priority`]) and stored on the occurrence. An XMP
/// occurrence recorded without one (a caller that synthesizes an XMP key
/// outside the parser) is treated as an unknown tag, which FoundXMP mints at
/// `Priority => 0` (XMP.pm ~3595).
fn xmp_found_tag_priority(occurrence: &TagOccurrence) -> i8 {
    occurrence.xmp_priority.unwrap_or(0)
}

/// ExifTool's winner among same-named XMP occurrences, per FoundTag
/// (ExifTool.pm ~9468-9590), folded in document order. The first occurrence
/// stores its priority only when non-zero, and an unstored or zero stored
/// priority is promoted to 1 for the comparison ("promote existing
/// 0-priority tag so it takes precedence over a new 0-tag"). A later
/// occurrence replaces the winner iff its priority is `>=` that, and then
/// stores its own priority, zero included (`$$self{PRIORITY}{$tag} =
/// $priority` in the duplicate branch). So an earlier 0 beats a later 0, a
/// later 1 beats an earlier 0 or 1, and a -1 winner is replaced by the next
/// 0, which then holds against later 0s.
///
/// This arbitrates XMP occurrences among themselves. How the XMP winner
/// competes with other groups' same-named tags is the pre-existing
/// cross-group fold (`found_tag_winner`), unchanged here.
fn xmp_found_tag_winner<'a>(mut matches: Vec<&'a TagOccurrence>) -> &'a TagOccurrence {
    matches.sort_by_key(|occurrence| occurrence.order);
    let mut remaining = matches.into_iter();
    let mut winner = remaining.next().expect("matches is non-empty");
    let mut stored = xmp_found_tag_priority(winner);
    for candidate in remaining {
        let old = if stored == 0 { 1 } else { stored };
        let priority = xmp_found_tag_priority(candidate);
        // ExifTool.pm's `(not $$self{DOC_NUM} or ...)` guard, as in
        // `found_tag_winner`.
        let instance_ok =
            candidate.instance == Instance::default() || candidate.instance == winner.instance;
        if priority >= old && instance_ok {
            winner = candidate;
            stored = priority;
        }
    }
    winner
}

/// The XMP occurrences JSON output with `-G<n>` prints for one tag name,
/// following the exiftool script rather than FoundTag alone (exiftool
/// ~2745 and ~2952): FoundTag has already chosen one `overall` winner across
/// every group; the script walks the tag and its duplicates in file order,
/// skips a duplicate whose label equals the winner's (the "look ahead" that
/// lets the priority tag print), and otherwise prints the first entry of
/// each `group:name` token (`%noDups`). So each label shows the overall
/// winner when it carries that label, else its first occurrence in file
/// order. Returned in file order.
fn xmp_label_selection<'a>(
    xmp: &[&'a TagOccurrence],
    overall: &'a TagOccurrence,
    families: &[u8],
) -> Vec<&'a TagOccurrence> {
    let mut sorted = xmp.to_vec();
    sorted.sort_by_key(|occurrence| occurrence.order);
    let overall_label = joined_family_label(overall, families);
    let mut labels: Vec<String> = Vec::new();
    let mut chosen: Vec<&'a TagOccurrence> = Vec::new();
    for occurrence in sorted {
        let label = joined_family_label(occurrence, families);
        if labels.contains(&label) {
            continue;
        }
        let pick = if label == overall_label && is_xmp_occurrence(overall) {
            overall
        } else {
            occurrence
        };
        labels.push(label);
        chosen.push(pick);
    }
    chosen.sort_by_key(|occurrence| occurrence.order);
    chosen
}

/// Lookup keys of XMP key-winners a listing without `-a` does not print.
///
/// XMP properties are keyed by their family-1 namespace group
/// (`XMP-xxxx:Test`, `XMP-tmp0:Test`), so same-named properties from two
/// namespaces are distinct keys and a key-winner listing would print both,
/// where ExifTool stores them under one tag and prints only the winner
/// unless `-a` is given (XMP6.xmp `-G1 -s`: `[XMP-xxxx] Test : trout`;
/// XMP.xmp: `[XMP-exif] NativeDigest` only), chosen by
/// [`xmp_found_tag_winner`]. With `label_families` (JSON with `-G<n>`) the
/// printed set is [`xmp_label_selection`] against the overall winner, which
/// is the XMP winner's pre-existing cross-group fold with the other groups'
/// same-named key winners.
///
/// Each key contributes its own winner occurrence; same-key duplicates
/// (several packets) are already resolved by the sink.
fn xmp_same_name_losers(metadata: &MetadataMap, label_families: Option<&[u8]>) -> HashSet<String> {
    let mut by_name: Vec<(&str, Vec<(&String, &TagOccurrence)>)> = Vec::new();
    for (key, occurrence) in metadata.winner_occurrences() {
        let name: &str = &occurrence.name;
        match by_name.iter_mut().find(|(known, _)| *known == name) {
            Some((_, entries)) => entries.push((key, occurrence)),
            None => by_name.push((name, vec![(key, occurrence)])),
        }
    }
    let mut losers = HashSet::new();
    for (_, entries) in by_name {
        let (xmp, others): (Vec<_>, Vec<_>) = entries
            .into_iter()
            .partition(|(_, occurrence)| is_xmp_occurrence(occurrence));
        if xmp.len() < 2 {
            continue;
        }
        let xmp_occurrences: Vec<&TagOccurrence> = xmp.iter().map(|(_, o)| *o).collect();
        let xmp_winner = xmp_found_tag_winner(xmp_occurrences.clone());
        let kept: Vec<&TagOccurrence> = match label_families {
            None => vec![xmp_winner],
            Some(families) => {
                let mut candidates: Vec<&TagOccurrence> = others.iter().map(|(_, o)| *o).collect();
                candidates.push(xmp_winner);
                let overall = found_tag_winner(candidates);
                xmp_label_selection(&xmp_occurrences, overall, families)
            }
        };
        for (key, occurrence) in xmp {
            if !kept.iter().any(|keep| std::ptr::eq(*keep, occurrence)) {
                losers.insert(key.clone());
            }
        }
    }
    losers
}

/// The value to display for `occurrence`: PrintConv-formatted (matching
/// `exiftool_compat`'s per-tag rules, without final output escaping)
/// per occurrence when `no_print_conv` is
/// false, or the pre-PrintConv form when true.
///
/// The pre-PrintConv form is `occurrence.value` when a migrated call site
/// attached one via `insert_occurrence_with_raw` (`File:FileSize`'s byte
/// count, for one), else the existing APEX ValueConv for legacy rational
/// storage, else `occurrence.raw`. This matches whole-map raw projection
/// and composite dependency resolution without inverting printed labels.
pub fn resolved_display_value(occurrence: &TagOccurrence, no_print_conv: bool) -> TagValue {
    if no_print_conv {
        occurrence.value_conv()
    } else {
        format_tag_value_rules(&occurrence.lookup_key(), &occurrence.raw)
    }
}

/// Builds a synthesized [`MetadataMap`] ready to hand to the existing
/// `OutputFormatter` implementations unfiltered (`filter_tags: None`):
/// every value is already display-ready (PrintConv applied or not, per
/// `no_print_conv`), so nothing downstream must re-run
/// `format_for_exiftool` or re-filter by name.
///
/// `group_display` selects the key shape:
/// * `None` -- the occurrence's own literal `lookup_key` (today's existing
///   convention for a plain `-TAG` request, unchanged).
/// * `Some(families)` -- `"{label}:{short_name}"` (no brackets), matching
///   the pinned oracle's own `-a -G0:1 -j` JSON key shape (`"EXIF:IFD0:
///   Make"`, `"MakerNotes:CIFF:Make"`) exactly. [`display_key_bracketed`]
///   is the human/short/CSV-formatter counterpart, used the same way but
///   with `[label] short_name` brackets instead.
///
/// Losing occurrences under `-a` share a short name; when they do, a
/// numeric suffix (`" (2)"`, `" (3)"`, ...) disambiguates the literal key so
/// none of them silently overwrite one another in the synthesized map --
/// mirroring `FoundTag`'s own `"$tag ($nextInd)"` duplicate-key convention
/// (`ExifTool.pm:9532`) at the point it becomes visible.
pub fn build_display_map(
    resolved: &[ResolvedOccurrence<'_>],
    group_display: Option<&[u8]>,
    no_print_conv: bool,
    bracketed: bool,
) -> MetadataMap {
    let mut out = MetadataMap::with_capacity(resolved.len());
    for entry in resolved {
        let base_key = match group_display {
            Some(families) => {
                let label = joined_family_label(entry.occurrence, families);
                if bracketed {
                    format!("[{label}] {}", entry.occurrence.name)
                } else {
                    format!("{label}:{}", entry.occurrence.name)
                }
            }
            None => entry.lookup_key.clone(),
        };
        let key = dedupe_key(&out, base_key);
        let value = resolved_display_value(entry.occurrence, no_print_conv);
        out.insert(key, value);
    }
    out
}

/// Appends `" (N)"` if `base_key` is already present in `map`, trying
/// successive `N` until a free key is found -- the numbering
/// `insert_low_priority_retained`/`FoundTag` itself uses for a genuine
/// same-key duplicate (`ExifTool.pm:9532`).
fn dedupe_key(map: &MetadataMap, base_key: String) -> String {
    if !map.contains_key(&base_key) {
        return base_key;
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{base_key} ({n})");
        if !map.contains_key(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// Renders `-a -Gn` human/short output directly, in `resolved`'s own order,
/// rather than through [`build_display_map`] + the existing
/// `HumanReadableFormatter`/`ShortFormatter`.
///
/// Those formatters sort their input alphabetically by key
/// (`tags.sort_by_key(|(name, _)| *name)`) -- a property every other caller
/// relies on and this step has no reason to change generally. But the
/// pinned oracle's `-a -G1 -s -Make` row requires file order (`[IFD0]
/// FUJIFILM` before `[CIFF] Canon`), and `[CIFF] Make` alphabetizes before
/// `[IFD0] Make` -- the bracket label is part of the sorted string. Since
/// [`resolve_requested_tags`] already returns `-a` matches in file order
/// (sorted by `occurrence.order`), rendering directly here preserves it
/// instead of losing it to a formatter written for a different case.
///
/// Reuses `output_formatter`'s own per-tag value rendering
/// (`format_tag_value`/`format_tag_value_short`) so enum/GPS/binary
/// rendering stays identical to every other output path; only the line
/// shape (`"[label] name: value\n"`) and the ordering are specific to this
/// function.
pub fn render_group_display_lines(
    resolved: &[ResolvedOccurrence<'_>],
    families: &[u8],
    no_print_conv: bool,
    short: bool,
) -> String {
    let mut out = String::new();
    for entry in resolved {
        let label = joined_family_label(entry.occurrence, families);
        let value = resolved_display_value(entry.occurrence, no_print_conv);
        let rendered = if short {
            super::output_formatter::format_tag_value_short_with_mode(
                &entry.lookup_key,
                &value,
                no_print_conv,
            )
        } else {
            super::output_formatter::format_tag_value_with_mode(
                &entry.lookup_key,
                &value,
                no_print_conv,
            )
        };
        out.push_str(&format!(
            "[{label}] {}: {rendered}\n",
            entry.occurrence.name
        ));
    }
    out
}

/// Step 21: the display-ready result of resolving one file's raw read
/// against a set of CLI flags -- either an already-rendered block of text
/// (the `-Gn` + human/short special case [`render_group_display_lines`]'s
/// doc comment explains) or a synthesized [`MetadataMap`] ready for any
/// `OutputFormatter` with `filter_tags: None`.
pub enum ResolvedFileOutput {
    /// Pre-rendered `"[label] name: value\n"` lines, from
    /// [`render_group_display_lines`].
    Lines(String),
    /// Display-ready metadata: PrintConv applied or not per
    /// `--no-print-conv`, already filtered/resolved, keyed the way the
    /// caller's `-Gn`/plain-key choice requires.
    Metadata(MetadataMap),
}

/// Builds one file's display-ready output from its raw read result and the
/// CLI flags that shape it -- shared by the single-file path
/// (`main.rs::handle_read_operation`) and the batch/directory path
/// (`cli::batch_processor`), so the two modes agree on `-a`, `-G*`,
/// `--no-print-conv` and the default (unfiltered) listing for the same file.
///
/// This closes the gap Step 20 left open: before this step, batch mode fed
/// `args.specific_tags()` straight into each `OutputFormatter`'s own
/// exact/suffix `filter_tags` matching, bypassing this module entirely, so
/// batch runs never saw Step 20's group/priority-aware resolution --
/// `-EXIF:Make`, `-a`, and `-Gn` were single-file-only. Batch also never
/// applied [`ReadOptions::strip_extended_only`], so a directory read still
/// showed Step 21's hex-fallback/ZIP-forensic diagnostic tags by default
/// while a single-file read of the same file did not.
///
/// Mirrors `handle_read_operation`'s two branches:
/// * a specific `-TAG` request resolves through
///   [`resolve_requested_tags`]/[`build_display_map`]/
///   [`render_group_display_lines`], exactly as documented on those
///   functions;
/// * the unfiltered default listing goes through
///   [`ReadOptions::strip_extended_only`] (Step 21's extended-namespace
///   filter -- moot for the specific-request branch above, since a
///   filtered-out tag can still be reached there by explicit name) and then
///   pre-PrintConv selection or the per-tag PrintConv rules. Final output
///   escaping belongs to the selected formatter.
pub fn resolve_file_output(raw_metadata: &MetadataMap, args: &CliArgs) -> ResolvedFileOutput {
    let tag_filter = args.specific_tags();
    let no_print_conv = !args.exiftool_compat();

    if let Some(requested) = &tag_filter {
        let label_families = args.group_display.as_deref().filter(|_| args.json);
        let resolved =
            resolve_requested_tags_labelled(raw_metadata, requested, args.all_tags, label_families);
        if let Some(families) = &args.group_display
            && !args.json
            && !args.csv
        {
            let lines =
                render_group_display_lines(&resolved, families, no_print_conv, args.short_format);
            return ResolvedFileOutput::Lines(lines);
        }
        let metadata = build_display_map(
            &resolved,
            args.group_display.as_deref(),
            no_print_conv,
            !args.json,
        );
        return ResolvedFileOutput::Metadata(metadata);
    }

    // No specific `-TAG` request: the "list everything" default. Before this
    // fix, `-G*` was silently a no-op here -- it only ever took effect on
    // the branch above, so `oxidex -G1 -s` (or `-j` with no explicit tag)
    // rendered every tag ungrouped, with no bracket/prefix at all,
    // regardless of `-G*`. That made every occurrence's real family-1 group
    // -- ICC's `ICC-header`/`ICC-cicp`/`ICC-view`/`ICC-meas` among them --
    // invisible through the single most natural way of asking for it.
    // Reuses the same two renderers the `-TAG` branch above does
    // (`render_group_display_lines` for the bracket/file-order human-short
    // case, `build_display_map` otherwise), just fed every occurrence that
    // survives `ReadOptions::strip_extended_only`'s filter instead of a
    // name-requested subset.
    let options = ReadOptions::new(&[], args.extended_output);

    // `strip_extended_only` only needs to decide which *keys* survive;
    // running it against `MetadataMap::iter()`'s winner-only view and
    // reading back its key set (rather than its flattened values) is
    // what lets the occurrences walked below keep their real `group1` --
    // `strip_extended_only`'s own output re-derives `group0` from the
    // literal key via the plain `insert()` shim and would flatten it
    // right back out otherwise.
    //
    // `-a` and `-G*` are independent axes, and this is where that stopped
    // being true before this fix: the occurrence walk below used to live
    // inside the `-G*` branch, so `args.all_tags` was consulted only when a
    // group display was also requested. An ungrouped `oxidex -a -s` (or
    // `-a -j`) fell through to the winner-only projection at the bottom and
    // silently dropped every retained duplicate that shares a lookup key --
    // `File:Comment` on `t/images/ExifTool.jpg` printed one of its two JPEG
    // COM segments where the pinned 13.59 oracle prints both. The same tag
    // under an *explicit* `-Comment` request already returned two, because
    // that path has always gone through `resolve_requested_tags`; only the
    // unfiltered listing was affected.
    let surviving = options.strip_extended_only(raw_metadata);
    if args.group_display.is_some() || args.all_tags {
        let surviving_keys: HashSet<&str> = surviving.keys().map(String::as_str).collect();
        let mut resolved: Vec<ResolvedOccurrence> = if args.all_tags {
            raw_metadata
                .all_occurrences()
                .filter(|(key, _)| surviving_keys.contains(key.as_str()))
                .map(|(lookup_key, occurrence)| ResolvedOccurrence {
                    occurrence,
                    lookup_key,
                })
                .collect()
        } else {
            let label_families = args.group_display.as_deref().filter(|_| args.json);
            let losers = xmp_same_name_losers(raw_metadata, label_families);
            raw_metadata
                .winner_occurrences()
                .filter(|(key, _)| surviving_keys.contains(key.as_str()))
                .filter(|(key, _)| !losers.contains(key.as_str()))
                .map(|(key, occurrence)| ResolvedOccurrence {
                    occurrence,
                    lookup_key: key.clone(),
                })
                .collect()
        };
        resolved.sort_by_key(|entry| entry.occurrence.order);

        if let Some(families) = &args.group_display {
            if !args.json && !args.csv {
                let lines = render_group_display_lines(
                    &resolved,
                    families,
                    no_print_conv,
                    args.short_format,
                );
                return ResolvedFileOutput::Lines(lines);
            }
            let metadata = build_display_map(&resolved, Some(families), no_print_conv, !args.json);
            return ResolvedFileOutput::Metadata(metadata);
        }

        // Ungrouped `-a`: the same occurrence set, keyed by each occurrence's
        // own literal `lookup_key`. `build_display_map`'s `dedupe_key` gives
        // occurrences that share one key the `" (N)"` suffix `FoundTag` itself
        // uses (`ExifTool.pm:9532`), so two `File:Comment`s survive into the
        // synthesized map instead of overwriting each other. Values go through
        // `resolved_display_value`, which applies the PrintConv rules one
        // occurrence at a time, without final output escaping. The
        // whole-map transform below uses those same rules, so the winner's
        // rendering is unchanged either way.
        let metadata = build_display_map(&resolved, None, no_print_conv, !args.json);
        return ResolvedFileOutput::Metadata(metadata);
    }

    let losers = xmp_same_name_losers(raw_metadata, None);
    let metadata = if no_print_conv {
        // strip_extended_only rebuilt the display map and discarded value
        // forms. Use its key selection with the original winning occurrences.
        let values = raw_metadata.without_print_conv();
        values
            .iter()
            .filter(|(key, _)| surviving.contains_key(key) && !losers.contains(key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    } else {
        // Keep the complete PrintConv scalar until the chosen renderer:
        // JSON checks numeric/boolean typing before deleting NULs, while
        // plain output applies Printable's own control/whitespace rules.
        let mut formatted = MetadataMap::with_capacity(surviving.len());
        for (key, value) in surviving.iter() {
            if losers.contains(key.as_str()) {
                continue;
            }
            formatted.insert(key.clone(), format_tag_value_rules(key, value));
        }
        formatted
    };
    ResolvedFileOutput::Metadata(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Instance;

    fn sample_metadata() -> MetadataMap {
        // Mirrors the pinned oracle's ExifTool.jpg shape: IFD0's Make comes
        // first (lower order), CIFF's Make second (higher order), both
        // ordinary (non-zero) priority.
        let mut metadata = MetadataMap::new();
        metadata.insert("IFD0:Make", TagValue::new_string("FUJIFILM"));
        metadata.insert("CIFF:Make", TagValue::new_string("Canon"));
        metadata
    }

    #[test]
    fn json_resolution_preserves_nuls_until_serialization() {
        use crate::cli::args::DetectorMode;
        use crate::cli::output_formatter::{JsonFormatter, OutputFormatter};

        let mut source = MetadataMap::new();
        source.insert_occurrence(
            "IFD0:DocumentName",
            TagValue::new_string("12\0"),
            1,
            "IFD0",
            Instance::default(),
        );
        source.insert_occurrence(
            "IFD0:DocumentName",
            TagValue::new_string("34\0"),
            0,
            "IFD0",
            Instance::default(),
        );
        source.insert("IFD0:Orientation", TagValue::new_integer(6));
        source.insert("ExifIFD:FNumber", TagValue::new_rational(28, 10));
        // Exercise unfiltered, selected, grouped and duplicate projection;
        // --no-print-conv must preserve the same complete string as well.
        for (selected, grouped, all_tags) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (false, false, true),
            (true, true, true),
        ] {
            for no_print_conv in [false, true] {
                let args = CliArgs {
                    detector: DetectorMode::Signature,
                    json: true,
                    csv: false,
                    short_format: false,
                    all_tags,
                    group_display: grouped.then_some(vec![0, 1]),
                    extended_output: false,
                    recursive: false,
                    preserve_file_times: false,
                    backup: false,
                    readonly: true,
                    exiftool_compat: !no_print_conv,
                    tags_from_file: None,
                    date_format: None,
                    dry_run: false,
                    strict: false,
                    args: if selected {
                        vec!["-DocumentName".into(), "fixture.webp".into()]
                    } else {
                        vec!["fixture.webp".into()]
                    },
                };
                let ResolvedFileOutput::Metadata(map) = resolve_file_output(&source, &args) else {
                    panic!("JSON resolution unexpectedly returned plain lines");
                };
                let values: Vec<_> = map
                    .iter()
                    .filter(|(key, _)| key.contains("DocumentName"))
                    .map(|(_, value)| value.as_string().unwrap())
                    .collect();
                assert_eq!(values.len(), if all_tags { 2 } else { 1 });
                assert!(values.contains(&"12\0"));
                if all_tags {
                    assert!(values.contains(&"34\0"));
                }
                let output = JsonFormatter.format(&map, None);
                let json: serde_json::Value = serde_json::from_str(&output).unwrap();
                let rendered: Vec<_> = json[0]
                    .as_object()
                    .unwrap()
                    .iter()
                    .filter(|(key, _)| key.contains("DocumentName"))
                    .map(|(_, value)| value)
                    .collect();
                assert!(rendered.contains(&&serde_json::json!("12")));
                assert!(rendered.iter().all(|value| value.is_string()));
                if all_tags {
                    assert!(rendered.contains(&&serde_json::json!("34")));
                }
                if !selected && !no_print_conv {
                    assert_eq!(
                        map.get_string(if grouped {
                            "EXIF:IFD0:Orientation"
                        } else {
                            "IFD0:Orientation"
                        }),
                        Some("Rotate 90 CW")
                    );
                    assert_eq!(
                        map.get_string(if grouped {
                            "EXIF:ExifIFD:FNumber"
                        } else {
                            "ExifIFD:FNumber"
                        }),
                        Some("2.8")
                    );
                }
            }
        }
        assert_eq!(source.get_string("IFD0:DocumentName"), Some("12\0"));
    }

    #[test]
    fn bare_request_picks_the_newest_equal_priority_occurrence() {
        let metadata = sample_metadata();
        let resolved = resolve_requested_tags(&metadata, &["Make".to_string()], false);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].occurrence.raw, TagValue::new_string("Canon"));
    }

    #[test]
    fn group_qualified_request_resolves_against_family_zero() {
        let metadata = sample_metadata();
        let resolved = resolve_requested_tags(&metadata, &["EXIF:Make".to_string()], false);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].occurrence.raw, TagValue::new_string("FUJIFILM"));
    }

    #[test]
    fn group_qualified_request_resolves_against_family_one() {
        let metadata = sample_metadata();
        let resolved = resolve_requested_tags(&metadata, &["IFD0:Make".to_string()], false);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].occurrence.raw, TagValue::new_string("FUJIFILM"));
    }

    #[test]
    fn all_occurrences_mode_returns_every_match_in_file_order() {
        let metadata = sample_metadata();
        let resolved = resolve_requested_tags(&metadata, &["Make".to_string()], true);
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0].occurrence.raw, TagValue::new_string("FUJIFILM"));
        assert_eq!(resolved[1].occurrence.raw, TagValue::new_string("Canon"));
    }

    #[test]
    fn a_lower_priority_occurrence_never_wins_a_bare_request() {
        let mut metadata = MetadataMap::new();
        metadata.insert_occurrence(
            "ExifIFD:FocalLength",
            TagValue::new_rational(34, 1),
            1,
            "ExifIFD",
            Instance::default(),
        );
        metadata.insert_occurrence(
            "Canon:FocalLength",
            TagValue::new_string("34 mm"),
            0,
            "Canon",
            Instance::default(),
        );
        let resolved = resolve_requested_tags(&metadata, &["FocalLength".to_string()], false);
        assert_eq!(resolved.len(), 1);
        assert_eq!(&*resolved[0].occurrence.group0, "ExifIFD");
    }

    #[test]
    fn bare_request_never_lets_a_later_instance_displace_an_earlier_one() {
        // Mirrors CanonRaw.cr3's four equal-priority `tkhd` TrackID
        // occurrences (Track1..Track4, ExifTool.pm:9564's DOC_NUM/Instance
        // guard, QuickTime.pm:1522-1524's `Priority => 0` on TrackID). Only
        // Track1's occurrence may ever win a bare `-TrackID` request: each
        // later track carries a *different* Instance than the incumbent
        // winner, so rule 2 must block it regardless of `order`/priority.
        // Before this fix, `resolve_requested_tags`'s flat `max_by_key`
        // picked Track4's -- the largest `order` among the ties -- which is
        // exactly the bug this test pins against silently returning.
        let mut metadata = MetadataMap::new();
        for track in 1..=4u32 {
            metadata.insert_occurrence(
                "QuickTime:TrackID",
                TagValue::new_integer(track as i64),
                0,
                "Track",
                Instance(track),
            );
        }
        let resolved = resolve_requested_tags(&metadata, &["TrackID".to_string()], false);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].occurrence.raw, TagValue::new_integer(1));
        assert_eq!(resolved[0].occurrence.instance, Instance(1));
    }

    #[test]
    fn bare_request_still_lets_the_last_default_instance_occurrence_win() {
        // The QuickTime counter-case the same guard must NOT break:
        // SourceImageWidth is grouped per-track via SET_GROUP1
        // (QuickTime.pm:10354), not DOC_NUM, so its occurrences are all
        // recorded under the default Instance and ordinary (non-zero)
        // priority -- the LAST track's value is meant to win, matching the
        // pinned oracle's own `-SourceImageWidth` answer on CanonRaw.cr3
        // (Track3's 6288, not Track1's 6000).
        let mut metadata = MetadataMap::new();
        for width in [6000, 6288] {
            metadata.insert_occurrence(
                "QuickTime:SourceImageWidth",
                TagValue::new_integer(width),
                1,
                "Track",
                Instance::default(),
            );
        }
        let resolved = resolve_requested_tags(&metadata, &["SourceImageWidth".to_string()], false);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].occurrence.raw, TagValue::new_integer(6288));
    }

    #[test]
    fn joined_family_label_matches_the_oracles_multi_family_shape() {
        let metadata = sample_metadata();
        let resolved = resolve_requested_tags(&metadata, &["Make".to_string()], true);
        let ciff = resolved
            .iter()
            .find(|r| &*r.occurrence.group0 == "CIFF")
            .unwrap();
        assert_eq!(joined_family_label(ciff.occurrence, &[1]), "CIFF");
        assert_eq!(
            joined_family_label(ciff.occurrence, &[0, 1]),
            "MakerNotes:CIFF"
        );
    }

    #[test]
    fn build_display_map_colon_style_matches_the_oracles_json_key_shape() {
        let metadata = sample_metadata();
        let resolved = resolve_requested_tags(&metadata, &["Make".to_string()], true);
        let map = build_display_map(&resolved, Some(&[0, 1]), false, false);
        assert_eq!(map.get_string("EXIF:IFD0:Make"), Some("FUJIFILM"));
        assert_eq!(map.get_string("MakerNotes:CIFF:Make"), Some("Canon"));
    }

    #[test]
    fn build_display_map_bracketed_style_prefixes_the_short_name() {
        let metadata = sample_metadata();
        let resolved = resolve_requested_tags(&metadata, &["Make".to_string()], true);
        let map = build_display_map(&resolved, Some(&[1]), false, true);
        assert_eq!(map.get_string("[IFD0] Make"), Some("FUJIFILM"));
        assert_eq!(map.get_string("[CIFF] Make"), Some("Canon"));
    }

    #[test]
    fn no_print_conv_selects_the_raw_form_when_one_was_attached() {
        let mut metadata = MetadataMap::new();
        metadata.insert_occurrence_with_raw(
            "File:FileSize",
            TagValue::new_string("26 kB"),
            TagValue::new_integer(26106),
            crate::core::SHIM_DEFAULT_PRIORITY,
            "System",
            Instance::default(),
        );
        let resolved = resolve_requested_tags(&metadata, &["FileSize".to_string()], false);
        assert_eq!(resolved.len(), 1);
        assert_eq!(
            resolved_display_value(resolved[0].occurrence, true),
            TagValue::new_integer(26106)
        );
        assert_eq!(
            resolved_display_value(resolved[0].occurrence, false),
            TagValue::new_string("26 kB")
        );
    }
    #[test]
    fn raw_projection_and_renderers_keep_values_in_default_and_selected_modes() {
        use crate::cli::args::DetectorMode;
        use crate::cli::output_formatter::{
            CsvFormatter, HumanReadableFormatter, JsonFormatter, OutputFormatter, ShortFormatter,
        };
        let mut source = MetadataMap::new();
        for (name, print, value) in [
            ("Canon:LensType", "Canon EF 300mm f/2.8L USM", "136"),
            ("Composite:ShootingMode", "Full auto", "10"),
            ("Composite:ImageSize", "65x100", "65 100"),
            ("Composite:Megapixels", "0.006", "0.0065"),
        ] {
            source.insert_occurrence_with_raw(
                name,
                TagValue::new_string(print),
                TagValue::new_string(value),
                1,
                "",
                Instance::default(),
            );
        }
        source.insert("IFD0:Orientation", TagValue::new_integer(6));
        source.insert("IFD0:0xDEAD", TagValue::new_string("hidden"));
        for (selected, grouped, all_tags) in [
            (false, false, false),
            (true, false, false),
            (false, true, true),
        ] {
            for raw in [false, true] {
                let mut args = CliArgs {
                    detector: DetectorMode::Signature,
                    json: true,
                    csv: false,
                    short_format: false,
                    all_tags,
                    group_display: grouped.then_some(vec![0, 1]),
                    extended_output: false,
                    recursive: false,
                    preserve_file_times: false,
                    backup: false,
                    readonly: true,
                    exiftool_compat: !raw,
                    tags_from_file: None,
                    date_format: None,
                    dry_run: false,
                    strict: false,
                    args: if selected {
                        vec![
                            "-LensType".into(),
                            "-ShootingMode".into(),
                            "-ImageSize".into(),
                            "-Megapixels".into(),
                            "-Orientation".into(),
                            "fixture.webp".into(),
                        ]
                    } else {
                        vec!["fixture.webp".into()]
                    },
                };
                let ResolvedFileOutput::Metadata(map) = resolve_file_output(&source, &args) else {
                    panic!("expected map")
                };
                assert!(!map.keys().any(|key| key.contains("0xDEAD")));
                let json: serde_json::Value =
                    serde_json::from_str(&JsonFormatter.format_with_mode(&map, None, raw)).unwrap();
                let field = |name: &str| {
                    json[0]
                        .as_object()
                        .unwrap()
                        .iter()
                        .find(|(key, _)| key.rsplit(':').next() == Some(name))
                        .unwrap()
                        .1
                };
                assert_eq!(
                    field("Orientation"),
                    &if raw {
                        serde_json::json!(6)
                    } else {
                        serde_json::json!("Rotate 90 CW")
                    }
                );
                assert_eq!(
                    field("LensType"),
                    &if raw {
                        serde_json::json!(136)
                    } else {
                        serde_json::json!("Canon EF 300mm f/2.8L USM")
                    }
                );
                assert_eq!(
                    field("ShootingMode"),
                    &if raw {
                        serde_json::json!(10)
                    } else {
                        serde_json::json!("Full auto")
                    }
                );
                assert_eq!(
                    field("ImageSize"),
                    &serde_json::json!(if raw { "65 100" } else { "65x100" })
                );
                assert_eq!(
                    field("Megapixels"),
                    &serde_json::json!(if raw { 0.0065 } else { 0.006 })
                );
                if !grouped {
                    for formatter in [
                        &ShortFormatter as &dyn OutputFormatter,
                        &HumanReadableFormatter,
                        &CsvFormatter,
                    ] {
                        let text = formatter.format_with_mode(&map, None, raw);
                        if raw {
                            assert!(!text.contains("Canon EF"));
                            assert!(!text.contains("Full auto"));
                            assert!(!text.contains("Rotate 90 CW"));
                            assert!(text.contains("136"));
                            assert!(text.contains("10"));
                        } else {
                            assert!(text.contains("Canon EF"));
                            assert!(text.contains("Full auto"));
                            assert!(text.contains("Rotate 90 CW"));
                        }
                    }
                } else {
                    args.json = false;
                    args.short_format = true;
                    let ResolvedFileOutput::Lines(lines) = resolve_file_output(&source, &args)
                    else {
                        panic!("expected lines")
                    };
                    assert!(lines.contains(if raw {
                        "LensType: 136"
                    } else {
                        "LensType: Canon EF"
                    }));
                    assert!(lines.contains(if raw {
                        "ShootingMode: 10"
                    } else {
                        "ShootingMode: Full auto"
                    }));
                }
            }
        }
    }

    /// A metadata map holding one XMP packet the way the sidecar reader
    /// stores it (`parse_xmp_prioritized` keys with their priorities).
    fn xmp_packet_map(packet: &[u8]) -> MetadataMap {
        let mut metadata = MetadataMap::new();
        for (key, value, priority) in
            crate::parsers::xmp::rdf_parser::parse_xmp_prioritized(packet).unwrap()
        {
            crate::parsers::xmp::rdf_parser::insert_xmp_tag(
                &mut metadata,
                key,
                TagValue::new_string(value.into_joined()),
                priority,
            );
        }
        metadata
    }

    fn xmp_args(json: bool, all_tags: bool, families: Option<u8>, args: Vec<String>) -> CliArgs {
        CliArgs {
            detector: crate::cli::args::DetectorMode::Signature,
            json,
            csv: false,
            short_format: !json,
            all_tags,
            group_display: families.map(|family| vec![family]),
            extended_output: false,
            recursive: false,
            preserve_file_times: false,
            backup: false,
            readonly: true,
            exiftool_compat: true,
            tags_from_file: None,
            date_format: None,
            dry_run: false,
            strict: false,
            args,
        }
    }

    /// Every `(key, value)` the output holds for an XMP tag other than
    /// XMPToolkit: a `[G] Name: value` line becomes `G:Name`; map keys are
    /// kept as they are.
    fn xmp_entries(output: ResolvedFileOutput) -> Vec<(String, String)> {
        let mut entries: Vec<(String, String)> = match output {
            ResolvedFileOutput::Lines(lines) => lines
                .lines()
                .filter_map(|line| line.strip_prefix('['))
                .filter_map(|line| line.split_once("] "))
                .filter_map(|(group, rest)| {
                    rest.split_once(": ")
                        .map(|(name, value)| (format!("{group}:{name}"), value.to_string()))
                })
                .collect(),
            ResolvedFileOutput::Metadata(map) => map
                .iter()
                .map(|(key, value)| {
                    let value = match value {
                        TagValue::Array(items) => items
                            .iter()
                            .map(|item| item.as_string().unwrap_or_default().to_string())
                            .collect::<Vec<_>>()
                            .join(", "),
                        other => other.as_string().unwrap_or_default().to_string(),
                    };
                    (key.clone(), value)
                })
                .collect(),
        };
        entries.retain(|(key, _)| key.starts_with("XMP") && !key.ends_with(":XMPToolkit"));
        entries.sort();
        entries
    }

    fn sorted(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = entries
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        out.sort();
        out
    }

    /// Same-named XMP tags from different namespaces, matched to pinned
    /// ExifTool 13.59 on the review packets p1-p9, d1, d2, c1/c2/c4/c5/c7,
    /// n1/n2 (a -1 winner replaced by a 0 that then holds), k1/k2/k3 and px
    /// (raw tag IDs are case-sensitive: `cc:LegalCode`, `dc:Title`,
    /// `exif:dateTimeOriginal` are unknown), v1 (a variable-namespace
    /// structure field copies its own table's entry) and rx.
    /// ExifTool arbitrates them with FoundTag and the tables' per-tag
    /// priorities: `-j -G0` (like every listing without `-a`) keeps the one
    /// winner, `-j -G1` keeps one per group label, a bare request returns the
    /// winner (per label under `-j -G1`), and `-a` keeps all of them.
    #[test]
    fn same_named_xmp_tags_match_pinned_exiftool_winners() {
        #[allow(clippy::type_complexity)]
        let cases: &[(&str, &[(&str, &str)], &[(&str, &str)])] = &[
            // Generated from pinned ExifTool 13.59 `-j -G0` and `-j -G1` output.
            (
                // adv/p1.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/" xmlns:Iptc4xmpExt="http://iptc.org/std/Iptc4xmpExt/2008-02-29/"><photoshop:Headline>PS</photoshop:Headline></rdf:Description><rdf:Description rdf:about="" xmlns:Iptc4xmpExt="http://iptc.org/std/Iptc4xmpExt/2008-02-29/"><Iptc4xmpExt:Headline><rdf:Alt><rdf:li xml:lang="x-default">EXT</rdf:li></rdf:Alt></Iptc4xmpExt:Headline></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Headline", "PS")],
                &[
                    ("XMP-photoshop:Headline", "PS"),
                    ("XMP-iptcExt:Headline", "EXT"),
                ],
            ),
            (
                // adv/p2.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/"><xmp:Rating>1</xmp:Rating></rdf:Description><rdf:Description rdf:about="" xmlns:dex="http://ns.optimasc.com/dex/1.0/"><dex:rating>2</dex:rating></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Rating", "1")],
                &[("XMP-xmp:Rating", "1"), ("XMP-dex:Rating", "2")],
            ),
            (
                // adv/p3.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y/"><yyyy:Test>fromY</yyyy:Test></rdf:Description><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:Test>fromDC</dc:Test></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Test", "fromY")],
                &[("XMP-yyyy:Test", "fromY"), ("XMP-dc:Test", "fromDC")],
            ),
            (
                // adv/p4.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y/"><yyyy:Prefs>fromY</yyyy:Prefs></rdf:Description><rdf:Description rdf:about="" xmlns:photomechanic="http://ns.camerabits.com/photomechanic/1.0/"><photomechanic:Prefs>1:2:3:4</photomechanic:Prefs></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Prefs", "Tagged:1, ColorClass:2, Rating:3, FrameNum:4")],
                &[
                    ("XMP-yyyy:Prefs", "fromY"),
                    (
                        "XMP-photomech:Prefs",
                        "Tagged:1, ColorClass:2, Rating:3, FrameNum:4",
                    ),
                ],
            ),
            (
                // adv/p5.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y/"><yyyy:Test>fromY</yyyy:Test></rdf:Description><rdf:Description rdf:about="" xmlns:iptcCore="http://e.com/IC/"><iptcCore:Test>fromIC</iptcCore:Test></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Test", "fromY")],
                &[("XMP-yyyy:Test", "fromY"), ("XMP-iptcCore:Test", "fromIC")],
            ),
            (
                // adv/p6.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y/"><yyyy:Test>fromY</yyyy:Test></rdf:Description><rdf:Description rdf:about="" xmlns:photomech="http://e.com/PM/"><photomech:Test>fromPM</photomech:Test></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Test", "fromY")],
                &[("XMP-yyyy:Test", "fromY"), ("XMP-photomech:Test", "fromPM")],
            ),
            (
                // adv/p7.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y/"><yyyy:Title>fromY</yyyy:Title></rdf:Description><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title><rdf:Alt><rdf:li xml:lang="x-default">DC</rdf:li></rdf:Alt></dc:title></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Title", "DC")],
                &[("XMP-yyyy:Title", "fromY"), ("XMP-dc:Title", "DC")],
            ),
            (
                // adv/p8.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/"><xmp:CreateDate>2020:01:01</xmp:CreateDate></rdf:Description><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y/"><yyyy:CreateDate>1999:01:01</yyyy:CreateDate></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:CreateDate", "2020:01:01")],
                &[
                    ("XMP-xmp:CreateDate", "2020:01:01"),
                    ("XMP-yyyy:CreateDate", "1999:01:01"),
                ],
            ),
            (
                // adv/p9.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:Device="http://ns.google.com/photos/dd/1.0/device/"><Device:Foo>dev</Device:Foo></rdf:Description><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y/"><yyyy:Foo>fromY</yyyy:Foo></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Foo", "dev")],
                &[("XMP-Device:Foo", "dev"), ("XMP-yyyy:Foo", "fromY")],
            ),
            (
                // adv/d1.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:zeta="http://e.com/Z/" xmlns:alpha="http://e.com/A/"><zeta:Foo>fromZeta</zeta:Foo><alpha:Foo>fromAlpha</alpha:Foo></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Foo", "fromZeta")],
                &[("XMP-zeta:Foo", "fromZeta"), ("XMP-alpha:Foo", "fromAlpha")],
            ),
            (
                // adv/d2.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:alpha="http://e.com/A/" xmlns:zeta="http://e.com/Z/"><alpha:Foo>fromAlpha</alpha:Foo><zeta:Foo>fromZeta</zeta:Foo></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Foo", "fromAlpha")],
                &[("XMP-alpha:Foo", "fromAlpha"), ("XMP-zeta:Foo", "fromZeta")],
            ),
            (
                // review-xmp3/c1.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y/"><yyyy:Test>a</yyyy:Test></rdf:Description><rdf:Description rdf:about="" xmlns:zzzz="http://e.com/Z/"><zzzz:Test>b</zzzz:Test></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Test", "a")],
                &[("XMP-yyyy:Test", "a"), ("XMP-zzzz:Test", "b")],
            ),
            (
                // review-xmp3/c2.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y/"><yyyy:Test>a</yyyy:Test></rdf:Description><rdf:Description rdf:about="" xmlns:zzzz="http://e.com/Z/"><zzzz:Test>b</zzzz:Test></rdf:Description><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:Test>c</dc:Test></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Test", "a")],
                &[
                    ("XMP-yyyy:Test", "a"),
                    ("XMP-zzzz:Test", "b"),
                    ("XMP-dc:Test", "c"),
                ],
            ),
            (
                // review-xmp3/c4.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y/"><yyyy:Test>a</yyyy:Test></rdf:Description><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y2/"><yyyy:Test>b</yyyy:Test></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Test", "a")],
                &[("XMP-yyyy:Test", "a"), ("XMP-tmp0:Test", "b")],
            ),
            (
                // review-xmp3/c5.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:yyyy="http://e.com/Y/"><yyyy:Test>a</yyyy:Test></rdf:Description><rdf:Description rdf:about="" xmlns:zzzz="http://e.com/Z/"><zzzz:Test>b</zzzz:Test></rdf:Description><rdf:Description rdf:about="" xmlns:wwww="http://e.com/W/"><wwww:Test>c</wwww:Test></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Test", "a")],
                &[
                    ("XMP-yyyy:Test", "a"),
                    ("XMP-zzzz:Test", "b"),
                    ("XMP-wwww:Test", "c"),
                ],
            ),
            (
                // review-xmp3/c7.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:Yyyy="http://e.com/Y/"><Yyyy:Test>a</Yyyy:Test></rdf:Description><rdf:Description rdf:about="" xmlns:zzzz="http://e.com/Z/"><zzzz:test>b</zzzz:test></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Test", "a")],
                &[("XMP-Yyyy:Test", "a"), ("XMP-zzzz:Test", "b")],
            ),
            (
                // review-xmp4/n1.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:pdf="http://ns.adobe.com/pdf/1.3/"><pdf:Keywords>PDF</pdf:Keywords></rdf:Description><rdf:Description rdf:about="" xmlns:aaa="http://a.example/"><aaa:Keywords>AAA</aaa:Keywords></rdf:Description><rdf:Description rdf:about="" xmlns:bbb="http://b.example/"><bbb:Keywords>BBB</bbb:Keywords></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Keywords", "AAA")],
                &[
                    ("XMP-pdf:Keywords", "PDF"),
                    ("XMP-aaa:Keywords", "AAA"),
                    ("XMP-bbb:Keywords", "BBB"),
                ],
            ),
            (
                // review-xmp4/n2.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:pdf="http://ns.adobe.com/pdf/1.3/"><pdf:Keywords>PDF</pdf:Keywords></rdf:Description><rdf:Description rdf:about="" xmlns:aaa="http://a.example/"><aaa:Keywords>AAA</aaa:Keywords></rdf:Description><rdf:Description rdf:about="" xmlns:pdf="http://ns.adobe.com/pdf/1.3/" xmlns:ccc="http://c.example/"><ccc:Keywords>CCC</ccc:Keywords></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Keywords", "AAA")],
                &[
                    ("XMP-pdf:Keywords", "PDF"),
                    ("XMP-aaa:Keywords", "AAA"),
                    ("XMP-ccc:Keywords", "CCC"),
                ],
            ),
            (
                // review-xmp4/k1.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:aaa="http://a.example/"><aaa:LegalCode>AAA</aaa:LegalCode></rdf:Description><rdf:Description rdf:about="" xmlns:cc="http://creativecommons.org/ns#"><cc:LegalCode>CC</cc:LegalCode></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:LegalCode", "AAA")],
                &[("XMP-aaa:LegalCode", "AAA"), ("XMP-cc:LegalCode", "CC")],
            ),
            (
                // review-xmp4/k2.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:aaa="http://a.example/"><aaa:Title>AAA</aaa:Title></rdf:Description><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:Title>DC</dc:Title></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Title", "AAA")],
                &[("XMP-aaa:Title", "AAA"), ("XMP-dc:Title", "DC")],
            ),
            (
                // review-xmp4/k3.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:aaa="http://a.example/"><aaa:DateTimeOriginal>AAA</aaa:DateTimeOriginal></rdf:Description><rdf:Description rdf:about="" xmlns:exif="http://ns.adobe.com/exif/1.0/"><exif:dateTimeOriginal>EXIF</exif:dateTimeOriginal></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:DateTimeOriginal", "AAA")],
                &[
                    ("XMP-aaa:DateTimeOriginal", "AAA"),
                    ("XMP-exif:DateTimeOriginal", "EXIF"),
                ],
            ),
            (
                // review-xmp4/v1.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:aaa="http://a.example/"><aaa:ImageRegionRating>AAA</aaa:ImageRegionRating></rdf:Description><rdf:Description rdf:about="" xmlns:Iptc4xmpExt="http://iptc.org/std/Iptc4xmpExt/2008-02-29/" xmlns:xmp="http://ns.adobe.com/xap/1.0/"><Iptc4xmpExt:ImageRegion><rdf:Bag><rdf:li rdf:parseType="Resource"><xmp:Rating>5</xmp:Rating></rdf:li></rdf:Bag></Iptc4xmpExt:ImageRegion></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:ImageRegionRating", "5")],
                &[
                    ("XMP-aaa:ImageRegionRating", "AAA"),
                    ("XMP-iptcExt:ImageRegionRating", "5"),
                ],
            ),
            (
                // review-xmp4/px.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:aaa="http://a.example/"><aaa:Title>AAA</aaa:Title></rdf:Description><rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title><rdf:Alt><rdf:li xml:lang="x-default">DC</rdf:li></rdf:Alt></dc:title></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Title", "DC")],
                &[("XMP-aaa:Title", "AAA"), ("XMP-dc:Title", "DC")],
            ),
            (
                // review-xmp4/rx.xmp
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:aaa="http://a.example/"><aaa:Rating>7</aaa:Rating></rdf:Description><rdf:Description rdf:about="" xmlns:xmp="http://ns.adobe.com/xap/1.0/"><xmp:Rating>5</xmp:Rating></rdf:Description></rdf:RDF></x:xmpmeta>"#,
                &[("XMP:Rating", "5")],
                &[("XMP-aaa:Rating", "7"), ("XMP-xmp:Rating", "5")],
            ),
        ];
        for (packet, g0, g1) in cases {
            let metadata = xmp_packet_map(packet.as_bytes());
            let run = |json, all, families, extra: &[&str]| {
                let mut args: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
                args.push("fixture.xmp".to_string());
                xmp_entries(resolve_file_output(
                    &metadata,
                    &xmp_args(json, all, families, args),
                ))
            };
            let expected_g0 = sorted(g0);
            let expected_g1 = sorted(g1);
            assert!(!expected_g0.is_empty() && !expected_g1.is_empty());
            assert_eq!(
                run(true, false, Some(0), &[]),
                expected_g0,
                "-j -G0 {packet}"
            );
            assert_eq!(
                run(true, false, Some(1), &[]),
                expected_g1,
                "-j -G1 {packet}"
            );
            // Listings without `-a` print the winners: the -G1 entries whose
            // values are the -G0 winners.
            let winners: Vec<(String, String)> = expected_g1
                .iter()
                .filter(|(key, value)| {
                    let name = key.rsplit(':').next().unwrap();
                    expected_g0.contains(&(format!("XMP:{name}"), value.clone()))
                })
                .cloned()
                .collect();
            assert_eq!(winners.len(), expected_g0.len(), "{packet}");
            assert_eq!(run(false, false, Some(1), &[]), winners, "-G1 -s {packet}");
            assert_eq!(run(true, false, None, &[]), winners, "-j {packet}");
            for (key, _) in g0.iter() {
                let name = key.rsplit(':').next().unwrap();
                let request = format!("-{name}");
                let by_name = |entries: &[(String, String)]| -> Vec<(String, String)> {
                    entries
                        .iter()
                        .filter(|(k, _)| k.rsplit(':').next() == Some(name))
                        .cloned()
                        .collect()
                };
                assert_eq!(
                    run(false, false, Some(1), &[&request]),
                    by_name(&winners),
                    "{request} {packet}"
                );
                assert_eq!(
                    run(true, false, Some(1), &[&request]),
                    by_name(&expected_g1),
                    "-j -G1 {request} {packet}"
                );
            }
            assert_eq!(
                run(false, true, Some(1), &[]),
                expected_g1,
                "-a -G1 -s {packet}"
            );
        }
    }

    /// Pinned corpus files whose same-named XMP tags exercise the generated
    /// priorities (pinned ExifTool 13.59, `-G1 -s` unless noted). XMP.xmp
    /// prints only `[XMP-exif] NativeDigest`: both tables have PRIORITY 0, so
    /// the first recorded keeps the tag. PhotoMechanic.jpg prints
    /// `[XMP-iptcCore] CountryCode`, also for `-CountryCode`: the photomech
    /// copy is priority 0. XMP6.xmp prints `[XMP-xxxx] Test : trout`, and
    /// both `Test`s under `-a` and under `-j -G1 -Test`.
    #[test]
    fn pinned_corpus_xmp_winners_match_exiftool() {
        let read = |name: &str| {
            crate::test_support::pinned_fixture_path(name).map(|path| {
                crate::core::operations::read_metadata(&path).expect("pinned fixture reads")
            })
        };
        let run = |metadata: &MetadataMap, json, all, families, extra: &[&str]| {
            let mut args: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
            args.push("fixture".to_string());
            xmp_entries(resolve_file_output(
                metadata,
                &xmp_args(json, all, families, args),
            ))
        };
        let keys = |entries: Vec<(String, String)>, name: &str| -> Vec<String> {
            entries
                .into_iter()
                .filter(|(key, _)| key.rsplit(':').next() == Some(name))
                .map(|(key, _)| key)
                .collect()
        };
        if let Some(metadata) = read("XMP.xmp") {
            assert_eq!(
                keys(run(&metadata, false, false, Some(1), &[]), "NativeDigest"),
                ["XMP-exif:NativeDigest"]
            );
            assert_eq!(
                keys(run(&metadata, true, false, Some(0), &[]), "NativeDigest"),
                ["XMP:NativeDigest"]
            );
            assert_eq!(
                keys(run(&metadata, false, true, Some(1), &[]), "NativeDigest"),
                ["XMP-exif:NativeDigest", "XMP-tiff:NativeDigest"]
            );
        }
        if let Some(metadata) = read("PhotoMechanic.jpg") {
            assert_eq!(
                keys(run(&metadata, false, false, Some(1), &[]), "CountryCode"),
                ["XMP-iptcCore:CountryCode"]
            );
            assert_eq!(
                keys(run(&metadata, true, false, Some(0), &[]), "CountryCode"),
                ["XMP:CountryCode"]
            );
            assert_eq!(
                keys(
                    run(&metadata, false, false, Some(1), &["-CountryCode"]),
                    "CountryCode"
                ),
                ["XMP-iptcCore:CountryCode"]
            );
        }
        if let Some(metadata) = read("XMP6.xmp") {
            assert_eq!(
                run(&metadata, false, false, Some(1), &["-Test"]),
                sorted(&[("XMP-xxxx:Test", "trout")])
            );
            assert_eq!(
                keys(run(&metadata, false, false, Some(1), &[]), "Test"),
                ["XMP-xxxx:Test"]
            );
            assert_eq!(
                run(&metadata, true, false, Some(1), &["-Test"]),
                sorted(&[("XMP-xxxx:Test", "trout"), ("XMP-tmp0:Test", "tabby")])
            );
            assert_eq!(
                keys(run(&metadata, false, true, Some(1), &[]), "Test"),
                ["XMP-tmp0:Test", "XMP-xxxx:Test"]
            );
        }
    }

    /// Review packet t2.png: an XMP iTXt `aaa:Title` (unknown, 0) and
    /// `dc:title` (1), then a tEXt `PNG:Title` that FoundTag makes the
    /// overall winner. The exiftool script prints each label's first entry
    /// when the winner does not carry that label: pinned 13.59 `-j -G0` is
    /// `"XMP:Title": "AAA"`, and `-j -G1` has both `XMP-aaa` and `XMP-dc`.
    #[test]
    fn json_labels_without_the_overall_winner_print_their_first_xmp_entry() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/png/xmp_title_across_groups.png");
        let metadata = crate::core::operations::read_metadata(&path).expect("PNG reads");
        let run = |families, extra: &[&str]| {
            let mut args: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
            args.push("fixture.png".to_string());
            xmp_entries(resolve_file_output(
                &metadata,
                &xmp_args(true, false, Some(families), args),
            ))
        };
        assert_eq!(run(0, &[]), sorted(&[("XMP:Title", "AAA")]));
        assert_eq!(
            run(1, &[]),
            sorted(&[("XMP-aaa:Title", "AAA"), ("XMP-dc:Title", "DC")])
        );
        assert_eq!(run(0, &["-Title"]), sorted(&[("XMP:Title", "AAA")]));
        assert_eq!(
            run(1, &["-Title"]),
            sorted(&[("XMP-aaa:Title", "AAA"), ("XMP-dc:Title", "DC")])
        );
    }

    /// Every XMP-group occurrence the readers record for the pinned corpus
    /// carries the priority the XMP parser derived for it: a key inserted
    /// without one would silently arbitrate as an unknown (0) tag.
    #[test]
    fn pinned_corpus_xmp_occurrences_carry_parser_priorities() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let Some(sample) = crate::test_support::pinned_fixture_path("XMP.xmp") else {
            return;
        };
        let images = sample.parent().expect("t/images");
        let mut missing = Vec::new();
        let mut checked = 0usize;
        let mut entries: Vec<_> = std::fs::read_dir(images)
            .expect("t/images lists")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        entries.sort();
        for path in entries {
            let Ok(metadata) = crate::core::operations::read_metadata(&path) else {
                continue;
            };
            for (key, occurrence) in metadata.all_occurrences() {
                if !occurrence.group0.starts_with("XMP-") {
                    continue;
                }
                checked += 1;
                if occurrence.xmp_priority.is_none() {
                    missing.push(format!("{}: {key}", path.display()));
                }
            }
        }
        assert!(checked > 400, "only {checked} XMP occurrences checked");
        assert!(
            missing.is_empty(),
            "XMP occurrences without a priority: {missing:?}"
        );
    }
}
