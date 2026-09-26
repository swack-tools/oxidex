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
    SET_NEW_VALUE_LOOKUP, StaticNativeLookupCandidate,
};
use super::generated_tag_exists::TAG_EXISTS;
use crate::core::FileFormat;
use crate::core::metadata_map::MetadataMap;
use crate::error::{ExifToolError, Result};

/// `Exif::Main`'s table-level `WRITE_GROUP` (Exif.pm:415), the write group of
/// every candidate that declares none of its own.
const EXIF_MAIN_WRITE_GROUP: &str = "ExifIFD";

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
    if census.blocks > 1 {
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
    if let Some((existing, _)) = baseline.iter().find(|(existing, _)| {
        existing
            .split_once(':')
            .is_some_and(|(group, existing_name)| {
                existing_name.eq_ignore_ascii_case(name)
                    && !NOT_ALSO_UPDATED.contains(&group)
                    && !super::exif_surgical::is_makernote_row(baseline, existing)
                    && others
                        .iter()
                        .any(|&(g0, g1)| g0 != "MakerNotes" && candidate_lives_in(g0, g1, group))
            })
    }) {
        return Err(use_exif_only(format!(
            "ExifTool would also update the existing {existing}, which oxidex cannot write"
        )));
    }
    Ok(())
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

/// Refuses a request -- grouped or not -- resolved to the EXIF-family `key`
/// in a file that carries MIE, wherever pinned 13.59 also edits MIE-Meta's
/// own EXIF copy, which oxidex does not write.
///
/// - A set is written into MIE's EXIF as well, which ExifTool creates when
///   the trailer has none (`-IFD0:Artist=x` and `-IFD0:CalibrationIlluminant1#=20`
///   on t/images/ExifTool.jpg, `-v2`: `Creating EXIF` under `MIE1-Meta1`,
///   and two `[IFD0]` rows after): always refused.
/// - A deletion (a tag, or `<group>:All`) is applied to MIE's EXIF where
///   that holds the tag (`-IFD0:Artist=` on the output above shrinks the
///   trailer from 188 to 90 bytes) and leaves a trailer without it as it was
///   (ExifTool.jpg's own, which has no EXIF; `-IFD0:Software=` on the 188):
///   refused unless every MIE trailer is proven not to hold the tag
///   (`parsers::mie::trailer_exif`, then the no-op scan every EXIF deletion
///   is decided by, `exif_surgical::exif_request_is_no_op`). An EXIF block
///   that scan cannot read proves nothing: 13.59 reads one whose TIFF magic
///   is not 42 anyway and deletes from it.
/// - A maker-note tag (`-MakerNotes:OwnerName=`, set or deletion) is edited
///   in MIE's EXIF where that carries a maker note (Writer.jpg with a MIE
///   holding Canon.jpg's EXIF: `[Canon] OwnerName` emptied, 2490 -> 2496
///   bytes, although the main EXIF has no maker note); ExifTool creates no
///   maker note, so a MIE without one is left alone.
///
/// `file` is the file's bytes where ExifTool reads a MIE trailer (after a
/// JPEG or a TIFF-structured file, not after a PNG's IEND), else `None`;
/// the reader's MIE rows count as MIE too, whose EXIF is unseen without it.
pub(crate) fn ensure_no_mie_copy(
    tag: &str,
    key: &str,
    baseline: &MetadataMap,
    removal: bool,
    file: Option<&[u8]>,
) -> Result<()> {
    use super::exif_surgical::{
        EXIF_BLOCK_MAGICS, exif_request_is_no_op, group_removal, makernote_census,
        scan_entries_with_magics,
    };
    use crate::parsers::mie::{MieExif, trailer_exif};
    let group = key.split_once(':').map_or("", |(group, _)| group);
    let group_all = removal && group_removal(key).is_some();
    let makernote = !group_all && group.eq_ignore_ascii_case("MakerNotes");
    let exif = matches!(
        group,
        "IFD0" | "IFD1" | "ExifIFD" | "GPS" | "InteropIFD" | "EXIF" | "SubIFD"
    ) || super::exif_surgical::chain_key_dir(key).is_some()
        || group_all;
    if !exif && !makernote {
        return Ok(());
    }
    let trailer = file.and_then(trailer_exif);
    let Some(mie) = mie_row(baseline)
        .map(str::to_string)
        .or_else(|| trailer.as_ref().map(|_| "a MIE trailer".to_string()))
    else {
        return Ok(());
    };
    if makernote {
        let untouched = match &trailer {
            Some(MieExif::Absent) => true,
            Some(MieExif::Held(blocks)) => {
                makernote_census(blocks, EXIF_BLOCK_MAGICS).tag_bearing == 0
            }
            Some(MieExif::Unknown) | None => false,
        };
        if untouched {
            return Ok(());
        }
        return Err(refuse(
            tag,
            format!(
                "the file carries MIE ({mie}) whose EXIF directory holds a maker note (or \
                 may), where ExifTool also edits {key}, which oxidex cannot write"
            ),
        ));
    }
    if !removal {
        return Err(refuse(
            tag,
            format!(
                "the file carries MIE ({mie}), whose EXIF directory ExifTool also writes \
                 {key} to (creating it), which oxidex cannot write"
            ),
        ));
    }
    let untouched = match &trailer {
        Some(MieExif::Absent) => true,
        Some(MieExif::Held(blocks)) => {
            blocks
                .iter()
                .all(|block| scan_entries_with_magics(block, EXIF_BLOCK_MAGICS).is_ok())
                && exif_request_is_no_op(
                    blocks,
                    blocks,
                    EXIF_BLOCK_MAGICS,
                    false,
                    &MetadataMap::new(),
                    &MetadataMap::new(),
                    &[key.to_string()],
                )
        }
        Some(MieExif::Unknown) | None => false,
    };
    if untouched {
        return Ok(());
    }
    Err(refuse(
        tag,
        format!(
            "the file carries MIE ({mie}) whose EXIF directory holds {key} (or may), \
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
    use super::generated_makernote_groups::{MAKERNOTE_CANDIDATES, MAKERNOTE_ROOTS};
    let lowered = name.to_ascii_lowercase();
    let groups: &[&str] = MAKERNOTE_CANDIDATES
        .binary_search_by(|(candidate, _)| (*candidate).cmp(lowered.as_str()))
        .map_or(&[], |at| MAKERNOTE_CANDIDATES[at].1);
    if groups.is_empty() {
        return None;
    }
    let decoded = super::exif_surgical::makernote_row_groups(baseline);
    let census = makernote_block();
    // A request that deletes the EXIF maker note leaves no copy there to
    // edit; a CIFF segment (a separate APP0 maker-note block) survives it.
    let ciff = decoded.contains("CIFF") || decoded.contains("CanonRaw");
    if census.deleted && !ciff {
        return None;
    }
    let tag_bearing = if census.deleted {
        0
    } else {
        census.tag_bearing
    };
    if decoded.is_empty() && tag_bearing == 0 {
        return None;
    }
    // Rows do not say which EXIF block they were decoded from, so more than
    // one tag-bearing note cannot be told apart; and the one tag-bearing
    // note must be one the reader identified -- rows only from outside it
    // (a JPEG's CIFF segment, a Qualcomm APP7) say nothing of it. A note
    // ExifTool reads as one value, or a JPEG preview, bears no tags and is
    // not counted (`exif_surgical::makernote_census`).
    if tag_bearing > 1
        || (tag_bearing == 1
            && !MAKERNOTE_ROOTS.iter().any(|root| {
                root.entry != "CIFF" && !root.group.is_empty() && decoded.contains(root.group)
            }))
    {
        return Some(format!(
            "the file carries a maker note oxidex cannot identify, where ExifTool also \
             writes {name} if the note holds it, which oxidex cannot write"
        ));
    }
    // Every root a decoded group is the root group of; then, for a decoded
    // group none of those reaches (`PreviewIFD`), every root that reaches
    // it; a group no root reaches (a Qualcomm APP7, a Samsung trailer) is a
    // maker-note-like block of its own and holds only its own candidates.
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

    /// One EXIF block, no tag-bearing maker note.
    fn no_note() -> MakerNoteCensus {
        MakerNoteCensus {
            blocks: 1,
            tag_bearing: 0,
            deleted: false,
        }
    }

    /// One EXIF block whose maker note bears tags.
    fn one_note() -> MakerNoteCensus {
        MakerNoteCensus {
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
            blocks: 2,
            tag_bearing: 2,
            deleted: false,
        };
        assert!(makernote_may_hold("WhiteBalance", &canon, &two).is_some());
        // A request that deletes the EXIF maker note leaves nothing to
        // edit (pinned 13.59: `-MakerNotes:All= -WhiteBalance#=1`), but a
        // CIFF segment survives it.
        let deleted = || MakerNoteCensus {
            deleted: true,
            ..one_note()
        };
        assert!(makernote_may_hold("WhiteBalance", &canon, &deleted).is_none());
        let mut ciff = canon.clone();
        ciff.insert("CIFF:FocalLength", TagValue::new_string("5 mm"));
        assert!(makernote_may_hold("FocalLength", &ciff, &deleted).is_some());
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
            tag_bearing: 0,
            deleted: false,
        };
        let err = ensure_not_also_updated("Artist", "IFD0:Artist", &empty, &two).unwrap_err();
        assert!(err.to_string().contains("2 EXIF blocks"), "{err}");
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
            assert!(ensure_no_mie_copy("Artist", "IFD0:Artist", &mie, removal, None).is_err());
            assert!(ensure_no_mie_copy("PNG:Title", "PNG:Title", &mie, removal, None).is_ok());
            assert!(ensure_no_mie_copy("Artist", "IFD0:Artist", &empty, removal, None).is_ok());
        }
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
