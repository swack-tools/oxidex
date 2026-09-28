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
use crate::core::read_options::ReadOptions;
use crate::core::tag_occurrence::{Instance, TagOccurrence, ValueChannel};
use crate::core::{MetadataMap, TagValue};
use crate::writers::generated_tag_exists::SHORTCUTS;
use std::collections::{HashMap, HashSet};

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

/// The family-0 label to show for `-Gn` display: [`resolve_family0`]
/// applied to `group0`, whether or not a call site set a real `group1`.
///
/// `insert_occurrence` callers pass a real family-0 `group0` (`File`,
/// `EXIF`, `ID3`, `MakerNotes`), which `resolve_family0` returns unchanged:
/// none of its arms maps a real family-0 group. Callers that keep a
/// family-1-flavored storage key and record the family-1 group beside it --
/// `Nikon:AdvancedRaw` under `NikonCapture`, `Canon:FNumber` under `Canon`,
/// `XMP-exif:FNumber` under `XMP-exif` -- still need the mapping: ExifTool
/// prints `[MakerNotes:NikonCapture]` and `[MakerNotes:Canon]` under
/// `-G0:1` (NikonCapture.pm:43 and Canon.pm `GROUPS => { 0 => 'MakerNotes'
/// }`), never `[Nikon:NikonCapture]` or `[Canon:Canon]`.
pub fn family0_label(occurrence: &TagOccurrence) -> &str {
    resolve_family0(&occurrence.group0)
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
///
/// A multi-family request is *simplified* the way `GetGroup` does it
/// (`ExifTool.pm` 13.59, the `$simplify` branch at the end of `GetGroup`):
/// an empty family name is dropped, a name identical to the one before it is
/// dropped, and a leading `Main` is dropped when anything follows it. So
/// `-G0:1` labels `File:FileType` `[File]`, not `[File:File]`, and `-G0:1:4`
/// labels a first-copy tag `MakerNotes:Olympus`, not `MakerNotes:Olympus:`
/// -- both confirmed against the pinned oracle. A single-family request
/// (`-G1`) is not simplified and is returned as-is, even when empty.
pub fn joined_family_label(occurrence: &TagOccurrence, families: &[u8]) -> String {
    if let [family] = families {
        return family_label(occurrence, *family);
    }
    let mut labels: Vec<String> = Vec::with_capacity(families.len());
    for &family in families {
        let label = family_label(occurrence, family);
        if label.is_empty() || labels.last() == Some(&label) {
            continue;
        }
        labels.push(label);
    }
    if labels.len() > 1 && labels[0] == "Main" {
        labels.remove(0);
    }
    labels.join(":")
}

/// Splits a requested token (`"Make"`, `"EXIF:Make"`, `"XMP-dc:Subject"`)
/// into an optional group qualifier and the short tag name, on the *last*
/// colon -- single-colon requests are the overwhelming case, and this still
/// isolates the tag name correctly for the rare multi-colon XMP-family
/// names.
pub(crate) fn split_request(token: &str) -> (Option<&str>, &str) {
    match token.rsplit_once(':') {
        Some((qualifier, short_name)) => (Some(qualifier), short_name),
        None => (None, token),
    }
}

/// Whether `occurrence` satisfies a request's group qualifier: matched
/// against its family-1 label (its own `group0`/`group1`, whichever is
/// real -- i.e. a request like `-IFD0:Make` or `-CIFF:Make`), or its
/// resolved family-0 label (`-EXIF:Make`, `-MakerNotes:Make`).
pub(crate) fn occurrence_matches_qualifier(occurrence: &TagOccurrence, qualifier: &str) -> bool {
    qualifier.eq_ignore_ascii_case(family1_label(occurrence))
        || qualifier.eq_ignore_ascii_case(family0_label(occurrence))
}

/// One occurrence chosen for CLI display, together with the literal key it
/// should render under.
pub struct ResolvedOccurrence<'a> {
    pub occurrence: &'a TagOccurrence,
    /// The literal public key retained by `MetadataMap`, independent of the
    /// occurrence's canonical source identity and true family groups.
    pub lookup_key: String,
    /// FoundTag copy number for selected rows. Full-list projections did not
    /// pass through `exiftool_keys` and retain their existing ordering rule.
    pub copy: Option<u32>,
}

/// The occurrences `token` names: every active occurrence whose short name
/// matches and, when `token` carries a group qualifier, whose family-0 or
/// family-1 label matches it -- in file order.
///
/// Reads only each occurrence's interned `name`/`group0`/`group1`, so it
/// allocates nothing. It used to walk
/// [`MetadataMap::all_occurrences`], which builds a `"{group0}:{name}"` key
/// per occurrence that this filter never read; the Composite layer pays this
/// walk per dependency per pass, and #821 measured that as the bulk of a
/// read's heap traffic.
fn matching_occurrences<'a, 'm>(
    metadata: &'m MetadataMap,
    token: &'a str,
) -> impl Iterator<Item = &'m TagOccurrence> {
    let (qualifier, short_name) = split_request(token);
    metadata
        .occurrences()
        .filter(move |occurrence| occurrence.name.eq_ignore_ascii_case(short_name))
        .filter(move |occurrence| {
            qualifier.is_none_or(|q| occurrence_matches_qualifier(occurrence, q))
        })
}

/// [`matching_occurrences`] with the retained literal public key alongside
/// each canonical occurrence.
///
/// `pub(crate)`, not `pub`: `core::operations`'s tests use it (and
/// [`arbitrate_keyed`]) to check bare-name arbitration directly against a
/// crafted `MetadataMap`, the same algorithm a bare CLI request runs through
/// [`resolve_requested_tags`], without reaching for a full CLI round-trip.
pub(crate) fn matching_keyed_occurrences<'a, 'm>(
    metadata: &'m MetadataMap,
    token: &'a str,
) -> impl Iterator<Item = (&'m str, &'m TagOccurrence)> {
    let (qualifier, short_name) = split_request(token);
    metadata
        .keyed_occurrences()
        .filter(move |(_, occurrence)| occurrence.name.eq_ignore_ascii_case(short_name))
        .filter(move |(_, occurrence)| {
            qualifier.is_none_or(|q| occurrence_matches_qualifier(occurrence, q))
        })
}

pub(crate) fn arbitrate_keyed<'m>(
    candidates: impl Iterator<Item = (&'m str, &'m TagOccurrence)>,
) -> Option<(&'m str, &'m TagOccurrence)> {
    let mut remaining = candidates;
    let mut winner = remaining.next()?;
    for candidate in remaining {
        let effective_old_priority = if winner.1.priority == 0 {
            1
        } else {
            winner.1.priority
        };
        let instance_ok = candidate.1.instance == Instance::default()
            || candidate.1.instance == winner.1.instance;
        if candidate.1.priority >= effective_old_priority && instance_ok {
            winner = candidate;
        }
    }
    Some(winner)
}

/// The single occurrence a bare or group-qualified `token` resolves to under
/// ExifTool's own arbitration ([`arbitrate`]), or `None` when nothing
/// matches. This is [`resolve_requested_tags`]'s non-`-a` answer for one
/// token, without allocating: no request vector, no match vector, no lookup
/// key.
pub fn resolve_requested_tag<'m>(
    metadata: &'m MetadataMap,
    token: &str,
) -> Option<&'m TagOccurrence> {
    arbitrate(matching_occurrences(metadata, token))
}

/// Picks the winner among one request's `candidates`, which must arrive in
/// file order (ascending `order`).
///
/// `FoundTag`'s own tie rule (`ExifTool.pm:9541-9564`), replicated here
/// exactly as `TagSink::record` applies it incrementally rather than as a
/// flat `max_by_key((priority, order))`: fold the matches in file order, and
/// an arrival displaces the running winner only when `new.priority >=
/// effective_old_priority` AND the instance guard below allows it, where a
/// running winner whose own priority is `0` is promoted to `1` for that
/// comparison (`ExifTool.pm:9541-9551`, "promote existing 0-priority tag so
/// it takes precedence over a new 0-tag"). A flat `max_by_key` gets
/// `Priority => 0` families wrong: among several 0-priority arrivals it
/// picks the one with the largest `order` (the LAST), the opposite of the
/// FIRST-wins default JPEG COM's `Comment` and JUMBF's
/// `JUMDType`/`JUMDLabel` both need and that `TagSink::record`'s own winner
/// projection already gives `-j`'s default (non-`-TAG`) output path -- this
/// bug was invisible for `Comment` (an explicit `-Comment` request silently
/// returned the wrong one of two occurrences) until the Stage 4
/// duplicate-loss scan (`tools/exiftool-tables/duplicate_loss_scan.py`)
/// started retaining JUMBF's occurrences too and made the same
/// mis-resolution visible there.
///
/// Rule 2, the DOC_NUM/`Instance` guard, rides along in the same fold: an
/// occurrence recorded under a non-default sub-document/track `Instance`
/// never displaces a winner recorded under a *different* `Instance`,
/// regardless of priority (`ExifTool.pm:9564`'s `(not $$self{DOC_NUM} or
/// ...)`). Without it, `-TrackID` against `CanonRaw.cr3` returns Track4's
/// `4` here (largest `order` among four equal-priority ties) where the
/// correct answer, matching `TagSink::record`'s own winner projection (what
/// default non-`-TAG` `-j` output uses) and the pinned oracle's `-TrackID`
/// default, is Track1's `1`.
///
/// Both callers feed this in [`MetadataMap::occurrences`]' own iteration
/// order: every occurrence's `order` is the sink's `next_order()` at record
/// time (see [`TagOccurrence::order`]), so that iteration is already sorted
/// by `order` and folding it directly is the same fold as sorting first.
pub(crate) fn arbitrate<'m>(
    candidates: impl Iterator<Item = &'m TagOccurrence>,
) -> Option<&'m TagOccurrence> {
    let mut remaining = candidates;
    let mut winner = remaining.next()?;
    for candidate in remaining {
        debug_assert!(
            candidate.order > winner.order,
            "candidates must arrive in ascending `order`"
        );
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
    Some(winner)
}

/// Resolves every requested token against every occurrence ever recorded in
/// `metadata` (not just each literal key's own winner), applying group
/// qualifiers and, unless `all_occurrences`, ExifTool's own priority/order
/// arbitration ([`resolve_requested_tag`]) to pick exactly one per request.
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
        if all_occurrences {
            let mut matches: Vec<(&str, &TagOccurrence)> =
                matching_keyed_occurrences(metadata, token).collect();
            matches.sort_by_key(|(_, occurrence)| occurrence.order);
            out.extend(
                matches
                    .into_iter()
                    .map(|(key, occurrence)| ResolvedOccurrence {
                        lookup_key: key.to_string(),
                        occurrence,
                        copy: None,
                    }),
            );
        } else if let Some((key, winner)) =
            arbitrate_keyed(matching_keyed_occurrences(metadata, token))
        {
            out.push(ResolvedOccurrence {
                lookup_key: key.to_string(),
                occurrence: winner,
                copy: None,
            });
        }
    }
    out
}

/// ExifTool's family-2 group names (`ExifTool.pod`, `GetGroup`, "Family 2
/// (Category)"), plus the family-3/4/6/8 names below. OxiDex does not carry
/// these families for every tag (`TagOccurrence::group2` is only partly
/// populated; families 3-8 are not modeled), so a selector naming one is
/// refused rather than silently matching fewer tags than ExifTool's
/// `GroupMatches` (`ExifTool.pm` 13.59), which checks a bare group name
/// against *every* family.
const UNMODELED_FAMILY2_GROUPS: &[&str] = &[
    "audio", "author", "camera", "device", "document", "exiftool", "image", "location", "other",
    "preview", "printing", "time", "unknown", "video",
];

/// ExifTool's family-6 names (EXIF/TIFF formats, `ExifTool.pod` "Family 6").
const UNMODELED_FAMILY6_GROUPS: &[&str] = &[
    "int8u",
    "string",
    "int16u",
    "int32u",
    "rational64u",
    "int8s",
    "undef",
    "int16s",
    "int32s",
    "rational64s",
    "float",
    "double",
    "ifd",
    "unicode",
    "complex",
    "int64u",
    "int64s",
    "ifd64",
];

/// A bare hyphenated group can be ExifTool's family 5 metadata path:
/// `MetadataPath` joins every directory in `PATH` with `-`, and
/// `GroupMatches` checks bare groups against all families. These are the
/// actual hyphenated family 1 names from pinned 13.59 `-listg1`; its family 0
/// inventory has no hyphenated names. Preserve these 0/1 selectors, and
/// refuse all other bare hyphenated groups until OxiDex records family 5.
const FAMILY1_HYPHENATED_GROUPS: &str = include_str!("family1_hyphenated_groups_1359.txt");

fn is_known_hyphenated_family1_group(name: &str) -> bool {
    FAMILY1_HYPHENATED_GROUPS
        .lines()
        .any(|line| !line.starts_with('#') && line == name)
}

/// One `:`-separated part of a selector's group qualifier, as
/// `GroupMatches` reads it: an optional leading family number and a
/// case-insensitive name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GroupPart {
    /// `Some(0)`/`Some(1)` for `0EXIF`/`1IFD0`; `None` for a bare name, which
    /// matches either family.
    family: Option<u8>,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupSelector {
    /// No qualifier (`-ISO`).
    None,
    /// `*:` or `all:` -- every group (`$allGrp` in `SetFoundTags`).
    All,
    /// Every part must match (`-EXIF:IFD0:Make`).
    Parts(Vec<GroupPart>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TagPattern {
    /// `all` or `*`.
    All,
    /// A name containing `*` or `?` wildcards.
    Glob(String),
    /// A plain tag name.
    Name(String),
}

/// One `-TAG` request or `--TAG` exclusion, parsed the way `SetFoundTags`
/// (`ExifTool.pm` 13.59) parses it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TagSelector {
    group: GroupSelector,
    tag: TagPattern,
}

/// The `-TAG` requests and `--TAG` exclusions of one command line, validated.
///
/// Everything here models `SetFoundTags` (`ExifTool.pm` 13.59), which is what
/// decides the row set and order for a requested tag list:
///
/// * `-GROUP:all`, `-GROUP:*`, `-all`, `-*` and wildcard names (`-Exp*`)
///   select every matching tag in file order. Without `-a` they select only
///   each tag name's priority winner, so `-EXIF:all` omits `WhiteBalance`
///   when the Canon maker note's copy wins it.
/// * `-all:TAG` / `-*:TAG` keep every copy of `TAG`, with or without `-a`.
/// * Any `--TAG` exclusion puts `SetFoundTags` in duplicate mode (`$doDups
///   = $duplicates || $exclude`): every copy is matched, the exclusions are
///   removed, and then -- unless `-a` -- each tag name is reduced to one
///   survivor: the priority winner if it is still in the list, otherwise its
///   most recently displaced copy. So `-EXIF:all --ISO` shows the EXIF copy
///   of `WhiteBalance` that plain `-EXIF:all` hides.
///
/// A selector OxiDex cannot resolve exactly is refused, by name, by
/// [`TagSelection::parse`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TagSelection {
    requests: Vec<TagSelector>,
    exclusions: Vec<TagSelector>,
}

fn is_tag_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Parses one group-qualifier part; `Ok(None)` for an `all` part, which
/// `GroupMatches` skips, so
/// `-EXIF:all:all` is `-EXIF:all`. An all-only group on a named tag,
/// such as `-1all:Make`, uses ordinary winner arbitration; only a direct
/// `-all:Make` requests every copy. Although `GroupMatches` also skips `*`,
/// `SetFoundTags` first rejects it inside a multipart group such as
/// `EXIF:*:Make` (`Invalid group name 'EXIF:*'` in pinned 13.59).
fn parse_group_part(selector: &str, part: &str) -> Result<Option<GroupPart>, String> {
    let digits = part.chars().take_while(char::is_ascii_digit).count();
    let (family, name) = part.split_at(digits);
    let refuse = |why: &str| Err(format!("unsupported tag selector {selector}: {why}"));
    if name.eq_ignore_ascii_case("all") {
        return Ok(None);
    }
    // `get`, never a byte slice: a multibyte name (`-éé:all`) must be
    // refused below, not panic on a non-boundary index.
    if name
        .get(..3)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("id-"))
    {
        return refuse("family-7 (tag ID) groups are not supported");
    }
    if name.is_empty() {
        return refuse("empty group name");
    }
    if !name.chars().all(is_tag_char) {
        return refuse("invalid group name");
    }
    let family = if family.is_empty() {
        None
    } else {
        match family.parse::<u32>() {
            Ok(0) => Some(0),
            Ok(1) => Some(1),
            _ => return refuse("only family 0 and 1 group names are supported"),
        }
    };
    let lower = name.to_ascii_lowercase();
    if family.is_none() {
        if lower.contains('-') && !is_known_hyphenated_family1_group(&lower) {
            return refuse(&format!(
                "'{name}' may be an ExifTool family 5 metadata path, which OxiDex does not resolve"
            ));
        }
        let numbered = |prefix: &str| {
            lower
                .strip_prefix(prefix)
                .is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
        };
        let doc = lower.strip_prefix("doc").is_some_and(|rest| {
            !rest.is_empty()
                && rest
                    .split('-')
                    .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        });
        if UNMODELED_FAMILY2_GROUPS.contains(&lower.as_str())
            || UNMODELED_FAMILY6_GROUPS.contains(&lower.as_str())
            || lower == "main"
            || doc
            || numbered("copy")
            || numbered("file")
        {
            return refuse(&format!(
                "'{name}' is an ExifTool family 2-8 group name, and OxiDex only resolves \
                 family 0 and 1 groups"
            ));
        }
    }
    Ok(Some(GroupPart {
        family,
        name: lower,
    }))
}

/// Parses one selector (without its leading dashes). `exclusion` selects
/// the `Exclude` option's reading: a trailing `#` is dropped
/// (`ExpandShortcuts($exclude, 1)`), and `all:`/`*:` means "no group".
fn parse_selector(token: &str, exclusion: bool) -> Result<TagSelector, String> {
    let shown = if exclusion {
        format!("--{token}")
    } else {
        format!("-{token}")
    };
    let refuse = |why: &str| Err(format!("unsupported tag selector {shown}: {why}"));
    let mut body = token;
    if let Some(stripped) = body.strip_suffix('#') {
        if !exclusion {
            return refuse(
                "the '#' (ValueConv) suffix is not supported; use --no-print-conv instead",
            );
        }
        body = stripped;
    }
    // `SetFoundTags`' own split, `/^(.*):(.+)/`: the last colon with at
    // least one character after it.
    let (group, tag) = match body.rfind(':') {
        Some(index) if index + 1 < body.len() => (Some(&body[..index]), &body[index + 1..]),
        Some(_) => return refuse("empty tag name"),
        None => (None, body),
    };
    if tag.is_empty() || !tag.chars().all(|c| is_tag_char(c) || c == '*' || c == '?') {
        return refuse("invalid tag name");
    }
    if SHORTCUTS
        .iter()
        .any(|(shortcut, _)| shortcut.eq_ignore_ascii_case(tag))
    {
        return refuse(&format!(
            "'{tag}' is an ExifTool shortcut, which OxiDex does not expand"
        ));
    }
    let group = match group {
        None => GroupSelector::None,
        Some(g) if g == "*" || g.eq_ignore_ascii_case("all") => {
            if exclusion {
                GroupSelector::None
            } else {
                GroupSelector::All
            }
        }
        Some(g) => GroupSelector::Parts(
            g.split(':')
                .map(|part| parse_group_part(&shown, part))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect(),
        ),
    };
    let tag = if tag == "*" || tag.eq_ignore_ascii_case("all") {
        TagPattern::All
    } else if tag.contains(['*', '?']) {
        TagPattern::Glob(tag.to_ascii_lowercase())
    } else {
        TagPattern::Name(tag.to_string())
    };
    Ok(TagSelector { group, tag })
}

impl TagSelection {
    /// Parses and validates `requests` (`-TAG`) and `exclusions` (`--TAG`),
    /// each given without its dashes, refusing by name any selector OxiDex
    /// cannot resolve exactly as pinned ExifTool 13.59 does:
    ///
    /// * a group name from an unmodeled family -- family 2 (`Camera`,
    ///   `Time`, ...), 3 (`Main`, `Doc1`), 4 (`Copy1`), 6 (`int16u`), 7
    ///   (`ID-271`), 8 (`File1`), or a family number other than 0 and 1;
    /// * an ExifTool shortcut (`AllDates`, `Common`, ...), which ExifTool
    ///   expands and OxiDex does not;
    /// * a `#` (ValueConv) suffix on a request;
    /// * a malformed tag or group name.
    pub fn parse(requests: &[String], exclusions: &[String]) -> Result<Self, String> {
        // `exiftool`:1815: a read whose exclusions include `*` or `*:*`
        // (`--*`, `-x all`) stops before reading anything. A literal
        // `--all` is allowed and simply excludes every row.
        if exclusions
            .iter()
            .any(|token| token == "*" || token == "*:*")
        {
            return Err("All tags excluded -- nothing to do.".to_string());
        }
        for token in exclusions {
            // `ExifTool.pm` 13.59 `ParseArguments`: an exclusion matching
            // `/(xmp-.*:[-\w]+)#?/i` also enters `EXCL_XMP_LOOKUP`, which
            // makes the XMP reader skip that property -- and everything
            // nested in it -- by *property* name, with `XMP-all` standing for
            // every namespace. OxiDex filters rows after reading and cannot
            // reproduce that, so it refuses the selector instead of keeping
            // rows ExifTool drops.
            let lower = token.to_ascii_lowercase();
            if lower
                .rfind(':')
                .is_some_and(|colon| lower[..colon].contains("xmp-"))
            {
                return Err(format!(
                    "unsupported tag selector --{token}: an XMP-namespace exclusion also skips \
                     XMP properties while reading, which OxiDex does not model"
                ));
            }
        }
        Ok(Self {
            requests: requests
                .iter()
                .map(|token| parse_selector(token, false))
                .collect::<Result<_, _>>()?,
            exclusions: exclusions
                .iter()
                .map(|token| parse_selector(token, true))
                .collect::<Result<_, _>>()?,
        })
    }

    /// The selection `args` asks for, or `None` for the plain full listing
    /// (no `-TAG` request and no `--TAG` exclusion).
    pub fn from_args(args: &CliArgs) -> Result<Option<Self>, String> {
        // `main` dispatches these modes before any read, so their arguments
        // (`-FileName<IFD0:ModifyDate`, `-AllDates+=1`, `-all=`) are never
        // tag selections.
        if args.filename_pattern().is_some()
            || args.is_clear_all_metadata()
            || !args.date_shift_operations().is_empty()
        {
            return Ok(None);
        }
        let requests = args.specific_tags().unwrap_or_default();
        let exclusions = args.excluded_tags();
        if requests.is_empty() && exclusions.is_empty() {
            return Ok(None);
        }
        Self::parse(&requests, &exclusions).map(Some)
    }

    /// The plain requested names (no wildcard), for [`ReadOptions`]'
    /// request awareness: an explicitly named tag is shown even when it is
    /// in OxiDex's extended-only namespace, as it always was.
    fn plain_request_names(&self) -> Vec<String> {
        self.requests
            .iter()
            .filter_map(|selector| match &selector.tag {
                TagPattern::Name(name) => Some(name.clone()),
                _ => None,
            })
            .collect()
    }
}

/// `GroupMatches` for the families OxiDex models: every part must name the
/// occurrence's family-0 or family-1 group (or the one family its number
/// selects).
fn group_parts_match(parts: &[GroupPart], occurrence: &TagOccurrence) -> bool {
    parts.iter().all(|part| {
        let family0 = || part.name.eq_ignore_ascii_case(family0_label(occurrence));
        let family1 = || part.name.eq_ignore_ascii_case(family1_label(occurrence));
        match part.family {
            Some(0) => family0(),
            Some(_) => family1(),
            None => family0() || family1(),
        }
    })
}

/// A `*`/`?` wildcard match over a tag name, case-insensitively, as
/// `SetFoundTags` builds it (`*` -> `[-\w]*`, `?` -> `[-\w]`).
///
/// Bottom-up over (pattern position, name position), so repeated stars
/// cost `O(pattern x name)` rather than backtracking exponentially
/// (`-*********Z` against every tag of a file).
fn glob_matches(pattern: &[u8], name: &[u8]) -> bool {
    let is_tag_byte = |c: u8| is_tag_char(c as char);
    // `next[j]`: does `pattern[i + 1..]` match `name[j..]`?
    let mut next: Vec<bool> = (0..=name.len()).map(|j| j == name.len()).collect();
    for &p in pattern.iter().rev() {
        let mut row = vec![false; name.len() + 1];
        for j in (0..=name.len()).rev() {
            row[j] = match p {
                b'*' => next[j] || (j < name.len() && is_tag_byte(name[j]) && row[j + 1]),
                b'?' => j < name.len() && is_tag_byte(name[j]) && next[j + 1],
                _ => j < name.len() && name[j].eq_ignore_ascii_case(&p) && next[j + 1],
            };
        }
        next = row;
    }
    next[0]
}

fn name_matches(pattern: &TagPattern, name: &str) -> bool {
    match pattern {
        TagPattern::All => true,
        TagPattern::Glob(glob) => glob_matches(glob.as_bytes(), name.as_bytes()),
        TagPattern::Name(wanted) => wanted.eq_ignore_ascii_case(name),
    }
}

/// One of ExifTool's tag keys (`$$self{VALUE}`) as `FoundTag` would have
/// named it: the tag name, and `copy` 0 for the priority winner (the bare
/// key, `ISO`) or the `N` of `ISO (N)` for a displaced or losing copy.
struct EtKey<'m> {
    lookup_key: &'m str,
    occurrence: &'m TagOccurrence,
    copy: u32,
}

/// Replays `FoundTag`'s duplicate-key numbering (`ExifTool.pm` 13.59,
/// "handle duplicate tag names") over `metadata`'s occurrences in file
/// order, keeping only those `visible` admits. Each arrival either
/// displaces the current winner -- by the same rule [`arbitrate`] folds,
/// so the copy-0 key of each name is exactly that fold's winner -- which
/// moves the old winner to the next `Tag (N)` key, or itself takes the next
/// `Tag (N)`.
fn exiftool_keys<'m>(
    metadata: &'m MetadataMap,
    mut visible: impl FnMut(&str) -> bool,
) -> Vec<EtKey<'m>> {
    let mut keys: Vec<EtKey<'m>> = Vec::new();
    // name -> (index of the current copy-0 key, next copy number)
    let mut by_name: HashMap<&'m str, (usize, u32)> = HashMap::new();
    for (lookup_key, occurrence) in metadata.keyed_occurrences() {
        if !visible(lookup_key) {
            continue;
        }
        let index = keys.len();
        match by_name.get_mut(occurrence.name.as_ref()) {
            None => {
                keys.push(EtKey {
                    lookup_key,
                    occurrence,
                    copy: 0,
                });
                by_name.insert(occurrence.name.as_ref(), (index, 1));
            }
            Some((winner, next)) => {
                let old = keys[*winner].occurrence;
                let effective_old_priority = if old.priority == 0 { 1 } else { old.priority };
                let instance_ok = occurrence.instance == Instance::default()
                    || occurrence.instance == old.instance;
                let copy = if occurrence.priority >= effective_old_priority && instance_ok {
                    keys[*winner].copy = *next;
                    *winner = index;
                    0
                } else {
                    *next
                };
                *next += 1;
                keys.push(EtKey {
                    lookup_key,
                    occurrence,
                    copy,
                });
            }
        }
    }
    keys
}

/// `SetFoundTags` (`ExifTool.pm` 13.59) over OxiDex's occurrences: the rows
/// `selection` names, in the order ExifTool returns them. See
/// [`TagSelection`] for the rules. `visible` admits the literal keys the
/// display may show at all ([`ReadOptions::shows_key`]); `group_sort` is the `-G` family list when ExifTool sorts the
/// result by group (a full listing -- no `-TAG` request -- under `-G`).
fn select_occurrences<'m>(
    metadata: &'m MetadataMap,
    selection: &TagSelection,
    duplicates: bool,
    visible: impl FnMut(&str) -> bool,
    group_sort: Option<&[u8]>,
) -> Vec<ResolvedOccurrence<'m>> {
    let keys = exiftool_keys(metadata, visible);
    let do_dups = duplicates || !selection.exclusions.is_empty();

    // Indices into `keys`, in ExifTool's return order.
    let mut found: Vec<usize> = Vec::new();
    if selection.requests.is_empty() {
        found.extend((0..keys.len()).filter(|&i| do_dups || keys[i].copy == 0));
    }
    for request in &selection.requests {
        let all_group = request.group == GroupSelector::All;
        let wildcard = !matches!(request.tag, TagPattern::Name(_));
        let mut matches: Vec<usize> = (0..keys.len())
            .filter(|&i| name_matches(&request.tag, &keys[i].occurrence.name))
            .filter(|&i| {
                // A wildcard without `-a`/`all:` sees only the bare keys; a
                // plain name without a group, likewise only its winner
                // (`$$tagHash{$reqTag}`).
                do_dups
                    || all_group
                    || keys[i].copy == 0
                    || (!wildcard && request.group != GroupSelector::None)
            })
            .collect();
        if let GroupSelector::Parts(parts) = &request.group {
            matches.retain(|&i| group_parts_match(parts, keys[i].occurrence));
        }
        matches.sort_by_key(|&i| keys[i].occurrence.order);
        if matches.len() > 1 && !(do_dups || wildcard || all_group) {
            // A group-qualified plain name without `-a`: one winner among the
            // group's copies, by the same fold every other `-TAG` uses.
            let winner = arbitrate_keyed(
                matches
                    .iter()
                    .map(|&i| (keys[i].lookup_key, keys[i].occurrence)),
            )
            .map(|(_, occurrence)| occurrence);
            matches.retain(|&i| winner.is_some_and(|w| std::ptr::eq(w, keys[i].occurrence)));
        }
        found.extend(matches);
    }

    if !selection.exclusions.is_empty() {
        let excluded: HashSet<usize> = found
            .iter()
            .copied()
            .filter(|&i| {
                selection.exclusions.iter().any(|exclusion| {
                    name_matches(&exclusion.tag, &keys[i].occurrence.name)
                        && match &exclusion.group {
                            GroupSelector::Parts(parts) => {
                                group_parts_match(parts, keys[i].occurrence)
                            }
                            _ => true,
                        }
                })
            })
            .collect();
        found.retain(|i| !excluded.contains(i));
        if !duplicates {
            // One survivor per tag name: the bare (winning) key if it is
            // still listed, else the highest-numbered copy.
            let mut best: HashMap<&str, usize> = HashMap::new();
            for &i in &found {
                let name = keys[i].occurrence.name.as_ref();
                let keep = match best.get(name) {
                    None => true,
                    Some(&b) => {
                        keys[i].copy == 0 || (keys[b].copy != 0 && keys[b].copy <= keys[i].copy)
                    }
                };
                if keep {
                    best.insert(name, i);
                }
            }
            let kept: HashSet<usize> = best.into_values().collect();
            found.retain(|i| kept.contains(i));
        }
    }

    if selection.requests.is_empty() {
        found.sort_by_key(|&i| keys[i].occurrence.order);
        if let Some(families) = group_sort {
            // `GetTagList`'s `Group` sort: groups ranked by first appearance
            // in file order, file order within a group.
            let mut rank: HashMap<String, usize> = HashMap::new();
            let ranks: Vec<usize> = found
                .iter()
                .map(|&i| {
                    let label = joined_family_label(keys[i].occurrence, families);
                    let next = rank.len();
                    *rank.entry(label).or_insert(next)
                })
                .collect();
            let mut ranked: Vec<(usize, usize)> = ranks.into_iter().zip(found).collect();
            ranked.sort_by_key(|&(rank, _)| rank);
            found = ranked.into_iter().map(|(_, i)| i).collect();
        }
    }

    found
        .into_iter()
        .map(|i| ResolvedOccurrence {
            occurrence: keys[i].occurrence,
            lookup_key: keys[i].lookup_key.to_string(),
            copy: Some(keys[i].copy),
        })
        .collect()
}

/// Drops repeated rows for the same occurrence, keeping the first: ExifTool's
/// JSON and CSV writers print a tag key once however many requests named it
/// (`-j -ISO -ISO` has one `ISO`), while its text writers repeat it.
fn dedupe_repeated_rows(resolved: Vec<ResolvedOccurrence<'_>>) -> Vec<ResolvedOccurrence<'_>> {
    let mut seen: HashSet<*const TagOccurrence> = HashSet::new();
    resolved
        .into_iter()
        .filter(|entry| seen.insert(std::ptr::from_ref(entry.occurrence)))
        .collect()
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
        occurrence.project(ValueChannel::ValueConv).into_owned()
    } else {
        resolved_print_value(occurrence)
    }
}

/// Projects a tag for normal output while preserving the compatibility
/// formatter used by legacy parser call sites.
///
/// Most migrated producers attach an explicit `print` form, which is the
/// canonical typed projection. The remaining `insert()` shim producers keep
/// a typed `raw` value with no print form; those rows historically went
/// through the name-keyed ExifTool compatibility rules (GPS reference labels,
/// APP14 flags, Ducky quality, and similar conversions). Apply those rules to
/// the typed ValueConv result only in that legacy case. This avoids reparsing
/// display strings while retaining the public output contract during the
/// migration.
fn resolved_print_value(occurrence: &TagOccurrence) -> TagValue {
    if occurrence.print.is_some() || occurrence.value.is_some() {
        occurrence.project(ValueChannel::PrintConv).into_owned()
    } else {
        let value = occurrence.project(ValueChannel::ValueConv);
        // APEX is the one legacy shim conversion whose ValueConv changes the
        // value's type/meaning. `format_tag_value_rules` expects the stored
        // APEX input for its PrintConv arm; passing this already-converted
        // float back through it would apply ValueConv a second time
        // (`14.0` -> `128.0`). Start from the stored input so both stages run
        // once and normal output keeps aperture rounding and shutter labels.
        if crate::core::exiftool_compat::apex_value_conv(&occurrence.name, &occurrence.raw)
            .is_some()
        {
            return crate::core::exiftool_compat::format_tag_value_rules(
                &occurrence.lookup_key(),
                &occurrence.raw,
            );
        }
        crate::core::exiftool_compat::format_tag_value_rules(
            &occurrence.lookup_key(),
            value.as_ref(),
        )
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
    let base_keys: Vec<String> = resolved
        .iter()
        .map(|entry| match group_display {
            Some(families) => {
                let label = joined_family_label(entry.occurrence, families);
                if bracketed {
                    format!("[{label}] {}", entry.occurrence.name)
                } else {
                    format!("{label}:{}", entry.occurrence.name)
                }
            }
            None => entry.lookup_key.clone(),
        })
        .collect();
    // A later priority winner may displace an earlier occurrence with the
    // same literal key. Reserve that winner's bare display key before naming
    // losing copies, independent of file-order output.
    let mut bare_winners: HashMap<&str, usize> = HashMap::new();
    for (index, base_key) in base_keys.iter().enumerate() {
        if !bare_winners.contains_key(base_key.as_str()) || resolved[index].copy == Some(0) {
            bare_winners.insert(base_key, index);
        }
    }
    let mut reserved: HashSet<&str> = HashSet::new();
    for (index, entry) in resolved.iter().enumerate() {
        let base_key = &base_keys[index];
        let key = if bare_winners[base_key.as_str()] == index {
            base_key.clone()
        } else {
            let mut n = 2u32;
            loop {
                let candidate = format!("{base_key} ({n})");
                if !out.contains_key(&candidate)
                    && !reserved.contains(candidate.as_str())
                    && !bare_winners.contains_key(candidate.as_str())
                {
                    break candidate;
                }
                n += 1;
            }
        };
        reserved.insert(base_key);
        let value = resolved_display_value(entry.occurrence, no_print_conv);
        out.insert(key, value);
    }
    // A display projection of rows the read produced, never a caller's
    // assignment.
    out.mark_read_complete();
    out
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
/// (`format_tag_value`) so enum/GPS/binary rendering stays identical to
/// every other output path; only the line shape (`"[label] name: value\n"`)
/// and the ordering are specific to this function.
///
/// This is the default (level 0) layout only. ExifTool's level 0 prints tag
/// *descriptions* padded to 32 columns, which needs per-table description
/// text OxiDex does not carry, so it is left as it was; the short levels go
/// through [`render_short_lines`].
pub fn render_group_display_lines(
    resolved: &[ResolvedOccurrence<'_>],
    families: &[u8],
    no_print_conv: bool,
) -> String {
    let mut out = String::new();
    for entry in resolved {
        let label = joined_family_label(entry.occurrence, families);
        let value = resolved_display_value(entry.occurrence, no_print_conv);
        let rendered = super::output_formatter::format_tag_value_with_mode(
            &entry.lookup_key,
            &value,
            no_print_conv,
        );
        out.push_str(&format!(
            "[{label}] {}: {rendered}\n",
            entry.occurrence.name
        ));
    }
    out
}

/// One line of ExifTool's short text output at `level` (1, 2, or 3 and
/// above), transcribed from the `exiftool` script's writer (13.59,
/// `exiftool`:3034-3061):
///
/// ```perl
/// } elsif ($outFormat == 0 or $outFormat == 1) {
///     if (defined $group) { $buff = sprintf("%-15s ", "[$group]"); $len = 16; }
///     $wid = 32 - (length($buff) - $len);
///     my $padLen = $wid - LengthUTF8($desc);  $padLen = 0 if $padLen < 0;
///     $buff .= $desc . (' ' x $padLen) . ": $val\n";
/// } elsif ($outFormat == 2) {
///     $buff = "[$group] " if defined $group;
///     $buff .= "$tagName: $val\n";
/// } ... else {
///     $buff = "$group " if defined $group;
///     $buff .= "$val\n";
/// }
/// ```
///
/// (`$desc` is the tag name once `$outFormat > 0`, `exiftool`:2861.) A group
/// label wider than the 15-column field pushes the name column right, and
/// the name's own padding shrinks by the same amount so `:` stays aligned:
/// `[MakerNotes:CIFF] Make                          : Canon`. Level 3 prints
/// the group *without* brackets.
pub fn short_output_line(level: u8, group: Option<&str>, name: &str, value: &str) -> String {
    match level {
        0 | 1 => {
            let mut line = String::new();
            let mut len = 0usize;
            if let Some(group) = group {
                line = format!("{:<15} ", format!("[{group}]"));
                len = 16;
            }
            let width = 32usize.saturating_sub(line.chars().count() - len);
            let pad = width.saturating_sub(name.chars().count());
            format!("{line}{name}{}: {value}\n", " ".repeat(pad))
        }
        2 => match group {
            Some(group) => format!("[{group}] {name}: {value}\n"),
            None => format!("{name}: {value}\n"),
        },
        _ => match group {
            Some(group) => format!("{group} {value}\n"),
            None => format!("{value}\n"),
        },
    }
}

/// Renders `resolved`, in its own order, at short output `level` (see
/// [`short_output_line`]): request order for a specific `-TAG` list
/// ([`resolve_requested_tags`]), file order for the full listing -- ExifTool
/// sorts neither. `families` is the `-G`/`-Gn` request, if any.
///
/// Values go through `output_formatter`'s short value rendering, exactly as
/// the `-G` short path and `ShortFormatter` always did. Without `-G`, the
/// tags `ShortFormatter` always hid stay hidden
/// ([`super::output_formatter::hidden_from_ungrouped_short_listing`]): this
/// renderer changes the line layout and order, not the tag set.
pub fn render_short_lines(
    resolved: &[ResolvedOccurrence<'_>],
    families: Option<&[u8]>,
    no_print_conv: bool,
    level: u8,
) -> String {
    let mut out = String::new();
    for entry in resolved {
        let value = resolved_display_value(entry.occurrence, no_print_conv);
        if families.is_none()
            && super::output_formatter::hidden_from_ungrouped_short_listing(
                &entry.lookup_key,
                &value,
            )
        {
            continue;
        }
        let rendered = super::output_formatter::format_tag_value_short_with_mode(
            &entry.lookup_key,
            &value,
            no_print_conv,
        );
        let label = families.map(|families| joined_family_label(entry.occurrence, families));
        out.push_str(&short_output_line(
            level,
            label.as_deref(),
            &entry.occurrence.name,
            &rendered,
        ));
    }
    out
}

/// Renders a tag selection at the default text level without `-G`, in
/// `resolved`'s own order, one `"{key}: {value}"` line per row.
///
/// `HumanReadableFormatter` sorts its map alphabetically and gives a repeated
/// key a `" (N)"` suffix. A selection retains request order and repeated
/// rows. Its key-based labels and value rendering match OxiDex's existing
/// human formatter; ExifTool's descriptive labels remain a separate gap.
pub fn render_human_lines(resolved: &[ResolvedOccurrence<'_>], no_print_conv: bool) -> String {
    let mut out = String::new();
    for entry in resolved {
        let value = resolved_display_value(entry.occurrence, no_print_conv);
        if super::output_formatter::hidden_from_ungrouped_short_listing(&entry.lookup_key, &value) {
            continue;
        }
        let rendered = super::output_formatter::format_tag_value_with_mode(
            &entry.lookup_key,
            &value,
            no_print_conv,
        );
        out.push_str(&format!("{}: {rendered}\n", entry.lookup_key));
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
    let no_print_conv = !args.exiftool_compat();

    // ExifTool's short text levels (`-s`, `-s2`/`-S`, `-s3`) render straight
    // from the resolved occurrences, in request or file order, through
    // `render_short_lines` -- with or without `-G`.
    let short_text = args.short_level > 0 && !args.json && !args.csv;

    let selection = match TagSelection::from_args(args) {
        Ok(selection) => selection,
        Err(error) => panic!(
            "resolve_file_output needs a validated tag selection \
             (CliArgs::parse_from refuses this one): {error}"
        ),
    };
    if let Some(selection) = selection {
        // Tags outside ExifTool's namespace (OxiDex's extended-only
        // diagnostics) stay hidden from wildcards, exactly as from the full
        // listing; a tag requested by name is shown, as it always was.
        let options = ReadOptions::new(&selection.plain_request_names(), args.extended_output);
        // ExifTool sorts by group only a full listing (`exiftool`:1853: `-G`
        // without a tag list), which with a selection means exclusions only.
        // Same rule, and same CSV carve-out, as the full listing below.
        let group_sort = if selection.requests.is_empty() && !args.csv {
            args.group_display.as_deref()
        } else {
            None
        };
        let mut resolved = select_occurrences(
            raw_metadata,
            &selection,
            args.all_tags,
            |key| options.shows_key(key),
            group_sort,
        );
        if args.json || args.csv {
            resolved = dedupe_repeated_rows(resolved);
        }
        if short_text {
            return ResolvedFileOutput::Lines(render_short_lines(
                &resolved,
                args.group_display.as_deref(),
                no_print_conv,
                args.short_level,
            ));
        }
        if let Some(families) = &args.group_display
            && !args.json
            && !args.csv
        {
            let lines = render_group_display_lines(&resolved, families, no_print_conv);
            return ResolvedFileOutput::Lines(lines);
        }
        if !args.json && !args.csv {
            return ResolvedFileOutput::Lines(render_human_lines(&resolved, no_print_conv));
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
    if args.group_display.is_some() || args.all_tags || short_text {
        let surviving_keys: HashSet<&str> = surviving.keys().map(String::as_str).collect();
        let mut resolved: Vec<ResolvedOccurrence> = if args.all_tags {
            raw_metadata
                .all_occurrences()
                .filter(|(key, _)| surviving_keys.contains(key.as_str()))
                .map(|(lookup_key, occurrence)| ResolvedOccurrence {
                    occurrence,
                    lookup_key,
                    copy: None,
                })
                .collect()
        } else {
            raw_metadata
                .winner_occurrences()
                .filter(|(key, _)| surviving_keys.contains(key.as_str()))
                .map(|(key, occurrence)| ResolvedOccurrence {
                    occurrence,
                    lookup_key: key.clone(),
                    copy: None,
                })
                .collect()
        };
        resolved.sort_by_key(|entry| entry.occurrence.order);
        // With `-G`, the `exiftool` script sorts the full listing by that
        // group (`exiftool` 13.59:1852-1855, `Sort => "Group$showGroup"`):
        // groups in order of first appearance, file order within each
        // (ExifTool.pm 13.59:3373-3397). File order alone is not it: the
        // IFD0 entries after `ExifOffset` are found after the whole ExifIFD
        // subtree, yet print with the rest of IFD0.
        if let Some(families) = &args.group_display
            && !args.csv
        {
            let mut rank: HashMap<String, usize> = HashMap::new();
            let ranks: Vec<usize> = resolved
                .iter()
                .map(|entry| {
                    let next = rank.len();
                    *rank
                        .entry(joined_family_label(entry.occurrence, families))
                        .or_insert(next)
                })
                .collect();
            let mut ranked: Vec<(usize, ResolvedOccurrence)> =
                ranks.into_iter().zip(resolved).collect();
            ranked.sort_by_key(|(rank, _)| *rank);
            resolved = ranked.into_iter().map(|(_, entry)| entry).collect();
        }

        if short_text {
            return ResolvedFileOutput::Lines(render_short_lines(
                &resolved,
                args.group_display.as_deref(),
                no_print_conv,
                args.short_level,
            ));
        }

        if let Some(families) = &args.group_display {
            if !args.json && !args.csv {
                let lines = render_group_display_lines(&resolved, families, no_print_conv);
                return ResolvedFileOutput::Lines(lines);
            }
            let metadata = build_display_map(&resolved, Some(families), no_print_conv, !args.json);
            return ResolvedFileOutput::Metadata(metadata);
        }

        // Ungrouped `-a`: the same occurrence set, keyed by each occurrence's
        // own literal `lookup_key`. `build_display_map` gives
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

    // `strip_extended_only` supplies only the surviving key set. Values must
    // remain attached to their original winning occurrence so selection never
    // reconstructs a channel from the flattened raw map.
    let channel = if no_print_conv {
        ValueChannel::ValueConv
    } else {
        ValueChannel::PrintConv
    };
    let metadata = raw_metadata
        .winner_occurrences()
        .filter(|(key, _)| surviving.contains_key(*key))
        .map(|(key, occurrence)| {
            let value = if no_print_conv {
                occurrence.project(channel).into_owned()
            } else {
                resolved_print_value(occurrence)
            };
            (key.clone(), value)
        })
        .collect();
    ResolvedFileOutput::Metadata(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Instance, Provenance, TagOccurrence};

    #[test]
    fn bare_metadata_paths_are_refused_without_losing_real_hyphenated_groups() {
        for path in [
            "JPEG-APP1",
            "JPEG-APP1-IFD0",
            "JPEG-APP1-IFD0-ExifIFD",
            "JPEG-SOF0",
            "TIFF-IFD0",
        ] {
            let error = parse_group_part(path, path).unwrap_err();
            assert!(error.contains("family 5 metadata path"), "{path}: {error}");
        }
        for group in ["XMP-dc", "JPEG-HDR", "DjVu-Meta", "MIE-Audio"] {
            assert!(parse_group_part(group, group).unwrap().is_some(), "{group}");
        }
        for group in ["0JPEG-APP1-IFD0", "1XMP-dc"] {
            assert!(parse_group_part(group, group).unwrap().is_some(), "{group}");
        }
    }

    fn canonical_occurrence(
        stored: i64,
        value: f64,
        print: &str,
        priority: u8,
        instance: Instance,
    ) -> TagOccurrence {
        TagOccurrence {
            id: oxidex_tags::TagId::Numeric(0x0900),
            name: crate::core::tag_occurrence::intern("ManometerPressure"),
            group0: crate::core::tag_occurrence::intern("MakerNotes"),
            group1: crate::core::tag_occurrence::intern("Olympus"),
            group2: Some(crate::core::tag_occurrence::intern("Camera")),
            instance,
            raw: TagValue::new_string(print),
            value: Some(TagValue::Float(value)),
            print: Some(TagValue::new_string(print)),
            stored: Some(TagValue::Integer(stored)),
            priority,
            is_list: false,
            order: 999,
            origin: Provenance {
                module: Some("Olympus"),
                table: Some("CameraSettings"),
                byte_range: None,
            },
        }
    }

    #[test]
    fn requested_output_uses_recorded_key_with_true_groups() {
        let mut metadata = MetadataMap::new();
        metadata.record_occurrence(
            "Olympus:ManometerPressure".to_string(),
            canonical_occurrence(1013, 101.3, "101.3 kPa", 1, Instance(1)),
        );
        metadata.record_occurrence(
            "Olympus:ManometerPressure".to_string(),
            canonical_occurrence(999, 99.9, "99.9 kPa", 0, Instance(1)),
        );
        metadata.record_occurrence(
            "Olympus:ManometerPressure".to_string(),
            canonical_occurrence(1200, 120.0, "120 kPa", 9, Instance(2)),
        );

        assert_eq!(
            metadata.get_string("Olympus:ManometerPressure"),
            Some("101.3 kPa"),
            "priority-zero and another instance must not displace the first winner"
        );
        for request in [
            "ManometerPressure",
            "MakerNotes:ManometerPressure",
            "Olympus:ManometerPressure",
        ] {
            let resolved = resolve_requested_tags(&metadata, &[request.to_string()], false);
            assert_eq!(resolved.len(), 1, "{request}");
            assert_eq!(resolved[0].lookup_key, "Olympus:ManometerPressure");
            assert_eq!(
                joined_family_label(resolved[0].occurrence, &[0, 1, 4]),
                "MakerNotes:Olympus"
            );
            assert_eq!(
                resolved_display_value(resolved[0].occurrence, true),
                TagValue::Float(101.3)
            );
        }

        let all =
            resolve_requested_tags(&metadata, &["Olympus:ManometerPressure".to_string()], true);
        assert_eq!(all.len(), 3);
        assert!(
            all.iter()
                .all(|resolved| resolved.lookup_key == "Olympus:ManometerPressure")
        );
        assert_eq!(
            all.iter()
                .map(|resolved| resolved.occurrence.order)
                .collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    fn canonical_cli_args(
        requested: &[&str],
        all_tags: bool,
        numeric: bool,
        groups: Option<Vec<u8>>,
    ) -> CliArgs {
        let mut args = requested
            .iter()
            .map(|name| std::ffi::OsString::from(format!("-{name}")))
            .collect::<Vec<_>>();
        args.push("fixture.orf".into());
        CliArgs {
            detector: crate::cli::args::DetectorMode::Signature,
            json: true,
            csv: false,
            short_level: 0,
            all_tags,
            group_display: groups,
            extended_output: false,
            recursive: false,
            preserve_file_times: false,
            backup: false,
            readonly: true,
            exiftool_compat: !numeric,
            tags_from_file: None,
            tags_from_file_position: None,
            date_format: None,
            dry_run: false,
            literal_paths: Vec::new(),
            strict: false,
            args,
        }
    }

    fn output_map(metadata: &MetadataMap, args: &CliArgs) -> MetadataMap {
        match resolve_file_output(metadata, args) {
            ResolvedFileOutput::Metadata(map) => map,
            ResolvedFileOutput::Lines(_) => panic!("JSON matrix must return metadata"),
        }
    }

    #[test]
    fn resolve_file_output_replays_the_complete_canonical_occurrence_matrix() {
        fn occurrence(
            name: &str,
            stored: i64,
            value: f64,
            print: &str,
            priority: u8,
            instance: Instance,
        ) -> TagOccurrence {
            TagOccurrence {
                id: oxidex_tags::TagId::Numeric(stored as u16),
                name: crate::core::tag_occurrence::intern(name),
                group0: crate::core::tag_occurrence::intern("MakerNotes"),
                group1: crate::core::tag_occurrence::intern("Olympus"),
                group2: Some(crate::core::tag_occurrence::intern("Camera")),
                instance,
                raw: TagValue::Float(value),
                value: Some(TagValue::Float(value)),
                print: Some(TagValue::new_string(print)),
                stored: Some(TagValue::Integer(stored)),
                priority,
                is_list: false,
                order: u32::MAX,
                origin: Provenance {
                    module: Some("Olympus"),
                    table: Some("CameraSettings"),
                    byte_range: None,
                },
            }
        }

        let mut metadata = MetadataMap::new();
        for row in [
            occurrence("NormalWinner", 1, 1.0, "old", 1, Instance::default()),
            occurrence("NormalWinner", 2, 2.0, "new", 1, Instance::default()),
            occurrence("PriorityZero", 3, 3.0, "first-zero", 0, Instance::default()),
            occurrence("PriorityZero", 4, 4.0, "later-zero", 0, Instance::default()),
            occurrence("InstanceWinner", 5, 5.0, "track-one", 1, Instance(1)),
            occurrence("InstanceWinner", 6, 6.0, "track-two", 9, Instance(2)),
            occurrence(
                "ChannelReplay",
                1013,
                101.3,
                "101.3 kPa",
                1,
                Instance::default(),
            ),
        ] {
            let key = format!("Olympus:{}", row.name);
            metadata.record_occurrence(key, row);
        }

        let default = output_map(&metadata, &canonical_cli_args(&[], false, false, None));
        assert_eq!(default.get_string("Olympus:NormalWinner"), Some("new"));
        assert_eq!(
            default.get_string("Olympus:PriorityZero"),
            Some("first-zero")
        );
        assert_eq!(
            default.get_string("Olympus:InstanceWinner"),
            Some("track-one")
        );
        assert_eq!(
            default.get_string("Olympus:ChannelReplay"),
            Some("101.3 kPa")
        );

        let requested = output_map(
            &metadata,
            &canonical_cli_args(
                &[
                    "NormalWinner",
                    "PriorityZero",
                    "InstanceWinner",
                    "ChannelReplay",
                ],
                false,
                false,
                None,
            ),
        );
        assert_eq!(requested.get_string("Olympus:NormalWinner"), Some("new"));
        assert_eq!(
            requested.get_string("Olympus:PriorityZero"),
            Some("first-zero")
        );
        assert_eq!(
            requested.get_string("Olympus:InstanceWinner"),
            Some("track-one")
        );

        for qualified in ["MakerNotes:ChannelReplay", "Olympus:ChannelReplay"] {
            let qualified_output = output_map(
                &metadata,
                &canonical_cli_args(&[qualified], false, false, None),
            );
            assert_eq!(
                qualified_output.get_string("Olympus:ChannelReplay"),
                Some("101.3 kPa"),
                "family-0 and family-1 qualifiers must reach the real output entry point: {qualified}"
            );
        }
        for wrong_family in ["EXIF:ChannelReplay", "Canon:ChannelReplay"] {
            let rejected = output_map(
                &metadata,
                &canonical_cli_args(&[wrong_family], false, false, None),
            );
            assert_eq!(
                rejected.len(),
                0,
                "a non-matching true-family qualifier must not fall back to the bare tag: {wrong_family}"
            );
        }

        let all = output_map(
            &metadata,
            &canonical_cli_args(&["PriorityZero"], true, false, None),
        );
        assert_eq!(all.get_string("Olympus:PriorityZero"), Some("first-zero"));
        assert_eq!(
            all.get_string("Olympus:PriorityZero (2)"),
            Some("later-zero")
        );

        let numeric = output_map(
            &metadata,
            &canonical_cli_args(&["ChannelReplay"], false, true, None),
        );
        assert_eq!(
            numeric.get("Olympus:ChannelReplay"),
            Some(&TagValue::Float(101.3))
        );

        let grouped = output_map(
            &metadata,
            &canonical_cli_args(&["ChannelReplay"], false, false, Some(vec![0, 1, 2])),
        );
        assert_eq!(
            grouped.get_string("MakerNotes:Olympus:Camera:ChannelReplay"),
            Some("101.3 kPa")
        );

        let qualified_group_014 = output_map(
            &metadata,
            &canonical_cli_args(
                &["MakerNotes:ChannelReplay"],
                false,
                false,
                Some(vec![0, 1, 4]),
            ),
        );
        // Pinned 13.59 `-j -G0:1:4 -Olympus:all t/images/Olympus.jpg` keys
        // `MakerNotes:Olympus:SpecialMode`: `GetGroup` drops the empty
        // family-4 slot of a multi-family request (see
        // `joined_family_label`), so no `::` survives into the key.
        assert_eq!(
            qualified_group_014.get_string("MakerNotes:Olympus:ChannelReplay"),
            Some("101.3 kPa"),
            "-G0:1:4 drops the empty family-4 slot, as ExifTool's GetGroup does"
        );

        let replay = metadata
            .keyed_occurrences()
            .find(|(_, row)| row.name.as_ref() == "ChannelReplay")
            .map(|(_, row)| row)
            .expect("canonical row retained");
        assert_eq!(replay.stored, Some(TagValue::Integer(1013)));
        assert_eq!(replay.value, Some(TagValue::Float(101.3)));
        assert_eq!(replay.print, Some(TagValue::new_string("101.3 kPa")));
        assert_eq!(replay.raw, TagValue::Float(101.3));
    }

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
    fn selected_duplicate_projection_keeps_later_winner_under_bare_key() {
        let mut metadata = MetadataMap::new();
        metadata.insert_occurrence(
            "File:Comment",
            TagValue::new_string("first"),
            1,
            "",
            Instance::default(),
        );
        metadata.insert_occurrence(
            "File:Comment",
            TagValue::new_string("winner"),
            1,
            "",
            Instance::default(),
        );
        let projected = output_map(
            &metadata,
            &canonical_cli_args(&["all:Comment"], false, false, None),
        );
        assert_eq!(projected.get_string("File:Comment"), Some("winner"));
        assert_eq!(projected.get_string("File:Comment (2)"), Some("first"));
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
        source.insert_occurrence_with_raw(
            "IFD0:Orientation",
            TagValue::new_string("Rotate 90 CW"),
            TagValue::new_integer(6),
            1,
            "IFD0",
            Instance::default(),
        );
        source.insert_occurrence_with_raw(
            "ExifIFD:FNumber",
            TagValue::new_string("2.8"),
            TagValue::new_rational(28, 10),
            1,
            "ExifIFD",
            Instance::default(),
        );
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
                    short_level: 0,
                    all_tags,
                    group_display: grouped.then_some(vec![0, 1]),
                    extended_output: false,
                    recursive: false,
                    preserve_file_times: false,
                    backup: false,
                    readonly: true,
                    exiftool_compat: !no_print_conv,
                    tags_from_file: None,
                    tags_from_file_position: None,
                    date_format: None,
                    dry_run: false,
                    literal_paths: Vec::new(),
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
    fn all_only_group_parts_use_the_named_tag_winner() {
        let metadata = sample_metadata();
        for (selector, expected_count) in [("all:Make", 2), ("1all:Make", 1), ("all:all:Make", 1)] {
            let selection = TagSelection::parse(&[selector.to_string()], &[]).unwrap();
            let resolved = select_occurrences(&metadata, &selection, false, |_| true, None);
            assert_eq!(resolved.len(), expected_count, "{selector}");
            if expected_count == 1 {
                assert_eq!(resolved[0].occurrence.raw, TagValue::new_string("Canon"));
            }
        }
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

    /// `GetGroup`'s multi-family simplification: pinned 13.59 prints
    /// `[File]` for `-G0:1 -FileType` and `[MakerNotes:CIFF]` for `-G0:1:4
    /// -Make` on `t/images/ExifTool.jpg`.
    #[test]
    fn joined_family_label_simplifies_multi_family_requests_like_get_group() {
        let mut metadata = MetadataMap::new();
        metadata.insert_occurrence_with_raw(
            "File:FileType",
            TagValue::new_string("JPEG"),
            TagValue::new_string("JPEG"),
            1,
            "File",
            Instance::default(),
        );
        metadata.insert("CIFF:Make", TagValue::new_string("Canon"));
        let file_type = resolve_requested_tag(&metadata, "FileType").unwrap();
        assert_eq!(joined_family_label(file_type, &[0, 1]), "File");
        assert_eq!(joined_family_label(file_type, &[1]), "File");
        let make = resolve_requested_tag(&metadata, "Make").unwrap();
        assert_eq!(joined_family_label(make, &[0, 1, 4]), "MakerNotes:CIFF");
        // A single family is never simplified, even when empty.
        assert_eq!(joined_family_label(make, &[4]), "");
    }

    /// Every layout below is the pinned 13.59 oracle's own line for
    /// `t/images/ExifTool.jpg` (`-s`, `-G1 -s`, `-G0:1 -s`, `-G1 -s2`,
    /// `-G1 -s3`, `-s3`).
    #[test]
    fn short_output_line_matches_the_exiftool_writer_at_each_level() {
        assert_eq!(
            short_output_line(1, None, "Make", "Canon"),
            "Make                            : Canon\n"
        );
        assert_eq!(
            short_output_line(1, Some("CIFF"), "Make", "Canon"),
            "[CIFF]          Make                            : Canon\n"
        );
        assert_eq!(
            short_output_line(1, Some("MakerNotes:CIFF"), "Make", "Canon"),
            "[MakerNotes:CIFF] Make                          : Canon\n"
        );
        // A name longer than its column is never truncated.
        let long = "A".repeat(40);
        assert_eq!(
            short_output_line(1, None, &long, "x"),
            format!("{long}: x\n")
        );
        assert_eq!(
            short_output_line(2, Some("CIFF"), "Make", "Canon"),
            "[CIFF] Make: Canon\n"
        );
        assert_eq!(short_output_line(2, None, "Make", "Canon"), "Make: Canon\n");
        assert_eq!(
            short_output_line(3, Some("CIFF"), "Make", "Canon"),
            "CIFF Canon\n"
        );
        assert_eq!(short_output_line(3, None, "Make", "Canon"), "Canon\n");
        assert_eq!(short_output_line(9, None, "Make", "Canon"), "Canon\n");
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
    fn legacy_untyped_occurrences_keep_name_based_print_conversions() {
        // These producers still use the compatibility insert shim: their raw
        // values are typed, but no explicit PrintConv form is attached.  The
        // CLI must retain the old ExifTool-compatible display projection
        // while the typed occurrence consumers are being migrated.
        let mut metadata = MetadataMap::new();
        metadata.insert("Ducky:Quality", TagValue::new_integer(84));
        metadata.insert("GPS:GPSLatitudeRef", TagValue::new_string("N"));
        metadata.insert("GPS:GPSLongitudeRef", TagValue::new_string("W"));
        metadata.insert("GPS:GPSDestDistanceRef", TagValue::new_string(""));
        metadata.insert_with_group1("APP14:APP14Flags0", TagValue::new_integer(0), "Adobe");
        metadata.insert_with_group1("APP14:APP14Flags1", TagValue::new_integer(0), "Adobe");

        for (request, expected) in [
            ("Quality", TagValue::new_string("84%")),
            ("GPSLatitudeRef", TagValue::new_string("North")),
            ("GPSLongitudeRef", TagValue::new_string("West")),
            ("GPSDestDistanceRef", TagValue::new_string("Unknown ()")),
            ("APP14Flags0", TagValue::new_string("(none)")),
            ("APP14Flags1", TagValue::new_string("(none)")),
        ] {
            let resolved = resolve_requested_tags(&metadata, &[request.to_string()], false);
            assert_eq!(resolved.len(), 1, "request {request} must resolve");
            assert_eq!(
                resolved_display_value(resolved[0].occurrence, false),
                expected,
                "legacy PrintConv compatibility for {request}"
            );
        }
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
        source.insert_occurrence_with_raw(
            "IFD0:Orientation",
            TagValue::new_string("Rotate 90 CW"),
            TagValue::new_integer(6),
            1,
            "IFD0",
            Instance::default(),
        );
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
                    short_level: 0,
                    all_tags,
                    group_display: grouped.then_some(vec![0, 1]),
                    extended_output: false,
                    recursive: false,
                    preserve_file_times: false,
                    backup: false,
                    readonly: true,
                    exiftool_compat: !raw,
                    tags_from_file: None,
                    tags_from_file_position: None,
                    date_format: None,
                    dry_run: false,
                    literal_paths: Vec::new(),
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
                    args.short_level = 2;
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
}
