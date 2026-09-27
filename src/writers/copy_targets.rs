//! Where a `-TagsFromFile` copy writes each tag, as pinned ExifTool 13.59
//! decides it -- and so what oxidex must write, or name as not written.
//!
//! `SetNewValuesFromFile` copies a source tag *by name* (Writer.pl:1254-1590):
//! for each tag it calls `SetNewValue(<name>, <value>)`, which resolves the
//! name exactly as `-<name>=<value>` is resolved (Writer.pl:607-1100): every
//! writable, unprotected candidate of the name gets the value, the
//! highest-priority ones are *created*, and every other one is written only
//! where the file already carries it. A selection (`-all`, a wildcard) copies
//! with `Protected` removed; a tag named without wildcards copies with
//! `Protected => 1`. [`COPY_TARGETS`] is that resolution, captured from the
//! pinned interpreter per writable name and mode
//! (`tools/exiftool-tables/copy_targets_codegen.py`).
//!
//! Which created candidates a destination realises is the per-format
//! directory map `InitWriteDirs` walks (Writer.pl:29-96 `%jpegMap`/
//! `%tiffMap`, PNG.pm:68 `%pngMap`, WritePDF.pl:35 `%pdfMap`): a candidate is
//! created only where its directory can be added to that file. A PNG prefers
//! its own text chunks: a candidate outside the `PNG` group is not created
//! when the same name is being created in `PNG` (Writer.pl:4153-4162). A
//! PDF's Info dictionary is always written, created or overwritten, whether
//! or not the candidate is the preferred one (WritePDF.pl:461).
//!
//! [`COPY_TARGETS`]: super::generated_copy_targets::COPY_TARGETS

use super::generated_copy_targets::{
    AVOID, COPY_TARGETS, CopyTarget, NAMED, NAMED_CREATE, PANASONIC_RAW, PERMANENT,
    PREFERRED_SHIFT, PROTECTED_BINARY_SOURCES, SELECTED, SELECTED_CREATE, STRUCT,
};
use crate::cli::tag_resolution::family1_label;
use crate::core::FileFormat;
use crate::core::metadata_map::MetadataMap;

/// How a tag reached the copy: through a selection (`-all`, `GROUP:all`, a
/// wildcard) or named without wildcards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyMode {
    Selected,
    Named,
}

/// One place 13.59 writes a copied tag in this destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Destination {
    /// The tag written.
    pub name: &'static str,
    pub group0: &'static str,
    /// The family-1 group written to (`IFD0`, `XMP-tiff`, `PNG`, `Canon`).
    pub group1: &'static str,
    /// The value is not the copied one: a `WriteAlso` or structure field
    /// another name's write derives (13.59's `Flash` also writes
    /// `XMP-exif:FlashFired`), or a Composite tag, which is not stored.
    pub derived: bool,
}

/// The directory map `InitWriteDirs` uses for a destination format; `None`
/// for a format whose own writer decides (none that oxidex writes).
fn realised_directories(
    format: FileFormat,
    tiff_structured: bool,
) -> Option<&'static [&'static str]> {
    // Writer.pl %jpegMap (which includes %exifMap).
    const JPEG: &[&str] = &[
        "IFD1",
        "EXIF",
        "ExifIFD",
        "GPS",
        "SubIFD",
        "GlobParamIFD",
        "PrintIM",
        "InteropIFD",
        "MakerNotes",
        "NikonCapture",
        "JFIF",
        "CIFF",
        "IFD0",
        "XMP",
        "ICC_Profile",
        "FlashPix",
        "MPF",
        "Meta",
        "MetaIFD",
        "RMETA",
        "SEAL",
        "AROT",
        "JUMBF",
        "Ducky",
        "Photoshop",
        "Adobe",
        "IPTC",
        "CanonVRD",
        "Comment",
    ];
    // Writer.pl %tiffMap.
    const TIFF: &[&str] = &[
        "IFD0",
        "IFD1",
        "XMP",
        "ICC_Profile",
        "ExifIFD",
        "GPS",
        "SubIFD",
        "GlobParamIFD",
        "PrintIM",
        "IPTC",
        "Photoshop",
        "SEAL",
        "InteropIFD",
        "MakerNotes",
        "CanonVRD",
        "NikonCapture",
        "PhaseOne",
    ];
    // PNG.pm %pngMap, plus the PNG chunks themselves (ADD_PNG).
    const PNG: &[&str] = &[
        "IFD1",
        "EXIF",
        "ExifIFD",
        "GPS",
        "SubIFD",
        "GlobParamIFD",
        "PrintIM",
        "InteropIFD",
        "MakerNotes",
        "IFD0",
        "XMP",
        "ICC_Profile",
        "Photoshop",
        "PNG-pHYs",
        "JUMBF",
        "IPTC",
        "PNG",
    ];
    // WritePDF.pl %pdfMap (the Info dictionary is handled apart).
    const PDF: &[&str] = &["XMP"];
    match format {
        FileFormat::JPEG => Some(JPEG),
        FileFormat::PNG => Some(PNG),
        FileFormat::PDF => Some(PDF),
        _ if tiff_structured => Some(TIFF),
        _ => None,
    }
}

fn candidates(name: &str) -> &'static [CopyTarget] {
    let lower = name.to_ascii_lowercase();
    COPY_TARGETS
        .binary_search_by(|(key, _)| (*key).cmp(lower.as_str()))
        .map_or(&[], |index| COPY_TARGETS[index].1)
}

/// Whether 13.59 has anywhere at all to write `name` in `mode` -- whether
/// `SetNewValue` sets it (and so whether the copy "set" a tag, which decides
/// 13.59's `No writable tags set from ...` warning).
pub(crate) fn is_copyable(name: &str, mode: CopyMode) -> bool {
    let bit = match mode {
        CopyMode::Selected => SELECTED,
        CopyMode::Named => NAMED,
    };
    candidates(name)
        .iter()
        .any(|target| target.modes & bit != 0)
}

/// Whether a selection skips a source tag: `Protected` `Binary` tags are
/// never copied through a wildcard (Writer.pl:1570-1572).
pub(crate) fn is_protected_binary_source(group0: &str, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    PROTECTED_BINARY_SOURCES
        .iter()
        .any(|(group, tag)| group.eq_ignore_ascii_case(group0) && *tag == lower)
}

/// The file a copy writes into, as the destination rules need it.
pub(crate) struct DestinationFile<'a> {
    pub format: FileFormat,
    /// A TIFF-structured file whose IFD0 is `Exif::Main` (a TIFF, a
    /// TIFF-based RAW): written with Writer.pl's `%tiffMap`.
    pub tiff_structured: bool,
    /// A Panasonic RAW (TIFF identifier 0x55), whose IFD0 is
    /// `PanasonicRaw::Main`.
    pub panasonic_raw: bool,
    /// The file's current tags.
    pub existing: &'a MetadataMap,
}

/// Whether `existing` carries a `name` row in `group1`.
fn carries(existing: &MetadataMap, group1: &str, name: &str) -> bool {
    existing.keyed_occurrences().any(|(_, occurrence)| {
        occurrence.name.eq_ignore_ascii_case(name)
            && family1_label(occurrence).eq_ignore_ascii_case(group1)
    })
}

/// The EXIF directories a group name can select: `SetNewValue` writes any
/// EXIF candidate to the IFD named (Writer.pl:652-661, `$writeGroup =
/// $ifdName`).
const EXIF_DIRECTORIES: &[&str] = &[
    "IFD0",
    "IFD1",
    "ExifIFD",
    "GPS",
    "InteropIFD",
    "SubIFD",
    "SubIFD1",
    "SubIFD2",
    "GlobParamIFD",
];

/// The candidates `SetNewValue` creates when a group is named: every
/// candidate in the group ranks 1000 plus its `Preferred` level; the
/// highest are preferred, an `Avoid` one dropping out when another is
/// preferred beside it; a preferred or `Preferred` candidate is created
/// unless it is `Permanent` (Writer.pl:622-825, 1040-1057).
fn grouped_creation(listed: &[&CopyTarget]) -> Vec<bool> {
    let level = |target: &CopyTarget| i32::from(target.flags >> PREFERRED_SHIFT);
    let highest = listed.iter().map(|t| level(t)).max().unwrap_or(0);
    let mut preferred: Vec<bool> = listed.iter().map(|t| level(t) == highest).collect();
    let chosen = preferred.iter().filter(|p| **p).count();
    let avoided = listed
        .iter()
        .zip(&preferred)
        .filter(|(t, p)| **p && t.flags & AVOID != 0)
        .count();
    if avoided > 0 && avoided < chosen {
        for (target, p) in listed.iter().zip(preferred.iter_mut()) {
            if target.flags & AVOID != 0 {
                *p = false;
            }
        }
    }
    listed
        .iter()
        .zip(preferred)
        .map(|(target, p)| (p || level(target) > 0) && target.flags & PERMANENT == 0)
        .collect()
}

/// One tag a copy sets: `SetNewValue(<name>, <value>)`, optionally with a
/// `Group`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CopyRequest<'a> {
    pub name: &'a str,
    pub mode: CopyMode,
    /// `-GROUP:all` / `-GROUP:TAG`: `SetNewValue` with `Group` -- only
    /// candidates in that family-0 or family-1 group, ranked among
    /// themselves (Writer.pl:622-693).
    pub group: Option<&'a str>,
    /// The copied value is a structure: a structure candidate refuses any
    /// other value (`Improperly formed structure`), and any other candidate
    /// refuses a structure (`Can't write a structure to`), Writer.pl:916-918.
    pub structure: bool,
}

/// One candidate of one request, before the destination decides.
struct Candidate<'a> {
    request: usize,
    target: &'a CopyTarget,
    group1: &'static str,
    dir: &'static str,
    /// `IsCreating`.
    creating: bool,
    /// A created candidate whose directory this file does not add on its
    /// account: a PNG prefers its own chunks (Writer.pl:4153-4162).
    deferred: bool,
}

/// The parent directory `InitWriteDirs` adds with a directory (Writer.pl
/// `%exifMap`, PNG.pm `%pngMap`): an ExifIFD entry creates IFD0 too.
fn parent(dir: &str) -> Option<&'static str> {
    match dir {
        "ExifIFD" | "GPS" | "IFD1" | "SubIFD" | "GlobParamIFD" | "PrintIM" => Some("IFD0"),
        "InteropIFD" | "MakerNotes" => Some("ExifIFD"),
        "IPTC" => Some("Photoshop"),
        _ => None,
    }
}

/// Every place pinned 13.59 writes each of `requests` in `file`, request by
/// request.
///
/// A candidate is written where the file already carries its tag, and
/// created where `IsCreating` and its directory is written: a directory is
/// added when a created candidate needs it (and its parents with it), and
/// then every created candidate in it is written -- also one whose own
/// creation deferred to a PNG's text chunks (`InitWriteDirs` decides which
/// directories to add; the directory writers then create every
/// `IsCreating` tag, Writer.pl:4137-4230). A PDF prefers XMP: every XMP
/// candidate that is not `Avoid` is created (WriteXMP.pl:788, 1287), and
/// its Info dictionary is always written (WritePDF.pl:461). The `File`
/// group's writable tags (`FileModifyDate`, `ExifByteOrder`) are written by
/// `WriteInfo` for every format, not through a directory.
pub(crate) fn destinations(
    requests: &[CopyRequest],
    file: &DestinationFile,
) -> Vec<Vec<Destination>> {
    let format = file.format;
    let directories = realised_directories(format, file.tiff_structured).unwrap_or(&[]);
    let mut all: Vec<Candidate> = Vec::new();
    for (index, request) in requests.iter().enumerate() {
        let (present, create) = match request.mode {
            CopyMode::Selected => (SELECTED, SELECTED_CREATE),
            CopyMode::Named => (NAMED, NAMED_CREATE),
        };
        let group = request.group;
        let exif_directory = group.and_then(|group| {
            EXIF_DIRECTORIES
                .iter()
                .copied()
                .find(|directory| directory.eq_ignore_ascii_case(group))
        });
        let in_group = |target: &CopyTarget| {
            group.is_none_or(|group| {
                target.group0.eq_ignore_ascii_case(group)
                    || target.group1.eq_ignore_ascii_case(group)
                    || (exif_directory.is_some() && target.group0 == "EXIF")
            })
        };
        let listed: Vec<&CopyTarget> = candidates(request.name)
            .iter()
            .filter(|target| {
                target.modes & present != 0
                    && in_group(target)
                    && (target.modes & STRUCT != 0) == request.structure
                    && (target.modes & PANASONIC_RAW == 0) == !file.panasonic_raw
            })
            .collect();
        let creating: Vec<bool> = if group.is_some() {
            grouped_creation(&listed)
        } else {
            listed
                .iter()
                .map(|target| target.modes & create != 0)
                .collect()
        };
        // `CreateGroups`: the family-0 groups this name is created in.
        let png_preferred = format == FileFormat::PNG
            && listed
                .iter()
                .zip(&creating)
                .any(|(target, creates)| *creates && target.group0 == "PNG");
        for (target, creating) in listed.into_iter().zip(creating) {
            // A named EXIF directory is where an EXIF candidate goes.
            let (group1, dir) = match exif_directory {
                Some(directory) if target.group0 == "EXIF" => (directory, directory),
                _ => (target.group1, target.dir),
            };
            let creating = creating
                || (format == FileFormat::PDF
                    && target.group0 == "XMP"
                    && target.flags & AVOID == 0);
            all.push(Candidate {
                request: index,
                target,
                group1,
                dir,
                creating,
                deferred: png_preferred && target.group0 != "PNG",
            });
        }
    }
    // The directories written: those the file has, and those a created
    // candidate adds (with their parents).
    let mut written_dirs: Vec<&str> = Vec::new();
    for (_, occurrence) in file.existing.keyed_occurrences() {
        let group1 = family1_label(occurrence);
        let dir = if group1.starts_with("XMP") {
            "XMP"
        } else {
            group1
        };
        if !written_dirs.contains(&dir) {
            written_dirs.push(dir);
        }
    }
    for candidate in &all {
        if !(candidate.creating && !candidate.deferred && directories.contains(&candidate.dir)) {
            continue;
        }
        let mut dir = Some(candidate.dir);
        while let Some(current) = dir {
            if !written_dirs.contains(&current) {
                written_dirs.push(current);
            }
            dir = parent(current);
        }
    }
    let mut out: Vec<Vec<Destination>> = vec![Vec::new(); requests.len()];
    for candidate in all {
        let target = candidate.target;
        let created = candidate.creating
            && (candidate.dir == "File"
                || (directories.contains(&candidate.dir) && written_dirs.contains(&candidate.dir)));
        let pdf_info = format == FileFormat::PDF && candidate.dir == "PDF";
        if !(created || pdf_info || carries(file.existing, candidate.group1, target.name)) {
            continue;
        }
        let name = requests[candidate.request].name;
        let destination = Destination {
            name: target.name,
            group0: target.group0,
            group1: candidate.group1,
            derived: !target.name.eq_ignore_ascii_case(name) || target.group0 == "Composite",
        };
        let list = &mut out[candidate.request];
        if !list.contains(&destination) {
            list.push(destination);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::tag_value::TagValue;

    fn file(format: FileFormat, existing: &MetadataMap) -> DestinationFile<'_> {
        DestinationFile {
            format,
            tiff_structured: false,
            panasonic_raw: false,
            existing,
        }
    }

    fn request(name: &str, mode: CopyMode) -> CopyRequest<'_> {
        CopyRequest {
            name,
            mode,
            group: None,
            structure: false,
        }
    }

    fn places(
        name: &str,
        mode: CopyMode,
        format: FileFormat,
        existing: &MetadataMap,
    ) -> Vec<String> {
        let mut places: Vec<String> = destinations(&[request(name, mode)], &file(format, existing))
            .remove(0)
            .into_iter()
            .map(|d| format!("{}:{}", d.group1, d.name))
            .collect();
        places.sort();
        places
    }

    #[test]
    fn capture_is_sorted_and_pinned() {
        assert!(COPY_TARGETS.windows(2).all(|pair| pair[0].0 < pair[1].0));
        let pinned = include_str!("../../.exiftool-version").trim();
        assert_eq!(
            super::super::generated_copy_targets::COPY_TARGETS_CAPTURE.exiftool_version,
            pinned
        );
    }

    /// Pinned 13.59, `-TagsFromFile synthetic_001.jpg -all` (`-v2`): Make is
    /// created in IFD0 (a JPEG has no PNG text chunk or MIE group to create
    /// it in); IFD0 ImageWidth is Protected, so the File row's ImageWidth is
    /// created in XMP-tiff.
    #[test]
    fn a_jpeg_destination_creates_the_preferred_group() {
        let empty = MetadataMap::new();
        assert_eq!(
            places("Make", CopyMode::Selected, FileFormat::JPEG, &empty),
            ["IFD0:Make"]
        );
        assert_eq!(
            places("ImageWidth", CopyMode::Selected, FileFormat::JPEG, &empty),
            ["XMP-tiff:ImageWidth"]
        );
        assert_eq!(
            places("ImageWidth", CopyMode::Named, FileFormat::JPEG, &empty),
            ["IFD0:ImageWidth"]
        );
    }

    /// Pinned 13.59 onto tests/fixtures/png/sample.png: `Make` goes to the
    /// PNG text chunk, not a new IFD0 -- and to the eXIf IFD0 Make the file
    /// already carries.
    #[test]
    fn a_png_prefers_its_text_chunks_and_updates_existing_exif() {
        let empty = MetadataMap::new();
        assert_eq!(
            places("Make", CopyMode::Selected, FileFormat::PNG, &empty),
            ["PNG:Make"]
        );
        let mut exif = MetadataMap::new();
        exif.insert("IFD0:Make", TagValue::new_string("PNG EXIF Test"));
        assert_eq!(
            places("Make", CopyMode::Named, FileFormat::PNG, &exif),
            ["IFD0:Make", "PNG:Make"]
        );
    }

    /// Pinned 13.59, a PDF onto itself: the Info Title is rewritten and
    /// XMP-dc Title created.
    #[test]
    fn a_pdf_writes_info_and_creates_xmp() {
        let empty = MetadataMap::new();
        assert_eq!(
            places("Title", CopyMode::Selected, FileFormat::PDF, &empty),
            ["PDF:Title", "XMP-dc:Title"]
        );
    }

    #[test]
    fn a_group_restricts_and_creates() {
        let empty = MetadataMap::new();
        let grouped = CopyRequest {
            group: Some("EXIF"),
            ..request("XResolution", CopyMode::Selected)
        };
        let found: Vec<_> = destinations(&[grouped], &file(FileFormat::JPEG, &empty))
            .remove(0)
            .into_iter()
            .map(|d| format!("{}:{}", d.group1, d.name))
            .collect();
        assert_eq!(found, ["IFD0:XResolution"]);
    }
}
