//! Adobe Photoshop (PSD) format parser
//!
//! PSD file structure:
//! - Header (26 bytes): signature, version, reserved, channels, height, width, depth, color mode
//! - Color Mode Data section
//! - Image Resources section (contains EXIF, IPTC, XMP, etc.)
//! - Layer and Mask Information section
//! - Image Data section

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::buffered_reader::BufferedReader;
use crate::io::{ByteOrder as EndianByteOrder, EndianReader};
use crate::parsers::icc::parse_icc_profile_data;
use crate::parsers::jpeg::iptc_parser::{
    dataset_to_tag_name, decode_iptc_string, parse_all_iptc_records,
};
use crate::parsers::tiff::ifd_parser::{ByteOrder, parse_ifd};
use crate::parsers::xmp::rdf_parser::parse_xmp;
use crate::tag_db::lookup_tag_name;

const PSD_SIGNATURE: &[u8] = b"8BPS";

/// Image resource IDs -- keys of `%Image::ExifTool::Photoshop::Main`.
const IPTC_NAA_RECORD: u16 = 0x0404; // IPTC-NAA record
const EXIF_DATA_1: u16 = 0x0422; // EXIF data 1
const EXIF_DATA_3: u16 = 0x0423; // EXIF data 3
const XMP_DATA: u16 = 0x0424; // XMP metadata
const ICC_PROFILE: u16 = 0x040F; // ICC profile
const RESOLUTION_INFO: u16 = 0x03ED; // Resolution info
const PRINT_FLAGS: u16 = 0x03F1; // Print flags
const COPYRIGHT_FLAG: u16 = 0x040A; // Copyright flag
const URL_RESOURCE: u16 = 0x040B; // URL
const GLOBAL_ANGLE: u16 = 0x040D; // Global lighting angle
const GLOBAL_ALTITUDE: u16 = 0x0419; // Global altitude
const SLICE_INFO: u16 = 0x041A; // Slices
const URL_LIST: u16 = 0x041E; // URL list
const VERSION_INFO: u16 = 0x0421; // Version info
const IPTC_DIGEST: u16 = 0x0425; // IPTC digest
const PRINT_SCALE_INFO: u16 = 0x0426; // Print scale info

/// Footer that marks the end of a Photo Mechanic trailer.
///
/// `PhotoMechanic.pm::ProcessPhotoMechanic` validates the last twelve bytes
/// of the file against `/cbipcbbl$/` and reads the trailer length from the
/// four bytes in front of that magic.
const PHOTO_MECHANIC_FOOTER: &[u8] = b"cbipcbbl";

/// `PrintConv` of DisplayedUnitsX / DisplayedUnitsY
/// (`%Image::ExifTool::Photoshop::Resolution`).
const DISPLAYED_UNITS: &[(i64, &str)] = &[(1, "inches"), (2, "cm")];

/// `PrintConv` of PrintStyle (`%Image::ExifTool::Photoshop::PrintScaleInfo`).
const PRINT_STYLES: &[(i64, &str)] = &[(0, "Centered"), (1, "Size to Fit"), (2, "User Defined")];

/// `PrintConv` of Compression (`%Image::ExifTool::Photoshop::ImageData`).
const PSD_COMPRESSION: &[(i64, &str)] = &[
    (0, "Uncompressed"),
    (1, "RLE"),
    (2, "ZIP without prediction"),
    (3, "ZIP with prediction"),
];

/// `PrintConv` of PhotoMechanic Rotation
/// (`%Image::ExifTool::PhotoMechanic::SoftEdit`, dataset 216).
const PHOTO_MECHANIC_ROTATION: &[(i64, &str)] = &[(0, "0"), (1, "90"), (2, "180"), (3, "270")];

/// `%colorClasses` from `PhotoMechanic.pm`, shared by the IPTC-trailer and
/// XMP flavours of ColorClass.
const PHOTO_MECHANIC_COLOR_CLASSES: &[(i64, &str)] = &[
    (0, "0 (None)"),
    (1, "1 (Winner)"),
    (2, "2 (Winner alt)"),
    (3, "3 (Superior)"),
    (4, "4 (Superior alt)"),
    (5, "5 (Typical)"),
    (6, "6 (Typical alt)"),
    (7, "7 (Extras)"),
    (8, "8 (Trash)"),
];

/// Parser for Adobe Photoshop (PSD) document files
///
/// Extracts metadata from PSD files including dimensions, color mode, channels,
/// bit depth, and embedded EXIF/IPTC/XMP data.
pub struct PSDParser;
/// Formats a PSD resolution the way ExifTool prints it.
///
/// The header stores a fixed-point value that is almost always whole, and
/// ExifTool prints it without a fractional part -- "72", not "72.00". A
/// hard-coded two decimals turned a correct number into a value mismatch.
fn format_psd_resolution(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

/// Stringifies a number the way Perl -- and therefore ExifTool -- does.
///
/// ExifTool reads `Format => 'float'` with `unpack('f')`, which widens the
/// 32-bit value into a Perl NV (a C double), and every print path then
/// stringifies that NV with `%.15g`. So PrintScale 1.0 comes out of
/// `exiftool -j` as `1`, not `1.0` and not `1.00000`, and a float that is
/// not exactly representable prints fifteen significant digits rather than
/// Rust's shortest round-tripping form (`0.100000001490116`, not `0.1`).
/// Both distinctions are value mismatches against the comparison harness.
fn format_perl_number(value: f64) -> String {
    if !value.is_finite() {
        return format!("{value}");
    }
    if value == 0.0 {
        // Covers -0.0 as well: Perl prints both as "0".
        return "0".to_string();
    }

    let exponent = value.abs().log10().floor() as i32;
    // %g uses fixed notation while -4 <= exponent < precision (15 here), and
    // switches to scientific notation outside that window. Scientific output
    // is left to Rust's shortest round-tripping form; no Photoshop resource
    // in the corpus reaches those magnitudes.
    if !(-4..15).contains(&exponent) {
        return format!("{value}");
    }

    let decimals = (14 - exponent).clamp(0, 17) as usize;
    let rendered = format!("{value:.decimals$}");
    if rendered.contains('.') {
        rendered
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        rendered
    }
}

/// Renders an enum code, falling back to ExifTool's `Unknown (N)` form.
///
/// An unrecognised code must report itself rather than being rounded to a
/// neighbouring label -- that is how a fabricated value gets mistaken for a
/// real one. The code is signed because the Photo Mechanic SoftEdit table is
/// `FORMAT => 'int32s'`: a stray -1 has to print as `Unknown (-1)`, not as
/// the 4294967295 an unsigned cast would produce.
fn print_conv(value: i64, table: &[(i64, &str)]) -> String {
    table
        .iter()
        .find(|(code, _)| *code == value)
        .map(|(_, label)| (*label).to_string())
        .unwrap_or_else(|| format!("Unknown ({value})"))
}

/// Reads a Photoshop `var_ustr32` string: a four-byte big-endian count of
/// UTF-16 code units, followed by that many big-endian code units.
///
/// Returns the decoded string plus the number of bytes the *characters*
/// occupy. ExifTool tracks exactly that quantity in `$varSize`
/// (ExifTool.pm: `$count *= 2 if $format eq 'ustr32'` then
/// `$varSize += $count`), shifting the byte offset of every later entry in
/// the table while the four-byte count word is absorbed by the entry's own
/// index. It also truncates the decoded value at the first NUL
/// (`$val =~ s/\0.*//s`).
fn read_var_ustr32(data: &[u8], offset: usize) -> Option<(String, usize)> {
    let reader = EndianReader::big_endian(data);
    let char_count = reader.u32_at(offset)? as usize;
    let byte_len = char_count.checked_mul(2)?;
    let start = offset.checked_add(4)?;
    let bytes = reader.bytes_at(start, byte_len)?;

    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    let decoded = String::from_utf16_lossy(&units);
    let truncated = match decoded.find('\0') {
        Some(nul) => decoded[..nul].to_string(),
        None => decoded,
    };

    Some((truncated, byte_len))
}

impl PSDParser {
    /// Verifies the PSD file signature ("8BPS")
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 4 {
            return Ok(false);
        }
        let header = reader.read(0, 4)?;
        Ok(header == PSD_SIGNATURE)
    }

    /// Reads the PSD file version number (1 for PSD, 2 for PSB)
    pub fn read_version(reader: &dyn FileReader) -> Result<u16> {
        if reader.size() < 6 {
            return Ok(0);
        }
        let version_bytes = reader.read(4, 2)?;
        // PSD uses big-endian byte order
        let version_reader = EndianReader::big_endian(version_bytes);
        Ok(version_reader.u16_at(0).unwrap_or(0))
    }

    /// Parse the PSD header (26 bytes)
    fn parse_header(reader: &dyn FileReader, metadata: &mut MetadataMap) -> Result<()> {
        if reader.size() < 26 {
            return Ok(());
        }

        let header = reader.read(0, 26)?;
        // PSD uses big-endian byte order
        let header_reader = EndianReader::big_endian(header);

        // Version (offset 4, 2 bytes)
        let version = header_reader.u16_at(4).unwrap_or(1);
        let format_name = if version == 1 { "PSD" } else { "PSB" };
        metadata.insert(
            "FileType".to_string(),
            TagValue::String(format_name.to_string()),
        );
        metadata.insert("PSDVersion".to_string(), TagValue::Integer(version as i64));

        // Channels (offset 12, 2 bytes)
        let channels = header_reader.u16_at(12).unwrap_or(0);
        metadata.insert(
            "Photoshop:NumChannels".to_string(),
            TagValue::Integer(channels as i64),
        );

        // Height (offset 14, 4 bytes)
        let height = header_reader.u32_at(14).unwrap_or(0);
        metadata.insert(
            "Photoshop:ImageHeight".to_string(),
            TagValue::Integer(height as i64),
        );

        // Width (offset 18, 4 bytes)
        let width = header_reader.u32_at(18).unwrap_or(0);
        metadata.insert(
            "Photoshop:ImageWidth".to_string(),
            TagValue::Integer(width as i64),
        );

        // Bit Depth (offset 22, 2 bytes)
        let depth = header_reader.u16_at(22).unwrap_or(0);
        metadata.insert(
            "Photoshop:BitDepth".to_string(),
            TagValue::Integer(depth as i64),
        );

        // Color Mode (offset 24, 2 bytes)
        let color_mode = header_reader.u16_at(24).unwrap_or(0);
        let color_mode_name = match color_mode {
            0 => "Bitmap",
            1 => "Grayscale",
            2 => "Indexed",
            3 => "RGB",
            4 => "CMYK",
            7 => "Multichannel",
            8 => "Duotone",
            9 => "Lab",
            _ => "Unknown",
        };
        metadata.insert(
            "Photoshop:ColorMode".to_string(),
            TagValue::String(color_mode_name.to_string()),
        );

        Ok(())
    }

    /// Parse Image Resources section.
    ///
    /// Returns the file offset one byte past the end of the section, which is
    /// where the Layer and Mask Information section begins. `None` means the
    /// section could not be located and no later section can be trusted.
    fn parse_image_resources(
        reader: &dyn FileReader,
        metadata: &mut MetadataMap,
    ) -> Result<Option<u64>> {
        if reader.size() < 34 {
            return Ok(None);
        }

        // Color mode data length at offset 26
        let cmd_len_bytes = reader.read(26, 4)?;
        // PSD uses big-endian byte order
        let cmd_len_reader = EndianReader::big_endian(cmd_len_bytes);
        let color_mode_data_length = cmd_len_reader.u32_at(0).unwrap_or(0);

        // Image resources section starts after color mode data
        let resources_offset = 30 + color_mode_data_length as usize;

        if reader.size() < (resources_offset + 4) as u64 {
            return Ok(None);
        }

        // Image resources length
        let irl_bytes = reader.read(resources_offset as u64, 4)?;
        let irl_reader = EndianReader::big_endian(irl_bytes);
        let resources_length = irl_reader.u32_at(0).unwrap_or(0) as usize;

        let resources_end = (resources_offset + 4 + resources_length) as u64;
        if reader.size() < resources_end {
            return Ok(None);
        }
        // An empty Image Resources section is legal, and the sections after it
        // are still there: report where they start instead of giving up.
        if resources_length == 0 {
            return Ok(Some(resources_end));
        }

        // Read entire resources section
        let resources_data = reader.read((resources_offset + 4) as u64, resources_length)?;

        // Parse individual resources
        let mut pos = 0;
        while pos + 12 <= resources_data.len() {
            // Resource signature "8BIM"
            if &resources_data[pos..pos + 4] != b"8BIM" {
                break;
            }
            pos += 4;

            // Resource ID (2 bytes)
            let res_reader = EndianReader::big_endian(&resources_data[pos..]);
            let resource_id = res_reader.u16_at(0).unwrap_or(0);
            pos += 2;

            // Pascal string name (padded to even)
            let name_len = resources_data[pos] as usize;
            let padded_name_len = if (name_len + 1).is_multiple_of(2) {
                name_len + 1
            } else {
                name_len + 2
            };
            pos += padded_name_len;

            if pos + 4 > resources_data.len() {
                break;
            }

            // Resource data size (4 bytes)
            let size_reader = EndianReader::big_endian(&resources_data[pos..]);
            let data_size = size_reader.u32_at(0).unwrap_or(0) as usize;
            pos += 4;

            if pos + data_size > resources_data.len() {
                break;
            }

            let resource_data = &resources_data[pos..pos + data_size];

            // Process specific resources
            match resource_id {
                RESOLUTION_INFO => {
                    Self::parse_resolution_info(resource_data, metadata);
                }
                EXIF_DATA_1 | EXIF_DATA_3 => {
                    Self::parse_exif_data(resource_data, metadata);
                }
                COPYRIGHT_FLAG => {
                    if !resource_data.is_empty() && resource_data[0] != 0 {
                        metadata.insert(
                            "Copyrighted".to_string(),
                            TagValue::String("Yes".to_string()),
                        );
                    }
                    Self::parse_copyright_flag(resource_data, metadata);
                }
                URL_RESOURCE => {
                    Self::parse_url(resource_data, metadata);
                }
                URL_LIST => {
                    Self::parse_url_list(resource_data, metadata);
                }
                GLOBAL_ANGLE => {
                    Self::parse_u32_resource(resource_data, "Photoshop:GlobalAngle", metadata);
                }
                GLOBAL_ALTITUDE => {
                    Self::parse_u32_resource(resource_data, "Photoshop:GlobalAltitude", metadata);
                }
                SLICE_INFO => {
                    Self::parse_slice_info(resource_data, metadata);
                }
                VERSION_INFO => {
                    Self::parse_version_info(resource_data, metadata);
                }
                IPTC_DIGEST => {
                    Self::parse_iptc_digest(resource_data, metadata);
                }
                PRINT_SCALE_INFO => {
                    Self::parse_print_scale_info(resource_data, metadata);
                }
                XMP_DATA => {
                    if let Ok(xmp_str) = std::str::from_utf8(resource_data) {
                        Self::parse_xmp_data(xmp_str, metadata);
                    }
                }
                ICC_PROFILE => {
                    metadata.insert(
                        "HasICCProfile".to_string(),
                        TagValue::String("Yes".to_string()),
                    );
                    // Parse ICC profile data
                    if let Ok(icc_tags) = parse_icc_profile_data(resource_data) {
                        for (key, value) in icc_tags {
                            metadata.insert(format!("ICC_Profile:{}", key), value);
                        }
                    }
                }
                IPTC_NAA_RECORD => {
                    Self::parse_iptc_data(resource_data, metadata);
                }
                _ => {}
            }

            // Pad to even boundary
            let padded_size = if data_size.is_multiple_of(2) {
                data_size
            } else {
                data_size + 1
            };
            pos += padded_size;
        }

        Ok(Some(resources_end))
    }

    /// Parse resolution info resource
    fn parse_resolution_info(data: &[u8], metadata: &mut MetadataMap) {
        if data.len() < 16 {
            return;
        }

        // PSD uses big-endian byte order
        let res_reader = EndianReader::big_endian(data);

        // Horizontal resolution (fixed point 16.16)
        let h_res_fixed = res_reader.u32_at(0).unwrap_or(0);
        let h_res = h_res_fixed as f64 / 65536.0;

        // Resolution unit (offset 4, 2 bytes): 1=pixels/inch, 2=pixels/cm
        let res_unit = res_reader.u16_at(4).unwrap_or(1);
        let unit_name = if res_unit == 1 { "inch" } else { "cm" };

        // Vertical resolution (offset 8, fixed point 16.16)
        let v_res_fixed = res_reader.u32_at(8).unwrap_or(0);
        let v_res = v_res_fixed as f64 / 65536.0;

        metadata.insert(
            "Photoshop:XResolution".to_string(),
            TagValue::String(format_psd_resolution(h_res)),
        );
        metadata.insert(
            "Photoshop:YResolution".to_string(),
            TagValue::String(format_psd_resolution(v_res)),
        );
        metadata.insert(
            "ResolutionUnit".to_string(),
            TagValue::String(unit_name.to_string()),
        );

        // `%Image::ExifTool::Photoshop::Resolution` has `FORMAT => 'int16u'`,
        // so its numeric keys are INDICES of two bytes, not byte offsets:
        // index 2 is byte 4 and index 6 is byte 12. Those are the same two
        // fields the block above reads, but ExifTool names them
        // DisplayedUnitsX / DisplayedUnitsY and prints "inches"/"cm" -- it has
        // no "ResolutionUnit" tag in this table at all, so the key above stays
        // as an oxidex-only extra rather than standing in for these.
        for (offset, tag) in [
            (4usize, "Photoshop:DisplayedUnitsX"),
            (12usize, "Photoshop:DisplayedUnitsY"),
        ] {
            if let Some(code) = res_reader.u16_at(offset) {
                metadata.insert(
                    tag.to_string(),
                    TagValue::String(print_conv(i64::from(code), DISPLAYED_UNITS)),
                );
            }
        }
    }

    /// Parse CopyrightFlag (resource 0x040A).
    ///
    /// `Photoshop.pm` gives this a `PrintConv` of `{0 => 'False', 1 => 'True'}`,
    /// but those two labels never survive serialisation intact: the exiftool
    /// script's `EscapeJSON` rewrites them as JSON booleans
    /// (`return lc($str) if $str =~ /^(true|false)$/i and $json < 2;`). So
    /// `exiftool -s3 -Photoshop:CopyrightFlag` prints `False` while
    /// `exiftool -j` -- the ground truth the tag-comparison harness reads --
    /// emits `false`, and lower case is what has to match. Any other code
    /// keeps ExifTool's `Unknown (N)` form.
    fn parse_copyright_flag(data: &[u8], metadata: &mut MetadataMap) {
        let Some(&flag) = data.first() else {
            return;
        };
        metadata.insert(
            "Photoshop:CopyrightFlag".to_string(),
            TagValue::String(print_conv(i64::from(flag), &[(0, "false"), (1, "true")])),
        );
    }

    /// Parse the URL resource (0x040B), a plain `Writable => 'string'`.
    ///
    /// The trailing padding is left alone: Photoshop.psd stores the URL with
    /// twenty trailing spaces and `exiftool -j` reports them verbatim. Only
    /// NUL terminators are trimmed. A resource that decodes to nothing is
    /// skipped rather than emitted as an empty tag.
    fn parse_url(data: &[u8], metadata: &mut MetadataMap) {
        let text = String::from_utf8_lossy(data);
        let text = text.trim_end_matches('\0');
        if text.is_empty() {
            return;
        }
        metadata.insert(
            "Photoshop:URL".to_string(),
            TagValue::String(text.to_string()),
        );
    }

    /// Parse URL_List (resource 0x041E).
    ///
    /// Layout per `Photoshop.pm`'s `ValueConv`: a four-byte count, then per
    /// entry an eight-byte header that is skipped (a word plus an ID) followed
    /// by a `var_ustr32` string. ExifTool emits the tag even when the count is
    /// zero -- `exiftool -j` reports `"Photoshop:URL_List": []`.
    fn parse_url_list(data: &[u8], metadata: &mut MetadataMap) {
        let reader = EndianReader::big_endian(data);
        let Some(count) = reader.u32_at(0) else {
            return;
        };

        let mut urls = Vec::new();
        let mut pos = 4usize;
        for _ in 0..count {
            // Skip the word and ID that precede each string.
            pos += 8;
            match read_var_ustr32(data, pos) {
                Some((url, char_bytes)) => {
                    urls.push(TagValue::String(url));
                    pos += 4 + char_bytes;
                }
                None => break,
            }
        }

        metadata.insert("Photoshop:URL_List".to_string(), TagValue::Array(urls));
    }

    /// Parse a resource whose whole payload is one big-endian `int32u`.
    ///
    /// Both GlobalAngle (0x040D) and GlobalAltitude (0x0419) are declared this
    /// way, with `ValueConv => 'unpack("N",$val)'`.
    fn parse_u32_resource(data: &[u8], tag: &str, metadata: &mut MetadataMap) {
        let reader = EndianReader::big_endian(data);
        if let Some(value) = reader.u32_at(0) {
            metadata.insert(tag.to_string(), TagValue::Integer(value as i64));
        }
    }

    /// Parse SliceInfo (resource 0x041A).
    ///
    /// `%Image::ExifTool::Photoshop::SliceInfo` has no `FORMAT` override, so
    /// the default `int8u` applies and its keys are byte offsets:
    /// SlicesGroupName at 20 (`var_ustr32`) and NumSlices at 24. The string's
    /// characters shift NumSlices later by exactly their byte length.
    fn parse_slice_info(data: &[u8], metadata: &mut MetadataMap) {
        let Some((group_name, char_bytes)) = read_var_ustr32(data, 20) else {
            return;
        };
        metadata.insert(
            "Photoshop:SlicesGroupName".to_string(),
            TagValue::String(group_name),
        );

        let reader = EndianReader::big_endian(data);
        if let Some(num_slices) = reader.u32_at(24 + char_bytes) {
            metadata.insert(
                "Photoshop:NumSlices".to_string(),
                TagValue::Integer(num_slices as i64),
            );
        }
    }

    /// Parse VersionInfo (resource 0x0421).
    ///
    /// `%Image::ExifTool::Photoshop::VersionInfo` also defaults to `int8u`
    /// keys, i.e. byte offsets: HasRealMergedData at 4, WriterName at 5 and
    /// ReaderName at 9, the last two both `var_ustr32`. WriterName's
    /// characters push ReaderName past its nominal offset.
    fn parse_version_info(data: &[u8], metadata: &mut MetadataMap) {
        let reader = EndianReader::big_endian(data);
        if let Some(flag) = reader.u8_at(4) {
            metadata.insert(
                "Photoshop:HasRealMergedData".to_string(),
                TagValue::String(print_conv(i64::from(flag), &[(0, "No"), (1, "Yes")])),
            );
        }

        let Some((writer, writer_bytes)) = read_var_ustr32(data, 5) else {
            return;
        };
        metadata.insert("Photoshop:WriterName".to_string(), TagValue::String(writer));

        if let Some((reader_name, _)) = read_var_ustr32(data, 9 + writer_bytes) {
            metadata.insert(
                "Photoshop:ReaderName".to_string(),
                TagValue::String(reader_name),
            );
        }
    }

    /// Parse IPTCDigest (resource 0x0425): `ValueConv => 'unpack("H*", $val)'`,
    /// i.e. the raw MD5 bytes rendered as lower-case hex.
    fn parse_iptc_digest(data: &[u8], metadata: &mut MetadataMap) {
        if data.is_empty() {
            return;
        }
        let mut digest = String::with_capacity(data.len() * 2);
        for byte in data {
            use std::fmt::Write;
            let _ = write!(digest, "{byte:02x}");
        }
        metadata.insert("Photoshop:IPTCDigest".to_string(), TagValue::String(digest));
    }

    /// Parse PrintScaleInfo (resource 0x0426).
    ///
    /// `%Image::ExifTool::Photoshop::PrintScaleInfo` defaults to `int8u` keys,
    /// so its numbers are byte offsets: PrintStyle `int16u` at 0,
    /// PrintPosition `float[2]` at 2 and PrintScale `float` at 10.
    fn parse_print_scale_info(data: &[u8], metadata: &mut MetadataMap) {
        let reader = EndianReader::big_endian(data);

        if let Some(style) = reader.u16_at(0) {
            metadata.insert(
                "Photoshop:PrintStyle".to_string(),
                TagValue::String(print_conv(i64::from(style), PRINT_STYLES)),
            );
        }

        if let (Some(x), Some(y)) = (reader.f32_at(2), reader.f32_at(6)) {
            metadata.insert(
                "Photoshop:PrintPosition".to_string(),
                TagValue::String(format!(
                    "{} {}",
                    format_perl_number(x as f64),
                    format_perl_number(y as f64)
                )),
            );
        }

        if let Some(scale) = reader.f32_at(10) {
            metadata.insert(
                "Photoshop:PrintScale".to_string(),
                TagValue::String(format_perl_number(scale as f64)),
            );
        }
    }

    /// Parse embedded EXIF data
    fn parse_exif_data(data: &[u8], metadata: &mut MetadataMap) {
        if data.len() < 8 {
            return;
        }

        // Detect byte order
        let byte_order = match &data[0..2] {
            b"II" => ByteOrder::LittleEndian,
            b"MM" => ByteOrder::BigEndian,
            _ => return,
        };

        // Create EndianReader with appropriate byte order
        let endian_order = match byte_order {
            ByteOrder::LittleEndian => EndianByteOrder::Little,
            ByteOrder::BigEndian => EndianByteOrder::Big,
        };
        let tiff_reader = EndianReader::new(data, endian_order);

        // Verify TIFF magic
        let magic = tiff_reader.u16_at(2).unwrap_or(0);
        if magic != 0x002A {
            return;
        }

        // Get IFD0 offset
        let ifd0_offset = tiff_reader.u32_at(4).unwrap_or(0);

        // Create a BufferedReader from the TIFF data
        let reader = BufferedReader::from_bytes(data);

        // Parse IFD0
        if let Ok(entries) = parse_ifd(&reader, ifd0_offset as u64, byte_order) {
            for (tag_id, field_type, value_count, raw_bytes) in &entries {
                let tag_name = lookup_tag_name(*tag_id, "IFD0");
                let value = raw_bytes_to_tag_value(
                    raw_bytes.as_ref(),
                    *field_type,
                    *value_count,
                    *tag_id,
                    byte_order,
                );
                metadata.insert(tag_name, value);

                // Check for ExifIFD pointer
                if *tag_id == 0x8769 && raw_bytes.len() >= 4 {
                    let tag_reader = EndianReader::new(raw_bytes, endian_order);
                    let exif_offset = tag_reader.u32_at(0).unwrap_or(0);
                    if let Ok(exif_entries) = parse_ifd(&reader, exif_offset as u64, byte_order) {
                        for (exif_tag_id, exif_field_type, exif_value_count, exif_raw_bytes) in
                            &exif_entries
                        {
                            let exif_tag_name = lookup_tag_name(*exif_tag_id, "ExifIFD");
                            let value = raw_bytes_to_tag_value(
                                exif_raw_bytes.as_ref(),
                                *exif_field_type,
                                *exif_value_count,
                                *exif_tag_id,
                                byte_order,
                            );
                            metadata.insert(exif_tag_name, value);
                        }
                    }
                }
            }
        }

        // parse_ifd() returns only the entries in the requested directory.
        // Follow IFD0's next-directory pointer because embedded EXIF commonly
        // stores thumbnail tags, including Compression, in IFD1.
        if let Some(ifd1_offset) = next_ifd_offset(data, ifd0_offset, byte_order) {
            if ifd1_offset != 0 {
                if let Ok(entries) = parse_ifd(&reader, ifd1_offset as u64, byte_order) {
                    for (tag_id, field_type, value_count, raw_bytes) in &entries {
                        let tag_name = lookup_ifd1_tag_name(*tag_id);
                        let value = raw_bytes_to_tag_value(
                            raw_bytes.as_ref(),
                            *field_type,
                            *value_count,
                            *tag_id,
                            byte_order,
                        );
                        metadata.insert(tag_name, value);
                    }
                }
            }
        }
    }

    /// Extract metadata from XMP using the proper RDF parser
    fn parse_xmp_data(xmp: &str, metadata: &mut MetadataMap) {
        if let Ok(xmp_tags) = parse_xmp(xmp.as_bytes()) {
            for (tag_name, value) in xmp_tags {
                metadata.insert(tag_name, TagValue::String(value));
            }
        }
    }

    /// Parse IPTC data from image resource block
    fn parse_iptc_data(data: &[u8], metadata: &mut MetadataMap) {
        if let Ok(records) = parse_all_iptc_records(data) {
            for record in records {
                // Only process Application Record (record 2)
                if record.record_number == 2 {
                    let tag_name = dataset_to_tag_name(record.record_number, record.dataset_number);
                    let value = decode_iptc_string(&record.data);

                    // Use IPTC: prefix for tag names
                    let full_name = if tag_name.starts_with("IPTC:") {
                        tag_name
                    } else {
                        format!("IPTC:{}", tag_name)
                    };
                    metadata.insert(full_name, TagValue::String(value));
                } else if record.record_number == 1 {
                    // Record 1 is the envelope record - parse version
                    if record.dataset_number == 0 && record.data.len() >= 2 {
                        let version = u16::from_be_bytes([record.data[0], record.data[1]]);
                        metadata.insert(
                            "IPTC:ApplicationRecordVersion".to_string(),
                            TagValue::Integer(version as i64),
                        );
                    }
                }
            }
        }
    }

    /// Parse the Layer and Mask Information section plus the two-byte Image
    /// Data header that follows it.
    ///
    /// `layers_offset` is the first byte after the Image Resources section.
    /// The layout, per `ProcessPSD` / `ProcessLayersAndMask`, is a length word
    /// (four bytes in PSD, eight in PSB) covering the whole layer-and-mask
    /// block, then the image data section, whose first `int16u` is the
    /// compression mode.
    fn parse_layers_and_image_data(
        reader: &dyn FileReader,
        layers_offset: u64,
        is_psb: bool,
        metadata: &mut MetadataMap,
    ) {
        let size_width: u64 = if is_psb { 8 } else { 4 };
        let Ok(size_bytes) = reader.read(layers_offset, size_width as usize) else {
            return;
        };
        let size_reader = EndianReader::big_endian(size_bytes);
        let Some(section_len) = (if is_psb {
            size_reader.u64_at(0)
        } else {
            size_reader.u32_at(0).map(u64::from)
        }) else {
            return;
        };

        // `ProcessLayersAndMask` bails out before touching the layer table when
        // the section is empty, so ExifTool reports no LayerCount for such a
        // file and neither do we.
        if section_len != 0 {
            Self::parse_layer_count(reader, layers_offset, size_width, metadata);
        }

        let Some(image_data_offset) = layers_offset
            .checked_add(size_width)
            .and_then(|o| o.checked_add(section_len))
        else {
            return;
        };
        let Ok(compression_bytes) = reader.read(image_data_offset, 2) else {
            return;
        };
        if let Some(code) = EndianReader::big_endian(compression_bytes).u16_at(0) {
            metadata.insert(
                "Photoshop:Compression".to_string(),
                TagValue::String(print_conv(i64::from(code), PSD_COMPRESSION)),
            );
        }
    }

    /// Emit LayerCount from the layer-info header.
    ///
    /// The count is an `int16s` whose sign only records that the first channel
    /// holds transparency data, so `ProcessLayers` negates it back
    /// (`$num = -$num if $num < 0`). It normally sits right after the
    /// layer-info length word, but when both that length and the count read
    /// zero, `ProcessLayersAndMask` looks for a 16-bit layer block (`Lr16`,
    /// optionally behind an `Mt16` block) and takes the count from there
    /// instead.
    fn parse_layer_count(
        reader: &dyn FileReader,
        layers_offset: u64,
        size_width: u64,
        metadata: &mut MetadataMap,
    ) {
        let header_start = layers_offset + size_width;
        let Ok(header) = reader.read(header_start, (size_width + 2) as usize) else {
            return;
        };
        let header_reader = EndianReader::big_endian(header);
        let Some(layer_info_len) = (if size_width == 8 {
            header_reader.u64_at(0)
        } else {
            header_reader.u32_at(0).map(u64::from)
        }) else {
            return;
        };
        let Some(count) = header_reader.i16_at(size_width as usize) else {
            return;
        };

        let count_offset = if layer_info_len == 0 && count == 0 {
            match Self::find_wide_layer_count_offset(reader, header_start, size_width) {
                Some(offset) => offset,
                // No 16-bit layer block: ExifTool seeks back and reads the same
                // zero count it already saw here.
                None => header_start + size_width,
            }
        } else {
            header_start + size_width
        };

        let Ok(count_bytes) = reader.read(count_offset, 2) else {
            return;
        };
        let Some(count) = EndianReader::big_endian(count_bytes).i16_at(0) else {
            return;
        };
        metadata.insert(
            "Photoshop:LayerCount".to_string(),
            TagValue::Integer(count.unsigned_abs() as i64),
        );
    }

    /// Locate the layer count inside a 16-bit-per-channel layer block.
    ///
    /// Mirrors the `Lr16` / `Mt16` probe in `ProcessLayersAndMask`: two bytes
    /// of padding then an `8BIMLr16` block, or an `8BIMMt16` block followed by
    /// an `8BIMLr16` one. In both cases the count sits two bytes past the
    /// block's own length word.
    fn find_wide_layer_count_offset(
        reader: &dyn FileReader,
        header_start: u64,
        size_width: u64,
    ) -> Option<u64> {
        let probe_start = header_start + size_width + 2;
        let probe = reader.read(probe_start, 10).ok()?;
        let signature = probe.get(2..10)?;

        if signature == b"8BIMLr16" {
            return Some(probe_start + 10 + size_width);
        }
        if signature == b"8BIMMt16" {
            let nested_start = probe_start + 10 + size_width;
            let nested = reader.read(nested_start, 8).ok()?;
            if nested == b"8BIMLr16" {
                return Some(nested_start + 8 + size_width);
            }
        }
        None
    }

    /// Parse the Photo Mechanic trailer, if the file carries one.
    ///
    /// `ProcessPhotoMechanic` validates the last twelve bytes against
    /// `/cbipcbbl$/`, reads the trailer length from the four bytes ahead of
    /// that magic, rejects a length with the high bit set, and then walks the
    /// trailer as an IPTC structure whose record 2 is the SoftEdit table.
    fn parse_photomechanic_trailer(reader: &dyn FileReader, metadata: &mut MetadataMap) {
        let file_size = reader.size();
        if file_size < 12 {
            return;
        }

        let Ok(footer) = reader.read(file_size - 12, 12) else {
            return;
        };
        if footer.get(4..12) != Some(PHOTO_MECHANIC_FOOTER) {
            return;
        }
        let Some(trailer_len) = EndianReader::big_endian(footer).u32_at(0) else {
            return;
        };
        // ExifTool warns "Bad PhotoMechanic trailer" rather than seeking to a
        // negative position when the length has its high bit set.
        if trailer_len & 0x8000_0000 != 0 {
            return;
        }
        let trailer_len = trailer_len as u64;
        if trailer_len + 12 > file_size {
            return;
        }

        let Ok(trailer) = reader.read(file_size - 12 - trailer_len, trailer_len as usize) else {
            return;
        };
        let Ok(records) = parse_all_iptc_records(trailer) else {
            return;
        };

        for record in records {
            // Only record 2 carries soft edit information.
            if record.record_number != 2 {
                continue;
            }
            if let Some((tag, value)) =
                photo_mechanic_soft_edit(record.dataset_number, &record.data)
            {
                metadata.insert(format!("PhotoMechanic:{tag}"), value);
            }
        }
    }
}

/// Decode one `%Image::ExifTool::PhotoMechanic::SoftEdit` dataset.
///
/// The table declares `FORMAT => 'int32s'`, so every value below is a signed
/// big-endian 32-bit integer. Datasets that ExifTool does not name are left
/// alone rather than guessed at.
fn photo_mechanic_soft_edit(dataset: u8, data: &[u8]) -> Option<(&'static str, TagValue)> {
    let raw = EndianReader::big_endian(data).i32_at(0)?;

    // The raw/preview crop coordinates share one conversion:
    // `ValueConv => '$val / 655.36'`, `PrintConv => 'sprintf("%.3f%%",$val)'`.
    let crop_percent = |value: i32| TagValue::String(format!("{:.3}%", f64::from(value) / 655.36));

    let tag_value = match dataset {
        209 => ("RawCropLeft", crop_percent(raw)),
        210 => ("RawCropTop", crop_percent(raw)),
        211 => ("RawCropRight", crop_percent(raw)),
        212 => ("RawCropBottom", crop_percent(raw)),
        213 => ("ConstrainedCropWidth", TagValue::Integer(raw as i64)),
        214 => ("ConstrainedCropHeight", TagValue::Integer(raw as i64)),
        215 => ("FrameNum", TagValue::Integer(raw as i64)),
        216 => (
            "Rotation",
            TagValue::String(print_conv(i64::from(raw), PHOTO_MECHANIC_ROTATION)),
        ),
        217 => ("CropLeft", TagValue::Integer(raw as i64)),
        218 => ("CropTop", TagValue::Integer(raw as i64)),
        219 => ("CropRight", TagValue::Integer(raw as i64)),
        220 => ("CropBottom", TagValue::Integer(raw as i64)),
        221 => (
            "Tagged",
            TagValue::String(print_conv(i64::from(raw), &[(0, "No"), (1, "Yes")])),
        ),
        222 => (
            "ColorClass",
            TagValue::String(print_conv(i64::from(raw), PHOTO_MECHANIC_COLOR_CLASSES)),
        ),
        223 => ("Rating", TagValue::Integer(raw as i64)),
        236 => ("PreviewCropLeft", crop_percent(raw)),
        237 => ("PreviewCropTop", crop_percent(raw)),
        238 => ("PreviewCropRight", crop_percent(raw)),
        239 => ("PreviewCropBottom", crop_percent(raw)),
        _ => return None,
    };

    Some(tag_value)
}

impl FormatParser for PSDParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid PSD signature"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert(
            "FileSize".to_string(),
            TagValue::Integer(reader.size() as i64),
        );

        // Parse header
        Self::parse_header(reader, &mut metadata)?;

        // Parse image resources (EXIF, XMP, etc.)
        if let Some(resources_end) = Self::parse_image_resources(reader, &mut metadata)? {
            // PSB (version 2) widens the section length words from four bytes
            // to eight; everything downstream of the resources depends on it.
            let is_psb = Self::read_version(reader)? == 2;
            Self::parse_layers_and_image_data(reader, resources_end, is_psb, &mut metadata);
        }

        // Trailers live at the very end of the file, independent of the
        // section chain above.
        Self::parse_photomechanic_trailer(reader, &mut metadata);

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::PSD)
    }
}

/// Parses metadata from PSD files.
pub fn parse_psd_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = PSDParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// Returns the offset of the IFD linked after `ifd_offset`.
///
/// A TIFF IFD consists of a two-byte entry count, twelve bytes per entry, and
/// then a four-byte offset to the next IFD.
fn next_ifd_offset(data: &[u8], ifd_offset: u32, byte_order: ByteOrder) -> Option<u32> {
    let endian_order = match byte_order {
        ByteOrder::LittleEndian => EndianByteOrder::Little,
        ByteOrder::BigEndian => EndianByteOrder::Big,
    };
    let reader = EndianReader::new(data, endian_order);
    let ifd_offset = ifd_offset as usize;
    let entry_count = reader.u16_at(ifd_offset)? as usize;
    let entries_size = entry_count.checked_mul(12)?;
    let next_offset_position = ifd_offset.checked_add(2)?.checked_add(entries_size)?;

    reader.u32_at(next_offset_position)
}

/// Looks up an IFD1 tag name, accounting for context-dependent EXIF aliases.
fn lookup_ifd1_tag_name(tag_id: u16) -> String {
    let database_name = lookup_tag_name(tag_id, "IFD1");

    // ExifTool names 0x0201 ThumbnailOffset when it occurs in IFD1. Keep the
    // group selected by the tag database rather than hard-coding an EXIF
    // prefix, since the same numeric ID has other names in other directories.
    if tag_id == 0x0201 {
        if let Some((group, _)) = database_name.rsplit_once(':') {
            return format!("{group}:ThumbnailOffset");
        }
    }

    // ExifTool names 0x0202 ThumbnailLength when it occurs in IFD1.
    if tag_id == 0x0202 {
        if let Some((group, _)) = database_name.rsplit_once(':') {
            return format!("{group}:ThumbnailLength");
        }
    }

    database_name
}

/// Converts raw bytes to TagValue
fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    _value_count: u32,
    tag_id: u16,
    byte_order: ByteOrder,
) -> TagValue {
    use crate::parsers::common::exif_types::ExifType;

    // Create EndianReader with appropriate byte order
    let endian_order = match byte_order {
        ByteOrder::LittleEndian => EndianByteOrder::Little,
        ByteOrder::BigEndian => EndianByteOrder::Big,
    };
    let reader = EndianReader::new(bytes, endian_order);

    if let Some(exif_type) = ExifType::from_u16(field_type) {
        match exif_type {
            ExifType::Ascii => {
                let text = String::from_utf8_lossy(bytes);
                return TagValue::String(text.trim_end_matches('\0').to_string());
            }
            ExifType::Short if bytes.len() >= 2 => {
                let value = reader.u16_at(0).unwrap_or(0);

                // Image::ExifTool::Exif::Main Compression PrintConv.
                if tag_id == 0x0103 && value == 6 {
                    return TagValue::String("JPEG (old-style)".to_string());
                }

                return TagValue::Integer(value as i64);
            }
            ExifType::Long if bytes.len() >= 4 => {
                let value = reader.u32_at(0).unwrap_or(0);
                return TagValue::Integer(value as i64);
            }
            ExifType::Rational if bytes.len() >= 8 => {
                if let Some((num, den)) = reader.rational_at(0) {
                    if den == 1 {
                        return TagValue::Integer(num as i64);
                    }
                    return TagValue::Rational {
                        numerator: num as i32,
                        denominator: den as i32,
                    };
                }
            }
            ExifType::Undefined => {
                if tag_id == 0x9000 && bytes.len() >= 4 {
                    let version = String::from_utf8_lossy(&bytes[0..4]);
                    return TagValue::String(version.to_string());
                }
                return TagValue::Binary(bytes.to_vec());
            }
            _ => {}
        }
    }

    if bytes.iter().all(|&b| b.is_ascii() || b == 0) {
        let text = String::from_utf8_lossy(bytes);
        TagValue::String(text.trim_end_matches('\0').to_string())
    } else {
        TagValue::Binary(bytes.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a PSD whose sections are laid out exactly as the format
    /// specifies, so the layer/image-data offset arithmetic is exercised end
    /// to end rather than against a hand-picked offset.
    fn build_psd(resources: &[u8], layer_and_mask: &[u8], image_data: &[u8]) -> Vec<u8> {
        let mut file = Vec::new();
        file.extend_from_slice(b"8BPS");
        file.extend_from_slice(&1u16.to_be_bytes()); // version 1 = PSD
        file.extend_from_slice(&[0u8; 6]); // reserved
        file.extend_from_slice(&3u16.to_be_bytes()); // channels
        file.extend_from_slice(&8u32.to_be_bytes()); // height
        file.extend_from_slice(&8u32.to_be_bytes()); // width
        file.extend_from_slice(&8u16.to_be_bytes()); // depth
        file.extend_from_slice(&3u16.to_be_bytes()); // color mode = RGB
        file.extend_from_slice(&0u32.to_be_bytes()); // color mode data length
        file.extend_from_slice(&(resources.len() as u32).to_be_bytes());
        file.extend_from_slice(resources);
        file.extend_from_slice(&(layer_and_mask.len() as u32).to_be_bytes());
        file.extend_from_slice(layer_and_mask);
        file.extend_from_slice(image_data);
        file
    }

    /// Wraps `data` in an 8BIM image resource block with an empty Pascal name.
    fn image_resource(id: u16, data: &[u8]) -> Vec<u8> {
        let mut block = Vec::new();
        block.extend_from_slice(b"8BIM");
        block.extend_from_slice(&id.to_be_bytes());
        block.extend_from_slice(&[0u8, 0u8]); // empty name, padded to even
        block.extend_from_slice(&(data.len() as u32).to_be_bytes());
        block.extend_from_slice(data);
        if !data.len().is_multiple_of(2) {
            block.push(0);
        }
        block
    }

    fn parse_bytes(file: &[u8]) -> MetadataMap {
        let reader = BufferedReader::from_bytes(file);
        PSDParser.parse(&reader).expect("PSD should parse")
    }

    /// A `var_ustr32` is a four-byte character count followed by UTF-16BE
    /// code units.
    fn var_ustr32(text: &str) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut out = (units.len() as u32).to_be_bytes().to_vec();
        for unit in units {
            out.extend_from_slice(&unit.to_be_bytes());
        }
        out
    }

    #[test]
    fn version_info_offsets_shift_by_the_writer_name_length() {
        // ExifTool keys %Photoshop::VersionInfo by BYTE OFFSET (the table has
        // no FORMAT override, so the default int8u gives an increment of 1),
        // and a var_ustr32's characters shift every later entry
        // (ExifTool.pm: `$count *= 2 if $format eq 'ustr32'; $varSize += $count`).
        // ReaderName is keyed 9 but actually lands at 9 + 2*len(WriterName).
        // Reading it at a fixed offset 9 would return garbage, so this test
        // uses names of DIFFERENT lengths than the sample's to make sure the
        // shift is computed rather than hard-coded to the sample's 30 bytes.
        let mut version_info = Vec::new();
        version_info.extend_from_slice(&1u32.to_be_bytes()); // version
        version_info.push(1); // HasRealMergedData
        version_info.extend_from_slice(&var_ustr32("Writer")); // 6 chars
        version_info.extend_from_slice(&var_ustr32("A Longer Reader"));
        version_info.extend_from_slice(&1u32.to_be_bytes()); // file version

        let metadata = parse_bytes(&build_psd(
            &image_resource(VERSION_INFO, &version_info),
            &[],
            &[],
        ));

        assert_eq!(
            metadata.get("Photoshop:HasRealMergedData"),
            Some(&TagValue::String("Yes".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:WriterName"),
            Some(&TagValue::String("Writer".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:ReaderName"),
            Some(&TagValue::String("A Longer Reader".to_string())),
            "ReaderName must be read at 9 + 2*len(WriterName), not at a fixed 9"
        );
    }

    #[test]
    fn slice_info_num_slices_shifts_past_the_group_name() {
        // Same var_ustr32 shift as VersionInfo: %Photoshop::SliceInfo keys
        // SlicesGroupName 20 and NumSlices 24, but NumSlices really sits at
        // 24 + 2*len(SlicesGroupName). The sample's group name is one
        // character; this one is four, so a hard-coded +2 fails here.
        let mut slice_info = vec![0u8; 20];
        slice_info[3] = 6; // slice info version
        slice_info.extend_from_slice(&var_ustr32("grid"));
        slice_info.extend_from_slice(&7u32.to_be_bytes());

        let metadata = parse_bytes(&build_psd(
            &image_resource(SLICE_INFO, &slice_info),
            &[],
            &[],
        ));

        assert_eq!(
            metadata.get("Photoshop:SlicesGroupName"),
            Some(&TagValue::String("grid".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:NumSlices"),
            Some(&TagValue::Integer(7))
        );
    }

    #[test]
    fn resolution_info_reads_displayed_units_as_int16u_indices() {
        // %Photoshop::Resolution declares FORMAT => 'int16u', so its keys 2
        // and 6 are indices of two bytes -- byte offsets 4 and 12. Treating
        // them as byte offsets instead would read the low half of the two
        // fixed-point resolutions. The two units differ here so a swap or a
        // single shared read is visible.
        let mut resolution = Vec::new();
        resolution.extend_from_slice(&0x0048_0000u32.to_be_bytes()); // 72 dpi
        resolution.extend_from_slice(&1u16.to_be_bytes()); // DisplayedUnitsX
        resolution.extend_from_slice(&1u16.to_be_bytes()); // X width unit
        resolution.extend_from_slice(&0x0048_0000u32.to_be_bytes()); // 72 dpi
        resolution.extend_from_slice(&2u16.to_be_bytes()); // DisplayedUnitsY
        resolution.extend_from_slice(&1u16.to_be_bytes()); // Y height unit

        let metadata = parse_bytes(&build_psd(
            &image_resource(RESOLUTION_INFO, &resolution),
            &[],
            &[],
        ));

        assert_eq!(
            metadata.get("Photoshop:DisplayedUnitsX"),
            Some(&TagValue::String("inches".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:DisplayedUnitsY"),
            Some(&TagValue::String("cm".to_string()))
        );
    }

    #[test]
    fn print_scale_info_uses_perl_number_formatting() {
        // PrintScaleInfo is byte-keyed: PrintStyle int16u at 0, PrintPosition
        // float[2] at 2, PrintScale float at 10. Perl stringifies an NV with
        // %.15g, so 1.0 prints as "1" -- a hard-coded {:.2} would emit "1.00"
        // and register as a value mismatch against ExifTool.
        let mut print_scale = Vec::new();
        print_scale.extend_from_slice(&1u16.to_be_bytes()); // PrintStyle
        print_scale.extend_from_slice(&0.0f32.to_be_bytes());
        print_scale.extend_from_slice(&2.5f32.to_be_bytes());
        print_scale.extend_from_slice(&1.0f32.to_be_bytes());

        let metadata = parse_bytes(&build_psd(
            &image_resource(PRINT_SCALE_INFO, &print_scale),
            &[],
            &[],
        ));

        assert_eq!(
            metadata.get("Photoshop:PrintStyle"),
            Some(&TagValue::String("Size to Fit".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:PrintPosition"),
            Some(&TagValue::String("0 2.5".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:PrintScale"),
            Some(&TagValue::String("1".to_string())),
            "Perl prints the float 1.0 as \"1\", not \"1.00\""
        );
    }

    #[test]
    fn unknown_enum_codes_report_themselves() {
        // BLIND-SPOT GUARD. Photoshop.psd only ever exercises the codes that
        // happen to be in it (DisplayedUnits 1, PrintStyle 0, Compression 1,
        // Rotation 2, ColorClass 6), so a green coverage run says nothing
        // about codes outside those. Rounding an unrecognised code to a
        // neighbouring label is how a fabricated value gets mistaken for a
        // real one; ExifTool renders it "Unknown (N)" instead.
        let mut resolution = vec![0u8; 16];
        resolution[5] = 9; // DisplayedUnitsX = 9, not in {1, 2}

        let metadata = parse_bytes(&build_psd(
            &image_resource(RESOLUTION_INFO, &resolution),
            &[],
            &[0x00, 0x63], // Compression = 99, not in {0, 1, 2, 3}
        ));

        assert_eq!(
            metadata.get("Photoshop:DisplayedUnitsX"),
            Some(&TagValue::String("Unknown (9)".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:Compression"),
            Some(&TagValue::String("Unknown (99)".to_string()))
        );
        assert_eq!(
            photo_mechanic_soft_edit(216, &99i32.to_be_bytes()),
            Some(("Rotation", TagValue::String("Unknown (99)".to_string())))
        );
        assert_eq!(
            photo_mechanic_soft_edit(222, &99i32.to_be_bytes()),
            Some(("ColorClass", TagValue::String("Unknown (99)".to_string())))
        );
        // The SoftEdit table is int32s, so a negative code has to print as
        // itself; casting through u32 first would report 4294967295.
        assert_eq!(
            photo_mechanic_soft_edit(216, &(-1i32).to_be_bytes()),
            Some(("Rotation", TagValue::String("Unknown (-1)".to_string())))
        );
    }

    #[test]
    fn compression_is_read_past_the_layer_and_mask_section() {
        // ProcessPSD reads the compression word from the image data section,
        // which starts after the layer-and-mask section's own length word AND
        // its payload. A parser that skipped only the length word would read
        // the layer bytes instead -- here that would report "RLE" (0x0001)
        // rather than the real "ZIP with prediction".
        let mut layer_and_mask = 0u32.to_be_bytes().to_vec(); // layer info length
        layer_and_mask.extend_from_slice(&4i16.to_be_bytes()); // layer count
        layer_and_mask.extend_from_slice(&[0x00, 0x01]); // decoy compression word

        let metadata = parse_bytes(&build_psd(
            &image_resource(GLOBAL_ANGLE, &120u32.to_be_bytes()),
            &layer_and_mask,
            &3u16.to_be_bytes(),
        ));

        assert_eq!(
            metadata.get("Photoshop:LayerCount"),
            Some(&TagValue::Integer(4))
        );
        assert_eq!(
            metadata.get("Photoshop:Compression"),
            Some(&TagValue::String("ZIP with prediction".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:GlobalAngle"),
            Some(&TagValue::Integer(120))
        );
    }

    #[test]
    fn negative_layer_count_is_reported_as_its_magnitude() {
        // ProcessLayers reads the count as int16s and negates it back
        // (`$num = -$num if $num < 0`); the sign only records that the first
        // channel holds transparency data. Reading it as int16u would report
        // 65534 for -2.
        let mut layer_and_mask = 0u32.to_be_bytes().to_vec();
        layer_and_mask.extend_from_slice(&(-2i16).to_be_bytes());

        let metadata = parse_bytes(&build_psd(&[], &layer_and_mask, &1u16.to_be_bytes()));

        assert_eq!(
            metadata.get("Photoshop:LayerCount"),
            Some(&TagValue::Integer(2))
        );
    }

    #[test]
    fn empty_layer_section_yields_no_layer_count() {
        // ProcessLayersAndMask returns before touching the layer table when
        // the section length is zero, so ExifTool reports no LayerCount at
        // all. Emitting a fabricated 0 here would be a tag ExifTool never
        // produces.
        let metadata = parse_bytes(&build_psd(&[], &[], &1u16.to_be_bytes()));

        assert!(metadata.get("Photoshop:LayerCount").is_none());
        assert_eq!(
            metadata.get("Photoshop:Compression"),
            Some(&TagValue::String("RLE".to_string()))
        );
    }

    #[test]
    fn url_list_is_emitted_even_when_empty() {
        // `exiftool -j` reports "Photoshop:URL_List": [] for the sample, so
        // the tag is present with no entries -- dropping it on a zero count
        // would leave the gap open.
        let metadata = parse_bytes(&build_psd(
            &image_resource(URL_LIST, &0u32.to_be_bytes()),
            &[],
            &[],
        ));

        assert_eq!(
            metadata.get("Photoshop:URL_List"),
            Some(&TagValue::Array(Vec::new()))
        );
    }

    #[test]
    fn url_list_decodes_utf16_entries_after_their_eight_byte_headers() {
        // Photoshop.pm's ValueConv skips a word and an ID (8 bytes) ahead of
        // each var_ustr32. The sample's list is empty, so nothing in the
        // corpus covers a populated one.
        let mut url_list = 2u32.to_be_bytes().to_vec();
        for url in ["https://a.example/", "https://bb.example/"] {
            url_list.extend_from_slice(&[0u8; 8]);
            url_list.extend_from_slice(&var_ustr32(url));
        }

        let metadata = parse_bytes(&build_psd(&image_resource(URL_LIST, &url_list), &[], &[]));

        assert_eq!(
            metadata.get("Photoshop:URL_List"),
            Some(&TagValue::Array(vec![
                TagValue::String("https://a.example/".to_string()),
                TagValue::String("https://bb.example/".to_string()),
            ]))
        );
    }

    #[test]
    fn iptc_digest_is_lowercase_hex_of_the_raw_bytes() {
        // ValueConv => 'unpack("H*", $val)'. The high nibbles here would be
        // lost by a %x that drops leading zeros, and uppercase would not match
        // ExifTool.
        let digest = [
            0x0b, 0x76, 0x5e, 0x54, 0x39, 0x6f, 0x28, 0x85, 0xe9, 0xa7, 0x6f, 0xf4, 0xad, 0x66,
            0x5b, 0x19,
        ];

        let metadata = parse_bytes(&build_psd(&image_resource(IPTC_DIGEST, &digest), &[], &[]));

        assert_eq!(
            metadata.get("Photoshop:IPTCDigest"),
            Some(&TagValue::String(
                "0b765e54396f2885e9a76ff4ad665b19".to_string()
            ))
        );
    }

    #[test]
    fn photo_mechanic_trailer_is_read_from_the_end_of_the_file() {
        // ProcessPhotoMechanic seeks from EOF: the last twelve bytes are a
        // four-byte length plus the magic "cbipcbbl", and the trailer body is
        // the length bytes immediately before them. The trailer sits after the
        // image data, so a parser that only walks forward through the section
        // chain never sees it.
        let mut trailer = Vec::new();
        for (dataset, value) in [(221u8, 1i32), (222, 6), (216, 2), (217, 438)] {
            trailer.extend_from_slice(&[0x1c, 0x02, dataset]);
            trailer.extend_from_slice(&4u16.to_be_bytes());
            trailer.extend_from_slice(&value.to_be_bytes());
        }

        let mut file = build_psd(&[], &[], &1u16.to_be_bytes());
        file.extend_from_slice(&trailer);
        file.extend_from_slice(&(trailer.len() as u32).to_be_bytes());
        file.extend_from_slice(PHOTO_MECHANIC_FOOTER);

        let metadata = parse_bytes(&file);

        assert_eq!(
            metadata.get("PhotoMechanic:Tagged"),
            Some(&TagValue::String("Yes".to_string()))
        );
        assert_eq!(
            metadata.get("PhotoMechanic:ColorClass"),
            Some(&TagValue::String("6 (Typical alt)".to_string()))
        );
        assert_eq!(
            metadata.get("PhotoMechanic:Rotation"),
            Some(&TagValue::String("180".to_string())),
            "dataset 216 is an enum code, not a degree count: 2 means 180"
        );
        assert_eq!(
            metadata.get("PhotoMechanic:CropLeft"),
            Some(&TagValue::Integer(438))
        );
    }

    #[test]
    fn a_file_without_the_footer_yields_no_photomechanic_tags() {
        // The magic is the only thing separating a real trailer from twelve
        // arbitrary trailing bytes; without it nothing may be emitted.
        let mut file = build_psd(&[], &[], &1u16.to_be_bytes());
        file.extend_from_slice(&[0x1c, 0x02, 0xdd, 0x00, 0x04, 0, 0, 0, 1]);
        file.extend_from_slice(&9u32.to_be_bytes());
        file.extend_from_slice(b"notpmtrl");

        let metadata = parse_bytes(&file);

        assert!(
            metadata
                .iter()
                .all(|(key, _)| !key.starts_with("PhotoMechanic:")),
            "no PhotoMechanic tag may be emitted without the cbipcbbl footer"
        );
    }

    #[test]
    fn photo_mechanic_raw_crop_uses_the_percentage_conversion() {
        // %rawCropConv: ValueConv => '$val / 655.36', PrintConv =>
        // 'sprintf("%.3f%%",$val)'. Datasets 209-212 and 236-239 share it,
        // unlike the plain integer crops at 217-220.
        assert_eq!(
            photo_mechanic_soft_edit(209, &65536i32.to_be_bytes()),
            Some(("RawCropLeft", TagValue::String("100.000%".to_string())))
        );
        assert_eq!(
            photo_mechanic_soft_edit(239, &0i32.to_be_bytes()),
            Some(("PreviewCropBottom", TagValue::String("0.000%".to_string())))
        );
        assert!(
            photo_mechanic_soft_edit(200, &0i32.to_be_bytes()).is_none(),
            "datasets ExifTool does not name must not be guessed at"
        );
    }

    #[test]
    fn perl_number_formatting_matches_percent_g() {
        // Perl stringifies an NV with %.15g. Rust's Display would print
        // 0.1_f32 widened to f64 as "0.10000000149011612"; Perl stops at
        // fifteen significant digits.
        assert_eq!(format_perl_number(1.0), "1");
        assert_eq!(format_perl_number(0.0), "0");
        assert_eq!(format_perl_number(-0.0), "0");
        assert_eq!(format_perl_number(2.5), "2.5");
        assert_eq!(format_perl_number(72.0), "72");
        assert_eq!(format_perl_number(f64::from(0.1f32)), "0.100000001490116");
    }

    #[test]
    fn parses_compression_from_embedded_exif_ifd1() {
        // Minimal little-endian TIFF:
        // - TIFF header points to an empty IFD0 at offset 8
        // - IFD0 points to IFD1 at offset 14
        // - IFD1 contains Compression (SHORT) = 6
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // Empty IFD0 followed by its next-IFD pointer.
        data[8..10].copy_from_slice(&0u16.to_le_bytes());
        data[10..14].copy_from_slice(&14u32.to_le_bytes());

        // IFD1 with one entry.
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16..18].copy_from_slice(&0x0103u16.to_le_bytes());
        data[18..20].copy_from_slice(&3u16.to_le_bytes()); // SHORT
        data[20..24].copy_from_slice(&1u32.to_le_bytes());
        data[24..26].copy_from_slice(&6u16.to_le_bytes());
        // Bytes 26..28 are inline-value padding; 28..32 is next IFD = 0.

        let mut metadata = MetadataMap::new();
        PSDParser::parse_exif_data(&data, &mut metadata);

        let tag_name = lookup_tag_name(0x0103, "IFD1");
        assert_eq!(
            metadata.get(&tag_name),
            Some(&TagValue::String("JPEG (old-style)".to_string()))
        );
    }

    #[test]
    fn parses_thumbnail_offset_from_embedded_exif_ifd1() {
        // Minimal little-endian TIFF:
        // - TIFF header points to an empty IFD0 at offset 8
        // - IFD0 points to IFD1 at offset 14
        // - IFD1 contains JPEGInterchangeFormat (LONG) = 390, which ExifTool
        //   names ThumbnailOffset in this directory
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // Empty IFD0 followed by its next-IFD pointer.
        data[8..10].copy_from_slice(&0u16.to_le_bytes());
        data[10..14].copy_from_slice(&14u32.to_le_bytes());

        // IFD1 with one inline LONG entry.
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16..18].copy_from_slice(&0x0201u16.to_le_bytes());
        data[18..20].copy_from_slice(&4u16.to_le_bytes()); // LONG
        data[20..24].copy_from_slice(&1u32.to_le_bytes());
        data[24..28].copy_from_slice(&390u32.to_le_bytes());
        // Bytes 28..32 are the zero next-IFD pointer.

        let mut metadata = MetadataMap::new();
        PSDParser::parse_exif_data(&data, &mut metadata);

        let tag_name = lookup_ifd1_tag_name(0x0201);
        assert!(tag_name.ends_with(":ThumbnailOffset"));
        assert_eq!(metadata.get(&tag_name), Some(&TagValue::Integer(390)));
    }

    #[test]
    fn parses_thumbnail_length_from_embedded_exif_ifd1() {
        // Ground truth, Exif.pm line 1295-1297: the FIRST variant of the 0x202
        // conditional list is `Name => 'ThumbnailLength'`, gated on
        // `$$self{DIR_NAME} eq 'IFD1'`. PSD-embedded EXIF reaches this code with
        // DIR_NAME == IFD1, so that variant is the one that matches.
        //
        // Measured 2026-07-26 on
        // /tmp/oxidex-exiftool-cache/combined-samples/Photoshop.psd:
        //   exiftool -G1 -a -s  ->  [IFD1] ThumbnailLength : 0
        //   oxidex (post-fix)   ->  IFD1:ThumbnailLength: 0
        //
        // The key is asserted as a LITERAL rather than via lookup_ifd1_tag_name()
        // so the test cannot pass by agreeing with whatever the function returns.
        // Deleting the 0x0202 branch makes the key fall back to "IFD1:0x0202"
        // and this test goes red.
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // Empty IFD0 followed by its next-IFD pointer.
        data[8..10].copy_from_slice(&0u16.to_le_bytes());
        data[10..14].copy_from_slice(&14u32.to_le_bytes());

        // IFD1 with one inline LONG entry: 0x0202 = 0, exactly as the sample
        // stores it (IFD1 entry #5, `- Tag 0x0202 (4 bytes, int32u[1])`).
        data[14..16].copy_from_slice(&1u16.to_le_bytes());
        data[16..18].copy_from_slice(&0x0202u16.to_le_bytes());
        data[18..20].copy_from_slice(&4u16.to_le_bytes()); // LONG
        data[20..24].copy_from_slice(&1u32.to_le_bytes());
        data[24..28].copy_from_slice(&0u32.to_le_bytes());
        // Bytes 28..32 are the zero next-IFD pointer.

        let mut metadata = MetadataMap::new();
        PSDParser::parse_exif_data(&data, &mut metadata);

        assert_eq!(
            metadata.get("IFD1:ThumbnailLength"),
            Some(&TagValue::Integer(0)),
            "0x0202 in IFD1 must be named ThumbnailLength (Exif.pm:1297)"
        );
        assert!(
            metadata.get("IFD1:0x0202").is_none(),
            "0x0202 must not also survive under its unnamed hex fallback"
        );
    }

    #[test]
    fn thumbnail_length_rename_is_confined_to_ifd1() {
        // BLIND-SPOT REGRESSION TEST. Photoshop.psd carries 0x0202 only in IFD1,
        // so no sample in the corpus exercises 0x0202 in any other directory. A
        // green "recheck-pass gaps=1->0" therefore says nothing about whether the
        // rename leaked into IFD0/ExifIFD -- which is exactly the hole that let
        // the TTF (%ttLang Spanish=12) and RAR (RAR5 host-OS 2/3/4) fabrications
        // ship on 2026-07-26 beside values the sample did happen to hit.
        //
        // ThumbnailLength is NOT 0x202's universal name. Exif.pm lists nine
        // conditional variants; outside IFD1 the same ID is PreviewImageLength
        // (DIR_NAME eq "MakerNotes", line ~1347), JpgFromRawLength (SubIFD /
        // IFD2, ~1368), or OtherImageLength (SubIFD1 / SubIFD2, ~1388). So
        // renaming 0x0202 unconditionally would replace real data with a wrong
        // tag name in every one of those directories.
        //
        // parse_exif_data routes IFD0 through lookup_tag_name(id, "IFD0") and
        // only IFD1 through lookup_ifd1_tag_name(), so this pins the scope of the
        // fix at the call site rather than trusting the branch to stay put.
        let mut data = vec![0u8; 32];
        data[0..2].copy_from_slice(b"II");
        data[2..4].copy_from_slice(&42u16.to_le_bytes());
        data[4..8].copy_from_slice(&8u32.to_le_bytes());

        // IFD0 holding 0x0202 directly, with no IFD1 chained after it.
        data[8..10].copy_from_slice(&1u16.to_le_bytes());
        data[10..12].copy_from_slice(&0x0202u16.to_le_bytes());
        data[12..14].copy_from_slice(&4u16.to_le_bytes()); // LONG
        data[14..18].copy_from_slice(&1u32.to_le_bytes());
        data[18..22].copy_from_slice(&12345u32.to_le_bytes());
        // Bytes 22..26 are the zero next-IFD pointer.

        let mut metadata = MetadataMap::new();
        PSDParser::parse_exif_data(&data, &mut metadata);

        assert!(
            metadata
                .iter()
                .all(|(name, _)| !name.ends_with(":ThumbnailLength")),
            "0x0202 outside IFD1 must not be renamed ThumbnailLength; got {:?}",
            metadata.iter().map(|(n, _)| n).collect::<Vec<_>>()
        );
        // Same guard for the sibling 0x0201 special case, which shares the
        // identical IFD1-only condition in Exif.pm (line 1149).
        assert!(
            metadata
                .iter()
                .all(|(name, _)| !name.ends_with(":ThumbnailOffset")),
            "0x0201 was never present; no ThumbnailOffset should appear"
        );
    }
}
