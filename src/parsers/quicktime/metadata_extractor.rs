//! QuickTime/MP4 metadata extraction.
//!
//! This module extracts metadata from QuickTime and MP4 files from various locations:
//! 1. Classic QuickTime user data atoms (©xxx in moov→udta)
//! 2. iTunes-style metadata (moov→udta→meta)
//! 3. MP4 metadata with keys/ilst (moov→meta→keys + moov→meta→ilst)
//! 4. XMP metadata in uuid atoms
//! 5. HEIF/HEIC EXIF data from meta→iinf/iloc referencing mdat

use super::atom_parser::Atom;
use super::tag_mapping::atom_to_exiftool_tag;
use crate::core::{FileReader, MetadataMap, TagValue};
use crate::io::timestamp::mac_time_to_iso8601;
use crate::io::{ByteOrder, EndianReader};
use crate::parsers::tiff::ifd_parser::{ByteOrder as TiffByteOrder, parse_ifd};
use crate::tag_db::lookup_tag_name;
use std::borrow::Cow;
use std::collections::HashMap;
use std::io;

/// Helper function to compute rational value as f64
/// Used when we need the computed float value rather than the (num, denom) tuple
fn rational_to_f64(reader: &EndianReader, offset: usize) -> Option<f64> {
    let (num, den) = reader.rational_at(offset)?;
    if den != 0 {
        Some(num as f64 / den as f64)
    } else {
        None
    }
}

/// Helper function to compute signed rational value as f64
fn srational_to_f64(reader: &EndianReader, offset: usize) -> Option<f64> {
    let (num, den) = reader.srational_at(offset)?;
    if den != 0 {
        Some(num as f64 / den as f64)
    } else {
        None
    }
}

/// Convert TiffByteOrder to the crate's ByteOrder type
fn tiff_to_byte_order(order: TiffByteOrder) -> ByteOrder {
    match order {
        TiffByteOrder::LittleEndian => ByteOrder::Little,
        TiffByteOrder::BigEndian => ByteOrder::Big,
    }
}

/// Extract metadata from a single track atom
///
/// Returns Err if required container atoms (mdia, minf, stbl) are missing.
/// Callers should ignore errors to preserve original behavior of skipping
/// incomplete tracks rather than failing the entire extraction.
fn extract_track_metadata(
    trak: &Atom,
    metadata: &mut MetadataMap,
    index: usize,
) -> Result<(), String> {
    // Extract track header - optional
    if let Some(tkhd) = trak.find_child("tkhd") {
        let _ = extract_track_header(&tkhd, metadata, index);
    }

    // Extract track aperture mode dimensions (tapt) - optional
    // Contains clef (clean aperture), prof (production aperture), enof (encoded pixels)
    if let Some(tapt) = trak.find_child("tapt") {
        let _ = extract_track_aperture(&tapt, metadata, index);
    }

    // Media container - required for further extraction
    // Uses ok_or_else() to convert Option to Result, enabling ? operator
    let mdia = trak
        .find_child("mdia")
        .ok_or_else(|| "missing mdia atom".to_string())?;

    // Extract media header - optional
    if let Some(mdhd) = mdia.find_child("mdhd") {
        let _ = extract_media_header(&mdhd, metadata, index);
    }

    // Media information - required for sample table access
    let minf = mdia
        .find_child("minf")
        .ok_or_else(|| "missing minf atom".to_string())?;

    // Extract video media header (vmhd) - optional, contains GraphicsMode and OpColor
    if let Some(vmhd) = minf.find_child("vmhd") {
        let _ = extract_video_media_header(&vmhd, metadata, index);
    }

    // Check if this is an audio track (has smhd)
    let is_audio_track = minf.find_child("smhd").is_some();

    // Extract sound media header (smhd) - optional, contains Balance
    if let Some(smhd) = minf.find_child("smhd") {
        let _ = extract_sound_media_header(&smhd, metadata, index);
    }

    // Extract handler reference from dinf (data information) container - contains HandlerClass
    // ExifTool only extracts HandlerClass for audio tracks
    if is_audio_track {
        if let Some(dinf) = minf.find_child("dinf")
            && let Some(dref) = dinf.find_child("dref")
        {
            let _ = extract_data_handler_info(dref.data, metadata, index);
        }

        // Also check for hdlr directly in minf (some formats use this)
        if let Some(hdlr) = minf.find_child("hdlr") {
            let _ = extract_track_handler_metadata(&hdlr, metadata, index);
        }
    }

    // Sample table - required for sample descriptions
    let stbl = minf
        .find_child("stbl")
        .ok_or_else(|| "missing stbl atom".to_string())?;

    // Extract sample description - optional
    if let Some(stsd) = stbl.find_child("stsd") {
        let _ = extract_sample_description(&stsd, metadata, index);
    }

    // Extract video frame rate from stts (sample-to-time) atom
    // VideoFrameRate = MediaTimeScale * SampleCount / TotalDuration
    if let Some(stts) = stbl.find_child("stts") {
        let _ = extract_video_frame_rate(&stts, metadata, index);
    }

    Ok(())
}

/// Extract video media header (vmhd) metadata
/// Contains GraphicsMode and OpColor for video tracks
fn extract_video_media_header(
    vmhd: &Atom,
    metadata: &mut MetadataMap,
    track_index: usize,
) -> Result<(), String> {
    // vmhd atom structure:
    // 1 byte version
    // 3 bytes flags
    // 2 bytes graphics mode
    // 6 bytes opcolor (3 x 2-byte RGB values)
    if vmhd.data.len() < 12 {
        return Ok(());
    }

    let r = EndianReader::big_endian(vmhd.data);

    let track_suffix = if track_index > 0 {
        format!("_{}", track_index + 1)
    } else {
        String::new()
    };

    // Graphics mode (transfer mode for compositing)
    if let Some(mode) = r.u16_at(4) {
        let mode_name = match mode {
            0x00 => "srcCopy",
            0x01 => "srcOr",
            0x02 => "srcXor",
            0x03 => "srcBic",
            0x04 => "notSrcCopy",
            0x05 => "notSrcOr",
            0x06 => "notSrcXor",
            0x07 => "notSrcBic",
            0x08 => "patCopy",
            0x09 => "patOr",
            0x0A => "patXor",
            0x0B => "patBic",
            0x0C => "notPatCopy",
            0x0D => "notPatOr",
            0x0E => "notPatXor",
            0x0F => "notPatBic",
            0x20 => "blend",
            0x21 => "addPin",
            0x22 => "addOver",
            0x23 => "subPin",
            0x24 => "transparent",
            0x25 => "addMax",
            0x26 => "subOver",
            0x27 => "addMin",
            0x40 => "ditherCopy",
            0x100 => "alpha",
            0x101 => "straightAlpha",
            0x102 => "premulWhiteAlpha",
            0x103 => "premulBlackAlpha",
            0x104 => "straightAlphaBlend",
            0x110 => "compositeOffer",
            _ => "unknown",
        };
        metadata.insert(
            format!("QuickTime:GraphicsMode{}", track_suffix),
            TagValue::String(mode_name.to_string()),
        );
    }

    // OpColor (RGB values used for certain graphics modes)
    if vmhd.data.len() >= 12 {
        let red = r.u16_at(6).unwrap_or(0);
        let green = r.u16_at(8).unwrap_or(0);
        let blue = r.u16_at(10).unwrap_or(0);
        metadata.insert(
            format!("QuickTime:OpColor{}", track_suffix),
            TagValue::String(format!("{} {} {}", red, green, blue)),
        );
    }

    Ok(())
}

/// Extract sound media header (smhd) metadata
/// Contains Balance for audio tracks
fn extract_sound_media_header(
    smhd: &Atom,
    metadata: &mut MetadataMap,
    track_index: usize,
) -> Result<(), String> {
    // smhd atom structure:
    // 1 byte version
    // 3 bytes flags
    // 2 bytes balance (fixed-point 8.8, -1.0 = full left, 0 = center, +1.0 = full right)
    // 2 bytes reserved
    if smhd.data.len() < 8 {
        return Ok(());
    }

    let r = EndianReader::big_endian(smhd.data);

    let track_suffix = if track_index > 0 {
        format!("_{}", track_index + 1)
    } else {
        String::new()
    };

    // Balance (fixed-point 8.8)
    if let Some(balance_raw) = r.i16_at(4) {
        let balance = balance_raw as f64 / 256.0;
        // ExifTool outputs balance as an integer (0 for center)
        metadata.insert(
            format!("QuickTime:Balance{}", track_suffix),
            TagValue::Integer(balance as i64),
        );
    }

    Ok(())
}

/// Extract track aperture mode dimensions (tapt atom)
/// Contains clef (clean aperture), prof (production aperture), enof (encoded pixels)
fn extract_track_aperture(
    tapt: &Atom,
    metadata: &mut MetadataMap,
    track_index: usize,
) -> Result<(), String> {
    let children = tapt.parse_children().unwrap_or_default();

    let track_suffix = if track_index > 0 {
        format!("_{}", track_index + 1)
    } else {
        String::new()
    };

    for atom in children {
        let atom_type = atom.atom_type.as_str();
        // Each aperture atom has: version (1), flags (3), width (4), height (4)
        // Width and height are 16.16 fixed-point values
        if atom.data.len() >= 12 {
            let r = EndianReader::big_endian(atom.data);
            // Fixed-point 16.16: width/height are stored as integers, interpret high 16 bits
            let width_fp = r.u32_at(4).unwrap_or(0);
            let height_fp = r.u32_at(8).unwrap_or(0);
            // Convert 16.16 fixed-point to integer by taking upper 16 bits
            let width = width_fp >> 16;
            let height = height_fp >> 16;

            if width > 0 && height > 0 {
                let dimensions = format!("{}x{}", width, height);
                let tag_name = match atom_type {
                    "clef" => "QuickTime:CleanApertureDimensions",
                    "prof" => "QuickTime:ProductionApertureDimensions",
                    "enof" => "QuickTime:EncodedPixelsDimensions",
                    _ => continue,
                };
                metadata.insert(
                    format!("{}{}", tag_name, track_suffix),
                    TagValue::String(dimensions),
                );
            }
        }
    }

    Ok(())
}

/// Extract all metadata from QuickTime/MP4 atoms
pub fn extract_metadata(root_atoms: &[Atom]) -> Result<MetadataMap, String> {
    let mut metadata = MetadataMap::with_capacity(50);

    // Extract file-level metadata from ftyp and mdat atoms
    extract_file_level_metadata(root_atoms, &mut metadata);

    // Find the moov atom (movie container) - optional for HEIF/HIF files
    let moov = root_atoms
        .iter()
        .find(|atom| atom.atom_type.matches("moov"));

    // If we have a moov atom, extract traditional QuickTime/MP4 metadata
    if let Some(moov) = moov {
        // Extract movie header metadata (mvhd)
        if let Some(mvhd) = moov.find_child("mvhd") {
            extract_movie_header(&mvhd, &mut metadata)?;
        }

        // Extract track headers (tkhd) from all trak atoms
        if let Ok(children) = moov.parse_children() {
            let trak_atoms: Vec<_> = children
                .iter()
                .filter(|a| a.atom_type.matches("trak"))
                .collect();

            for (index, trak) in trak_atoms.iter().enumerate() {
                // Ignore errors - missing atoms in a track should not prevent
                // processing other tracks (preserves original behavior)
                let _ = extract_track_metadata(trak, &mut metadata, index);
            }
        }

        // Extract from all possible locations
        if let Some(udta) = moov.find_child("udta") {
            // Extract handler metadata (hdlr) - may be in udta or udta→meta
            if let Some(meta) = udta.find_child("meta") {
                // Parse meta children (skip version/flags)
                let meta_data = if meta.data.len() >= 4 && meta.data[0..4] == [0, 0, 0, 0] {
                    &meta.data[4..]
                } else {
                    meta.data
                };

                if let Ok((_, atoms)) = super::atom_parser::parse_atoms(meta_data)
                    && let Some(hdlr) = atoms.iter().find(|a| a.atom_type.matches("hdlr"))
                {
                    extract_handler_metadata(hdlr, &mut metadata)?;
                }
            }

            // Also check for hdlr directly in udta
            if let Some(hdlr) = udta.find_child("hdlr") {
                extract_handler_metadata(&hdlr, &mut metadata)?;
            }
            // Extract classic QuickTime user data (©xxx atoms)
            extract_user_data_atoms(&udta, &mut metadata)?;

            // Extract iTunes-style metadata (udta→meta)
            if let Some(meta) = udta.find_child("meta") {
                extract_itunes_metadata(&meta, &mut metadata)?;
            }
        }

        // Extract MP4 metadata (moov→meta with keys/ilst)
        if let Some(meta) = moov.find_child("meta") {
            extract_mp4_metadata(&meta, &mut metadata)?;
        }
    }

    // HEIF/HIF files have a root-level meta atom instead of moov
    // Extract metadata from root-level meta atom if present
    if let Some(meta) = root_atoms.iter().find(|a| a.atom_type.matches("meta")) {
        // Extract handler metadata from root-level meta
        let meta_data = if meta.data.len() >= 4 && meta.data[0..4] == [0, 0, 0, 0] {
            &meta.data[4..]
        } else {
            meta.data
        };

        if let Ok((_, atoms)) = super::atom_parser::parse_atoms(meta_data)
            && let Some(hdlr) = atoms.iter().find(|a| a.atom_type.matches("hdlr"))
        {
            extract_handler_metadata(hdlr, &mut metadata)?;
        }

        // Extract HEIF-specific metadata (iinf, iloc, etc.) including EXIF data
        extract_heif_metadata(meta, root_atoms, &mut metadata)?;
    }

    // If no metadata was extracted, return error
    if metadata.is_empty() {
        Err("No metadata found in QuickTime/MP4 file".to_string())
    } else {
        Ok(metadata)
    }
}

/// Extract file-level metadata from ftyp and mdat atoms
fn extract_file_level_metadata(root_atoms: &[Atom], metadata: &mut MetadataMap) {
    // Extract file type information from ftyp atom
    if let Some(ftyp) = root_atoms.iter().find(|a| a.atom_type.matches("ftyp"))
        && ftyp.data.len() >= 8
    {
        // Major brand (4 bytes)
        let brand_bytes = &ftyp.data[0..4];
        if let Ok(brand) = std::str::from_utf8(brand_bytes) {
            let brand_desc = match brand {
                "isom" => "MP4 Base Media v1 [IS0 14496-12:2003]",
                "iso2" => "MP4 Base Media v2",
                "mp41" => "MP4 v1 [ISO 14496-1:ch13]",
                "mp42" => "MP4 v2 [ISO 14496-14]",
                "M4A " | "M4B " => "Apple iTunes AAC-LC (.M4A) Audio",
                "M4V " => "Apple iTunes Video (.M4V) Video",
                "qt  " => "Apple QuickTime (.MOV/QT)",
                "mp4 " => "MP4 Base Media v1 [IS0 14496-12:2003]",
                // HEIF/HEIC brands
                "mif1" => "High Efficiency Image Format still image (.HEIC)",
                "msf1" => "High Efficiency Image Format sequence (.HEICS)",
                "heic" => "High Efficiency Image Coding (.HEIC)",
                "heix" => "High Efficiency Image Coding (.HEIC)",
                "hevc" => "High Efficiency Video Coding (.HEVC)",
                "hevx" => "High Efficiency Video Coding (.HEVC)",
                "heim" => "High Efficiency Image Coding Multiview",
                "heis" => "High Efficiency Image Coding Scalable",
                "hevm" => "High Efficiency Video Coding Multiview",
                "hevs" => "High Efficiency Video Coding Scalable",
                "avif" => "AV1 Image File Format (.AVIF)",
                "avis" => "AV1 Image Sequence File Format",
                _ => brand,
            };
            metadata.insert(
                "QuickTime:MajorBrand".to_string(),
                TagValue::String(brand_desc.to_string()),
            );
        }

        // Minor version (4 bytes)
        if ftyp.data.len() >= 8 {
            let r = EndianReader::big_endian(ftyp.data);
            let minor_version = r.u32_at(4).unwrap_or(0);
            let version_str = format!(
                "{}.{}.{}",
                (minor_version >> 16) & 0xFF,
                (minor_version >> 8) & 0xFF,
                minor_version & 0xFF
            );
            metadata.insert(
                "QuickTime:MinorVersion".to_string(),
                TagValue::String(version_str),
            );
        }

        // Compatible brands (remaining bytes, each 4 bytes)
        if ftyp.data.len() > 8 {
            let mut compatible_brands = Vec::new();
            let mut offset = 8;
            while offset + 4 <= ftyp.data.len() {
                if let Ok(brand) = std::str::from_utf8(&ftyp.data[offset..offset + 4]) {
                    compatible_brands.push(TagValue::String(brand.to_string()));
                }
                offset += 4;
            }
            if !compatible_brands.is_empty() {
                metadata.insert(
                    "QuickTime:CompatibleBrands".to_string(),
                    TagValue::Array(compatible_brands),
                );
            }
        }
    }

    // Extract media data offset and size from mdat atom
    // We need to track position in the original file
    let mut offset = 0u64;
    for atom in root_atoms {
        if atom.atom_type.matches("mdat") {
            metadata.insert(
                "QuickTime:MediaDataSize".to_string(),
                TagValue::Integer(atom.data.len() as i64),
            );
            metadata.insert(
                "QuickTime:MediaDataOffset".to_string(),
                TagValue::Integer((offset + atom.header_size as u64) as i64),
            );
            break;
        }
        // Calculate atom size (header + data length), accounting for extended headers
        offset += atom.header_size as u64 + atom.data.len() as u64;
    }
}

/// Extract movie header metadata from mvhd atom
fn extract_movie_header(mvhd: &Atom, metadata: &mut MetadataMap) -> Result<(), String> {
    if mvhd.data.len() < 100 {
        return Ok(());
    }

    let r = EndianReader::big_endian(mvhd.data);
    let version = mvhd.data[0];

    // Parse time fields based on version (v0: 32-bit, v1: 64-bit)
    let (creation_time, modification_time, timescale, duration, rate_offset) = if version == 1 {
        if r.len() < 32 {
            return Ok(());
        }
        (
            r.u64_at(4).unwrap_or(0),
            r.u64_at(12).unwrap_or(0),
            r.u32_at(20).unwrap_or(0),
            r.u64_at(24).unwrap_or(0),
            32usize,
        )
    } else {
        (
            r.u32_at(4).unwrap_or(0) as u64,
            r.u32_at(8).unwrap_or(0) as u64,
            r.u32_at(12).unwrap_or(0),
            r.u32_at(16).unwrap_or(0) as u64,
            20usize,
        )
    };

    metadata.insert(
        "QuickTime:MovieHeaderVersion".to_string(),
        TagValue::Integer(version as i64),
    );

    // Add both legacy CreateDate/ModifyDate and new MediaCreateDate/MediaModifyDate
    // Use shared timestamp utility for dates after 1970, fallback to legacy for older dates
    let create_date_str =
        mac_time_to_iso8601(creation_time).unwrap_or_else(|| format_mac_time_legacy(creation_time));
    let modify_date_str = mac_time_to_iso8601(modification_time)
        .unwrap_or_else(|| format_mac_time_legacy(modification_time));

    metadata.insert(
        "QuickTime:CreateDate".to_string(),
        TagValue::String(create_date_str.clone()),
    );
    metadata.insert(
        "QuickTime:MediaCreateDate".to_string(),
        TagValue::String(create_date_str),
    );
    metadata.insert(
        "QuickTime:ModifyDate".to_string(),
        TagValue::String(modify_date_str.clone()),
    );
    metadata.insert(
        "QuickTime:MediaModifyDate".to_string(),
        TagValue::String(modify_date_str),
    );
    metadata.insert(
        "QuickTime:TimeScale".to_string(),
        TagValue::Integer(timescale as i64),
    );

    let duration_sec = if timescale > 0 {
        duration as f64 / timescale as f64
    } else {
        0.0
    };
    metadata.insert(
        "QuickTime:Duration".to_string(),
        TagValue::String(format!("{:.2} s", duration_sec)),
    );

    // Preferred rate (fixed-point 16.16)
    if let Some(rate) = r.i32_at(rate_offset) {
        metadata.insert(
            "QuickTime:PreferredRate".to_string(),
            TagValue::Integer((rate as f64 / 65536.0) as i64),
        );
    }

    // Preferred volume (fixed-point 8.8)
    if let Some(volume) = r.i16_at(rate_offset + 4) {
        metadata.insert(
            "QuickTime:PreferredVolume".to_string(),
            TagValue::String(format!("{:.2}%", (volume as f64 / 256.0) * 100.0)),
        );
    }

    // Matrix structure (9 x 4 bytes)
    let matrix_offset = if version == 1 { rate_offset + 16 } else { 36 };
    if r.len() >= matrix_offset + 36 {
        let matrix: Vec<i32> = (0..9)
            .filter_map(|i| r.i32_at(matrix_offset + i * 4))
            .collect();
        if matrix.len() == 9 {
            let matrix_str = format!(
                "{} {} {} {} {} {} {} {} {}",
                matrix[0] / 65536,
                matrix[1] / 65536,
                matrix[2] / 65536,
                matrix[3] / 65536,
                matrix[4] / 65536,
                matrix[5] / 65536,
                matrix[6] / 1073741824,
                matrix[7] / 1073741824,
                matrix[8] / 1073741824
            );
            metadata.insert(
                "QuickTime:MatrixStructure".to_string(),
                TagValue::String(matrix_str),
            );
        }
    }

    // Time fields (preview, poster, selection, current)
    let time_offset = if version == 1 { rate_offset + 52 } else { 72 };
    let ts = timescale.max(1);
    let time_fields = [
        ("PreviewTime", 0),
        ("PreviewDuration", 4),
        ("PosterTime", 8),
        ("SelectionTime", 12),
        ("SelectionDuration", 16),
        ("CurrentTime", 20),
    ];
    for (name, offset) in time_fields {
        if let Some(val) = r.u32_at(time_offset + offset) {
            metadata.insert(
                format!("QuickTime:{}", name),
                TagValue::String(format!("{} s", val / ts)),
            );
        }
    }

    // Next track ID
    let next_track_offset = if version == 1 { time_offset + 24 } else { 96 };
    if let Some(next_track_id) = r.u32_at(next_track_offset) {
        metadata.insert(
            "QuickTime:NextTrackID".to_string(),
            TagValue::Integer(next_track_id as i64),
        );
    }

    Ok(())
}

/// Extract track header metadata from tkhd atom
fn extract_track_header(
    tkhd: &Atom,
    metadata: &mut MetadataMap,
    track_index: usize,
) -> Result<(), String> {
    if tkhd.data.len() < 84 {
        return Ok(());
    }

    let r = EndianReader::big_endian(tkhd.data);
    let version = tkhd.data[0];
    let flags = [tkhd.data[1], tkhd.data[2], tkhd.data[3]];

    // Parse time fields based on version (v0: 32-bit, v1: 64-bit)
    // tkhd v0 layout: version(1) flags(3) create(4) modify(4) trackID(4) reserved(4) duration(4)
    //                 reserved(8) layer(2) altGroup(2) volume(2) reserved(2) matrix(36) width(4) height(4)
    // tkhd v1 layout: version(1) flags(3) create(8) modify(8) trackID(4) reserved(4) duration(8)
    //                 reserved(8) layer(2) altGroup(2) volume(2) reserved(2) matrix(36) width(4) height(4)
    let (creation_time, modification_time, track_id, duration, volume_offset, width_offset) =
        if version == 1 {
            if r.len() < 36 {
                return Ok(());
            }
            (
                r.u64_at(4).unwrap_or(0),
                r.u64_at(12).unwrap_or(0),
                r.u32_at(20).unwrap_or(0),
                r.u64_at(28).unwrap_or(0),
                46usize, // layer(2) + altGroup(2) after duration
                76usize,
            )
        } else {
            (
                r.u32_at(4).unwrap_or(0) as u64,
                r.u32_at(8).unwrap_or(0) as u64,
                r.u32_at(12).unwrap_or(0),
                r.u32_at(20).unwrap_or(0) as u64,
                36usize, // layer at offset 32, altGroup at 34, volume at 36
                60usize,
            )
        };

    // Use track index for tag names if we have multiple tracks
    let track_suffix = if track_index > 0 {
        format!("_{}", track_index + 1)
    } else {
        String::new()
    };

    // Track header version (ExifTool outputs this)
    metadata.insert(
        format!("QuickTime:TrackHeaderVersion{}", track_suffix),
        TagValue::Integer(version as i64),
    );

    // Add track-specific timestamp tags
    // Use shared timestamp utility for dates after 1970, fallback to legacy for older dates
    let create_date_str =
        mac_time_to_iso8601(creation_time).unwrap_or_else(|| format_mac_time_legacy(creation_time));
    let modify_date_str = mac_time_to_iso8601(modification_time)
        .unwrap_or_else(|| format_mac_time_legacy(modification_time));

    metadata.insert(
        format!("QuickTime:TrackCreateDate{}", track_suffix),
        TagValue::String(create_date_str),
    );
    metadata.insert(
        format!("QuickTime:TrackModifyDate{}", track_suffix),
        TagValue::String(modify_date_str),
    );

    // Track ID
    metadata.insert(
        format!("QuickTime:TrackID{}", track_suffix),
        TagValue::Integer(track_id as i64),
    );

    // Track duration - ExifTool formats this as "X.XX s" using movie timescale
    // Get movie timescale from previously extracted metadata
    let duration_str = if let Some(timescale_value) = metadata.get("QuickTime:TimeScale")
        && let Some(timescale) = timescale_value.as_integer()
        && timescale > 0
    {
        let duration_sec = duration as f64 / timescale as f64;
        format!("{:.2} s", duration_sec)
    } else {
        format!("{} units", duration)
    };
    metadata.insert(
        format!("QuickTime:TrackDuration{}", track_suffix),
        TagValue::String(duration_str),
    );

    // Track layer (2 bytes at version-dependent offset)
    let layer_offset = if version == 1 { 44 } else { 32 };
    if let Some(layer) = r.i16_at(layer_offset) {
        metadata.insert(
            format!("QuickTime:TrackLayer{}", track_suffix),
            TagValue::Integer(layer as i64),
        );
    }

    // Track volume (fixed-point 8.8 format, 2 bytes after altGroup)
    // Volume is at layer_offset + 4 (2 bytes altGroup + 2 bytes to volume)
    if let Some(volume_raw) = r.i16_at(volume_offset) {
        let volume_pct = (volume_raw as f64 / 256.0) * 100.0;
        metadata.insert(
            format!("QuickTime:TrackVolume{}", track_suffix),
            TagValue::String(format!("{:.2}%", volume_pct)),
        );
    }

    // Balance (not in tkhd, but typically 0 for video tracks)
    // Balance is actually in smhd (sound media header) atom, not tkhd
    // We'll add it when we parse smhd

    // Track enabled flag (bit 0 of flags)
    let enabled = (flags[2] & 0x01) != 0;
    metadata.insert(
        format!("QuickTime:TrackEnabled{}", track_suffix),
        TagValue::String(if enabled { "Yes" } else { "No" }.to_string()),
    );

    // Track width and height (fixed-point 16.16 at end of atom)
    if r.len() >= width_offset + 8 {
        if let Some(width_fixed) = r.u32_at(width_offset) {
            let width = (width_fixed >> 16) as f64 + (width_fixed & 0xFFFF) as f64 / 65536.0;
            metadata.insert(
                format!("QuickTime:TrackWidth{}", track_suffix),
                TagValue::String(format!("{:.2}", width)),
            );
        }

        if let Some(height_fixed) = r.u32_at(width_offset + 4) {
            let height = (height_fixed >> 16) as f64 + (height_fixed & 0xFFFF) as f64 / 65536.0;
            metadata.insert(
                format!("QuickTime:TrackHeight{}", track_suffix),
                TagValue::String(format!("{:.2}", height)),
            );
        }
    }

    Ok(())
}

/// Extract media header metadata from mdhd atom
fn extract_media_header(
    mdhd: &Atom,
    metadata: &mut MetadataMap,
    track_index: usize,
) -> Result<(), String> {
    if mdhd.data.len() < 24 {
        return Ok(());
    }

    let r = EndianReader::big_endian(mdhd.data);
    let version = mdhd.data[0];

    // Use track index for tag names if we have multiple tracks
    let track_suffix = if track_index > 0 {
        format!("_{}", track_index + 1)
    } else {
        String::new()
    };

    // Media header version (ExifTool outputs this)
    metadata.insert(
        format!("QuickTime:MediaHeaderVersion{}", track_suffix),
        TagValue::Integer(version as i64),
    );

    // Parse time fields based on version (v0: 32-bit, v1: 64-bit)
    let (creation_time, modification_time, timescale, duration, lang_offset) = if version == 1 {
        if r.len() < 32 {
            return Ok(());
        }
        (
            r.u64_at(4).unwrap_or(0),
            r.u64_at(12).unwrap_or(0),
            r.u32_at(20).unwrap_or(0),
            r.u64_at(24).unwrap_or(0),
            32usize,
        )
    } else {
        (
            r.u32_at(4).unwrap_or(0) as u64,
            r.u32_at(8).unwrap_or(0) as u64,
            r.u32_at(12).unwrap_or(0),
            r.u32_at(16).unwrap_or(0) as u64,
            20usize,
        )
    };

    // Media timestamps
    // Use shared timestamp utility for dates after 1970, fallback to legacy for older dates
    let create_date_str =
        mac_time_to_iso8601(creation_time).unwrap_or_else(|| format_mac_time_legacy(creation_time));
    let modify_date_str = mac_time_to_iso8601(modification_time)
        .unwrap_or_else(|| format_mac_time_legacy(modification_time));

    metadata.insert(
        format!("QuickTime:MediaCreateDate{}", track_suffix),
        TagValue::String(create_date_str),
    );
    metadata.insert(
        format!("QuickTime:MediaModifyDate{}", track_suffix),
        TagValue::String(modify_date_str),
    );

    // Media timescale
    metadata.insert(
        format!("QuickTime:MediaTimeScale{}", track_suffix),
        TagValue::Integer(timescale as i64),
    );

    // Media duration
    let duration_sec = if timescale > 0 {
        duration as f64 / timescale as f64
    } else {
        0.0
    };
    metadata.insert(
        format!("QuickTime:MediaDuration{}", track_suffix),
        TagValue::String(format!("{:.2} s", duration_sec)),
    );

    // Language code (ISO 639-2/T language code packed in 16 bits)
    if let Some(lang_code) = r.u16_at(lang_offset) {
        // Language code is packed: 3 characters, 5 bits each
        let lang_char1 = ((lang_code >> 10) & 0x1F) as u8 + 0x60;
        let lang_char2 = ((lang_code >> 5) & 0x1F) as u8 + 0x60;
        let lang_char3 = (lang_code & 0x1F) as u8 + 0x60;

        if (0x61..=0x7A).contains(&lang_char1) {
            let lang_str = String::from_utf8(vec![lang_char1, lang_char2, lang_char3])
                .unwrap_or_else(|_| "und".to_string());
            metadata.insert(
                format!("QuickTime:MediaLanguageCode{}", track_suffix),
                TagValue::String(lang_str),
            );
        }
    }

    Ok(())
}

/// Extract sample description metadata from stsd atom
fn extract_sample_description(
    stsd: &Atom,
    metadata: &mut MetadataMap,
    track_index: usize,
) -> Result<(), String> {
    if stsd.data.len() < 8 {
        return Ok(());
    }

    let r = EndianReader::big_endian(stsd.data);

    // Skip version/flags (4 bytes)
    let entry_count = r.u32_at(4).unwrap_or(0);

    // Use track index for tag names
    let track_suffix = if track_index > 0 {
        format!("_{}", track_index + 1)
    } else {
        String::new()
    };

    metadata.insert(
        format!("QuickTime:SampleDescriptionCount{}", track_suffix),
        TagValue::Integer(entry_count as i64),
    );

    // Parse first sample description entry
    if entry_count > 0 && stsd.data.len() >= 16 {
        // Each sample description starts at offset 8
        let entry_data = &stsd.data[8..];

        if entry_data.len() >= 8 {
            // Size (4 bytes)
            let _size = r.u32_at(8).unwrap_or(0);

            // Format/Codec ID (4 bytes)
            if let Ok(format) = std::str::from_utf8(&entry_data[4..8]) {
                let format_trimmed = format.trim();
                metadata.insert(
                    format!("QuickTime:CompressorID{}", track_suffix),
                    TagValue::String(format_trimmed.to_string()),
                );

                // Determine if this is an audio or video codec
                // Note: format_trimmed has trailing spaces removed, so "raw " becomes "raw"
                let is_audio_codec = matches!(
                    format_trimmed,
                    "mp4a"
                        | "sowt"
                        | "twos"
                        | "alaw"
                        | "ulaw"
                        | "raw"   // Raw PCM (trimmed from "raw ")
                        | "raw "  // Raw PCM (untrimmed)
                        | "lpcm"
                        | "ac-3"
                        | "ec-3"
                        | "aac "
                        | "alac"
                        | "samr"
                        | "sawb"
                        | "Qclp"
                        | "mp3 "
                );

                // Map common codec IDs to readable names
                let codec_name = match format_trimmed {
                    "avc1" | "avc3" => "H.264/AVC",
                    "hvc1" | "hev1" => "H.265/HEVC",
                    "mp4v" => "MPEG-4 Video",
                    "mp4a" => "MPEG-4 Audio (AAC)",
                    "jpeg" => "Photo - JPEG",
                    "raw " => "Uncompressed",
                    "sowt" => "PCM (little-endian)",
                    "twos" => "PCM (big-endian)",
                    "alaw" => "A-law",
                    "ulaw" => "u-law",
                    "vp08" => "VP8",
                    "vp09" => "VP9",
                    "av01" => "AV1",
                    "alac" => "Apple Lossless",
                    "ac-3" => "AC-3",
                    "ec-3" => "E-AC-3",
                    "lpcm" => "Linear PCM",
                    _ => format_trimmed,
                };
                metadata.insert(
                    format!("QuickTime:CompressorName{}", track_suffix),
                    TagValue::String(codec_name.to_string()),
                );

                // For audio codecs, also output AudioFormat (ExifTool convention)
                if is_audio_codec {
                    metadata.insert(
                        format!("QuickTime:AudioFormat{}", track_suffix),
                        TagValue::String(format_trimmed.to_string()),
                    );
                }
            }

            // For video sample descriptions
            if entry_data.len() >= 86 {
                // Check if this looks like a video sample description
                // Video sample descriptions have width/height at specific offsets
                let entry_reader = EndianReader::big_endian(entry_data);

                // VendorID (4 bytes at offset 20) - camera manufacturer
                // Video sample entry structure:
                // size(4) + format(4) + reserved(6) + data_ref_index(2) + version(2) + revision(2) = 20
                if let Some(vendor_bytes) = entry_reader.bytes_at(20, 4) {
                    if let Ok(vendor_str) = std::str::from_utf8(vendor_bytes) {
                        let vendor_trimmed =
                            vendor_str.trim_matches(|c: char| c == '\0' || c.is_whitespace());
                        if !vendor_trimmed.is_empty() {
                            // Map common vendor codes to readable names
                            let vendor_name = match vendor_trimmed {
                                "pent" => "Pentax",
                                "niko" => "Nikon",
                                "cano" => "Canon",
                                "sony" => "Sony",
                                "fuji" => "Fujifilm",
                                "pana" => "Panasonic",
                                "olym" => "Olympus",
                                "appl" => "Apple",
                                _ => vendor_trimmed,
                            };
                            metadata.insert(
                                format!("QuickTime:VendorID{}", track_suffix),
                                TagValue::String(vendor_name.to_string()),
                            );
                        }
                    }
                }

                // Width (2 bytes at offset 32)
                if let Some(width) = entry_reader.u16_at(32)
                    && width > 0
                    && width < 10000
                {
                    // Sanity check
                    // Output both ImageWidth (ExifTool convention) and legacy VideoWidth
                    metadata.insert(
                        format!("QuickTime:ImageWidth{}", track_suffix),
                        TagValue::Integer(width as i64),
                    );
                    metadata.insert(
                        format!("QuickTime:SourceImageWidth{}", track_suffix),
                        TagValue::Integer(width as i64),
                    );
                }

                // Height (2 bytes at offset 34)
                if let Some(height) = entry_reader.u16_at(34)
                    && height > 0
                    && height < 10000
                {
                    // Sanity check
                    // Output both ImageHeight (ExifTool convention) and legacy VideoHeight
                    metadata.insert(
                        format!("QuickTime:ImageHeight{}", track_suffix),
                        TagValue::Integer(height as i64),
                    );
                    metadata.insert(
                        format!("QuickTime:SourceImageHeight{}", track_suffix),
                        TagValue::Integer(height as i64),
                    );
                }

                // Bit depth (2 bytes at offset 82)
                if let Some(depth) = entry_reader.u16_at(82) {
                    metadata.insert(
                        format!("QuickTime:BitDepth{}", track_suffix),
                        TagValue::Integer(depth as i64),
                    );
                }

                // Extract HEVC configuration from hvcC box if present (for hvc1/hev1 codecs)
                // Video sample entry structure ends at byte 86, extensions follow
                if entry_data.len() > 86 {
                    extract_hevc_configuration(&entry_data[86..], metadata, &track_suffix);
                }

                // Horizontal and vertical resolution (fixed-point 16.16 at offsets 36 and 40)
                if let Some(xres_fixed) = entry_reader.u32_at(36) {
                    let xres = (xres_fixed >> 16) as i64;
                    if xres > 0 && xres <= 10000 {
                        metadata.insert(
                            format!("QuickTime:XResolution{}", track_suffix),
                            TagValue::Integer(xres),
                        );
                    }
                }
                if let Some(yres_fixed) = entry_reader.u32_at(40) {
                    let yres = (yres_fixed >> 16) as i64;
                    if yres > 0 && yres <= 10000 {
                        metadata.insert(
                            format!("QuickTime:YResolution{}", track_suffix),
                            TagValue::Integer(yres),
                        );
                    }
                }
            }

            // For audio sample descriptions
            if entry_data.len() >= 36 {
                // Audio sample descriptions have channel count and sample rate
                let entry_reader = EndianReader::big_endian(entry_data);

                // Channel count (2 bytes at offset 24)
                if let Some(channels) = entry_reader.u16_at(24)
                    && channels > 0
                    && channels <= 32
                {
                    // Sanity check
                    metadata.insert(
                        format!("QuickTime:AudioChannels{}", track_suffix),
                        TagValue::Integer(channels as i64),
                    );
                }

                // Sample size (2 bytes at offset 26)
                if let Some(sample_size) = entry_reader.u16_at(26)
                    && sample_size > 0
                    && sample_size <= 64
                {
                    // Sanity check
                    metadata.insert(
                        format!("QuickTime:AudioBitsPerSample{}", track_suffix),
                        TagValue::Integer(sample_size as i64),
                    );
                }

                // Sample rate (fixed-point 16.16 at offset 32)
                if let Some(sample_rate_fixed) = entry_reader.u32_at(32) {
                    let sample_rate = (sample_rate_fixed >> 16) as f64;
                    // Allow sample rates from 1000 to 192000 Hz (some old cameras use lower rates)
                    if (1000.0..=192000.0).contains(&sample_rate) {
                        metadata.insert(
                            format!("QuickTime:AudioSampleRate{}", track_suffix),
                            TagValue::Integer(sample_rate as i64),
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

/// Extract HEVC configuration (hvcC box) from video sample entry extension data
///
/// The hvcC box contains HEVC decoder configuration record with profile, level,
/// and constraint information. It's found in hvc1/hev1 sample entries.
///
/// HEVCDecoderConfigurationRecord structure:
/// - configurationVersion (1 byte)
/// - general_profile_space (2 bits) + general_tier_flag (1 bit) + general_profile_idc (5 bits)
/// - general_profile_compatibility_flags (4 bytes)
/// - general_constraint_indicator_flags (6 bytes)
/// - general_level_idc (1 byte)
/// - min_spatial_segmentation_idc (4 bits reserved + 12 bits value)
/// - parallelismType (6 bits reserved + 2 bits value)
/// - chromaFormat (6 bits reserved + 2 bits value)
/// - bitDepthLumaMinus8 (5 bits reserved + 3 bits value)
/// - bitDepthChromaMinus8 (5 bits reserved + 3 bits value)
/// - avgFrameRate (2 bytes)
/// - constantFrameRate (2 bits) + numTemporalLayers (3 bits) + temporalIdNested (1 bit) + lengthSizeMinusOne (2 bits)
fn extract_hevc_configuration(data: &[u8], metadata: &mut MetadataMap, _track_suffix: &str) {
    // Look for hvcC box in the extension data
    // Box format: 4 bytes size + 4 bytes type ("hvcC")
    let mut offset = 0;
    while offset + 8 <= data.len() {
        let box_size = u32::from_be_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        if box_size < 8 || offset + box_size > data.len() {
            break;
        }

        let box_type = &data[offset + 4..offset + 8];
        if box_type == b"hvcC" {
            // Found hvcC box, parse the configuration record
            let hvcc_data = &data[offset + 8..offset + box_size];
            if hvcc_data.len() >= 23 {
                // configurationVersion (1 byte)
                let config_version = hvcc_data[0];
                metadata.insert(
                    "QuickTime:HEVCConfigurationVersion".to_string(),
                    TagValue::Integer(config_version as i64),
                );

                // general_profile_space (2 bits) + general_tier_flag (1 bit) + general_profile_idc (5 bits)
                let profile_byte = hvcc_data[1];
                let profile_space = (profile_byte >> 6) & 0x03;
                let tier_flag = (profile_byte >> 5) & 0x01;
                let profile_idc = profile_byte & 0x1F;

                let profile_space_str = match profile_space {
                    0 => "Conforming",
                    1 => "Reserved 1",
                    2 => "Reserved 2",
                    3 => "Reserved 3",
                    _ => "Unknown",
                };
                metadata.insert(
                    "QuickTime:GeneralProfileSpace".to_string(),
                    TagValue::String(profile_space_str.to_string()),
                );

                let tier_str = if tier_flag == 0 {
                    "Main Tier"
                } else {
                    "High Tier"
                };
                metadata.insert(
                    "QuickTime:GeneralTierFlag".to_string(),
                    TagValue::String(tier_str.to_string()),
                );

                let profile_name = match profile_idc {
                    1 => "Main",
                    2 => "Main 10",
                    3 => "Main Still Picture",
                    4 => "Format Range Extensions",
                    5 => "High Throughput",
                    6 => "Multiview Main",
                    7 => "Scalable Main",
                    8 => "3D Main",
                    9 => "Screen-Extended Main",
                    10 => "Scalable Format Range Extensions",
                    11 => "High Throughput Screen-Extended",
                    _ => "Unknown",
                };
                metadata.insert(
                    "QuickTime:GeneralProfileIDC".to_string(),
                    TagValue::String(profile_name.to_string()),
                );

                // general_profile_compatibility_flags (4 bytes)
                let compat_flags =
                    u32::from_be_bytes([hvcc_data[2], hvcc_data[3], hvcc_data[4], hvcc_data[5]]);
                let mut compat_profiles = Vec::new();
                if compat_flags & (1 << 31) != 0 {
                    compat_profiles.push("Main");
                }
                if compat_flags & (1 << 30) != 0 {
                    compat_profiles.push("Main 10");
                }
                if compat_flags & (1 << 29) != 0 {
                    compat_profiles.push("Main Still Picture");
                }
                if !compat_profiles.is_empty() {
                    metadata.insert(
                        "QuickTime:GenProfileCompatibilityFlags".to_string(),
                        TagValue::String(compat_profiles.join(", ")),
                    );
                }

                // general_constraint_indicator_flags (6 bytes at offset 6-11)
                let constraint_bytes: Vec<String> =
                    hvcc_data[6..12].iter().map(|b| format!("{}", b)).collect();
                metadata.insert(
                    "QuickTime:ConstraintIndicatorFlags".to_string(),
                    TagValue::String(constraint_bytes.join(" ")),
                );

                // general_level_idc (1 byte at offset 12)
                let level_idc = hvcc_data[12];
                let level = level_idc as f64 / 30.0;
                metadata.insert(
                    "QuickTime:GeneralLevelIDC".to_string(),
                    TagValue::String(format!("{} (level {:.1})", level_idc, level)),
                );

                // min_spatial_segmentation_idc (4 bits reserved + 12 bits value at offset 13-14)
                let min_spatial = u16::from_be_bytes([hvcc_data[13], hvcc_data[14]]) & 0x0FFF;
                metadata.insert(
                    "QuickTime:MinSpatialSegmentationIDC".to_string(),
                    TagValue::Integer(min_spatial as i64),
                );

                // parallelismType (6 bits reserved + 2 bits value at offset 15)
                let parallelism = hvcc_data[15] & 0x03;
                metadata.insert(
                    "QuickTime:ParallelismType".to_string(),
                    TagValue::Integer(parallelism as i64),
                );

                // chromaFormat (6 bits reserved + 2 bits value at offset 16)
                let chroma_format = hvcc_data[16] & 0x03;
                let chroma_str = match chroma_format {
                    0 => "Monochrome",
                    1 => "4:2:0",
                    2 => "4:2:2",
                    3 => "4:4:4",
                    _ => "Unknown",
                };
                metadata.insert(
                    "QuickTime:ChromaFormat".to_string(),
                    TagValue::String(chroma_str.to_string()),
                );

                // bitDepthLumaMinus8 (5 bits reserved + 3 bits value at offset 17)
                let bit_depth_luma = (hvcc_data[17] & 0x07) + 8;
                metadata.insert(
                    "QuickTime:BitDepthLuma".to_string(),
                    TagValue::Integer(bit_depth_luma as i64),
                );

                // bitDepthChromaMinus8 (5 bits reserved + 3 bits value at offset 18)
                let bit_depth_chroma = (hvcc_data[18] & 0x07) + 8;
                metadata.insert(
                    "QuickTime:BitDepthChroma".to_string(),
                    TagValue::Integer(bit_depth_chroma as i64),
                );

                // avgFrameRate (2 bytes at offset 19-20)
                let avg_frame_rate = u16::from_be_bytes([hvcc_data[19], hvcc_data[20]]);
                metadata.insert(
                    "QuickTime:AverageFrameRate".to_string(),
                    TagValue::Integer(avg_frame_rate as i64),
                );

                // Byte at offset 21: constantFrameRate (2 bits) + numTemporalLayers (3 bits) + temporalIdNested (1 bit) + lengthSizeMinusOne (2 bits)
                let flags_byte = hvcc_data[21];
                let constant_frame_rate = (flags_byte >> 6) & 0x03;
                let num_temporal_layers = (flags_byte >> 3) & 0x07;
                let temporal_id_nested = (flags_byte >> 2) & 0x01;

                let cfr_str = match constant_frame_rate {
                    0 => "Unknown",
                    1 => "Constant",
                    2 => "Variable",
                    _ => "Reserved",
                };
                metadata.insert(
                    "QuickTime:ConstantFrameRate".to_string(),
                    TagValue::String(cfr_str.to_string()),
                );

                metadata.insert(
                    "QuickTime:NumTemporalLayers".to_string(),
                    TagValue::Integer(num_temporal_layers as i64),
                );

                metadata.insert(
                    "QuickTime:TemporalIDNested".to_string(),
                    TagValue::String(
                        if temporal_id_nested == 1 { "Yes" } else { "No" }.to_string(),
                    ),
                );
            }
            return;
        }

        offset += box_size;
    }
}

/// Extract video frame rate from stts (sample-to-time table) atom
///
/// The stts atom contains entries that describe sample duration. For constant
/// frame rate video, there's typically one entry covering all samples.
/// VideoFrameRate = MediaTimeScale / SampleDelta
///
/// Note: This only extracts frame rate for video tracks (requires MediaTimeScale
/// to be present from mdhd).
fn extract_video_frame_rate(
    stts: &Atom,
    metadata: &mut MetadataMap,
    track_index: usize,
) -> Result<(), String> {
    // stts atom format:
    // 1 byte version
    // 3 bytes flags
    // 4 bytes entry count
    // For each entry:
    //   4 bytes sample count
    //   4 bytes sample delta (duration)
    if stts.data.len() < 16 {
        return Ok(());
    }

    let r = EndianReader::big_endian(stts.data);
    let entry_count = r.u32_at(4).unwrap_or(0);

    if entry_count == 0 {
        return Ok(());
    }

    // Read first entry - for constant frame rate video, this covers all samples
    let _sample_count = r.u32_at(8).unwrap_or(0);
    let sample_delta = r.u32_at(12).unwrap_or(0);

    if sample_delta == 0 {
        return Ok(());
    }

    // Get the MediaTimeScale from metadata to calculate frame rate
    let track_suffix = if track_index > 0 {
        format!("_{}", track_index + 1)
    } else {
        String::new()
    };

    let timescale_key = format!("QuickTime:MediaTimeScale{}", track_suffix);
    if let Some(timescale_value) = metadata.get(&timescale_key)
        && let Some(timescale) = timescale_value.as_integer()
        && timescale > 0
    {
        let frame_rate = timescale as f64 / sample_delta as f64;
        // Only output if this looks like a reasonable frame rate (1-240 fps)
        if (1.0..=240.0).contains(&frame_rate) {
            // Round to reasonable precision for display
            let frame_rate_rounded = (frame_rate * 100.0).round() / 100.0;
            metadata.insert(
                format!("QuickTime:VideoFrameRate{}", track_suffix),
                TagValue::Float(frame_rate_rounded),
            );
        }
    }

    Ok(())
}

/// Extract data handler information from dref atom in minf→dinf→dref
/// The dref contains data references that can specify data handlers
fn extract_data_handler_info(
    data: &[u8],
    metadata: &mut MetadataMap,
    track_index: usize,
) -> Result<(), String> {
    // dref structure:
    // 0-3: version/flags
    // 4-7: number of entries
    // 8+: data reference entries (each entry has atom header + content)

    if data.len() < 8 {
        return Ok(());
    }

    let track_suffix = if track_index > 0 {
        format!("_{}", track_index + 1)
    } else {
        String::new()
    };

    let reader = EndianReader::big_endian(data);

    // Check entry count
    let entry_count = reader.u32_at(4).unwrap_or(0) as usize;
    if entry_count == 0 {
        return Ok(());
    }

    // Parse first entry to get handler class
    if data.len() >= 20 {
        // First entry atom header starts at offset 8
        // The entry has: size (4) + type (4) + version/flags (4) + data
        // Entry type often tells us the handler class:
        // - "alis" = Data Handler (alias)
        // - "url " = URL Data Handler
        // - "dhlr" = Data Handler
        let entry_type = &data[12..16];
        if let Ok(type_str) = std::str::from_utf8(entry_type) {
            let handler_class = match type_str.trim() {
                "alis" | "dhlr" => "Data Handler",
                "url " => "URL Data Handler",
                "rsrc" => "Resource Data Handler",
                _ => return Ok(()), // Unknown type, don't output
            };
            metadata.insert(
                format!("QuickTime:HandlerClass{}", track_suffix),
                TagValue::String(handler_class.to_string()),
            );
        }
    }

    Ok(())
}

/// Extract track-level handler metadata from hdlr atom in track
fn extract_track_handler_metadata(
    hdlr: &Atom,
    metadata: &mut MetadataMap,
    track_index: usize,
) -> Result<(), String> {
    if hdlr.data.len() < 12 {
        return Ok(());
    }

    let track_suffix = if track_index > 0 {
        format!("_{}", track_index + 1)
    } else {
        String::new()
    };

    // Component type / handler class (4 bytes at offset 4)
    let component_type = &hdlr.data[4..8];
    if let Ok(component_str) = std::str::from_utf8(component_type) {
        let component_desc = match component_str {
            "mhlr" => "Media Handler",
            "dhlr" => "Data Handler",
            _ => component_str.trim(),
        };
        if !component_desc.is_empty() {
            metadata.insert(
                format!("QuickTime:HandlerClass{}", track_suffix),
                TagValue::String(component_desc.to_string()),
            );
        }
    }

    Ok(())
}

/// Extract handler metadata from hdlr atom (movie-level, without track suffix)
fn extract_handler_metadata(hdlr: &Atom, metadata: &mut MetadataMap) -> Result<(), String> {
    if hdlr.data.len() < 24 {
        return Ok(());
    }

    // Component type / handler class (4 bytes at offset 4)
    // "mhlr" = Media Handler, "dhlr" = Data Handler
    // Note: Movie-level metadata handler may have empty component type - don't output HandlerClass in that case
    let component_type = &hdlr.data[4..8];
    if let Ok(component_str) = std::str::from_utf8(component_type) {
        // Trim null bytes and whitespace
        let component_trimmed =
            component_str.trim_matches(|c: char| c == '\0' || c.is_whitespace());
        if !component_trimmed.is_empty() {
            let component_desc = match component_trimmed {
                "mhlr" => "Media Handler",
                "dhlr" => "Data Handler",
                _ => component_trimmed,
            };
            metadata.insert(
                "QuickTime:HandlerClass".to_string(),
                TagValue::String(component_desc.to_string()),
            );
        }
    }

    // Handler type / component subtype (4 bytes at offset 8)
    let handler_type = &hdlr.data[8..12];
    if let Ok(handler_str) = std::str::from_utf8(handler_type) {
        let handler_desc = match handler_str {
            "mdir" => "Metadata",
            "vide" => "Video Track",
            "soun" => "Audio Track",
            "hint" => "Hint Track",
            "meta" => "Timed Metadata",
            "text" => "Text Track",
            "tmcd" => "Time Code",
            "pict" => "Picture",
            "auxv" => "Auxiliary Video",
            "auxC" => "Auxiliary Codec",
            _ => handler_str,
        };
        metadata.insert(
            "QuickTime:HandlerType".to_string(),
            TagValue::String(handler_desc.to_string()),
        );
    }

    // Handler vendor ID (4 bytes at offset 12, but it's actually 'reserved' fields)
    // The real vendor/manufacturer is at offset 16-20 in some implementations
    // or it's set to "appl" for Apple
    // Let's check multiple offsets
    if hdlr.data.len() >= 16 {
        // Try reserved fields first (offset 12-16) - often contains "appl" for Apple
        let vendor_bytes = &hdlr.data[12..16];
        if let Ok(vendor) = std::str::from_utf8(vendor_bytes) {
            let trimmed = vendor.trim_matches('\0').trim();
            if !trimmed.is_empty() && trimmed != "\0\0\0\0" {
                let vendor_name = match trimmed {
                    "appl" => "Apple",
                    _ => trimmed,
                };
                metadata.insert(
                    "QuickTime:HandlerVendorID".to_string(),
                    TagValue::String(vendor_name.to_string()),
                );
            }
        }
    }

    // Handler description (name) - null-terminated or Pascal string after reserved fields
    // Offset 24+ contains the name (may be Pascal string with length prefix or null-terminated)
    if hdlr.data.len() > 24 {
        let name_data = &hdlr.data[24..];

        // Try Pascal string first (length prefix)
        if !name_data.is_empty() && name_data[0] > 0 && name_data[0] < 128 {
            let length = name_data[0] as usize;
            if name_data.len() > length
                && let Ok(name) = std::str::from_utf8(&name_data[1..=length])
            {
                let trimmed = name.trim();
                if !trimmed.is_empty() {
                    metadata.insert(
                        "QuickTime:HandlerDescription".to_string(),
                        TagValue::String(trimmed.to_string()),
                    );
                }
            }
        } else {
            // Try null-terminated string
            if let Some(null_pos) = name_data.iter().position(|&b| b == 0)
                && null_pos > 0
                && let Ok(name) = std::str::from_utf8(&name_data[..null_pos])
            {
                let trimmed = name.trim();
                if !trimmed.is_empty() {
                    metadata.insert(
                        "QuickTime:HandlerDescription".to_string(),
                        TagValue::String(trimmed.to_string()),
                    );
                }
            }
        }
    }

    Ok(())
}

/// Convert Mac epoch time (seconds since 1904-01-01) to date string.
///
/// This is a legacy helper that handles pre-1970 dates. For dates after 1970,
/// prefer using `mac_time_to_iso8601` from the shared timestamp utilities.
fn format_mac_time_legacy(mac_time: u64) -> String {
    // Mac epoch is 1904-01-01 00:00:00 UTC, Unix epoch is 1970-01-01 00:00:00 UTC
    // Difference is 66 years = 2082844800 seconds
    const MAC_EPOCH_OFFSET: i64 = 2082844800;

    if mac_time == 0 {
        return "0000:00:00 00:00:00".to_string();
    }

    let unix_time = mac_time as i64 - MAC_EPOCH_OFFSET;
    if unix_time <= 0 {
        return "1904:01:01 00:00:00".to_string();
    }

    // Convert Unix timestamp to date components
    // This is a simplified calculation for dates after 1970-01-01
    const SECONDS_PER_DAY: i64 = 86400;
    const SECONDS_PER_HOUR: i64 = 3600;
    const SECONDS_PER_MINUTE: i64 = 60;

    let days_since_epoch = unix_time / SECONDS_PER_DAY;
    let remaining_seconds = unix_time % SECONDS_PER_DAY;
    let hours = remaining_seconds / SECONDS_PER_HOUR;
    let minutes = (remaining_seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE;
    let seconds = remaining_seconds % SECONDS_PER_MINUTE;

    // Simple year/month/day calculation (approximate, good enough for metadata)
    // Using average of 365.25 days per year
    let mut year = 1970;
    let mut days = days_since_epoch;

    // Add years
    while days >= 365 {
        let year_days = if is_leap_year(year) { 366 } else { 365 };
        if days >= year_days {
            days -= year_days;
            year += 1;
        } else {
            break;
        }
    }

    // Calculate month and day
    let (month, day) = days_to_month_day(days as u32, is_leap_year(year));

    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        year, month, day, hours, minutes, seconds
    )
}

/// Check if a year is a leap year
fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Convert day of year to month and day
fn days_to_month_day(mut days: u32, is_leap: bool) -> (u32, u32) {
    const MONTH_DAYS: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    const MONTH_DAYS_LEAP: [u32; 12] = [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

    let month_days = if is_leap {
        &MONTH_DAYS_LEAP
    } else {
        &MONTH_DAYS
    };

    for (i, &month_len) in month_days.iter().enumerate() {
        if days < month_len {
            return ((i + 1) as u32, days + 1);
        }
        days -= month_len;
    }

    // Fallback for invalid input
    (12, 31)
}

/// Extract classic QuickTime user data atoms (©xxx and others)
fn extract_user_data_atoms(udta: &Atom, metadata: &mut MetadataMap) -> Result<(), String> {
    let children = udta.parse_children().unwrap_or_default();

    for atom in children {
        let atom_bytes = atom.atom_type.as_bytes();

        // QuickTime user data atoms start with © character (0xA9)
        if atom_bytes[0] == 0xA9 {
            if let Some(value) = extract_string_value(atom.data) {
                // Convert atom bytes to string for mapping lookup
                if let Ok(atom_str) = std::str::from_utf8(atom_bytes) {
                    // Try to use the tag mapping first
                    if let Some(exiftool_tag) = atom_to_exiftool_tag(atom_str) {
                        metadata.insert(
                            exiftool_tag.to_string(),
                            TagValue::new_string(value.clone()),
                        );
                    }
                }

                // Also keep the legacy UserData and QuickTime tags for backward compatibility
                let suffix = match atom_bytes {
                    b"\xa9nam" => Some("Title"),
                    b"\xa9ART" => Some("Artist"),
                    b"\xa9alb" => Some("Album"),
                    b"\xa9day" => Some("Year"),
                    b"\xa9cmt" => Some("Comment"),
                    b"\xa9cpy" => Some("Copyright"),
                    b"\xa9gen" => Some("Genre"),
                    b"\xa9too" => Some("Encoder"),
                    b"\xa9des" => Some("Description"),
                    b"\xa9dir" => Some("Director"),
                    b"\xa9prd" => Some("Producer"),
                    b"\xa9prf" => Some("Performers"),
                    b"\xa9wrt" => Some("Composer"),
                    b"\xa9lyr" => Some("Lyrics"),
                    b"\xa9grp" => Some("Grouping"),
                    b"\xa9fmt" => Some("Format"), // Camera format description
                    b"\xa9inf" => Some("Information"), // Camera information
                    _ => None,
                };

                // Handle GPS coordinates (©xyz atom)
                if atom_bytes == b"\xa9xyz"
                    && let Some((lat, lon, alt)) = parse_iso6709(&value)
                {
                    metadata.insert("QuickTime:GPSLatitude".to_string(), TagValue::Float(lat));
                    metadata.insert("QuickTime:GPSLongitude".to_string(), TagValue::Float(lon));
                    if let Some(altitude) = alt {
                        metadata.insert(
                            "QuickTime:GPSAltitude".to_string(),
                            TagValue::Float(altitude),
                        );
                    }
                    metadata.insert(
                        "QuickTime:GPSCoordinates".to_string(),
                        TagValue::new_string(value.clone()),
                    );
                }

                if let Some(suffix) = suffix {
                    metadata.insert(
                        format!("QuickTime:{}", suffix),
                        TagValue::new_string(value.clone()),
                    );
                    metadata.insert(format!("UserData:{}", suffix), TagValue::new_string(value));
                }
            }
        } else {
            // Handle non-© atoms (3GPP and other user data atoms)
            let atom_type_str = std::str::from_utf8(atom_bytes).unwrap_or("");
            match atom_type_str {
                "cprt" => {
                    // Copyright (3GPP style)
                    if let Some(value) = extract_3gpp_string_value(atom.data) {
                        metadata.insert(
                            "QuickTime:Copyright".to_string(),
                            TagValue::new_string(value),
                        );
                    }
                }
                "auth" => {
                    // Author
                    if let Some(value) = extract_3gpp_string_value(atom.data) {
                        metadata
                            .insert("QuickTime:Author".to_string(), TagValue::new_string(value));
                    }
                }
                "titl" => {
                    // Title (3GPP style)
                    if let Some(value) = extract_3gpp_string_value(atom.data) {
                        metadata.insert("QuickTime:Title".to_string(), TagValue::new_string(value));
                    }
                }
                "dscp" => {
                    // Description (3GPP style)
                    if let Some(value) = extract_3gpp_string_value(atom.data) {
                        metadata.insert(
                            "QuickTime:Description".to_string(),
                            TagValue::new_string(value),
                        );
                    }
                }
                "perf" => {
                    // Performer (3GPP style)
                    if let Some(value) = extract_3gpp_string_value(atom.data) {
                        metadata.insert(
                            "QuickTime:Performer".to_string(),
                            TagValue::new_string(value),
                        );
                    }
                }
                "albm" => {
                    // Album (3GPP style)
                    if let Some(value) = extract_3gpp_string_value(atom.data) {
                        metadata.insert("QuickTime:Album".to_string(), TagValue::new_string(value));
                    }
                }
                "yrrc" => {
                    // Recording year (3GPP style)
                    if atom.data.len() >= 6 {
                        let r = EndianReader::big_endian(atom.data);
                        if let Some(year) = r.u16_at(4) {
                            metadata.insert(
                                "QuickTime:Year".to_string(),
                                TagValue::Integer(year as i64),
                            );
                        }
                    }
                }
                "gnre" => {
                    // Genre (3GPP style) - numeric genre ID
                    if atom.data.len() >= 6 {
                        let r = EndianReader::big_endian(atom.data);
                        if let Some(genre_id) = r.u16_at(4) {
                            let genre_name = decode_id3_genre(genre_id);
                            metadata.insert(
                                "QuickTime:Genre".to_string(),
                                TagValue::new_string(genre_name),
                            );
                        }
                    }
                }
                "FIRM" | "INFO" => {
                    // Firmware / Information
                    if let Some(value) = extract_raw_string(atom.data) {
                        metadata.insert(
                            "QuickTime:Information".to_string(),
                            TagValue::new_string(value),
                        );
                    }
                }
                "MAKE" | "MODL" => {
                    // Camera make/model in some formats
                    if let Some(value) = extract_raw_string(atom.data) {
                        let tag = if atom_type_str == "MAKE" {
                            "QuickTime:Make"
                        } else {
                            "QuickTime:Model"
                        };
                        metadata.insert(tag.to_string(), TagValue::new_string(value));
                    }
                }
                "CNCV" | "CNFV" | "CNMN" => {
                    // Camera info atoms (Canon, etc.)
                    if let Some(value) = extract_raw_string(atom.data) {
                        let tag = match atom_type_str {
                            "CNCV" => "QuickTime:CompressorVersion",
                            "CNFV" => "QuickTime:FirmwareVersion",
                            "CNMN" => "QuickTime:Model",
                            _ => "QuickTime:Unknown",
                        };
                        metadata.insert(tag.to_string(), TagValue::new_string(value));
                    }
                }
                "PENT" | "PXTH" => {
                    // Pentax-specific atoms
                    if let Some(value) = extract_raw_string(atom.data) {
                        metadata.insert(
                            format!("QuickTime:{}", atom_type_str),
                            TagValue::new_string(value),
                        );
                    }
                }
                "tmpo" => {
                    // Beats per minute (tempo) - in udta directly
                    if atom.data.len() >= 4 {
                        let r = EndianReader::big_endian(atom.data);
                        if let Some(bpm) = r.u16_at(2)
                            && bpm > 0
                        {
                            metadata.insert(
                                "QuickTime:BeatsPerMinute".to_string(),
                                TagValue::Integer(bpm as i64),
                            );
                        }
                    }
                }
                "fmt " => {
                    // Format atom (Pentax cameras)
                    if let Some(value) = extract_raw_string(atom.data) {
                        metadata
                            .insert("QuickTime:Format".to_string(), TagValue::new_string(value));
                    }
                }
                "TAGS" => {
                    // Pentax MakerNotes (TAGS atom)
                    let _ = extract_pentax_maker_notes(atom.data, metadata);
                }
                "XMP_" => {
                    // XMP metadata atom
                    let _ = extract_xmp_from_atom(atom.data, metadata);
                }
                _ => {
                    // Skip unknown atoms
                }
            }
        }
    }

    Ok(())
}

/// Extract 3GPP-style string value from atom data
/// Format: 4 bytes version/flags, 2 bytes language, then UTF-8 string
fn extract_3gpp_string_value(data: &[u8]) -> Option<String> {
    if data.len() < 6 {
        return None;
    }
    // Skip version/flags (4 bytes) and language (2 bytes)
    let text_data = &data[6..];
    // Remove null terminator if present
    let text_end = text_data
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(text_data.len());
    String::from_utf8(text_data[..text_end].to_vec()).ok()
}

/// Extract raw string from atom data (just read as UTF-8)
fn extract_raw_string(data: &[u8]) -> Option<String> {
    // Remove null terminator if present
    let text_end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
    String::from_utf8(data[..text_end].to_vec()).ok()
}

/// Decode ID3 genre ID to genre name
fn decode_id3_genre(genre_id: u16) -> String {
    // Standard ID3v1 genres (partial list of most common)
    match genre_id {
        0 => "Blues",
        1 => "Classic Rock",
        2 => "Country",
        3 => "Dance",
        4 => "Disco",
        5 => "Funk",
        6 => "Grunge",
        7 => "Hip-Hop",
        8 => "Jazz",
        9 => "Metal",
        10 => "New Age",
        11 => "Oldies",
        12 => "Other",
        13 => "Pop",
        14 => "R&B",
        15 => "Rap",
        16 => "Reggae",
        17 => "Rock",
        18 => "Techno",
        19 => "Industrial",
        20 => "Alternative",
        21 => "Ska",
        22 => "Death Metal",
        23 => "Pranks",
        24 => "Soundtrack",
        25 => "Euro-Techno",
        26 => "Ambient",
        27 => "Trip-Hop",
        28 => "Vocal",
        29 => "Jazz+Funk",
        30 => "Fusion",
        31 => "Trance",
        32 => "Classical",
        33 => "Instrumental",
        34 => "Acid",
        35 => "House",
        36 => "Game",
        37 => "Sound Clip",
        38 => "Gospel",
        39 => "Noise",
        40 => "AlternRock",
        41 => "Bass",
        42 => "Soul",
        43 => "Punk",
        44 => "Space",
        45 => "Meditative",
        46 => "Instrumental Pop",
        47 => "Instrumental Rock",
        48 => "Ethnic",
        49 => "Gothic",
        50 => "Darkwave",
        51 => "Techno-Industrial",
        52 => "Electronic",
        53 => "Pop-Folk",
        54 => "Eurodance",
        55 => "Dream",
        56 => "Southern Rock",
        57 => "Comedy",
        58 => "Cult",
        59 => "Gangsta",
        60 => "Top 40",
        61 => "Christian Rap",
        62 => "Pop/Funk",
        63 => "Jungle",
        64 => "Native American",
        65 => "Cabaret",
        66 => "New Wave",
        67 => "Psychedelic",
        68 => "Rave",
        69 => "Showtunes",
        70 => "Trailer",
        71 => "Lo-Fi",
        72 => "Tribal",
        73 => "Acid Punk",
        74 => "Acid Jazz",
        75 => "Polka",
        76 => "Retro",
        77 => "Musical",
        78 => "Rock & Roll",
        79 => "Hard Rock",
        _ => return format!("Genre {}", genre_id),
    }
    .to_string()
}

/// Extract iTunes-style metadata from meta atom
fn extract_itunes_metadata(meta: &Atom, metadata: &mut MetadataMap) -> Result<(), String> {
    // Meta atoms have a 4-byte version/flags header before the child atoms
    // We need to skip this header to parse the children correctly
    let data = if meta.data.len() >= 4 && meta.data[0..4] == [0, 0, 0, 0] {
        &meta.data[4..]
    } else {
        meta.data
    };

    // Parse children from the adjusted data
    let children = match super::atom_parser::parse_atoms(data) {
        Ok((_, atoms)) => atoms,
        Err(_) => return Ok(()), // Gracefully handle parsing errors
    };

    // Find ilst atom among the children
    let ilst = children.iter().find(|atom| atom.atom_type.matches("ilst"));

    // iTunes metadata is in the ilst (item list) atom
    if let Some(ilst) = ilst {
        let items = ilst.parse_children().unwrap_or_default();

        for item in items {
            let atom_bytes = item.atom_type.as_bytes();

            // Each item contains a data atom
            if let Some(data_atom) = item.find_child("data")
                && let Some(value) = extract_itunes_data_value(data_atom.data)
            {
                let mut add_year_tag = false;
                let tag_name: Cow<'static, str> = match atom_bytes {
                    b"\xa9nam" => Cow::Borrowed("ItemList:Title"),
                    b"\xa9ART" => Cow::Borrowed("ItemList:Artist"),
                    b"\xa9alb" => Cow::Borrowed("ItemList:Album"),
                    b"\xa9day" => {
                        add_year_tag = true;
                        Cow::Borrowed("ItemList:ContentCreateDate")
                    }
                    b"\xa9cmt" => Cow::Borrowed("ItemList:Comment"),
                    b"\xa9gen" => Cow::Borrowed("ItemList:Genre"),
                    b"\xa9too" => Cow::Borrowed("ItemList:Encoder"),
                    b"aART" => Cow::Borrowed("ItemList:AlbumArtist"),
                    b"\xa9wrt" => Cow::Borrowed("ItemList:Composer"),
                    b"\xa9grp" => Cow::Borrowed("ItemList:Grouping"),
                    b"\xa9lyr" => Cow::Borrowed("ItemList:Lyrics"),
                    b"trkn" => Cow::Borrowed("ItemList:TrackNumber"),
                    b"disk" => Cow::Borrowed("ItemList:DiscNumber"),
                    b"cprt" | b"\xa9cpy" => Cow::Borrowed("ItemList:Copyright"),
                    b"tmpo" => Cow::Borrowed("ItemList:BeatsPerMinute"),
                    b"covr" => Cow::Borrowed("ItemList:CoverArt"),
                    b"gnre" => Cow::Borrowed("ItemList:Genre"),
                    b"desc" => Cow::Borrowed("ItemList:Description"),
                    b"ldes" => Cow::Borrowed("ItemList:LongDescription"),
                    _ => {
                        if let Ok(s) = std::str::from_utf8(atom_bytes) {
                            Cow::Owned(format!("ItemList:{}", s))
                        } else {
                            Cow::Owned(format!(
                                "ItemList:{:02X}{:02X}{:02X}{:02X}",
                                atom_bytes[0], atom_bytes[1], atom_bytes[2], atom_bytes[3]
                            ))
                        }
                    }
                };

                // Also insert into QuickTime: namespace for iTunes ilst metadata
                // This matches ExifTool behavior which uses QuickTime: prefix
                let qt_tag = match atom_bytes {
                    b"\xa9nam" => Some("QuickTime:Title"),
                    b"\xa9ART" => Some("QuickTime:Artist"),
                    b"\xa9alb" => Some("QuickTime:Album"),
                    b"\xa9day" => Some("QuickTime:ContentCreateDate"),
                    b"\xa9cmt" => Some("QuickTime:Comment"),
                    b"\xa9gen" => Some("QuickTime:Genre"),
                    b"\xa9too" => Some("QuickTime:Encoder"),
                    b"aART" => Some("QuickTime:AlbumArtist"),
                    b"\xa9wrt" => Some("QuickTime:Composer"),
                    b"\xa9grp" => Some("QuickTime:Grouping"),
                    b"\xa9lyr" => Some("QuickTime:Lyrics"),
                    b"cprt" | b"\xa9cpy" => Some("QuickTime:Copyright"),
                    b"tmpo" => Some("QuickTime:BeatsPerMinute"),
                    b"covr" => Some("QuickTime:CoverArt"),
                    b"gnre" => Some("QuickTime:Genre"),
                    b"desc" => Some("QuickTime:Description"),
                    b"ldes" => Some("QuickTime:LongDescription"),
                    _ => None,
                };
                // Handle TrackNumber and DiscNumber formatted as "X of Y"
                // This must be done FIRST, before inserting raw value, so formatted value takes precedence
                let mut formatted_track_or_disc = false;
                if (atom_bytes == b"trkn" || atom_bytes == b"disk")
                    && let TagValue::Binary(ref data) = value
                    && data.len() >= 6
                {
                    let r = EndianReader::big_endian(data);
                    let current = r.u16_at(2).unwrap_or(0);
                    let total = r.u16_at(4).unwrap_or(0);
                    let formatted = if total > 0 {
                        format!("{} of {}", current, total)
                    } else {
                        format!("{}", current)
                    };
                    let tag = if atom_bytes == b"trkn" {
                        "QuickTime:TrackNumber"
                    } else {
                        "QuickTime:DiskNumber"
                    };
                    metadata.insert(tag.to_string(), TagValue::new_string(formatted));
                    formatted_track_or_disc = true;
                }

                // For trkn/disk, don't insert the raw binary ItemList value - only the formatted QuickTime value
                if !formatted_track_or_disc {
                    if let Some(qt_tag_name) = qt_tag {
                        metadata.insert(qt_tag_name.to_string(), value.clone());
                    }
                    metadata.insert(tag_name.into_owned(), value.clone());
                }

                if add_year_tag
                    && let TagValue::String(ref text) = value
                    && text.len() >= 4
                {
                    let year = text.chars().take(4).collect::<String>();
                    metadata.insert("ItemList:Year".to_string(), TagValue::new_string(year));
                }
            }
        }
    }

    Ok(())
}

/// Extract MP4 metadata using keys/ilst atoms
fn extract_mp4_metadata(meta: &Atom, metadata: &mut MetadataMap) -> Result<(), String> {
    // MP4 metadata uses a keys atom to define key names
    // and an ilst atom to store the values
    let keys_atom = meta.find_child("keys");
    let ilst_atom = meta.find_child("ilst");

    if let (Some(keys), Some(ilst)) = (keys_atom, ilst_atom) {
        // Parse the keys
        let key_map = parse_mp4_keys(keys.data)?;

        // Parse the ilst items
        let items = ilst.parse_children().unwrap_or_default();

        for item in items {
            // MP4 ilst uses numeric atom types that correspond to key indices
            // The atom type is a 4-byte integer (index into keys)
            let atom_type_bytes = item.atom_type.as_bytes();

            // Try to interpret as a big-endian integer
            let key_index = EndianReader::big_endian(atom_type_bytes)
                .u32_at(0)
                .unwrap_or(0);

            if let Some(data_atom) = item.find_child("data")
                && let Some(value) = extract_itunes_data_value(data_atom.data)
            {
                // Look up the key name
                if let Some(key_name) = key_map.get(&key_index) {
                    // Map Apple-specific keys to standard tag names
                    let tag_name = map_apple_key_to_tag(key_name);
                    metadata.insert(tag_name, value.clone());

                    // Special handling for GPS coordinates
                    if key_name == "com.apple.quicktime.location.ISO6709"
                        && let TagValue::String(ref gps_str) = value
                        && let Some((lat, lon, alt)) = parse_iso6709(gps_str)
                    {
                        metadata.insert("QuickTime:GPSLatitude".to_string(), TagValue::Float(lat));
                        metadata.insert("QuickTime:GPSLongitude".to_string(), TagValue::Float(lon));
                        if let Some(altitude) = alt {
                            metadata.insert(
                                "QuickTime:GPSAltitude".to_string(),
                                TagValue::Float(altitude),
                            );
                        }
                    }
                } else {
                    // Fallback to using the atom type as the tag name
                    let tag_name = format!("MP4:{}", item.atom_type.as_str());
                    metadata.insert(tag_name, value);
                }
            }
        }
    }

    Ok(())
}

/// Map Apple-specific mdta keys to standard QuickTime tag names
fn map_apple_key_to_tag(key_name: &str) -> String {
    match key_name {
        "com.apple.quicktime.location.ISO6709" => "QuickTime:GPSCoordinates".to_string(),
        "com.apple.quicktime.location.accuracy.horizontal" => {
            "QuickTime:LocationAccuracyHorizontal".to_string()
        }
        "com.apple.quicktime.location.role" => "QuickTime:LocationRole".to_string(),
        "com.apple.quicktime.creationLocation.name" => "QuickTime:CreationLocationName".to_string(),
        "com.apple.quicktime.make" => "QuickTime:Make".to_string(),
        "com.apple.quicktime.model" => "QuickTime:Model".to_string(),
        "com.apple.quicktime.software" => "QuickTime:Software".to_string(),
        "com.apple.quicktime.creationdate" => "QuickTime:ContentCreateDate".to_string(),
        _ => format!("QuickTime:{}", key_name),
    }
}

/// Parse MP4 keys atom to build a map of key indices to key names
fn parse_mp4_keys(data: &[u8]) -> Result<HashMap<u32, String>, String> {
    let mut keys = HashMap::new();

    // Keys atom format:
    // 4 bytes: version + flags
    // 4 bytes: entry count
    // For each entry:
    //   4 bytes: key size
    //   4 bytes: key namespace (e.g., "mdta")
    //   N bytes: key value

    if data.len() < 8 {
        return Ok(keys);
    }

    let r = EndianReader::big_endian(data);
    let entry_count = r.u32_at(4).unwrap_or(0);
    let mut offset = 8;
    let mut index = 1; // Keys are 1-indexed

    for _ in 0..entry_count {
        if offset + 8 > data.len() {
            break;
        }

        let key_size = r.u32_at(offset).unwrap_or(0) as usize;

        if key_size < 8 || offset + key_size > data.len() {
            break;
        }

        // Skip namespace (4 bytes)
        let key_data = &data[offset + 8..offset + key_size];
        if let Ok(key_name) = std::str::from_utf8(key_data) {
            keys.insert(index, key_name.to_string());
        }

        offset += key_size;
        index += 1;
    }

    Ok(keys)
}

/// Extract string value from QuickTime user data atom
fn extract_string_value(data: &[u8]) -> Option<String> {
    // QuickTime user data format:
    // 2 bytes: data size
    // 2 bytes: language code
    // N bytes: string data

    if data.len() < 4 {
        return None;
    }

    let r = EndianReader::big_endian(data);
    let size = r.u16_at(0)? as usize;
    // Skip language code (2 bytes)
    let text_start = 4;

    if text_start >= data.len() {
        return None;
    }

    let text_data = &data[text_start..data.len().min(text_start + size)];
    String::from_utf8(text_data.to_vec()).ok()
}

/// Extract value from iTunes data atom
fn extract_itunes_data_value(data: &[u8]) -> Option<TagValue> {
    // iTunes data atom format:
    // 4 bytes: version + flags (type indicator)
    // 4 bytes: reserved (usually 0)
    // N bytes: actual data

    if data.len() < 8 {
        return None;
    }

    let r = EndianReader::big_endian(data);
    let type_indicator = r.u32_at(0)?;
    let value_data = &data[8..];

    match type_indicator {
        1 => {
            // UTF-8 text
            String::from_utf8(value_data.to_vec())
                .ok()
                .map(TagValue::String)
        }
        2 => {
            // UTF-16 text
            decode_utf16(value_data).map(TagValue::String)
        }
        21 => {
            // Signed integer (1, 2, 3, or 4 bytes)
            let vr = EndianReader::big_endian(value_data);
            match value_data.len() {
                1 => Some(TagValue::Integer(value_data[0] as i64)),
                2 => vr.i16_at(0).map(|v| TagValue::Integer(v as i64)),
                4 => vr.i32_at(0).map(|v| TagValue::Integer(v as i64)),
                _ => None,
            }
        }
        0 => {
            // Implicit data type (binary) - used for TrackNumber, DiscNumber, etc.
            Some(TagValue::Binary(value_data.to_vec()))
        }
        13 | 14 => {
            // JPEG or PNG image data
            Some(TagValue::Binary(value_data.to_vec()))
        }
        _ => {
            // Unknown type, try as string
            String::from_utf8(value_data.to_vec())
                .ok()
                .map(TagValue::String)
        }
    }
}

/// Decode UTF-16 big-endian text
fn decode_utf16(data: &[u8]) -> Option<String> {
    if !data.len().is_multiple_of(2) {
        return None;
    }

    // Use EndianReader to read each u16 in big-endian order
    let r = EndianReader::big_endian(data);
    let utf16_chars: Vec<u16> = (0..data.len() / 2)
        .filter_map(|i| r.u16_at(i * 2))
        .collect();

    String::from_utf16(&utf16_chars).ok()
}

/// Parse ISO 6709 GPS coordinate string
/// Format: +DD.DDDD+DDD.DDDD+AAA.AAA/ or variations
/// Returns (latitude, longitude, altitude)
fn parse_iso6709(gps_string: &str) -> Option<(f64, f64, Option<f64>)> {
    let s = gps_string.trim();
    if s.is_empty() {
        return None;
    }

    // Remove trailing slash if present
    let s = s.trim_end_matches('/');

    // Parse latitude (starts with + or -)
    let (lat_str, rest) = if let Some(pos) = s[1..].find(&['+', '-'][..]) {
        s.split_at(pos + 1)
    } else {
        return None;
    };

    let latitude = lat_str.parse::<f64>().ok()?;

    // Parse longitude (next + or -)
    let (lon_str, alt_str) = if let Some(pos) = rest[1..].find(&['+', '-'][..]) {
        rest.split_at(pos + 1)
    } else {
        (rest, "")
    };

    let longitude = lon_str.parse::<f64>().ok()?;

    // Parse altitude if present
    let altitude = if !alt_str.is_empty() {
        alt_str.parse::<f64>().ok()
    } else {
        None
    };

    Some((latitude, longitude, altitude))
}

/// Extract HEIF-specific metadata from meta atom including EXIF data
fn extract_heif_metadata(
    meta: &Atom,
    root_atoms: &[Atom],
    metadata: &mut MetadataMap,
) -> Result<(), String> {
    let meta_data = skip_version_flags(meta.data);
    let children = match super::atom_parser::parse_atoms(meta_data) {
        Ok((_, atoms)) => atoms,
        Err(_) => return Ok(()),
    };

    // Find Exif item ID from iinf atom
    let exif_item_id = find_exif_item_id(&children, metadata);

    // Parse iloc to build item locations
    let item_locations = parse_iloc_locations(&children);

    // Extract image dimensions from ispe atoms (in iprp->ipco)
    extract_ispe_dimensions(&children, metadata);

    // Extract primary item reference from pitm atom
    extract_primary_item_reference(&children, metadata);

    // Extract HEVC configuration from iprp->ipco container
    extract_heif_hevc_config(&children, metadata);

    // Extract EXIF data from mdat if we found an Exif item
    if let Some(id) = exif_item_id
        && let Some(&(offset, length)) = item_locations.get(&id)
    {
        extract_exif_from_mdat(root_atoms, offset, length, metadata);
    }

    Ok(())
}

/// Skip version/flags header if present in atom data
fn skip_version_flags(data: &[u8]) -> &[u8] {
    if data.len() >= 4 && data[0..4] == [0, 0, 0, 0] {
        &data[4..]
    } else {
        data
    }
}

/// Find the Exif item ID from iinf (item information) atom
fn find_exif_item_id(children: &[Atom], metadata: &mut MetadataMap) -> Option<u16> {
    let iinf = children.iter().find(|a| a.atom_type.matches("iinf"))?;
    if iinf.data.len() < 6 {
        return None;
    }

    let r = EndianReader::big_endian(iinf.data);
    let version = iinf.data[0];

    let (entry_count, entries_offset) = if version == 0 {
        (r.u16_at(4)? as u32, 6usize)
    } else if iinf.data.len() >= 8 {
        (r.u32_at(4)?, 8usize)
    } else {
        return None;
    };

    metadata.insert(
        "HEIF:ItemCount".to_string(),
        TagValue::Integer(entry_count as i64),
    );

    // Parse infe atoms to find Exif item
    let (_, infe_atoms) = super::atom_parser::parse_atoms(&iinf.data[entries_offset..]).ok()?;
    for infe in infe_atoms.iter().filter(|a| a.atom_type.matches("infe")) {
        if infe.data.len() >= 12 {
            let infe_reader = EndianReader::big_endian(infe.data);
            let item_id = infe_reader.u16_at(4).unwrap_or(0);
            if &infe.data[8..12] == b"Exif" {
                return Some(item_id);
            }
        }
    }
    None
}

/// Parse iloc (item location) atom to build item locations map
fn parse_iloc_locations(children: &[Atom]) -> HashMap<u16, (u64, u64)> {
    let mut locations = HashMap::new();

    let Some(iloc) = children.iter().find(|a| a.atom_type.matches("iloc")) else {
        return locations;
    };

    if iloc.data.len() < 8 {
        return locations;
    }

    let r = EndianReader::big_endian(iloc.data);
    let version = iloc.data[0];
    let offset_size = ((iloc.data[4] >> 4) & 0x0F) as usize;
    let length_size = (iloc.data[4] & 0x0F) as usize;
    let base_offset_size = ((iloc.data[5] >> 4) & 0x0F) as usize;

    let (item_count, mut pos) = if version < 2 {
        (r.u16_at(6).unwrap_or(0) as u32, 8usize)
    } else if iloc.data.len() >= 10 {
        (r.u32_at(6).unwrap_or(0), 10usize)
    } else {
        return locations;
    };

    for _ in 0..item_count {
        if pos + 2 > iloc.data.len() {
            break;
        }

        // Read item_id based on version
        let item_id = if version < 2 {
            let id = r.u16_at(pos).unwrap_or(0);
            pos += 2;
            id
        } else {
            let id = r.u32_at(pos).unwrap_or(0) as u16;
            pos += 4;
            id
        };

        if version >= 1 {
            pos += 2; // construction_method
        }
        pos += 2; // data_reference_index

        let base_offset = read_variable_size(iloc.data, &mut pos, base_offset_size);

        if pos + 2 > iloc.data.len() {
            break;
        }
        let extent_count = r.u16_at(pos).unwrap_or(0);
        pos += 2;

        if extent_count >= 1 {
            let extent_offset = read_variable_size(iloc.data, &mut pos, offset_size);
            let extent_length = read_variable_size(iloc.data, &mut pos, length_size);
            locations.insert(item_id, (base_offset + extent_offset, extent_length));

            // Skip remaining extents
            for _ in 1..extent_count {
                pos += offset_size + length_size;
            }
        }
    }

    locations
}

/// Extract image dimensions from ispe (image spatial extents) atoms
/// The ispe atom is located inside iprp -> ipco container
fn extract_ispe_dimensions(children: &[Atom], metadata: &mut MetadataMap) {
    // First, try to find ispe atoms in iprp -> ipco container (HEIF standard structure)
    if let Some(iprp) = children.iter().find(|a| a.atom_type.matches("iprp")) {
        if let Ok((_, iprp_children)) = super::atom_parser::parse_atoms(iprp.data) {
            if let Some(ipco) = iprp_children.iter().find(|a| a.atom_type.matches("ipco")) {
                if let Ok((_, ipco_children)) = super::atom_parser::parse_atoms(ipco.data) {
                    for atom in &ipco_children {
                        if atom.atom_type.matches("ispe") && atom.data.len() >= 12 {
                            let r = EndianReader::big_endian(atom.data);
                            if let (Some(width), Some(height)) = (r.u32_at(4), r.u32_at(8))
                                && !metadata.contains_key("HEIF:ImageWidth")
                            {
                                metadata.insert(
                                    "HEIF:ImageWidth".to_string(),
                                    TagValue::Integer(width as i64),
                                );
                                metadata.insert(
                                    "HEIF:ImageHeight".to_string(),
                                    TagValue::Integer(height as i64),
                                );
                                // Also add ImageSpatialExtent in ExifTool format
                                metadata.insert(
                                    "QuickTime:ImageSpatialExtent".to_string(),
                                    TagValue::String(format!("{}x{}", width, height)),
                                );
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback: check if ispe atoms are directly in children (legacy path)
    for atom in children {
        if atom.atom_type.matches("ispe") && atom.data.len() >= 12 {
            let r = EndianReader::big_endian(atom.data);
            if let (Some(width), Some(height)) = (r.u32_at(4), r.u32_at(8))
                && !metadata.contains_key("HEIF:ImageWidth")
            {
                metadata.insert(
                    "HEIF:ImageWidth".to_string(),
                    TagValue::Integer(width as i64),
                );
                metadata.insert(
                    "HEIF:ImageHeight".to_string(),
                    TagValue::Integer(height as i64),
                );
                // Also add ImageSpatialExtent in ExifTool format
                metadata.insert(
                    "QuickTime:ImageSpatialExtent".to_string(),
                    TagValue::String(format!("{}x{}", width, height)),
                );
                return;
            }
        }
    }
}

/// Extract primary item reference from pitm atom
fn extract_primary_item_reference(children: &[Atom], metadata: &mut MetadataMap) {
    if let Some(pitm) = children.iter().find(|a| a.atom_type.matches("pitm")) {
        if pitm.data.len() >= 6 {
            let version = pitm.data[0];
            let item_id = if version == 0 {
                u16::from_be_bytes([pitm.data[4], pitm.data[5]]) as u32
            } else if pitm.data.len() >= 8 {
                u32::from_be_bytes([pitm.data[4], pitm.data[5], pitm.data[6], pitm.data[7]])
            } else {
                return;
            };
            metadata.insert(
                "QuickTime:PrimaryItemReference".to_string(),
                TagValue::Integer(item_id as i64),
            );
        }
    }
}

/// Extract HEVC configuration from HEIF item properties (iprp -> ipco -> hvcC)
fn extract_heif_hevc_config(children: &[Atom], metadata: &mut MetadataMap) {
    // Find iprp (item properties) atom
    let Some(iprp) = children.iter().find(|a| a.atom_type.matches("iprp")) else {
        return;
    };

    // Parse children of iprp
    let iprp_children = match super::atom_parser::parse_atoms(iprp.data) {
        Ok((_, atoms)) => atoms,
        Err(_) => return,
    };

    // Find ipco (item property container) atom
    let Some(ipco) = iprp_children.iter().find(|a| a.atom_type.matches("ipco")) else {
        return;
    };

    // Parse children of ipco - this contains the actual properties
    let ipco_children = match super::atom_parser::parse_atoms(ipco.data) {
        Ok((_, atoms)) => atoms,
        Err(_) => return,
    };

    // Look for hvcC atom in ipco children
    for atom in &ipco_children {
        if atom.atom_type.matches("hvcC") && atom.data.len() >= 23 {
            // Parse HEVC configuration directly from the atom data
            let hvcc_data = atom.data;

            // configurationVersion (1 byte)
            let config_version = hvcc_data[0];
            metadata.insert(
                "QuickTime:HEVCConfigurationVersion".to_string(),
                TagValue::Integer(config_version as i64),
            );

            // general_profile_space (2 bits) + general_tier_flag (1 bit) + general_profile_idc (5 bits)
            let profile_byte = hvcc_data[1];
            let profile_space = (profile_byte >> 6) & 0x03;
            let tier_flag = (profile_byte >> 5) & 0x01;
            let profile_idc = profile_byte & 0x1F;

            let profile_space_str = match profile_space {
                0 => "Conforming",
                1 => "Reserved 1",
                2 => "Reserved 2",
                3 => "Reserved 3",
                _ => "Unknown",
            };
            metadata.insert(
                "QuickTime:GeneralProfileSpace".to_string(),
                TagValue::String(profile_space_str.to_string()),
            );

            let tier_str = if tier_flag == 0 {
                "Main Tier"
            } else {
                "High Tier"
            };
            metadata.insert(
                "QuickTime:GeneralTierFlag".to_string(),
                TagValue::String(tier_str.to_string()),
            );

            let profile_name = match profile_idc {
                1 => "Main",
                2 => "Main 10",
                3 => "Main Still Picture",
                4 => "Format Range Extensions",
                5 => "High Throughput",
                6 => "Multiview Main",
                7 => "Scalable Main",
                8 => "3D Main",
                9 => "Screen-Extended Main",
                10 => "Scalable Format Range Extensions",
                11 => "High Throughput Screen-Extended",
                _ => "Unknown",
            };
            metadata.insert(
                "QuickTime:GeneralProfileIDC".to_string(),
                TagValue::String(profile_name.to_string()),
            );

            // general_profile_compatibility_flags (4 bytes)
            let compat_flags =
                u32::from_be_bytes([hvcc_data[2], hvcc_data[3], hvcc_data[4], hvcc_data[5]]);
            let mut compat_profiles = Vec::new();
            if compat_flags & (1 << 31) != 0 {
                compat_profiles.push("Main");
            }
            if compat_flags & (1 << 30) != 0 {
                compat_profiles.push("Main 10");
            }
            if compat_flags & (1 << 29) != 0 {
                compat_profiles.push("Main Still Picture");
            }
            if !compat_profiles.is_empty() {
                metadata.insert(
                    "QuickTime:GenProfileCompatibilityFlags".to_string(),
                    TagValue::String(compat_profiles.join(", ")),
                );
            }

            // general_constraint_indicator_flags (6 bytes at offset 6-11)
            let constraint_bytes: Vec<String> =
                hvcc_data[6..12].iter().map(|b| format!("{}", b)).collect();
            metadata.insert(
                "QuickTime:ConstraintIndicatorFlags".to_string(),
                TagValue::String(constraint_bytes.join(" ")),
            );

            // general_level_idc (1 byte at offset 12)
            let level_idc = hvcc_data[12];
            let level = level_idc as f64 / 30.0;
            metadata.insert(
                "QuickTime:GeneralLevelIDC".to_string(),
                TagValue::String(format!("{} (level {:.1})", level_idc, level)),
            );

            // min_spatial_segmentation_idc (4 bits reserved + 12 bits value at offset 13-14)
            let min_spatial = u16::from_be_bytes([hvcc_data[13], hvcc_data[14]]) & 0x0FFF;
            metadata.insert(
                "QuickTime:MinSpatialSegmentationIDC".to_string(),
                TagValue::Integer(min_spatial as i64),
            );

            // parallelismType (6 bits reserved + 2 bits value at offset 15)
            let parallelism = hvcc_data[15] & 0x03;
            metadata.insert(
                "QuickTime:ParallelismType".to_string(),
                TagValue::Integer(parallelism as i64),
            );

            // chromaFormat (6 bits reserved + 2 bits value at offset 16)
            let chroma_format = hvcc_data[16] & 0x03;
            let chroma_str = match chroma_format {
                0 => "Monochrome",
                1 => "4:2:0",
                2 => "4:2:2",
                3 => "4:4:4",
                _ => "Unknown",
            };
            metadata.insert(
                "QuickTime:ChromaFormat".to_string(),
                TagValue::String(chroma_str.to_string()),
            );

            // bitDepthLumaMinus8 (5 bits reserved + 3 bits value at offset 17)
            let bit_depth_luma = (hvcc_data[17] & 0x07) + 8;
            metadata.insert(
                "QuickTime:BitDepthLuma".to_string(),
                TagValue::Integer(bit_depth_luma as i64),
            );

            // bitDepthChromaMinus8 (5 bits reserved + 3 bits value at offset 18)
            let bit_depth_chroma = (hvcc_data[18] & 0x07) + 8;
            metadata.insert(
                "QuickTime:BitDepthChroma".to_string(),
                TagValue::Integer(bit_depth_chroma as i64),
            );

            // avgFrameRate (2 bytes at offset 19-20)
            let avg_frame_rate = u16::from_be_bytes([hvcc_data[19], hvcc_data[20]]);
            metadata.insert(
                "QuickTime:AverageFrameRate".to_string(),
                TagValue::Integer(avg_frame_rate as i64),
            );

            // Byte at offset 21: constantFrameRate (2 bits) + numTemporalLayers (3 bits) + temporalIdNested (1 bit) + lengthSizeMinusOne (2 bits)
            let flags_byte = hvcc_data[21];
            let constant_frame_rate = (flags_byte >> 6) & 0x03;
            let num_temporal_layers = (flags_byte >> 3) & 0x07;
            let temporal_id_nested = (flags_byte >> 2) & 0x01;

            let cfr_str = match constant_frame_rate {
                0 => "Unknown",
                1 => "Constant",
                2 => "Variable",
                _ => "Reserved",
            };
            metadata.insert(
                "QuickTime:ConstantFrameRate".to_string(),
                TagValue::String(cfr_str.to_string()),
            );

            metadata.insert(
                "QuickTime:NumTemporalLayers".to_string(),
                TagValue::Integer(num_temporal_layers as i64),
            );

            metadata.insert(
                "QuickTime:TemporalIDNested".to_string(),
                TagValue::String(if temporal_id_nested == 1 { "Yes" } else { "No" }.to_string()),
            );

            return;
        }
    }
}

/// Extract EXIF data from mdat atom using iloc offset/length
fn extract_exif_from_mdat(
    root_atoms: &[Atom],
    offset: u64,
    length: u64,
    metadata: &mut MetadataMap,
) {
    let Some(mdat) = root_atoms.iter().find(|a| a.atom_type.matches("mdat")) else {
        return;
    };

    let exif_length = length as usize;

    // Try to find EXIF data with different header size assumptions
    let tiff_data = [8u64, 16u64].iter().find_map(|&header_size| {
        let file_offset: u64 = root_atoms
            .iter()
            .take_while(|a| !a.atom_type.matches("mdat"))
            .map(|a| 8 + a.data.len() as u64)
            .sum();
        let mdat_start = file_offset + header_size;

        if offset >= mdat_start {
            let mdat_offset = (offset - mdat_start) as usize;
            if mdat_offset + exif_length <= mdat.data.len() {
                let exif_data = &mdat.data[mdat_offset..mdat_offset + exif_length];
                if exif_data.len() >= 10 && &exif_data[4..8] == b"Exif" {
                    return Some(&exif_data[10..]);
                }
            }
        }
        None
    });

    // Fallback: try direct offset
    let tiff_data = tiff_data.or_else(|| {
        let off = offset as usize;
        if off + exif_length <= mdat.data.len() {
            let exif_data = &mdat.data[off..off + exif_length];
            if exif_data.len() >= 10 && &exif_data[4..8] == b"Exif" {
                return Some(&exif_data[10..]);
            }
        }
        None
    });

    if let Some(data) = tiff_data {
        let _ = parse_heif_exif_data(data, metadata);
    }
}

/// Helper function to read variable-size integers from iloc data
fn read_variable_size(data: &[u8], pos: &mut usize, size: usize) -> u64 {
    if *pos + size > data.len() {
        return 0;
    }

    let r = EndianReader::big_endian(data);
    let value = match size {
        0 => 0u64,
        1 => data[*pos] as u64,
        2 => r.u16_at(*pos).unwrap_or(0) as u64,
        4 => r.u32_at(*pos).unwrap_or(0) as u64,
        8 => r.u64_at(*pos).unwrap_or(0),
        _ => 0,
    };
    *pos += size;
    value
}

/// Simple in-memory FileReader for EXIF data embedded in HEIF files
struct HeifExifDataReader {
    data: Vec<u8>,
}

impl HeifExifDataReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl FileReader for HeifExifDataReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start + length;

        if end > self.data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "read beyond end of EXIF data",
            ));
        }

        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

/// Parse TIFF/EXIF data from HEIF Exif item and insert into metadata
fn parse_heif_exif_data(tiff_data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    if tiff_data.len() < 8 {
        return Err("TIFF data too short".to_string());
    }

    // Detect byte order from TIFF header
    let byte_order = match &tiff_data[0..2] {
        b"II" => TiffByteOrder::LittleEndian,
        b"MM" => TiffByteOrder::BigEndian,
        _ => return Err("Invalid TIFF byte order marker".to_string()),
    };

    // Create EndianReader with detected byte order
    let r = EndianReader::new(tiff_data, tiff_to_byte_order(byte_order));

    // Verify TIFF magic number (0x002A)
    let magic = r.u16_at(2).ok_or("Failed to read TIFF magic number")?;

    if magic != 0x002A {
        return Err(format!("Invalid TIFF magic number: 0x{:04X}", magic));
    }

    // Read IFD0 offset
    let ifd_offset = r.u32_at(4).ok_or("Failed to read IFD0 offset")?;

    // Create a reader for the TIFF data
    let exif_reader = HeifExifDataReader::new(tiff_data.to_vec());

    // Track sub-IFD offsets
    let mut exif_ifd_offset = None;
    let mut gps_ifd_offset = None;

    // Parse IFD0
    let ifd0_tags = parse_ifd(&exif_reader, ifd_offset as u64, byte_order)
        .map_err(|e| format!("Failed to parse IFD0: {}", e))?;

    for (tag_id, field_type, value_count, raw_bytes) in &ifd0_tags {
        // Create reader for raw bytes with detected byte order
        let raw_reader = EndianReader::new(raw_bytes, tiff_to_byte_order(byte_order));

        // Check for ExifIFD pointer (tag 0x8769)
        if *tag_id == 0x8769 && raw_bytes.len() >= 4 {
            if let Some(offset) = raw_reader.u32_at(0) {
                exif_ifd_offset = Some(offset as u64);
            }
            continue;
        }

        // Check for GPS Sub-IFD pointer (tag 0x8825)
        if *tag_id == 0x8825 && raw_bytes.len() >= 4 {
            if let Some(offset) = raw_reader.u32_at(0) {
                gps_ifd_offset = Some(offset as u64);
            }
            continue;
        }

        // Convert tag to name and value
        let tag_name = lookup_tag_name(*tag_id, "IFD0");
        let tag_value = raw_bytes_to_tag_value(raw_bytes, *field_type, *value_count, byte_order);
        metadata.insert(tag_name, tag_value);
    }

    // Parse ExifIFD if present
    if let Some(offset) = exif_ifd_offset
        && let Ok(exif_tags) = parse_ifd(&exif_reader, offset, byte_order)
    {
        for (tag_id, field_type, value_count, raw_bytes) in exif_tags {
            let tag_name = lookup_tag_name(tag_id, "ExifIFD");
            let tag_value = raw_bytes_to_tag_value(&raw_bytes, field_type, value_count, byte_order);
            metadata.insert(tag_name, tag_value);
        }
    }

    // Parse GPS IFD if present
    if let Some(offset) = gps_ifd_offset
        && let Ok(gps_tags) = parse_ifd(&exif_reader, offset, byte_order)
    {
        for (tag_id, field_type, value_count, raw_bytes) in gps_tags {
            let tag_name = lookup_tag_name(tag_id, "GPS");
            let tag_value = raw_bytes_to_tag_value(&raw_bytes, field_type, value_count, byte_order);
            metadata.insert(tag_name, tag_value);
        }
    }

    Ok(())
}

/// Convert raw EXIF bytes to TagValue
fn raw_bytes_to_tag_value(
    bytes: &[u8],
    field_type: u16,
    value_count: u32,
    byte_order: TiffByteOrder,
) -> TagValue {
    use crate::parsers::common::exif_types::ExifType;

    // Create EndianReader with the TIFF byte order (converted to crate's ByteOrder)
    let r = EndianReader::new(bytes, tiff_to_byte_order(byte_order));

    let Some(exif_type) = ExifType::from_u16(field_type) else {
        return TagValue::Binary(bytes.to_vec());
    };

    match exif_type {
        ExifType::Byte if !bytes.is_empty() => {
            if value_count == 1 {
                TagValue::Integer(bytes[0] as i64)
            } else {
                TagValue::Binary(bytes.to_vec())
            }
        }
        ExifType::Ascii => {
            let text = String::from_utf8_lossy(bytes);
            TagValue::String(text.trim_end_matches('\0').to_string())
        }
        ExifType::Short if r.len() >= 2 => {
            if value_count == 1 {
                r.u16_at(0)
                    .map(|v| TagValue::Integer(v as i64))
                    .unwrap_or_else(|| TagValue::Binary(bytes.to_vec()))
            } else {
                let values: Vec<_> = (0..value_count as usize)
                    .filter_map(|i| r.u16_at(i * 2).map(|v| v.to_string()))
                    .collect();
                TagValue::String(values.join(" "))
            }
        }
        ExifType::Long if r.len() >= 4 => r
            .u32_at(0)
            .map(|v| TagValue::Integer(v as i64))
            .unwrap_or_else(|| TagValue::Binary(bytes.to_vec())),
        ExifType::Rational if r.len() >= 8 => {
            if value_count == 1 {
                // Use helper function to compute rational as f64
                rational_to_f64(&r, 0)
                    .map(TagValue::Float)
                    .unwrap_or_else(|| TagValue::Binary(bytes.to_vec()))
            } else {
                let values: Vec<_> = (0..value_count as usize)
                    .filter_map(|i| rational_to_f64(&r, i * 8).map(|v| format!("{}", v)))
                    .collect();
                TagValue::String(values.join(" "))
            }
        }
        ExifType::SByte if !bytes.is_empty() => TagValue::Integer(bytes[0] as i8 as i64),
        ExifType::Undefined => {
            if bytes
                .iter()
                .all(|&b| b.is_ascii_graphic() || b.is_ascii_whitespace() || b == 0)
            {
                let text = String::from_utf8_lossy(bytes);
                let trimmed = text.trim_end_matches('\0');
                if !trimmed.is_empty() {
                    return TagValue::String(trimmed.to_string());
                }
            }
            TagValue::Binary(bytes.to_vec())
        }
        ExifType::SShort if r.len() >= 2 => r
            .i16_at(0)
            .map(|v| TagValue::Integer(v as i64))
            .unwrap_or_else(|| TagValue::Binary(bytes.to_vec())),
        ExifType::SLong if r.len() >= 4 => r
            .i32_at(0)
            .map(|v| TagValue::Integer(v as i64))
            .unwrap_or_else(|| TagValue::Binary(bytes.to_vec())),
        ExifType::SRational if r.len() >= 8 => {
            // Use helper function to compute signed rational as f64
            srational_to_f64(&r, 0)
                .map(TagValue::Float)
                .unwrap_or_else(|| TagValue::Binary(bytes.to_vec()))
        }
        ExifType::Float if r.len() >= 4 => r
            .f32_at(0)
            .map(|v| TagValue::Float(v as f64))
            .unwrap_or_else(|| TagValue::Binary(bytes.to_vec())),
        ExifType::Double if r.len() >= 8 => r
            .f64_at(0)
            .map(TagValue::Float)
            .unwrap_or_else(|| TagValue::Binary(bytes.to_vec())),
        _ => TagValue::Binary(bytes.to_vec()),
    }
}

/// Formats an exposure time in seconds the way ExifTool's `PrintExposureTime`
/// does: as a `1/N` fraction for sub-quarter-second exposures, otherwise as a
/// decimal number of seconds (with a trailing `.0` stripped).
fn format_exposure_time_seconds(secs: f64) -> String {
    if secs > 0.0 && secs < 0.25001 {
        format!("1/{}", (0.5 + 1.0 / secs) as i64)
    } else {
        let s = format!("{:.1}", secs);
        s.strip_suffix(".0").map(str::to_string).unwrap_or(s)
    }
}

/// Extract Pentax MakerNotes from the TAGS atom in QuickTime/MOV files.
///
/// This mirrors ExifTool's `Image::ExifTool::Pentax::MOV` table (a
/// `ProcessBinaryData` block, little-endian, found in movies such as the
/// Optio WP): a fixed-size layout keyed off absolute byte offsets from the
/// start of the atom payload, not relative to the end of the maker string.
fn extract_pentax_maker_notes(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    // ExifTool only recognizes this sub-format when the payload starts with
    // this exact maker string (see QuickTime.pm's `TAGS` tag definition).
    if !data.starts_with(b"PENTAX DIGITAL CAMERA\0") || data.len() < 24 {
        return Ok(());
    }

    // 0x00: Make, string[24]
    if let Ok(make) = std::str::from_utf8(&data[0..24]) {
        let make_trimmed = make.trim_end_matches('\0').trim();
        if !make_trimmed.is_empty() {
            metadata.insert(
                "MakerNotes:Make".to_string(),
                TagValue::new_string(make_trimmed.to_string()),
            );
        }
    }

    let r = EndianReader::little_endian(data);

    // 0x26: ExposureTime, int32u; ValueConv: $val ? 10/$val : 0
    if let Some(raw) = r.u32_at(0x26) {
        if raw != 0 {
            let secs = 10.0 / raw as f64;
            metadata.insert(
                "MakerNotes:ExposureTime".to_string(),
                TagValue::new_string(format_exposure_time_seconds(secs)),
            );
        }
    }

    // 0x2a: FNumber, rational64u; PrintConv: sprintf("%.1f", $val)
    if let Some((num, den)) = r.rational_at(0x2a) {
        if den != 0 {
            let fnumber = num as f64 / den as f64;
            metadata.insert(
                "MakerNotes:FNumber".to_string(),
                TagValue::new_string(format!("{:.1}", fnumber)),
            );
        }
    }

    // 0x32: ExposureCompensation, rational64s; PrintConv: $val ? "+X.X" : 0
    if let Some((num, den)) = r.srational_at(0x32) {
        if den != 0 {
            let val = num as f64 / den as f64;
            let ec_str = if val != 0.0 {
                format!("{:+.1}", val)
            } else {
                "0".to_string()
            };
            metadata.insert(
                "MakerNotes:ExposureCompensation".to_string(),
                TagValue::new_string(ec_str),
            );
        }
    }

    // 0x44: WhiteBalance, int16u
    if let Some(wb) = r.u16_at(0x44) {
        let wb_str = match wb {
            0 => Some("Auto"),
            1 => Some("Daylight"),
            2 => Some("Shade"),
            3 => Some("Fluorescent"),
            4 => Some("Tungsten"),
            5 => Some("Manual"),
            _ => None,
        };
        if let Some(wb_str) = wb_str {
            metadata.insert(
                "MakerNotes:WhiteBalance".to_string(),
                TagValue::new_string(wb_str.to_string()),
            );
        }
    }

    // 0x48: FocalLength, rational64u; PrintConv: sprintf("%.1f mm", $val)
    if let Some((num, den)) = r.rational_at(0x48) {
        if den != 0 {
            let focal_length = num as f64 / den as f64;
            metadata.insert(
                "MakerNotes:FocalLength".to_string(),
                TagValue::new_string(format!("{:.1} mm", focal_length)),
            );
        }
    }

    // 0xaf: ISO, int16u
    if let Some(iso) = r.u16_at(0xaf) {
        metadata.insert("MakerNotes:ISO".to_string(), TagValue::Integer(iso as i64));
    }

    Ok(())
}

/// Extract XMP metadata from XMP_ atom in QuickTime files
fn extract_xmp_from_atom(data: &[u8], metadata: &mut MetadataMap) -> Result<(), String> {
    // XMP_ atom contains raw XMP data (XML format)
    // Find the start of XMP data - may start directly or after some header
    let xmp_start = if data.starts_with(b"<?xpacket") {
        0
    } else if let Some(pos) = data.windows(9).position(|w| w == b"<?xpacket") {
        pos
    } else {
        return Ok(());
    };

    let xmp_data = &data[xmp_start..];

    // Parse XMP tags using the existing XMP parser (takes bytes)
    if let Ok(xmp_tags) = crate::parsers::xmp::rdf_parser::parse_xmp(xmp_data) {
        for (key, value) in xmp_tags {
            // The XMP parser returns keys without "XMP:" prefix, so add it
            // But some keys might already be prefixed, so check first
            let full_key = if key.starts_with("XMP:") {
                key
            } else {
                format!("XMP:{}", key)
            };
            metadata.insert(full_key, TagValue::new_string(value));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::quicktime::FourCC;

    #[test]
    fn test_extract_string_value() {
        // Create QuickTime user data: size=11, lang=0, text="Hello World"
        let data = [
            0x00, 0x0B, // size = 11
            0x00, 0x00, // language = 0
            b'H', b'e', b'l', b'l', b'o', b' ', b'W', b'o', b'r', b'l', b'd',
        ];

        let result = extract_string_value(&data);
        assert_eq!(result, Some("Hello World".to_string()));
    }

    #[test]
    fn test_extract_itunes_utf8_value() {
        // Create iTunes data atom: type=1 (UTF-8), reserved=0, text="Test"
        let data = [
            0x00, 0x00, 0x00, 0x01, // type = 1 (UTF-8)
            0x00, 0x00, 0x00, 0x00, // reserved
            b'T', b'e', b's', b't', // text = "Test"
        ];

        let result = extract_itunes_data_value(&data);
        match result {
            Some(TagValue::String(s)) => assert_eq!(s, "Test"),
            _ => panic!("Expected string value"),
        }
    }

    #[test]
    fn test_extract_itunes_integer_value() {
        // Create iTunes data atom: type=21 (signed int), value=42
        let data = [
            0x00, 0x00, 0x00, 0x15, // type = 21 (signed int)
            0x00, 0x00, 0x00, 0x00, // reserved
            0x00, 0x00, 0x00, 0x2A, // value = 42
        ];

        let result = extract_itunes_data_value(&data);
        match result {
            Some(TagValue::Integer(i)) => assert_eq!(i, 42),
            _ => panic!("Expected integer value"),
        }
    }

    #[test]
    fn test_decode_utf16() {
        // "Hi" in UTF-16 BE
        let data = [0x00, 0x48, 0x00, 0x69]; // H=0x0048, i=0x0069

        let result = decode_utf16(&data);
        assert_eq!(result, Some("Hi".to_string()));
    }

    #[test]
    fn test_format_mac_time_legacy() {
        // Test zero time
        assert_eq!(format_mac_time_legacy(0), "0000:00:00 00:00:00");

        // Test Mac epoch (1904-01-01)
        const MAC_EPOCH_OFFSET: u64 = 2082844800;
        assert_eq!(
            format_mac_time_legacy(MAC_EPOCH_OFFSET),
            "1904:01:01 00:00:00"
        );

        // Test Unix epoch (1970-01-01) - this should use mac_time_to_iso8601 in practice
        assert_eq!(
            format_mac_time_legacy(MAC_EPOCH_OFFSET),
            "1904:01:01 00:00:00"
        );

        // Test a known timestamp: 2024-01-01 00:00:00 UTC
        // Unix timestamp for 2024-01-01: 1704067200
        let mac_time = 1704067200 + MAC_EPOCH_OFFSET;
        let result = format_mac_time_legacy(mac_time);
        assert!(result.starts_with("2024:01:01"));
    }

    #[test]
    fn test_mac_time_to_iso8601_integration() {
        // Test that the shared timestamp utility works for dates after 1970
        const MAC_EPOCH_OFFSET: u64 = 2082844800;

        // Unix timestamp for 2024-01-01: 1704067200
        let mac_time = 1704067200 + MAC_EPOCH_OFFSET;
        let result = mac_time_to_iso8601(mac_time);
        assert!(result.is_some());
        assert!(result.unwrap().starts_with("2024-01-01"));

        // Test that dates before 1970 return None
        let old_mac_time = 100u64; // Very early date (1904)
        assert!(mac_time_to_iso8601(old_mac_time).is_none());
    }

    #[test]
    fn test_is_leap_year() {
        assert!(is_leap_year(2000)); // Divisible by 400
        assert!(is_leap_year(2004)); // Divisible by 4, not by 100
        assert!(!is_leap_year(1900)); // Divisible by 100, not by 400
        assert!(!is_leap_year(2001)); // Not divisible by 4
        assert!(is_leap_year(2024));
    }

    #[test]
    fn test_days_to_month_day() {
        // Test regular year
        assert_eq!(days_to_month_day(0, false), (1, 1)); // Jan 1
        assert_eq!(days_to_month_day(31, false), (2, 1)); // Feb 1
        assert_eq!(days_to_month_day(59, false), (3, 1)); // Mar 1
        assert_eq!(days_to_month_day(364, false), (12, 31)); // Dec 31

        // Test leap year
        assert_eq!(days_to_month_day(59, true), (2, 29)); // Feb 29
        assert_eq!(days_to_month_day(60, true), (3, 1)); // Mar 1
    }

    #[test]
    fn test_parse_iso6709_basic() {
        // Test basic GPS coordinates
        let result = parse_iso6709("+37.7749-122.4194/");
        assert!(result.is_some());
        let (lat, lon, alt) = result.unwrap();
        assert!((lat - 37.7749).abs() < 0.0001);
        assert!((lon - (-122.4194)).abs() < 0.0001);
        assert!(alt.is_none());
    }

    #[test]
    fn test_parse_iso6709_with_altitude() {
        // Test GPS coordinates with altitude
        let result = parse_iso6709("+40.7128-074.0060+010.5/");
        assert!(result.is_some());
        let (lat, lon, alt) = result.unwrap();
        assert!((lat - 40.7128).abs() < 0.0001);
        assert!((lon - (-74.0060)).abs() < 0.0001);
        assert!(alt.is_some());
        assert!((alt.unwrap() - 10.5).abs() < 0.01);
    }

    #[test]
    fn test_parse_iso6709_no_slash() {
        // Test without trailing slash
        let result = parse_iso6709("+51.5074-000.1278");
        assert!(result.is_some());
        let (lat, lon, _) = result.unwrap();
        assert!((lat - 51.5074).abs() < 0.0001);
        assert!((lon - (-0.1278)).abs() < 0.0001);
    }

    #[test]
    fn test_parse_iso6709_invalid() {
        // Test invalid inputs
        assert!(parse_iso6709("").is_none());
        assert!(parse_iso6709("invalid").is_none());
        assert!(parse_iso6709("+37.7749").is_none()); // Missing longitude
    }

    #[test]
    fn test_map_apple_key_to_tag() {
        assert_eq!(
            map_apple_key_to_tag("com.apple.quicktime.location.ISO6709"),
            "QuickTime:GPSCoordinates"
        );
        assert_eq!(
            map_apple_key_to_tag("com.apple.quicktime.make"),
            "QuickTime:Make"
        );
        assert_eq!(
            map_apple_key_to_tag("com.apple.quicktime.model"),
            "QuickTime:Model"
        );
        assert_eq!(
            map_apple_key_to_tag("com.apple.quicktime.software"),
            "QuickTime:Software"
        );
        assert_eq!(map_apple_key_to_tag("unknown.key"), "QuickTime:unknown.key");
    }

    #[test]
    fn test_extract_video_frame_rate() {
        // Create a minimal stts atom with:
        // - version/flags: 0
        // - entry count: 1
        // - sample count: 30
        // - sample delta: 1001 (for ~29.97 fps at 30000 timescale)
        let stts_data = [
            0x00, 0x00, 0x00, 0x00, // version/flags
            0x00, 0x00, 0x00, 0x01, // entry count = 1
            0x00, 0x00, 0x00, 0x1E, // sample count = 30
            0x00, 0x00, 0x03, 0xE9, // sample delta = 1001
        ];

        let stts = Atom {
            atom_type: FourCC::from_string("stts").unwrap(),
            data: &stts_data,
            header_size: 8,
        };

        let mut metadata = MetadataMap::new();
        // Set MediaTimeScale to 30000 (common for 29.97 fps video)
        metadata.insert(
            "QuickTime:MediaTimeScale".to_string(),
            TagValue::Integer(30000),
        );

        let result = extract_video_frame_rate(&stts, &mut metadata, 0);
        assert!(result.is_ok());

        // Should have VideoFrameRate = 30000 / 1001 = ~29.97
        assert!(metadata.contains_key("QuickTime:VideoFrameRate"));
        if let Some(TagValue::Float(fps)) = metadata.get("QuickTime:VideoFrameRate") {
            assert!((fps - 29.97).abs() < 0.01);
        } else {
            panic!("Expected float value for VideoFrameRate");
        }
    }

    #[test]
    fn test_extract_video_frame_rate_30fps() {
        // Test exact 30 fps (timescale 600, delta 20)
        let stts_data = [
            0x00, 0x00, 0x00, 0x00, // version/flags
            0x00, 0x00, 0x00, 0x01, // entry count = 1
            0x00, 0x00, 0x00, 0x64, // sample count = 100
            0x00, 0x00, 0x00, 0x14, // sample delta = 20
        ];

        let stts = Atom {
            atom_type: FourCC::from_string("stts").unwrap(),
            data: &stts_data,
            header_size: 8,
        };

        let mut metadata = MetadataMap::new();
        // Set MediaTimeScale to 600 (common QuickTime timescale)
        metadata.insert(
            "QuickTime:MediaTimeScale".to_string(),
            TagValue::Integer(600),
        );

        let result = extract_video_frame_rate(&stts, &mut metadata, 0);
        assert!(result.is_ok());

        // Should have VideoFrameRate = 600 / 20 = 30.0
        if let Some(TagValue::Float(fps)) = metadata.get("QuickTime:VideoFrameRate") {
            assert!((fps - 30.0).abs() < 0.01);
        } else {
            panic!("Expected float value for VideoFrameRate");
        }
    }
}
