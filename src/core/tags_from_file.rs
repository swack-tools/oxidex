//! `-TagsFromFile`: copying tags from one file into another the way pinned
//! ExifTool 13.59's `SetNewValuesFromFile` copies them (Writer.pl:1254-1590).
//!
//! ExifTool copies a tag **by name**. Each source tag the arguments select
//! is handed to `SetNewValue(<name>, <value>)` -- the value as ExifTool
//! prints it, converted back by the destination tag's own inverse
//! conversions -- so it lands wherever `-<name>=<value>` would: the name's
//! preferred group, *plus* every other group that already carries the name.
//! A JPEG's `File:ImageWidth` is written to `XMP-tiff` (IFD0's ImageWidth is
//! protected), a PDF's Info fields to `XMP-dc`/`XMP-pdf`/`XMP-xmp`, EXIF into
//! a PNG's text chunks, a Canon maker note's `MeteringMode` into ExifIFD.
//! [`crate::writers::copy_targets`] holds that resolution, captured from the
//! pinned interpreter.
//!
//! oxidex writes every such destination its writers can write, with the
//! value the same `-TAG=VALUE` conversion gives it, and names every other
//! one -- by family-1 group and tag -- in [`CopyReport::uncopied_tags`] and
//! [`CopyReport::uncopied_groups`]. Nothing 13.59 would write is skipped
//! silently.

use crate::cli::tag_resolution::{arbitrate, family0_label, family1_label, resolved_display_value};
use crate::core::metadata_map::MetadataMap;
use crate::core::operations::{
    CopyReport, is_surgical_tiff_target, read_metadata, write_metadata_counted,
};
use crate::core::tag_occurrence::{TagOccurrence, ValueChannel};
use crate::core::{FileFormat, FileReader, TagValue};
use crate::error::{ExifToolError, Result, TagNotWritten};
use crate::io::MMapReader;
use crate::parsers::detection::detect_format;
use crate::writers::copy_targets::{
    CopyMode, CopyRequest, Destination, DestinationFile, destinations, is_copyable,
    is_protected_binary_source,
};
use std::path::Path;

/// One `-TagsFromFile` argument's selection: an optional group and a tag
/// name, either of which may carry ExifTool's wildcards (`*`, `?`).
#[derive(Debug, Clone)]
struct CopyPattern {
    group: Option<String>,
    name: String,
}

impl CopyPattern {
    fn matches(&self, key_group: &str, group1: &str, name: &str) -> bool {
        (self.name.eq_ignore_ascii_case("all") || glob_matches(&self.name, name))
            && self
                .group
                .as_deref()
                .is_none_or(|wanted| copy_group_matches(wanted, key_group, group1))
    }
}

/// A copy's filter, classified as pinned ExifTool 13.59's `-TagsFromFile`
/// classifies its arguments (`core::operations::copy_metadata_report`).
#[derive(Debug, Default)]
pub(crate) struct CopySelectors {
    /// Every source tag (`all`, no filter, or exclusions alone).
    all: bool,
    /// `GROUP:all` and wildcard selections.
    selections: Vec<CopyPattern>,
    /// `--TAG` exclusions from the selection.
    exclusions: Vec<CopyPattern>,
    /// (filter as given, source group, source name, destination) for each
    /// named tag.
    named: Vec<(String, Option<String>, String, String)>,
}

impl CopySelectors {
    pub(crate) fn parse(filters: &[String]) -> Result<Self> {
        let refuse = |filter: &str, why: &str| {
            ExifToolError::unsupported_format(format!("Cannot copy '{filter}': {why}"))
        };
        let group_ok = |group: &str| {
            !group.is_empty()
                && group
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'*' | b'?'))
        };
        let name_ok = |name: &str| {
            !name.is_empty()
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'*' | b'?'))
        };
        let wild = |text: &str| text.contains(['*', '?']);
        let mut selectors = CopySelectors::default();
        for filter in filters {
            let (exclusion, body) = match filter.strip_prefix('-') {
                Some(rest) => (true, rest),
                None => (false, filter.as_str()),
            };
            let (source_spec, dest_spec) = if let Some((from, to)) = body.split_once('>') {
                (from.trim(), Some(to.trim()))
            } else if let Some((to, from)) = body.split_once('<') {
                (from.trim(), Some(to.trim()))
            } else {
                (body.trim(), None)
            };
            let (group, name) = match source_spec.rsplit_once(':') {
                Some((group, name)) => (Some(group), name),
                None => (None, source_spec),
            };
            let group = group.filter(|group| !group.eq_ignore_ascii_case("all") && *group != "*");
            if !name_ok(name) || group.is_some_and(|group| !group_ok(group)) {
                return Err(refuse(
                    filter,
                    "not a TAG, GROUP:TAG, GROUP:all, -TAG exclusion or SRC>DST copy \
                     selector",
                ));
            }
            let selection = name.eq_ignore_ascii_case("all")
                || name == "*"
                || wild(name)
                || group.is_some_and(wild);
            let pattern = CopyPattern {
                group: group.map(str::to_string),
                name: name.to_string(),
            };
            match (exclusion, dest_spec) {
                (true, Some(_)) => {
                    return Err(refuse(filter, "an exclusion cannot be redirected"));
                }
                (true, None) => selectors.exclusions.push(pattern),
                (false, Some(_)) if selection => {
                    return Err(refuse(
                        filter,
                        "oxidex does not redirect a group, wildcard or all selection; \
                         redirect named tags",
                    ));
                }
                (false, Some(dest)) => {
                    if dest.is_empty() {
                        return Err(refuse(filter, "no destination tag"));
                    }
                    selectors.named.push((
                        filter.clone(),
                        pattern.group,
                        pattern.name,
                        dest.to_string(),
                    ));
                }
                (false, None) if selection && group.is_none() && !wild(name) => {
                    selectors.all = true;
                }
                (false, None) if selection => selectors.selections.push(pattern),
                (false, None) => selectors.named.push((
                    filter.clone(),
                    pattern.group,
                    pattern.name,
                    source_spec.to_string(),
                )),
            }
        }
        // No filter, or exclusions alone: every tag (13.59, Writer.pl
        // `SetNewValuesFromFile`: "implicitly assume '*' if first entry is an
        // exclusion").
        if selectors.selections.is_empty() && selectors.named.is_empty() {
            selectors.all = true;
        }
        Ok(selectors)
    }

    /// The copies a selection (not the named tags) makes of a source tag:
    /// `None` for a by-name copy (`all`, an ungrouped wildcard), `Some(group)`
    /// for `GROUP:all` / `GROUP:*` -- which 13.59 writes to that same group
    /// (Writer.pl:1498-1505: "use same group name for dest"). Empty when the
    /// tag is not selected or is excluded.
    fn selections(&self, key_group: &str, group1: &str, name: &str) -> Vec<Option<String>> {
        if self
            .exclusions
            .iter()
            .any(|pattern| pattern.matches(key_group, group1, name))
        {
            return Vec::new();
        }
        let mut copies: Vec<Option<String>> = Vec::new();
        if self.all {
            copies.push(None);
        }
        for pattern in &self.selections {
            if !pattern.matches(key_group, group1, name) {
                continue;
            }
            // A wildcard group (`XMP-*:all`) names no one destination group:
            // the copy goes to the source tag's own family-1 group
            // (Writer.pl:1545-1551).
            let group = pattern.group.as_deref().map(|group| {
                if group.contains(['*', '?']) {
                    group1.to_string()
                } else {
                    group.to_string()
                }
            });
            if !copies.contains(&group) {
                copies.push(group);
            }
        }
        copies
    }
}

/// ExifTool's wildcard match of a tag or group name (`*` any run, `?` one
/// character), without regard to case.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let (pattern, text): (Vec<char>, Vec<char>) = (
        pattern.to_ascii_lowercase().chars().collect(),
        text.to_ascii_lowercase().chars().collect(),
    );
    fn at(pattern: &[char], text: &[char]) -> bool {
        match pattern.split_first() {
            None => text.is_empty(),
            Some(('*', rest)) => (0..=text.len()).any(|skip| at(rest, &text[skip..])),
            Some(('?', rest)) => !text.is_empty() && at(rest, &text[1..]),
            Some((c, rest)) => text.first() == Some(c) && at(rest, &text[1..]),
        }
    }
    at(&pattern, &text)
}

/// Whether a copy selector's group names a source row's: its family-0 key
/// group or its family-1 group (`XMP-dc` for an `XMP:Title` row), or `EXIF`
/// / `XMP` for any of their directories -- without regard to case, with
/// wildcards.
fn copy_group_matches(wanted: &str, key_group: &str, group1: &str) -> bool {
    if wanted.eq_ignore_ascii_case("EXIF") {
        return ["IFD0", "IFD1", "ExifIFD", "GPS", "InteropIFD", "EXIF"]
            .iter()
            .any(|group| group.eq_ignore_ascii_case(key_group));
    }
    if wanted.eq_ignore_ascii_case("XMP") {
        return key_group.eq_ignore_ascii_case("XMP")
            || key_group
                .get(..4)
                .is_some_and(|p| p.eq_ignore_ascii_case("XMP-"));
    }
    glob_matches(wanted, key_group) || glob_matches(wanted, group1)
}

/// The value 13.59 copies for `occurrence` into `key`: the value as it
/// prints it (`GetInfo` with `PrintConv`), converted back the way
/// `-KEY=VALUE` converts it -- a rational re-rationalised from its printed
/// decimal, a label through the destination tag's own `PrintConvInv` -- or
/// the data itself for a binary value (the source is read with `Binary`,
/// and a binary value is not print-converted).
fn copied_value(source_key: &str, occurrence: &TagOccurrence, key: &str) -> Result<TagValue> {
    // An XP string copies as its stored UCS-2 re-packed, which keeps a
    // stored surrogate pair; its printed text cannot say whether a code
    // point above U+FFFF was one (xp_strings::refuse_unknown_provenance).
    if crate::writers::xp_strings::is_xp_tag_key(key)
        && crate::writers::xp_strings::is_xp_tag_key(source_key)
    {
        let value = occurrence.project(ValueChannel::Stored).into_owned();
        crate::writers::xp_strings::refuse_unknown_provenance(source_key, &value)?;
        return Ok(value);
    }
    let printed = resolved_display_value(occurrence, false);
    let text = match printed {
        TagValue::Binary(bytes) => return Ok(TagValue::Binary(bytes)),
        TagValue::String(text) if text.starts_with("(Binary data ") => {
            return match occurrence.project(ValueChannel::Stored).into_owned() {
                TagValue::Binary(bytes) => Ok(TagValue::Binary(bytes)),
                _ => Err(ExifToolError::tag_not_written(
                    key,
                    "oxidex did not keep the source's binary data for this tag",
                )),
            };
        }
        TagValue::String(text) => text,
        // A list is copied item by item (the source is read with `List`,
        // and `SetNewValue` takes the list), each item converted as a
        // `-TAG=ITEM` would be.
        TagValue::Array(items) => {
            let items = items
                .iter()
                .map(|item| {
                    let text = match item {
                        TagValue::String(text) => text.clone(),
                        other => crate::cli::output_formatter::format_tag_value_short_with_mode(
                            source_key, other, false,
                        ),
                    };
                    parse(key, &text)
                })
                .collect::<Result<Vec<_>>>()?;
            return Ok(TagValue::Array(items));
        }
        other => crate::cli::output_formatter::format_tag_value_short_with_mode(
            source_key, &other, false,
        ),
    };
    parse(key, &text)
}

/// `-KEY=TEXT`'s conversion of `text`.
fn parse(key: &str, text: &str) -> Result<TagValue> {
    crate::cli::value_parser::parse_cli_tag_value(key, text)
        .map_err(|err| ExifToolError::tag_not_written(key, err.to_string()))
}

/// One destination a copied tag is written to, and the source row whose
/// value it takes.
struct Planned<'a> {
    destination: Destination,
    /// The source row's map key and occurrence.
    source_key: &'a str,
    occurrence: &'a TagOccurrence,
    /// A named tag: a destination it cannot reach refuses the whole copy.
    strict: bool,
    /// The argument that named it, for a refusal.
    filter: Option<&'a str>,
}

/// A selection (`all`, a wildcard, `GROUP:all`) copies the source's maker
/// note block whole: 13.59 creates `ExifIFD:MakerNote<Make>` from the
/// source's, so every maker-note tag the source carries reappears in the
/// destination. oxidex does not copy a maker note block; it names each.
///
/// The block is written only where its `MakerNotes::Main` `Condition`
/// holds for the destination's Make as the write leaves it (13.59:
/// `-TagsFromFile Canon.jpg -all --Make` into a non-Canon JPEG writes no
/// Canon maker note). oxidex does not evaluate those conditions; it names
/// the block when that Make is the source's own -- the Make the source's
/// block was selected by -- and names nothing otherwise.
fn maker_note_rows<'a>(
    source: &'a MetadataMap,
    selectors: &CopySelectors,
    format: FileFormat,
    surgical: bool,
    final_make: Option<String>,
) -> Vec<(String, &'a TagOccurrence)> {
    let holds_exif = matches!(format, FileFormat::JPEG | FileFormat::PNG) || surgical;
    if final_make.is_none() || final_make != printed_make(source) {
        return Vec::new();
    }
    let rows: Vec<(String, &TagOccurrence)> = source
        .keyed_occurrences()
        .filter(|(_, occurrence)| family0_label(occurrence) == "MakerNotes")
        .map(|(_, occurrence)| (family1_label(occurrence).to_string(), occurrence))
        .collect();
    let Some((group1, _)) = rows.first() else {
        return Vec::new();
    };
    let block = format!("MakerNote{group1}");
    if !holds_exif
        || selectors
            .selections("ExifIFD", "ExifIFD", &block)
            .is_empty()
    {
        return Vec::new();
    }
    rows
}

/// A file's Make as 13.59 prints it (its bare `-Make`).
fn printed_make(metadata: &MetadataMap) -> Option<String> {
    crate::cli::tag_resolution::resolve_requested_tag(metadata, "Make")
        .map(|occurrence| printed_text(&resolved_display_value(occurrence, false), "Make"))
}

fn printed_text(value: &TagValue, key: &str) -> String {
    match value {
        TagValue::String(text) => text.clone(),
        other => crate::cli::output_formatter::format_tag_value_short_with_mode(key, other, false),
    }
}

/// oxidex's PDF reader also reports the Info dictionary's two dates under
/// their PDF key names (`CreationDate`, `ModDate`), beside ExifTool's own
/// `CreateDate` / `ModifyDate` for the same fields. 13.59's source has no
/// such tags, so they are not copied by name (their fields are, under
/// ExifTool's names).
fn is_pdf_date_alias(occurrence: &TagOccurrence) -> bool {
    family0_label(occurrence) == "PDF" && matches!(&*occurrence.name, "CreationDate" | "ModDate")
}

/// Whether the copied value is a structure (only a structure copies to a
/// structure tag).
fn is_structure(occurrence: &TagOccurrence) -> bool {
    matches!(
        resolved_display_value(occurrence, false),
        TagValue::Struct(_)
    )
}

/// The row 13.59 sets last among `rows` -- the one a bare `-NAME` reports
/// (`FoundTag`'s priority and file-order arbitration).
fn arbitrate_rows<'a>(
    mut rows: Vec<(&'a str, &'a TagOccurrence)>,
) -> Option<(&'a str, &'a TagOccurrence)> {
    rows.sort_by_key(|(_, occurrence)| occurrence.order);
    let winner = arbitrate(rows.iter().map(|(_, occurrence)| *occurrence))?;
    rows.into_iter()
        .find(|(_, occurrence)| std::ptr::eq(*occurrence, winner))
}

/// Copies what `selectors` pick from `source_metadata` into `dest`
/// (`core::operations::copy_metadata_report`).
pub(crate) fn copy_tags(
    source_metadata: &MetadataMap,
    dest: &Path,
    selectors: &CopySelectors,
) -> Result<CopyReport> {
    let reader = MMapReader::new(dest)?;
    let format = detect_format(&reader)?;
    let surgical = is_surgical_tiff_target(format, &reader);
    // TIFF identifier 0x55: a Panasonic RAW (ExifTool.pm:8646-8659).
    let header = reader.read(0, reader.size().min(4) as usize).unwrap_or(&[]);
    let panasonic_raw = matches!(header, [b'I', b'I', 0x55, 0x00] | [b'M', b'M', 0x00, 0x55]);
    // The in-place TIFF writer grows an ExifIFD or GPS IFD but cannot create
    // one (`tiff_surgical::missing_sub_ifds`).
    let (exif_missing, gps_missing) = if surgical {
        reader
            .read(0, reader.size() as usize)
            .ok()
            .and_then(|bytes| crate::writers::tiff_surgical::missing_sub_ifds(bytes).ok())
            .unwrap_or((false, false))
    } else {
        (false, false)
    };
    drop(reader);
    let dest_baseline = read_metadata(dest)?;
    let file = DestinationFile {
        format,
        tiff_structured: surgical,
        panasonic_raw,
        existing: &dest_baseline,
    };
    let mut report = CopyReport::default();
    // Every tag the copy sets (`SetNewValue` calls), with the source row
    // whose value it takes: (request, source key, occurrence, strict, filter).
    let mut requests: Vec<CopyRequest> = Vec::new();
    let mut sources: Vec<(&str, &TagOccurrence, bool, Option<&str>)> = Vec::new();

    // Selections, grouped by (destination group, name): 13.59 sets each
    // selected source tag in turn, so for one name the tag it sets last --
    // the one it reports for a bare `-NAME` -- is the value that stands.
    let mut picks: Vec<((Option<String>, String), Vec<(&str, &TagOccurrence)>)> = Vec::new();
    for (key, occurrence) in source_metadata.keyed_occurrences() {
        let key_group = key.split_once(':').map_or("", |(group, _)| group);
        let group1 = family1_label(occurrence);
        if is_protected_binary_source(family0_label(occurrence), &occurrence.name)
            || is_pdf_date_alias(occurrence)
        {
            continue;
        }
        for group in selectors.selections(key_group, group1, &occurrence.name) {
            let pick = (group, occurrence.name.to_ascii_lowercase());
            match picks.iter_mut().find(|(existing, _)| *existing == pick) {
                Some((_, rows)) => rows.push((key, occurrence)),
                None => picks.push((pick, vec![(key, occurrence)])),
            }
        }
    }
    let groups: Vec<Option<String>> = picks.iter().map(|((group, _), _)| group.clone()).collect();
    for (((_, name), rows), group) in picks.into_iter().zip(&groups) {
        let Some((source_key, winner)) = arbitrate_rows(rows) else {
            continue;
        };
        if !is_copyable(&name, CopyMode::Selected) {
            continue; // 13.59 sets nothing for it, and says nothing
        }
        report.requested += 1;
        requests.push(CopyRequest {
            name: &winner.name,
            mode: CopyMode::Selected,
            group: group.as_deref(),
            structure: is_structure(winner),
        });
        sources.push((source_key, winner, false, None));
    }

    // Named tags, after the selection: for one destination, the named
    // request is the later, and the one that stands.
    for (filter, source_group, source_name, dest_spec) in &selectors.named {
        let rows: Vec<(&str, &TagOccurrence)> = source_metadata
            .keyed_occurrences()
            .filter(|(key, occurrence)| {
                !is_pdf_date_alias(occurrence)
                    && occurrence.name.eq_ignore_ascii_case(source_name)
                    && source_group.as_deref().is_none_or(|wanted| {
                        let key_group = key.split_once(':').map_or("", |(group, _)| group);
                        copy_group_matches(wanted, key_group, family1_label(occurrence))
                    })
            })
            .collect();
        let Some((source_key, winner)) = arbitrate_rows(rows) else {
            continue; // the source does not carry it
        };
        let (dest_group, dest_name) = match dest_spec.rsplit_once(':') {
            Some((group, name)) => (Some(group), name),
            None => (None, dest_spec.as_str()),
        };
        let dest_group = dest_group.filter(|group| !group.eq_ignore_ascii_case("all"));
        if !is_copyable(dest_name, CopyMode::Named) {
            continue;
        }
        report.requested += 1;
        requests.push(CopyRequest {
            name: dest_name,
            mode: CopyMode::Named,
            group: dest_group,
            structure: is_structure(winner),
        });
        sources.push((source_key, winner, true, Some(filter.as_str())));
    }
    let mut planned: Vec<Planned> = Vec::new();
    for (list, (source_key, occurrence, strict, filter)) in
        destinations(&requests, &file).into_iter().zip(sources)
    {
        for destination in list {
            planned.push(Planned {
                destination,
                source_key,
                occurrence,
                strict,
                filter,
            });
        }
    }

    // Resolve each destination to a key and value oxidex writes, or name it.
    let direct: Vec<(String, String)> = planned
        .iter()
        .filter(|plan| !plan.destination.derived)
        .map(|plan| {
            (
                plan.destination.group1.to_ascii_lowercase(),
                plan.destination.name.to_ascii_lowercase(),
            )
        })
        .collect();
    let mut writes: Vec<(String, TagValue, bool)> = Vec::new();
    let refuse_named = |plan: &Planned, reason: String| -> ExifToolError {
        ExifToolError::tag_not_written(
            plan.filter.unwrap_or(plan.destination.name),
            format!(
                "ExifTool copies it to {}:{}, and {reason}",
                plan.destination.group1, plan.destination.name
            ),
        )
    };
    // `ExifByteOrder` sets the byte order of an EXIF block 13.59 creates; a
    // destination that already carries EXIF keeps its own (a no-op).
    let dest_has_exif = dest_baseline
        .keyed_occurrences()
        .any(|(_, occurrence)| family0_label(occurrence) == "EXIF");
    let creates_exif = planned.iter().any(|plan| plan.destination.group0 == "EXIF");
    for plan in &planned {
        let destination = &plan.destination;
        let key = format!("{}:{}", destination.group1, destination.name);
        if destination.group0 == "File"
            && matches!(destination.name, "ExifByteOrder" | "ExifUnicodeByteOrder")
            && (dest_has_exif || !creates_exif)
        {
            continue;
        }
        if destination.derived {
            if destination.group0 == "Composite"
                || direct.contains(&(
                    destination.group1.to_ascii_lowercase(),
                    destination.name.to_ascii_lowercase(),
                ))
            {
                continue; // not stored, or written directly by its own name
            }
            let reason = format!(
                "13.59 derives its value from the copied {} (WriteAlso or a structure \
                 field), which oxidex does not model",
                plan.occurrence.name
            );
            if plan.strict {
                return Err(refuse_named(plan, reason));
            }
            report.uncopied_tags.push(TagNotWritten::new(key, reason));
            continue;
        }
        let missing_directory = match destination.group1 {
            "GPS" => gps_missing,
            "ExifIFD" | "InteropIFD" => exif_missing,
            _ => false,
        };
        let resolved = if missing_directory {
            Err(ExifToolError::tag_not_written(
                &key,
                format!(
                    "oxidex's TIFF writer cannot create the {} directory this file lacks",
                    destination.group1
                ),
            ))
        } else {
            crate::writers::write_request::ensure_writer_addresses(&key, &key, format, surgical)
                .and_then(|()| copied_value(plan.source_key, plan.occurrence, &key))
        };
        match resolved {
            Ok(value) => {
                writes.retain(|(existing, _, _)| !existing.eq_ignore_ascii_case(&key));
                writes.push((key, value, plan.strict));
            }
            Err(err) if plan.strict => return Err(refuse_named(plan, err.to_string())),
            Err(err) => {
                let reason = match err.tags_not_written().first() {
                    Some(refused) => refused.reason.clone(),
                    None => err.to_string(),
                };
                report.uncopied_tags.push(TagNotWritten::new(
                    key,
                    format!(
                        "13.59 writes it here (where its own conversion accepts the value); \
                         {reason}"
                    ),
                ));
            }
        }
    }
    // One write transaction; a selected tag the writer refuses is skipped
    // and named, a named one refuses the copy.
    while !writes.is_empty() {
        let mut dest_metadata = dest_baseline.clone();
        for (key, value, _) in &writes {
            dest_metadata.insert(key.clone(), value.clone());
        }
        let skippable = |tag: &str| {
            writes
                .iter()
                .any(|(key, _, strict)| !strict && key.eq_ignore_ascii_case(tag))
        };
        match write_metadata_counted(dest, &dest_metadata, &[]) {
            Ok((outcome, proven)) => {
                report.outcome = outcome;
                report.copied = proven;
                break;
            }
            Err(ExifToolError::TagsNotWritten { tags })
                if tags.iter().all(|refused| skippable(&refused.tag)) =>
            {
                writes.retain(|(key, _, _)| !tags.iter().any(|refused| refused.tag == *key));
                report.uncopied_tags.extend(tags);
            }
            Err(ExifToolError::InvalidTagValue { tag_name, reason }) if skippable(&tag_name) => {
                writes.retain(|(key, _, _)| *key != tag_name);
                report
                    .uncopied_tags
                    .push(TagNotWritten::new(tag_name, reason));
            }
            Err(other) => return Err(other),
        }
    }
    // Setting IFD0's resolution (or Compression) makes 13.59 also add the
    // IFD1 %mandatory entry a TIFF's IFD1 lacks (`tiff_surgical::
    // add_ifd1_mandatory_entries`); one oxidex does not set is not added.
    if surgical {
        let has_ifd1 = dest_baseline
            .keyed_occurrences()
            .any(|(_, occurrence)| family1_label(occurrence) == "IFD1");
        let consequences: Vec<TagNotWritten> = report
            .uncopied_tags
            .iter()
            .filter_map(|tag| {
                let name = tag.tag.strip_prefix("IFD0:")?;
                matches!(
                    name,
                    "Compression" | "XResolution" | "YResolution" | "ResolutionUnit"
                )
                .then_some(name)
            })
            .filter(|name| {
                has_ifd1
                    && !dest_baseline.keyed_occurrences().any(|(_, occurrence)| {
                        family1_label(occurrence) == "IFD1" && &*occurrence.name == *name
                    })
            })
            .map(|name| {
                TagNotWritten::new(
                    format!("IFD1:{name}"),
                    format!(
                        "13.59 adds IFD1's mandatory {name} when it sets IFD0's, which oxidex \
                         did not"
                    ),
                )
            })
            .collect();
        report.uncopied_tags.extend(consequences);
    }
    let final_make = writes
        .iter()
        .find(|(key, _, _)| key.eq_ignore_ascii_case("IFD0:Make"))
        .map(|(key, value, _)| printed_text(value, key))
        .or_else(|| printed_make(&dest_baseline));
    for (group1, occurrence) in
        maker_note_rows(source_metadata, selectors, format, surgical, final_make)
    {
        report.uncopied_tags.push(TagNotWritten::new(
            format!("{group1}:{}", occurrence.name),
            "13.59 copies the source's maker note block whole; oxidex does not copy a \
             maker note block",
        ));
    }

    let mut seen = std::collections::HashSet::new();
    report
        .uncopied_tags
        .retain(|tag| seen.insert(tag.tag.to_ascii_lowercase()));
    for tag in &report.uncopied_tags {
        let group = tag.tag.split_once(':').map_or("", |(group, _)| group);
        if !report.uncopied_groups.iter().any(|known| known == group) {
            report.uncopied_groups.push(group.to_string());
        }
    }
    report.uncopied_groups.sort();
    Ok(report)
}
