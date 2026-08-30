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
use crate::core::exiftool_compat::format_for_exiftool;
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
                let instance_ok = candidate.instance == Instance::default()
                    || candidate.instance == winner.instance;
                if candidate.priority >= effective_old_priority && instance_ok {
                    winner = candidate;
                }
            }
            out.push(ResolvedOccurrence {
                lookup_key: winner.lookup_key(),
                occurrence: winner,
            });
        }
    }
    out
}

/// The value to display for `occurrence`: PrintConv-formatted (matching
/// `exiftool_compat::format_for_exiftool`'s per-key transform, applied here
/// per-occurrence instead of over a whole map) when `no_print_conv` is
/// false, or the pre-PrintConv form when true.
///
/// The pre-PrintConv form is `occurrence.value` when a migrated call site
/// attached one via `insert_occurrence_with_raw` (`File:FileSize`'s byte
/// count, for one), else `occurrence.raw` itself -- which, for every
/// call site not yet migrated, already *is* the pre-PrintConv form (see
/// `MetadataMap::without_print_conv`'s doc comment for why skipping
/// PrintConv already gave the right answer for the other ~99.5% of tags
/// before this step).
pub fn resolved_display_value(occurrence: &TagOccurrence, no_print_conv: bool) -> TagValue {
    if no_print_conv {
        occurrence
            .value
            .clone()
            .unwrap_or_else(|| occurrence.raw.clone())
    } else {
        crate::core::exiftool_compat::format_tag_value(&occurrence.lookup_key(), &occurrence.raw)
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
            super::output_formatter::format_tag_value_short(&entry.lookup_key, &value)
        } else {
            super::output_formatter::format_tag_value(&entry.lookup_key, &value)
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
///   `without_print_conv`/`format_for_exiftool`, unchanged from before this
///   step.
pub fn resolve_file_output(raw_metadata: &MetadataMap, args: &CliArgs) -> ResolvedFileOutput {
    let tag_filter = args.specific_tags();
    let no_print_conv = !args.exiftool_compat();

    if let Some(requested) = &tag_filter {
        let resolved = resolve_requested_tags(raw_metadata, requested, args.all_tags);
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
            raw_metadata
                .winner_occurrences()
                .filter(|(key, _)| surviving_keys.contains(key.as_str()))
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
        // `resolved_display_value`, which is `format_for_exiftool`'s own
        // per-key `format_tag_value` applied one occurrence at a time -- the
        // whole-map transform below is purely per-key, so the winner's
        // rendering is unchanged either way.
        let metadata = build_display_map(&resolved, None, no_print_conv, !args.json);
        return ResolvedFileOutput::Metadata(metadata);
    }

    let metadata = if no_print_conv {
        surviving.without_print_conv()
    } else {
        format_for_exiftool(&surviving)
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
}
