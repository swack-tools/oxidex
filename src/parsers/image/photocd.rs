//! Kodak Photo CD (PCD) Image Pac metadata parser.
//!
//! ExifTool 13.59's `PhotoCD::ProcessPCD` (PhotoCD.pm:449-465) seeks to byte
//! 2048, reads exactly 2048 bytes, requires those to begin `PCD_IPI`, sets
//! big-endian order and runs `ProcessBinaryData` over the block with
//! `PhotoCD::Main`. This parser does the same, and reads the layout out of the
//! generated table rather than restating offsets or enum maps here.
//!
//! # Why the table is not enough on its own
//!
//! `PhotoCD::Main` leans hard on Perl the transcription deliberately does not
//! run: nine fields carry a `ValueConv`, three a `RawConv`, and five a
//! `Condition`. Every one of those is spelled out below against the Perl it
//! reproduces, because a `ValueConv` skipped rather than implemented turns
//! `ImageWidth` into the 2-bit code `2` under a real ExifTool tag name.
//!
//! Three tags -- `ImageWidth`, `ImageHeight` and `CompressionClass` -- are
//! `1538.1`/`.2`/`.3`, fractional bit-field entries sharing byte 1538 with
//! `Orientation`. `decode_binary_table` refused those outright until masked
//! fractional entries began decoding, so this format could not have been read
//! from the generated table before that.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/PhotoCD.pm`
//! - Format notes: <http://pcdtojpeg.sourceforge.net/>

use crate::core::file_metadata::format_unix_time_local;
use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::exiftool_tables::{
    Acknowledged, DecodedField, DecodedValue, PerlCitation, RawAccess, decode_binary_table,
    find_table,
};
use crate::io::ByteOrder;

/// A [`PerlCitation`] into `PhotoCD::Main`, the only table this parser reads.
const fn citation(tag: &'static str, lines: &'static str) -> PerlCitation {
    PerlCitation {
        module: "PhotoCD",
        table: "Main",
        tag,
        lines,
    }
}

const SPECIFICATION_VERSION: PerlCitation = citation("SpecificationVersion", "PhotoCD.pm:25-30");
const AUTHORING_SOFTWARE_RELEASE: PerlCitation =
    citation("AuthoringSoftwareRelease", "PhotoCD.pm:31-35");
const IMAGE_MAGNIFICATION_DESCRIPTOR: PerlCitation =
    citation("ImageMagnificationDescriptor", "PhotoCD.pm:36-40");
const CREATE_DATE: PerlCitation = citation("CreateDate", "PhotoCD.pm:43-49");
const MODIFY_DATE: PerlCitation = citation("ModifyDate", "PhotoCD.pm:51-57");
const PRODUCT_TYPE: PerlCitation = citation("ProductType", "PhotoCD.pm:73-76");
const SCANNER_VENDOR_ID: PerlCitation = citation("ScannerVendorID", "PhotoCD.pm:78-81");
const SCANNER_PRODUCT_ID: PerlCitation = citation("ScannerProductID", "PhotoCD.pm:83-86");
const SCANNER_FIRMWARE_VERSION: PerlCitation =
    citation("ScannerFirmwareVersion", "PhotoCD.pm:88-91");
const SCANNER_FIRMWARE_DATE: PerlCitation = citation("ScannerFirmwareDate", "PhotoCD.pm:93-96");
const SCANNER_SERIAL_NUMBER: PerlCitation = citation("ScannerSerialNumber", "PhotoCD.pm:98-101");
const SCANNER_PIXEL_SIZE: PerlCitation = citation("ScannerPixelSize", "PhotoCD.pm:103-106");
const IMAGE_WORKSTATION_MAKE: PerlCitation = citation("ImageWorkstationMake", "PhotoCD.pm:109-111");
const PHOTO_FINISHER_NAME: PerlCitation = citation("PhotoFinisherName", "PhotoCD.pm:131-134");
const SCENE_BALANCE_ALGORITHM_REVISION: PerlCitation =
    citation("SceneBalanceAlgorithmRevision", "PhotoCD.pm:142-146");
const SCENE_BALANCE_ALGORITHM_COMMAND: PerlCitation =
    citation("SceneBalanceAlgorithmCommand", "PhotoCD.pm:148-157");
const SCENE_BALANCE_ALGORITHM_FILM_ID: PerlCitation =
    citation("SceneBalanceAlgorithmFilmID", "PhotoCD.pm:158-236");
const COPYRIGHT_STATUS: PerlCitation = citation("CopyrightStatus", "PhotoCD.pm:384-391");
const COPYRIGHT_FILE_NAME: PerlCitation = citation("CopyrightFileName", "PhotoCD.pm:393-397");
const ORIENTATION: PerlCitation = citation("Orientation", "PhotoCD.pm:399-408");
const IMAGE_WIDTH: PerlCitation = citation("ImageWidth", "PhotoCD.pm:409-415");
const IMAGE_HEIGHT: PerlCitation = citation("ImageHeight", "PhotoCD.pm:416-420");

/// File offset of the Image Pac info block (PhotoCD.pm:454, `Seek(2048, 0)`).
const IPI_OFFSET: u64 = 2048;

/// Length ExifTool requires the block to have; a short read aborts the parse
/// there (`Read($buff, 2048) == 2048`), so the file must be at least 4096
/// bytes for any PhotoCD tag to exist at all.
const IPI_LEN: usize = 2048;

/// PhotoCD.pm:456, `$buff =~ /^PCD_IPI/`.
const IPI_SIGNATURE: &[u8] = b"PCD_IPI";

/// Offset of the three bytes whose value `"SBA"` gates the scene-balance
/// group (PhotoCD.pm:225, the `HasSBA` tag). The tag is `Hidden` and its
/// `RawConv` returns `undef`, so it exists only to set that flag.
const HAS_SBA_RANGE: std::ops::Range<usize> = 225..228;

/// `CopyrightStatus` value that unlocks `CopyrightFileName` (PhotoCD.pm:332,
/// `Condition => '$$self{CopyrightStatus} and $$self{CopyrightStatus} == 1'`).
const COPYRIGHT_RESTRICTED: i64 = 1;

/// Parser for Kodak Photo CD Image Pac files.
pub struct PhotoCDParser;

/// The `DataMember`s later tags read, gathered in one pass.
///
/// ExifTool accumulates these in `$$self{...}` as `ProcessBinaryData` walks the
/// table in index order, so a tag's condition can depend on an earlier tag's
/// raw value. Collecting them up front is the same thing without the ordering
/// coupling: all three sources sit at lower offsets than every tag that uses
/// them.
struct DataMembers {
    /// PhotoCD.pm:225 -- `$val eq "SBA" and $$self{HasSBA} = 1`.
    has_sba: bool,
    /// PhotoCD.pm:1538 -- `$$self{Orient} = $val`, the masked 2-bit code.
    orient: i64,
    /// PhotoCD.pm:331 -- `$$self{CopyrightStatus} = $val`.
    copyright_status: Option<i64>,
}

/// Renders `int8u[2]` as ExifTool's `$val =~ tr/ /./`.
///
/// ExifTool's `int8u[2]` value is the two numbers joined by a space, so
/// transliterating the space to a dot is a two-element join on `"."` -- not a
/// fixed-point number. `7 61` is release "7.61", not 7.61.
fn dotted_pair(raw: &DecodedValue) -> Option<String> {
    let DecodedValue::Array(values) = raw else {
        return None;
    };
    let [first, second] = values.as_slice() else {
        return None;
    };
    match (first, second) {
        (DecodedValue::Integer(a), DecodedValue::Integer(b)) => Some(format!("{a}.{b}")),
        _ => None,
    }
}

/// `$val eq "255 255" ? "n/a" : $val`, the `RawConv` on the two version pairs
/// (PhotoCD.pm:31, :38). It runs before the `tr/ /./`, and `"n/a"` has no
/// space to transliterate, so it passes through unchanged.
fn dotted_pair_or_na(raw: &DecodedValue) -> Option<String> {
    if let DecodedValue::Array(values) = raw
        && values
            .iter()
            .all(|v| matches!(v, DecodedValue::Integer(255)))
        && values.len() == 2
    {
        return Some("n/a".to_string());
    }
    dotted_pair(raw)
}

/// `$val =~ s/[ \0]+$//`, the `ValueConv` on every `string[N]` field here.
///
/// The generated `Fmt::Str` has already cut the value at the first NUL, which
/// is what ExifTool's `string` format does before any conversion; what remains
/// is the trailing padding. An all-blank field converts to the empty string
/// and ExifTool still reports it, so this does not suppress the tag.
fn trimmed_string(raw: &DecodedValue) -> Option<String> {
    let DecodedValue::String(text) = raw else {
        return None;
    };
    Some(text.trim_end_matches([' ', '\0']).to_string())
}

/// `join(".",unpack("H2H2",$val))` then `"$val micrometers"` (PhotoCD.pm:110).
///
/// The two bytes are BCD-ish: they are rendered as hex digits, not decoded as
/// a number, so `0x11 0x48` is "11.48" and not "17.72".
fn scanner_pixel_size(raw: &DecodedValue) -> Option<String> {
    let DecodedValue::Undefined(bytes) = raw else {
        return None;
    };
    let [high, low] = bytes.as_slice() else {
        return None;
    };
    Some(format!("{high:02x}.{low:02x} micrometers"))
}

/// `ConvertUnixTime($val,1)` with `RawConv => '$val == 0xffffffff ? undef : $val'`.
///
/// The `1` is ExifTool's `$toLocal`, so the rendering is local civil time with
/// the UTC offset in force at that instant -- the same conversion the File
/// group's timestamps already use.
fn unix_date(raw: &DecodedValue) -> Option<String> {
    let DecodedValue::Integer(seconds) = raw else {
        return None;
    };
    if *seconds == 0xffff_ffff {
        return None;
    }
    Some(format_unix_time_local(*seconds))
}

/// ExifTool's fallback for a value its `PrintConv` hash does not list
/// (ExifTool.pm:3610). None of these tags declare `PrintHex`, so it is decimal.
fn unknown(value: i64) -> String {
    format!("Unknown ({value})")
}

/// A short-hand for `RawAccess::new` that reads more naturally at each of
/// this parser's dozens of call sites.
fn raw_access<'a>(
    field: &'a DecodedField,
    acknowledged: Acknowledged,
    justification: &'static PerlCitation,
) -> Option<RawAccess<'a>> {
    RawAccess::new(field, acknowledged, justification)
}

/// `emit`/`emit_raw`'s rendering, collapsed to the string this parser inserts:
/// the `PrintConv` string when it matched, or ExifTool's `Unknown (N)`
/// fallback (see [`unknown`]) when it fell back to a raw integer that had no
/// enum entry. Any other `TagValue` shape (or `None`, meaning `emit`
/// refused) yields nothing to insert.
fn render_or_unknown(value: Option<TagValue>) -> Option<String> {
    match value {
        Some(TagValue::String(s)) => Some(s),
        Some(TagValue::Integer(raw)) => Some(unknown(raw)),
        _ => None,
    }
}

/// `($$self{Orient} & 0x01 ? 512 : 768) * ($val * 2 || 1)` and its transpose
/// (PhotoCD.pm:1538.1, :1538.2).
///
/// `$val` is the 2-bit resolution code: 0=Base (768x512), 1=4Base, 2=16Base.
/// Perl's `||` makes code 0 a multiplier of 1 rather than 0, and an odd
/// orientation swaps the two base dimensions because the image is stored
/// rotated a quarter turn.
fn base_dimension(orient: i64, code: i64, rotated_base: i64, upright_base: i64) -> i64 {
    let base = if orient & 0x01 != 0 {
        rotated_base
    } else {
        upright_base
    };
    let multiplier = if code == 0 { 1 } else { code * 2 };
    base * multiplier
}

impl PhotoCDParser {
    /// Reads the Image Pac info block, or `None` when this is not a PhotoCD.
    fn read_ipi(reader: &dyn FileReader) -> Option<&[u8]> {
        if reader.size() < IPI_OFFSET + IPI_LEN as u64 {
            return None;
        }
        let block = reader.read(IPI_OFFSET, IPI_LEN).ok()?;
        block.starts_with(IPI_SIGNATURE).then_some(block)
    }
}

impl FormatParser for PhotoCDParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let block = Self::read_ipi(reader).ok_or_else(|| {
            ExifToolError::parse_error("not a Kodak Photo CD Image Pac (no PCD_IPI at 2048)")
        })?;
        let table = find_table("PhotoCD", "Main")
            .ok_or_else(|| ExifToolError::parse_error("missing generated PhotoCD::Main table"))?;

        // SetByteOrder('MM') -- PhotoCD.pm:457.
        let decode = decode_binary_table(table, block, ByteOrder::Big);
        let find = |name: &str| -> Option<&DecodedField> {
            decode
                .fields()
                .iter()
                .find(|field| field.field.name == name)
        };
        // `Orientation` (`RawConv => '$$self{Orient} = $val'`, PhotoCD.pm:402)
        // and `CopyrightStatus` (`RawConv => '$$self{CopyrightStatus} = $val'`
        // + `Condition => '$$self{HasSBA}'`, PhotoCD.pm:386-387) are both
        // withheld from `emit` -- these `RawAccess`es are how `DataMembers`
        // reaches their raw integers, exactly as ExifTool's `RawConv` does
        // when it populates `$$self{...}` before any later tag is read.
        let orient = find("Orientation")
            .and_then(|field| RawAccess::new(field, Acknowledged::RAW_CONV, &ORIENTATION))
            .and_then(|access| match access.raw() {
                DecodedValue::Integer(v) => Some(*v),
                _ => None,
            });
        let copyright_status = find("CopyrightStatus")
            .and_then(|field| {
                RawAccess::new(
                    field,
                    Acknowledged::RAW_CONV | Acknowledged::CONDITION,
                    &COPYRIGHT_STATUS,
                )
            })
            .and_then(|access| match access.raw() {
                DecodedValue::Integer(v) => Some(*v),
                _ => None,
            });

        let members = DataMembers {
            has_sba: block.get(HAS_SBA_RANGE) == Some(b"SBA"),
            orient: orient.unwrap_or(0),
            copyright_status,
        };

        let mut metadata = MetadataMap::new();
        // `crate::filetype` identifies PCD from the extension alone -- its
        // magic-number pass only sees the first 1 KiB, and the marker is at
        // 2048 -- so a correctly-named file already reports `File:FileType`
        // and this bare key is dropped by `normalize_identity_tags`. On a PCD
        // that arrives as `.dat`, which the pinned ExifTool still calls PCD,
        // it fills the `Unknown` the tables produced. That fill is the one
        // identity contribution a parser is allowed to make.
        metadata.insert("FileType", TagValue::new_string("PCD"));

        for field in decode.fields() {
            let name = field.field.name;
            // Every scene-balance tag is `Condition => '$$self{HasSBA}'`
            // (PhotoCD.pm:229, :232, :327, :333). ExifTool does not report
            // them at all on a file without the marker. This external gate is
            // the `condition` acknowledgment every `RawAccess` below that
            // covers `Acknowledged::CONDITION` is justified by.
            let gated_on_sba = matches!(
                name,
                "SceneBalanceAlgorithmRevision"
                    | "SceneBalanceAlgorithmCommand"
                    | "SceneBalanceAlgorithmFilmID"
                    | "CopyrightStatus"
                    | "CopyrightFileName"
            );
            if gated_on_sba && !members.has_sba {
                continue;
            }

            let value = match name {
                // Hidden, and its RawConv returns undef: it exists only to set
                // `HasSBA`, which was read from the block above.
                "HasSBA" => continue,

                "SpecificationVersion" => raw_access(
                    field,
                    Acknowledged::VALUE_CONV | Acknowledged::RAW_CONV,
                    &SPECIFICATION_VERSION,
                )
                .and_then(|access| dotted_pair_or_na(access.raw())),
                "AuthoringSoftwareRelease" => raw_access(
                    field,
                    Acknowledged::VALUE_CONV | Acknowledged::RAW_CONV,
                    &AUTHORING_SOFTWARE_RELEASE,
                )
                .and_then(|access| dotted_pair_or_na(access.raw())),
                "ImageMagnificationDescriptor" => raw_access(
                    field,
                    Acknowledged::VALUE_CONV,
                    &IMAGE_MAGNIFICATION_DESCRIPTOR,
                )
                .and_then(|access| dotted_pair(access.raw())),
                "SceneBalanceAlgorithmRevision" => raw_access(
                    field,
                    Acknowledged::VALUE_CONV | Acknowledged::CONDITION,
                    &SCENE_BALANCE_ALGORITHM_REVISION,
                )
                .and_then(|access| dotted_pair(access.raw())),
                "CreateDate" => raw_access(
                    field,
                    Acknowledged::VALUE_CONV | Acknowledged::RAW_CONV,
                    &CREATE_DATE,
                )
                .and_then(|access| unix_date(access.raw())),
                "ModifyDate" => raw_access(
                    field,
                    Acknowledged::VALUE_CONV | Acknowledged::RAW_CONV,
                    &MODIFY_DATE,
                )
                .and_then(|access| unix_date(access.raw())),
                "ScannerPixelSize" => {
                    raw_access(field, Acknowledged::VALUE_CONV, &SCANNER_PIXEL_SIZE)
                        .and_then(|access| scanner_pixel_size(access.raw()))
                }

                "ProductType" => raw_access(field, Acknowledged::VALUE_CONV, &PRODUCT_TYPE)
                    .and_then(|access| trimmed_string(access.raw())),
                "ScannerVendorID" => {
                    raw_access(field, Acknowledged::VALUE_CONV, &SCANNER_VENDOR_ID)
                        .and_then(|access| trimmed_string(access.raw()))
                }
                "ScannerProductID" => {
                    raw_access(field, Acknowledged::VALUE_CONV, &SCANNER_PRODUCT_ID)
                        .and_then(|access| trimmed_string(access.raw()))
                }
                "ScannerFirmwareVersion" => {
                    raw_access(field, Acknowledged::VALUE_CONV, &SCANNER_FIRMWARE_VERSION)
                        .and_then(|access| trimmed_string(access.raw()))
                }
                "ScannerFirmwareDate" => {
                    raw_access(field, Acknowledged::VALUE_CONV, &SCANNER_FIRMWARE_DATE)
                        .and_then(|access| trimmed_string(access.raw()))
                }
                "ScannerSerialNumber" => {
                    raw_access(field, Acknowledged::VALUE_CONV, &SCANNER_SERIAL_NUMBER)
                        .and_then(|access| trimmed_string(access.raw()))
                }
                "ImageWorkstationMake" => {
                    raw_access(field, Acknowledged::VALUE_CONV, &IMAGE_WORKSTATION_MAKE)
                        .and_then(|access| trimmed_string(access.raw()))
                }
                "PhotoFinisherName" => {
                    raw_access(field, Acknowledged::VALUE_CONV, &PHOTO_FINISHER_NAME)
                        .and_then(|access| trimmed_string(access.raw()))
                }

                "CopyrightFileName" => {
                    if members.copyright_status != Some(COPYRIGHT_RESTRICTED) {
                        continue;
                    }
                    raw_access(
                        field,
                        Acknowledged::VALUE_CONV | Acknowledged::CONDITION,
                        &COPYRIGHT_FILE_NAME,
                    )
                    .and_then(|access| trimmed_string(access.raw()))
                }

                "ImageWidth" | "ImageHeight" => {
                    let citation = if name == "ImageWidth" {
                        &IMAGE_WIDTH
                    } else {
                        &IMAGE_HEIGHT
                    };
                    let Some(access) = raw_access(field, Acknowledged::VALUE_CONV, citation) else {
                        continue;
                    };
                    let DecodedValue::Integer(code) = access.raw() else {
                        continue;
                    };
                    let size = if name == "ImageWidth" {
                        base_dimension(members.orient, *code, 512, 768)
                    } else {
                        base_dimension(members.orient, *code, 768, 512)
                    };
                    metadata.insert(format!("PhotoCD:{name}"), TagValue::Integer(size));
                    continue;
                }

                // `SceneBalanceAlgorithmCommand`/`FilmID` are `Condition`-gated
                // exactly like `SceneBalanceAlgorithmRevision` above; every
                // other name reaching here (`ImageMedium`, `CharacterSet`) is
                // `Omitted::NONE`. Both cases are an enum the generator
                // transcribed whole, with no ValueConv between the bytes and
                // its keys, so `emit`/`emit_raw` render it directly.
                "SceneBalanceAlgorithmCommand" => render_or_unknown(
                    raw_access(
                        field,
                        Acknowledged::CONDITION,
                        &SCENE_BALANCE_ALGORITHM_COMMAND,
                    )
                    .map(|access| access.emit_raw()),
                ),
                "SceneBalanceAlgorithmFilmID" => render_or_unknown(
                    raw_access(
                        field,
                        Acknowledged::CONDITION,
                        &SCENE_BALANCE_ALGORITHM_FILM_ID,
                    )
                    .map(|access| access.emit_raw()),
                ),
                // `RawConv => '$$self{CopyrightStatus} = $val'` sets
                // `raw_conv`, and the `Condition => '$$self{HasSBA}'` gate
                // (already applied above via `gated_on_sba`) sets
                // `condition`; `emit` would otherwise refuse this field even
                // though its two-entry `PrintConv` needs nothing else.
                "CopyrightStatus" => render_or_unknown(
                    raw_access(
                        field,
                        Acknowledged::RAW_CONV | Acknowledged::CONDITION,
                        &COPYRIGHT_STATUS,
                    )
                    .map(|access| access.emit_raw()),
                ),
                // `RawConv => '$$self{Orient} = $val'` sets `raw_conv`; the
                // `PrintConv` itself needs nothing else. `members.orient`
                // above reads the same field through its own `RawAccess`.
                "Orientation" => render_or_unknown(
                    raw_access(field, Acknowledged::RAW_CONV, &ORIENTATION)
                        .map(|access| access.emit_raw()),
                ),
                _ => render_or_unknown(field.emit()),
            };

            if let Some(value) = value {
                metadata.insert(format!("PhotoCD:{name}"), TagValue::new_string(value));
            }
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::PCD)
    }
}

/// Parses metadata from a Kodak Photo CD file.
pub fn parse_pcd_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    PhotoCDParser
        .parse(reader)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// Byte 1538 of `combined-samples/PhotoCD.pcd`: orientation code 1,
    /// resolution code 2, compression class 0.
    const SAMPLE_1538: u8 = 0x09;

    /// A synthetic Image Pac carrying the fields this parser reads.
    ///
    /// The values are the ones `combined-samples/PhotoCD.pcd` holds, so the
    /// expectations below are the strings the pinned ExifTool 13.59 prints for
    /// that file -- but the block is built here rather than read from the
    /// sample corpus, which is a developer cache CI does not have.
    fn sample_ipi() -> Vec<u8> {
        let mut file = vec![0u8; 2048];
        let mut ipi = vec![0u8; IPI_LEN];
        ipi[..7].copy_from_slice(IPI_SIGNATURE);
        ipi[7] = 0;
        ipi[8] = 6; // SpecificationVersion 0.6
        ipi[9] = 7;
        ipi[10] = 61; // AuthoringSoftwareRelease 7.61
        ipi[11] = 1;
        ipi[12] = 0; // ImageMagnificationDescriptor 1.0
        ipi[13..17].copy_from_slice(&993_619_776u32.to_be_bytes()); // CreateDate
        ipi[17..21].copy_from_slice(&993_619_776u32.to_be_bytes()); // ModifyDate
        ipi[21] = 0; // ImageMedium: Color negative
        ipi[22..42].copy_from_slice(b"080/11 SPD 0100     ");
        ipi[42..62].copy_from_slice(b"KODAK     /4220     ");
        ipi[62..78].copy_from_slice(b"FilmScanner 2000");
        ipi[78..82].copy_from_slice(b"4.17");
        ipi[82..90].copy_from_slice(b"        "); // ScannerFirmwareDate: blank
        ipi[90..110].copy_from_slice(b"436                 ");
        ipi[110] = 0x11;
        ipi[111] = 0x48; // ScannerPixelSize 11.48
        ipi[112..132].copy_from_slice(b"Eastman Kodak       ");
        ipi[132] = 3; // CharacterSet: 95 characters ISO 646
        ipi[165..190].copy_from_slice(b"Prolab, Inc. 206-547-5447");
        ipi[225..228].copy_from_slice(b"SBA");
        ipi[228] = 6;
        ipi[229] = 7; // SceneBalanceAlgorithmRevision 6.7
        ipi[230] = 0; // SceneBalanceAlgorithmCommand
        ipi[325..327].copy_from_slice(&72u16.to_be_bytes()); // FilmID
        ipi[331] = 0xff; // CopyrightStatus: Not specified
        ipi[1538] = SAMPLE_1538;
        file.extend_from_slice(&ipi);
        file
    }

    fn parse(data: Vec<u8>) -> MetadataMap {
        PhotoCDParser
            .parse(&TestReader::new(data))
            .expect("synthetic Image Pac must parse")
    }

    /// End-to-end against the pinned ExifTool 13.59's output for the corpus
    /// sample these bytes reproduce, group-qualified and value for value.
    #[test]
    fn synthetic_image_pac_matches_exiftool_tag_for_tag() {
        let metadata = parse(sample_ipi());
        let expected = [
            ("PhotoCD:SpecificationVersion", "0.6"),
            ("PhotoCD:AuthoringSoftwareRelease", "7.61"),
            ("PhotoCD:ImageMagnificationDescriptor", "1.0"),
            ("PhotoCD:ImageMedium", "Color negative"),
            ("PhotoCD:ProductType", "080/11 SPD 0100"),
            ("PhotoCD:ScannerVendorID", "KODAK     /4220"),
            ("PhotoCD:ScannerProductID", "FilmScanner 2000"),
            ("PhotoCD:ScannerFirmwareVersion", "4.17"),
            ("PhotoCD:ScannerFirmwareDate", ""),
            ("PhotoCD:ScannerSerialNumber", "436"),
            ("PhotoCD:ScannerPixelSize", "11.48 micrometers"),
            ("PhotoCD:ImageWorkstationMake", "Eastman Kodak"),
            ("PhotoCD:CharacterSet", "95 characters ISO 646"),
            ("PhotoCD:PhotoFinisherName", "Prolab, Inc. 206-547-5447"),
            ("PhotoCD:SceneBalanceAlgorithmRevision", "6.7"),
            (
                "PhotoCD:SceneBalanceAlgorithmCommand",
                "Neutral SBA On, Color SBA On",
            ),
            (
                "PhotoCD:SceneBalanceAlgorithmFilmID",
                "Kodak Gold 100 Gen 2",
            ),
            ("PhotoCD:CopyrightStatus", "Not specified"),
            ("PhotoCD:Orientation", "Rotate 270 CW"),
            (
                "PhotoCD:CompressionClass",
                "Class 1 - 35mm film; Pictoral hard copy",
            ),
        ];
        for (key, want) in expected {
            assert_eq!(
                metadata.get_string(key),
                Some(want),
                "{key} must match ExifTool"
            );
        }
        // The two masked fractional entries are numbers, not strings.
        assert_eq!(
            metadata.get("PhotoCD:ImageWidth"),
            Some(&TagValue::Integer(2048))
        );
        assert_eq!(
            metadata.get("PhotoCD:ImageHeight"),
            Some(&TagValue::Integer(3072))
        );
        // Local-time rendering is the machine's zone, so pin the shape rather
        // than a fixed offset: CI runs UTC and a developer usually does not.
        let created = metadata
            .get_string("PhotoCD:CreateDate")
            .expect("CreateDate");
        assert!(
            created.starts_with("2001:06:2") && created.len() == "2001:06:27 00:29:36-05:00".len(),
            "unexpected CreateDate rendering {created:?}"
        );
        // Hidden, and never a tag of its own.
        assert!(metadata.get("PhotoCD:HasSBA").is_none());
        // A correctly-named file gets its File:FileType from the tables; the
        // bare key is the parser's fill for the wrongly-named case.
        assert_eq!(metadata.get_string("FileType"), Some("PCD"));
    }

    /// Without the `SBA` marker ExifTool reports none of the scene-balance
    /// group, and `CopyrightStatus` is inside that group -- so a file lacking
    /// it must lose five tags, not render them from whatever bytes are there.
    #[test]
    fn the_scene_balance_group_is_absent_without_the_marker() {
        let mut data = sample_ipi();
        data[2048 + 225..2048 + 228].copy_from_slice(b"\0\0\0");
        let metadata = parse(data);
        for name in [
            "SceneBalanceAlgorithmRevision",
            "SceneBalanceAlgorithmCommand",
            "SceneBalanceAlgorithmFilmID",
            "CopyrightStatus",
            "CopyrightFileName",
        ] {
            assert!(
                metadata.get(&format!("PhotoCD:{name}")).is_none(),
                "{name} is Condition => '$$self{{HasSBA}}'"
            );
        }
        // Everything outside the group still reports.
        assert_eq!(
            metadata.get_string("PhotoCD:ImageMedium"),
            Some("Color negative")
        );
    }

    /// `CopyrightFileName` needs `CopyrightStatus == 1`, not merely a
    /// non-empty name field.
    #[test]
    fn copyright_file_name_follows_the_status_it_depends_on() {
        let mut data = sample_ipi();
        data[2048 + 332..2048 + 344].copy_from_slice(b"NOTICE.TXT  ");
        assert!(
            parse(data.clone())
                .get("PhotoCD:CopyrightFileName")
                .is_none(),
            "status 0xff means the name must stay unreported"
        );

        data[2048 + 331] = 1;
        let metadata = parse(data);
        assert_eq!(
            metadata.get_string("PhotoCD:CopyrightStatus"),
            Some("Restrictions apply")
        );
        assert_eq!(
            metadata.get_string("PhotoCD:CopyrightFileName"),
            Some("NOTICE.TXT")
        );
    }

    /// A file with no marker at 2048 is not a PhotoCD, and neither is one too
    /// short to hold the block ExifTool insists on reading in full.
    #[test]
    fn a_block_without_the_marker_is_refused() {
        let mut data = sample_ipi();
        data[2048..2055].copy_from_slice(b"NOT_IPI");
        assert!(PhotoCDParser.parse(&TestReader::new(data)).is_err());

        let short = sample_ipi()[..4000].to_vec();
        assert!(PhotoCDParser.parse(&TestReader::new(short)).is_err());
    }

    #[test]
    fn dotted_pair_joins_rather_than_computing() {
        let pair = DecodedValue::Array(vec![DecodedValue::Integer(7), DecodedValue::Integer(61)]);
        assert_eq!(dotted_pair(&pair).as_deref(), Some("7.61"));
        // 0 and 6 must render "0.6", which a numeric read would too -- but 7
        // and 61 is where a fixed-point reading would print "7.61" only by
        // luck and "1.0" would come out "1".
        let zero_six =
            DecodedValue::Array(vec![DecodedValue::Integer(1), DecodedValue::Integer(0)]);
        assert_eq!(dotted_pair(&zero_six).as_deref(), Some("1.0"));
    }

    #[test]
    fn version_pair_of_255s_is_not_available() {
        let na = DecodedValue::Array(vec![DecodedValue::Integer(255), DecodedValue::Integer(255)]);
        assert_eq!(dotted_pair_or_na(&na).as_deref(), Some("n/a"));
        // One 255 is a real version, not the sentinel.
        let real = DecodedValue::Array(vec![DecodedValue::Integer(255), DecodedValue::Integer(3)]);
        assert_eq!(dotted_pair_or_na(&real).as_deref(), Some("255.3"));
    }

    #[test]
    fn scanner_pixel_size_is_hex_digits_not_a_number() {
        let raw = DecodedValue::Undefined(vec![0x11, 0x48]);
        assert_eq!(
            scanner_pixel_size(&raw).as_deref(),
            Some("11.48 micrometers"),
            "unpack(\"H2H2\") renders the bytes as hex; 0x11 0x48 is 11.48, not 17.72"
        );
    }

    #[test]
    fn trailing_pad_is_stripped_but_an_empty_field_survives() {
        assert_eq!(
            trimmed_string(&DecodedValue::String("436                 ".into())).as_deref(),
            Some("436")
        );
        // ScannerFirmwareDate on the sample is eight spaces, and ExifTool
        // still reports it -- as the empty string.
        assert_eq!(
            trimmed_string(&DecodedValue::String("        ".into())).as_deref(),
            Some("")
        );
    }

    #[test]
    fn an_all_ones_date_is_absent_rather_than_1970() {
        assert_eq!(unix_date(&DecodedValue::Integer(0xffff_ffff)), None);
        assert!(unix_date(&DecodedValue::Integer(993_619_776)).is_some());
    }

    /// The sample's stored image is a quarter turn off, so the base dimensions
    /// swap: 512x768 scaled by 4 is the 2048x3072 ExifTool reports.
    #[test]
    fn rotated_image_swaps_the_base_dimensions() {
        let orient = i64::from(SAMPLE_1538 & 0x03);
        let code = i64::from((SAMPLE_1538 & 0x0c) >> 2);
        assert_eq!(orient, 1);
        assert_eq!(code, 2);
        assert_eq!(base_dimension(orient, code, 512, 768), 2048);
        assert_eq!(base_dimension(orient, code, 768, 512), 3072);

        // Code 0 is Base resolution, and Perl's `$val * 2 || 1` makes that a
        // multiplier of one rather than collapsing the image to zero.
        assert_eq!(base_dimension(0, 0, 512, 768), 768);
        assert_eq!(base_dimension(0, 0, 768, 512), 512);
        // An upright image keeps 768x512.
        assert_eq!(base_dimension(0, 1, 512, 768), 1536);
        assert_eq!(base_dimension(0, 1, 768, 512), 1024);
    }

    /// The three fields this parser reads out of byte 1538 are `1538.1`,
    /// `1538.2` and `1538.3` -- fractional entries that only decode because
    /// they declare a Mask. If the generated table ever loses those masks the
    /// decoder refuses them again and this format silently loses three tags.
    #[test]
    fn the_shared_byte_still_carries_its_masks() {
        let table = find_table("PhotoCD", "Main").expect("generated PhotoCD::Main");
        for name in ["ImageWidth", "ImageHeight", "CompressionClass"] {
            let field = table
                .fields
                .iter()
                .find(|f| f.name == name)
                .unwrap_or_else(|| panic!("{name} missing from PhotoCD::Main"));
            assert_eq!(field.index, 1538, "{name} shares byte 1538");
            assert!(field.sub.is_some(), "{name} is a fractional entry");
            assert!(
                field.mask.is_some(),
                "{name} needs its Mask to be decodable at all"
            );
        }
    }
}
