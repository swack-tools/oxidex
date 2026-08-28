//! JPEG 2000: both the bare codestream (J2C) and the JP2 box container.
//!
//! `Jpeg2000::ProcessJP2` (Jpeg2000.pm:1537-1596) splits on the first twelve
//! bytes. A bare `\xff\x4f\xff\x51\0` codestream is handed to ExifTool's JPEG
//! marker processor with the J2C marker names added (Jpeg2000.pm:1553-1563);
//! anything with a `jP` signature box is walked by `ProcessJpeg2000Box`
//! against `Jpeg2000::Main` (Jpeg2000.pm:127-400).
//!
//! # What comes from the transcription
//!
//! Five of the sub-tables `Main` points at are real `ProcessBinaryData`
//! layouts and are read through [`find_table`] rather than re-derived:
//! `Jpeg2000::FileType`, `::ImageHeader`, `::ColorSpec`, `::CaptureResolution`
//! and `::DisplayResolution`. `find_table` supplies every offset, format and
//! enum `PrintConv` in them.
//!
//! Four conversions the generator refuses to model are hand-implemented below
//! against the cited Perl, each behind a [`RawAccess`] that records the
//! refusal it is reading past:
//!
//! * `MinorVersion`'s `sprintf("%x.%x.%x", unpack("nCC", $val))`
//!   (Jpeg2000.pm:569-573)
//! * `BitsPerComponent`'s sign/width split (Jpeg2000.pm:528-535)
//! * `ColorSpecMethod`'s `RawConv` `DataMember` (Jpeg2000.pm:654-668)
//! * `ColorSpec` index 3, a three-way `Condition` on that `DataMember`
//!   (Jpeg2000.pm:685-735) -- the generator counted it as
//!   `tag_variant_cond_unsupported`, which is why the transcribed
//!   `Jpeg2000::ColorSpec` carries only three fields.
//!
//! `CompatibleBrands` (Jpeg2000.pm:574-580) is not in the transcribed table at
//! all: its `Format` is `undef[$size-8]`, a length that depends on the box
//! rather than on the layout.
//!
//! # Embedded directories
//!
//! A `uuid` box whose first sixteen bytes match one of the EXIF or GeoJP2
//! UUIDs carries a complete TIFF file (Jpeg2000.pm:279-351), which is
//! re-entered through this crate's own TIFF reader rather than a second EXIF
//! implementation. An `xml ` box goes to `XMP::XML` (Jpeg2000.pm:257-272),
//! which is the same schema-less walk
//! [`crate::parsers::xmp::generic_xml`] already implements.
//!
//! # What is deliberately absent
//!
//! 1. **`IsOffset` absolutisation for the embedded TIFF.**
//!    `ProcessJpeg2000Box` gives the sub-directory
//!    `Base => $base + $dataPos + $subdirStart` (Jpeg2000.pm:1244), the
//!    TIFF's absolute file offset, and ExifTool absolutises the directory's
//!    `IsOffset => 1` tags against it: `t/images/Jpeg2000.jp2` stores
//!    `StripOffsets 0` in a TIFF beginning at file offset 101 and ExifTool
//!    prints `101`. This module has no list of which EXIF tags carry that
//!    flag, and rebasing the wrong one emits a confident wrong file offset,
//!    so the three offset tags it does know about are dropped instead of
//!    reported un-rebased.
//! 2. **`ColorSpec`'s `ICC_Profile` branch** (Jpeg2000.pm:685-695), taken when
//!    `ColorSpecMethod` is 2 or 3. It is an `ICC_Profile::Main` sub-directory,
//!    not a scalar; the pinned fixture uses method 1 (`ColorSpace`).
//! 3. **`ColorSpecData`** (Jpeg2000.pm:729-734), the `Binary => 1` fallback
//!    for any other method.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/Jpeg2000.pm`, `ExifTool.pm:8442-8447`

use crate::core::tag_occurrence::Instance;
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::exiftool_tables::{
    Acknowledged, DecodedValue, PerlCitation, RawAccess, decode_binary_table, find_table,
};
use crate::io::ByteOrder;
use crate::parsers::xmp::generic_xml::{XmlWalkOptions, extract_xml_properties_with};

/// Jpeg2000.pm:1547-1548, the two `jP` signature boxes `ProcessJP2` accepts.
const JP2_SIGNATURES: [&[u8]; 2] = [
    b"\x00\x00\x00\x0cjP  \x0d\x0a\x87\x0a",
    b"\x00\x00\x00\x0cjP\x1a\x1a\x0d\x0a\x87\x0a",
];
/// Jpeg2000.pm:1552, `$hdr =~ /^\xff\x4f\xff\x51\0/`.
const J2C_SIGNATURE: &[u8] = b"\xff\x4f\xff\x51\x00";

/// `%uuid`'s EXIF prefix (Jpeg2000.pm:80) -- `UUID-EXIF`'s `Condition`
/// (Jpeg2000.pm:283) matches this and takes the TIFF from `$valuePtr + 16`.
const UUID_EXIF: &[u8] = b"JpgTiffExif->JP2";
/// `UUID-EXIF2`, Photoshop's (Jpeg2000.pm:294-303).
const UUID_EXIF2: &[u8; 16] = b"\x05\x37\xcd\xab\x9d\x0c\x44\x31\xa7\x2a\xfa\x56\x1f\x2a\x11\x3e";
/// `UUID-GeoJP2` (Jpeg2000.pm:344-351).
const UUID_GEOJP2: &[u8; 16] = b"\xb1\x4b\xf8\xbd\x08\x3d\x4b\x43\xa5\xae\x8c\xd7\xd5\xa6\xce\x03";

/// EXIF tags this module would have to rebase against the box's `Base` and
/// cannot -- see the module header's omission #1. All three are
/// `IsOffset => 1` in `Exif::Main`.
const UNREBASABLE_OFFSET_TAGS: &[&str] = &["StripOffsets", "TileOffsets", "FreeOffsets"];

/// The family-1 group `Jpeg2000::Main`'s sub-tables report under. Each
/// transcribed table carries `group1: "Jpeg2000"` already; this is the same
/// string, spelled once for the hand-implemented tags.
const GROUP: &str = "Jpeg2000";

/// Priority for the embedded TIFF's tags.
///
/// `ProcessTIFF` mints a non-priority directory's tags at priority 0
/// (ExifTool.pm's `PRIORITY_DIR`): in a JP2 the priority directory is the JP2
/// box tree, not the TIFF inside a `uuid` box. That is what decides
/// `Composite:ImageSize` here -- `t/images/Jpeg2000.jp2` carries a 16x16
/// `ihdr` and a 1x1 GeoTIFF stub, and the oracle reports `16x16`.
const EMBEDDED_TIFF_PRIORITY: u8 = 0;

/// Priority for `File:ExifByteOrder` from an embedded TIFF.
///
/// Unlike the directory tags above, ExifByteOrder is not minted from a
/// directory at all: `ProcessTIFF` itself calls
/// `FoundTag('ExifByteOrder', $byteOrder)` (ExifTool.pm:8702), and on a
/// duplicate the tag gets the normal default priority 1 (ExifTool.pm:9562-
/// 9563), so `$priority >= $oldPriority` (:9564) lets each later TIFF-bearing
/// `uuid` box replace the value. `t/images/Jpeg2000.jp2` pins it: the
/// little-endian GeoJP2 stub (offset 77) precedes the big-endian UUID-EXIF
/// box (offset 1914), and the oracle prints `Big-endian (Motorola, MM)`.
const EXIF_BYTE_ORDER_PRIORITY: u8 = 1;

const MINOR_VERSION: PerlCitation = PerlCitation {
    module: "Jpeg2000",
    table: "FileType",
    tag: "MinorVersion",
    lines: "Jpeg2000.pm:569-573",
};
const BITS_PER_COMPONENT: PerlCitation = PerlCitation {
    module: "Jpeg2000",
    table: "ImageHeader",
    tag: "BitsPerComponent",
    lines: "Jpeg2000.pm:528-535",
};
const COLOR_SPEC_METHOD: PerlCitation = PerlCitation {
    module: "Jpeg2000",
    table: "ColorSpec",
    tag: "ColorSpecMethod",
    lines: "Jpeg2000.pm:654-668",
};

/// Jpeg2000.pm:700-727, `ColorSpec` index 3's `ColorSpace` `PrintConv`.
#[rustfmt::skip]
const COLOR_SPACE: &[(u32, &str)] = &[
    (0, "Bi-level"), (1, "YCbCr(1)"), (3, "YCbCr(2)"), (4, "YCbCr(3)"),
    (9, "PhotoYCC"), (11, "CMY"), (12, "CMYK"), (13, "YCCK"), (14, "CIELab"),
    (15, "Bi-level(2)"), (16, "sRGB"), (17, "Grayscale"), (18, "sYCC"),
    (19, "CIEJab"), (20, "e-sRGB"), (21, "ROMM-RGB"),
    (22, "YPbPr(1125/60)"), (23, "YPbPr(1250/50)"), (24, "e-sYCC"),
];

fn be_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn be_u64(data: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(
        data.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// An in-memory reader over a slice, for re-entering this crate's TIFF reader
/// on an embedded directory.
struct SliceReader {
    data: Vec<u8>,
}

impl FileReader for SliceReader {
    fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
        let start = offset as usize;
        let end = start.checked_add(length).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "offset overflow")
        })?;
        self.data.get(start..end).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "read past end of slice")
        })
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

/// Extract JPEG 2000 metadata (`Image::ExifTool::Jpeg2000::ProcessJP2`).
pub fn parse_jpeg2000_metadata(
    reader: &dyn FileReader,
) -> std::result::Result<MetadataMap, String> {
    let data = reader
        .read(0, reader.size() as usize)
        .map_err(|err| err.to_string())?;
    let mut metadata = MetadataMap::new();

    if data.starts_with(J2C_SIGNATURE) {
        parse_codestream(data, &mut metadata);
        return Ok(metadata);
    }
    if JP2_SIGNATURES
        .iter()
        .any(|signature| data.starts_with(signature))
    {
        walk_boxes(data, &mut metadata);
    }
    Ok(metadata)
}

// ---------------------------------------------------------------------------
// J2C codestream (ExifTool's JPEG marker processor with %j2cMarker names)
// ---------------------------------------------------------------------------

/// The J2C markers with no length word: SOC, SOD, SOP, EPH and the tile-part
/// delimiters (Jpeg2000.pm:90-123).
const J2C_STANDALONE_MARKERS: [u8; 5] = [0x4f, 0x90, 0x91, 0x92, 0x93];

fn parse_codestream(data: &[u8], metadata: &mut MetadataMap) {
    let mut pos = 2usize;
    let mut got_size = false;
    while pos + 1 < data.len() {
        if data[pos] != 0xff {
            pos += 1;
            continue;
        }
        let marker = data[pos + 1];
        if marker == 0xd9 {
            break;
        }
        if J2C_STANDALONE_MARKERS.contains(&marker) {
            pos += 2;
            continue;
        }
        let Some(length_bytes) = data.get(pos + 2..pos + 4) else {
            break;
        };
        let length = u16::from_be_bytes([length_bytes[0], length_bytes[1]]) as usize;
        if length < 2 {
            break;
        }
        let Some(end) = pos.checked_add(2 + length) else {
            break;
        };
        let Some(payload) = data.get(pos + 4..end) else {
            break;
        };
        match marker {
            // SIZ (ExifTool.pm:8442-8447): `unpack('x2N2')` past the two-byte
            // Rsiz capability field gives Xsiz and Ysiz. Only the first SIZ
            // sets the size (`unless ($gotSize)`).
            0x51 if !got_size => {
                if let (Some(width), Some(height)) = (be_u32(payload, 2), be_u32(payload, 6)) {
                    got_size = true;
                    metadata.insert("File:ImageWidth", TagValue::new_integer(i64::from(width)));
                    metadata.insert("File:ImageHeight", TagValue::new_integer(i64::from(height)));
                }
            }
            // COM starts with the two-byte registration value (`Rcom`); only
            // the following bytes are the ExifTool `File:Comment` string.
            // ExifTool.pm:8432-8440 keeps a `Rcom` of 0 or 65535 as binary and
            // Latin-decodes `Rcom == 1`; both real comments in Jpeg2000.j2c
            // use Rcom=1 (ISO/IEC 15444-1 SS A.9).
            0x64 => {
                if let Some(comment) = payload.get(2..)
                    && let Ok(comment) = std::str::from_utf8(comment)
                {
                    metadata.insert("File:Comment", TagValue::new_string(comment));
                }
            }
            _ => {}
        }
        pos = end;
    }
}

// ---------------------------------------------------------------------------
// JP2 box tree (ProcessJpeg2000Box)
// ---------------------------------------------------------------------------

/// Box ids whose payload is itself a run of boxes (`SubDirectory => { }` with
/// no `TagTable`, Jpeg2000.pm:163-233).
const CONTAINER_BOXES: [&[u8; 4]; 5] = [b"jp2h", b"res ", b"jpch", b"jplh", b"asoc"];

/// `ProcessJpeg2000Box` (Jpeg2000.pm:1100-1340).
fn walk_boxes(buffer: &[u8], metadata: &mut MetadataMap) {
    let mut pos = 0usize;
    while pos + 8 <= buffer.len() {
        let Some(size32) = be_u32(buffer, pos) else {
            return;
        };
        let mut id = [0u8; 4];
        id.copy_from_slice(&buffer[pos + 4..pos + 8]);
        let (header_len, box_len) = match size32 {
            // "box size of 1 indicates an 8-byte length follows"
            1 => match be_u64(buffer, pos + 8) {
                Some(long) if long >= 16 => (16usize, long as usize),
                _ => return,
            },
            // "box extends to end of file"
            0 => (8usize, buffer.len() - pos),
            other if (other as usize) >= 8 => (8usize, other as usize),
            _ => return,
        };
        let end = pos + box_len;
        if end > buffer.len() {
            return;
        }
        let payload = &buffer[pos + header_len..end];

        if CONTAINER_BOXES.contains(&&id) {
            walk_boxes(payload, metadata);
        } else {
            handle_box(&id, payload, metadata);
        }
        pos = end;
    }
}

fn handle_box(id: &[u8; 4], payload: &[u8], metadata: &mut MetadataMap) {
    match id {
        b"ftyp" => file_type_box(payload, metadata),
        b"ihdr" => image_header_box(payload, metadata),
        b"colr" => color_spec_box(payload, metadata),
        b"resc" => resolution_box("CaptureResolution", payload, metadata),
        b"resd" => resolution_box("DisplayResolution", payload, metadata),
        b"xml " => xml_box(payload, metadata),
        b"uuid" => uuid_box(payload, metadata),
        _ => {}
    }
}

/// `Jpeg2000::FileType` (Jpeg2000.pm:554-581).
fn file_type_box(payload: &[u8], metadata: &mut MetadataMap) {
    let Some(table) = find_table("Jpeg2000", "FileType") else {
        return;
    };
    // Jpeg2000.pm:1584, `SetByteOrder('MM')`.
    let decode = decode_binary_table(table, payload, ByteOrder::Big);
    for decoded in decode.fields() {
        match decoded.field.name {
            // `ValueConv => 'sprintf("%x.%x.%x", unpack("nCC", $val))'`.
            "MinorVersion" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::VALUE_CONV, &MINOR_VERSION)
                    && let DecodedValue::Undefined(bytes) = access.raw()
                    && bytes.len() >= 4
                {
                    let high = u16::from_be_bytes([bytes[0], bytes[1]]);
                    metadata.insert(
                        format!("{GROUP}:MinorVersion"),
                        TagValue::new_string(format!("{high:x}.{:x}.{:x}", bytes[2], bytes[3])),
                    );
                }
            }
            name => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(format!("{GROUP}:{name}"), value);
                }
            }
        }
    }
    // `CompatibleBrands` (Jpeg2000.pm:574-580) is `Format => 'undef[$size-8]'`
    // -- a length taken from the box, not from the layout, which is why the
    // transcribed table stops at MinorVersion. Its ValueConv is
    // `my @a=($val=~/.{4}/sg); @a=grep(!/\0/,@a); \@a`.
    if payload.len() > 8 {
        let brands: Vec<String> = payload[8..]
            .chunks_exact(4)
            .filter(|chunk| !chunk.contains(&0))
            .filter_map(|chunk| std::str::from_utf8(chunk).ok().map(ToString::to_string))
            .collect();
        if !brands.is_empty() {
            metadata.insert(
                format!("{GROUP}:CompatibleBrands"),
                TagValue::new_string(brands.join(", ")),
            );
        }
    }
}

/// `Jpeg2000::ImageHeader` (Jpeg2000.pm:513-550).
fn image_header_box(payload: &[u8], metadata: &mut MetadataMap) {
    let Some(table) = find_table("Jpeg2000", "ImageHeader") else {
        return;
    };
    let decode = decode_binary_table(table, payload, ByteOrder::Big);
    for decoded in decode.fields() {
        match decoded.field.name {
            // Jpeg2000.pm:530-534:
            //   $val == 0xff and return 'Variable';
            //   my $sign = ($val & 0x80) ? 'Signed' : 'Unsigned';
            //   return (($val & 0x7f) + 1) . " Bits, $sign";
            "BitsPerComponent" => {
                if let Some(access) =
                    RawAccess::new(decoded, Acknowledged::NONE, &BITS_PER_COMPONENT)
                    && let Some(raw) = access.raw().as_integer()
                {
                    metadata.insert(
                        format!("{GROUP}:BitsPerComponent"),
                        TagValue::new_string(render_bits_per_component(raw)),
                    );
                }
            }
            name => {
                if let Some(value) = decoded.emit() {
                    metadata.insert(format!("{GROUP}:{name}"), value);
                }
            }
        }
    }
}

/// Jpeg2000.pm:530-534.
fn render_bits_per_component(raw: i64) -> String {
    if raw == 0xff {
        return "Variable".to_string();
    }
    let sign = if raw & 0x80 != 0 {
        "Signed"
    } else {
        "Unsigned"
    };
    format!("{} Bits, {sign}", (raw & 0x7f) + 1)
}

/// `Jpeg2000::ColorSpec` (Jpeg2000.pm:630-735).
fn color_spec_box(payload: &[u8], metadata: &mut MetadataMap) {
    let Some(table) = find_table("Jpeg2000", "ColorSpec") else {
        return;
    };
    let decode = decode_binary_table(table, payload, ByteOrder::Big);
    let mut method: Option<i64> = None;
    for decoded in decode.fields() {
        // `RawConv => '$$self{ColorSpecMethod} = $val'` (Jpeg2000.pm:656) is a
        // pure DataMember side effect returning the value unchanged; the
        // generator flags it as an unmodelled RawConv, so the enum PrintConv
        // it also carries is reached through `emit_raw`.
        if decoded.field.name == "ColorSpecMethod" {
            if let Some(access) =
                RawAccess::new(decoded, Acknowledged::RAW_CONV, &COLOR_SPEC_METHOD)
            {
                method = access.raw().as_integer();
                metadata.insert(format!("{GROUP}:ColorSpecMethod"), access.emit_raw());
            }
            continue;
        }
        if let Some(value) = decoded.emit() {
            metadata.insert(format!("{GROUP}:{}", decoded.field.name), value);
        }
    }
    // Index 3 is a three-way Condition on that DataMember (Jpeg2000.pm:685-735).
    // Only the `ColorSpecMethod == 1` branch is implemented; see the module
    // header for the two that are not.
    if method == Some(1)
        && let Some(raw) = be_u32(payload, 3)
    {
        let rendered = COLOR_SPACE
            .iter()
            .find(|(key, _)| *key == raw)
            .map_or_else(|| raw.to_string(), |(_, label)| (*label).to_string());
        metadata.insert(
            format!("{GROUP}:ColorSpace"),
            TagValue::new_string(rendered),
        );
    }
}

/// `Jpeg2000::CaptureResolution` / `::DisplayResolution` (Jpeg2000.pm:583-637)
/// -- both fully transcribed, `PrintConv` included.
fn resolution_box(table_name: &str, payload: &[u8], metadata: &mut MetadataMap) {
    let Some(table) = find_table("Jpeg2000", table_name) else {
        return;
    };
    let decode = decode_binary_table(table, payload, ByteOrder::Big);
    for decoded in decode.fields() {
        if let Some(value) = decoded.emit() {
            metadata.insert(format!("{GROUP}:{}", decoded.field.name), value);
        }
    }
}

/// An `xml ` box (Jpeg2000.pm:257-272): `SubDirectory => { TagTable =>
/// 'Image::ExifTool::XMP::XML' }`, which `ProcessXMP` walks as schema-less
/// XMP -- the same walk plain `.xml` files get.
fn xml_box(payload: &[u8], metadata: &mut MetadataMap) {
    let options = XmlWalkOptions {
        // `%Image::ExifTool::XMP::XML`'s `GROUPS => { 0 => 'XML', 1 => 'XML' }`.
        group0: "XML",
        ..XmlWalkOptions::default()
    };
    let Ok(properties) = extract_xml_properties_with(payload, &options) else {
        return;
    };
    for property in properties {
        metadata.insert_occurrence(
            format!("{}:{}", property.group1, property.name),
            TagValue::new_string(property.value),
            // `FoundXMP` mints an unknown tag at priority 0 (XMP.pm:3595) --
            // the same reasoning `generic_xml::parse_xml_file` records.
            0,
            &property.group1,
            Instance::default(),
        );
    }
}

/// A `uuid` box (Jpeg2000.pm:279-380). Only the three whose `SubDirectory`
/// is `Exif::Main` under `ProcessTIFF` are routed here.
fn uuid_box(payload: &[u8], metadata: &mut MetadataMap) {
    if payload.len() < 16 {
        return;
    }
    let is_tiff_uuid = payload.starts_with(UUID_EXIF)
        || payload.starts_with(UUID_EXIF2)
        || payload.starts_with(UUID_GEOJP2);
    if !is_tiff_uuid {
        return;
    }
    // All three use `Start => '$valuePtr + 16'`.
    let reader = SliceReader {
        data: payload[16..].to_vec(),
    };
    let Ok(embedded) = crate::core::operations::parse_tiff_metadata(&reader) else {
        return;
    };
    for (key, value) in embedded.iter() {
        let bare = key.rsplit(':').next().unwrap_or(key);
        // See the module header's omission #1: without the box's `Base` these
        // would be file offsets short by exactly that base.
        if UNREBASABLE_OFFSET_TAGS.contains(&bare) {
            continue;
        }
        // `parse_tiff_metadata` also mints the identity tags a standalone
        // TIFF file would carry. This is a directory inside a JP2, and
        // ExifTool names the container, not the directory.
        if matches!(bare, "FileType" | "FileTypeExtension" | "MIMEType") {
            continue;
        }
        let group1 = key
            .rsplit_once(':')
            .map_or(String::new(), |(group, _)| group.to_string());
        // ExifByteOrder is FoundTag'd by ProcessTIFF itself, not from a
        // directory, so it carries the normal priority and the last
        // TIFF-bearing uuid box wins -- see EXIF_BYTE_ORDER_PRIORITY.
        let priority = if bare == "ExifByteOrder" {
            EXIF_BYTE_ORDER_PRIORITY
        } else {
            EMBEDDED_TIFF_PRIORITY
        };
        metadata.insert_occurrence(
            key.clone(),
            value.clone(),
            priority,
            &group1,
            Instance::default(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal one-entry TIFF (ImageWidth = 1) behind `prefix`, as the
    /// payload of a TIFF-bearing `uuid` box.
    fn uuid_tiff_payload(prefix: &[u8], big_endian: bool) -> Vec<u8> {
        let mut p = prefix.to_vec();
        if big_endian {
            p.extend_from_slice(b"MM\x00\x2a\x00\x00\x00\x08"); // header, IFD at 8
            p.extend_from_slice(b"\x00\x01"); // 1 entry
            p.extend_from_slice(b"\x01\x00\x00\x03\x00\x00\x00\x01\x00\x01\x00\x00");
            p.extend_from_slice(b"\x00\x00\x00\x00"); // no next IFD
        } else {
            p.extend_from_slice(b"II\x2a\x00\x08\x00\x00\x00");
            p.extend_from_slice(b"\x01\x00");
            p.extend_from_slice(b"\x00\x01\x03\x00\x01\x00\x00\x00\x01\x00\x00\x00");
            p.extend_from_slice(b"\x00\x00\x00\x00");
        }
        p
    }

    /// ExifByteOrder comes from `ProcessTIFF` itself, at normal priority
    /// (ExifTool.pm:8702, :9562-9564), so with two TIFF-bearing uuid boxes
    /// the LAST one's byte order is displayed. `t/images/Jpeg2000.jp2` is the
    /// real-file pin (GeoJP2 `II` at offset 77, UUID-EXIF `MM` at 1914,
    /// oracle prints `Big-endian (Motorola, MM)`); this exercises both
    /// orders. Directory tags keep priority 0, so the first box still wins
    /// those (the 1x1 GeoTIFF stub must not beat `ihdr` -- see
    /// EMBEDDED_TIFF_PRIORITY).
    #[test]
    fn last_uuid_box_wins_exif_byte_order() {
        let geojp2 = uuid_tiff_payload(UUID_GEOJP2, false); // II
        let exif = uuid_tiff_payload(UUID_EXIF, true); // MM

        let mut metadata = MetadataMap::new();
        uuid_box(&geojp2, &mut metadata);
        uuid_box(&exif, &mut metadata);
        assert_eq!(
            metadata.get_string("File:ExifByteOrder"),
            Some("Big-endian (Motorola, MM)"),
            "file order GeoJP2(II) then EXIF(MM): the later box must win"
        );

        let mut metadata = MetadataMap::new();
        uuid_box(&exif, &mut metadata);
        uuid_box(&geojp2, &mut metadata);
        assert_eq!(
            metadata.get_string("File:ExifByteOrder"),
            Some("Little-endian (Intel, II)"),
            "file order EXIF(MM) then GeoJP2(II): the later box must win"
        );
    }

    #[test]
    fn bits_per_component_splits_sign_from_width() {
        // `t/images/Jpeg2000.jp2` stores 0x07 and ExifTool reports
        // "8 Bits, Unsigned".
        assert_eq!(render_bits_per_component(0x07), "8 Bits, Unsigned");
        assert_eq!(render_bits_per_component(0x87), "8 Bits, Signed");
        assert_eq!(render_bits_per_component(0xff), "Variable");
    }

    #[test]
    fn the_five_jpeg2000_tables_are_transcribed() {
        // A `None` here would mean this parser silently stopped reading a box
        // it used to read -- exactly the ambiguity `find_table`'s own docs
        // warn about.
        for table in [
            "FileType",
            "ImageHeader",
            "ColorSpec",
            "CaptureResolution",
            "DisplayResolution",
        ] {
            assert!(
                find_table("Jpeg2000", table).is_some(),
                "Jpeg2000::{table} is no longer transcribed"
            );
        }
    }
}
