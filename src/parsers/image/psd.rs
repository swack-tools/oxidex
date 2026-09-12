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
use crate::io::EndianReader;
use crate::parsers::icc::parse_icc_profile_data;
use crate::parsers::image::embedded::{parse_embedded_exif_at, parse_embedded_thumbnail_ifd};
use crate::parsers::jpeg::iptc_parser::{
    dataset_to_tag_name, decode_iptc_string, parse_all_iptc_records,
};
use crate::parsers::xmp::rdf_parser::parse_xmp;

const PSD_SIGNATURE: &[u8] = b"8BPS";

/// Image resource IDs
///
/// IDs and their tag names come from `Image::ExifTool::Photoshop::Main`
/// (Photoshop.pm lines 102-345).
const IPTC_NAA_RECORD: u16 = 0x0404; // IPTC-NAA record
const EXIF_DATA_1: u16 = 0x0422; // EXIF data 1
const EXIF_DATA_3: u16 = 0x0423; // ExifInfo2 (Photoshop.pm:262 Unknown/Binary), not parsed
const XMP_DATA: u16 = 0x0424; // XMP metadata
const ICC_PROFILE: u16 = 0x040F; // ICC profile
const RESOLUTION_INFO: u16 = 0x03ED; // Resolution info
const PRINT_FLAGS: u16 = 0x03F1; // Print flags
const COPYRIGHT_FLAG: u16 = 0x040A; // Copyright flag
const URL: u16 = 0x040B; // URL
const GLOBAL_ANGLE: u16 = 0x040D; // Global angle
const GLOBAL_ALTITUDE: u16 = 0x0419; // Global altitude
const SLICE_INFO: u16 = 0x041A; // Slice info
const URL_LIST: u16 = 0x041E; // URL list
const VERSION_INFO: u16 = 0x0421; // Version info
const IPTC_DIGEST: u16 = 0x0425; // IPTC digest
const PRINT_SCALE_INFO: u16 = 0x0426; // Print scale info

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

    /// Parse Image Resources section
    ///
    /// Returns the file offset of the Layer and Mask Information section, which
    /// begins immediately after the image resources, when it could be located.
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

        if reader.size() < (resources_offset + 4 + resources_length) as u64 {
            return Ok(None);
        }

        // Everything after the resources belongs to the Layer and Mask section.
        let layers_offset = Some((resources_offset + 4 + resources_length) as u64);

        if resources_length == 0 {
            return Ok(layers_offset);
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
                EXIF_DATA_1 => {
                    Self::parse_exif_data(resource_data, metadata);
                }
                COPYRIGHT_FLAG => {
                    // Photoshop.pm:171-181 -- PrintConv { 0 => 'False', 1 => 'True' }.
                    // ExifTool reports the tag whenever the resource exists, so a
                    // zero flag is a value to emit, not a reason to stay silent.
                    if let Some(&flag) = resource_data.first() {
                        metadata.insert(
                            "Photoshop:CopyrightFlag".to_string(),
                            TagValue::String(if flag != 0 { "True" } else { "False" }.to_string()),
                        );
                    }
                }
                URL => {
                    // Photoshop.pm:182-186 -- Writable => 'string'. ExifTool keeps
                    // the space padding Photoshop writes and strips only NULs.
                    if let Ok(text) = std::str::from_utf8(resource_data) {
                        metadata.insert(
                            "Photoshop:URL".to_string(),
                            TagValue::String(text.trim_end_matches('\0').to_string()),
                        );
                    }
                }
                GLOBAL_ANGLE => {
                    // Photoshop.pm:192-197 -- ValueConv => 'unpack("N",$val)'.
                    if let Some(angle) = EndianReader::big_endian(resource_data).u32_at(0) {
                        metadata.insert(
                            "Photoshop:GlobalAngle".to_string(),
                            TagValue::Integer(angle as i64),
                        );
                    }
                }
                GLOBAL_ALTITUDE => {
                    // Photoshop.pm:213-218 -- ValueConv => 'unpack("N",$val)'.
                    if let Some(altitude) = EndianReader::big_endian(resource_data).u32_at(0) {
                        metadata.insert(
                            "Photoshop:GlobalAltitude".to_string(),
                            TagValue::Integer(altitude as i64),
                        );
                    }
                }
                SLICE_INFO => {
                    Self::parse_slice_info(resource_data, metadata);
                }
                URL_LIST => {
                    Self::parse_url_list(resource_data, metadata);
                }
                VERSION_INFO => {
                    Self::parse_version_info(resource_data, metadata);
                }
                IPTC_DIGEST => {
                    // Photoshop.pm:269-297 -- ValueConv => 'unpack("H*", $val)'.
                    if !resource_data.is_empty() {
                        let digest: String = resource_data
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect();
                        metadata
                            .insert("Photoshop:IPTCDigest".to_string(), TagValue::String(digest));
                    }
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
                    // Parse ICC profile data, grouped by table provenance
                    // (ICC-header/ICC-cicp/ICC-view/ICC-meas -- Step 22).
                    if let Ok(icc_tags) = parse_icc_profile_data(resource_data) {
                        crate::parsers::icc::insert_icc_tags(metadata, icc_tags);
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

        Ok(layers_offset)
    }

    /// Parse the Layer and Mask Information section and the Image Data section
    /// that follows it.
    ///
    /// Mirrors `Image::ExifTool::Photoshop::ProcessLayersAndMask`
    /// (Photoshop.pm:746-798) and the `ImageData` table (Photoshop.pm:689-702):
    ///
    /// - `layers_offset + 0`: total length of the layer and mask section
    ///   (4 bytes for PSD, 8 for PSB)
    /// - `layers_offset + psiz`: length of the layer info sub-section
    /// - `layers_offset + 2 * psiz`: `int16s` layer count
    /// - `layers_offset + psiz + total`: the Image Data section, whose first
    ///   `int16u` is the compression mode
    fn parse_layers_and_image_data(
        reader: &dyn FileReader,
        layers_offset: u64,
        is_psb: bool,
        metadata: &mut MetadataMap,
    ) -> Result<()> {
        let psiz: u64 = if is_psb { 8 } else { 4 };

        if reader.size() < layers_offset + psiz {
            return Ok(());
        }

        let header = reader.read(layers_offset, psiz as usize)?;
        let header_reader = EndianReader::big_endian(header);
        let total = if is_psb {
            let Some(high) = header_reader.u32_at(0) else {
                return Ok(());
            };
            let Some(low) = header_reader.u32_at(4) else {
                return Ok(());
            };
            ((high as u64) << 32) | low as u64
        } else {
            let Some(len) = header_reader.u32_at(0) else {
                return Ok(());
            };
            len as u64
        };

        // A zero-length section means there are no layers at all, so ExifTool
        // never reaches the layer count. Photoshop.pm:763 returns early there.
        if total != 0 {
            let count_offset = layers_offset + 2 * psiz;
            if reader.size() >= count_offset + 2 {
                let count_bytes = reader.read(count_offset, 2)?;
                if let Some(raw) = EndianReader::big_endian(count_bytes).u16_at(0) {
                    // Photoshop.pm:837 -- a negative count means the first
                    // channel holds transparency data; the reported count is
                    // its magnitude.
                    let count = (raw as i16).unsigned_abs();
                    metadata.insert(
                        "Photoshop:LayerCount".to_string(),
                        TagValue::Integer(count as i64),
                    );
                }
            }
        }

        let image_data_offset = layers_offset + psiz + total;
        if reader.size() < image_data_offset + 2 {
            return Ok(());
        }

        let compression_bytes = reader.read(image_data_offset, 2)?;
        if let Some(compression) = EndianReader::big_endian(compression_bytes).u16_at(0) {
            let label = match compression {
                0 => "Uncompressed".to_string(),
                1 => "RLE".to_string(),
                2 => "ZIP without prediction".to_string(),
                3 => "ZIP with prediction".to_string(),
                other => format!("Unknown ({other})"),
            };
            metadata.insert("Photoshop:Compression".to_string(), TagValue::String(label));
        }

        Ok(())
    }

    /// Parse resolution info resource (0x03ED)
    ///
    /// `Image::ExifTool::Photoshop::Resolution` (Photoshop.pm:437-473) is a
    /// binary-data table with `FORMAT => 'int16u'`, so its tag indices are in
    /// 16-bit units: index 0 = byte 0 (XResolution, int32u), index 2 = byte 4
    /// (DisplayedUnitsX), index 4 = byte 8 (YResolution, int32u), index 6 =
    /// byte 12 (DisplayedUnitsY).
    fn parse_resolution_info(data: &[u8], metadata: &mut MetadataMap) {
        if data.len() < 16 {
            return;
        }

        // PSD uses big-endian byte order
        let res_reader = EndianReader::big_endian(data);

        // Horizontal resolution (fixed point 16.16)
        let h_res_fixed = res_reader.u32_at(0).unwrap_or(0);
        let h_res = h_res_fixed as f64 / 65536.0;

        // Vertical resolution (byte 8, fixed point 16.16)
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

        // DisplayedUnitsX / DisplayedUnitsY, PrintConv { 1 => 'inches', 2 => 'cm' }.
        // The unit belongs to the display axis, not to a single shared
        // "ResolutionUnit" -- ExifTool has no Photoshop:ResolutionUnit tag at all.
        for (byte_offset, tag) in [
            (4usize, "Photoshop:DisplayedUnitsX"),
            (12usize, "Photoshop:DisplayedUnitsY"),
        ] {
            if let Some(unit) = res_reader.u16_at(byte_offset) {
                let name = match unit {
                    1 => "inches".to_string(),
                    2 => "cm".to_string(),
                    other => format!("Unknown ({other})"),
                };
                metadata.insert(tag.to_string(), TagValue::String(name));
            }
        }
    }

    /// Parse slice info resource (0x041A)
    ///
    /// `Image::ExifTool::Photoshop::SliceInfo` (Photoshop.pm:430-434) reads
    /// SlicesGroupName at byte 20 as `var_ustr32` and NumSlices as `int32u`
    /// directly after it.
    fn parse_slice_info(data: &[u8], metadata: &mut MetadataMap) {
        let Some((group_name, next)) = read_unicode_string(data, 20) else {
            return;
        };

        metadata.insert(
            "Photoshop:SlicesGroupName".to_string(),
            TagValue::String(group_name),
        );

        if let Some(count) = EndianReader::big_endian(data).u32_at(next) {
            metadata.insert(
                "Photoshop:NumSlices".to_string(),
                TagValue::Integer(count as i64),
            );
        }
    }

    /// Parse version info resource (0x0421)
    ///
    /// `Image::ExifTool::Photoshop::VersionInfo` (Photoshop.pm:480-491):
    /// HasRealMergedData is an int8u at byte 4 with PrintConv
    /// `{ 0 => 'No', 1 => 'Yes' }`, followed by two `var_ustr32` strings.
    fn parse_version_info(data: &[u8], metadata: &mut MetadataMap) {
        let reader = EndianReader::big_endian(data);

        if let Some(flag) = reader.u8_at(4) {
            let label = match flag {
                0 => "No".to_string(),
                1 => "Yes".to_string(),
                other => format!("Unknown ({other})"),
            };
            metadata.insert(
                "Photoshop:HasRealMergedData".to_string(),
                TagValue::String(label),
            );
        }

        let Some((writer, next)) = read_unicode_string(data, 5) else {
            return;
        };
        metadata.insert("Photoshop:WriterName".to_string(), TagValue::String(writer));

        if let Some((reader_name, _)) = read_unicode_string(data, next) {
            metadata.insert(
                "Photoshop:ReaderName".to_string(),
                TagValue::String(reader_name),
            );
        }
    }

    /// Parse print scale info resource (0x0426)
    ///
    /// `Image::ExifTool::Photoshop::PrintScaleInfo` (Photoshop.pm:494-512):
    /// PrintStyle int16u at byte 0, PrintPosition float[2] at byte 2,
    /// PrintScale float at byte 10.
    fn parse_print_scale_info(data: &[u8], metadata: &mut MetadataMap) {
        let reader = EndianReader::big_endian(data);

        if let Some(style) = reader.u16_at(0) {
            let label = match style {
                0 => "Centered".to_string(),
                1 => "Size to Fit".to_string(),
                2 => "User Defined".to_string(),
                other => format!("Unknown ({other})"),
            };
            metadata.insert("Photoshop:PrintStyle".to_string(), TagValue::String(label));
        }

        if let (Some(x), Some(y)) = (reader.f32_at(2), reader.f32_at(6)) {
            metadata.insert(
                "Photoshop:PrintPosition".to_string(),
                TagValue::String(format!(
                    "{} {}",
                    format_exiftool_float(x),
                    format_exiftool_float(y)
                )),
            );
        }

        if let Some(scale) = reader.f32_at(10) {
            metadata.insert(
                "Photoshop:PrintScale".to_string(),
                TagValue::String(format_exiftool_float(scale)),
            );
        }
    }

    /// Parse the URL list resource (0x041E)
    ///
    /// Photoshop.pm:226-247 -- a 4-byte count followed by that many entries of
    /// `[4-byte word][4-byte ID][4-byte UTF-16 length][UTF-16BE string]`.
    /// ExifTool emits the tag as a (possibly empty) list.
    fn parse_url_list(data: &[u8], metadata: &mut MetadataMap) {
        let reader = EndianReader::big_endian(data);
        let Some(count) = reader.u32_at(0) else {
            return;
        };

        let mut urls = Vec::new();
        let mut pos = 4usize;
        for _ in 0..count {
            // Skip the 4-byte word and 4-byte ID preceding each string.
            let Some((url, next)) = pos
                .checked_add(8)
                .and_then(|p| read_unicode_string(data, p))
            else {
                break;
            };
            urls.push(TagValue::String(url));
            pos = next;
        }

        metadata.insert("Photoshop:URL_List".to_string(), TagValue::Array(urls));
    }

    /// Parses the TIFF block in image resource 0x0422 (ExifInfo).
    ///
    /// Photoshop.pm 13.59:254-260 declares the resource as a SubDirectory
    /// of Image::ExifTool::Exif::Main processed by ProcessTIFF, so IFD0, the
    /// EXIF and GPS sub-IFDs and the thumbnail IFD go through the same
    /// decoders the JPEG path uses; the PDF Photoshop-resource path takes the
    /// identical route for the identical resource.
    fn parse_exif_data(data: &[u8], metadata: &mut MetadataMap) {
        parse_embedded_exif_at(data, 0, metadata);
        parse_embedded_thumbnail_ifd(data, metadata);
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

                    // Use IPTC: prefix for tag names
                    let full_name = if tag_name.starts_with("IPTC:") {
                        tag_name
                    } else {
                        format!("IPTC:{}", tag_name)
                    };

                    // IPTC.pm gives dataset 2:0 Format => 'int16u'
                    // (ApplicationRecordVersion), so its two bytes are a number,
                    // not text. Decoding it as a string yielded the raw
                    // "\0\x02" bytes where ExifTool reports "2".
                    if record.dataset_number == 0 && record.data.len() >= 2 {
                        let version = u16::from_be_bytes([record.data[0], record.data[1]]);
                        metadata.insert(full_name, TagValue::Integer(version as i64));
                    } else {
                        let value = decode_iptc_string(&record.data);
                        metadata.insert(full_name, TagValue::String(value));
                    }
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
}

impl FormatParser for PSDParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid PSD signature"));
        }

        let mut metadata = MetadataMap::new();

        // Parse header
        Self::parse_header(reader, &mut metadata)?;

        // Parse image resources (EXIF, XMP, etc.)
        let layers_offset = Self::parse_image_resources(reader, &mut metadata)?;

        // Layer count and the image data compression mode live after the
        // resources; ExifTool reads both in ProcessPSD (Photoshop.pm:1206-1223).
        if let Some(offset) = layers_offset {
            let is_psb = Self::read_version(reader)? == 2;
            Self::parse_layers_and_image_data(reader, offset, is_psb, &mut metadata)?;
        }

        // ProcessPSD finishes by handing the tail of the file to the trailer
        // scanners (Photoshop.pm:1226-1227).
        if let Ok(file) = reader.read(0, reader.size() as usize) {
            for (key, value) in
                crate::parsers::photo_mechanic::parse_photo_mechanic_trailer(file).iter()
            {
                metadata.insert(key.clone(), value.clone());
            }
        }

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

/// Reads an ExifTool `var_ustr32` value: a 4-byte big-endian character count
/// followed by that many UTF-16BE code units.
///
/// Returns the decoded string and the offset of the first byte after it.
fn read_unicode_string(data: &[u8], offset: usize) -> Option<(String, usize)> {
    let char_count = EndianReader::big_endian(data).u32_at(offset)? as usize;
    let start = offset.checked_add(4)?;
    let byte_len = char_count.checked_mul(2)?;
    let end = start.checked_add(byte_len)?;
    let bytes = data.get(start..end)?;

    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
        .collect();
    let text = String::from_utf16(&units).ok()?;

    Some((text.trim_end_matches('\0').to_string(), end))
}

/// Formats a float the way ExifTool prints one.
///
/// ExifTool renders a whole-numbered float without a fractional part, so
/// PrintScale 1.0 is reported as "1", not "1.0".
fn format_exiftool_float(value: f32) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod image_resource_tests {
    use super::*;
    use crate::io::buffered_reader::BufferedReader;

    /// Encodes an ExifTool `var_ustr32`: 4-byte character count then UTF-16BE.
    fn ustr32(text: &str) -> Vec<u8> {
        let units: Vec<u16> = text.encode_utf16().collect();
        let mut out = (units.len() as u32).to_be_bytes().to_vec();
        for unit in units {
            out.extend_from_slice(&unit.to_be_bytes());
        }
        out
    }

    #[test]
    fn resolution_info_reads_each_display_unit_from_its_own_offset() {
        // Photoshop.pm:437-473 -- FORMAT => 'int16u', so index 2 is byte 4
        // (DisplayedUnitsX) and index 6 is byte 12 (DisplayedUnitsY). The two
        // units are given DIFFERENT values here on purpose: the previous code
        // read one unit at byte 4 and reported it as a single ResolutionUnit,
        // so a regression that reuses byte 4 for both axes turns this red.
        let mut data = Vec::new();
        data.extend_from_slice(&(72u32 * 0x10000).to_be_bytes()); // XResolution
        data.extend_from_slice(&1u16.to_be_bytes()); // DisplayedUnitsX = inches
        data.extend_from_slice(&3u16.to_be_bytes()); // (width unit, not a tag)
        data.extend_from_slice(&(300u32 * 0x10000).to_be_bytes()); // YResolution
        data.extend_from_slice(&2u16.to_be_bytes()); // DisplayedUnitsY = cm
        data.extend_from_slice(&4u16.to_be_bytes()); // (height unit, not a tag)

        let mut metadata = MetadataMap::new();
        PSDParser::parse_resolution_info(&data, &mut metadata);

        assert_eq!(
            metadata.get("Photoshop:XResolution"),
            Some(&TagValue::String("72".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:YResolution"),
            Some(&TagValue::String("300".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:DisplayedUnitsX"),
            Some(&TagValue::String("inches".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:DisplayedUnitsY"),
            Some(&TagValue::String("cm".to_string()))
        );
        assert!(
            metadata.get("ResolutionUnit").is_none(),
            "ExifTool has no Photoshop ResolutionUnit tag; it must not be emitted"
        );
    }

    #[test]
    fn version_info_chains_past_the_variable_length_writer_name() {
        // Photoshop.pm:480-491. ReaderName's nominal index is 9, but ExifTool's
        // var_ format shifts it by however long WriterName turned out to be.
        // Reading ReaderName at a fixed offset would yield garbage here.
        let mut data = vec![0u8; 4]; // version
        data.push(1); // HasRealMergedData
        data.extend_from_slice(&ustr32("Adobe Photoshop"));
        data.extend_from_slice(&ustr32("Adobe Photoshop 7.0"));
        data.extend_from_slice(&1u32.to_be_bytes()); // file version

        let mut metadata = MetadataMap::new();
        PSDParser::parse_version_info(&data, &mut metadata);

        assert_eq!(
            metadata.get("Photoshop:HasRealMergedData"),
            Some(&TagValue::String("Yes".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:WriterName"),
            Some(&TagValue::String("Adobe Photoshop".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:ReaderName"),
            Some(&TagValue::String("Adobe Photoshop 7.0".to_string()))
        );
    }

    #[test]
    fn slice_info_reads_num_slices_after_the_group_name() {
        // Photoshop.pm:430-434. NumSlices sits directly after the var_ustr32
        // group name, not at a fixed byte 24.
        let mut data = vec![0u8; 20]; // version + bounding rectangle
        data.extend_from_slice(&ustr32("group b"));
        data.extend_from_slice(&7u32.to_be_bytes());

        let mut metadata = MetadataMap::new();
        PSDParser::parse_slice_info(&data, &mut metadata);

        assert_eq!(
            metadata.get("Photoshop:SlicesGroupName"),
            Some(&TagValue::String("group b".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:NumSlices"),
            Some(&TagValue::Integer(7))
        );
    }

    #[test]
    fn print_scale_info_prints_whole_floats_without_a_fraction() {
        // Photoshop.pm:494-512. ExifTool reports PrintScale 1.0 as "1"; a naive
        // float format would say "1" only by luck and "0" as "0" -- pinning both
        // keeps the formatting honest.
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_be_bytes()); // PrintStyle = Centered
        data.extend_from_slice(&0f32.to_be_bytes()); // PrintPosition x
        data.extend_from_slice(&0f32.to_be_bytes()); // PrintPosition y
        data.extend_from_slice(&1f32.to_be_bytes()); // PrintScale

        let mut metadata = MetadataMap::new();
        PSDParser::parse_print_scale_info(&data, &mut metadata);

        assert_eq!(
            metadata.get("Photoshop:PrintStyle"),
            Some(&TagValue::String("Centered".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:PrintPosition"),
            Some(&TagValue::String("0 0".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:PrintScale"),
            Some(&TagValue::String("1".to_string()))
        );
    }

    #[test]
    fn print_scale_info_keeps_a_fractional_scale() {
        let mut data = Vec::new();
        data.extend_from_slice(&2u16.to_be_bytes()); // User Defined
        data.extend_from_slice(&1.5f32.to_be_bytes());
        data.extend_from_slice(&(-2.25f32).to_be_bytes());
        data.extend_from_slice(&0.5f32.to_be_bytes());

        let mut metadata = MetadataMap::new();
        PSDParser::parse_print_scale_info(&data, &mut metadata);

        assert_eq!(
            metadata.get("Photoshop:PrintStyle"),
            Some(&TagValue::String("User Defined".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:PrintPosition"),
            Some(&TagValue::String("1.5 -2.25".to_string()))
        );
        assert_eq!(
            metadata.get("Photoshop:PrintScale"),
            Some(&TagValue::String("0.5".to_string()))
        );
    }

    #[test]
    fn url_list_reports_an_empty_list_rather_than_nothing() {
        // Photoshop.pm:226-247 -- ExifTool emits URL_List as a list even when
        // the count is zero, which is exactly the case in
        // combined-samples/Photoshop.psd.
        let mut metadata = MetadataMap::new();
        PSDParser::parse_url_list(&0u32.to_be_bytes(), &mut metadata);

        assert_eq!(
            metadata.get("Photoshop:URL_List"),
            Some(&TagValue::Array(Vec::new()))
        );
    }

    #[test]
    fn url_list_decodes_utf16_entries() {
        let mut data = 1u32.to_be_bytes().to_vec();
        data.extend_from_slice(&0u32.to_be_bytes()); // word
        data.extend_from_slice(&0u32.to_be_bytes()); // ID
        data.extend_from_slice(&ustr32("https://example.test/"));

        let mut metadata = MetadataMap::new();
        PSDParser::parse_url_list(&data, &mut metadata);

        assert_eq!(
            metadata.get("Photoshop:URL_List"),
            Some(&TagValue::Array(vec![TagValue::String(
                "https://example.test/".to_string()
            )]))
        );
    }

    #[test]
    fn layer_count_and_compression_come_from_their_own_sections() {
        // Photoshop.pm:746-798 and 689-702. Lay out a minimal tail: a
        // layer-and-mask section of 6 bytes (4-byte inner length + int16s
        // count), then the image data section.
        let layer_total: u32 = 6;
        let mut file = vec![0u8; 8];
        file.extend_from_slice(&layer_total.to_be_bytes()); // total length
        file.extend_from_slice(&2u32.to_be_bytes()); // layer info length
        file.extend_from_slice(&(-3i16).to_be_bytes()); // count, negative form
        file.extend_from_slice(&1u16.to_be_bytes()); // Compression = RLE

        let reader = BufferedReader::from_bytes(&file);
        let mut metadata = MetadataMap::new();
        PSDParser::parse_layers_and_image_data(&reader, 8, false, &mut metadata).unwrap();

        assert_eq!(
            metadata.get("Photoshop:LayerCount"),
            Some(&TagValue::Integer(3)),
            "a negative layer count means transparency data, not -3 layers \
             (Photoshop.pm:837)"
        );
        assert_eq!(
            metadata.get("Photoshop:Compression"),
            Some(&TagValue::String("RLE".to_string()))
        );
    }

    #[test]
    fn a_zero_length_layer_section_still_locates_the_compression_mode() {
        let mut file = vec![0u8; 8];
        file.extend_from_slice(&0u32.to_be_bytes()); // no layer and mask section
        file.extend_from_slice(&0u16.to_be_bytes()); // Compression = Uncompressed

        let reader = BufferedReader::from_bytes(&file);
        let mut metadata = MetadataMap::new();
        PSDParser::parse_layers_and_image_data(&reader, 8, false, &mut metadata).unwrap();

        assert!(
            metadata.get("Photoshop:LayerCount").is_none(),
            "there is no layer count to read when the section is empty"
        );
        assert_eq!(
            metadata.get("Photoshop:Compression"),
            Some(&TagValue::String("Uncompressed".to_string()))
        );
    }

    #[test]
    fn parse_reports_a_photo_mechanic_trailer_appended_after_the_psd() {
        // ProcessPSD hands the tail of the file to the trailer scanners
        // (Photoshop.pm:1226-1227); this exercises that wiring end to end
        // rather than re-testing photo_mechanic's own trailer parsing.
        let mut iptc = Vec::new();
        for (dataset, value) in [(221u8, 1i32), (222, 6), (216, 2)] {
            iptc.extend_from_slice(&[0x1c, 0x02, dataset]);
            iptc.extend_from_slice(&4u16.to_be_bytes());
            iptc.extend_from_slice(&value.to_be_bytes());
        }

        let mut file = psd_with_resources(&[]);
        file.extend_from_slice(&iptc);
        file.extend_from_slice(&(iptc.len() as u32).to_be_bytes());
        file.extend_from_slice(b"cbipcbbl");

        let reader = BufferedReader::from_bytes(&file);
        let metadata = PSDParser.parse(&reader).unwrap();

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
            Some(&TagValue::String("180".to_string()))
        );
    }

    /// Wraps resource blocks in the minimum PSD envelope the parser needs.
    fn psd_with_resources(resources: &[u8]) -> Vec<u8> {
        let mut file = b"8BPS".to_vec();
        file.extend_from_slice(&1u16.to_be_bytes()); // version: PSD
        file.extend_from_slice(&[0u8; 6]); // reserved
        file.extend_from_slice(&3u16.to_be_bytes()); // channels
        file.extend_from_slice(&8u32.to_be_bytes()); // height
        file.extend_from_slice(&8u32.to_be_bytes()); // width
        file.extend_from_slice(&8u16.to_be_bytes()); // depth
        file.extend_from_slice(&3u16.to_be_bytes()); // colour mode: RGB
        file.extend_from_slice(&0u32.to_be_bytes()); // colour mode data length
        file.extend_from_slice(&(resources.len() as u32).to_be_bytes());
        file.extend_from_slice(resources);
        file
    }

    /// Builds one 8BIM block with an empty name.
    fn irb(id: u16, data: &[u8]) -> Vec<u8> {
        let mut block = b"8BIM".to_vec();
        block.extend_from_slice(&id.to_be_bytes());
        block.extend_from_slice(&[0, 0]); // empty, padded Pascal name
        block.extend_from_slice(&(data.len() as u32).to_be_bytes());
        block.extend_from_slice(data);
        if !data.len().is_multiple_of(2) {
            block.push(0);
        }
        block
    }

    #[test]
    fn a_cleared_copyright_flag_is_reported_as_false() {
        // Photoshop.pm:171-181 gives 0x040a PrintConv { 0 => 'False',
        // 1 => 'True' } and ExifTool reports it whenever the resource is
        // present. The old code emitted a "Copyrighted = Yes" tag only for a
        // set flag, so a cleared flag -- the case in
        // combined-samples/Photoshop.psd -- produced nothing at all.
        let file = psd_with_resources(&irb(COPYRIGHT_FLAG, &[0]));
        let reader = BufferedReader::from_bytes(&file);
        let mut metadata = MetadataMap::new();
        PSDParser::parse_image_resources(&reader, &mut metadata).unwrap();

        assert_eq!(
            metadata.get("Photoshop:CopyrightFlag"),
            Some(&TagValue::String("False".to_string()))
        );
    }

    #[test]
    fn a_set_copyright_flag_is_reported_as_true() {
        let file = psd_with_resources(&irb(COPYRIGHT_FLAG, &[1]));
        let reader = BufferedReader::from_bytes(&file);
        let mut metadata = MetadataMap::new();
        PSDParser::parse_image_resources(&reader, &mut metadata).unwrap();

        assert_eq!(
            metadata.get("Photoshop:CopyrightFlag"),
            Some(&TagValue::String("True".to_string()))
        );
    }

    #[test]
    fn iptc_digest_and_url_are_read_from_their_resources() {
        let digest = [
            0xb7, 0x65, 0xe5, 0x43, 0x96, 0xf2, 0x88, 0x5e, 0x9a, 0x76, 0xff, 0x4a, 0xd6, 0x65,
            0xb1, 0x90,
        ];
        let mut resources = irb(IPTC_DIGEST, &digest);
        // ExifTool keeps the space padding Photoshop writes into the URL and
        // strips only NULs, so the padding must survive the parser.
        resources.extend_from_slice(&irb(URL, b"https://exiftool.org/   \0"));

        let file = psd_with_resources(&resources);
        let reader = BufferedReader::from_bytes(&file);
        let mut metadata = MetadataMap::new();
        PSDParser::parse_image_resources(&reader, &mut metadata).unwrap();

        assert_eq!(
            metadata.get("Photoshop:IPTCDigest"),
            Some(&TagValue::String(
                "b765e54396f2885e9a76ff4ad665b190".to_string()
            ))
        );
        assert_eq!(
            metadata.get("Photoshop:URL"),
            Some(&TagValue::String("https://exiftool.org/   ".to_string()))
        );
    }

    #[test]
    fn global_angle_and_altitude_are_read_as_integers() {
        let mut resources = irb(GLOBAL_ANGLE, &120u32.to_be_bytes());
        resources.extend_from_slice(&irb(GLOBAL_ALTITUDE, &30u32.to_be_bytes()));

        let file = psd_with_resources(&resources);
        let reader = BufferedReader::from_bytes(&file);
        let mut metadata = MetadataMap::new();
        PSDParser::parse_image_resources(&reader, &mut metadata).unwrap();

        assert_eq!(
            metadata.get("Photoshop:GlobalAngle"),
            Some(&TagValue::Integer(120))
        );
        assert_eq!(
            metadata.get("Photoshop:GlobalAltitude"),
            Some(&TagValue::Integer(30))
        );
    }

    #[test]
    fn application_record_version_is_a_number_not_its_raw_bytes() {
        // IPTC dataset 2:0 is Format => 'int16u'. Decoding it as text produced
        // the literal bytes "\0\x02" where ExifTool reports 2.
        let mut record = vec![0x1c, 0x02, 0x00];
        record.extend_from_slice(&2u16.to_be_bytes()); // length
        record.extend_from_slice(&2u16.to_be_bytes()); // value

        let mut metadata = MetadataMap::new();
        PSDParser::parse_iptc_data(&record, &mut metadata);

        assert_eq!(
            metadata.get("IPTC:ApplicationRecordVersion"),
            Some(&TagValue::Integer(2))
        );
    }

    #[test]
    fn read_unicode_string_returns_the_offset_after_the_value() {
        let mut data = vec![0xffu8; 3]; // leading padding
        data.extend_from_slice(&ustr32("ok"));
        let (text, next) = read_unicode_string(&data, 3).unwrap();
        assert_eq!(text, "ok");
        assert_eq!(next, 3 + 4 + 4);
    }

    #[test]
    fn read_unicode_string_rejects_a_truncated_value() {
        // A count that runs past the buffer must not panic or return junk.
        let mut data = 9u32.to_be_bytes().to_vec();
        data.extend_from_slice(&[0x00, 0x61]);
        assert_eq!(read_unicode_string(&data, 0), None);
    }

    /// Exif.pm 13.59:3849-3873 through resource 0x0422: an all-NUL
    /// PanasonicTitle creates no tag, a NUL-padded PanasonicTitle2 prints
    /// exactly, and Make loses its trailing blank (Exif.pm:585) -- the same
    /// rows the same TIFF block prints inside a JPEG.
    #[test]
    fn exif_resource_panasonic_title_rawconv_matches_jpeg_path() {
        use crate::parsers::image::embedded::test_fixtures::{
            assert_block_a, assert_block_b, panasonic_title_block_a, panasonic_title_block_b,
        };

        let file = psd_with_resources(&irb(EXIF_DATA_1, &panasonic_title_block_a()));
        let mut metadata = MetadataMap::new();
        PSDParser::parse_image_resources(&BufferedReader::from_bytes(&file), &mut metadata)
            .unwrap();
        assert_block_a(&metadata);
        assert_eq!(metadata.get("IFD0:ExifOffset"), None);

        let file = psd_with_resources(&irb(EXIF_DATA_1, &panasonic_title_block_b()));
        let mut metadata = MetadataMap::new();
        PSDParser::parse_image_resources(&BufferedReader::from_bytes(&file), &mut metadata)
            .unwrap();
        assert_block_b(&metadata);

        // Resource 0x0423 (ExifInfo2) is `Unknown => 1, Binary => 1` in
        // Photoshop.pm 13.59:262: ExifTool does not parse it as EXIF without
        // -u, so no IFD0 row may come from it.
        let file = psd_with_resources(&irb(EXIF_DATA_3, &panasonic_title_block_a()));
        let mut metadata = MetadataMap::new();
        PSDParser::parse_image_resources(&BufferedReader::from_bytes(&file), &mut metadata)
            .unwrap();
        assert!(
            metadata.keys().all(|k| !k.starts_with("IFD0:")),
            "0x0423 must not be parsed as EXIF, got {:?}",
            metadata.keys().collect::<Vec<_>>()
        );
    }

    /// A complete PSD carrier whose IFD0 contains one readable width and
    /// optionally an entry that parse_ifd must skip. The physical directory
    /// still includes that entry before its next-IFD pointer.
    fn psd_with_ifd1_after_ifd0(skipped: Option<(u16, u16, u32, u32)>) -> Vec<u8> {
        fn entry(data: &mut Vec<u8>, tag: u16, kind: u16, count: u32, value: u32) {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&kind.to_le_bytes());
            data.extend_from_slice(&count.to_le_bytes());
            data.extend_from_slice(&value.to_le_bytes());
        }

        let count = 1 + u16::from(skipped.is_some());
        let ifd1_offset = 8 + 2 + 12 * u32::from(count) + 4;
        let mut tiff = b"II".to_vec();
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&count.to_le_bytes());
        entry(&mut tiff, 0x0100, 3, 1, 8); // ImageWidth, SHORT
        if let Some((tag, kind, count, value)) = skipped {
            entry(&mut tiff, tag, kind, count, value);
        }
        tiff.extend_from_slice(&ifd1_offset.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        entry(&mut tiff, 0x0103, 3, 1, 6); // Compression
        entry(&mut tiff, 0x0128, 3, 1, 2); // ResolutionUnit
        entry(&mut tiff, 0x013b, 2, 3, u32::from_le_bytes(*b"Ab\0\0"));
        tiff.extend_from_slice(&0u32.to_le_bytes());

        let mut file = psd_with_resources(&irb(EXIF_DATA_1, &tiff));
        file.extend_from_slice(&0u32.to_be_bytes()); // no layer/mask data
        file.extend_from_slice(&0u16.to_be_bytes()); // uncompressed pixels
        file.extend_from_slice(&[0u8; 8 * 8 * 3]);
        file
    }

    fn assert_psd_ifd1_facts(skipped: Option<(u16, u16, u32, u32)>) {
        let file = psd_with_ifd1_after_ifd0(skipped);
        let metadata = PSDParser
            .parse(&BufferedReader::from_bytes(&file))
            .expect("complete PSD carrier must parse");
        assert_eq!(metadata.get_integer("IFD0:ImageWidth"), Some(8));
        assert!(metadata.get("IFD0:ImageDescription").is_none());

        // The explicit pinned 13.59 oracle reports these three facts for
        // all three carriers, including after the skipped IFD0 entries.
        // Compression is handled by the thumbnail pass; the other two by
        // the ordinary IFD1 pass, so both routes must find the same IFD1.
        for (key, expected) in [
            ("IFD1:Compression", "JPEG (old-style)"),
            ("IFD1:ResolutionUnit", "inches"),
            ("IFD1:Artist", "Ab"),
        ] {
            let raw = metadata.get(key).unwrap_or_else(|| panic!("missing {key}"));
            let printed = crate::core::exiftool_compat::format_tag_value(key, raw);
            assert_eq!(printed.as_string(), Some(expected), "{key}");
        }
    }

    #[test]
    fn exif_resource_ifd1_follows_a_clean_ifd0() {
        assert_psd_ifd1_facts(None);
    }

    #[test]
    fn exif_resource_ifd1_survives_a_type_zero_ifd0_entry() {
        // Type 0 padding is skipped without consuming the warning budget
        // (Exif.pm 13.59:6470), but still occupies twelve physical bytes.
        assert_psd_ifd1_facts(Some((0, 0, 0, 0)));
    }

    #[test]
    fn exif_resource_ifd1_survives_a_bad_offset_ifd0_entry() {
        // This out-of-line string runs beyond the TIFF block. ExifTool
        // warns about it and continues to the unaffected IFD1.
        assert_psd_ifd1_facts(Some((0x010e, 2, 10, 10_000)));
    }

    /// Photoshop.pm 13.59:254-259 hands resource 0x0422 to ProcessTIFF as a
    /// self-contained block, so the offsets reported from it stay
    /// block-relative (the oracle prints 1000 for this resource) -- the same
    /// rule that puts ThumbnailOffset at 390 for Photoshop.psd.
    #[test]
    fn exif_resource_offsets_stay_block_relative() {
        use crate::parsers::image::embedded::test_fixtures::{
            assert_other_image_start, interop_offset_block,
        };

        let file = psd_with_resources(&irb(EXIF_DATA_1, &interop_offset_block()));
        let mut metadata = MetadataMap::new();
        PSDParser::parse_image_resources(&BufferedReader::from_bytes(&file), &mut metadata)
            .unwrap();
        assert_other_image_start(&metadata, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tag_db::lookup_tag_name;

    /// A little-endian TIFF block with an empty IFD0 chained to an IFD1
    /// carrying the given inline `(tag, field_type, value)` entries -- the
    /// shape Photoshop.psd stores its thumbnail directory in.
    fn tiff_with_ifd1(entries: &[(u16, u16, [u8; 4])]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(b"II");
        data.extend_from_slice(&42u16.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        // Empty IFD0 followed by its next-IFD pointer.
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&14u32.to_le_bytes());
        data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (tag, field_type, value) in entries {
            data.extend_from_slice(&tag.to_le_bytes());
            data.extend_from_slice(&field_type.to_le_bytes());
            data.extend_from_slice(&1u32.to_le_bytes());
            data.extend_from_slice(value);
        }
        data.extend_from_slice(&0u32.to_le_bytes());
        data
    }

    /// IFD1 = Compression (SHORT) 6, ResolutionUnit (SHORT) 2,
    /// JPEGInterchangeFormat (LONG) 390 and JPEGInterchangeFormatLength
    /// (LONG) 0: the IFD1 of combined-samples/Photoshop.psd minus its two
    /// rational resolution entries (which need an out-of-line payload).
    fn photoshop_psd_ifd1() -> Vec<u8> {
        tiff_with_ifd1(&[
            (0x0103, 3, [6, 0, 0, 0]),
            (0x0128, 3, [2, 0, 0, 0]),
            (0x0201, 4, 390u32.to_le_bytes()),
            (0x0202, 4, 0u32.to_le_bytes()),
        ])
    }

    #[test]
    fn real_psd_ifd1_occurrences_follow_native_directory_order() {
        let Some(reader) = crate::test_support::pinned_fixture_reader("Photoshop.psd") else {
            return;
        };
        let metadata = PSDParser.parse(&reader).expect("pinned PSD parses");
        let keys: Vec<_> = metadata
            .all_occurrences()
            .map(|(key, _)| key)
            .filter(|key| key.starts_with("IFD1:"))
            .collect();
        // Native 13.59 and the pre-consolidation control agree in both
        // -a -G0:1 -s and numeric mode. Aggregate JSON scores cannot detect
        // moving the thumbnail pair before the three resolution fields.
        assert_eq!(
            keys,
            [
                "IFD1:Compression",
                "IFD1:XResolution",
                "IFD1:YResolution",
                "IFD1:ResolutionUnit",
                "IFD1:ThumbnailOffset",
                "IFD1:ThumbnailLength",
            ]
        );
    }

    #[test]
    fn parses_compression_from_embedded_exif_ifd1() {
        let mut metadata = MetadataMap::new();
        PSDParser::parse_exif_data(&photoshop_psd_ifd1(), &mut metadata);

        // The raw value; `JPEG (old-style)` is the Exif::Main PrintConv that
        // exiftool_compat applies at output, exactly as on the JPEG path.
        let tag_name = lookup_tag_name(0x0103, "IFD1");
        assert_eq!(metadata.get(&tag_name), Some(&TagValue::Integer(6)));
        // ProcessTIFF walks IFD1 in full: `exiftool -G1 -a` prints
        // `[IFD1] ResolutionUnit : inches` for Photoshop.psd, and the
        // lossless conformance view scores it as a row of its own.
        assert_eq!(
            metadata.get("IFD1:ResolutionUnit"),
            Some(&TagValue::Integer(2))
        );
        // The sub-IFD pointer is structural: Exif.pm 13.59:7103-7104 stores
        // no tag for a SubDirectory entry in a default dump.
        assert_eq!(metadata.get("IFD0:ExifOffset"), None);
    }

    #[test]
    fn parses_thumbnail_offset_from_embedded_exif_ifd1() {
        // ExifTool names 0x0201 ThumbnailOffset inside IFD1 (Exif.pm:1149)
        // and reports it relative to the block's own TIFF header, so 390 as
        // stored -- `exiftool -G1 -a -s` on Photoshop.psd prints
        // `[IFD1] ThumbnailOffset : 390`.
        let mut metadata = MetadataMap::new();
        PSDParser::parse_exif_data(&photoshop_psd_ifd1(), &mut metadata);

        assert_eq!(
            metadata.get("IFD1:ThumbnailOffset"),
            Some(&TagValue::Integer(390))
        );
        assert_eq!(metadata.get("IFD0:ExifOffset"), None);
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
        // The key is asserted as a LITERAL so the test cannot pass by agreeing
        // with whatever a lookup function returns. A zero length is not a
        // thumbnail, so no ThumbnailImage is emitted either (ExifTool reports
        // none for the four corpus files with Length 0).
        let mut metadata = MetadataMap::new();
        PSDParser::parse_exif_data(&photoshop_psd_ifd1(), &mut metadata);

        assert_eq!(
            metadata.get("IFD1:ThumbnailLength"),
            Some(&TagValue::Integer(0)),
            "0x0202 in IFD1 must be named ThumbnailLength (Exif.pm:1297)"
        );
        assert!(
            metadata.get("IFD1:0x0202").is_none(),
            "0x0202 must not also survive under its unnamed hex fallback"
        );
        assert_eq!(metadata.get("IFD1:ThumbnailImage"), None);
        assert_eq!(metadata.get("IFD0:ExifOffset"), None);
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
        // parse_embedded_exif_at routes IFD0 through lookup_tag_name(id, "IFD0")
        // and only the IFD1 collector (`tiff_helpers::collect_ifd1_thumbnail`) names the IFD1 pair, so this pins the
        // scope of the fix at the call site rather than trusting the branch to
        // stay put.
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
