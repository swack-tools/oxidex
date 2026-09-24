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

/// ExifTool's warning for a `-TAG=` whose name it does not define, spelled
/// exactly as `SetNewValue` spells it (Writer.pl:584), or `None` when the name
/// is defined or is not a plain name this check applies to.
///
/// `exiftool -NoSuchTag=v a.jpg` (13.59): `Warning: Tag 'NoSuchTag' is not
/// defined`, then `Nothing to do.` and exit 1 when no other tag was set.
pub fn undefined_tag_warning(tag: &str) -> Option<String> {
    // `CreationDate` and `ModDate` are the PDF Info dictionary's keys, not
    // ExifTool tag names: PDF.pm's Info table names them `CreateDate` and
    // `ModifyDate` (PDF.pm:126-143), so pinned 13.59 answers `-PDF:CreationDate=` with `Sorry,
    // PDF:CreationDate doesn't exist or isn't writable` / `Nothing to do.`
    // (the reader surfaces both spellings; only ExifTool's writes).
    if let Some((group, name)) = tag.rsplit_once(':')
        && group.eq_ignore_ascii_case("PDF")
        && (name.eq_ignore_ascii_case("CreationDate") || name.eq_ignore_ascii_case("ModDate"))
    {
        return Some(format!("Sorry, {tag} doesn't exist or isn't writable"));
    }
    let name = plain_name(tag)?;
    if exiftool_tag_exists(name) {
        return None;
    }
    let spelled = tag.strip_suffix('#').unwrap_or(tag);
    Some(format!("Tag '{spelled}' is not defined"))
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
    if candidates
        .iter()
        .any(|other| matches!(family0(other), Some("File" | "Composite")))
    {
        return Err(refuse(
            tag,
            "ExifTool prefers a File/Composite tag of this name over EXIF",
        ));
    }
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
    if field.flags.avoid || field.flags.protected || field.subdir.is_some() {
        return Err(refuse(
            tag,
            "the EXIF field is Avoid/Protected or a SubDirectory, which this \
             route does not model; name the group explicitly",
        ));
    }
    let group = candidate.write_group.unwrap_or(EXIF_MAIN_WRITE_GROUP);
    if !matches!(group, "IFD0" | "ExifIFD") {
        return Err(refuse(
            tag,
            format!("ExifTool writes it to {group}, which oxidex cannot write"),
        ));
    }
    ensure_not_also_updated(tag, &format!("{group}:{}", field.name), baseline)?;
    Ok(format!("{group}:{}", field.name))
}

/// Refuses an ungrouped `tag` resolved to `key` when the file carries the
/// same name in a group ExifTool would also update (Writer.pl:613-782: every
/// non-preferred candidate is written "if tag exists") but oxidex cannot
/// write -- half of ExifTool's write is not ExifTool's write.
pub(crate) fn ensure_not_also_updated(tag: &str, key: &str, baseline: &MetadataMap) -> Result<()> {
    let name = key.rsplit(':').next().unwrap_or(key);
    if let Some((existing, _)) = baseline.iter().find(|(existing, _)| {
        existing
            .split_once(':')
            .is_some_and(|(group, existing_name)| {
                existing_name.eq_ignore_ascii_case(name) && !NOT_ALSO_UPDATED.contains(&group)
            })
    }) {
        return Err(refuse(
            tag,
            format!(
                "ExifTool would also update the existing {existing}, which oxidex cannot \
                 write; use -{key}= to change only the EXIF field"
            ),
        ));
    }
    Ok(())
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
                resolve_write_key(tag, true, false, &empty).unwrap(),
                key,
                "{tag}"
            );
        }
        assert_eq!(
            resolve_write_key("XMP:Title", true, false, &empty).unwrap(),
            "XMP:Title"
        );
    }

    #[test]
    fn ungrouped_names_exiftool_writes_elsewhere_are_refused() {
        let empty = MetadataMap::new();
        // XMP-dc:Title (pinned 13.59 on Writer.jpg); no EXIF candidate.
        assert!(resolve_write_key("Title", true, false, &empty).is_err());
        // Exif.pm 0x4746 Rating is `Avoid => 1`: ExifTool creates XMP instead.
        assert!(resolve_write_key("Rating", true, false, &empty).is_err());
        // Not defined at all.
        let err = resolve_write_key("NoSuchTag", true, false, &empty).unwrap_err();
        assert!(err.to_string().contains("is not defined"), "{err}");
        // Outside JPEG/TIFF no ungrouped name is resolved.
        assert!(resolve_write_key("XPTitle", false, false, &empty).is_err());
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
            resolve_write_key("Make", true, false, &baseline).unwrap(),
            "IFD0:Make"
        );
        baseline.insert("CIFF:Make", TagValue::new_string("Canon"));
        let err = resolve_write_key("Make", true, false, &baseline).unwrap_err();
        assert!(err.to_string().contains("CIFF:Make"), "{err}");
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
