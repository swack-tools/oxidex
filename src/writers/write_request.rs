//! Which physical address a `-TAG=VALUE` request names, decided before any
//! writer runs -- and a refusal whenever that cannot be proven.
//!
//! The defect this closes: `oxidex -XPTitle=v photo.jpg` printed
//! `1 image files updated` and wrote nothing. An ungrouped name reached the
//! writers unresolved, every EXIF writer only visits `IFD0:`/`ExifIFD:`/
//! `GPS:`/`EXIF:` keys (`exif_surgical::plan_exif_write_with_removals`'s
//! `exif_family_keys`, `tiff_surgical::is_exif_family`), and nothing noticed
//! that the requested key had been skipped. The same silence swallowed any
//! grouped key a format's writer does not address (`-XMP:Title=` in a JPEG,
//! `-IFD1:...`, `-File:Comment=`, a PDF key outside its Info dictionary).
//!
//! Two rules, applied to every write request by the library's one write
//! transaction (`core::write_transaction`), which `write_metadata`,
//! `modify_tag`, `remove_tag`, `copy_metadata`, the C ABI and the CLI all
//! go through:
//!
//! 1. [`resolve_write_key`] turns an ungrouped name into the address pinned
//!    ExifTool 13.59 would *create* it at, and only where that is provable
//!    from source-captured data; otherwise it refuses.
//! 2. [`ensure_writer_addresses`] refuses a key the target format's writer
//!    would drop. A refusal is an error ([`ExifToolError::TagsNotWritten`],
//!    naming the key), never a success.
//!
//! # How ExifTool resolves an ungrouped write (Writer.pl, 13.59)
//!
//! `SetNewValue` collects every `FindTagInfo` candidate for the name across
//! all tables (Writer.pl:613-782). Each writable candidate gets its group's
//! `WRITE_PRIORITY` (ExifTool.pm:3950-3960: EXIF 90, IPTC 80, XMP 70,
//! MakerNotes 60, QuickTime 50, ..., File and Composite 500; unlisted 0),
//! plus any `Preferred`/`PREFERRED` bump. The highest-priority candidates are
//! *created*; every other candidate is written only where the file already
//! carries it ("if tag exists"). An `Avoid` candidate yields creation to the
//! next best (Writer.pl:792-825).
//!
//! So for a JPEG or TIFF, an ungrouped name is exactly one `Exif::Main` field
//! when: the name has exactly one writable `Exif::Main` candidate, it is not
//! `Avoid`/`Protected`/`Permanent`/a SubDirectory (none of which this route
//! models), no File/Composite candidate outranks it, and the file carries the
//! name in no other group ExifTool would also update. The candidate set is
//! [`SET_NEW_VALUE_LOOKUP`] (captured from the pinned `FindTagInfo`), the
//! flags come from the transcribed `Exif::Main` table, and the write group is
//! the candidate's `WriteGroup` or the table's `WRITE_GROUP => 'ExifIFD'`
//! (Exif.pm:415). Anything else is refused with the reason, so the user can
//! name the group explicitly.
//!
//! [`SET_NEW_VALUE_LOOKUP`]: super::generated_setnewvalue_address_rules::SET_NEW_VALUE_LOOKUP

use super::exif_surgical::MakerNoteCensus;
use super::generated_setnewvalue_address_rules::{
    SET_NEW_VALUE_LOOKUP, StaticCandidatePrintConv, StaticNativeLookupCandidate,
};
use super::generated_tag_exists::{SHORTCUTS, TAG_EXISTS};
use crate::core::FileFormat;
use crate::core::metadata_map::MetadataMap;
use crate::error::{ExifToolError, Result};

/// `Exif::Main`'s table-level `WRITE_GROUP` (Exif.pm:415), the write group of
/// every candidate that declares none of its own.
const EXIF_MAIN_WRITE_GROUP: &str = "ExifIFD";

/// A closed, source-captured PrintConv hash proves a request cannot convert
/// only when no eligible native candidate can interpret its text. Hashes with
/// callbacks, inverse expressions or unsupported variants remain unknown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CandidateConversion {
    NotInPrintConv,
    Ambiguous,
    Possible,
    Unknown,
}

impl CandidateConversion {
    fn rejected(self) -> bool {
        matches!(self, Self::NotInPrintConv | Self::Ambiguous)
    }
}

fn candidate_conversion(
    candidate: &StaticNativeLookupCandidate,
    value: &str,
) -> CandidateConversion {
    let StaticCandidatePrintConv::PlainHash(labels) = candidate.print_conv else {
        return CandidateConversion::Unknown;
    };
    // ReverseLookup accepts Unknown (X) before looking at the hash. Its
    // subsequent matching tiers include case-insensitive substring matches.
    // The source rejects ambiguous partial matches. Non-ASCII folding stays
    // unknown so it cannot prove that every candidate rejects the value.
    let unknown_form = value.strip_suffix('\n').unwrap_or(value);
    let lower = unknown_form.to_ascii_lowercase();
    if lower.starts_with("unknown")
        && lower[7..]
            .trim_start_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}'])
            .starts_with('(')
        && lower.ends_with(')')
        && !lower
            .split_once('(')
            .is_some_and(|(_, inner)| inner.contains('\n'))
    {
        return CandidateConversion::Possible;
    }
    if labels.iter().any(|label| !label.is_ascii()) {
        return CandidateConversion::Unknown;
    }
    // Native CLI operands are byte strings. A non-ASCII query cannot match
    // an all-ASCII hash, and Unicode whitespace must not be stripped.
    let query = value
        .trim_end_matches([' ', '\t', '\n', '\r', '\u{b}', '\u{c}'])
        .to_ascii_lowercase();
    // Writer.pl ReverseLookup tries exact, case-insensitive exact, prefix,
    // then substring. A non-exact tier with multiple matches stops there;
    // it does not use a later tier to choose one of them.
    if labels
        .iter()
        .any(|label| label.eq_ignore_ascii_case(&query))
    {
        return CandidateConversion::Possible;
    }
    for count in [
        labels
            .iter()
            .filter(|label| label.to_ascii_lowercase().starts_with(&query))
            .count(),
        labels
            .iter()
            .filter(|label| label.to_ascii_lowercase().contains(&query))
            .count(),
    ] {
        match count {
            0 => continue,
            1 => return CandidateConversion::Possible,
            _ => return CandidateConversion::Ambiguous,
        }
    }
    CandidateConversion::NotInPrintConv
}

/// The EXIF destination for a bare request rejected by every writable native
/// candidate. `applicable` may narrow candidates using a file's physical
/// evidence; without it this is the command-wide SetNewValue decision.
pub(crate) fn rejected_bare_conversion(
    name: &str,
    value: &str,
    applicable: impl Fn(&StaticNativeLookupCandidate) -> bool,
) -> Option<(&'static str, &'static str)> {
    let candidates: Vec<_> = SET_NEW_VALUE_LOOKUP
        .iter()
        .filter(|candidate| {
            candidate.name.eq_ignore_ascii_case(name) && candidate.candidate_writable
        })
        .collect();
    let exif = candidates.iter().find(|candidate| {
        is_exif_main(candidate)
            && matches!(candidate.print_conv, StaticCandidatePrintConv::PlainHash(_))
    })?;
    if candidates
        .iter()
        .filter(|candidate| applicable(candidate))
        .all(|candidate| candidate_conversion(candidate, value).rejected())
    {
        let reason = match candidate_conversion(exif, value) {
            CandidateConversion::Ambiguous => "matches more than one PrintConv",
            CandidateConversion::NotInPrintConv => "not in PrintConv",
            _ => return None,
        };
        Some((exif.write_group.unwrap_or(EXIF_MAIN_WRITE_GROUP), reason))
    } else {
        None
    }
}

/// Whether one candidate can still be selected in this file. The only
/// absence proofs used here are the physical maker-note census and absence of
/// a MIE row. Other families stay possible when their creation rules are not
/// captured; that deliberately prevents a false conversion warning.
pub(crate) fn candidate_applies_to_file(
    candidate: &StaticNativeLookupCandidate,
    name: &str,
    baseline: &MetadataMap,
    census: MakerNoteCensus,
) -> bool {
    match family0(candidate) {
        Some("MakerNotes") => family1(candidate)
            .is_none_or(|group| makernote_group_may_hold(name, group, baseline, &|| census)),
        Some("MIE") => mie_row(baseline).is_some(),
        _ => true,
    }
}

/// Whether ExifTool writes an ungrouped `name` to a PNG as a PNG text tag:
/// pinned 13.59 answers `-Software=NEW` on a PNG with `[PNG] Software`, not
/// `[IFD0] Software` -- the name has a `PNG` candidate (`PNG::TextualData`,
/// `PREFERRED => 1`, PNG.pm:596) -- while `-XPTitle=v` (no PNG candidate)
/// lands in `[IFD0]`. oxidex refuses the former rather than guess how the
/// text chunk would be spelled.
pub(crate) fn png_prefers_text(name: &str) -> bool {
    SET_NEW_VALUE_LOOKUP.iter().any(|candidate| {
        candidate.name.eq_ignore_ascii_case(name) && family0(candidate) == Some("PNG")
    })
}

/// Groups whose same-named rows ExifTool's `Exif::Main` write never touches:
/// the EXIF directories themselves (a `WriteGroup => 'IFD0'` value is written
/// to IFD0 only) and groups with no writable table (file-system, derived and
/// ExifTool-generated rows).
const NOT_ALSO_UPDATED: &[&str] = &[
    "IFD0",
    "IFD1",
    "ExifIFD",
    "GPS",
    "InteropIFD",
    "EXIF",
    "SubIFD",
    "File",
    "System",
    "Composite",
    "ExifTool",
];

/// A name part `SetNewValue` would judge with `TagExists`: word characters,
/// optionally followed by the `#` no-conversion suffix. Language-coded
/// (`Title-de`) and wildcard names follow other paths and are not judged here.
fn plain_name(tag: &str) -> Option<&str> {
    let name = tag.rsplit(':').next().unwrap_or(tag);
    let name = name.strip_suffix('#').unwrap_or(name);
    (!name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        .then_some(name)
}

/// Whether pinned ExifTool defines `name` at all (`TagLookup::TagExists`, or a
/// `Shortcuts::Main` key). Case-insensitive, like ExifTool.
pub fn exiftool_tag_exists(name: &str) -> bool {
    TAG_EXISTS
        .binary_search(&name.to_ascii_lowercase().as_str())
        .is_ok()
}

/// The tags a `-TAG=VALUE` / `-TAG=` request stands for when `TAG` names a
/// `Shortcuts::Main` key (any case), or `None` for any other name.
///
/// Pinned ExifTool 13.59's `SetNewValue` sets every tag of the shortcut in
/// turn with the request's options (Writer.pl:562-578): a member's own group
/// replaces the request's (`-XMP:CommonIFD0=` still writes `IFD0:Make`), a
/// member without one takes the request's (`-XMP:AllDates=` writes
/// `XMP-exif:DateTimeOriginal`), and a trailing `#` (`ValueConv`) applies to
/// every member. The expansion is the generated table
/// ([`SHORTCUTS`](super::generated_tag_exists::SHORTCUTS)), never a hand
/// list: `-CommonIFD0=` deletes the seventeen IFD0 fields ExifTool deletes.
pub fn expand_write_shortcut(tag: &str) -> Option<Vec<String>> {
    let (group, name) = match tag.rsplit_once(':') {
        Some((group, name)) => (Some(group), name),
        None => (None, tag),
    };
    let (name, suffix) = match name.strip_suffix('#') {
        Some(name) => (name, "#"),
        None => (name, ""),
    };
    let (_, members) = SHORTCUTS
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))?;
    Some(
        members
            .iter()
            .map(|member| match group {
                Some(group) if !member.contains(':') => format!("{group}:{member}{suffix}"),
                _ => format!("{member}{suffix}"),
            })
            .collect(),
    )
}

/// ExifTool 13.59's fixed wording for a name `SetNewValue` finds no writable
/// candidate for at all (Writer.pl:584): `Sorry, <tag> doesn't exist or isn't
/// writable`. Shared by [`undefined_tag_warning`]'s `PDF:CreationDate`/
/// `PDF:ModDate` case (name-based, decided before any file opens) and
/// `core::operations::resolve_write_key_for`'s `PNG:XMP` case (data-dependent:
/// only when the file's `PNG:XMP` names a literal `XMP`-keyword text chunk,
/// which pinned 13.59 also refuses this way -- see `write_transaction.rs`'s
/// `png_xmp_packets`). Both are "ExifTool itself says so", so the CLI must
/// echo ExifTool's own `Warning: <this>` / `Nothing to do.` shape rather than
/// oxidex's own "Failed to ..." wrapping (`cli::write_transaction::
/// describe_set_failure`).
pub(crate) fn sorry_not_writable(tag: &str) -> String {
    format!("Sorry, {tag} doesn't exist or isn't writable")
}

/// ExifTool's warning for a `-TAG=` whose name it does not define, spelled
/// exactly as `SetNewValue` spells it (Writer.pl:584), or `None` when the name
/// is defined or is not a plain name this check applies to.
///
/// `exiftool -NoSuchTag=v a.jpg` (13.59): `Warning: Tag 'NoSuchTag' is not
/// defined`, then `Nothing to do.` and exit 1 when no other tag was set.
pub fn undefined_tag_warning(tag: &str) -> Option<String> {
    if crate::writers::png_writer::old_png_read_only_tag(tag) {
        return Some(sorry_not_writable(tag));
    }
    // `-GROUP:All=` is a group-wide deletion (Writer.pl:400-484 rewrites
    // `all` to `*`), not a tag named `All`: `-GPS:All=` is defined wherever
    // GPS is a deletable group, and an unknown group is ExifTool's `Not a
    // deletable group: Foo` / `Nothing to do.`.
    if let Some(group) = group_deletion(tag) {
        return (!deletable_group(group) && !is_family2_group(group))
            .then(|| format!("Not a deletable group: {group}"));
    }
    // `CreationDate` and `ModDate` are the PDF Info dictionary's keys, not
    // ExifTool tag names: PDF.pm's Info table names them `CreateDate` and
    // `ModifyDate` (PDF.pm:126-143), so pinned 13.59 answers `-PDF:CreationDate=` with `Sorry,
    // PDF:CreationDate doesn't exist or isn't writable` / `Nothing to do.`
    // (the reader surfaces both spellings; only ExifTool's writes).
    if let Some((group, name)) = tag.rsplit_once(':')
        && group.eq_ignore_ascii_case("PDF")
        && (name.eq_ignore_ascii_case("CreationDate") || name.eq_ignore_ascii_case("ModDate"))
    {
        return Some(sorry_not_writable(tag));
    }
    let name = plain_name(tag)?;
    if exiftool_tag_exists(name) {
        // A defined name in an EXIF-family group it has no address in:
        // `SetNewValue` finds no candidate there (pinned 13.59:
        // `-IFD0:CanonModelID=1` is `Sorry, IFD0:CanonModelID doesn't exist or
        // isn't writable` / `Nothing to do.`). Only where the captured
        // candidates prove it.
        if let Some((group, _)) = tag.rsplit_once(':')
            && exif_group_answer(group, name) == Some(ExifGroupAnswer::Rejected)
        {
            let spelled = tag.strip_suffix('#').unwrap_or(tag);
            return Some(format!("Sorry, {spelled} doesn't exist or isn't writable"));
        }
        return None;
    }
    let spelled = tag.strip_suffix('#').unwrap_or(tag);
    Some(format!("Tag '{spelled}' is not defined"))
}

/// What ExifTool's `SetNewValue` makes of `GROUP:NAME` for an EXIF-family
/// group, as far as oxidex can prove it (see [`exif_group_answer`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExifGroupAnswer {
    /// The name has an address the group may hold: an `Exif::Main` or
    /// `GPS::Main` tag in any EXIF directory (pinned 13.59 accepts
    /// `GPS:Make`, `IFD1:GPSAltitude`, `ExifIFD:Artist`), a maker-note tag
    /// under `MakerNotes`.
    Accepted,
    /// The name's captured candidates hold no such address: ExifTool's
    /// `Sorry, GROUP:NAME doesn't exist or isn't writable`.
    Rejected,
    /// Neither provable.
    Unknown,
}

/// [`ExifGroupAnswer`] for `name` in `group`, or `None` when `group` is not
/// an EXIF-family group (`IFD0`, `IFD1`, `ExifIFD`, `GPS`, `InteropIFD`,
/// `EXIF`, `MakerNotes`; any letter case).
///
/// Proof comes from [`SET_NEW_VALUE_LOOKUP`], the pinned `FindTagInfo`
/// candidates, which list every table a captured name lives in -- so a
/// captured name with no candidate of the group's family 0 (`EXIF`, or
/// `MakerNotes`) is `Rejected` -- and, for a name the lookup did not
/// capture, the registry's `EXIF:`/`GPS:` rows, which only prove `Accepted`
/// (pinned 13.59 accepts `IFD1:GPSAltitude`).
pub fn exif_group_answer(group: &str, name: &str) -> Option<ExifGroupAnswer> {
    let exif_dirs = ["IFD0", "IFD1", "ExifIFD", "GPS", "InteropIFD", "EXIF"];
    let family = if exif_dirs.iter().any(|g| g.eq_ignore_ascii_case(group)) {
        "EXIF"
    } else if group.eq_ignore_ascii_case("MakerNotes") {
        "MakerNotes"
    } else {
        return None;
    };
    let candidates: Vec<&StaticNativeLookupCandidate> = SET_NEW_VALUE_LOOKUP
        .iter()
        .filter(|candidate| candidate.name.eq_ignore_ascii_case(name))
        .collect();
    // A captured name's candidates are the whole answer: the registry also
    // lists EXIF tags ExifTool cannot write (`ExifIFD:DeviceSettingDescription`
    // is `Sorry, ... doesn't exist or isn't writable` in pinned 13.59).
    if !candidates.is_empty() {
        return Some(
            if candidates
                .iter()
                .any(|candidate| family0(candidate) == Some(family))
            {
                ExifGroupAnswer::Accepted
            } else {
                ExifGroupAnswer::Rejected
            },
        );
    }
    // PixelUnits is PNG::PhysicalDimensions 8 in every reviewed source,
    // with no Exif::Main or GPS::Main row. The broad registry name fallback
    // cannot establish an IFD write address for it.
    if family == "EXIF" && name.eq_ignore_ascii_case("PixelUnits") {
        return Some(ExifGroupAnswer::Rejected);
    }
    let registered = family == "EXIF"
        && ["EXIF", "GPS"].iter().any(|prefix| {
            crate::tag_db::tag_registry::get_tag_descriptor(&format!("{prefix}:{name}")).is_some()
        });
    Some(if registered {
        ExifGroupAnswer::Accepted
    } else {
        ExifGroupAnswer::Unknown
    })
}

/// ExifTool 13.59's `@delGroups` (Writer.pl:141-149): the groups a
/// `-GROUP:All=` may delete, besides any `XMP-*`/`XML-*` family-1 group and
/// `MIE<n>` (Writer.pl:426).
const DELETABLE_GROUPS: &[&str] = &[
    "Adobe",
    "AFCP",
    "APP0",
    "APP1",
    "APP2",
    "APP3",
    "APP4",
    "APP5",
    "APP6",
    "APP7",
    "APP8",
    "APP9",
    "APP10",
    "APP11",
    "APP12",
    "APP13",
    "APP14",
    "APP15",
    "AROT",
    "AudioKeys",
    "CanonVRD",
    "CIFF",
    "Ducky",
    "EXIF",
    "ExifIFD",
    "File",
    "FlashPix",
    "FotoStation",
    "GlobParamIFD",
    "GPS",
    "ICC_Profile",
    "IFD0",
    "IFD1",
    "Insta360",
    "InteropIFD",
    "IPTC",
    "ItemList",
    "iTunes",
    "JFIF",
    "Jpeg2000",
    "JUMBF",
    "Keys",
    "MakerNotes",
    "Meta",
    "MetaIFD",
    "Microsoft",
    "MIE",
    "MPF",
    "Nextbase",
    "NikonApp",
    "NikonCapture",
    "PDF",
    "PDF-update",
    "PhotoMechanic",
    "Photoshop",
    "PNG",
    "PNG-pHYs",
    "PrintIM",
    "QuickTime",
    "RMETA",
    "RSRC",
    "SEAL",
    "SubIFD",
    "Trailer",
    "UserData",
    "VideoKeys",
    "Vivo",
    "XML",
    "XMP",
];

/// ExifTool 13.59's `@delGroup2` (Writer.pl:151-154): family-2 names a
/// group deletion also accepts.
const FAMILY2_GROUPS: &[&str] = &[
    "Audio", "Author", "Camera", "Document", "ExifTool", "Image", "Location", "Other", "Preview",
    "Printing", "Time", "Video",
];

/// Groups a request may name in any letter case that the checks downstream
/// compare by their canonical spelling, beyond the EXIF and maker-note
/// groups [`crate::writers::exif_surgical::canonical_write_key`] knows.
const CANONICAL_REQUEST_GROUPS: &[&str] = &["PDF", "PNG", "XMP", "IPTC", "File", "Photoshop"];

/// The one spelling every step of a write request sees: value typing
/// (`cli::value_parser`), address resolution ([`resolve_write_key`]), the
/// hand-kept spellings, the no-op checks and the transaction.
///
/// Group and tag names are case-insensitive to ExifTool (`-exififd:iso=200`
/// writes ExifIFD:ISO, pinned 13.59), while oxidex's registry lookups are
/// not: typed as spelled, `exififd:iso` found no descriptor, its value
/// stayed a string, and the write was refused as a type mismatch. A grouped
/// name takes #943's canonical spelling (`exif_surgical::canonical_write_key`
/// against no file: the EXIF and maker-note groups, `All`, the registry's
/// tag spelling) and a group in [`CANONICAL_REQUEST_GROUPS`] its own. An
/// ungrouped name takes the registry's spelling (its EXIF one where the
/// spellings differ). A trailing `#` is kept. A name the registry does not
/// know is returned as given.
pub fn canonical_request_tag(tag: &str) -> String {
    let (base, hash) = match tag.strip_suffix('#') {
        Some(base) => (base, "#"),
        None => (tag, ""),
    };
    let spelled = match base.rsplit_once(':') {
        Some(_) => {
            let key = crate::writers::exif_surgical::canonical_write_key(base, &MetadataMap::new());
            let (group, name) = key.rsplit_once(':').unwrap_or(("", key.as_str()));
            let group = CANONICAL_REQUEST_GROUPS
                .iter()
                .find(|known| known.eq_ignore_ascii_case(group))
                .map_or_else(
                    || match group.get(..4) {
                        Some(prefix) if prefix.eq_ignore_ascii_case("xmp-") => {
                            format!("XMP-{}", &group[4..])
                        }
                        _ => group.to_string(),
                    },
                    |known| known.to_string(),
                );
            let name = crate::tag_db::tag_registry::canonical_tag_name_spelling(&group, name)
                .unwrap_or(name);
            format!("{group}:{name}")
        }
        None => crate::tag_db::tag_registry::canonical_tag_name_spelling("EXIF", base)
            .unwrap_or(base)
            .to_string(),
    };
    format!("{spelled}{hash}")
}

/// The group of a `-GROUP:All=` (or `GROUP:*`) group deletion.
pub fn group_deletion(tag: &str) -> Option<&str> {
    let (group, name) = tag.rsplit_once(':')?;
    (name.eq_ignore_ascii_case("all") || name == "*").then_some(group)
}

/// Whether ExifTool deletes `group` by `-GROUP:All=` (see [`DELETABLE_GROUPS`]).
pub fn deletable_group(group: &str) -> bool {
    let lower = group.to_ascii_lowercase();
    DELETABLE_GROUPS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(group))
        || ((lower.starts_with("xmp-") || lower.starts_with("xml-")) && group.len() > 4)
        || (lower.starts_with("mie")
            && lower.len() > 3
            && lower[3..].bytes().all(|b| b.is_ascii_digit()))
}

fn is_family2_group(group: &str) -> bool {
    FAMILY2_GROUPS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(group))
}

/// A typed refusal of `tag` ([`ExifToolError::TagsNotWritten`]); it displays
/// as `Cannot write tag '<tag>': <reason>`.
fn refuse(tag: &str, reason: impl std::fmt::Display) -> ExifToolError {
    ExifToolError::tag_not_written(tag, reason.to_string())
}

fn is_exif_main(candidate: &StaticNativeLookupCandidate) -> bool {
    candidate.module == Some("Exif") && candidate.table == Some("Main")
}

fn family0(candidate: &StaticNativeLookupCandidate) -> Option<&'static str> {
    candidate
        .groups
        .iter()
        .find(|group| group.family == 0)
        .map(|group| group.value)
}

fn family1(candidate: &StaticNativeLookupCandidate) -> Option<&'static str> {
    candidate
        .groups
        .iter()
        .find(|group| group.family == 1)
        .map(|group| group.value)
}

/// Resolves the key a writer should receive for `tag`.
///
/// A grouped key (`IFD0:Make`, `XMP:Title`) is returned as written; whether
/// the format's writer can address it is [`ensure_writer_addresses`]'s call.
/// An ungrouped name is resolved per the module docs when `exif_ifd0_target`
/// (the file's IFD0 is read with `Exif::Main`: a JPEG's APP1, or a TIFF
/// structure whose header identifier is 42 -- ExifTool.pm:8718; Panasonic's
/// 0x55 selects `PanasonicRaw::Main` instead, ExifTool.pm:8646-8659), and
/// refused otherwise. `baseline` is the file's current metadata map, used to
/// find same-named rows ExifTool would also update.
pub(crate) fn resolve_write_key(
    tag: &str,
    exif_ifd0_target: bool,
    png_target: bool,
    baseline: &MetadataMap,
    makernote_block: &dyn Fn() -> MakerNoteCensus,
) -> Result<String> {
    if tag.contains(':') {
        return Ok(tag.to_string());
    }
    let Some(name) = plain_name(tag) else {
        return Err(refuse(
            tag,
            "an ungrouped name must be a plain tag name; name its group explicitly",
        ));
    };
    if !exiftool_tag_exists(name) {
        return Err(refuse(tag, format!("Tag '{tag}' is not defined")));
    }
    if !exif_ifd0_target {
        return Err(refuse(
            tag,
            "oxidex resolves an ungrouped tag name only where IFD0 is Exif::Main \
             (JPEG and TIFF-structured files); name the group explicitly \
             (for example -PDF:Title=)",
        ));
    }
    if png_target && png_prefers_text(name) {
        return Err(refuse(
            tag,
            "ExifTool writes this name to a PNG as a PNG text tag; name the group \
             explicitly (-PNG:<Name>= for the text chunk, -IFD0:<Name>= for EXIF)",
        ));
    }
    let candidates: Vec<&StaticNativeLookupCandidate> = SET_NEW_VALUE_LOOKUP
        .iter()
        .filter(|candidate| candidate.name.eq_ignore_ascii_case(name))
        .collect();
    let exif: Vec<&StaticNativeLookupCandidate> = candidates
        .iter()
        .copied()
        .filter(|candidate| is_exif_main(candidate))
        .collect();
    let [candidate] = exif.as_slice() else {
        return Err(refuse(
            tag,
            if exif.is_empty() {
                "ExifTool writes this tag to a group other than EXIF, which oxidex \
                 cannot write in this file; name the group explicitly"
            } else {
                "the name has more than one EXIF field; name the group explicitly"
            },
        ));
    };
    if candidate.writable.is_none() || candidate.permanent {
        return Err(refuse(tag, "the EXIF field is not writable by name"));
    }
    let id = match candidate.raw_id.strip_prefix("0x") {
        Some(hex) => u16::from_str_radix(hex, 16).ok(),
        None => candidate.raw_id.parse().ok(),
    };
    let table = crate::exiftool_tables::find_ifd_table("Exif", "Main");
    let field = match (id, table) {
        (Some(id), Some(table)) if table.variant_group(id).is_none() => table
            .tag(id)
            .filter(|field| field.name.eq_ignore_ascii_case(name)),
        _ => None,
    };
    let Some(field) = field else {
        return Err(refuse(tag, "the EXIF field is not transcribed as one tag"));
    };
    // `Protected` is not refused: the transcription's flag is any nonzero
    // `Protected`, and the command line sets every named tag with
    // `Protected => 1` (exiftool 13.59:1771), so only bit 0x02 ("protected")
    // would stop it (Writer.pl:735-747). Every `Exif::Main` entry with bit
    // 0x02 -- 0x0111, 0x0117, 0x014a, 0x0201, 0x0202 -- is a variant group
    // or unwritable, refused above (pinned by
    // `protected_2_exif_fields_never_reach_the_bare_route`). Pinned 13.59
    // writes `-CalibrationIlluminant1#=20` (`Protected => 1`) to
    // `[IFD0] CalibrationIlluminant1` on t/images/Canon.jpg.
    if field.flags.avoid || field.subdir.is_some() {
        return Err(refuse(
            tag,
            "the EXIF field is Avoid or a SubDirectory, which this route does \
             not model; name the group explicitly",
        ));
    }
    let group = candidate.write_group.unwrap_or(EXIF_MAIN_WRITE_GROUP);
    if !matches!(group, "IFD0" | "ExifIFD") {
        return Err(refuse(
            tag,
            format!("ExifTool writes it to {group}, which oxidex cannot write"),
        ));
    }
    ensure_not_also_updated(
        tag,
        &format!("{group}:{}", field.name),
        baseline,
        makernote_block,
    )?;
    Ok(format!("{group}:{}", field.name))
}

/// The `Exif::Main` row a grouped (or bare) EXIF `key` addresses when the
/// table carries more than one row of that name -- chosen by the request's
/// destination directory, as ExifTool's `SetNewValue` chooses it, never by
/// the documentation registry's id or by first-row order (PR #959,
/// `4112816750`). `Exif::Main` repeats `ChromaticAberrationCorrection` and
/// `DistortionCorrection`: the Sony ARW rows `0x7034`/`0x7036` carry
/// `WriteGroup => 'SubIFD'` and an `Off`/`Auto` PrintConv, while the Exif 3.1
/// rows `0xa410`/`0xa40f` default to ExifIFD with `No`/`Yes`. Pinned 13.59
/// writes `-ExifIFD:ChromaticAberrationCorrection=Yes` (and `-IFD0:`,
/// `-EXIF:`, bare) to `0xa410` and refuses `=Auto` as not in PrintConv.
///
/// A `SubIFD*` destination selects the rows whose SetNewValue write group is
/// `SubIFD`; any other EXIF-family destination (bare, `EXIF`, `IFD0`,
/// `ExifIFD`, `IFD1`, `InteropIFD`) selects the rest. `None` when the name
/// has at most one row (nothing to choose), when the group is not an EXIF
/// directory, or when the destination still leaves more than one row -- the
/// caller then keeps its own single-row resolution.
pub(crate) fn exif_main_row_for_destination(
    key: &str,
) -> Option<&'static crate::exiftool_tables::IfdTag> {
    let (group, name) = match key.rsplit_once(':') {
        Some((group, name)) => (Some(group), name),
        None => (None, key),
    };
    let to_subifd = match group {
        None => false,
        Some(group) if group.len() >= 6 && group[..6].eq_ignore_ascii_case("SubIFD") => true,
        Some(group)
            if ["EXIF", "IFD0", "ExifIFD", "IFD1", "InteropIFD"]
                .iter()
                .any(|known| known.eq_ignore_ascii_case(group)) =>
        {
            false
        }
        Some(_) => return None,
    };
    let table = crate::exiftool_tables::find_ifd_table("Exif", "Main")?;
    let rows: Vec<&'static crate::exiftool_tables::IfdTag> = table
        .tags
        .iter()
        .filter(|row| row.name.eq_ignore_ascii_case(name))
        .collect();
    if rows.len() < 2 {
        return None;
    }
    let write_group = |row: &crate::exiftool_tables::IfdTag| {
        SET_NEW_VALUE_LOOKUP
            .iter()
            .find(|candidate| {
                is_exif_main(candidate)
                    && candidate.name.eq_ignore_ascii_case(row.name)
                    && match candidate.raw_id.strip_prefix("0x") {
                        Some(hex) => u16::from_str_radix(hex, 16).ok(),
                        None => candidate.raw_id.parse().ok(),
                    } == Some(row.id)
            })
            .and_then(|candidate| candidate.write_group)
            .unwrap_or(EXIF_MAIN_WRITE_GROUP)
    };
    let mut chosen = rows
        .into_iter()
        .filter(|row| write_group(row).eq_ignore_ascii_case("SubIFD") == to_subifd);
    match (chosen.next(), chosen.next()) {
        (Some(row), None) => Some(row),
        _ => None,
    }
}

/// The sole transcribed row would be placed into an explicit EXIF directory
/// although its source-captured `WriteGroup` is SubIFD. In 12.64,
/// `ChromaticAberrationCorrection` has only the Sony 0x7034 row; the Exif 3.1
/// ExifIFD row 0xa410 appears later. The JPEG/TIFF writers do not write a
/// SubIFD and must not create 0x7034 in ExifIFD just because the registry
/// knows that tag id.
fn lone_subifd_row_id_for_destination(
    key: &str,
    rows: &[&crate::exiftool_tables::IfdTag],
) -> Option<u16> {
    let (group, _) = key.rsplit_once(':')?;
    if !["EXIF", "IFD0", "ExifIFD", "IFD1", "InteropIFD"]
        .iter()
        .any(|directory| directory.eq_ignore_ascii_case(group))
    {
        return None;
    }
    let [only] = rows else { return None };
    SET_NEW_VALUE_LOOKUP
        .iter()
        .find(|candidate| {
            is_exif_main(candidate)
                && candidate.name.eq_ignore_ascii_case(only.name)
                && match candidate.raw_id.strip_prefix("0x") {
                    Some(hex) => u16::from_str_radix(hex, 16).ok(),
                    None => candidate.raw_id.parse().ok(),
                } == Some(only.id)
        })
        .filter(|candidate| candidate.write_group == Some("SubIFD"))
        .map(|_| only.id)
}

/// Refuse a wrong EXIF address, either a sole row physically owned by SubIFD
/// or a destination-selected duplicate row with a different registry id.
/// Both cases are value-independent and leave the file untouched.
pub(crate) fn exif_row_misaddressed(key: &str) -> Option<ExifToolError> {
    let row = exif_main_row_for_destination(key);
    if row.is_none() {
        let (group, name) = key.rsplit_once(':')?;
        let table = crate::exiftool_tables::find_ifd_table("Exif", "Main")?;
        let rows: Vec<_> = table
            .tags
            .iter()
            .filter(|candidate| candidate.name.eq_ignore_ascii_case(name))
            .collect();
        if let Some(id) = lone_subifd_row_id_for_destination(key, &rows) {
            return Some(refuse(
                key,
                format!(
                    "ExifTool's only transcribed row is tag 0x{id:04x} in SubIFD; \
                     oxidex cannot write it into {group}"
                ),
            ));
        }
    }
    let row = row?;
    let registry_id = match crate::tag_db::tag_registry::get_tag_descriptor(key)?.id() {
        crate::core::TagId::Numeric(id) => *id,
        crate::core::TagId::Named(_) => return None,
    };
    (registry_id != row.id).then(|| {
        refuse(
            key,
            format!(
                "ExifTool writes this name as tag 0x{:04x} here, but oxidex's tag \
                 registry addresses it as 0x{registry_id:04x}; refusing rather than \
                 write the wrong tag",
                row.id
            ),
        )
    })
}

/// Refuses an ungrouped `tag` resolved to the EXIF address `key` when
/// ExifTool's write of it would not be `key` alone.
///
/// `SetNewValue` creates the tag in its highest-priority group and writes
/// every other candidate "if tag exists" (Writer.pl:613-825, verbose `-v2`:
/// `Writing Canon:WhiteBalance if tag exists`). Measured against pinned
/// 13.59 (evidence `multigroup-write/exp/`): `-WhiteBalance#=1` on
/// t/images/Canon.jpg writes `[ExifIFD]` *and* `[Canon] WhiteBalance`; on
/// Nikon.jpg it writes `[ExifIFD]` and `[Nikon] WhiteBalance` -- a row
/// oxidex's reader does not surface; `-Flash#=16` writes the writable
/// `Composite:Flash`, whose `WriteAlso` creates an XMP-exif Flash structure
/// beside `[ExifIFD] Flash`; on t/images/ExifTool.jpg `-ColorSpace#=2`
/// also creates `[MIE-Image] ColorSpace` (MIE tables are `PREFERRED`).
/// oxidex writes only EXIF, so each of these is refused -- the whole
/// request, as the write transaction applies all of a request or none of
/// it -- naming what ExifTool would also write:
///
/// 1. a writable `File`/`Composite` candidate (ExifTool writes that tag,
///    priority 500, with its `WriteAlso` targets);
/// 2. a `MakerNotes` candidate the file's maker note could hold
///    ([`makernote_may_hold`]);
/// 3. an `MIE` candidate where the file carries MIE;
/// 4. a same-named row of the file in a group one of the name's candidates
///    lives in (`XMP-exif`, `IPTC`, `CIFF`, ...). A same-named row no
///    candidate can be (`[SPIFF] ColorSpace`, `[PictureInfo] Flash`) is one
///    ExifTool leaves alone (pinned 13.59 on ExifTool.jpg) and does not
///    refuse.
///
/// A name the candidate capture ([`SET_NEW_VALUE_LOOKUP`]) does not hold
/// (the GPS names) keeps the earlier rule: any same-named row outside the
/// EXIF directories refuses.
pub(crate) fn ensure_not_also_updated(
    tag: &str,
    key: &str,
    baseline: &MetadataMap,
    makernote_block: &dyn Fn() -> MakerNoteCensus,
) -> Result<()> {
    let name = key.rsplit(':').next().unwrap_or(key);
    let use_exif_only = |what: String| {
        refuse(
            tag,
            format!("{what}; use -{key}= to change only the EXIF field"),
        )
    };
    // Every writable candidate as (family 0, family 1): the SetNewValue
    // address capture, or -- for the `GPS::Main` names it holds no rows for
    // (`GPSDateStamp`) -- the pinned `FindTagInfo` capture of those names.
    // A name neither capture holds is refused: no candidate is ever
    // inferred from the rows the reader happened to surface.
    let lowered = name.to_ascii_lowercase();
    let mut everywhere: Vec<(&str, &str)> = SET_NEW_VALUE_LOOKUP
        .iter()
        .filter(|candidate| candidate.name.eq_ignore_ascii_case(name))
        .map(|candidate| {
            (
                family0(candidate).unwrap_or(""),
                family1(candidate).unwrap_or(""),
            )
        })
        .collect();
    if everywhere.is_empty() {
        everywhere = super::generated_makernote_groups::GPS_NAME_CANDIDATES
            .iter()
            .filter(|(candidate, _, _)| *candidate == lowered)
            .map(|(_, g0, g1)| (*g0, *g1))
            .collect();
    }
    if everywhere.is_empty() {
        return Err(refuse(
            tag,
            "oxidex holds no capture of the groups ExifTool writes this name to; name the \
             group explicitly",
        ));
    }
    // ExifTool writes the tag in every EXIF APP1 of a JPEG (13.59:
    // `Warning: Multiple APP1 EXIF records`, then both written); oxidex
    // writes one.
    let census = makernote_block();
    if census.blocks > 1 && !census.deletions.exif_blocks {
        return Err(use_exif_only(format!(
            "the file carries {} EXIF blocks, each of which ExifTool writes {name} to, \
             and oxidex writes one",
            if census.blocks == usize::MAX {
                "several".to_string()
            } else {
                census.blocks.to_string()
            }
        )));
    }
    let others: Vec<(&str, &str)> = everywhere
        .into_iter()
        .filter(|(g0, _)| *g0 != "EXIF")
        .collect();
    if let Some(group) = others
        .iter()
        .map(|(g0, _)| *g0)
        .find(|group| matches!(*group, "File" | "Composite"))
    {
        return Err(use_exif_only(format!(
            "ExifTool writes the {group}:{name} tag for this name (and every tag it \
             writes along with it), which oxidex cannot write"
        )));
    }
    if let Some(reason) = makernote_may_hold(name, baseline, &|| census) {
        return Err(use_exif_only(reason));
    }
    if others.iter().any(|(g0, _)| *g0 == "MIE")
        && let Some(mie) = mie_row(baseline)
    {
        return Err(use_exif_only(format!(
            "the file carries MIE ({mie}), where ExifTool also writes {name} (its MIE \
             tables are PREFERRED), which oxidex cannot write"
        )));
    }
    // A same-named row a non-maker-note candidate can be (`XMP-exif`,
    // `IPTC`): the reader surfaces these, unlike maker-note rows, which
    // `makernote_may_hold` decides from the complete capture.
    if let Some((existing, _)) = baseline.keyed_occurrences().find(|(existing, occurrence)| {
        let existing_name = existing.rsplit(':').next().unwrap_or(existing);
        existing_name.eq_ignore_ascii_case(name)
            && &*occurrence.group0 != "EXIF"
            && &*occurrence.group0 != "MakerNotes"
            && !NOT_ALSO_UPDATED.contains(&occurrence.group1.as_ref())
            && others.iter().any(|&(g0, g1)| {
                g0 != "MakerNotes"
                    && (candidate_lives_in(g0, g1, &occurrence.group1)
                        || (!occurrence.group0.is_empty()
                            && candidate_lives_in(g0, g1, &occurrence.group0)))
            })
    }) {
        return Err(use_exif_only(format!(
            "ExifTool would also update the existing {existing}, which oxidex cannot write"
        )));
    }
    Ok(())
}

/// Whether every group pinned ExifTool writes the bare `name` to is one
/// whose absence oxidex can prove: an EXIF directory (its block scan), a
/// maker note (`makernote_may_hold`), or MIE in a file without any
/// ([`mie_row`]). `false` for a name neither capture holds.
pub(crate) fn candidates_are_provable(name: &str, baseline: &MetadataMap) -> bool {
    let lowered = name.to_ascii_lowercase();
    let mut groups: Vec<&str> = SET_NEW_VALUE_LOOKUP
        .iter()
        .filter(|candidate| candidate.name.eq_ignore_ascii_case(name))
        .map(|candidate| family0(candidate).unwrap_or(""))
        .collect();
    if groups.is_empty() {
        groups = super::generated_makernote_groups::GPS_NAME_CANDIDATES
            .iter()
            .filter(|(candidate, _, _)| *candidate == lowered)
            .map(|(_, g0, _)| *g0)
            .collect();
    }
    !groups.is_empty()
        && groups.iter().all(|group| match *group {
            "EXIF" | "MakerNotes" => true,
            "MIE" => mie_row(baseline).is_none(),
            _ => false,
        })
}

/// A row of the file's MIE metadata (family-0 `MIE`, or a family-1 `MIE*`
/// group), if it carries any.
pub(crate) fn mie_row(baseline: &MetadataMap) -> Option<&str> {
    let mie = |group: &str| {
        group
            .get(..3)
            .is_some_and(|g| g.eq_ignore_ascii_case("MIE"))
    };
    baseline
        .keyed_occurrences()
        .find(|(key, occurrence)| {
            mie(&occurrence.group0)
                || mie(&occurrence.group1)
                || key.split_once(':').is_some_and(|(group, _)| mie(group))
        })
        .map(|(key, _)| key)
}

/// Maximum TIFF bytes copied or read while proving absence across every MIE
/// EXIF element in one file. A repeated offset can otherwise turn a small
/// trailer into many copies of the same value before a write is refused.
const MIE_CENSUS_BYTE_BUDGET: usize = 16 * 1024 * 1024;

/// One EXIF element of a MIE trailer. Decoding its rows and scanning its
/// structure are both deferred until a deletion needs that proof.
#[derive(Debug)]
pub(crate) struct MieExifBlock<'a> {
    pub tiff: std::borrow::Cow<'a, [u8]>,
    rows: std::cell::OnceCell<Option<MetadataMap>>,
    scan: std::cell::OnceCell<Option<super::exif_surgical::ExifScan>>,
    notes: std::cell::OnceCell<MakerNoteCensus>,
    budget: std::rc::Rc<std::cell::Cell<usize>>,
}

impl<'a> MieExifBlock<'a> {
    fn new(tiff: std::borrow::Cow<'a, [u8]>, budget: std::rc::Rc<std::cell::Cell<usize>>) -> Self {
        Self {
            tiff,
            rows: std::cell::OnceCell::new(),
            scan: std::cell::OnceCell::new(),
            notes: std::cell::OnceCell::new(),
            budget,
        }
    }

    fn rows(&self) -> Option<&MetadataMap> {
        self.rows
            .get_or_init(|| {
                let reader = MieBudgetReader {
                    bytes: &self.tiff,
                    budget: &self.budget,
                    exhausted: std::cell::Cell::new(false),
                };
                let rows = crate::core::metadata_map::file_rows(|| {
                    crate::core::operations::parse_tiff_metadata(&reader)
                });
                // The TIFF parser may skip an entry on a failed read. A
                // budget failure must never become an empty absence proof.
                if reader.exhausted.get() {
                    None
                } else {
                    rows.ok()
                }
            })
            .as_ref()
    }

    fn scan(&self) -> Option<&super::exif_surgical::ExifScan> {
        self.scan
            .get_or_init(|| {
                super::exif_surgical::scan_entries_with_magics_budgeted(
                    &self.tiff,
                    super::exif_surgical::EXIF_BLOCK_MAGICS,
                    &self.budget,
                )
                .ok()
            })
            .as_ref()
    }

    fn note_census(&self) -> MakerNoteCensus {
        *self.notes.get_or_init(|| {
            super::exif_surgical::makernote_census_budgeted(
                &[self.tiff.as_ref()],
                super::exif_surgical::EXIF_BLOCK_MAGICS,
                &self.budget,
            )
        })
    }
}

struct MieBudgetReader<'a, 'b> {
    bytes: &'a [u8],
    budget: &'b std::cell::Cell<usize>,
    exhausted: std::cell::Cell<bool>,
}

impl crate::core::FileReader for MieBudgetReader<'_, '_> {
    fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
        let start = usize::try_from(offset)
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::UnexpectedEof))?;
        let end = start
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(std::io::ErrorKind::UnexpectedEof)?;
        if length > self.budget.get() {
            self.exhausted.set(true);
            return Err(std::io::ErrorKind::OutOfMemory.into());
        }
        self.budget.set(self.budget.get() - length);
        Ok(&self.bytes[start..end])
    }

    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
}

/// What a file carries of MIE, as [`ensure_no_mie_copy`] asks it: taken once
/// per request, or per write transaction, from the file's bytes
/// (`core::operations::mie_census`), never once per requested tag (a walk
/// of the trailer chain, and for a JPEG a scan for its EOI, are
/// file-sized).
#[derive(Debug)]
pub(crate) enum MieCensus<'a> {
    /// A file ExifTool reads no MIE trailer after (a PNG's IEND ends it;
    /// a `.mie` document is its own writer's to refuse): the reader's MIE
    /// rows alone say whether it carries MIE, whose EXIF is then unseen.
    NotATrailerCarrier,
    /// A JPEG or TIFF-structured file whose trailer chain holds no MIE
    /// trailer ([`crate::parsers::mie::trailer_exif`]).
    NoTrailer,
    /// MIE trailers, none of which holds MIE-Meta's `EXIF`.
    Absent,
    /// Every MIE-Meta `EXIF` element of the file's MIE trailers.
    Held(Vec<MieExifBlock<'a>>),
    /// MIE trailers that may hold an `EXIF` element the walk cannot see.
    Unknown,
}

impl<'a> MieCensus<'a> {
    /// The MIE-Meta EXIF blocks the census holds (none unless
    /// [`MieCensus::Held`]).
    #[cfg(test)]
    fn blocks(&self) -> &[MieExifBlock<'a>] {
        match self {
            Self::Held(blocks) => blocks,
            _ => &[],
        }
    }

    /// The census of a file ExifTool reads MIE trailers after (a JPEG,
    /// with `jpeg_trailer_start` the byte after its EOI, or a
    /// TIFF-structured file): each EXIF block decoded once.
    pub(crate) fn of_trailers(file: &'a [u8], jpeg_trailer_start: Option<usize>) -> Self {
        use crate::parsers::mie::{MieExif, trailer_exif};
        match trailer_exif(file, jpeg_trailer_start) {
            None => Self::NoTrailer,
            Some(MieExif::Absent) => Self::Absent,
            Some(MieExif::Unknown) => Self::Unknown,
            Some(MieExif::Held(blocks)) => {
                let budget = std::rc::Rc::new(std::cell::Cell::new(MIE_CENSUS_BYTE_BUDGET));
                Self::Held(
                    blocks
                        .into_iter()
                        .map(|tiff| {
                            MieExifBlock::new(std::borrow::Cow::Borrowed(tiff), budget.clone())
                        })
                        .collect(),
                )
            }
        }
    }
}

/// The pinned MakerNotes::Main entry named by a grouped request. ExifTool
/// accepts these physical names under MakerNotes, ExifIFD, and EXIF, with a
/// numeric `#` suffix; CIFF is a separate APP0 root, not an EXIF entry.
/// The existing generic-MakerNotes MIE path used the literal name, including
/// CIFF; retain that lookup for its set behavior while extending the two
/// EXIF aliases and the main-file guard with the source-derived root names.
fn named_makernote_root(
    key: &str,
    legacy_generic_mie: bool,
) -> Option<&'static super::generated_makernote_groups::MakerNoteRoot> {
    let (group, name) = key.split_once(':')?;
    if !["MakerNotes", "ExifIFD", "EXIF"]
        .iter()
        .any(|known| known.eq_ignore_ascii_case(group))
    {
        return None;
    }
    let name = if legacy_generic_mie {
        name
    } else {
        name.strip_suffix('#').unwrap_or(name)
    };
    super::generated_makernote_groups::MAKERNOTE_ROOTS
        .iter()
        .find(|root| {
            (legacy_generic_mie || root.entry != "CIFF") && root.entry.eq_ignore_ascii_case(name)
        })
}

/// Refuses a request -- grouped or not -- resolved to the EXIF-family `key`
/// in a file that carries MIE, wherever pinned 13.59 also edits MIE-Meta's
/// own EXIF copy, which oxidex does not write. `mie` is the file's
/// [`MieCensus`].
///
/// - A set is written into MIE's EXIF as well, which ExifTool creates when
///   the trailer has none (`-IFD0:Artist=x` and `-IFD0:CalibrationIlluminant1#=20`
///   on t/images/ExifTool.jpg, `-v2`: `Creating EXIF` under `MIE1-Meta1`,
///   and two `[IFD0]` rows after): always refused.
/// - A deletion (a tag, or `<group>:All`) is applied to MIE's EXIF where
///   that holds the tag (`-IFD0:Artist=` on the output above shrinks the
///   trailer from 188 to 90 bytes) and leaves a trailer without it as it was
///   (ExifTool.jpg's own, which has no EXIF; `-IFD0:Software=` on the 188):
///   refused unless every MIE-Meta EXIF block is proven not to hold the
///   tag. An EXIF block the scan cannot read proves nothing: 13.59 reads
///   one whose TIFF magic is not 42 anyway and deletes from it.
///   - `<group>:All`, in any spelling (`-gps:all=`), is decided by the
///     group it names (`exif_surgical::group_removal`): `MakerNotes:All`
///     deletes a block's maker note -- an Adobe `DNGPrivateData` `MakN`
///     note too (Writer.jpg + a MIE holding t/images/DNG.dng:
///     13967 -> 5593 bytes) -- except a 0x927c note ExifTool files under
///     ExifIFD (`MakerNoteUnknownText`, `MakerNoteUnknownBinary`,
///     `MakerNoteSamsung1a`), which it leaves (`1 image files unchanged`),
///     as the block's own decoded rows tell.
///   - A named tag, by the no-op scan every EXIF deletion is decided by
///     (`exif_surgical::exif_request_is_no_op`) and the block's decoded rows.
///   - A bare name is deleted from MIE's maker note as well, where that may
///     hold it (`makernote_may_hold` on the block's rows, as for the main
///     EXIF's note; Writer.jpg + a MIE holding Nikon.jpg's EXIF, which has
///     no ExifIFD WhiteBalance: `-WhiteBalance=` resolves to
///     `ExifIFD:WhiteBalance` and 13.59 deletes `[Nikon] WhiteBalance`,
///     1747 -> 1731 bytes).
/// - A maker-note tag (`-MakerNotes:OwnerName=`, `-Canon:OwnerName=`, set
///   or deletion) is edited in MIE's EXIF where that carries a maker note
///   (Writer.jpg with a MIE holding Canon.jpg's EXIF: `[Canon] OwnerName`
///   emptied, 2490 -> 2496 bytes, although the main EXIF has no maker
///   note); ExifTool creates no maker note, so a MIE without one is left
///   alone.
pub(crate) fn ensure_no_mie_copy(
    tag: &str,
    key: &str,
    baseline: &MetadataMap,
    removal: bool,
    mie: &MieCensus<'_>,
) -> Result<()> {
    use super::exif_surgical::{
        GroupRemoval, census_walk_is_complete, exif_named_removal_is_no_op_in_scan,
        group_directory_walked, group_has_content, group_removal, has_dng_makernote,
        has_unwalked_exif_directory, is_makernote_group,
    };
    let group = key.split_once(':').map_or("", |(group, _)| group);
    let removed_group = if removal { group_removal(key) } else { None };
    // Generic MakerNotes keeps its original root lookup for sets and
    // removals. For the two EXIF aliases, physical-root absence is only a
    // deletion proof; sets take the unconditional EXIF-set refusal below.
    let generic_makernotes = group.eq_ignore_ascii_case("MakerNotes");
    let physical_root = if generic_makernotes {
        named_makernote_root(key, true)
    } else if removal {
        named_makernote_root(key, false)
    } else {
        None
    };
    let makernote = removed_group.is_none()
        && (physical_root.is_some() || generic_makernotes || is_makernote_group(group));
    let exif = matches!(
        group,
        "IFD0" | "IFD1" | "ExifIFD" | "GPS" | "InteropIFD" | "EXIF" | "SubIFD"
    ) || super::exif_surgical::chain_key_dir(key).is_some()
        || removed_group.is_some();
    if !exif && !makernote {
        return Ok(());
    }
    let (mie_name, blocks): (String, Option<&[MieExifBlock<'_>]>) = match mie {
        MieCensus::NoTrailer => return Ok(()),
        MieCensus::NotATrailerCarrier => match mie_row(baseline) {
            Some(row) => (row.to_string(), None),
            None => return Ok(()),
        },
        MieCensus::Absent => ("a MIE trailer".to_string(), Some(&[])),
        MieCensus::Held(blocks) => ("a MIE trailer".to_string(), Some(blocks.as_slice())),
        MieCensus::Unknown => ("a MIE trailer".to_string(), None),
    };
    if makernote {
        let untouched = blocks.is_some_and(|blocks| {
            if physical_root.is_some() || generic_makernotes {
                // A named generic request selects only the pinned maker-note
                // candidate groups for this name. A Nikon block cannot hold
                // Canon-only OwnerName, but may hold WhiteBalance even when
                // the reader did not expose that row.
                let name = key.rsplit_once(':').map_or(key, |(_, name)| name);
                blocks.iter().all(|block| {
                    let Some(rows) = block.rows() else {
                        return false;
                    };
                    let note = || block.note_census();
                    if let Some(root) = physical_root {
                        let census = note();
                        // A named MakerNotes::Main entry selects the physical
                        // note root. Writable child-field candidates do not
                        // inventory these root names (MakerNoteNikon, for
                        // example, has only an AdobeDNG candidate).
                        if census.notes == 1
                            && !census.uncertain_outside_ifd1
                            && !census.uncertain_ifd1
                            && census.identified_single_root.is_some()
                        {
                            census.identified_single_root != Some(root.entry)
                        } else if census.notes == 0
                            && !census.uncertain_outside_ifd1
                            && !census.uncertain_ifd1
                        {
                            true
                        } else if root.group.is_empty() {
                            false
                        } else {
                            !makernote_group_may_hold(name, root.group, rows, &note)
                        }
                    } else {
                        makernote_may_hold(name, rows, &note).is_none()
                    }
                })
            } else {
                // A named vendor only selects roots whose source-derived
                // closure reaches that group. A proven Nikon note cannot
                // contain Canon:OwnerName, while an unidentified note may.
                let name = key.rsplit_once(':').map_or(key, |(_, name)| name);
                blocks.iter().all(|block| {
                    let Some(rows) = block.rows() else {
                        return false;
                    };
                    !makernote_group_may_hold(name, group, rows, &|| block.note_census())
                })
            }
        });
        if untouched {
            return Ok(());
        }
        return Err(refuse(
            tag,
            format!(
                "the file carries MIE ({mie_name}) whose EXIF directory holds a maker note (or \
                 may), where ExifTool also edits {key}, which oxidex cannot write"
            ),
        ));
    }
    if !removal {
        return Err(refuse(
            tag,
            format!(
                "the file carries MIE ({mie_name}), whose EXIF directory ExifTool also writes \
                 {key} to (creating it), which oxidex cannot write"
            ),
        ));
    }
    let untouched = blocks.is_some_and(|blocks| {
        blocks.iter().all(|block| {
            let Some(scan) = block.scan() else {
                return false;
            };
            if !census_walk_is_complete(&block.tiff, scan.byte_order) {
                return false;
            }
            let Some(rows) = block.rows() else {
                return false;
            };
            let unwalked_child = scan
                .entries
                .iter()
                .any(|entry| has_unwalked_exif_directory(entry.ifd, entry.tag_id))
                || scan
                    .ifd1_next
                    .as_ref()
                    .is_some_and(|chain| chain.refusal.is_some());
            match removed_group {
                // The block's decoded rows tell a note ExifTool files under
                // ExifIFD; a DNGPrivateData maker note is deleted as well.
                Some(GroupRemoval::MakerNotes) => {
                    !group_has_content(GroupRemoval::MakerNotes, scan, rows)
                        && !has_dng_makernote(scan)
                }
                // An empty directory the group names goes too.
                Some(group) => {
                    // The surgical scan intentionally does not descend into
                    // TIFF SubIFDs or unmodelled children in the IFD2 chain.
                    // Native ExifTool can apply a nested GPS or
                    // ExifIFD clear there, so their absence cannot be proven
                    // from the directories this scan did walk.
                    let may_target_child = matches!(
                        group,
                        GroupRemoval::ExifIfd | GroupRemoval::Gps | GroupRemoval::Interop
                    );
                    !(may_target_child && unwalked_child)
                        && !group_has_content(group, scan, rows)
                        && !group_directory_walked(group, scan)
                }
                None => {
                    // A grouped named deletion can reach the same child as
                    // its group clear. An unread MIE SubIFD or IFD2 cannot
                    // make a missing decoded row a proof of absence.
                    let may_target_child = ["ExifIFD", "GPS", "InteropIFD", "EXIF", "SubIFD"]
                        .iter()
                        .any(|candidate| group.eq_ignore_ascii_case(candidate));
                    !(may_target_child && unwalked_child)
                        && !rows.contains_key(key)
                        && exif_named_removal_is_no_op_in_scan(&block.tiff, scan, key)
                }
            }
        })
    });
    // A bare name is deleted from every group that holds it: MIE's maker
    // note too, where that may hold it -- decided as for the main EXIF's
    // own note (`ensure_not_also_updated`), from the block's decoded rows.
    if untouched
        && !tag.contains(':')
        && let Some(reason) = blocks.unwrap_or_default().iter().find_map(|block| {
            let Some(rows) = block.rows() else {
                return Some(
                    "the MIE EXIF block could not be decoded within the census budget".to_string(),
                );
            };
            makernote_may_hold(key.rsplit(':').next().unwrap_or(key), rows, &|| {
                block.note_census()
            })
        })
    {
        return Err(refuse(
            tag,
            format!("the file carries MIE ({mie_name}) with its own EXIF, and {reason}"),
        ));
    }
    if untouched {
        return Ok(());
    }
    Err(refuse(
        tag,
        format!(
            "the file carries MIE ({mie_name}) whose EXIF directory holds {key} (or may), \
             which ExifTool also deletes and oxidex cannot write"
        ),
    ))
}

/// Whether a row the reader files under family-1 `group` can be a candidate
/// of family-0 `g0` and family-1 `g1`: either group, or an `XMP-*` group of
/// an XMP candidate.
fn candidate_lives_in(g0: &str, g1: &str, group: &str) -> bool {
    g0.eq_ignore_ascii_case(group)
        || g1.eq_ignore_ascii_case(group)
        || (g0 == "XMP"
            && group
                .get(..4)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("XMP-")))
}

/// Refuses a request naming a maker-note entry itself --
/// `-ExifIFD:MakerNoteCanon=`, `-EXIF:MakerNoteCanon=`,
/// `-MakerNotes:MakerNoteCanon=` (a `MakerNotes::Main` entry name) -- in a
/// file that carries a maker note: pinned 13.59 deletes the note such a
/// name selects (t/images/Canon.jpg: all three delete its MakerNoteCanon),
/// which oxidex neither does nor can tell apart from a note the name does
/// not select, and which the no-op check (reading rows, which name no such
/// entry) used to report `unchanged`. A file with no maker note is left to
/// that check (13.59: `unchanged`).
pub(crate) fn ensure_makernote_entry_not_named(
    tag: &str,
    key: &str,
    makernote_block: &dyn Fn() -> MakerNoteCensus,
) -> Result<()> {
    let census = named_makernote_root(key, false).map(|_| makernote_block());
    if census.is_some_and(|census| {
        census.notes > 0 || census.uncertain_outside_ifd1 || census.uncertain_ifd1
    }) {
        return Err(refuse(
            tag,
            "it names a maker-note entry, which ExifTool deletes or replaces whole where \
             the file's note is that entry; oxidex does not edit a maker note by name \
             (use -MakerNotes:All= to delete it)",
        ));
    }
    Ok(())
}

/// The family scope of the unit-bearing ExifIFD tags. Creation resolves to
/// ExifIFD, but an unqualified or EXIF deletion still names every directory.
pub(crate) fn unit_suffix_family_key(tag: &str) -> Option<String> {
    let (group, leaf) = tag
        .rsplit_once(':')
        .map_or((None, tag), |(g, n)| (Some(g), n));
    if group.is_some_and(|g| !g.eq_ignore_ascii_case("EXIF")) {
        return None;
    }
    [
        "FocalLength",
        "FocalLengthIn35mmFormat",
        "SubjectDistance",
        "AmbientTemperature",
    ]
    .into_iter()
    .find(|name| leaf.eq_ignore_ascii_case(name))
    .map(|name| format!("EXIF:{name}"))
}

/// Why pinned ExifTool may also write `name` in the file's maker note, or
/// `None` when it provably cannot: the name has no writable `MakerNotes`
/// candidate ([`MAKERNOTE_CANDIDATES`], the pinned `FindTagInfo` over every
/// writable name -- never the reader's rows), or the file carries no maker
/// note that bears tags, or the request deletes it, or no candidate's
/// family-1 group is one the file's maker note can reach.
///
/// ExifTool edits a maker-note tag only where the note already holds it,
/// but oxidex's maker-note decoders do not surface every row ExifTool reads
/// (t/images/Nikon.jpg: no `[Nikon] WhiteBalance` row, which pinned 13.59
/// edits), so holding is decided from tables, not rows: the reader's
/// maker-note rows name the family-1 groups of the note it decoded, each
/// such group selects every maker-note root whose closure reaches it
/// ([`MAKERNOTE_ROOTS`], captured from the pinned `MakerNotes::Main` and
/// every `SubDirectory` below it), and the note may hold `name` when a
/// candidate's family-1 group is in the union of those closures. A file
/// whose EXIF carries a tag-bearing maker note (`makernote_block`'s
/// [`MakerNoteCensus`], counted per EXIF block) the reader decoded no row
/// of -- or more than one, whose rows cannot be told apart -- may hold any.
///
/// [`MAKERNOTE_ROOTS`]: super::generated_makernote_groups::MAKERNOTE_ROOTS
/// [`MAKERNOTE_CANDIDATES`]: super::generated_makernote_groups::MAKERNOTE_CANDIDATES
pub(crate) fn makernote_may_hold(
    name: &str,
    baseline: &MetadataMap,
    makernote_block: &dyn Fn() -> MakerNoteCensus,
) -> Option<String> {
    use super::generated_makernote_groups::MAKERNOTE_CANDIDATES;
    let lowered = name.to_ascii_lowercase();
    let groups: &[&str] = MAKERNOTE_CANDIDATES
        .binary_search_by(|(candidate, _)| (*candidate).cmp(lowered.as_str()))
        .map_or(&[], |at| MAKERNOTE_CANDIDATES[at].1);
    makernote_may_hold_in_groups(name, groups, baseline, makernote_block)
}

pub(crate) fn makernote_group_may_hold(
    name: &str,
    group: &str,
    baseline: &MetadataMap,
    makernote_block: &dyn Fn() -> MakerNoteCensus,
) -> bool {
    makernote_may_hold_in_groups(name, &[group], baseline, makernote_block).is_some()
}

fn makernote_may_hold_in_groups(
    name: &str,
    groups: &[&str],
    baseline: &MetadataMap,
    makernote_block: &dyn Fn() -> MakerNoteCensus,
) -> Option<String> {
    use super::generated_makernote_groups::MAKERNOTE_ROOTS;
    if groups.is_empty() {
        return None;
    }
    let mut decoded = super::exif_surgical::makernote_row_groups(baseline);
    let census = makernote_block();
    // A request that deletes the EXIF maker note leaves no copy there to
    // edit; a CIFF segment (a separate APP0 maker-note block) survives it
    // unless the deletion is `MakerNotes:All`, which takes CIFF too.
    let deletions = census.deletions;
    let ciff = census.ciff;
    let surviving_direct = if deletions.exif_ifd_only() {
        let surviving = if deletions.ifd1 && census.surviving_exif_ifd_clear != usize::MAX {
            census
                .surviving_exif_ifd_clear
                .saturating_sub(census.ifd1_tag_bearing)
        } else {
            census.surviving_exif_ifd_clear
        };
        surviving > 0
            || census.uncertain_survivor_outside_ifd1
            || (!deletions.ifd1 && census.uncertain_survivor_ifd1)
    } else {
        false
    };
    // Rows do not identify which physical note supplied them. After an
    // ExifIFD clear, a surviving direct note must stay an unknown candidate
    // even if the removed ExifIFD note supplied a decoded vendor row.
    if surviving_direct {
        return Some(format!(
            "the file carries a maker note outside ExifIFD where ExifTool also \
             writes {name} if the note holds it, which oxidex cannot write"
        ));
    }
    if deletions.makernotes && (!ciff || deletions.ciff) {
        return None;
    }
    // An APP0 CIFF can be physically present while none of its fields is
    // surfaced. In that case no decoded row narrows its root table.
    if ciff
        && !deletions.ciff
        && !super::exif_surgical::has_ciff_rows(baseline)
        && MAKERNOTE_ROOTS.iter().any(|root| {
            root.entry == "CIFF" && groups.iter().any(|group| root.closure.contains(group))
        })
    {
        return Some(format!(
            "the file carries a CIFF APP0 maker note oxidex cannot identify, where ExifTool \
             also writes {name} if the note holds it, which oxidex cannot write"
        ));
    }
    if deletions.makernotes {
        // Only the CIFF segment's rows remain: its root's groups.
        let ciff_root = MAKERNOTE_ROOTS.iter().find(|root| root.entry == "CIFF");
        decoded.retain(|group| {
            ciff_root.is_some_and(|root| root.closure.contains(&group.as_str())) || group == "CIFF"
        });
    }
    let tag_bearing = if deletions.makernotes {
        0
    } else if deletions.ifd1 && census.tag_bearing != usize::MAX {
        census.tag_bearing.saturating_sub(census.ifd1_tag_bearing)
    } else {
        census.tag_bearing
    };
    let uncertain_note = !deletions.makernotes
        && (census.uncertain_outside_ifd1 || (!deletions.ifd1 && census.uncertain_ifd1));
    if decoded.is_empty() && tag_bearing == 0 && !uncertain_note {
        return None;
    }
    // Rows do not say which EXIF block they were decoded from, so more than
    // one tag-bearing note cannot be told apart; and the one tag-bearing
    // note must be one the reader identified -- rows only from outside it
    // say nothing of it. Those are a Samsung SEFT trailer's (family 1
    // `Samsung`, after EOI: `exif_makernote_row_groups` leaves them out), a
    // JPEG's CIFF segment's (family 1 `CIFF`, `CanonRaw`, and `Canon` for
    // its `Canon::*` subdirectories -- so with a CIFF segment present no
    // group the CIFF root reaches identifies the EXIF note), and a Qualcomm
    // APP7's (a group no root claims). A note ExifTool reads as one value,
    // or a JPEG preview, bears no tags and is not counted
    // (`exif_surgical::makernote_census`).
    // An otherwise rowless note can still have a physically proven root.
    // Pinned MakerNotes::Main selects a headerless Nikon note by its ordered
    // conditions; that root has no Pentax Artist address. Keep other notes
    // unknown until their actual root is established.
    if !ciff && !uncertain_note && tag_bearing == 1 && decoded.is_empty() {
        if let Some(root) = census
            .identified_single_root
            .and_then(|entry| MAKERNOTE_ROOTS.iter().find(|root| root.entry == entry))
        {
            if !groups.iter().any(|group| root.closure.contains(group)) {
                return None;
            }
        }
    }
    let exif_decoded = super::exif_surgical::exif_makernote_row_groups(baseline);
    let ciff_reaches = |group: &str| {
        ciff && MAKERNOTE_ROOTS
            .iter()
            .any(|root| root.entry == "CIFF" && root.closure.contains(&group))
    };
    if uncertain_note
        || tag_bearing > 1
        || (tag_bearing == 1
            && !MAKERNOTE_ROOTS.iter().any(|root| {
                root.entry != "CIFF"
                    && !root.group.is_empty()
                    && exif_decoded.contains(root.group)
                    && !ciff_reaches(root.group)
            }))
    {
        return Some(format!(
            "the file carries a maker note oxidex cannot identify, where ExifTool also \
             writes {name} if the note holds it, which oxidex cannot write"
        ));
    }
    // Every root a decoded group is the root group of; then, for a decoded
    // group none of those reaches (`PreviewIFD`), every root that reaches
    // it; a group no root reaches (a Qualcomm APP7) is a maker-note-like
    // block of its own and holds only its own candidates. (A Samsung
    // trailer's `Samsung` rows, kept here, only widen what may be held.)
    let mut reachable: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for root in MAKERNOTE_ROOTS
        .iter()
        .filter(|root| !root.group.is_empty() && decoded.contains(root.group))
    {
        reachable.extend(root.closure.iter().copied());
    }
    for group in &decoded {
        if reachable.contains(group.as_str()) {
            continue;
        }
        let mut found = false;
        for root in MAKERNOTE_ROOTS
            .iter()
            .filter(|root| root.closure.contains(&group.as_str()))
        {
            found = true;
            reachable.extend(root.closure.iter().copied());
        }
        if !found && let Some(own) = groups.iter().find(|own| own.eq_ignore_ascii_case(group)) {
            reachable.insert(own);
        }
    }
    groups
        .iter()
        .find(|group| reachable.contains(*group))
        .map(|group| {
            format!(
                "the file's maker note may hold {group}:{name}, which ExifTool would also \
                 update and oxidex cannot write"
            )
        })
}

/// The directory an `EXIF:<name>` key really names. `EXIF` is family 0, not a
/// directory: pinned ExifTool 13.59 writes `-EXIF:ISO=100` to `[ExifIFD] ISO`
/// (the tag's `WriteGroup`, else `Exif::Main`'s `WRITE_GROUP => 'ExifIFD'`,
/// Exif.pm:415), where the legacy planner created `[IFD0] ISO`. Resolved from
/// the captured `FindTagInfo` candidates with the transcribed `Avoid` flags
/// (an `Avoid` duplicate yields, Writer.pl:792-825), then from `GPS::Main`.
/// `None` when the name is not one unambiguous EXIF field; the caller then
/// keeps the key as written.
pub(crate) fn resolve_exif_family_key(key: &str) -> Option<String> {
    let (group, name) = key.split_once(':')?;
    if !group.eq_ignore_ascii_case("EXIF") || plain_name(name) != Some(name) {
        return None;
    }
    let table = crate::exiftool_tables::find_ifd_table("Exif", "Main")?;
    let mut rows: Vec<(&str, &str)> = SET_NEW_VALUE_LOOKUP
        .iter()
        .filter(|candidate| is_exif_main(candidate) && candidate.name.eq_ignore_ascii_case(name))
        .filter_map(|candidate| {
            let id = match candidate.raw_id.strip_prefix("0x") {
                Some(hex) => u16::from_str_radix(hex, 16).ok(),
                None => candidate.raw_id.parse().ok(),
            }?;
            let field = table
                .tag(id)
                .filter(|field| field.name.eq_ignore_ascii_case(name))?;
            (!field.flags.avoid).then(|| {
                (
                    candidate.write_group.unwrap_or(EXIF_MAIN_WRITE_GROUP),
                    field.name,
                )
            })
        })
        .collect();
    rows.dedup();
    match rows.as_slice() {
        [(group, name)] => return Some(format!("{group}:{name}")),
        [] => {}
        _ => return None,
    }
    let gps = crate::exiftool_tables::find_ifd_table("GPS", "Main")?;
    let hits: Vec<&str> = gps
        .tags
        .iter()
        .filter(|field| field.name.eq_ignore_ascii_case(name))
        .map(|field| field.name)
        .collect();
    match hits.as_slice() {
        [name] => Some(format!("GPS:{name}")),
        _ => None,
    }
}

/// Refuses a key the format's writer would silently drop.
///
/// The addressable sets are the writers' own: the JPEG/TIFF EXIF planners
/// take the `IFD0:`/`ExifIFD:`/`GPS:`/`EXIF:` families plus whatever the
/// generated public-write resolver resolves; the PNG writer takes `PNG:` text
/// keys and the EXIF families it serializes into `eXIf`
/// (`png_writer::serialize_exif_chunk`); the PDF writer takes the Info
/// dictionary fields (`pdf_writer::write_info_object`). Every other format's
/// writer already refuses the whole write.
pub(crate) fn ensure_writer_addresses(
    requested: &str,
    key: &str,
    format: FileFormat,
    exif_surgical_target: bool,
) -> Result<()> {
    let group = key.split_once(':').map(|(group, _)| group);
    // The PNG `eXIf` chunk goes through the JPEG writer's surgical EXIF
    // transaction (#943), so it addresses what the JPEG writer addresses,
    // plus the PNG text chunks.
    let exif_transaction =
        exif_surgical_target || matches!(format, FileFormat::JPEG | FileFormat::PNG);
    let addressed = if exif_transaction {
        group.is_some_and(|group| matches!(group, "IFD0" | "ExifIFD" | "GPS" | "EXIF"))
            || (matches!(format, FileFormat::PNG) && group == Some("PNG"))
            || generated_route_resolves(key)
    } else {
        match (format, key.split_once(':')) {
            (FileFormat::PDF, Some((group, name))) if group == "PDF" => {
                if super::pdf_writer::is_info_field(name) {
                    true
                } else {
                    return Err(refuse(
                        requested,
                        format!(
                            "oxidex's PDF writer writes only the Info dictionary fields \
                             (Title, Author, Subject, Keywords, Creator, Producer, \
                             CreationDate, ModDate); {name} is not one of them"
                        ),
                    ));
                }
            }
            (FileFormat::PDF, _) => false,
            _ => true,
        }
    };
    if addressed {
        return Ok(());
    }
    Err(refuse(
        requested,
        match group {
            Some(group) => format!("oxidex's {format:?} writer cannot write the {group} group"),
            None => "an ungrouped name reached the writer unresolved".to_string(),
        },
    ))
}

/// Whether the generated public-write resolver (`generated_public_write::
/// resolve_public`) resolves `key` to a physical row -- the route that already
/// answered an ungrouped migrated name such as `Artist` in a TIFF-structured
/// RAW before this module existed.
pub(crate) fn generated_route_resolves(key: &str) -> bool {
    matches!(
        super::generated_public_write::resolve_public(
            key,
            &super::generated_write_address::generated_rules(),
            super::generated_setnewvalue_public_migration_rules::PUBLIC_SET_NEW_VALUE_MIGRATIONS,
        ),
        super::generated_write_address::Resolution::Resolved(_)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;

    #[test]
    fn historical_sole_sony_row_cannot_be_written_into_exififd() {
        let table = crate::exiftool_tables::find_ifd_table("Exif", "Main").unwrap();
        let sony = table.tag(0x7034).unwrap();
        assert_eq!(sony.name, "ChromaticAberrationCorrection");
        // 12.64 has this row alone; its captured WriteGroup is SubIFD.
        let historical_rows = [sony];
        for key in [
            "ExifIFD:ChromaticAberrationCorrection",
            "IFD0:ChromaticAberrationCorrection",
            "EXIF:ChromaticAberrationCorrection",
        ] {
            assert_eq!(
                lone_subifd_row_id_for_destination(key, &historical_rows),
                Some(0x7034),
                "{key}"
            );
        }
        assert_eq!(
            lone_subifd_row_id_for_destination(
                "SubIFD:ChromaticAberrationCorrection",
                &historical_rows
            ),
            None
        );

        let key = "ExifIFD:ChromaticAberrationCorrection";
        if let Some(exif31) = table.tag(0xa410) {
            // 13.59 adds a genuine ExifIFD row. Its duplicate-row selector,
            // and the registry-address refusal, still decide this request.
            assert_eq!(exif31.name, sony.name);
            assert_eq!(
                lone_subifd_row_id_for_destination(key, &[sony, exif31]),
                None
            );
            assert_eq!(
                exif_main_row_for_destination(key).map(|row| row.id),
                Some(0xa410)
            );
            // The source-generated registry now addresses the genuine Exif
            // 3.1 row, so the ordinary ExifIFD request is correctly routed.
            assert_eq!(
                crate::tag_db::tag_registry::get_tag_descriptor(key).map(|tag| tag.id()),
                Some(&crate::core::TagId::new_numeric(0xa410))
            );
            assert!(exif_row_misaddressed(key).is_none());
        } else {
            // In the actual 12.64/11.78 generated table, the Sony row is
            // alone. Exercise the production guard, not only the helper.
            assert_eq!(
                table
                    .tags
                    .iter()
                    .filter(|row| row.name == sony.name)
                    .count(),
                1
            );
            assert_eq!(exif_main_row_for_destination(key).map(|row| row.id), None);
            assert!(
                exif_row_misaddressed(key)
                    .unwrap()
                    .to_string()
                    .contains("tag 0x7034 in SubIFD")
            );
        }
    }

    /// PR #957 review (Codex, 4112788433): every `Shortcuts::Main` key
    /// expands, with SetNewValue's group and `#` rules (Writer.pl:562-578).
    #[test]
    fn write_shortcuts_expand_with_setnewvalues_group_rules() {
        assert_eq!(
            expand_write_shortcut("AllDates").unwrap(),
            ["DateTimeOriginal", "CreateDate", "ModifyDate"]
        );
        assert_eq!(
            expand_write_shortcut("XMP:alldates").unwrap(),
            ["XMP:DateTimeOriginal", "XMP:CreateDate", "XMP:ModifyDate"]
        );
        let common = expand_write_shortcut("commonifd0").unwrap();
        assert_eq!(common.len(), 17);
        assert_eq!(common[1], "IFD0:Make");
        // A member's own group replaces the request's; `#` reaches every one.
        let grouped = expand_write_shortcut("XMP:CommonIFD0#").unwrap();
        assert_eq!(grouped[1], "IFD0:Make#");
        // Shortcuts.pm 11.78 declares CommonIFD0 but no ImageDataMD5
        // alias. The alias appears in both 12.64 and 13.59 sources.
        match crate::exiftool_tables::EXIFTOOL_VERSION {
            "11.78" => {
                assert!(SHORTCUTS.iter().all(|(name, _)| *name != "ImageDataMD5"));
                assert!(expand_write_shortcut("ImageDataMD5").is_none());
            }
            "12.64" | "13.59" => {
                assert!(SHORTCUTS.iter().any(|(name, _)| *name == "ImageDataMD5"));
                assert_eq!(
                    expand_write_shortcut("ImageDataMD5").unwrap(),
                    ["ImageDataHash"]
                );
            }
            other => panic!("unsupported ExifTool source {other}"),
        }
        assert!(expand_write_shortcut("Make").is_none());
        assert!(expand_write_shortcut("IFD0:Artist").is_none());
    }

    /// One EXIF block, no tag-bearing maker note.
    fn no_note() -> MakerNoteCensus {
        MakerNoteCensus {
            blocks: 1,
            ..MakerNoteCensus::default()
        }
    }

    #[test]
    fn physical_uncertainty_survives_only_the_directories_left_by_clears() {
        use super::super::exif_surgical::RequestDeletions;
        let empty = MetadataMap::new();
        let ifd2 = MakerNoteCensus {
            uncertain_ifd1: true,
            uncertain_survivor_ifd1: true,
            ..no_note()
        };
        assert!(makernote_may_hold("WhiteBalance", &empty, &|| ifd2).is_some());
        assert!(
            makernote_may_hold("WhiteBalance", &empty, &|| MakerNoteCensus {
                deletions: RequestDeletions::of("ExifIFD:All"),
                ..ifd2
            })
            .is_some()
        );
        assert!(
            makernote_may_hold("WhiteBalance", &empty, &|| MakerNoteCensus {
                deletions: RequestDeletions::of("IFD1:All"),
                ..ifd2
            })
            .is_none()
        );
        assert!(
            makernote_may_hold("WhiteBalance", &empty, &|| MakerNoteCensus {
                deletions: RequestDeletions::of("MakerNotes:All"),
                ..ifd2
            })
            .is_none()
        );

        let subifd = MakerNoteCensus {
            uncertain_outside_ifd1: true,
            uncertain_survivor_outside_ifd1: true,
            ..no_note()
        };
        assert!(
            makernote_may_hold("WhiteBalance", &empty, &|| MakerNoteCensus {
                deletions: RequestDeletions::of("IFD1:All"),
                ..subifd
            })
            .is_some()
        );
        let exif_child = MakerNoteCensus {
            uncertain_outside_ifd1: true,
            ..no_note()
        };
        assert!(
            makernote_may_hold("WhiteBalance", &empty, &|| MakerNoteCensus {
                deletions: RequestDeletions::of("ExifIFD:All"),
                ..exif_child
            })
            .is_none()
        );
        assert!(makernote_may_hold("WhiteBalance", &empty, &no_note).is_none());

        // A whole-file sentinel retains uncertainty after subtracting any
        // known IFD1 count; its MAX fields must never wrap into zero.
        let unknown = MakerNoteCensus {
            deletions: RequestDeletions::of("IFD1:All"),
            ..MakerNoteCensus::UNKNOWN
        };
        assert!(makernote_may_hold("WhiteBalance", &empty, &|| unknown).is_some());
    }

    /// One EXIF block whose maker note bears tags.
    fn one_note() -> MakerNoteCensus {
        MakerNoteCensus {
            notes: 1,
            tag_bearing: 1,
            ..no_note()
        }
    }

    #[test]
    fn tag_exists_capture_is_sorted_lowercase_and_pinned() {
        assert!(TAG_EXISTS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(TAG_EXISTS.iter().all(|name| *name == name.to_lowercase()));
        let pinned = include_str!("../../.exiftool-version").trim();
        assert_eq!(
            super::super::generated_tag_exists::TAG_EXISTS_CAPTURE.exiftool_version,
            pinned
        );
        for defined in [
            "XPTitle",
            "make",
            "Title",
            "FileSize",
            "Common",
            "CommonIFD0",
        ] {
            assert!(exiftool_tag_exists(defined), "{defined}");
        }
        assert!(!exiftool_tag_exists("NoSuchTag"));
    }

    /// `exiftool -NoSuchTag=v` / `-EXIF:NoSuchTag=v` (13.59) warn with the
    /// name exactly as given, group included.
    #[test]
    fn undefined_warning_is_exiftools_wording() {
        assert_eq!(
            undefined_tag_warning("NoSuchTag").as_deref(),
            Some("Tag 'NoSuchTag' is not defined")
        );
        assert_eq!(
            undefined_tag_warning("EXIF:NoSuchTag").as_deref(),
            Some("Tag 'EXIF:NoSuchTag' is not defined")
        );
        assert_eq!(undefined_tag_warning("XPTitle"), None);
        assert_eq!(undefined_tag_warning("IFD0:FileSize"), None);
        assert_eq!(undefined_tag_warning("Title-de"), None);
    }

    /// Pinned 13.59 on t/images/Writer.jpg: `-XPTitle=v` -> [IFD0] XPTitle,
    /// `-Make=v` -> [IFD0] Make, `-DateTimeOriginal=...` is Exif::Main's
    /// table-default ExifIFD.
    #[test]
    fn ungrouped_exif_names_resolve_to_exiftools_write_group() {
        let empty = MetadataMap::new();
        for (tag, key) in [
            ("XPTitle", "IFD0:XPTitle"),
            ("xptitle", "IFD0:XPTitle"),
            ("Make", "IFD0:Make"),
            ("Artist", "IFD0:Artist"),
            ("ImageDescription", "IFD0:ImageDescription"),
            ("DateTimeOriginal", "ExifIFD:DateTimeOriginal"),
        ] {
            assert_eq!(
                resolve_write_key(tag, true, false, &empty, &no_note).unwrap(),
                key,
                "{tag}"
            );
        }
        assert_eq!(
            resolve_write_key("XMP:Title", true, false, &empty, &no_note).unwrap(),
            "XMP:Title"
        );
    }

    #[test]
    fn ungrouped_names_exiftool_writes_elsewhere_are_refused() {
        let empty = MetadataMap::new();
        // XMP-dc:Title (pinned 13.59 on Writer.jpg); no EXIF candidate.
        assert!(resolve_write_key("Title", true, false, &empty, &no_note).is_err());
        // Exif.pm 0x4746 Rating is `Avoid => 1`: ExifTool creates XMP instead.
        assert!(resolve_write_key("Rating", true, false, &empty, &no_note).is_err());
        // Not defined at all.
        let err = resolve_write_key("NoSuchTag", true, false, &empty, &no_note).unwrap_err();
        assert!(err.to_string().contains("is not defined"), "{err}");
        // Outside JPEG/TIFF no ungrouped name is resolved.
        assert!(resolve_write_key("XPTitle", false, false, &empty, &no_note).is_err());
    }

    /// Pinned 13.59 on t/images/ExifTool.jpg (which carries [CIFF] Make):
    /// `-Make=v` writes [IFD0] Make *and* [CIFF] Make. oxidex cannot write
    /// CIFF, so the ungrouped request is refused rather than half-applied.
    #[test]
    fn a_same_named_row_exiftool_would_also_update_is_refused() {
        let mut baseline = MetadataMap::new();
        baseline.insert("IFD1:Make", TagValue::new_string("x"));
        baseline.insert("File:Make", TagValue::new_string("x"));
        assert_eq!(
            resolve_write_key("Make", true, false, &baseline, &no_note).unwrap(),
            "IFD0:Make"
        );
        baseline.insert("CIFF:Make", TagValue::new_string("Canon"));
        let err = resolve_write_key("Make", true, false, &baseline, &no_note).unwrap_err();
        assert!(err.to_string().contains(":Make"), "{err}");
    }

    /// The capture of maker-note roots is the pinned release's.
    #[test]
    fn makernote_root_capture_is_pinned() {
        use super::super::generated_makernote_groups::{MAKERNOTE_GROUPS_CAPTURE, MAKERNOTE_ROOTS};
        let pinned = include_str!("../../.exiftool-version").trim();
        assert_eq!(MAKERNOTE_GROUPS_CAPTURE.exiftool_version, pinned);
        let canon = MAKERNOTE_ROOTS
            .iter()
            .find(|root| root.entry == "MakerNoteCanon")
            .unwrap();
        assert_eq!(canon.group, "Canon");
        assert!(canon.closure.contains(&"CanonCustom"));
        assert!(!canon.closure.contains(&"CanonRaw"));
        // Sony notes reach Minolta tables; every root reaches its own group.
        let sony = MAKERNOTE_ROOTS
            .iter()
            .find(|root| root.entry == "MakerNoteSony")
            .unwrap();
        assert!(sony.closure.contains(&"Minolta"));
        assert!(
            MAKERNOTE_ROOTS
                .iter()
                .filter(|root| !root.table.is_empty())
                .all(|root| root.closure.contains(&root.group))
        );
        // The value-typed entries hold no tags.
        let binary = MAKERNOTE_ROOTS
            .iter()
            .find(|root| root.entry == "MakerNoteUnknownBinary")
            .unwrap();
        assert!(binary.table.is_empty() && binary.closure.is_empty());
    }

    /// `Exif::Main`'s `Protected => 2` entries (0x0111, 0x0117, 0x014a,
    /// 0x0201, 0x0202, pinned 13.59) never resolve by bare name: each is a
    /// variant group or unwritable. Every other `Protected` field is
    /// `Protected => 1`, which the command line writes (exiftool:1771).
    #[test]
    fn protected_2_exif_fields_never_reach_the_bare_route() {
        let empty = MetadataMap::new();
        for name in [
            "StripOffsets",
            "PreviewImageStart",
            "JpgFromRawStart",
            "StripByteCounts",
            "PreviewImageLength",
            "JpgFromRawLength",
            "A100DataOffset",
            "ThumbnailOffset",
            "OtherImageStart",
            "ThumbnailLength",
            "OtherImageLength",
        ] {
            assert!(
                resolve_write_key(name, true, false, &empty, &no_note).is_err(),
                "{name}"
            );
        }
        // Pinned 13.59: `-CalibrationIlluminant1#=20` -> [IFD0].
        assert_eq!(
            resolve_write_key("CalibrationIlluminant1", true, false, &empty, &no_note).unwrap(),
            "IFD0:CalibrationIlluminant1"
        );
    }

    /// Pinned 13.59: `-Flash#=16` writes the writable Composite:Flash, whose
    /// WriteAlso creates an XMP-exif Flash structure beside [ExifIFD] Flash.
    #[test]
    fn a_writable_composite_candidate_refuses_the_bare_name() {
        let empty = MetadataMap::new();
        let err = ensure_not_also_updated("Flash", "ExifIFD:Flash", &empty, &no_note).unwrap_err();
        assert!(err.to_string().contains("Composite:Flash"), "{err}");
    }

    /// Maker-note holding is decided from the captured tables, not rows:
    /// the reader's maker-note group selects the roots.
    #[test]
    fn makernote_holding_follows_the_decoded_note() {
        let mut canon = MetadataMap::new();
        canon.insert("Canon:MacroMode", TagValue::new_string("Normal"));
        // Canon::ShotInfo defines WhiteBalance; no Canon table defines
        // CalibrationIlluminant1 (only EXIF does).
        assert!(makernote_may_hold("WhiteBalance", &canon, &one_note).is_some());
        assert!(makernote_may_hold("CalibrationIlluminant1", &canon, &one_note).is_none());
        // A Canon note reaches no CanonRaw (CIFF) table.
        assert!(makernote_may_hold("DateTimeOriginal", &canon, &one_note).is_none());

        let mut fuji = MetadataMap::new();
        fuji.insert("FujiFilm:Quality", TagValue::new_string("NORMAL"));
        // No FujiFilm table defines ColorSpace; Nikon::Main does.
        assert!(makernote_may_hold("ColorSpace", &fuji, &one_note).is_none());
        let mut nikon = MetadataMap::new();
        nikon.insert("Nikon:Quality", TagValue::new_string("FINE"));
        assert!(makernote_may_hold("ColorSpace", &nikon, &one_note).is_some());

        // No maker note at all: nothing to hold. A maker note the reader
        // decoded no row of: anything.
        let empty = MetadataMap::new();
        assert!(makernote_may_hold("WhiteBalance", &empty, &no_note).is_none());
        let err = makernote_may_hold("WhiteBalance", &empty, &one_note).unwrap();
        assert!(err.contains("cannot identify"), "{err}");
        // A note ExifTool reads as one value holds no tags: the census does
        // not count it (a SilverFast `LSI1` note, MakerNoteUnknownBinary).
        assert!(makernote_may_hold("Artist", &empty, &no_note).is_none());
        // Two tag-bearing notes (two EXIF APP1s) cannot be told apart by
        // rows, even when one of them was decoded.
        let two = || MakerNoteCensus {
            blocks: 1,
            notes: 2,
            tag_bearing: 2,
            ..MakerNoteCensus::default()
        };
        assert!(makernote_may_hold("WhiteBalance", &canon, &two).is_some());
        // A request that deletes the EXIF maker note leaves nothing to
        // edit (pinned 13.59: `-MakerNotes:All= -WhiteBalance#=1`), but a
        // CIFF segment survives it.
        let exif_ifd_all = || MakerNoteCensus {
            deletions: super::super::exif_surgical::RequestDeletions::of("ExifIFD:All"),
            ..one_note()
        };
        assert!(makernote_may_hold("WhiteBalance", &canon, &exif_ifd_all).is_none());
        // A direct IFD0 note survives ExifIFD:All. Even a decoded Nikon row
        // could have belonged to an ExifIFD note that was just removed, so
        // its physical survivor is kept as unknown rather than attributed
        // to that row.
        let direct_survivor = || MakerNoteCensus {
            surviving_exif_ifd_clear: 1,
            ..exif_ifd_all()
        };
        assert!(
            makernote_may_hold("WhiteBalance", &canon, &direct_survivor)
                .unwrap()
                .contains("outside ExifIFD")
        );
        for clear in ["IFD0:All", "EXIF:All", "MakerNotes:All"] {
            let removed = || MakerNoteCensus {
                deletions: super::super::exif_surgical::RequestDeletions::of(clear),
                ..direct_survivor()
            };
            assert!(makernote_may_hold("WhiteBalance", &canon, &removed).is_none());
        }
        let mut ciff = canon.clone();
        ciff.insert("CIFF:FocalLength", TagValue::new_string("5 mm"));
        let exif_ifd_all_ciff = || MakerNoteCensus {
            ciff: true,
            ..exif_ifd_all()
        };
        assert!(makernote_may_hold("FocalLength", &ciff, &exif_ifd_all_ciff).is_some());
        // A CIFF segment holding only a nested Canon row (keyed
        // `CIFF:FocalLength`, family 1 `Canon`) is still a CIFF segment
        // that `ExifIFD:All` keeps (pinned 13.59 updates its FocalLength).
        let mut ciff_only = MetadataMap::new();
        ciff_only.insert_with_group1("CIFF:FocalLength", TagValue::new_string("5 mm"), "Canon");
        let exif_ifd_all_no_note = || MakerNoteCensus {
            ciff: true,
            deletions: super::super::exif_surgical::RequestDeletions::of("ExifIFD:All"),
            ..no_note()
        };
        assert!(makernote_may_hold("FocalLength", &ciff_only, &exif_ifd_all_no_note).is_some());
        // `MakerNotes:All` takes the CIFF segment too.
        let makernotes_all = || MakerNoteCensus {
            ciff: true,
            deletions: super::super::exif_surgical::RequestDeletions::of("MakerNotes:All"),
            ..one_note()
        };
        assert!(makernote_may_hold("FocalLength", &ciff, &makernotes_all).is_none());
    }

    /// Rows from a maker-note block outside the EXIF note never identify
    /// it (#960 review 4113017918): a Samsung SEFT trailer's rows are
    /// family 1 `Samsung` (pinned 13.59 on t/images/Nikon.jpg with a Sound
    /// & Shot trailer appended: `[MakerNotes:Samsung]
    /// EmbeddedAudioFileName`, and `-WhiteBalance#=1` writes `[Nikon]` and
    /// `[ExifIFD] WhiteBalance`), and a CIFF segment's include family 1
    /// `Canon` (t/images/ExifTool.jpg's CIFF: `[Canon] FocalLength`).
    /// Taken for the EXIF note's root, either closure missed the hidden
    /// note's candidate and the bare write went EXIF-only.
    #[test]
    fn rows_from_outside_the_exif_note_do_not_identify_it() {
        use crate::parsers::samsung_trailer::{EMBEDDED_AUDIO_FILE, EMBEDDED_AUDIO_FILE_NAME};
        let mut seft = MetadataMap::new();
        seft.insert_with_group1(
            EMBEDDED_AUDIO_FILE_NAME,
            TagValue::new_string("SoundShot_000"),
            "Samsung",
        );
        seft.insert_with_group1(
            EMBEDDED_AUDIO_FILE,
            TagValue::new_binary(vec![0]),
            "Samsung",
        );
        let err = makernote_may_hold("WhiteBalance", &seft, &one_note).unwrap();
        assert!(err.contains("cannot identify"), "{err}");
        // A trailer beside no tag-bearing EXIF note still holds only its
        // own groups' candidates.
        assert!(makernote_may_hold("WhiteBalance", &seft, &no_note).is_none());
        // A real Samsung EXIF note's rows still identify it.
        let mut samsung = seft.clone();
        samsung.insert_with_group1(
            "MakerNotes:DeviceType",
            TagValue::new_string("Compact Digital Camera"),
            "Samsung",
        );
        assert!(makernote_may_hold("WhiteBalance", &samsung, &one_note).is_none());

        let mut ciff = MetadataMap::new();
        ciff.insert("CIFF:CanonImageType", TagValue::new_string("CRW:EOS"));
        ciff.insert_with_group1(
            "MakerNotes:FocalLength",
            TagValue::new_string("5 mm"),
            "Canon",
        );
        // `Lens` has only a Nikon maker-note candidate.
        let one_note_ciff = || MakerNoteCensus {
            ciff: true,
            ..one_note()
        };
        let err = makernote_may_hold("Lens", &ciff, &one_note_ciff).unwrap();
        assert!(err.contains("cannot identify"), "{err}");
        // Without a CIFF segment a Canon row is the EXIF note's.
        let mut canon = MetadataMap::new();
        canon.insert("Canon:MacroMode", TagValue::new_string("Normal"));
        assert!(makernote_may_hold("Lens", &canon, &one_note).is_none());
    }

    /// Candidates come from the pinned `FindTagInfo` capture, never from
    /// surfaced rows: `FocusMode` (absent from the SetNewValue address
    /// capture) has a Nikon candidate, which pinned 13.59 deletes from
    /// t/images/Nikon.jpg on `-MakerNotes:FocusMode=`; no bare GPS name has
    /// a maker-note candidate, and the GPS names are captured with their
    /// XMP and MIE candidates.
    #[test]
    fn candidates_come_from_the_capture_not_rows() {
        use super::super::generated_makernote_groups::{GPS_NAME_CANDIDATES, MAKERNOTE_CANDIDATES};
        assert!(
            MAKERNOTE_CANDIDATES
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0)
        );
        let empty = MetadataMap::new();
        assert!(makernote_may_hold("FocusMode", &empty, &one_note).is_some());
        // Sony Tag2010c/e rows have no per-tag Writable, but their tables
        // declare WRITABLE=1; SetNewValue treats them as effective candidates.
        assert!(
            MAKERNOTE_CANDIDATES
                .iter()
                .any(|(name, groups)| { *name == "digitalzoomratio" && groups.contains(&"Sony") })
        );
        // Kodak::SubIFD2 declares Writable, but its WRITE_PROC returns
        // false on SetNewValue's no-argument capability probe.
        assert!(
            !MAKERNOTE_CANDIDATES
                .iter()
                .any(|(name, groups)| { *name == "scenemodeused" && groups.contains(&"Kodak") })
        );
        assert!(
            !MAKERNOTE_CANDIDATES
                .iter()
                .any(|(name, groups)| { *name == "maxaperture" && groups.contains(&"Kodak") })
        );
        assert!(
            MAKERNOTE_CANDIDATES
                .iter()
                .any(|(name, groups)| { *name == "focusmode" && groups.contains(&"Nikon") })
        );
        for gps in [
            "GPSVersionID",
            "GPSDateStamp",
            "GPSLatitudeRef",
            "GPSDestLatitudeRef",
            "GPSDestBearing",
        ] {
            assert!(
                makernote_may_hold(gps, &empty, &one_note).is_none(),
                "{gps}"
            );
            assert!(
                GPS_NAME_CANDIDATES
                    .iter()
                    .any(|(name, g0, _)| name.eq_ignore_ascii_case(gps) && *g0 == "EXIF"),
                "{gps}"
            );
        }
        assert!(GPS_NAME_CANDIDATES.contains(&("gpsdestbearing", "XMP", "XMP-exif")));
        // A GPS name is judged by its captured candidates: an XMP-exif row
        // refuses, a GPS row does not.
        let mut xmp = MetadataMap::new();
        xmp.insert("XMP-exif:GPSDestBearing", TagValue::new_string("1"));
        assert!(
            ensure_not_also_updated("GPSDestBearing", "GPS:GPSDestBearing", &xmp, &one_note)
                .is_err()
        );
        let mut gps = MetadataMap::new();
        gps.insert("GPS:GPSDestBearing", TagValue::new_string("1"));
        assert!(
            ensure_not_also_updated("GPSDestBearing", "GPS:GPSDestBearing", &gps, &one_note)
                .is_ok()
        );
    }

    /// ExifTool writes every EXIF APP1; oxidex writes one, so a bare name in
    /// a file with two is refused.
    #[test]
    fn a_bare_name_in_several_exif_blocks_is_refused() {
        let empty = MetadataMap::new();
        let two = || MakerNoteCensus {
            blocks: 2,
            ..MakerNoteCensus::default()
        };
        let err = ensure_not_also_updated("Artist", "IFD0:Artist", &empty, &two).unwrap_err();
        assert!(err.to_string().contains("2 EXIF blocks"), "{err}");
        // Unless the request deletes every EXIF block first.
        let recreated = || MakerNoteCensus {
            deletions: super::super::exif_surgical::RequestDeletions::of("EXIF:All"),
            ..two()
        };
        assert!(ensure_not_also_updated("Artist", "IFD0:Artist", &empty, &recreated).is_ok());
    }

    #[test]
    fn a_hidden_candidate_occurrence_still_refuses_a_bare_write() {
        use crate::core::tag_value::TagValue;
        let mut baseline = MetadataMap::new();
        baseline.insert_with_group1("XMP-exif:ColorSpace", TagValue::new_integer(1), "XMP-exif");
        baseline.insert_with_group1("XMP-exif:ColorSpace", TagValue::new_integer(2), "ExifIFD");
        assert!(
            ensure_not_also_updated("ColorSpace", "ExifIFD:ColorSpace", &baseline, &no_note)
                .is_err()
        );
    }

    /// A maker-note entry named directly is refused where the file carries
    /// a note (13.59 deletes it: `-ExifIFD:MakerNoteCanon=` on Canon.jpg).
    #[test]
    fn a_named_maker_note_entry_is_refused() {
        for key in [
            "ExifIFD:MakerNoteCanon",
            "ExifIFD:MakerNoteCanon#",
            "EXIF:MakerNoteCanon",
            "exif:makernotecanon#",
            "MakerNotes:MakerNoteNikon",
            "MakerNotes:MakerNoteNikon#",
        ] {
            assert!(
                ensure_makernote_entry_not_named(key, key, &one_note).is_err(),
                "{key}"
            );
            assert!(
                ensure_makernote_entry_not_named(key, key, &no_note).is_ok(),
                "{key}"
            );
        }
        assert!(ensure_makernote_entry_not_named("X", "ExifIFD:WhiteBalance", &one_note).is_ok());
        assert!(ensure_makernote_entry_not_named("X", "IFD0:MakerNoteCanon", &one_note).is_ok());
        assert!(ensure_makernote_entry_not_named("X", "EXIF:CIFF", &one_note).is_ok());
    }

    /// Only a same-named row a candidate can be is one ExifTool also
    /// updates (pinned 13.59 on ExifTool.jpg leaves `[SPIFF] ColorSpace`);
    /// MIE takes every EXIF write.
    #[test]
    fn rows_no_candidate_can_be_do_not_refuse() {
        let mut spiff = MetadataMap::new();
        spiff.insert("SPIFF:ColorSpace", TagValue::new_integer(1));
        assert!(
            ensure_not_also_updated("ColorSpace", "ExifIFD:ColorSpace", &spiff, &no_note).is_ok()
        );
        let mut xmp = MetadataMap::new();
        xmp.insert("XMP-exif:ColorSpace", TagValue::new_integer(1));
        assert!(
            ensure_not_also_updated("ColorSpace", "ExifIFD:ColorSpace", &xmp, &no_note).is_err()
        );
        let mut mie = MetadataMap::new();
        mie.insert("MIE:TrailerSignature", TagValue::new_string("x"));
        let empty = MetadataMap::new();
        for removal in [false, true] {
            assert!(
                ensure_no_mie_copy(
                    "Artist",
                    "IFD0:Artist",
                    &mie,
                    removal,
                    &MieCensus::NotATrailerCarrier
                )
                .is_err()
            );
            assert!(
                ensure_no_mie_copy(
                    "PNG:Title",
                    "PNG:Title",
                    &mie,
                    removal,
                    &MieCensus::NotATrailerCarrier
                )
                .is_ok()
            );
            assert!(
                ensure_no_mie_copy(
                    "Artist",
                    "IFD0:Artist",
                    &empty,
                    removal,
                    &MieCensus::NotATrailerCarrier
                )
                .is_ok()
            );
        }
    }

    /// A big-endian TIFF: IFD0 holding `ifd0` (tag, type, value bytes) and,
    /// when `exif` is not empty, an ExifIFD holding those. Every value is
    /// stored out of line (each is longer than four bytes).
    fn tiff_with(ifd0: &[(u16, u16, &[u8])], exif: &[(u16, u16, &[u8])]) -> Vec<u8> {
        let ifd_len = |n: usize| 2 + 12 * n + 4;
        let n0 = ifd0.len() + usize::from(!exif.is_empty());
        let exif_at = 8 + ifd_len(n0);
        let mut data_at = exif_at
            + if exif.is_empty() {
                0
            } else {
                ifd_len(exif.len())
            };
        let mut data = Vec::new();
        let mut ifd = |entries: &[(u16, u16, &[u8])], pointer: Option<usize>| {
            let mut out = ((entries.len() + usize::from(pointer.is_some())) as u16)
                .to_be_bytes()
                .to_vec();
            for (tag, kind, value) in entries {
                out.extend_from_slice(&tag.to_be_bytes());
                out.extend_from_slice(&kind.to_be_bytes());
                out.extend_from_slice(&(value.len() as u32).to_be_bytes());
                out.extend_from_slice(&(data_at as u32).to_be_bytes());
                data.extend_from_slice(value);
                data_at += value.len();
            }
            if let Some(at) = pointer {
                out.extend_from_slice(&[0x87, 0x69, 0, 4, 0, 0, 0, 1]);
                out.extend_from_slice(&(at as u32).to_be_bytes());
            }
            out.extend_from_slice(&[0; 4]);
            out
        };
        let mut out = b"MM\0*\0\0\0\x08".to_vec();
        out.extend(ifd(ifd0, (!exif.is_empty()).then_some(exif_at)));
        if !exif.is_empty() {
            out.extend(ifd(exif, None));
        }
        out.extend(data);
        out
    }

    /// A MIE census holding `tiff` as its one MIE-Meta EXIF block, decoded
    /// as `MieCensus::of_trailers` decodes one.
    fn census_holding(tiff: Vec<u8>) -> MieCensus<'static> {
        MieCensus::Held(vec![MieExifBlock::new(
            std::borrow::Cow::Owned(tiff),
            std::rc::Rc::new(std::cell::Cell::new(MIE_CENSUS_BYTE_BUDGET)),
        )])
    }

    #[test]
    fn mie_set_refuses_before_decoding_any_held_tiff() {
        let census = census_holding(tiff_with(&[], &[]));
        let block = &census.blocks()[0];
        assert!(block.rows.get().is_none());
        let empty = MetadataMap::new();
        for key in [
            "IFD0:Artist",
            "ExifIFD:MakerNoteCanon",
            "EXIF:MakerNoteNikon",
        ] {
            assert!(
                ensure_no_mie_copy(key, key, &empty, false, &census).is_err(),
                "{key}"
            );
            assert!(block.rows.get().is_none(), "{key}: set must not decode MIE");
            assert!(
                ensure_no_mie_copy(key, key, &empty, false, &MieCensus::Unknown).is_err(),
                "{key}: unknown MIE remains a refusal"
            );
        }
        assert!(block.rows.get().is_none());
    }

    #[test]
    fn mie_row_decode_budget_failure_is_not_an_empty_absence_proof() {
        let mut tiff = b"II*\0\x08\0\0\0".to_vec();
        let aliases = 32_u16;
        let payload_len = 128 * 1024_u32;
        let data_at = 8 + 2 + usize::from(aliases) * 12 + 4;
        tiff.extend(aliases.to_le_bytes());
        for _ in 0..aliases {
            tiff.extend(0x010e_u16.to_le_bytes());
            tiff.extend(7_u16.to_le_bytes());
            tiff.extend(payload_len.to_le_bytes());
            tiff.extend((data_at as u32).to_le_bytes());
        }
        tiff.extend(0_u32.to_le_bytes());
        tiff.resize(data_at + payload_len as usize, b'x');
        let budget = std::rc::Rc::new(std::cell::Cell::new(1024 * 1024));
        let block = MieExifBlock::new(std::borrow::Cow::Owned(tiff), budget);
        assert!(block.rows().is_none());
        assert!(
            block.rows().is_none(),
            "exhaustion remains cached as unknown"
        );
    }

    #[test]
    fn grouped_mie_absence_requires_a_complete_directory_walk() {
        // The tolerant scanner sees this root table, but TIFF type 13 is an
        // unmodelled child pointer. A missing GPS row says nothing about its
        // contents, even when the ordinary reader returns other rows.
        let mut tiff = b"II*\0\x08\0\0\0\x01\0".to_vec();
        tiff.extend(0x010f_u16.to_le_bytes());
        tiff.extend(13_u16.to_le_bytes());
        tiff.extend(1_u32.to_le_bytes());
        tiff.extend(0_u32.to_le_bytes());
        tiff.extend(0_u32.to_le_bytes());
        let census = census_holding(tiff);
        let block = &census.blocks()[0];
        let scan = block.scan().expect("tolerant scan");
        assert!(!super::super::exif_surgical::census_walk_is_complete(
            &block.tiff,
            scan.byte_order
        ));
        assert!(block.rows().is_some(), "reader can still return other rows");
        let empty = MetadataMap::new();
        assert!(
            ensure_no_mie_copy(
                "GPS:GPSVersionID",
                "GPS:GPSVersionID",
                &empty,
                true,
                &census
            )
            .is_err()
        );
    }

    /// MIE deletion safety shares PR960's Adobe framing and record count.
    #[test]
    fn mie_dng_census_uses_framed_adobe_records() {
        use super::super::exif_surgical::{EXIF_BLOCK_MAGICS, makernote_census};
        let record = |tag: &[u8; 4], payload: &[u8]| {
            let mut bytes = tag.to_vec();
            bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
            bytes.extend_from_slice(payload);
            if payload.len() & 1 == 1 {
                bytes.push(0);
            }
            bytes
        };
        let foreign = record(b"XxxN", b"payload MakN text");
        let mut two = record(b"MakN", b"II\0\0\0\0");
        two.extend(record(b"MakN", b"MM\0\0\0\0"));
        let malformed = [foreign.clone(), b"MakN\0\0\0\x06II".to_vec()].concat();
        let empty = MetadataMap::new();
        for (label, records, count, refused) in [
            ("foreign payload", foreign, Some(0), false),
            ("two actual records", two, Some(2), true),
            ("malformed successor", malformed, None, true),
        ] {
            let data = [b"Adobe\0".as_slice(), &records].concat();
            let tiff = tiff_with(&[(0xc634, 7, &data)], &[]);
            let mut mie = b"~\x10\x04\xfe0MIE\0\0\0\0~\x10\x04\0Meta~\0\x04\xfeEXIF".to_vec();
            mie.extend_from_slice(&(tiff.len() as u32).to_be_bytes());
            mie.extend_from_slice(&tiff);
            mie.extend_from_slice(b"~\0\0\0~\0\x04\0zmie~\0\0\x06");
            let total = mie.len() as u32 + 6;
            mie.extend_from_slice(&total.to_be_bytes());
            mie.extend_from_slice(&[0x10, 4]);
            let census = MieCensus::of_trailers(&mie, None);
            let blocks = census.blocks();
            assert_eq!(blocks.len(), 1, "{label}: MIE EXIF block");
            assert!(matches!(&blocks[0].tiff, std::borrow::Cow::Borrowed(_)));
            let notes = makernote_census(&[&blocks[0].tiff], EXIF_BLOCK_MAGICS);
            match count {
                Some(count) => {
                    assert_eq!((notes.notes, notes.tag_bearing), (count, count), "{label}")
                }
                None => {
                    assert_eq!(notes.notes, 0, "{label}: no confirmed record");
                    assert!(
                        notes.uncertain_outside_ifd1,
                        "{label}: malformed successor must not prove absence"
                    );
                }
            }
            for (tag, removal) in [("MakerNotes:All", true), ("MakerNotes:OwnerName", false)] {
                assert_eq!(
                    ensure_no_mie_copy(tag, tag, &empty, removal, &census).is_err(),
                    refused,
                    "{label}: {tag}"
                );
            }
        }
    }

    /// PR #966 review threads on the MIE census: a group removal in any
    /// spelling (4112736390), a DNGPrivateData maker note (4112736397), a
    /// note ExifTool files under ExifIFD (4113020034), and what each census
    /// proves.
    #[test]
    fn mie_census_decides_deletions_as_the_oracle_does() {
        let empty = MetadataMap::new();
        let make: &[u8] = b"FooCam\0";
        let dng = tiff_with(
            &[
                (0x010f, 2, make),
                (0xc634, 1, b"Adobe\0MakN\0\0\0\x04II\0\0"),
            ],
            &[],
        );
        let unknown_note = tiff_with(
            &[(0x010f, 2, make)],
            &[(0x927c, 7, b"Plain text maker note\0")],
        );
        let gps_all = |census: &MieCensus<'_>, spelling: &str| {
            ensure_no_mie_copy(spelling, spelling, &empty, true, census)
        };
        let dng = census_holding(dng);
        assert!(gps_all(&dng, "MakerNotes:All").is_err(), "DNG MakN note");
        assert!(gps_all(&dng, "makernotes:all").is_err(), "any spelling");
        assert!(gps_all(&dng, "gps:all").is_ok(), "no GPS in MIE's EXIF");
        // Codex pre-review of 1fdcc215: an empty GPS IFD (or ExifIFD) the
        // block's IFD0 points at is deleted with its pointer.
        let empty_ifd = |pointer: u16| {
            // IFD0 at 8 (Make at 38, then a pad byte), an empty IFD at 46.
            let mut tiff = b"MM\0*\0\0\0\x08\0\x02\x01\x0f\0\x02\0\0\0\x07\0\0\0\x26".to_vec();
            tiff.extend_from_slice(&pointer.to_be_bytes());
            tiff.extend_from_slice(b"\0\x04\0\0\0\x01\0\0\0\x2e\0\0\0\0FooCam\0\0\0\0\0\0\0\0");
            census_holding(tiff)
        };
        assert!(
            gps_all(&empty_ifd(0x8825), "GPS:All").is_err(),
            "empty GPS IFD"
        );
        assert!(
            gps_all(&empty_ifd(0x8769), "ExifIFD:All").is_err(),
            "empty ExifIFD"
        );
        assert!(gps_all(&empty_ifd(0x8825), "IFD1:All").is_ok(), "no IFD1");
        assert!(gps_all(&dng, "IFD0:All").is_err(), "the block goes");
        let unknown_note = census_holding(unknown_note);
        assert!(
            unknown_note.blocks().iter().any(|block| block
                .rows()
                .is_some_and(|rows| rows.contains_key("ExifIFD:MakerNoteUnknownText"))),
            "the reader files the note under ExifIFD"
        );
        assert!(gps_all(&unknown_note, "MakerNotes:All").is_ok());
        assert!(gps_all(&unknown_note, "ExifIFD:All").is_err());
        assert!(ensure_no_mie_copy("IFD0:Make", "IFD0:Make", &empty, true, &unknown_note).is_err());
        assert!(
            ensure_no_mie_copy("IFD0:Model", "IFD0:Model", &empty, true, &unknown_note).is_ok()
        );
        // Sets are refused wherever ExifTool reads a MIE trailer; nothing is
        // refused without one.
        for census in [&MieCensus::Absent, &MieCensus::Unknown, &unknown_note] {
            assert!(ensure_no_mie_copy("IFD0:Model", "IFD0:Model", &empty, false, census).is_err());
        }
        assert!(
            ensure_no_mie_copy(
                "IFD0:Model",
                "IFD0:Model",
                &empty,
                false,
                &MieCensus::NoTrailer
            )
            .is_ok()
        );
        assert!(
            ensure_no_mie_copy("IFD0:Model", "IFD0:Model", &empty, true, &MieCensus::Absent)
                .is_ok()
        );
        assert!(
            ensure_no_mie_copy(
                "IFD0:Model",
                "IFD0:Model",
                &empty,
                true,
                &MieCensus::Unknown
            )
            .is_err()
        );
    }

    #[test]
    fn writers_refuse_keys_they_would_drop() {
        for key in [
            "IFD0:XPTitle",
            "EXIF:XPTitle",
            "ExifIFD:ISO",
            "GPS:GPSAltitude",
        ] {
            assert!(
                ensure_writer_addresses(key, key, FileFormat::JPEG, false).is_ok(),
                "{key}"
            );
        }
        for key in [
            "XPTitle",
            "XMP:Title",
            "IPTC:Keywords",
            "File:Comment",
            "JFIF:XResolution",
        ] {
            assert!(
                ensure_writer_addresses(key, key, FileFormat::JPEG, false).is_err(),
                "{key}"
            );
        }
        assert!(ensure_writer_addresses("X", "PDF:Title", FileFormat::PDF, false).is_ok());
        assert!(ensure_writer_addresses("X", "PDF:CreateDate", FileFormat::PDF, false).is_ok());
        assert!(ensure_writer_addresses("X", "PDF:Trapped", FileFormat::PDF, false).is_err());
        assert!(ensure_writer_addresses("X", "XMP:Title", FileFormat::PDF, false).is_err());
        assert!(ensure_writer_addresses("X", "PNG:Title", FileFormat::PNG, false).is_ok());
        assert!(ensure_writer_addresses("X", "XMP:Title", FileFormat::PNG, false).is_err());
    }
}
