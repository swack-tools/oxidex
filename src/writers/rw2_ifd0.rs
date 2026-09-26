//! IFD0-group edits of a Panasonic RAW/RW2/RWL file, routed as pinned
//! ExifTool 13.59 routes them, and refused by name where this writer cannot
//! make them.
//!
//! # Where ExifTool puts an `IFD0` tag of such a file
//!
//! A TIFF whose header magic is 0x55 is read and written with the
//! `PanasonicRaw::Main` table, not `Exif::Main` (ExifTool.pm 13.59:8646-8659;
//! RAW, RW2 and RWL alike). Its 0x002e JpgFromRaw is a writable directory,
//! "processed as an embedded document because it contains full EXIF"
//! (PanasonicRaw.pm 13.59:198-216), which `WriteJpgFromRaw` (:831-866)
//! rewrites as a whole JPEG through `WriteJPEG`, with `jpgFromRawMap` (:31-44)
//! routing `IFD0` into its APP1: that is `Doc1:IFD0` in a `-G3` read-back.
//! So a tag of family-1 group `IFD0` is written
//!
//! - into the outer IFD0 by its `PanasonicRaw::Main` entry, when the table
//!   has a writable one -- created if absent unless `Permanent`
//!   (`Artist` 0x013b and `Copyright` 0x8298, "so we don't add it if the
//!   model doesn't write it", :307-313, :337-346);
//! - into `Doc1:IFD0` by its `Exif::Main` entry, when the file has an
//!   embedded EXIF and `Exif::Main` has a writable tag of that name.
//!
//! Measured on t/images Panasonic.rw2 (evidence `rw2-embedded-ifd0/`):
//! `-IFD0:Artist=x` lands in `Doc1:IFD0` only; `-IFD0:Make=Acme` in both;
//! `-IFD0:ISO=` deletes the outer 0x0017, `-IFD0:ISO=100` rewrites it, adds
//! outer 0x0037 and `Doc1:IFD0` 0x8827. With no embedded document,
//! `-IFD0:Artist=x` changes nothing ("0 image files updated").
//!
//! # What this writer does
//!
//! It edits the outer TIFF in place and never the JpgFromRaw: an edit there
//! changes the JPEG's length, so its 0x002e offset/length and the raw data
//! after it (RawDataOffset, which ExifTool re-lays out, `PatchRawDataOffset`
//! :770-826) would move. [`refuse_rw2_ifd0_edits`] therefore refuses by name
//!
//! - a set ExifTool makes in `Doc1:IFD0` (never made in the outer IFD0
//!   alone: `Make` is updated in both places or not at all);
//! - a set ExifTool makes nowhere, which the planner would otherwise have
//!   put in the outer IFD0;
//! - a set of a `PanasonicRaw::Main` tag this writer does not write (one
//!   whose id is not its `Exif::Main` namesake's, or that `Exif::Main` does
//!   not name), which reached no writer and was reported done unchanged;
//! - a deletion of an outer `PanasonicRaw::Main` entry: this writer cannot
//!   shrink an IFD, and without this check `IFD0:ISO` (0x0017, no
//!   `Exif::Main` 0x8827 in the outer IFD0) was reported done unchanged.
//!
//! What remains -- an `Exif::Main`-named tag whose outer entry has the same
//! id (`Make`, `Model`, `Orientation`, an `Artist` the camera wrote) set to
//! a value the embedded IFD0 already holds, or with no embedded document --
//! is what ExifTool writes to the outer IFD0 alone, and the planner writes
//! there; [`verify_jpg_from_raw_kept`] then checks the embedded document is
//! byte-identical.

use crate::core::metadata_map::MetadataMap;
use crate::core::tag_value::TagValue;
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::{IfdTag, PrintConv, find_ifd_table};
use crate::parsers::tiff::ifd_parser::ByteOrder;
use crate::writers::exif_surgical::{
    ExifScan, IfdKind, jpeg_exif_payload, native_to_byte_order, scan_entries_with_magics,
    scan_exif_entries, tag_value_to_field_for_key,
};
use crate::writers::tiff_surgical::WALKABLE_TIFF_MAGICS;

/// The TIFF header magic of a Panasonic RAW/RW2/RWL file (ExifTool.pm
/// 13.59:8646).
const PANASONIC_RAW_MAGIC: u16 = 0x55;

/// PanasonicRaw.pm 13.59:198 `0x2e => JpgFromRaw`.
const JPG_FROM_RAW: u16 = 0x002e;

/// The `PanasonicRaw::Main` tags declared `Permanent => 1`: Artist
/// (PanasonicRaw.pm 13.59:307-313) and Copyright (:337-346). The generated
/// table does not carry the flag.
const PERMANENT: &[u16] = &[0x013b, 0x8298];

/// How a write key addresses the outer IFD0.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Spelling {
    /// `IFD0:<Name>`: the family-1 group of both tables' IFD0.
    Ifd0,
    /// `EXIF:<Name>`: the family-0 group of `PanasonicRaw::Main` (GROUPS
    /// :71) and `Exif::Main`.
    Family,
    /// A bare name: every table of every module naming the tag.
    Bare,
}

fn spelling(key: &str) -> Option<(Spelling, &str)> {
    let (spelling, name) = match key.split_once(':') {
        None => (Spelling::Bare, key),
        Some((group, name)) if group.eq_ignore_ascii_case("IFD0") => (Spelling::Ifd0, name),
        Some((group, name)) if group.eq_ignore_ascii_case("EXIF") => (Spelling::Family, name),
        Some(_) => return None,
    };
    (!name.is_empty() && !name.eq_ignore_ascii_case("all")).then_some((spelling, name))
}

/// The byte order of `file_bytes` when it is a Panasonic RAW/RW2/RWL TIFF.
fn panasonic_raw_order(file_bytes: &[u8]) -> Option<ByteOrder> {
    let order = match file_bytes.get(..2)? {
        b"II" => ByteOrder::LittleEndian,
        b"MM" => ByteOrder::BigEndian,
        _ => return None,
    };
    let magic = file_bytes.get(2..4)?;
    let magic = match order {
        ByteOrder::LittleEndian => u16::from_le_bytes([magic[0], magic[1]]),
        ByteOrder::BigEndian => u16::from_be_bytes([magic[0], magic[1]]),
    };
    (magic == PANASONIC_RAW_MAGIC).then_some(order)
}

/// The `PanasonicRaw::Main` tags named `name` (ExifTool tag names are
/// case-insensitive): `ISO` is two, 0x0017 and 0x0037.
fn panasonic_tags(name: &str) -> Vec<&'static IfdTag> {
    find_ifd_table("PanasonicRaw", "Main")
        .map(|table| {
            table
                .tags
                .iter()
                .filter(|tag| tag.name.eq_ignore_ascii_case(name))
                .collect()
        })
        .unwrap_or_default()
}

/// The writable `Exif::Main` tag named `name`, if there is one.
fn exif_tag(name: &str) -> Option<&'static IfdTag> {
    find_ifd_table("Exif", "Main")?
        .tags
        .iter()
        .find(|tag| tag.name.eq_ignore_ascii_case(name) && tag.writable.is_some())
}

/// A Panasonic RAW/RW2/RWL file's outer IFD0 and its JpgFromRaw's EXIF.
struct Rw2 {
    outer: ExifScan,
    /// The JpgFromRaw's EXIF: `None` when there is no JpgFromRaw, or it
    /// holds no EXIF. `Some(None)` when the EXIF does not scan (then
    /// nothing is assumed about what it holds).
    embedded: Option<Option<ExifScan>>,
    file_type: String,
}

impl Rw2 {
    fn read(file_bytes: &[u8], baseline: &MetadataMap) -> Option<Self> {
        panasonic_raw_order(file_bytes)?;
        let outer = scan_entries_with_magics(file_bytes, WALKABLE_TIFF_MAGICS).ok()?;
        let embedded = outer
            .entries
            .iter()
            .find(|entry| entry.ifd == IfdKind::Ifd0 && entry.tag_id == JPG_FROM_RAW)
            .and_then(|entry| jpeg_exif_payload(&entry.value).ok().flatten())
            .map(|tiff| scan_exif_entries(&tiff).ok());
        Some(Self {
            outer,
            embedded,
            file_type: baseline
                .get_string("File:FileType")
                .unwrap_or("RW2")
                .to_string(),
        })
    }

    fn outer_has(&self, tag_id: u16) -> bool {
        self.outer
            .entries
            .iter()
            .any(|entry| entry.ifd == IfdKind::Ifd0 && entry.tag_id == tag_id)
    }

    /// Whether the embedded IFD0 holds `tag` (a scan failure answers yes).
    fn embedded_has(&self, tag: &IfdTag) -> bool {
        match &self.embedded {
            None => false,
            Some(None) => true,
            Some(Some(scan)) => scan
                .entries
                .iter()
                .any(|entry| entry.ifd == IfdKind::Ifd0 && entry.tag_id == tag.id),
        }
    }

    /// Whether pinned ExifTool 13.59 leaves the embedded EXIF as it is for
    /// `IFD0:<tag>` = `value`: its IFD0 already holds `value`, byte for byte
    /// as the planner would encode it, and its ExifIFD does not hold the tag
    /// -- ExifTool moves such a copy into IFD0 (`-v2` of `-IFD0:ISO=80` on a
    /// JpgFromRaw whose IFD0 and ExifIFD both hold ISO 80: "- ExifIFD:ISO =
    /// '80'"; evidence `probe-review-956.txt`). ExifIFD is the only
    /// directory it moves one from: WriteExif.pl 13.59:20-23 `%crossDelete =
    /// (ExifIFD => 'IFD0', IFD0 => 'ExifIFD')`, applied at :1156-1171. A
    /// same-ID entry in IFD1 (the thumbnail's Make, XResolution) is another
    /// directory's tag and stays as it is (#956 review, rw2_ifd0.rs:198:
    /// `-IFD0:Make=Acmesonic` leaves `Doc1:IFD1` Make untouched).
    fn embedded_unchanged(&self, key: &str, tag: &IfdTag, value: &TagValue) -> bool {
        let Some(Some(scan)) = &self.embedded else {
            return false;
        };
        scan.entries
            .iter()
            .all(|entry| entry.ifd != IfdKind::ExifIfd || entry.tag_id != tag.id)
            && holds(scan, IfdKind::Ifd0, key, tag, value)
    }

    /// Whether the outer IFD0 holds `value` for `tag`.
    fn outer_holds(&self, key: &str, tag: &IfdTag, value: &TagValue) -> bool {
        holds(&self.outer, IfdKind::Ifd0, key, tag, value)
    }

    /// ExifTool's refusal of a `PanasonicRaw::Main` tag no writable table of
    /// the group names: "Sorry, EXIF:SensorWidth doesn't exist or isn't
    /// writable", exit 1, file unchanged.
    fn not_writable(&self, verb: &str, key: &str, name: &str) -> ExifToolError {
        self.refused(
            verb,
            key,
            format!(
                "PanasonicRaw IFD0 tag {name} is not writable (pinned ExifTool 13.59: \
                 \"{key} doesn't exist or isn't writable\")"
            ),
        )
    }

    fn refused(&self, verb: &str, key: &str, why: String) -> ExifToolError {
        ExifToolError::tag_not_written(
            key.to_string(),
            format!(
                "{verb} '{key}' {} a {} file is not supported: {why}",
                if verb == "Writing" { "to" } else { "from" },
                self.file_type
            ),
        )
    }

    /// The refusal of the set `key` = `value` (`name` in `spelling`), if
    /// this writer cannot make it as ExifTool does. `same_value`: an
    /// explicit set to the value the reader reports (`IFD0:ISO=80` over an
    /// ISO of 80), which ExifTool still writes wherever an entry is absent
    /// or holds another value.
    fn check_set(
        &self,
        key: &str,
        spelling: Spelling,
        name: &str,
        value: &TagValue,
        same_value: bool,
    ) -> Option<ExifToolError> {
        let outer_tags: Vec<&IfdTag> = panasonic_tags(name);
        let writable: Vec<&IfdTag> = outer_tags
            .iter()
            .copied()
            .filter(|tag| tag.writable.is_some())
            .collect();
        let exif = exif_tag(name);
        let ids = |tags: &[&IfdTag]| {
            tags.iter()
                .map(|tag| format!("0x{:04x}", tag.id))
                .collect::<Vec<_>>()
                .join(", ")
        };
        if spelling == Spelling::Family
            && writable.is_empty()
            && !outer_tags.is_empty()
            && exif.is_none()
        {
            return Some(self.not_writable("Writing", key, name));
        }
        if spelling != Spelling::Ifd0 {
            // An `Exif::Main` tag outside IFD0 is the embedded-document
            // check's and the planner's; here only what reaches a
            // `PanasonicRaw::Main` entry this writer does not write.
            return (!writable.is_empty()
                && !exif.is_some_and(|exif| writable.iter().all(|tag| tag.id == exif.id)))
            .then(|| {
                self.refused(
                    "Writing",
                    key,
                    format!(
                        "pinned ExifTool 13.59 writes PanasonicRaw IFD0 tag {} ({name}) in \
                         the outer IFD0, which this writer does not write",
                        ids(&writable)
                    ),
                )
            });
        }
        // Where ExifTool writes it: the outer entries it writes (present,
        // or creatable), and the embedded IFD0 if it holds a different
        // value or none.
        let outer: Vec<&IfdTag> = writable
            .iter()
            .copied()
            .filter(|tag| self.outer_has(tag.id) || !PERMANENT.contains(&tag.id))
            .collect();
        let embedded = exif.filter(|_| self.embedded.is_some());
        if let Some(exif) = embedded
            && !self.embedded_unchanged(key, exif, value)
        {
            let also = if outer.is_empty() {
                String::new()
            } else {
                format!(" and PanasonicRaw tag {} in the outer IFD0", ids(&outer))
            };
            return Some(self.refused(
                "Writing",
                key,
                format!(
                    "pinned ExifTool 13.59 writes it into the IFD0 of the embedded JpgFromRaw \
                     (PanasonicRaw 0x002e){also}, and this writer does not edit that JPEG; it \
                     does not update the outer IFD0 alone"
                ),
            ));
        }
        if outer.is_empty() {
            if exif.is_none() && outer_tags.is_empty() {
                return None; // no table names it: the planner's refusal
            }
            if exif.is_none() {
                return Some(self.not_writable("Writing", key, name));
            }
            if same_value {
                return None; // ExifTool writes nothing either
            }
            let place = if embedded.is_some() {
                "the embedded JpgFromRaw's IFD0 (PanasonicRaw 0x002e), which already holds \
                 that value"
            } else {
                "the IFD0 of an embedded JpgFromRaw (PanasonicRaw 0x002e), which this file \
                 does not have"
            };
            return Some(self.refused(
                "Writing",
                key,
                format!(
                    "pinned ExifTool 13.59 writes it only into {place}; the outer PanasonicRaw \
                     IFD0 {}, so ExifTool leaves the file unchanged",
                    if writable.is_empty() {
                        "has no such tag".to_string()
                    } else {
                        format!("holds {name} only when the camera wrote it")
                    }
                ),
            ));
        }
        if same_value {
            // ExifTool writes every outer entry it would for any value: one
            // absent is created (`-IFD0:ISO=80` over 0x0017 = 80 adds 0x0037;
            // evidence `probe-review-956.txt`), one holding another value is
            // rewritten. Neither is made here.
            let changed: Vec<&IfdTag> = outer
                .iter()
                .copied()
                .filter(|tag| !self.outer_holds(key, tag, value))
                .collect();
            if changed.is_empty() {
                return None;
            }
            return Some(self.refused(
                "Writing",
                key,
                format!(
                    "pinned ExifTool 13.59 writes PanasonicRaw IFD0 tag {} ({name}) in the \
                     outer IFD0, creating it where absent, although another entry already \
                     holds this value; this writer does not write it",
                    ids(&changed)
                ),
            ));
        }
        // The planner writes the `Exif::Main` id into the outer IFD0: exact
        // only when that is the one entry ExifTool writes there.
        if exif.is_some_and(|exif| outer.iter().all(|tag| tag.id == exif.id)) {
            return None;
        }
        Some(self.refused(
            "Writing",
            key,
            format!(
                "pinned ExifTool 13.59 writes PanasonicRaw IFD0 tag {} ({name}) in the outer \
                 IFD0, which this writer does not write",
                ids(&outer)
            ),
        ))
    }

    /// The refusal of the named removal `key` (`name` in `spelling`), if
    /// ExifTool changes the outer IFD0 for it (or refuses it) and this
    /// writer cannot.
    fn check_removal(&self, key: &str, spelling: Spelling, name: &str) -> Option<ExifToolError> {
        let outer_tags = panasonic_tags(name);
        let present: Vec<&IfdTag> = outer_tags
            .iter()
            .copied()
            .filter(|tag| self.outer_has(tag.id))
            .collect();
        let deleted: Vec<String> = present
            .iter()
            .filter(|tag| tag.writable.is_some())
            .map(|tag| format!("0x{:04x}", tag.id))
            .collect();
        let exif = exif_tag(name);
        if !deleted.is_empty() {
            let also = if spelling == Spelling::Ifd0 && exif.is_some_and(|t| self.embedded_has(t)) {
                " (and the embedded JpgFromRaw's IFD0 one, PanasonicRaw 0x002e)"
            } else {
                ""
            };
            return Some(self.refused(
                "Removing",
                key,
                format!(
                    "pinned ExifTool 13.59 deletes PanasonicRaw IFD0 tag {} ({name}) from the \
                     outer IFD0{also}, and this writer edits entries in place and cannot shrink \
                     an IFD table",
                    deleted.join(", ")
                ),
            ));
        }
        // `IFD0:` and `EXIF:` name only `PanasonicRaw::Main` and
        // `Exif::Main` here, so a tag neither declares writable is refused by
        // ExifTool, file unchanged, exit 1. A bare name reaches every
        // module's tables, and pinned ExifTool 13.59 exits 0 with the file
        // unchanged for `-SensorWidth=` ("0 image files updated", evidence
        // `probe-review-956.txt`): not refused here.
        if spelling != Spelling::Bare
            && !outer_tags.is_empty()
            && outer_tags.iter().all(|tag| tag.writable.is_none())
            && exif.is_none()
        {
            return Some(self.not_writable("Removing", key, name));
        }
        if spelling == Spelling::Ifd0
            && let Some(exif) = exif
            && self.embedded_has(exif)
        {
            let why = "the tag lives in the IFD0 of the embedded JpgFromRaw (PanasonicRaw \
                       0x002e), where pinned ExifTool 13.59 deletes it, and this writer does \
                       not edit that JPEG";
            return Some(self.refused("Removing", key, why.to_string()));
        }
        None
    }
}

/// How pinned ExifTool 13.59 routes a write request's name on a Panasonic
/// RAW/RW2/RWL file (`file_bytes`), for the write transaction's resolver
/// (`core::operations::resolve_write_key_for`). Measured on t/images
/// Panasonic.rw2 and its no-JpgFromRaw variant (roll-up evidence
/// `rw2-bare-names-oracle.txt`): a bare name and the family-0 `EXIF:` name
/// act exactly as `IFD0:<Name>` does -- both tables' IFD0 is family-1
/// `IFD0` -- except for a `PanasonicRaw::Main` tag no writable table names
/// (`SensorWidth`): `IFD0:` / `EXIF:` answer "Sorry, <key> doesn't exist or
/// isn't writable" (exit 1), while a bare name reaches every module's tables
/// and leaves the file unchanged (exit 0).
pub(crate) enum Rw2Name {
    /// Not a Panasonic RAW, or a name neither `PanasonicRaw::Main` nor
    /// `Exif::Main` declares: resolved as for any other file.
    Other,
    /// Resolved to this `IFD0:<Name>` key, which [`refuse_rw2_ifd0_edits`]
    /// then checks.
    Ifd0(String),
    /// ExifTool leaves the file unchanged.
    NoOp,
    /// ExifTool refuses: "Sorry, <key> doesn't exist or isn't writable".
    NotWritable,
}

/// [`Rw2Name`] for `key` (bare, `IFD0:` or `EXIF:`) in `file_bytes`.
pub(crate) fn route_rw2_name(file_bytes: &[u8], key: &str) -> Rw2Name {
    if panasonic_raw_order(file_bytes).is_none() {
        return Rw2Name::Other;
    }
    let Some((spelling, name)) = spelling(key) else {
        return Rw2Name::Other;
    };
    let outer_tags = panasonic_tags(name);
    let exif = exif_tag(name);
    if outer_tags.is_empty() && exif.is_none() {
        return Rw2Name::Other;
    }
    if exif.is_none() && outer_tags.iter().all(|tag| tag.writable.is_none()) {
        return if spelling == Spelling::Bare {
            Rw2Name::NoOp
        } else {
            Rw2Name::NotWritable
        };
    }
    let spelled = exif
        .map(|tag| tag.name)
        .or_else(|| outer_tags.first().map(|tag| tag.name))
        .unwrap_or(name);
    Rw2Name::Ifd0(format!("IFD0:{spelled}"))
}

/// Whether pinned ExifTool 13.59 leaves a Panasonic RAW/RW2/RWL file
/// (`file_bytes`, read into `baseline`) unchanged for the set `key` (an
/// `IFD0:` key) = `value`: no outer `PanasonicRaw::Main` entry it writes (the
/// table has none writable, or only a `Permanent` one the file lacks:
/// `Artist`, `Copyright`), and the `Exif::Main` namesake's only destination
/// -- the embedded JpgFromRaw's IFD0 -- either absent or already holding
/// `value`. On the no-JpgFromRaw variant of t/images Panasonic.rw2 pinned
/// 13.59 answers `-IFD0:Artist=x`, `-Software=x`, `-EXIF:XResolution=300`
/// "0 image files updated" / "1 image files unchanged". This writer refused
/// them ("ExifTool leaves the file unchanged"); the transaction now reports
/// them unchanged, as ExifTool does.
pub(crate) fn rw2_set_is_no_op(
    file_bytes: &[u8],
    baseline: &MetadataMap,
    key: &str,
    value: &TagValue,
) -> bool {
    let Some((Spelling::Ifd0, name)) = spelling(key) else {
        return false;
    };
    let Some(rw2) = Rw2::read(file_bytes, baseline) else {
        return false;
    };
    let Some(exif) = exif_tag(name) else {
        return false;
    };
    let outer_writes = panasonic_tags(name).iter().any(|tag| {
        tag.writable.is_some() && (rw2.outer_has(tag.id) || !PERMANENT.contains(&tag.id))
    });
    !outer_writes && (rw2.embedded.is_none() || rw2.embedded_unchanged(key, exif, value))
}

/// Refuses, by name and before anything is written, an IFD0-group edit of
/// a Panasonic RAW/RW2/RWL file (`file_bytes`, read into `baseline`) that
/// pinned ExifTool 13.59 makes where this writer cannot (see the module
/// documentation): `desired`'s changed `IFD0:`, `EXIF:` and bare keys, and
/// the named removals in `removed`. Any other file passes.
pub(crate) fn refuse_rw2_ifd0_edits(
    file_bytes: &[u8],
    baseline: &MetadataMap,
    desired: &MetadataMap,
    removed: &[String],
) -> Result<()> {
    if panasonic_raw_order(file_bytes).is_none() {
        return Ok(());
    }
    let sets: Vec<(&String, &TagValue, Spelling, &str)> = desired
        .iter()
        .filter(|(key, value)| baseline.get(key.as_str()) != Some(*value))
        .filter_map(|(key, value)| {
            spelling(key).map(|(spelling, name)| (key, value, spelling, name))
        })
        .collect();
    let removals: Vec<(&String, Spelling, &str)> = removed
        .iter()
        .filter_map(|key| spelling(key).map(|(spelling, name)| (key, spelling, name)))
        .collect();
    if sets.is_empty() && removals.is_empty() {
        return Ok(());
    }
    let Some(rw2) = Rw2::read(file_bytes, baseline) else {
        return Ok(());
    };
    for (key, spelling, name) in removals {
        if let Some(refusal) = rw2.check_removal(key, spelling, name) {
            return Err(refusal);
        }
    }
    for (key, value, spelling, name) in sets {
        if let Some(refusal) = rw2.check_set(key, spelling, name, value, false) {
            return Err(refusal);
        }
    }
    Ok(())
}

/// [`refuse_rw2_ifd0_edits`] for the explicit `IFD0:` sets (`assigned`)
/// whose value is the file's own (`baseline`'s): the map cannot tell those
/// from carried rows, so they reach no other check, and every destination
/// ExifTool writes is checked as for any set. `IFD0:Make=Panasonic` where the
/// embedded Make is not Panasonic is a change pinned ExifTool 13.59 makes in
/// the JpgFromRaw; `IFD0:ISO=80` over an outer 0x0017 of 80 creates 0x0037.
/// Both were reported done unchanged. Reads the file only when there is such
/// a set.
pub(crate) fn refuse_rw2_same_value_sets(
    path: &std::path::Path,
    baseline: &MetadataMap,
    desired: &MetadataMap,
    assigned: &[String],
) -> Result<()> {
    let same: MetadataMap = assigned
        .iter()
        .filter(|key| spelling(key).is_some_and(|(spelling, _)| spelling == Spelling::Ifd0))
        .filter_map(|key| {
            let canonical = crate::writers::exif_surgical::canonical_write_key(key, baseline);
            let value = desired.get(key.as_str())?;
            (baseline.get(canonical.as_str()) == Some(value)).then(|| (canonical, value.clone()))
        })
        .collect();
    if same.is_empty() {
        return Ok(());
    }
    let mut head = [0u8; 4];
    let is_rw2 = std::fs::File::open(path)
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut head))
        .is_ok_and(|()| panasonic_raw_order(&head).is_some());
    if !is_rw2 {
        return Ok(());
    }
    let file_bytes = std::fs::read(path).map_err(ExifToolError::from)?;
    let Some(rw2) = Rw2::read(&file_bytes, baseline) else {
        return Ok(());
    };
    for (key, value) in same.iter() {
        let Some((spelling, name)) = spelling(key) else {
            continue;
        };
        if let Some(refusal) = rw2.check_set(key, spelling, name, value, true) {
            return Err(refusal);
        }
    }
    Ok(())
}

/// Whether `scan`'s directory `ifd` holds `value` for `tag`, byte for byte
/// as the planner would encode it under `key` -- or, for a value the reader
/// print-converts, as it would encode the code ExifTool's inverse
/// conversion gives it ([`inverse_print_conv`]): the RW2 reader reports the
/// JpgFromRaw's `IFD0:ResolutionUnit` SHORT 2 as "inches", and an explicit
/// set of that same "inches" is no change (#956 review, rw2_ifd0.rs:539).
fn holds(scan: &ExifScan, ifd: IfdKind, key: &str, tag: &IfdTag, value: &TagValue) -> bool {
    let Some(entry) = scan
        .entries
        .iter()
        .find(|entry| entry.ifd == ifd && entry.tag_id == tag.id)
    else {
        return false;
    };
    let encodes = |value: &TagValue| {
        tag_value_to_field_for_key(key, value, Some(entry.field_type), scan.byte_order).is_ok_and(
            |(field_type, count, native)| {
                field_type == entry.field_type
                    && count == entry.count
                    && native_to_byte_order(field_type, &native, scan.byte_order) == entry.value
            },
        )
    };
    encodes(value) || inverse_print_conv(tag, value).is_some_and(|raw| encodes(&raw))
}

/// ExifTool's `PrintConvInv` of the text `value` for `tag` when the tag's
/// `PrintConv` is a plain integer hash (`Exif::Main` ResolutionUnit and
/// YCbCrPositioning): `ReverseLookup` (Writer.pl
/// 13.59:3609-3650) takes the key whose label is `value`, trailing
/// whitespace dropped, exactly -- else case-insensitively. Only a single
/// match is taken here. ExifTool's prefix and substring passes, `Unknown
/// (..)`, `OTHER` and every other kind of conversion are not modelled and
/// give `None`, so the value is compared as given, and one that differs is a
/// change: refused, never guessed at.
fn inverse_print_conv(tag: &IfdTag, value: &TagValue) -> Option<TagValue> {
    let PrintConv::IntEnum(map) = tag.print_conv else {
        return None;
    };
    let text = value.as_string()?.trim_end();
    let single = |matches: &mut dyn Iterator<Item = i64>| match (matches.next(), matches.next()) {
        (Some(code), None) => Some(TagValue::Integer(code)),
        _ => None,
    };
    single(
        &mut map
            .iter()
            .filter(|(_, label)| *label == text)
            .map(|(code, _)| *code),
    )
    .or_else(|| {
        single(
            &mut map
                .iter()
                .filter(|(_, label)| label.eq_ignore_ascii_case(text))
                .map(|(code, _)| *code),
        )
    })
}

/// Post-condition of a write to a Panasonic RAW/RW2/RWL file: the outer
/// 0x002e record of `out` still locates the JpgFromRaw of `original`, byte
/// for byte. This writer never edits the embedded document
/// ([`refuse_rw2_ifd0_edits`] refuses every edit ExifTool makes there), so
/// a moved, cut or changed one is a defect, refused before anything is
/// written.
pub(crate) fn verify_jpg_from_raw_kept(original: &[u8], out: &[u8]) -> Result<()> {
    if panasonic_raw_order(original).is_none() {
        return Ok(());
    }
    let jpg = |bytes: &[u8]| {
        scan_entries_with_magics(bytes, WALKABLE_TIFF_MAGICS)
            .ok()
            .and_then(|scan| {
                scan.entries
                    .into_iter()
                    .find(|entry| entry.ifd == IfdKind::Ifd0 && entry.tag_id == JPG_FROM_RAW)
            })
            .map(|entry| (entry.field_type, entry.count, entry.value))
    };
    let Some(before) = jpg(original) else {
        return Ok(());
    };
    if jpg(out).as_ref() != Some(&before) {
        return Err(ExifToolError::unsupported_format(
            "EXIF write verification failed: the outer 0x002e record no longer locates the \
             original embedded JpgFromRaw; nothing was written",
        ));
    }
    Ok(())
}
