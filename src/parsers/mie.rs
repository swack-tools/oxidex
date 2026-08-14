//! MIE trailer marker parser.
//!
//! ExifTool 13.59 (`ExifTool.pm:7007-7010`, `MIE.pm:1693-1730`) recognizes a
//! MIE trailer from the final empty `zmie` element in a main `0MIE` group. The
//! final element is followed by the group terminator, whose length reaches
//! back exactly to the `0MIE` header. The trailer may sit inside later
//! trailers, so candidates are scanned from the end and validated at both
//! ends rather than read only at EOF.

use crate::core::{MetadataMap, TagValue};
use crate::parsers::trailer;

const GROUP_HEADER: &[u8; 4] = b"0MIE";
const SHORT_TRAILER_MARKER: &[u8] = b"~\0\x04\0zmie~\0\0\x06";
const LONG_TRAILER_MARKER: &[u8] = b"~\0\x04\0zmie~\0\0\x0a";

/// Extracts the supported MIE trailer tags from a valid MIE trailer.
///
/// `TrailerSignature` is a marker with an empty value.  The trailer is still
/// a normal MIE hierarchy, so the one losslessly-described nested route seen
/// in the pinned JPEG fixture -- `MIE-Meta` / `MIE-Doc` / UTF-8 `Copyright` --
/// is decoded as well.
pub fn parse_mie_trailer(file: &[u8]) -> MetadataMap {
    let mut metadata = MetadataMap::new();
    if let Some(trailer) = find_trailer(file) {
        // ExifTool's group-1 name is MIE-Main, but the comparison harness's
        // canonical family for MIE trailers is MIE.
        metadata.insert("MIE:TrailerSignature", TagValue::String(String::new()));
        extract_document_copyright(file, trailer, &mut metadata);
    }
    metadata
}

#[derive(Clone, Copy)]
struct MieTrailer {
    start: usize,
    end: usize,
}

/// Finds the last valid MIE trailer footer, including ExifTool's two supported
/// data-length encodings: four bytes (`0x06`) and eight bytes (`0x0a`).
fn find_trailer(file: &[u8]) -> Option<MieTrailer> {
    let short = trailer::find_last(
        file,
        SHORT_TRAILER_MARKER.len(),
        SHORT_TRAILER_MARKER,
        SHORT_TRAILER_MARKER.len() + 6,
        |file, end| trailer_start(file, end, 4).map(|start| MieTrailer { start, end }),
    );
    let long = trailer::find_last(
        file,
        LONG_TRAILER_MARKER.len(),
        LONG_TRAILER_MARKER,
        LONG_TRAILER_MARKER.len() + 10,
        |file, end| trailer_start(file, end, 8).map(|start| MieTrailer { start, end }),
    );
    short
        .into_iter()
        .chain(long)
        .max_by_key(|trailer| trailer.end)
}

/// Validates ExifTool's second trailer boundary: the footer's byte-order-aware
/// length must point to a main MIE group header.
fn trailer_start(file: &[u8], end: usize, length_width: usize) -> Option<usize> {
    let footer_len = 4 + length_width + 2;
    let Some(footer) = end.checked_sub(footer_len).and_then(|at| file.get(at..end)) else {
        return None;
    };
    if footer[..4] != [b'~', 0, 0, if length_width == 4 { 6 } else { 10 }]
        || footer[footer_len - 1] != length_width as u8
    {
        return None;
    }
    let length_bytes = &footer[4..4 + length_width];
    let length = match (footer[footer_len - 2], length_width) {
        (0x10, 4) => u32::from_be_bytes(length_bytes.try_into().expect("four bytes")) as u64,
        (0x18, 4) => u32::from_le_bytes(length_bytes.try_into().expect("four bytes")) as u64,
        (0x10, 8) => u64::from_be_bytes(length_bytes.try_into().expect("eight bytes")),
        (0x18, 8) => u64::from_le_bytes(length_bytes.try_into().expect("eight bytes")),
        _ => return None,
    };
    let Ok(length) = usize::try_from(length) else {
        return None;
    };
    let Some(group) = end.checked_sub(length).and_then(|at| file.get(at..)) else {
        return None;
    };
    (length >= 12
        && group.len() >= 8
        && group[0] == b'~'
        && matches!(group[1], 0x10 | 0x18)
        && group[2] == 4
        && group[4..8] == *GROUP_HEADER)
        .then_some(end - length)
}

/// Decode exactly the `0MIE` / `Meta` / `Document` / UTF-8 `Copyright` path
/// declared by MIE.pm.  Group elements have no inline data and are delimited
/// by the normal empty MIE terminator; other MIE tables and compression stay
/// deliberately out of scope.
fn extract_document_copyright(file: &[u8], trailer: MieTrailer, metadata: &mut MetadataMap) {
    let mut cursor = trailer.start;
    let mut groups = Vec::new();
    let mut little_endian = false;

    while cursor < trailer.end {
        let Some(header) = file.get(cursor..cursor + 4) else {
            return;
        };
        if header[0] != b'~' {
            return;
        }
        let format = header[1];
        let tag_len = usize::from(header[2]);
        let len_code = header[3];
        let tag_start = cursor + 4;
        let Some(tag_bytes) = file.get(tag_start..tag_start + tag_len) else {
            return;
        };
        let tag_end = tag_start + tag_len;
        let Some((data_len, length_width)) =
            mie_data_length(file, tag_end, len_code, little_endian)
        else {
            return;
        };
        let data_start = tag_end + length_width;
        let Some(data) = file.get(data_start..data_start + data_len) else {
            return;
        };
        cursor = data_start + data_len;

        // Empty format-0/tag-0/data-0 element terminates the current group.
        if format == 0 && tag_bytes.is_empty() && data.is_empty() {
            groups.pop();
            continue;
        }

        let Ok(tag) = std::str::from_utf8(tag_bytes) else {
            return;
        };
        if format & 0xf0 == 0x10 {
            // MIE.pm's Main and Meta tables define these as subdirectories.
            // The byte-order modifier belongs to the group it opens.
            little_endian = format & 0x08 != 0;
            groups.push(tag);
            continue;
        }

        if tag == "Copyright"
            && format == 0x28
            && groups.as_slice() == ["0MIE", "Meta", "Document"]
            && let Ok(value) = std::str::from_utf8(data)
        {
            metadata.insert("MIE:Copyright", TagValue::String(value.to_string()));
        }
    }
}

/// MIE 1.1's variable-width data-length field.  The byte order of extended
/// lengths is inherited from the containing MIE group.
fn mie_data_length(
    file: &[u8],
    at: usize,
    code: u8,
    little_endian: bool,
) -> Option<(usize, usize)> {
    match code {
        0..=252 => Some((usize::from(code), 0)),
        253 => {
            let bytes: [u8; 8] = file.get(at..at + 8)?.try_into().ok()?;
            usize::try_from(if little_endian {
                u64::from_le_bytes(bytes)
            } else {
                u64::from_be_bytes(bytes)
            })
            .ok()
            .map(|length| (length, 8))
        }
        254 => {
            let bytes: [u8; 4] = file.get(at..at + 4)?.try_into().ok()?;
            Some((
                if little_endian {
                    u32::from_le_bytes(bytes)
                } else {
                    u32::from_be_bytes(bytes)
                } as usize,
                4,
            ))
        }
        255 => {
            let bytes: [u8; 2] = file.get(at..at + 2)?.try_into().ok()?;
            Some((
                usize::from(if little_endian {
                    u16::from_le_bytes(bytes)
                } else {
                    u16::from_be_bytes(bytes)
                }),
                2,
            ))
        }
    }
}

/// Element budget for [`subfile_identifier`], well past any real MIE file's
/// top-level tag count. Bounds a malformed file rather than modelling a real
/// limit ExifTool enforces.
const MAX_TOP_LEVEL_ELEMENTS: usize = 10_000;

/// `%mimeType{MIE}`, `application/x-mie` -- the base type [`combine_mime`]
/// starts from. Not read from the generated table because this file has no
/// `crate::filetype::identify` call in its path to ask.
const MIE_BASE_MIME: &str = "application/x-mie";

/// ExifTool's `MIMEType` for a `.mie` file: `application/x-mie`, sharpened to
/// name the subfile it encapsulates.
///
/// A MIE file's own MIME type is generic; a real answer names the payload
/// too, e.g. `image/x-mie-jpeg` for a wrapped JPEG. `ProcessMIE` builds that
/// from the top-level `0Type` or `2MIME` tag -- whichever was read last
/// (MIE.pm:1609, 1681):
///
/// ```text
///     $mime = $val if $tag eq '0Type' or $tag eq '2MIME';
///     ...
///     $mime and not $$dirInfo{Parent} and $et->ModifyMimeType($mime);
/// ```
///
/// `not $$dirInfo{Parent}` is why only the file's own top-level group is
/// searched: a subfile nested inside a MIE group (an embedded thumbnail, say)
/// carries its own `0Type`/`2MIME` too, and those must not leak into the
/// outer file's MIME type.
///
/// `None` when the file has no `0Type`/`2MIME` at its top level, or the walk
/// cannot make sense of the structure -- either way, the caller's own
/// `application/x-mie` default from `%mimeType` stands.
pub(crate) fn document_mime_type(file: &[u8]) -> Option<String> {
    let mime = subfile_identifier(file)?;
    combine_mime(MIE_BASE_MIME, &mime)
}

/// The value of the last `0Type` or `2MIME` tag directly inside the file's
/// top-level `0MIE` group.
///
/// Mirrors `ProcessMIE`'s file-level header handling followed by
/// `ProcessMIEGroup`'s element loop, restricted to depth 1: a nested group
/// element is skipped over rather than entered when its own data length says
/// where it ends (MIE.pm:1699-1712 for the header, 1483-1580 for the element
/// loop -- the tag-value capture is 1609, quoted on [`document_mime_type`]).
///
/// A group element can also declare a length of zero, meaning "read my
/// contents from right here in the stream" rather than "skip N bytes" --
/// `MIE.mie`'s `Meta` and `Camera` groups are both this shape. Those are
/// still skipped correctly: entering them just means the *next* elements read
/// are that group's own tags, one level deeper, until its terminator (a
/// zero-`TagLength` element) is reached and depth returns to where it was.
fn subfile_identifier(file: &[u8]) -> Option<String> {
    // File-level header: Sync + FormatCode + TagLength(=4) + DataLength +
    // "0MIE" (ProcessMIE's own regex, MIE.pm:1701:
    // `/^~(\x10|\x18)\x04(.)0MIE/s`). TagLength is asserted rather than read,
    // matching the literal `\x04` in that pattern.
    let header = file.get(0..8)?;
    if header[0] != b'~' || header[2] != 4 || &header[4..8] != b"0MIE" {
        return None;
    }
    // 0x10 = MM (big-endian), 0x18 = II (little-endian) -- the same code as
    // any other MIE group, just spelled out here because there is no
    // enclosing group to have set it already.
    let doc_little_endian = match header[1] {
        0x10 => false,
        0x18 => true,
        _ => return None,
    };

    // ProcessMIE never decodes this length -- it only needs to know how many
    // extension bytes to step over, which is `1 << (256 - code)` for any code
    // above 252 (MIE.pm:1706-1707). The actual byte count skipped for 253/
    // 254/255 is 8/4/2, i.e. `mie_data_length`'s own extension widths; reusing
    // it here keeps the two derivations from silently drifting apart.
    let mut pos = 8usize;
    if header[3] > 252 {
        let (_, extension_width) = mie_data_length(file, pos, header[3], doc_little_endian)?;
        pos += extension_width;
    }

    // One entry per group currently open below the top level, holding the
    // byte order in effect for elements at that depth. Empty means "still
    // directly inside the top-level 0MIE group", which is the only depth
    // `not $$dirInfo{Parent}` allows a capture at.
    let mut open_groups: Vec<bool> = Vec::new();
    let mut mime = None;

    for _ in 0..MAX_TOP_LEVEL_ELEMENTS {
        let header = file.get(pos..pos + 4)?;
        if header[0] != b'~' {
            return mime;
        }
        let format = header[1];
        let tag_len = usize::from(header[2]);
        let length_code = header[3];
        let little_endian = open_groups.last().copied().unwrap_or(doc_little_endian);

        let tag_start = pos + 4;
        let tag = file.get(tag_start..tag_start + tag_len)?;
        let data_pos = tag_start + tag_len;
        let (data_len, extension_width) =
            mie_data_length(file, data_pos, length_code, little_endian)?;
        let data_start = data_pos + extension_width;
        let data = file.get(data_start..data_start + data_len)?;
        pos = data_start + data_len;

        if tag_len == 0 {
            // Group terminator. Closes the innermost open group, or -- with
            // none open -- ends the top-level group itself, which is as far
            // as this function ever needs to look.
            if open_groups.pop().is_none() {
                return mime;
            }
            continue;
        }

        if format & 0xf0 == 0x10 {
            // A group. `format & 0x08` is its own byte order, which governs
            // its contents but not this element's own header -- exactly the
            // asymmetry `little_endian` above already captures for every
            // element, group or not.
            if data_len == 0 {
                // Streamed inline: its elements are read next, one level
                // deeper, ending at its own terminator.
                open_groups.push(format & 0x08 != 0);
            }
            // A self-contained group (data_len > 0) needs nothing further:
            // `pos` already skipped past its entire encoded form above.
            continue;
        }

        if open_groups.is_empty() && (tag == b"0Type" || tag == b"2MIME") {
            mime = std::str::from_utf8(data).ok().map(str::to_string);
        }
    }
    mime
}

/// `ModifyMimeType`'s `a/b + c/d => c/b-d` (ExifTool.pm:9748-9761).
///
/// `new_type` is either a literal MIME type (contains `/`) or a bare file
/// type name to resolve through `%mimeType` first -- `0Type` gives "JPEG",
/// not a MIME type, while `2MIME` (when present) already is one.
fn combine_mime(old: &str, new_type: &str) -> Option<String> {
    let resolved;
    let new_type = if new_type.contains('/') {
        new_type
    } else {
        resolved = crate::filetype::mime_for_type(new_type)?;
        resolved
    };
    let (_, old_subtype) = old.split_once('/')?;
    let (new_type_part, new_subtype) = new_type.split_once('/')?;
    let new_subtype = new_subtype.strip_prefix("x-").unwrap_or(new_subtype);
    Some(format!("{new_type_part}/{old_subtype}-{new_subtype}"))
}

// ============================================================================
// Standalone `.mie` document parser
// ============================================================================
//
// Everything above this point exists to read a MIE trailer *appended* to
// another format (a JPEG's `zmie` marker). A standalone `.mie` file is the
// same element grammar with no host file around it, and until this step
// nothing routed `FileFormat::MIE` to a parser at all -- `add_identity_tags`
// named it correctly (`File:FileType: MIE`, off the same `%magicNumber`
// pattern [`document_mime_type`] cites) and stopped there, so a real `.mie`
// file reported zero of its own tags (`Detected is not parsed`, AGENTS.md).
//
// This walks the file's element tree with the same primitives
// [`subfile_identifier`] already uses ([`mie_data_length`], the `~ format
// tagLen dataLen tag data` element shape, `format & 0xf0 == 0x10` group
// detection, the zero-`TagLength` terminator rule at MIE.pm:1483-1512's
// `unless ($tagLen) { ... last }`), but where that function only chases one
// path (`0Type`/`2MIME`) to sharpen a MIME type, this one visits every
// element and emits a tag for each one recognised in [`MieTable::leaf`],
// against the nine group tables reachable from `t/images/MIE.mie`
// (`Main` MIE.pm:124-219, `Meta` 219-286 -- pure routing, no leaf tags of
// its own in this fixture -- `Camera` 565-624, `Flash` 693-708, `Lens`
// 649-682, `Orient` 629-649, `Doc` 294-321, `Geo` 321-346, `Image` 438-479,
// `Thumbnail` 509-532).

use crate::core::FileReader;

/// Parses metadata from a standalone `.mie` file.
pub fn parse_mie_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let size = reader.size() as usize;
    let data = reader.read(0, size).map_err(|e| e.to_string())?;
    let mut metadata = MetadataMap::new();
    parse_mie_document(data, &mut metadata);
    Ok(metadata)
}

/// The group tables reachable from `t/images/MIE.mie`'s top-level `0MIE`
/// group. `Skip` is not one of MIE.pm's own tables -- it stands in for any
/// group this walker does not recognise, so its content is still consumed
/// (keeping the element stream in sync) without emitting tags for it or
/// misattributing its children to the enclosing table.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MieTable {
    Main,
    Meta,
    Camera,
    Flash,
    Lens,
    Orient,
    Doc,
    Geo,
    Image,
    Thumbnail,
    Skip,
}

/// How an element's raw bytes become a [`TagValue`].
#[derive(Clone, Copy)]
enum MieValue {
    /// Text, decoded per the element's own format byte (`0x20` Latin-1,
    /// `0x28` UTF-8, `0x29` UTF-16, `0x2a` UTF-32).
    Str,
    /// A `List => 1` tag: `_list`-format bytes split on NUL, or a single
    /// non-list value treated as a one-item list either way -- MIE.pm's
    /// `References` in `t/images/MIE.mie` is `List => 1` but encoded as a
    /// single plain string, and displays unchanged. Joined with `", "`,
    /// ExifTool's default list-value separator.
    StringList,
    /// A plain integer, no `PrintConv` (`ColorTemperature`, `ISO`, ...).
    Int,
    /// A single rational, no `PrintConv`: displayed as `numerator /
    /// denominator` reduced to a decimal (`FNumber`, `Rotation`, ...).
    Rational,
    /// `Camera::ExposureTime`'s own `PrintConv =>
    /// 'Image::ExifTool::Exif::PrintExposureTime($val)'` (`MPC.pm` FLAC
    /// heritage aside, `Exif.pm:5701-5711`): sub-quarter-second values print
    /// as `1/N`, everything else as a trimmed one-decimal number.
    ExposureTime,
    /// `ImageSize`/`ThumbnailImageSize`: a sequence of integers, `PrintConv
    /// => '$val=~tr/ /x/;$val'` joins them with `x` (`Image.pm:456-464`).
    Dimensions,
    /// `Resolution`: the same `tr/ /x/` join over a sequence of rationals,
    /// plus MIE's units suffix (`ProcessMIEGroup`, MIE.pm:1655-1658:
    /// `$val .= "($units)" if defined $units`) when the on-disk tag name
    /// carried one -- `t/images/MIE.mie` writes the literal tag bytes
    /// `Resolution(/cm)`, not the `/in` default.
    Resolution,
    /// Raw bytes (`SubfileData`, `ThumbnailImage`), rendered the same way
    /// `ape.rs`'s cover-art item is: `TagValue::Binary`.
    Binary,
}

impl MieTable {
    /// This table's declared `GROUPS => { 1 => ... }`, or `None` for `Meta`
    /// and `Skip`, neither of which emits a tag directly.
    fn group1(self) -> Option<&'static str> {
        match self {
            MieTable::Main => Some("MIE-Main"),
            MieTable::Meta | MieTable::Skip => None,
            MieTable::Camera => Some("MIE-Camera"),
            MieTable::Flash => Some("MIE-Flash"),
            MieTable::Lens => Some("MIE-Lens"),
            MieTable::Orient => Some("MIE-Orient"),
            MieTable::Doc => Some("MIE-Doc"),
            MieTable::Geo => Some("MIE-Geo"),
            MieTable::Image => Some("MIE-Image"),
            MieTable::Thumbnail => Some("MIE-Thumbnail"),
        }
    }

    /// The child table a group-format element named `tag` opens from this
    /// table, per this table's own `SubDirectory` entries. `Skip` for a
    /// group `Skip` itself does not recognise, so its content is walked
    /// (and safely discarded) rather than mis-attributed to the parent.
    fn child(self, tag: &str) -> MieTable {
        use MieTable::*;
        match (self, tag) {
            (Main, "Meta") => Meta,
            (Meta, "Camera") => Camera,
            (Meta, "Document") => Doc,
            (Meta, "Geo") => Geo,
            (Meta, "Image") => Image,
            (Meta, "Thumbnail") => Thumbnail,
            (Camera, "Flash") => Flash,
            (Camera, "Lens") => Lens,
            (Camera, "Orientation") => Orient,
            _ => Skip,
        }
    }

    /// The output tag name and value kind for a leaf element named `tag`
    /// directly inside this table, or `None` for anything this table
    /// doesn't declare (MIE.pm's own many other tags -- `Brightness`,
    /// `GPS`, `Audio`, etc -- are out of scope: no sample in the corpus
    /// exercises them, see this module's `parse_mie_document` doc comment).
    fn leaf(self, tag: &str) -> Option<(&'static str, MieValue)> {
        use MieValue::*;
        match (self, tag) {
            (MieTable::Main, "0Type") => Some(("SubfileType", Str)),
            (MieTable::Main, "0Vers") => Some(("MIEVersion", Str)),
            (MieTable::Main, "1Name") => Some(("SubfileName", Str)),
            (MieTable::Main, "2MIME") => Some(("SubfileMIMEType", Str)),
            (MieTable::Main, "data") => Some(("SubfileData", Binary)),

            (MieTable::Camera, "ColorTemperature") => Some(("ColorTemperature", Int)),
            (MieTable::Camera, "Contrast") => Some(("Contrast", Int)),
            (MieTable::Camera, "ExposureComp") => Some(("ExposureCompensation", Rational)),
            (MieTable::Camera, "ExposureMode") => Some(("ExposureMode", Str)),
            (MieTable::Camera, "ExposureTime") => Some(("ExposureTime", ExposureTime)),
            (MieTable::Camera, "FocusMode") => Some(("FocusMode", Str)),
            (MieTable::Camera, "ISO") => Some(("ISO", Int)),
            (MieTable::Camera, "Make") => Some(("Make", Str)),
            (MieTable::Camera, "Model") => Some(("Model", Str)),
            (MieTable::Camera, "OwnerName") => Some(("OwnerName", Str)),
            (MieTable::Camera, "Saturation") => Some(("Saturation", Int)),
            (MieTable::Camera, "SerialNumber") => Some(("SerialNumber", Str)),
            (MieTable::Camera, "Sharpness") => Some(("Sharpness", Int)),
            (MieTable::Camera, "ShootingMode") => Some(("ShootingMode", Str)),

            (MieTable::Flash, "ExposureComp") => Some(("FlashExposureComp", Rational)),
            (MieTable::Flash, "GuideNumber") => Some(("FlashGuideNumber", Str)),

            (MieTable::Lens, "FNumber") => Some(("FNumber", Rational)),
            (MieTable::Lens, "MaxAperture") => Some(("MaxAperture", Rational)),
            (MieTable::Lens, "MinAperture") => Some(("MinAperture", Rational)),

            (MieTable::Orient, "Rotation") => Some(("Rotation", Rational)),

            (MieTable::Doc, "Comment") => Some(("Comment", Str)),
            (MieTable::Doc, "Copyright") => Some(("Copyright", Str)),
            (MieTable::Doc, "CreateDate") => Some(("CreateDate", Str)),
            (MieTable::Doc, "Keywords") => Some(("Keywords", StringList)),
            (MieTable::Doc, "ModifyDate") => Some(("ModifyDate", Str)),
            (MieTable::Doc, "OriginalDate") => Some(("DateTimeOriginal", Str)),
            (MieTable::Doc, "References") => Some(("References", StringList)),
            (MieTable::Doc, "Software") => Some(("Software", Str)),
            (MieTable::Doc, "Title") => Some(("Title", Str)),
            (MieTable::Doc, "URL") => Some(("URL", Str)),

            (MieTable::Geo, "City") => Some(("City", Str)),
            (MieTable::Geo, "Country") => Some(("Country", Str)),
            (MieTable::Geo, "State") => Some(("State", Str)),

            (MieTable::Image, "ColorSpace") => Some(("ColorSpace", Str)),
            (MieTable::Image, "Components") => Some(("ComponentsConfiguration", Str)),
            (MieTable::Image, "ImageSize") => Some(("ImageSize", Dimensions)),
            (MieTable::Image, "Resolution") => Some(("Resolution", Resolution)),

            (MieTable::Thumbnail, "ImageSize") => Some(("ThumbnailImageSize", Dimensions)),
            (MieTable::Thumbnail, "data") => Some(("ThumbnailImage", Binary)),

            _ => None,
        }
    }
}

/// Walks a standalone MIE file's element tree, recording a tag for every
/// leaf [`MieTable::leaf`] recognises.
///
/// Mirrors [`subfile_identifier`]'s file-level header handling (MIE.pm:1701:
/// `/^~(\x10|\x18)\x04(.)0MIE/s`) and its element loop's group/terminator
/// handling, generalized from "track one path" to "visit every element" --
/// see this module's own doc comment above for the fuller comparison.
fn parse_mie_document(file: &[u8], metadata: &mut MetadataMap) {
    let Some(header) = file.get(0..8) else {
        return;
    };
    if header[0] != b'~' || header[2] != 4 || &header[4..8] != b"0MIE" {
        return;
    }
    let doc_little_endian = match header[1] {
        0x10 => false,
        0x18 => true,
        _ => return,
    };

    let mut pos = 8usize;
    if header[3] > 252 {
        let Some((_, extension_width)) = mie_data_length(file, pos, header[3], doc_little_endian)
        else {
            return;
        };
        pos += extension_width;
    }

    // Stack of (byte order, table, end offset) frames, one per group
    // currently open. An inline group's (`data_len == 0`) real boundary is
    // its own terminator, not a declared length, so it is given `file.len()`
    // here and the terminator branch below pops it instead.
    let mut stack: Vec<(bool, MieTable, usize)> =
        vec![(doc_little_endian, MieTable::Main, file.len())];

    while let Some(&(little_endian, table, end)) = stack.last() {
        if pos >= end {
            stack.pop();
            continue;
        }
        let Some(elem_header) = file.get(pos..pos + 4) else {
            break;
        };
        if elem_header[0] != b'~' {
            break;
        }
        let format = elem_header[1];
        let tag_len = usize::from(elem_header[2]);
        let len_code = elem_header[3];

        let tag_start = pos + 4;
        let Some(tag_bytes) = file.get(tag_start..tag_start + tag_len) else {
            break;
        };
        let tag_end = tag_start + tag_len;
        let Some((data_len, extension_width)) =
            mie_data_length(file, tag_end, len_code, little_endian)
        else {
            break;
        };
        let data_start = tag_end + extension_width;
        let Some(data) = file.get(data_start..data_start + data_len) else {
            break;
        };
        pos = data_start + data_len;

        if tag_len == 0 {
            // Group terminator (MIE.pm:1483-1512's `unless ($tagLen) { ...
            // last }`): closes the innermost open frame regardless of the
            // format byte.
            stack.pop();
            continue;
        }

        let Ok(raw_tag) = std::str::from_utf8(tag_bytes) else {
            continue;
        };

        if format & 0xf0 == 0x10 {
            let group_little_endian = format & 0x08 != 0;
            let child = table.child(raw_tag);
            let child_end = if data_len == 0 {
                file.len()
            } else {
                data_start + data_len
            };
            stack.push((group_little_endian, child, child_end));
            continue;
        }

        record_mie_leaf(table, raw_tag, format, data, little_endian, metadata);
    }
}

/// Decodes one leaf element and records it, keyed `"{group1}:{name}"` (or
/// `"{group1}:{name}-{lang}"` for a localized tag).
fn record_mie_leaf(
    table: MieTable,
    raw_tag: &str,
    format: u8,
    data: &[u8],
    little_endian: bool,
    metadata: &mut MetadataMap,
) {
    let Some(group1) = table.group1() else {
        return;
    };
    // ProcessMIEGroup strips a trailing `(units)` from the raw tag bytes
    // before table lookup (MIE.pm:1495: `$units = $1 if $tag =~
    // s/\((.*)\)$//;`), then a trailing `-xx_YY` locale code
    // (MIE.pm:1521-1524).
    let (tag_no_units, units) = split_units(raw_tag);
    let (base_tag, lang) = split_lang(tag_no_units);

    let Some((name, kind)) = table.leaf(base_tag) else {
        return;
    };

    let value = match kind {
        MieValue::Str => decode_string(data, format, little_endian).map(TagValue::new_string),
        // A `List => 1` tag's items become a `TagValue::Array`, matching
        // `iptc_parser.rs`'s `collapse_iptc_entries` convention -- except
        // ExifTool itself prints a single-item list as a bare scalar
        // (`References` in `t/images/MIE.mie`), so a one-element list stays
        // a plain string here too rather than a one-element array.
        MieValue::StringList => decode_string_list(data, format, little_endian).map(|mut items| {
            if items.len() == 1 {
                TagValue::new_string(items.remove(0))
            } else {
                TagValue::Array(items.into_iter().map(TagValue::new_string).collect())
            }
        }),
        // Stored as a string, not `TagValue::Integer`: `exiftool_compat`'s
        // formatter applies `PrintConv` purely by bare tag name, with no
        // group awareness, and `Contrast`/`Saturation`/`Sharpness` collide
        // with EXIF's own same-named 0/1/2 enums (Exif.pm) even though
        // MIE.pm declares no `PrintConv` for any of them (`Contrast =>
        // { Writable => 'int8s' }`, `MPC.pm`/`MIE.pm`). A string sidesteps
        // every `value.as_integer()`-gated rule in that dispatch and always
        // renders identically to the bare integer for the fields that don't
        // collide (`ColorTemperature`, `ISO`).
        MieValue::Int => {
            decode_int(data, format, little_endian).map(|v| TagValue::new_string(v.to_string()))
        }
        MieValue::Rational => decode_rational(data, format, little_endian)
            .map(|(n, d)| TagValue::new_string(format_decimal(n, d))),
        MieValue::ExposureTime => decode_rational(data, format, little_endian)
            .map(|(n, d)| TagValue::new_string(format_exposure_time(n, d))),
        MieValue::Dimensions => {
            decode_dimensions(data, format, little_endian).map(TagValue::new_string)
        }
        MieValue::Resolution => decode_resolution(data, format, little_endian, units),
        MieValue::Binary => Some(TagValue::Binary(data.to_vec())),
    };

    let Some(value) = value else {
        return;
    };

    let key = match lang {
        Some(lang) => format!("{group1}:{name}-{lang}"),
        None => format!("{group1}:{name}"),
    };
    metadata.insert(key, value);
}

/// Strips a trailing `(units)` suffix from a raw on-disk MIE tag name.
fn split_units(tag: &str) -> (&str, Option<&str>) {
    if tag.ends_with(')')
        && let Some(open) = tag.rfind('(')
    {
        return (&tag[..open], Some(&tag[open + 1..tag.len() - 1]));
    }
    (tag, None)
}

/// Strips a trailing `-xx_YY` locale suffix (MIE.pm:1521:
/// `/^(\w+)-([a-z]{2}_[A-Z]{2})$/`) from a raw on-disk MIE tag name.
fn split_lang(tag: &str) -> (&str, Option<&str>) {
    if let Some(dash) = tag.rfind('-') {
        let (base, rest) = (&tag[..dash], &tag[dash + 1..]);
        let bytes = rest.as_bytes();
        let is_lang_code = bytes.len() == 5
            && bytes[0].is_ascii_lowercase()
            && bytes[1].is_ascii_lowercase()
            && bytes[2] == b'_'
            && bytes[3].is_ascii_uppercase()
            && bytes[4].is_ascii_uppercase()
            && !base.is_empty();
        if is_lang_code {
            return (base, Some(rest));
        }
    }
    (tag, None)
}

/// Decodes `data` as text per MIE's format byte: `0x20` Latin-1 (ISO
/// 8859-1, a direct byte-to-codepoint mapping), `0x28` UTF-8, `0x29`
/// UTF-16, `0x2a` UTF-32 (`%mieFormat`, MIE.pm:30-63).
fn decode_string(data: &[u8], format: u8, little_endian: bool) -> Option<String> {
    match format {
        0x20 => Some(data.iter().map(|&b| b as char).collect()),
        0x28 => std::str::from_utf8(data).ok().map(str::to_string),
        0x29 => decode_utf16(data, little_endian),
        0x2a => decode_utf32(data, little_endian),
        _ => None,
    }
}

fn decode_utf16(data: &[u8], little_endian: bool) -> Option<String> {
    if data.is_empty() || data.len() % 2 != 0 {
        return None;
    }
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| {
            if little_endian {
                u16::from_le_bytes([c[0], c[1]])
            } else {
                u16::from_be_bytes([c[0], c[1]])
            }
        })
        .collect();
    String::from_utf16(&units).ok()
}

fn decode_utf32(data: &[u8], little_endian: bool) -> Option<String> {
    if data.is_empty() || data.len() % 4 != 0 {
        return None;
    }
    let mut out = String::new();
    for c in data.chunks_exact(4) {
        let word = [c[0], c[1], c[2], c[3]];
        let code_point = if little_endian {
            u32::from_le_bytes(word)
        } else {
            u32::from_be_bytes(word)
        };
        out.push(char::from_u32(code_point)?);
    }
    Some(out)
}

/// Decodes a `List => 1` tag's items: `_list`-format bytes split on NUL
/// (`ProcessMIEGroup`, MIE.pm:1652-1654), or -- since the file may still
/// encode a single-item list as a plain non-list value, as `References`
/// does in `t/images/MIE.mie` -- one item for any other text format.
fn decode_string_list(data: &[u8], format: u8, little_endian: bool) -> Option<Vec<String>> {
    match format {
        0x30 => Some(
            data.split(|&b| b == 0)
                .map(|chunk| chunk.iter().map(|&b| b as char).collect())
                .collect(),
        ),
        0x38 => Some(
            data.split(|&b| b == 0)
                .filter_map(|chunk| std::str::from_utf8(chunk).ok().map(str::to_string))
                .collect(),
        ),
        _ => decode_string(data, format, little_endian).map(|s| vec![s]),
    }
}

/// Decodes a signed or unsigned integer of `bytes.len()` width (1-8 bytes)
/// in the given byte order.
fn decode_int_signed(bytes: &[u8], little_endian: bool, signed: bool) -> Option<i64> {
    if bytes.is_empty() || bytes.len() > 8 {
        return None;
    }
    let mut value: u64 = 0;
    if little_endian {
        for (i, &b) in bytes.iter().enumerate() {
            value |= u64::from(b) << (8 * i);
        }
    } else {
        for &b in bytes {
            value = (value << 8) | u64::from(b);
        }
    }
    if signed {
        let bits = bytes.len() * 8;
        if bits < 64 && value & (1u64 << (bits - 1)) != 0 {
            return Some(value as i64 - (1i64 << bits));
        }
    }
    Some(value as i64)
}

/// `int8u`/`int16u`/.../`int8s`/... (`0x40-0x4b`, MIE.pm:41-48).
fn decode_int(data: &[u8], format: u8, little_endian: bool) -> Option<i64> {
    let signed = matches!(format, 0x48..=0x4b);
    decode_int_signed(data, little_endian, signed)
}

/// A single rational: the data is evenly split in half, numerator first
/// (`rational32u` `0x52`, `rational64u` `0x53`, `rational32s` `0x5a`,
/// `rational64s` `0x5b`, MIE.pm:49-52).
fn decode_rational(data: &[u8], format: u8, little_endian: bool) -> Option<(i64, i64)> {
    if data.is_empty() || data.len() % 2 != 0 {
        return None;
    }
    let half = data.len() / 2;
    let (num, den) = data.split_at(half);
    let signed = matches!(format, 0x5a | 0x5b);
    Some((
        decode_int_signed(num, little_endian, signed)?,
        decode_int_signed(den, little_endian, signed)?,
    ))
}

/// ExifTool's default numeric rendering for a rational with no `PrintConv`:
/// the reduced decimal, trailing `.0` implicitly absent since Rust's `f64`
/// `Display` already omits it (`4.0_f64` prints `"4"`).
fn format_decimal(numerator: i64, denominator: i64) -> String {
    if denominator == 0 {
        return numerator.to_string();
    }
    format!("{}", numerator as f64 / denominator as f64)
}

/// `Exif.pm:5701-5711`'s `PrintExposureTime`.
fn format_exposure_time(numerator: i64, denominator: i64) -> String {
    if denominator == 0 {
        return numerator.to_string();
    }
    let secs = numerator as f64 / denominator as f64;
    if secs > 0.0 && secs < 0.25001 {
        return format!("1/{}", (0.5 + 1.0 / secs) as i64);
    }
    let formatted = format!("{secs:.1}");
    formatted
        .strip_suffix(".0")
        .map(str::to_string)
        .unwrap_or(formatted)
}

/// The per-component byte width of a plain (non-rational) integer format.
fn int_width(format: u8) -> Option<usize> {
    match format {
        0x40 | 0x48 => Some(1),
        0x41 | 0x49 => Some(2),
        0x42 | 0x4a => Some(4),
        0x43 | 0x4b => Some(8),
        _ => None,
    }
}

/// `ImageSize`/`ThumbnailImageSize`: a sequence of integers joined with `x`
/// (`PrintConv => '$val=~tr/ /x/;$val'`, e.g. `Image.pm:456-464`).
fn decode_dimensions(data: &[u8], format: u8, little_endian: bool) -> Option<String> {
    let width = int_width(format)?;
    if data.is_empty() || data.len() % width != 0 {
        return None;
    }
    let signed = matches!(format, 0x48..=0x4b);
    let parts: Option<Vec<String>> = data
        .chunks_exact(width)
        .map(|chunk| decode_int_signed(chunk, little_endian, signed).map(|v| v.to_string()))
        .collect();
    parts.map(|p| p.join("x"))
}

/// `Resolution`: the same `x`-joined sequence as [`decode_dimensions`], but
/// over rationals, with the on-disk units suffix (already split off by
/// [`split_units`]) appended in parens (MIE.pm:1655-1658).
fn decode_resolution(
    data: &[u8],
    format: u8,
    little_endian: bool,
    units: Option<&str>,
) -> Option<TagValue> {
    let half = match format {
        0x52 | 0x5a => 2,
        0x53 | 0x5b => 4,
        _ => return None,
    };
    let component = half * 2;
    if data.is_empty() || data.len() % component != 0 {
        return None;
    }
    let signed = matches!(format, 0x5a | 0x5b);
    let mut parts = Vec::new();
    for chunk in data.chunks_exact(component) {
        let (num, den) = chunk.split_at(half);
        let num = decode_int_signed(num, little_endian, signed)?;
        let den = decode_int_signed(den, little_endian, signed)?;
        parts.push(format_decimal(num, den));
    }
    let mut joined = parts.join("x");
    if let Some(units) = units {
        joined.push('(');
        joined.push_str(units);
        joined.push(')');
    }
    Some(TagValue::new_string(joined))
}

#[cfg(test)]
mod document_mime_tests {
    use super::*;

    /// One MIE element: `~` + FormatCode + TagLength + DataLength + tag + data.
    /// Short form only -- every fixture here fits under 253 bytes of data.
    fn element(format: u8, tag: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = vec![b'~', format, tag.len() as u8, data.len() as u8];
        out.extend_from_slice(tag);
        out.extend_from_slice(data);
        out
    }

    /// The group terminator: a zero-`TagLength` element with no data.
    fn terminator() -> Vec<u8> {
        vec![b'~', 0, 0, 0]
    }

    /// A `.mie` file: the file-level `0MIE` header (big-endian, short-form
    /// length since real files are far under 253 bytes in these tests), then
    /// `elements`, then the top-level terminator.
    fn mie_file(elements: &[Vec<u8>]) -> Vec<u8> {
        let body_len: usize = elements.iter().map(Vec::len).sum::<usize>() + terminator().len();
        let mut out = vec![b'~', 0x10, 4, body_len as u8];
        out.extend_from_slice(b"0MIE");
        for e in elements {
            out.extend_from_slice(e);
        }
        out.extend_from_slice(&terminator());
        out
    }

    #[test]
    fn a_2mime_tag_names_the_subfile_directly() {
        // MIE.mie's own shape: 0Type "JPEG" followed by 2MIME "image/jpeg" --
        // the later tag wins per ExifTool's last-one-set rule.
        let file = mie_file(&[
            element(0x20, b"0Type", b"JPEG"),
            element(0x20, b"2MIME", b"image/jpeg"),
        ]);
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-jpeg")
        );
    }

    #[test]
    fn a_bare_0type_resolves_through_the_mime_table() {
        let file = mie_file(&[element(0x20, b"0Type", b"JPEG")]);
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-jpeg")
        );
    }

    #[test]
    fn an_x_subtype_is_not_doubled() {
        // "image/x-mie-x-raw" would be wrong; ExifTool strips the leading
        // "x-" from the subfile's own subtype before splicing it in.
        let file = mie_file(&[element(0x20, b"2MIME", b"image/x-raw")]);
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-raw")
        );
    }

    #[test]
    fn a_self_contained_nested_group_is_skipped_by_its_own_length() {
        // A group element with data_len > 0 is a complete blob; a 0Type
        // inside it must not surface as the outer file's answer.
        let inner = element(0x20, b"0Type", b"PNG");
        let file = mie_file(&[
            element(0x10, b"Thumbnail", &inner),
            element(0x20, b"0Type", b"JPEG"),
        ]);
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-jpeg")
        );
    }

    #[test]
    fn a_streamed_nested_group_is_walked_past_not_into() {
        // MIE.mie's actual shape: `Meta` and `Camera` both declare data_len 0
        // ("read my contents from right here"), nested two deep, each ending
        // at its own terminator. A tag inside either must not be captured,
        // and the walk must still reach the real answer afterward.
        let file = mie_file(&[
            element(0x20, b"2MIME", b"image/jpeg"),
            element(0x10, b"Meta", &[]),
            element(0x10, b"Camera", &[]),
            element(0x20, b"0Type", b"NotTheAnswer"),
            terminator(), // closes Camera
            terminator(), // closes Meta
        ]);
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-jpeg")
        );
    }

    #[test]
    fn no_top_level_tag_leaves_the_generic_mime_type_alone() {
        let file = mie_file(&[element(0x20, b"0Vers", b"1.1")]);
        assert_eq!(document_mime_type(&file), None);
    }

    #[test]
    fn little_endian_document_header_is_accepted() {
        let mut file = mie_file(&[element(0x20, b"0Type", b"JPEG")]);
        file[1] = 0x18; // II
        assert_eq!(
            document_mime_type(&file).as_deref(),
            Some("image/x-mie-jpeg")
        );
    }

    #[test]
    fn a_non_mie_file_is_declined() {
        assert_eq!(document_mime_type(b"not a mie file at all"), None);
    }

    #[test]
    fn combine_mime_matches_modifymimetype() {
        assert_eq!(
            combine_mime("application/x-mie", "image/jpeg").as_deref(),
            Some("image/x-mie-jpeg")
        );
        assert_eq!(
            combine_mime("application/x-mie", "image/x-raw").as_deref(),
            Some("image/x-mie-raw")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_pinned_exiftool_jpeg_mie_document_trailer() {
        if !crate::test_support::pinned_corpus_available() {
            return;
        }
        let file = std::fs::read("/tmp/oxidex-exiftool-cache/combined-samples/ExifTool.jpg")
            .expect("pinned ExifTool JPEG fixture should be available");
        let metadata = parse_mie_trailer(&file);

        assert_eq!(metadata.get_string("MIE:TrailerSignature"), Some(""));
        assert_eq!(
            metadata.get_string("MIE:Copyright"),
            Some("© 2006 Phil Harvey")
        );
    }

    fn trailer(length_width: usize, byte_order: u8) -> Vec<u8> {
        let mut file = b"image data".to_vec();
        let group_start = file.len();
        file.extend_from_slice(&[b'~', byte_order, 4, 12]);
        file.extend_from_slice(GROUP_HEADER);
        file.extend_from_slice(b"body");
        file.extend_from_slice(b"~\0\x04\0zmie");
        file.extend_from_slice(&[b'~', 0, 0, if length_width == 4 { 6 } else { 10 }]);
        let length = (file.len() + length_width + 2 - group_start) as u64;
        match (length_width, byte_order) {
            (4, 0x10) => file.extend_from_slice(&(length as u32).to_be_bytes()),
            (4, 0x18) => file.extend_from_slice(&(length as u32).to_le_bytes()),
            (8, 0x10) => file.extend_from_slice(&length.to_be_bytes()),
            (8, 0x18) => file.extend_from_slice(&length.to_le_bytes()),
            _ => unreachable!(),
        }
        file.extend_from_slice(&[byte_order, length_width as u8]);
        file
    }

    #[test]
    fn accepts_both_mie_trailer_length_encodings() {
        for (width, order) in [(4, 0x10), (4, 0x18), (8, 0x10), (8, 0x18)] {
            let metadata = parse_mie_trailer(&trailer(width, order));
            assert_eq!(metadata.get_string("MIE:TrailerSignature"), Some(""));
        }
    }

    #[test]
    fn rejects_a_zmie_marker_without_a_main_group_at_its_declared_start() {
        let mut file = trailer(4, 0x10);
        let group_start = file
            .windows(GROUP_HEADER.len())
            .position(|w| w == GROUP_HEADER)
            .unwrap();
        file[group_start] = b'X';
        assert!(parse_mie_trailer(&file).is_empty());
    }
}
