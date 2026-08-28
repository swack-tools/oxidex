//! Windows Recorded TV Show (WTV) reader.
//!
//! `WTV::ProcessWTV` (WTV.pm:213-267) validates a 16-byte container GUID,
//! reads a sector size from byte 0x28, gathers the WTV directory out of the
//! sector table at 0x38, and walks that directory's entries. Exactly one entry
//! is followed: `WTV::Main` (WTV.pm:32-46) declares a single tag,
//! `table.0.entries.legacy_attrib`, whose `SubDirectory` is `WTV::Metadata`;
//! every other directory name in the file is listed there only as a commented
//! "not decoded" line, and `next unless $$tagTablePtr{$tag}` skips it.
//!
//! `WTV::Metadata`'s `ProcessMetadata` (WTV.pm:169-211) then walks a flat run
//! of records, each `GUID(16) | format(4) | length(4) | UTF-16LE name NUL NUL
//! | length bytes of data`, and decodes the data by that format word.
//!
//! # Why there is no transcribed table to consult
//!
//! Neither WTV table is a `ProcessBinaryData` layout -- one is a
//! `SubDirectory` edge keyed by a directory *name*, the other is a
//! `PROCESS_PROC` over self-describing records with `VARS => { ID_FMT =>
//! 'none' }` -- so `src/exiftool_tables` emits neither, and
//! `find_table("WTV", ...)` is `None` by construction rather than by
//! omission. The name table below is transcribed by hand from WTV.pm:50-147
//! with the line of each entry cited.
//!
//! # What is deliberately absent
//!
//! Four `WTV::Metadata` entries carry `Unknown => 1`, which keeps them out of
//! ordinary output (they appear only under `exiftool -U`): `MediaThumbTimeStamp`
//! (WTV.pm:101), `Bitrate` (WTV.pm:119), `ExpirationDate` (WTV.pm:129) and
//! `ExpirationSpan` (WTV.pm:130). They are recognised here -- so their raw
//! records are still walked past correctly -- and then dropped, matching what
//! the oracle prints.
//!
//! # References
//!
//! - ExifTool source: `lib/Image/ExifTool/WTV.pm`

use crate::core::file_metadata::round_to_second;
use crate::core::formatters::convert_duration;
use crate::core::value_formatter::format_exif_datetime;
use crate::core::{FileReader, MetadataMap, TagValue};

/// WTV.pm:214-215, the container GUID `ProcessWTV` requires at offset 0.
const FILE_GUID: &[u8; 16] = b"\xb7\xd8\x00\x20\x37\x49\xda\x11\xa6\x4e\x00\x07\xe9\x5e\xad\x8d";
/// WTV.pm:216, `$raf->Read($buff, 0x60) == 0x60`.
const FILE_HEADER_LEN: usize = 0x60;
/// WTV.pm:220, `Get32u(\$buff, 0x28)` -- the sector size.
const SECTOR_SIZE_OFFSET: usize = 0x28;
/// WTV.pm:223, the two sector sizes ExifTool will accept: the standard 0x1000
/// and the 0x100 its own test file uses.
const STANDARD_SECTOR_SIZE: u32 = 0x1000;
const TEST_SECTOR_SIZE: u32 = 0x100;
/// WTV.pm:224, the offset of the sector table holding the WTV directory.
const DIRECTORY_SECTOR_TABLE: usize = 0x38;
/// WTV.pm:232, the GUID every WTV directory entry starts with.
const DIRECTORY_ENTRY_GUID: &[u8; 16] =
    b"\x92\xb7\x74\x91\x59\x70\x70\x44\x88\xdf\x06\x3b\x82\xcc\x21\x3d";
/// WTV.pm:180, the GUID every `WTV::Metadata` record starts with.
const METADATA_RECORD_GUID: &[u8; 16] =
    b"\x5a\xfe\xd7\x6d\xc8\x1d\x8f\x4a\x99\x22\xfa\xb1\x1c\x38\x14\x53";
/// The only `WTV::Main` tag with a table entry (WTV.pm:40-43); every other
/// directory name is a "not decoded" comment there.
const METADATA_DIRECTORY: &str = "table.0.entries.legacy_attrib";

/// Days from 0001:01:01 to 1970:01:01 (WTV.pm:22-26, `719162*24*3600`).
const EPOCH_DAYS_0001_TO_1970: f64 = 719_162.0;

/// How a `WTV::Metadata` entry's decoded value is rendered.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Conv {
    /// No `ValueConv`/`PrintConv`: report the decoded value as-is.
    None,
    /// `%bool` (WTV.pm:30): `{ 0 => 'No', 1 => 'Yes' }`.
    Bool,
    /// `ValueConv => '$val/1e7'`, `PrintConv => 'ConvertDuration($val)'`
    /// (WTV.pm:56-59, :84-88).
    Duration,
    /// `%timeInfo` (WTV.pm:21-27): 100-ns intervals since 0001:01:01 UTC.
    Time,
    /// `ValueConv => '$val =~ tr/-T/: /; $val'` (WTV.pm:78-83, :103-108).
    XsdDateTime,
    /// `Unknown => 1` -- recognised so the record is walked, then dropped.
    Unknown,
}

/// `%Image::ExifTool::WTV::Metadata` (WTV.pm:50-147): the raw record name, the
/// reported tag name, and the conversion its entry declares.
///
/// A record whose name is absent here is still extracted -- "ExifTool will
/// extract any tag found, even if not in this table" (WTV.pm:53) -- under the
/// name `AddTagToTable` mints at WTV.pm:196-197.
#[rustfmt::skip]
const METADATA_TAGS: &[(&str, &str, Conv)] = &[
    ("Duration",                            "Duration",                       Conv::Duration),    // WTV.pm:56-59
    ("Title",                               "Title",                          Conv::None),        // WTV.pm:61
    ("WM/Genre",                            "Genre",                          Conv::None),        // WTV.pm:62
    ("WM/Language",                         "Language",                       Conv::None),        // WTV.pm:63
    ("WM/MediaClassPrimaryID",              "MediaClassPrimaryID",            Conv::None),        // WTV.pm:64
    ("WM/MediaClassSecondaryID",            "MediaClassSecondaryID",          Conv::None),        // WTV.pm:65
    ("WM/MediaCredits",                     "MediaCredits",                   Conv::None),        // WTV.pm:66
    ("WM/MediaIsDelay",                     "MediaIsDelay",                   Conv::Bool),        // WTV.pm:67
    ("WM/MediaIsFinale",                    "MediaIsFinale",                  Conv::Bool),        // WTV.pm:68
    ("WM/MediaIsLive",                      "MediaIsLive",                    Conv::Bool),        // WTV.pm:69
    ("WM/MediaIsMovie",                     "MediaIsMovie",                   Conv::Bool),        // WTV.pm:70
    ("WM/MediaIsPremiere",                  "MediaIsPremiere",                Conv::Bool),        // WTV.pm:71
    ("WM/MediaIsRepeat",                    "MediaIsRepeat",                  Conv::Bool),        // WTV.pm:72
    ("WM/MediaIsSAP",                       "MediaIsSAP",                     Conv::Bool),        // WTV.pm:73
    ("WM/MediaIsSport",                     "MediaIsSport",                   Conv::Bool),        // WTV.pm:74
    ("WM/MediaIsStereo",                    "MediaIsStereo",                  Conv::Bool),        // WTV.pm:75
    ("WM/MediaIsSubtitled",                 "MediaIsSubtitled",               Conv::Bool),        // WTV.pm:76
    ("WM/MediaIsTape",                      "MediaIsTape",                    Conv::Bool),        // WTV.pm:77
    ("WM/MediaNetworkAffiliation",          "MediaNetworkAffiliation",        Conv::None),        // WTV.pm:78
    ("WM/MediaOriginalBroadcastDateTime",   "MediaOriginalBroadcastDateTime", Conv::XsdDateTime), // WTV.pm:79-84
    ("WM/MediaOriginalChannel",             "MediaOriginalChannel",           Conv::None),        // WTV.pm:85
    ("WM/MediaOriginalChannelSubNumber",    "MediaOriginalChannelSubNumber",  Conv::None),        // WTV.pm:86
    ("WM/MediaOriginalRunTime",             "MediaOriginalRunTime",           Conv::Duration),    // WTV.pm:87-91
    ("WM/MediaStationCallSign",             "MediaStationCallSign",           Conv::None),        // WTV.pm:92
    ("WM/MediaStationName",                 "MediaStationName",               Conv::None),        // WTV.pm:93
    ("WM/MediaThumbAspectRatioX",           "MediaThumbAspectRatioX",         Conv::None),        // WTV.pm:94
    ("WM/MediaThumbAspectRatioY",           "MediaThumbAspectRatioY",         Conv::None),        // WTV.pm:95
    ("WM/MediaThumbHeight",                 "MediaThumbHeight",               Conv::None),        // WTV.pm:96
    ("WM/MediaThumbRatingAttributes",       "MediaThumbRatingAttributes",     Conv::None),        // WTV.pm:97
    ("WM/MediaThumbRatingLevel",            "MediaThumbRatingLevel",          Conv::None),        // WTV.pm:98
    ("WM/MediaThumbRatingSystem",           "MediaThumbRatingSystem",         Conv::None),        // WTV.pm:99
    ("WM/MediaThumbRet",                    "MediaThumbRet",                  Conv::None),        // WTV.pm:100
    ("WM/MediaThumbStride",                 "MediaThumbStride",               Conv::None),        // WTV.pm:101
    ("WM/MediaThumbTimeStamp",              "MediaThumbTimeStamp",            Conv::Unknown),     // WTV.pm:102
    ("WM/MediaThumbWidth",                  "MediaThumbWidth",                Conv::None),        // WTV.pm:103
    ("WM/OriginalReleaseTime",              "OriginalReleaseTime",            Conv::XsdDateTime), // WTV.pm:104-109
    ("WM/ParentalRating",                   "ParentalRating",                 Conv::None),        // WTV.pm:110
    ("WM/ParentalRatingReason",             "ParentalRatingReason",           Conv::None),        // WTV.pm:111
    ("WM/Provider",                         "Provider",                       Conv::None),        // WTV.pm:112
    ("WM/ProviderCopyright",                "ProviderCopyright",              Conv::None),        // WTV.pm:113
    ("WM/ProviderRating",                   "ProviderRating",                 Conv::None),        // WTV.pm:114
    ("WM/SubTitle",                         "Subtitle",                       Conv::None),        // WTV.pm:115
    ("WM/SubTitleDescription",              "SubtitleDescription",            Conv::None),        // WTV.pm:116
    ("WM/VideoClosedCaptioning",            "VideoClosedCaptioning",          Conv::Bool),        // WTV.pm:117
    ("WM/WMRVATSCContent",                  "ATSCContent",                    Conv::Bool),        // WTV.pm:118
    ("WM/WMRVActualSoftPostPadding",        "ActualSoftPostPadding",          Conv::None),        // WTV.pm:119
    ("WM/WMRVActualSoftPrePadding",         "ActualSoftPrePadding",           Conv::None),        // WTV.pm:120
    ("WM/WMRVBitrate",                      "Bitrate",                        Conv::Unknown),     // WTV.pm:121
    ("WM/WMRVBrandingImageID",              "BrandingImageID",                Conv::None),        // WTV.pm:122
    ("WM/WMRVBrandingName",                 "BrandingName",                   Conv::None),        // WTV.pm:123
    ("WM/WMRVContentProtected",             "ContentProtected",               Conv::Bool),        // WTV.pm:124
    ("WM/WMRVContentProtectedPercent",      "ContentProtectedPercent",        Conv::None),        // WTV.pm:125
    ("WM/WMRVDTVContent",                   "DTVContent",                     Conv::Bool),        // WTV.pm:126
    ("WM/WMRVEncodeTime",                   "EncodeTime",                     Conv::Time),        // WTV.pm:127
    ("WM/WMRVEndTime",                      "EndTime",                        Conv::Time),        // WTV.pm:128
    ("WM/WMRVExpirationDate",               "ExpirationDate",                 Conv::Unknown),     // WTV.pm:129
    ("WM/WMRVExpirationSpan",               "ExpirationSpan",                 Conv::Unknown),     // WTV.pm:130
    ("WM/WMRVHDContent",                    "HDContent",                      Conv::Bool),        // WTV.pm:131
    ("WM/WMRVHardPostPadding",              "HardPostPadding",                Conv::None),        // WTV.pm:132
    ("WM/WMRVHardPrePadding",               "HardPrePadding",                 Conv::None),        // WTV.pm:133
    ("WM/WMRVInBandRatingAttributes",       "InBandRatingAttributes",         Conv::None),        // WTV.pm:134
    ("WM/WMRVInBandRatingLevel",            "InBandRatingLevel",              Conv::None),        // WTV.pm:135
    ("WM/WMRVInBandRatingSystem",           "InBandRatingSystem",             Conv::None),        // WTV.pm:136
    ("WM/WMRVKeepUntil",                    "KeepUntil",                      Conv::None),        // WTV.pm:137
    ("WM/WMRVOriginalSoftPostPadding",      "OriginalSoftPostPadding",        Conv::None),        // WTV.pm:138
    ("WM/WMRVOriginalSoftPrePadding",       "OriginalSoftPrePadding",         Conv::None),        // WTV.pm:139
    ("WM/WMRVProgramID",                    "ProgramID",                      Conv::None),        // WTV.pm:140
    ("WM/WMRVQuality",                      "Quality",                        Conv::None),        // WTV.pm:141
    ("WM/WMRVRequestID",                    "RequestID",                      Conv::None),        // WTV.pm:142
    ("WM/WMRVScheduleItemID",               "ScheduleItemID",                 Conv::None),        // WTV.pm:143
    ("WM/WMRVSeriesUID",                    "SeriesUID",                      Conv::None),        // WTV.pm:144
    ("WM/WMRVServiceID",                    "ServiceID",                      Conv::None),        // WTV.pm:145
    ("WM/WMRVWatched",                      "Watched",                        Conv::Bool),        // WTV.pm:146
];

fn le_u32(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        data.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// `Image::ExifTool::Decode($str, 'UTF16')` for a little-endian buffer, which
/// is what `SetByteOrder('II')` (WTV.pm:219) selects.
///
/// The truncation is not a convenience: `Charset::Recompose`'s UTF-8 branch
/// ends with `$outVal =~ s/\0.*//s;   # truncate at null terminator`
/// (Charset.pm:326), and every `Decode(..., 'UTF16')` in this module lands
/// there because the default output charset is UTF8. It is load-bearing for
/// the directory walk: WTV pads each entry's name length up to a multiple of
/// four characters, so the sole directory ExifTool follows arrives as
/// `table.0.entries.legacy_attrib\0\0\0` and would match no table key without
/// it.
fn decode_utf16le(bytes: &[u8]) -> Option<String> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|unit| u16::from_le_bytes([unit[0], unit[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16(&units).ok()
}

/// `ReadSectors` (WTV.pm:152-166).
///
/// The Perl's assignment order is load-bearing and reproduced literally: the
/// data accumulator lags one sector behind the read, and the final sector is
/// appended by the `return` rather than by the loop. A `0xffff` entry (the
/// literal, not `0xffffffff`) aborts the whole read.
fn read_sectors(
    reader: &dyn FileReader,
    table: &[u8],
    mut pos: usize,
    sector_size: u32,
) -> Option<Vec<u8>> {
    let mut data: Option<Vec<u8>> = None;
    let mut buff: Option<Vec<u8>> = None;
    while table.len() >= 4 && pos <= table.len() - 4 {
        let sector = le_u32(table, pos)?;
        if sector == 0xffff {
            return None;
        }
        if sector == 0 {
            break;
        }
        match (&mut data, &buff) {
            (Some(data), Some(buff)) => data.extend_from_slice(buff),
            _ => data = buff.clone(),
        }
        let offset = u64::from(sector) * u64::from(sector_size);
        let read = reader.read(offset, sector_size as usize).ok()?;
        if read.len() != sector_size as usize {
            return None;
        }
        buff = Some(read.to_vec());
        pos += 4;
    }
    match (data, buff) {
        (Some(mut data), Some(buff)) => {
            data.extend_from_slice(&buff);
            Some(data)
        }
        (Some(data), None) => Some(data),
        (None, buff) => buff,
    }
}

/// One decoded `WTV::Metadata` value, before the table's own conversions.
enum Raw {
    /// `Get32s` under formats 0 (int32u) and 3 (boolean32) -- WTV.pm:200-201
    /// reads both as *signed*, which is why `KeepUntil` prints `-1`.
    Int32(i32),
    Int64(u64),
    Text(String),
    /// `unpack('H*', $dat)` over a 16-byte GUID (WTV.pm:204-205).
    Hex(String),
}

impl Raw {
    fn as_plain(&self) -> String {
        match self {
            Raw::Int32(value) => value.to_string(),
            Raw::Int64(value) => value.to_string(),
            Raw::Text(value) | Raw::Hex(value) => value.clone(),
        }
    }

    fn as_f64(&self) -> Option<f64> {
        match self {
            Raw::Int32(value) => Some(f64::from(*value)),
            Raw::Int64(value) => Some(*value as f64),
            _ => None,
        }
    }
}

/// WTV.pm:198-209's format word.
fn decode_value(format: u32, data: &[u8]) -> Option<Raw> {
    match format {
        // int32u or boolean32, both read with Get32s.
        0 | 3 => Some(Raw::Int32(i32::from_le_bytes(
            data.get(0..4)?.try_into().ok()?,
        ))),
        1 => decode_utf16le(data).map(Raw::Text),
        4 => Some(Raw::Int64(u64::from_le_bytes(
            data.get(0..8)?.try_into().ok()?,
        ))),
        6 => Some(Raw::Hex(
            data.iter().map(|byte| format!("{byte:02x}")).collect(),
        )),
        // WTV.pm:208-209 keeps the raw bytes under a synthetic
        // `Unknown(<fmt>)` format. There is no exact rendering for arbitrary
        // bytes here, so the record is walked past and nothing is reported.
        _ => None,
    }
}

/// `%timeInfo`'s ValueConv (WTV.pm:22-26) followed by its `ConvertDateTime`
/// PrintConv, which is the identity under the default `DateFormat`.
fn convert_wtv_time(hundred_nanos: f64) -> String {
    let seconds = hundred_nanos / 1e7 - EPOCH_DAYS_0001_TO_1970 * 24.0 * 3600.0;
    // `ConvertUnixTime` (ExifTool.pm:6784-6810) with no `$toLocal`: UTC, and
    // the epoch itself reported as an all-zero date. The trailing 'Z' is
    // appended by the WTV ValueConv itself, not by ConvertUnixTime.
    if seconds == 0.0 {
        return "0000:00:00 00:00:00Z".to_string();
    }
    chrono::DateTime::from_timestamp(round_to_second(seconds), 0)
        .map_or_else(String::new, |dt| format!("{}Z", format_exif_datetime(&dt)))
}

/// `ProcessMetadata` (WTV.pm:169-211).
fn process_metadata(data: &[u8], metadata: &mut MetadataMap) {
    let end = data.len();
    let mut pos = 0usize;
    while pos + 0x18 < end {
        if data.get(pos..pos + 16) != Some(METADATA_RECORD_GUID.as_slice()) {
            break;
        }
        let (Some(format), Some(len)) = (le_u32(data, pos + 0x10), le_u32(data, pos + 0x14)) else {
            break;
        };
        pos += 0x18;
        // The UTF-16 name, terminated by a NUL code unit (WTV.pm:186-192).
        let name_start = pos;
        loop {
            if pos + 2 > end {
                // `$et->Warn('Corrupt metadata directory'), last`.
                return;
            }
            let unit = &data[pos..pos + 2];
            pos += 2;
            if unit == b"\0\0" {
                break;
            }
        }
        let Some(name) = decode_utf16le(&data[name_start..pos - 2]) else {
            return;
        };
        let len = len as usize;
        if pos + len > end {
            break;
        }
        let payload = &data[pos..pos + len];
        pos += len;

        let entry = METADATA_TAGS.iter().find(|(record, _, _)| *record == name);
        let (tag, conv) = match entry {
            Some((_, tag, conv)) => ((*tag).to_string(), *conv),
            // WTV.pm:195-198: `$name =~ s{^(WTV_Metadata_)?WM/(WMRV)?}{}` --
            // an anchored strip of an optional `WTV_Metadata_`, a required
            // `WM/`, and an optional `WMRV`. A name without `WM/` is kept whole.
            None => (mint_tag_name(&name), Conv::None),
        };
        if conv == Conv::Unknown {
            continue;
        }
        let Some(raw) = decode_value(format, payload) else {
            continue;
        };
        let value = match conv {
            Conv::None | Conv::Unknown => raw.as_plain(),
            Conv::Bool => match raw.as_f64() {
                Some(value) if value == 0.0 => "No".to_string(),
                Some(value) if value == 1.0 => "Yes".to_string(),
                // A PrintConv with no matching key prints the raw value.
                _ => raw.as_plain(),
            },
            Conv::Duration => raw
                .as_f64()
                .map_or_else(|| raw.as_plain(), |value| convert_duration(value / 1e7)),
            Conv::Time => raw
                .as_f64()
                .map_or_else(|| raw.as_plain(), convert_wtv_time),
            // `tr/-T/: /`: every `-` becomes `:` and every `T` a space.
            Conv::XsdDateTime => raw
                .as_plain()
                .chars()
                .map(|c| match c {
                    '-' => ':',
                    'T' => ' ',
                    other => other,
                })
                .collect(),
        };
        metadata.insert(format!("WTV:{tag}"), TagValue::new_string(value));
    }
}

/// WTV.pm:195-198's `$name =~ s{^(WTV_Metadata_)?WM/(WMRV)?}{}`.
fn mint_tag_name(name: &str) -> String {
    let rest = name.strip_prefix("WTV_Metadata_").unwrap_or(name);
    match rest.strip_prefix("WM/") {
        Some(rest) => rest.strip_prefix("WMRV").unwrap_or(rest).to_string(),
        // The `WM/` in the pattern is not optional, so a name that lacks it
        // fails the whole substitution and keeps its original spelling --
        // including any `WTV_Metadata_` prefix.
        None => name.to_string(),
    }
}

/// Extract WTV metadata (`Image::ExifTool::WTV::ProcessWTV`).
pub fn parse_wtv_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    if reader.size() < FILE_HEADER_LEN as u64 {
        return Err("WTV file is too short for the 0x60-byte header".to_string());
    }
    let header = reader
        .read(0, FILE_HEADER_LEN)
        .map_err(|error| error.to_string())?;
    if !header.starts_with(FILE_GUID) {
        return Err("invalid WTV signature".to_string());
    }

    let sector_size = le_u32(header, SECTOR_SIZE_OFFSET).unwrap_or(STANDARD_SECTOR_SIZE);
    // WTV.pm:221-223: "in case I'm wrong about this, constrain sector size".
    let sector_size = if sector_size == STANDARD_SECTOR_SIZE || sector_size == TEST_SECTOR_SIZE {
        sector_size
    } else {
        STANDARD_SECTOR_SIZE
    };

    let Some(directory) = read_sectors(reader, header, DIRECTORY_SECTOR_TABLE, sector_size) else {
        return Err("could not read the WTV directory".to_string());
    };

    let mut metadata = MetadataMap::new();
    let mut pos = 0usize;
    while directory.len() >= 0x28 && pos < directory.len() - 0x28 {
        if directory.get(pos..pos + 0x10) != Some(DIRECTORY_ENTRY_GUID.as_slice()) {
            // `$et->Warn("WTV directory wasn't at expected location") unless $pos`
            break;
        }
        let Some(len) = le_u32(&directory, pos + 0x10).map(|len| len as usize) else {
            break;
        };
        if len == 0 || pos + len > directory.len() {
            break;
        }
        let Some(name_units) = le_u32(&directory, pos + 0x20).map(|n| n as usize) else {
            break;
        };
        if 0x28 + name_units * 2 + 8 > len {
            // `$et->Warn('WTV directory error'), last`
            break;
        }
        let name =
            decode_utf16le(&directory[pos + 0x28..pos + 0x28 + name_units * 2]).unwrap_or_default();
        let ptr = pos + 0x28 + name_units * 2;
        let flag = le_u32(&directory, ptr + 4).unwrap_or(u32::MAX);
        pos += len;

        // WTV.pm:255: `next unless $$tagTablePtr{$tag} and ($flg == 0 or $flg == 1)`.
        if name != METADATA_DIRECTORY || (flag != 0 && flag != 1) {
            continue;
        }
        let Some(sector) = directory.get(ptr..ptr + 4) else {
            continue;
        };
        let Some(mut data) = read_sectors(reader, sector, 0, sector_size) else {
            break;
        };
        // "read sectors from table if necessary (flag=1 indicates a sector
        // table)" (WTV.pm:259-260).
        if flag == 1 {
            let Some(indirect) = read_sectors(reader, &data, 0, sector_size) else {
                continue;
            };
            data = indirect;
        }
        process_metadata(&data, &mut metadata);
    }

    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minted_names_strip_only_the_documented_prefixes() {
        assert_eq!(mint_tag_name("WM/WMRVSomethingNew"), "SomethingNew");
        assert_eq!(mint_tag_name("WM/SomethingNew"), "SomethingNew");
        assert_eq!(
            mint_tag_name("WTV_Metadata_WM/WMRVSomethingNew"),
            "SomethingNew"
        );
        // No `WM/`, so the substitution does not match and the name stands.
        assert_eq!(mint_tag_name("Duration"), "Duration");
        assert_eq!(mint_tag_name("WTV_Metadata_Other"), "WTV_Metadata_Other");
    }

    #[test]
    fn timeinfo_matches_the_oracle_on_the_pinned_fixture() {
        // `t/images/WTV.wtv`'s EncodeTime, which the pinned ExifTool prints as
        // `2018:05:25 18:44:44Z`. 719162 days is 0001:01:01 -> 1970:01:01.
        let unix = 1_527_273_884.0_f64;
        let hundred_nanos = (unix + EPOCH_DAYS_0001_TO_1970 * 24.0 * 3600.0) * 1e7;
        assert_eq!(convert_wtv_time(hundred_nanos), "2018:05:25 18:44:44Z");
    }

    #[test]
    fn run_time_and_duration_use_convert_duration() {
        // MediaOriginalRunTime 1096 s -> `0:18:16`; Duration 27.38 s stays in
        // the sub-30-second branch.
        assert_eq!(convert_duration(1096.0), "0:18:16");
        assert_eq!(convert_duration(27.38), "27.38 s");
    }
}
