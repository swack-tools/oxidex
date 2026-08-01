//! Multi-Picture Format (MPF) parser for JPEG APP2 segments
//!
//! MPF is used in dual-camera phones and 3D cameras to store multiple images
//! in a single JPEG file. The MPF data is stored in an APP2 segment with the
//! "MPF\x00" identifier, followed by a TIFF-like IFD structure.
//!
//! # MPF Structure
//!
//! ```text
//! APP2 marker (0xFFE2)
//! Length (2 bytes, big-endian)
//! "MPF\x00" identifier (4 bytes)
//! TIFF header (8 bytes: byte order + magic 42 + IFD offset)
//! MP Index IFD (IFD 0) - contains MPFVersion, NumberOfImages, MPEntry
//! MP Attribute IFD (per image) - contains positioning/3D metadata
//! ```
//!
//! # Groups
//!
//! MPF.pm files its tags under three different family-1 groups, and the group
//! -- not the tag name -- is what carries the image index:
//!
//! * `MPF0` -- the MP Index IFD (MPF.pm:24, `GROUPS => { 0 => 'MPF', 1 =>
//!   'MPF0', 2 => 'Image'}`).
//! * `MPF1` -- the MP Attribute IFD. ExifTool.pm:7959 sets `$dirInfo{Multi} =
//!   1;  # the MP Attribute IFD will be MPF1` before handing the segment to
//!   `ProcessTIFF`.
//! * `MPImage1`, `MPImage2`, ... -- one group per MP Entry. MPF.pm:96 gives the
//!   MPImage table group1 `MPImage`, and `ProcessMPImageList` appends the index
//!   per record (MPF.pm:247, `$$et{SET_GROUP1} = '+' . ($i + 1);`).
//!
//! The seven per-entry tag NAMES repeat verbatim in every `MPImage#` group;
//! they are never suffixed with the index. `exiftool -a -G1 -s` on
//! `Apple/Apple_iPhone16Pro.jpg`:
//!
//! ```text
//! [MPF0]     MPFVersion                 : 0100
//! [MPImage1] MPImageStart               : 0
//! [MPImage2] MPImageStart               : 4289837
//! ```
//!
//! # References
//!
//! - CIPA DC-007-2009 Multi-Picture Format Specification

use crate::core::{MetadataMap, TagValue};
use crate::exiftool_tables::{PrintConv, find_table};
use crate::io::EndianReader;

// =============================================================================
// Constants: MPF Tag IDs
// =============================================================================

/// MPF Version tag (MP Index IFD)
const MPF_VERSION: u16 = 0xB000;
/// Number of Images tag (MP Index IFD)
const NUMBER_OF_IMAGES: u16 = 0xB001;
/// MP Entry tag (MP Index IFD) - contains image info array
const MP_ENTRY: u16 = 0xB002;
/// Image UID List tag (MP Index IFD)
const IMAGE_UID_LIST: u16 = 0xB003;
/// Total Frames tag (MP Index IFD)
const TOTAL_FRAMES: u16 = 0xB004;

/// MP Individual Number tag (MP Attribute IFD)
const MP_INDIVIDUAL_NUM: u16 = 0xB101;
/// Panorama Orientation tag (MP Attribute IFD)
const PAN_ORIENTATION: u16 = 0xB201;
/// Panorama Horizontal Overlap tag (MP Attribute IFD)
const PAN_OVERLAP_H: u16 = 0xB202;
/// Panorama Vertical Overlap tag (MP Attribute IFD)
const PAN_OVERLAP_V: u16 = 0xB203;
/// Base Viewpoint Number tag (MP Attribute IFD)
const BASE_VIEWPOINT_NUM: u16 = 0xB204;
/// Convergence Angle tag (MP Attribute IFD)
const CONVERGENCE_ANGLE: u16 = 0xB205;
/// Baseline Length tag (MP Attribute IFD)
const BASELINE_LENGTH: u16 = 0xB206;
/// Vertical Divergence tag (MP Attribute IFD)
const VERTICAL_DIVERGENCE: u16 = 0xB207;
/// Axis Distance X tag (MP Attribute IFD)
const AXIS_DISTANCE_X: u16 = 0xB208;
/// Axis Distance Y tag (MP Attribute IFD)
const AXIS_DISTANCE_Y: u16 = 0xB209;
/// Axis Distance Z tag (MP Attribute IFD)
const AXIS_DISTANCE_Z: u16 = 0xB20A;
/// Yaw Angle tag (MP Attribute IFD)
const YAW_ANGLE: u16 = 0xB20B;
/// Pitch Angle tag (MP Attribute IFD)
const PITCH_ANGLE: u16 = 0xB20C;
/// Roll Angle tag (MP Attribute IFD)
const ROLL_ANGLE: u16 = 0xB20D;

// =============================================================================
// MPF Byte Order Detection
// =============================================================================

/// Byte order for MPF TIFF-like structure
#[derive(Debug, Clone, Copy, PartialEq)]
enum MpfByteOrder {
    LittleEndian,
    BigEndian,
}

// =============================================================================
// Public API
// =============================================================================

/// Parses an MPF APP2 segment and extracts Multi-Picture Format metadata.
///
/// MPF segments start with the "MPF\x00" identifier followed by a TIFF-like
/// IFD structure containing:
/// - MP Index IFD with version, image count, and entry array
/// - Optional MP Attribute IFDs for each image with positioning metadata
///
/// # Arguments
///
/// * `data` - Raw APP2 segment data (should start with "MPF\x00")
/// * `tiff_base` - Absolute file offset of the MPF TIFF header, i.e. of the
///   byte immediately after the `MPF\0` identifier. `MPImageStart` is stored
///   relative to it and ExifTool rebases against it; see
///   [`parse_mp_entry_array`].
/// * `metadata` - MetadataMap to populate with extracted MPF tags
///
/// # Returns
///
/// * `Ok(())` - Successfully parsed MPF data
/// * `Err(String)` - Parse error with description
///
/// # Example
///
/// ```ignore
/// use oxidex::parsers::jpeg::mpf_parser::parse_mpf_segment;
/// use oxidex::core::MetadataMap;
///
/// let mut metadata = MetadataMap::new();
/// parse_mpf_segment(app2_data, tiff_base, &mut metadata)?;
/// ```
pub fn parse_mpf_segment(
    data: &[u8],
    tiff_base: u64,
    metadata: &mut MetadataMap,
) -> Result<(), String> {
    // Minimum size: 4 (identifier) + 8 (TIFF header) = 12 bytes
    if data.len() < 12 {
        return Err("MPF segment too short".to_string());
    }

    // Verify MPF identifier "MPF\x00"
    if &data[0..4] != b"MPF\0" {
        return Err("Not an MPF segment (invalid identifier)".to_string());
    }

    // TIFF-like structure starts at offset 4 (after "MPF\0")
    let tiff_data = &data[4..];

    // Detect byte order from TIFF header (bytes 0-1)
    let byte_order = detect_byte_order(&tiff_data[0..2])?;

    // Create EndianReader based on detected byte order
    let reader = match byte_order {
        MpfByteOrder::LittleEndian => EndianReader::little_endian(tiff_data),
        MpfByteOrder::BigEndian => EndianReader::big_endian(tiff_data),
    };

    // Verify TIFF magic number 42 (bytes 2-3)
    let magic = reader.u16_at(2).ok_or("Failed to read TIFF magic")?;
    if magic != 42 {
        return Err(format!(
            "Invalid MPF TIFF magic number: expected 42, got {}",
            magic
        ));
    }

    // Read MP Index IFD offset (bytes 4-7)
    let mp_index_ifd_offset = reader.u32_at(4).ok_or("Failed to read IFD offset")? as usize;

    // Parse MP Index IFD
    parse_mp_index_ifd(&reader, mp_index_ifd_offset, tiff_base, metadata)?;

    Ok(())
}

// =============================================================================
// Internal Functions
// =============================================================================

/// Detects the byte order from the TIFF header marker.
///
/// # Arguments
///
/// * `marker` - 2-byte slice containing "II" (little-endian) or "MM" (big-endian)
///
/// # Returns
///
/// * `Ok(MpfByteOrder)` - Detected byte order
/// * `Err(String)` - Invalid byte order marker
fn detect_byte_order(marker: &[u8]) -> Result<MpfByteOrder, String> {
    match marker {
        b"II" => Ok(MpfByteOrder::LittleEndian),
        b"MM" => Ok(MpfByteOrder::BigEndian),
        _ => Err(format!(
            "Invalid MPF byte order marker: {:02X} {:02X}",
            marker.first().unwrap_or(&0),
            marker.get(1).unwrap_or(&0)
        )),
    }
}

/// Parses the MP Index IFD (IFD 0) containing MPF version, image count, and entry data.
///
/// # Arguments
///
/// * `reader` - EndianReader with correct byte order for the MPF data
/// * `offset` - Offset to the start of the IFD within the TIFF structure
/// * `metadata` - MetadataMap to populate with tags
fn parse_mp_index_ifd(
    reader: &EndianReader,
    offset: usize,
    tiff_base: u64,
    metadata: &mut MetadataMap,
) -> Result<(), String> {
    // Read IFD entry count (2 bytes)
    let entry_count = reader
        .u16_at(offset)
        .ok_or("Failed to read MP Index IFD entry count")? as usize;

    // Each IFD entry is 12 bytes
    // Structure: tag (2) + type (2) + count (4) + value/offset (4)
    let mut mp_entry_data: Option<Vec<u8>> = None;
    let mut mp_entry_count: usize = 0;

    for i in 0..entry_count {
        let entry_offset = offset + 2 + (i * 12);

        // Read tag ID
        let tag_id = reader.u16_at(entry_offset).ok_or("Failed to read tag ID")?;
        // Read field type
        let field_type = reader
            .u16_at(entry_offset + 2)
            .ok_or("Failed to read field type")?;
        // Read value count
        let value_count = reader
            .u32_at(entry_offset + 4)
            .ok_or("Failed to read value count")? as usize;
        // Read value/offset (4 bytes)
        let value_or_offset = reader
            .u32_at(entry_offset + 8)
            .ok_or("Failed to read value/offset")?;

        match tag_id {
            MPF_VERSION => {
                // MPFVersion is typically 4 bytes representing "0100" (version 1.0)
                let version =
                    parse_mpf_version(reader, entry_offset + 8, value_count, value_or_offset)?;
                metadata.insert("MPF0:MPFVersion".to_string(), TagValue::String(version));
            }
            NUMBER_OF_IMAGES => {
                // NumberOfImages is a LONG (4 bytes)
                metadata.insert(
                    "MPF0:NumberOfImages".to_string(),
                    TagValue::Integer(value_or_offset as i64),
                );
            }
            MP_ENTRY => {
                // MPEntry is an array of 16-byte structures (UNDEFINED type)
                // Value count is total bytes, offset points to data
                let data_offset = value_or_offset as usize;
                let data_size = value_count;

                // Store for later processing
                if let Some(bytes) = reader.bytes_at(data_offset, data_size) {
                    mp_entry_data = Some(bytes.to_vec());
                    // Each MP Entry is 16 bytes
                    mp_entry_count = data_size / 16;
                }
            }
            IMAGE_UID_LIST => {
                // ImageUIDList - 33 bytes per image (UNDEFINED type)
                // Match ExifTool format exactly (no comma)
                metadata.insert(
                    "MPF0:ImageUIDList".to_string(),
                    TagValue::String(format!(
                        "(Binary data {} bytes, use -b option to extract)",
                        value_count
                    )),
                );
            }
            TOTAL_FRAMES => {
                // TotalFrames - LONG
                metadata.insert(
                    "MPF0:TotalFrames".to_string(),
                    TagValue::Integer(value_or_offset as i64),
                );
            }
            _ => {
                // Unknown tag in MP Index IFD
                let tag_name = format!("MPF0:0x{:04X}", tag_id);
                let value =
                    parse_generic_ifd_value(reader, field_type, value_count, value_or_offset);
                metadata.insert(tag_name, value);
            }
        }
    }

    // Process MP Entry array if present
    if let Some(entry_data) = mp_entry_data {
        parse_mp_entry_array(&entry_data, mp_entry_count, reader, tiff_base, metadata)?;
    }

    // Check for MP Attribute IFD offset (after IFD entries + next IFD pointer)
    let next_ifd_offset_pos = offset + 2 + (entry_count * 12);
    if let Some(attr_ifd_offset) = reader.u32_at(next_ifd_offset_pos)
        && attr_ifd_offset > 0
    {
        // Parse MP Attribute IFD (IFD 1). This is optional/supplementary data;
        // some cameras (e.g. Fujifilm) write a malformed or absent Attribute IFD,
        // so a failure here should not invalidate the Index IFD data already parsed.
        let _ = parse_mp_attribute_ifd(reader, attr_ifd_offset as usize, metadata);
    }

    Ok(())
}

/// Parses the MPFVersion tag value into a string.
///
/// The version is stored as 4 ASCII characters (e.g., "0100" for version 1.0).
/// Per ExifTool compatibility, we output the raw 4-character string as-is.
///
/// MPF.pm declares the tag with no format override and no PrintConv
/// (`0xb000 => 'MPFVersion'`, MPF.pm:34), so ExifTool prints the four
/// UNDEFINED bytes exactly as they sit in the file. Those bytes are
/// characters, not an integer, and must never be run through the IFD's byte
/// order: reading the value field as a `u32` and re-serializing it with
/// `to_le_bytes()` reversed every big-endian ("MM") MPF segment, which is
/// every real one -- `0100` came back as `0010`. Read the value field's raw
/// bytes instead, which is byte-order independent by construction.
///
/// # Arguments
///
/// * `reader` - EndianReader for accessing the data
/// * `value_field_offset` - Offset of the IFD entry's 4-byte value/offset field
/// * `value_count` - Number of bytes in the version field
/// * `value_or_offset` - Either the inline value or offset to data
fn parse_mpf_version(
    reader: &EndianReader,
    value_field_offset: usize,
    value_count: usize,
    value_or_offset: u32,
) -> Result<String, String> {
    // Version is 4 ASCII bytes: "0100" = version 1.0
    // ExifTool outputs the raw format "0100", not "1.0"
    if value_count <= 4 {
        // Value is stored inline, left-justified, in the entry's 4-byte
        // value field -- read those bytes directly rather than decoding an
        // integer and re-encoding it in some other order.
        if let Some(bytes) = reader.bytes_at(value_field_offset, value_count)
            && let Ok(s) = std::str::from_utf8(bytes)
        {
            // Return raw version string (e.g., "0100") for ExifTool compatibility
            return Ok(s.trim_end_matches('\0').to_string());
        }
    } else {
        // Value is at offset
        let offset = value_or_offset as usize;
        if let Some(bytes) = reader.bytes_at(offset, value_count.min(4))
            && let Ok(s) = std::str::from_utf8(bytes)
        {
            // Return raw version string for ExifTool compatibility
            return Ok(s.trim_end_matches('\0').to_string());
        }
    }
    Ok("Unknown".to_string())
}

/// Parses the MP Entry array containing individual image information.
///
/// Each MP Entry is 16 bytes; the layout is ExifTool's `MPF::MPImage` binary
/// table (MPF.pm:91-158), and every offset, mask and label below is quoted
/// from it:
///
/// | offset | tag | MPF.pm |
/// |---|---|---|
/// | `0` (masked) | `MPImageFlags` | 103-112 |
/// | `0` (masked) | `MPImageFormat` | 113-120 |
/// | `0` (masked) | `MPImageType` | 121-140 |
/// | `4` | `MPImageLength` | 141-144 |
/// | `8` | `MPImageStart` | 145-149 |
/// | `12` | `DependentImage1EntryNumber` | 150-153 |
/// | `14` | `DependentImage2EntryNumber` | 154-157 |
///
/// The record repeats, and ExifTool distinguishes the repeats **by family-1
/// group, not by tag name**: `ProcessMPImageList` runs the same table once per
/// 16-byte record with `$$et{SET_GROUP1} = '+' . ($i + 1);` (MPF.pm:247), which
/// suffixes the table's group1 `MPImage` (MPF.pm:96) with the 1-based index.
/// So entry 2's start offset is `MPImage2:MPImageStart`, never
/// `MPF:MPImage2Offset`.
///
/// # `MPImageStart` is an absolute file offset
///
/// MPF.pm:148 declares `IsOffset => '$val'` on `MPImageStart`. ExifTool applies
/// that in `ProcessBinaryData`:
///
/// ```text
/// ExifTool.pm:10130  if ($$tagInfo{IsOffset} and $$tagInfo{IsOffset} ne '3') {
/// ExifTool.pm:10133      $val += $base + $$self{BASE} if eval $$tagInfo{IsOffset};
/// ```
///
/// `$base` is `$$dirInfo{Base}` (ExifTool.pm:9863). For MPF that is set once,
/// where the APP2 segment is dispatched:
///
/// ```text
/// ExifTool.pm:7956  } elsif ($$segDataPt =~ /^MPF\0/) {
/// ExifTool.pm:7958      DirStart(\%dirInfo, 4, 4);
/// ExifTool.pm:7238      $$dirInfo{Base} = $$dirInfo{DataPos} + $base;
/// ```
///
/// `DataPos` is the absolute file position of the APP2 segment's data, so
/// `Base` is that plus 4 -- the byte after the `MPF\0` identifier, i.e. the
/// start of the MPF TIFF header. It is NOT the segment start, not the marker,
/// and not the file start. `Exif.pm:6852` (`my $subdirBase = $base;`) carries
/// it unchanged into the `MPImageList` subdirectory, and `$$self{BASE}` is 0
/// for a top-level JPEG.
///
/// The `if eval $$tagInfo{IsOffset}` guard is load-bearing: `IsOffset` is the
/// string `'$val'`, so a raw offset of 0 is false and is **not** rebased. That
/// is why the primary image, which the CIPA spec stores as offset 0, prints as
/// `0` rather than as the header position.
///
/// Verified on `Apple/Apple_iPhone12ProMax.jpg`: the `MPF\0` identifier starts
/// at file offset 6265, so `tiff_base` is 6269; entry 2's raw offset is
/// 5171952 and `exiftool -a -G1 -s` prints
/// `[MPImage2] MPImageStart : 5178221` = 5171952 + 6269.
///
/// # Arguments
///
/// * `data` - Raw bytes of the MP Entry array
/// * `count` - Number of entries in the array
/// * `reader` - EndianReader for byte order handling
/// * `tiff_base` - Absolute file offset of the MPF TIFF header
/// * `metadata` - MetadataMap to populate with per-image tags
fn parse_mp_entry_array(
    data: &[u8],
    count: usize,
    reader: &EndianReader,
    tiff_base: u64,
    metadata: &mut MetadataMap,
) -> Result<(), String> {
    // Create reader with same byte order as the main data
    let entry_reader = EndianReader::new(data, reader.byte_order());

    // MPF.pm:206 -- only the FIRST "Large Thumbnail" becomes PreviewImage.
    let mut did_preview = false;

    for i in 0..count {
        let entry_offset = i * 16;

        if entry_offset + 16 > data.len() {
            break;
        }

        // Read Individual Image Attribute (4 bytes)
        let image_attr = entry_reader
            .u32_at(entry_offset)
            .ok_or("Failed to read image attribute")?;

        // Read Individual Image Size (4 bytes)
        let image_size = entry_reader
            .u32_at(entry_offset + 4)
            .ok_or("Failed to read image size")?;

        // Read Individual Image Data Offset (4 bytes)
        let image_offset = entry_reader
            .u32_at(entry_offset + 8)
            .ok_or("Failed to read image offset")?;

        // Read Dependent Image 1 Entry Number (2 bytes)
        let dep_image1 = entry_reader
            .u16_at(entry_offset + 12)
            .ok_or("Failed to read dependent image 1")?;

        // Read Dependent Image 2 Entry Number (2 bytes)
        let dep_image2 = entry_reader
            .u16_at(entry_offset + 14)
            .ok_or("Failed to read dependent image 2")?;

        // Family-1 group carries the index; the tag names do not (MPF.pm:247).
        let group = format!("MPImage{}", i + 1);

        metadata.insert(
            format!("{}:MPImageFlags", group),
            TagValue::String(decode_image_flags(image_attr)),
        );
        metadata.insert(
            format!("{}:MPImageFormat", group),
            TagValue::String(decode_image_format(image_attr)),
        );
        metadata.insert(
            format!("{}:MPImageType", group),
            TagValue::String(decode_image_type(image_attr)),
        );
        metadata.insert(
            format!("{}:MPImageLength", group),
            TagValue::Integer(i64::from(image_size)),
        );
        metadata.insert(
            format!("{}:MPImageStart", group),
            TagValue::Integer(rebase_mp_image_start(image_offset, tiff_base)),
        );
        // ExifTool emits both DependentImage#EntryNumber tags unconditionally,
        // including the very common 0 (verified on every MPF sample in the
        // corpus, e.g. `[MPImage1] DependentImage2EntryNumber : 0`).
        metadata.insert(
            format!("{}:DependentImage1EntryNumber", group),
            TagValue::Integer(i64::from(dep_image1)),
        );
        metadata.insert(
            format!("{}:DependentImage2EntryNumber", group),
            TagValue::Integer(i64::from(dep_image2)),
        );

        // The embedded image itself, as a tag (MPF.pm:190-233 `ExtractMPImages`).
        //
        //   MPF.pm:202  if ($off and $len) {
        //   MPF.pm:204      my $tag = "MPImage$i";
        //   MPF.pm:206      if (not $didPreview and $type and ($type & 0x0f0000) == 0x010000) {
        //   MPF.pm:207          $tag = 'PreviewImage';
        //   MPF.pm:220      my $key = $et->FoundTag($tag, $val, $et->GetGroup("MPImageStart$xtra"));
        //
        // The `$off and $len` guard is why the primary image (stored offset 0)
        // never produces one. The group is inherited from the entry's own
        // `MPImageStart`, so the group index and the tag-name index always
        // agree -- verified over all 732 instances ExifTool reports on the
        // sample corpus (`MPImage1:MPImage1` x2, `MPImage2:MPImage2` x25,
        // `MPImage2:PreviewImage` x664, `MPImage3:MPImage3` x41; zero
        // mismatched pairs).
        //
        // No file access is needed for the printed value. `ExtractImage`
        // (Exif.pm:6121) delegates to `ExtractBinary` (ExifTool.pm:9814), which
        // returns the placeholder before it ever seeks:
        //
        //   ExifTool.pm:9828  if ((not $$options{Binary} or $$self{EXCL_TAG_LOOKUP}{$lcTag}) and
        //   ExifTool.pm:9829       not $$options{Verbose} and not $$options{Validate} and
        //   ExifTool.pm:9830       not $$self{REQ_TAG_LOOKUP}{$lcTag})
        //   ExifTool.pm:9832      return "Binary data $length bytes";
        //
        // which is why ExifTool prints the full length even on the header-only
        // sample files whose image payload was truncated away.
        if image_offset != 0 && image_size != 0 {
            let image_type = image_attr & 0x00ff_ffff;
            let name =
                if !did_preview && image_type != 0 && (image_type & 0x000f_0000) == 0x0001_0000 {
                    did_preview = true;
                    "PreviewImage".to_string()
                } else {
                    format!("MPImage{}", i + 1)
                };
            metadata.insert(
                format!("{}:{}", group, name),
                TagValue::String(format!(
                    "(Binary data {} bytes, use -b option to extract)",
                    image_size
                )),
            );
        }
    }

    Ok(())
}

/// Rebases a raw `MPImageStart` onto the absolute file offset ExifTool reports.
///
/// See [`parse_mp_entry_array`] for the derivation and the ExifTool source
/// lines. A raw 0 is returned unchanged because `IsOffset => '$val'`
/// (MPF.pm:148) is evaluated as a condition (ExifTool.pm:10133).
fn rebase_mp_image_start(raw: u32, tiff_base: u64) -> i64 {
    if raw == 0 {
        return 0;
    }
    (u64::from(raw) + tiff_base) as i64
}

/// Looks up one `MPF::MPImage` field's `PrintConv` in the generated tables.
///
/// The enum bodies are ExifTool's own, extracted from the Perl symbol table by
/// `tools/exiftool-tables` -- not retyped here. `MPImageFlags` has no entry
/// (its `PrintConv` is a `BITMASK`, which the generator declines to
/// approximate), so [`decode_image_flags`] spells that one out against
/// MPF.pm:107-111.
fn mp_image_print_conv(name: &str) -> Option<PrintConv> {
    find_table("MPF", "MPImage")?
        .fields
        .iter()
        .find(|f| f.name == name)
        .map(|f| f.print_conv)
}

/// Decodes `MPImageFlags` from the 32-bit Individual Image Attribute.
///
/// MPF.pm:103-112:
///
/// ```text
///     0.1 => {
///         Name => 'MPImageFlags',
///         Format => 'int32u',
///         Mask => 0xf8000000,
///         PrintConv => { BITMASK => {
///             2 => 'Representative image',
///             3 => 'Dependent child image',
///             4 => 'Dependent parent image',
///         }},
///     },
/// ```
///
/// ExifTool shifts a masked value down by the mask's lowest set bit
/// (ExifTool.pm:5894-5897 computes `BitShift` from `Mask`; ExifTool.pm:10057
/// applies `($val & $mask) >> $$tagInfo{BitShift}`), so the five flag bits
/// become 0..4 and the table's keys are bit numbers within that. `DecodeBits`
/// (ExifTool.pm:6362-6383) walks the bits in ascending order, joins with
/// `", "`, renders an unlisted bit as `[n]`, and returns `(none)` when no bit
/// is set.
fn decode_image_flags(image_attr: u32) -> String {
    const MASK: u32 = 0xf800_0000; // MPF.pm:106
    const SHIFT: u32 = 27; // lowest set bit of 0xf8000000

    let bits = (image_attr & MASK) >> SHIFT;
    let mut set = Vec::new();
    for bit in 0..5u32 {
        if bits & (1 << bit) == 0 {
            continue;
        }
        set.push(match bit {
            // MPF.pm:108-110
            2 => "Representative image".to_string(),
            3 => "Dependent child image".to_string(),
            4 => "Dependent parent image".to_string(),
            other => format!("[{}]", other),
        });
    }
    if set.is_empty() {
        // ExifTool.pm:6382: `return '(none)' unless @bitList;`
        return "(none)".to_string();
    }
    set.join(", ")
}

/// Decodes `MPImageFormat` from the 32-bit Individual Image Attribute.
///
/// MPF.pm:113-120 -- `Mask => 0x07000000`, `PrintConv => { 0 => 'JPEG' }`.
/// An unlisted value prints as `Unknown ($val)` (ExifTool.pm:3610), decimal
/// because the tag has no `PrintHex`.
fn decode_image_format(image_attr: u32) -> String {
    const MASK: u32 = 0x0700_0000; // MPF.pm:116
    const SHIFT: u32 = 24; // lowest set bit of 0x07000000

    let val = i64::from((image_attr & MASK) >> SHIFT);
    mp_image_print_conv("MPImageFormat")
        .and_then(|pc| pc.apply(val))
        .unwrap_or_else(|| format!("Unknown ({})", val))
}

/// Decodes `MPImageType` from the 32-bit Individual Image Attribute.
///
/// MPF.pm:121-140 -- `Mask => 0x00ffffff`, `PrintHex => 1`, and a twelve-entry
/// `PrintConv` which is read out of the generated tables rather than retyped.
/// An unlisted value prints as `Unknown (0x%x)` because `PrintHex` is set
/// (ExifTool.pm:3608).
fn decode_image_type(image_attr: u32) -> String {
    const MASK: u32 = 0x00ff_ffff; // MPF.pm:124 (BitShift 0)

    let val = i64::from(image_attr & MASK);
    mp_image_print_conv("MPImageType")
        .and_then(|pc| pc.apply(val))
        .unwrap_or_else(|| format!("Unknown (0x{:x})", val))
}

/// Parses the MP Attribute IFD containing per-image positioning and 3D metadata.
///
/// # Arguments
///
/// * `reader` - EndianReader for accessing the data
/// * `offset` - Offset to the start of the Attribute IFD
/// * `metadata` - MetadataMap to populate with attribute tags
fn parse_mp_attribute_ifd(
    reader: &EndianReader,
    offset: usize,
    metadata: &mut MetadataMap,
) -> Result<(), String> {
    // Read IFD entry count
    let entry_count = reader
        .u16_at(offset)
        .ok_or("Failed to read MP Attribute IFD entry count")? as usize;

    for i in 0..entry_count {
        let entry_offset = offset + 2 + (i * 12);

        let tag_id = reader.u16_at(entry_offset).ok_or("Failed to read tag ID")?;
        let field_type = reader
            .u16_at(entry_offset + 2)
            .ok_or("Failed to read field type")?;
        let value_count = reader
            .u32_at(entry_offset + 4)
            .ok_or("Failed to read value count")? as usize;
        let value_or_offset = reader
            .u32_at(entry_offset + 8)
            .ok_or("Failed to read value/offset")?;

        let tag_name = match tag_id {
            MP_INDIVIDUAL_NUM => "MPF1:MPIndividualNum".to_string(),
            PAN_ORIENTATION => "MPF1:PanOrientation".to_string(),
            PAN_OVERLAP_H => "MPF1:PanOverlapH".to_string(),
            PAN_OVERLAP_V => "MPF1:PanOverlapV".to_string(),
            BASE_VIEWPOINT_NUM => "MPF1:BaseViewpointNum".to_string(),
            CONVERGENCE_ANGLE => "MPF1:ConvergenceAngle".to_string(),
            BASELINE_LENGTH => "MPF1:BaselineLength".to_string(),
            VERTICAL_DIVERGENCE => "MPF1:VerticalDivergence".to_string(),
            AXIS_DISTANCE_X => "MPF1:AxisDistanceX".to_string(),
            AXIS_DISTANCE_Y => "MPF1:AxisDistanceY".to_string(),
            AXIS_DISTANCE_Z => "MPF1:AxisDistanceZ".to_string(),
            YAW_ANGLE => "MPF1:YawAngle".to_string(),
            PITCH_ANGLE => "MPF1:PitchAngle".to_string(),
            ROLL_ANGLE => "MPF1:RollAngle".to_string(),
            _ => format!("MPF1:0x{:04X}", tag_id),
        };

        let value = parse_generic_ifd_value(reader, field_type, value_count, value_or_offset);
        metadata.insert(tag_name, value);
    }

    Ok(())
}

/// Parses a generic IFD value based on field type.
///
/// # Arguments
///
/// * `reader` - EndianReader for accessing the data
/// * `field_type` - TIFF field type (1=BYTE, 2=ASCII, 3=SHORT, 4=LONG, 5=RATIONAL, etc.)
/// * `value_count` - Number of values
/// * `value_or_offset` - Either the inline value or offset to data
fn parse_generic_ifd_value(
    reader: &EndianReader,
    field_type: u16,
    value_count: usize,
    value_or_offset: u32,
) -> TagValue {
    // Calculate total bytes needed
    let bytes_per_value = match field_type {
        1 | 2 | 7 => 1, // BYTE, ASCII, UNDEFINED
        3 => 2,         // SHORT
        4 | 9 => 4,     // LONG, SLONG
        5 | 10 => 8,    // RATIONAL, SRATIONAL
        _ => 1,
    };
    let total_bytes = bytes_per_value * value_count;

    // Value is inline if it fits in 4 bytes
    if total_bytes <= 4 {
        match field_type {
            1 => TagValue::Integer((value_or_offset & 0xFF) as i64),
            3 => TagValue::Integer((value_or_offset & 0xFFFF) as i64),
            4 => TagValue::Integer(value_or_offset as i64),
            9 => TagValue::Integer(value_or_offset as i32 as i64),
            2 => {
                // ASCII - inline string
                let bytes = value_or_offset.to_le_bytes();
                std::str::from_utf8(&bytes[..value_count.min(4)])
                    .map(|s| TagValue::String(s.trim_end_matches('\0').to_string()))
                    .unwrap_or_else(|_| TagValue::Integer(value_or_offset as i64))
            }
            _ => TagValue::Integer(value_or_offset as i64),
        }
    } else {
        // Value is at offset
        let offset = value_or_offset as usize;
        match field_type {
            2 => {
                // ASCII string
                reader
                    .cstr_at(offset, value_count)
                    .map(|s| TagValue::String(s.to_string()))
                    .unwrap_or_else(|| TagValue::String("(invalid)".to_string()))
            }
            5 => {
                // RATIONAL - unsigned
                reader
                    .rational_at(offset)
                    .map(|(num, denom)| {
                        if denom != 0 {
                            TagValue::Rational {
                                numerator: num as i32,
                                denominator: denom as i32,
                            }
                        } else {
                            TagValue::Integer(0)
                        }
                    })
                    .unwrap_or_else(|| TagValue::Integer(0))
            }
            10 => {
                // SRATIONAL - signed
                reader
                    .srational_at(offset)
                    .map(|(num, denom)| {
                        if denom != 0 {
                            TagValue::Rational {
                                numerator: num,
                                denominator: denom,
                            }
                        } else {
                            TagValue::Integer(0)
                        }
                    })
                    .unwrap_or_else(|| TagValue::Integer(0))
            }
            _ => {
                // Binary data
                reader
                    .bytes_at(offset, value_count)
                    .map(|bytes| TagValue::Binary(bytes.to_vec()))
                    .unwrap_or_else(|| TagValue::Binary(vec![]))
            }
        }
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a minimal valid MPF segment with "MPF\x00" identifier and TIFF header.
    fn create_minimal_mpf_segment() -> Vec<u8> {
        let mut data = Vec::new();

        // "MPF\x00" identifier
        data.extend_from_slice(b"MPF\0");

        // TIFF header (little-endian)
        data.extend_from_slice(b"II"); // Byte order mark
        data.extend_from_slice(&42u16.to_le_bytes()); // Magic number 42
        data.extend_from_slice(&8u32.to_le_bytes()); // IFD offset (after header)

        // MP Index IFD starts at offset 8 (from TIFF start)
        // IFD entry count: 2 entries
        data.extend_from_slice(&2u16.to_le_bytes());

        // Entry 1: MPFVersion (0xB000)
        data.extend_from_slice(&MPF_VERSION.to_le_bytes()); // Tag ID
        data.extend_from_slice(&2u16.to_le_bytes()); // Type: ASCII
        data.extend_from_slice(&4u32.to_le_bytes()); // Count
        data.extend_from_slice(b"0100"); // Value: "0100"

        // Entry 2: NumberOfImages (0xB001)
        data.extend_from_slice(&NUMBER_OF_IMAGES.to_le_bytes()); // Tag ID
        data.extend_from_slice(&4u16.to_le_bytes()); // Type: LONG
        data.extend_from_slice(&1u32.to_le_bytes()); // Count
        data.extend_from_slice(&2u32.to_le_bytes()); // Value: 2 images

        // Next IFD offset (0 = no more IFDs)
        data.extend_from_slice(&0u32.to_le_bytes());

        data
    }

    /// Creates an MPF segment with MP Entry data for testing.
    fn create_mpf_segment_with_entries() -> Vec<u8> {
        let mut data = Vec::new();

        // "MPF\x00" identifier
        data.extend_from_slice(b"MPF\0");

        // TIFF header (little-endian)
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes()); // IFD offset

        // MP Index IFD at offset 8
        // 3 entries
        data.extend_from_slice(&3u16.to_le_bytes());

        // Entry 1: MPFVersion
        data.extend_from_slice(&MPF_VERSION.to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes()); // ASCII
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(b"0100");

        // Entry 2: NumberOfImages
        data.extend_from_slice(&NUMBER_OF_IMAGES.to_le_bytes());
        data.extend_from_slice(&4u16.to_le_bytes()); // LONG
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes()); // 2 images

        // Entry 3: MPEntry (offset to entry array)
        let mp_entry_offset = 8 + 2 + (3 * 12) + 4; // After IFD header + entries + next IFD ptr
        data.extend_from_slice(&MP_ENTRY.to_le_bytes());
        data.extend_from_slice(&7u16.to_le_bytes()); // UNDEFINED
        data.extend_from_slice(&32u32.to_le_bytes()); // 32 bytes (2 entries)
        data.extend_from_slice(&(mp_entry_offset as u32).to_le_bytes());

        // Next IFD offset (0 = no more IFDs)
        data.extend_from_slice(&0u32.to_le_bytes());

        // MP Entry array (2 entries of 16 bytes each)
        // Entry 1: Primary image (representative)
        let attr1: u32 = 0x20000000 | 0x030000; // Representative + Baseline MP Primary
        data.extend_from_slice(&attr1.to_le_bytes()); // Image attribute
        data.extend_from_slice(&100000u32.to_le_bytes()); // Size
        data.extend_from_slice(&0u32.to_le_bytes()); // Offset (0 for first image)
        data.extend_from_slice(&0u16.to_le_bytes()); // Dependent image 1
        data.extend_from_slice(&0u16.to_le_bytes()); // Dependent image 2

        // Entry 2: Thumbnail
        let attr2: u32 = 0x010001; // Large Thumbnail (class 1)
        data.extend_from_slice(&attr2.to_le_bytes());
        data.extend_from_slice(&50000u32.to_le_bytes());
        data.extend_from_slice(&100000u32.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());

        data
    }

    /// Same minimal segment as [`create_minimal_mpf_segment`] but with the
    /// big-endian ("MM") TIFF header every real MPF segment actually uses
    /// (verified on Apple iPhone 11/13/14/15 samples). The all-little-endian
    /// fixture above could never catch a byte-order bug in the version field:
    /// a `u32` read little-endian and re-serialized with `to_le_bytes()`
    /// round-trips exactly, so the reversal only appeared under "MM".
    fn create_minimal_mpf_segment_big_endian() -> Vec<u8> {
        let mut data = Vec::new();

        data.extend_from_slice(b"MPF\0");

        data.extend_from_slice(b"MM"); // Big-endian byte order mark
        data.extend_from_slice(&42u16.to_be_bytes());
        data.extend_from_slice(&8u32.to_be_bytes()); // IFD offset

        // MP Index IFD at offset 8: 2 entries
        data.extend_from_slice(&2u16.to_be_bytes());

        // Entry 1: MPFVersion (0xB000), UNDEFINED[4], inline "0100"
        data.extend_from_slice(&MPF_VERSION.to_be_bytes());
        data.extend_from_slice(&7u16.to_be_bytes()); // Type: UNDEFINED
        data.extend_from_slice(&4u32.to_be_bytes()); // Count
        data.extend_from_slice(b"0100"); // Value: "0100"

        // Entry 2: NumberOfImages (0xB001), LONG
        data.extend_from_slice(&NUMBER_OF_IMAGES.to_be_bytes());
        data.extend_from_slice(&4u16.to_be_bytes());
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&2u32.to_be_bytes());

        // Next IFD offset (0 = no more IFDs)
        data.extend_from_slice(&0u32.to_be_bytes());

        data
    }

    /// `exiftool -G1 -s` on every MPF-bearing sample in the corpus prints
    /// `[MPF0] MPFVersion : 0100`. MPF.pm:34 declares the tag as a bare
    /// `0xb000 => 'MPFVersion'` -- no Format, no PrintConv -- so the four
    /// UNDEFINED bytes print verbatim and must not be byte-swapped.
    #[test]
    fn test_mpf_version_not_byte_swapped_in_big_endian_segment() {
        let data = create_minimal_mpf_segment_big_endian();
        let mut metadata = MetadataMap::new();

        let result = parse_mpf_segment(&data, 0, &mut metadata);
        assert!(result.is_ok(), "Failed to parse: {:?}", result);

        assert_eq!(
            metadata.get_string("MPF0:MPFVersion"),
            Some("0100"),
            "MPFVersion must print the raw ASCII bytes; '0010' is the \
             reversal that comes from decoding them as an integer"
        );
    }

    #[test]
    fn test_parse_minimal_mpf_segment() {
        let data = create_minimal_mpf_segment();
        let mut metadata = MetadataMap::new();

        let result = parse_mpf_segment(&data, 0, &mut metadata);
        assert!(result.is_ok(), "Failed to parse: {:?}", result);

        // Check MPFVersion - should be raw "0100" format for ExifTool compatibility
        assert!(
            metadata.contains_key("MPF0:MPFVersion"),
            "Missing MPFVersion"
        );
        assert_eq!(
            metadata.get_string("MPF0:MPFVersion"),
            Some("0100"),
            "MPFVersion should be raw '0100' format"
        );

        // Check NumberOfImages
        assert_eq!(
            metadata.get_integer("MPF0:NumberOfImages"),
            Some(2),
            "Wrong NumberOfImages"
        );
    }

    /// The seven MP Entry tag names repeat once per image under an indexed
    /// family-1 group; the name itself never carries the index. Ground truth,
    /// `exiftool -a -G1 -s Apple/Apple_iPhone16Pro.jpg`:
    ///
    /// ```text
    /// [MPImage1]      MPImageType                 : Baseline MP Primary Image
    /// [MPImage1]      MPImageStart                : 0
    /// [MPImage2]      MPImageType                 : Undefined
    /// [MPImage2]      MPImageStart                : 4289837
    /// ```
    ///
    /// The old flat spelling (`MPF:MPImage2Offset`, plus one collapsed
    /// `MPF:MPImageStart` for whichever entry happened to be scanned last)
    /// exists nowhere in ExifTool and matched nothing.
    #[test]
    fn test_parse_mpf_segment_with_entries() {
        let data = create_mpf_segment_with_entries();
        let mut metadata = MetadataMap::new();

        // tiff_base 6269 is Apple_iPhone12ProMax.jpg's real MPF header offset.
        let result = parse_mpf_segment(&data, 6269, &mut metadata);
        assert!(result.is_ok(), "Failed to parse: {:?}", result);

        // Per-image groups, one repeated name per group.
        assert_eq!(
            metadata.get_string("MPImage1:MPImageType"),
            Some("Baseline MP Primary Image"),
            "entry 1 type"
        );
        assert_eq!(
            metadata.get_string("MPImage2:MPImageType"),
            Some("Large Thumbnail (VGA equivalent)"),
            "entry 2 type"
        );
        assert_eq!(
            metadata.get_integer("MPImage1:MPImageLength"),
            Some(100000),
            "entry 1 length"
        );
        assert_eq!(
            metadata.get_integer("MPImage2:MPImageLength"),
            Some(50000),
            "entry 2 length"
        );

        // MPF.pm:148 `IsOffset => '$val'`: a raw 0 stays 0 (the eval is false),
        // a nonzero raw offset is rebased onto the MPF TIFF header.
        assert_eq!(
            metadata.get_integer("MPImage1:MPImageStart"),
            Some(0),
            "the primary image's stored offset of 0 must not be rebased"
        );
        assert_eq!(
            metadata.get_integer("MPImage2:MPImageStart"),
            Some(100000 + 6269),
            "nonzero MPImageStart must be rebased onto the MPF TIFF header"
        );

        // Entry 1 sets the representative bit (0x20000000).
        assert_eq!(
            metadata.get_string("MPImage1:MPImageFlags"),
            Some("Representative image"),
            "entry 1 flags"
        );
        assert_eq!(
            metadata.get_string("MPImage2:MPImageFlags"),
            Some("(none)"),
            "entry 2 flags"
        );
        assert_eq!(
            metadata.get_string("MPImage1:MPImageFormat"),
            Some("JPEG"),
            "entry 1 format"
        );

        // Both dependent-entry tags are always emitted, including the zeros.
        assert_eq!(
            metadata.get_integer("MPImage2:DependentImage1EntryNumber"),
            Some(0)
        );
        assert_eq!(
            metadata.get_integer("MPImage2:DependentImage2EntryNumber"),
            Some(0)
        );

        // MPF.pm:190-233: the embedded image is itself a tag, in the same
        // group as its MPImageStart, and the first Large Thumbnail takes the
        // name PreviewImage. Entry 1 has a stored offset of 0 so it produces
        // none.
        assert_eq!(
            metadata.get_string("MPImage2:PreviewImage"),
            Some("(Binary data 50000 bytes, use -b option to extract)"),
            "entry 2 is a Large Thumbnail, so its image tag is PreviewImage"
        );
        assert!(
            !metadata.contains_key("MPImage1:MPImage1"),
            "an MP entry stored at offset 0 has no image tag (MPF.pm:202)"
        );

        // The invented flat names are gone.
        for gone in [
            "MPF:MPImage1Type",
            "MPF:MPImage2Type",
            "MPF:MPImage1Size",
            "MPF:MPImage2Offset",
            "MPF:MPImageStart",
            "MPF:MPImageFlags",
        ] {
            assert!(
                !metadata.contains_key(gone),
                "{} is not an ExifTool key and must not be emitted",
                gone
            );
        }
    }

    /// MPF.pm:107-111 is a `BITMASK`, so several flags can be set at once and
    /// ExifTool joins them with `", "` in ascending bit order
    /// (ExifTool.pm:6362-6383). Ground truth,
    /// `exiftool -a -G1 -s Leica/LeicaM11.jpg`:
    ///
    /// ```text
    /// [MPImage1] MPImageFlags : Representative image, Dependent parent image
    /// [MPImage2] MPImageFlags : Dependent child image
    /// ```
    ///
    /// The previous decoder read bits 31-30 as a 2-bit enum and inverted the
    /// parent/child sense, printing `Dependent parent image` where ExifTool
    /// prints `(none)` on 663 corpus files.
    #[test]
    fn test_decode_image_flags_bitmask() {
        // 0x20000000 = bit 2 after the >>27 shift.
        assert_eq!(decode_image_flags(0x2000_0000), "Representative image");
        // 0x40000000 = bit 3 => child, NOT parent.
        assert_eq!(decode_image_flags(0x4000_0000), "Dependent child image");
        // 0x80000000 = bit 4 => parent.
        assert_eq!(decode_image_flags(0x8000_0000), "Dependent parent image");
        assert_eq!(
            decode_image_flags(0xA000_0000),
            "Representative image, Dependent parent image"
        );
        assert_eq!(decode_image_flags(0x0003_0000), "(none)");
        // Bits 0 and 1 of the masked field have no label in MPF.pm; ExifTool
        // renders an unlisted bit as "[n]".
        assert_eq!(decode_image_flags(0x0800_0000), "[0]");
    }

    /// Type and format come from the generated `MPF::MPImage` table, so the
    /// enum bodies are ExifTool's. Unknown values follow ExifTool.pm:3604-3611:
    /// hex for `MPImageType` (`PrintHex => 1`, MPF.pm:125), decimal for
    /// `MPImageFormat`.
    #[test]
    fn test_decode_image_type_and_format() {
        assert_eq!(decode_image_type(0x0003_0000), "Baseline MP Primary Image");
        assert_eq!(
            decode_image_type(0x0001_0001),
            "Large Thumbnail (VGA equivalent)"
        );
        assert_eq!(
            decode_image_type(0x0001_0002),
            "Large Thumbnail (full HD equivalent)"
        );
        // Lower-case "frame"/"angle" -- MPF.pm:133-135.
        assert_eq!(decode_image_type(0x0002_0001), "Multi-frame Panorama");
        assert_eq!(decode_image_type(0x0002_0003), "Multi-angle");
        assert_eq!(decode_image_type(0x0000_0000), "Undefined");
        assert_eq!(decode_image_type(0x0005_0000), "Gain Map Image");
        // The high flag/format bits must be masked off before the lookup.
        assert_eq!(decode_image_type(0xE003_0000), "Baseline MP Primary Image");
        assert_eq!(decode_image_type(0x00AB_CDEF), "Unknown (0xabcdef)");

        assert_eq!(decode_image_format(0x0000_0000), "JPEG");
        assert_eq!(decode_image_format(0xE003_0000), "JPEG");
        assert_eq!(decode_image_format(0x0100_0000), "Unknown (1)");
    }

    #[test]
    fn test_parse_invalid_identifier() {
        let data = b"NOT_MPF\0II*\0\x08\0\0\0";
        let mut metadata = MetadataMap::new();

        let result = parse_mpf_segment(data, 0, &mut metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Not an MPF segment"));
    }

    #[test]
    fn test_parse_too_short() {
        let data = b"MPF\0II*";
        let mut metadata = MetadataMap::new();

        let result = parse_mpf_segment(data, 0, &mut metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too short"));
    }

    #[test]
    fn test_parse_invalid_byte_order() {
        let mut data = create_minimal_mpf_segment();
        // Corrupt byte order marker
        data[4] = b'X';
        data[5] = b'X';

        let mut metadata = MetadataMap::new();
        let result = parse_mpf_segment(&data, 0, &mut metadata);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("byte order"));
    }

    #[test]
    fn test_parse_big_endian_mpf() {
        let mut data = Vec::new();

        // "MPF\x00" identifier
        data.extend_from_slice(b"MPF\0");

        // TIFF header (big-endian)
        data.extend_from_slice(b"MM");
        data.extend_from_slice(&42u16.to_be_bytes());
        data.extend_from_slice(&8u32.to_be_bytes());

        // IFD entry count: 1
        data.extend_from_slice(&1u16.to_be_bytes());

        // Entry: NumberOfImages
        data.extend_from_slice(&NUMBER_OF_IMAGES.to_be_bytes());
        data.extend_from_slice(&4u16.to_be_bytes()); // LONG
        data.extend_from_slice(&1u32.to_be_bytes());
        data.extend_from_slice(&3u32.to_be_bytes()); // 3 images

        // Next IFD offset
        data.extend_from_slice(&0u32.to_be_bytes());

        let mut metadata = MetadataMap::new();
        let result = parse_mpf_segment(&data, 0, &mut metadata);
        assert!(result.is_ok());

        assert_eq!(metadata.get_integer("MPF0:NumberOfImages"), Some(3));
    }

    #[test]
    fn test_detect_byte_order() {
        assert_eq!(
            detect_byte_order(b"II").unwrap(),
            MpfByteOrder::LittleEndian
        );
        assert_eq!(detect_byte_order(b"MM").unwrap(), MpfByteOrder::BigEndian);
        assert!(detect_byte_order(b"XX").is_err());
    }
}
