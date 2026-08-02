//! Windows Shortcut (LNK) format parser
//!
//! Transcribed from `Image::ExifTool::LNK` (LNK.pm 1.18). Every tag this module
//! emits carries ExifTool's own name under the `LNK` group, and every printed
//! form is the `PrintConv` written in LNK.pm -- not an approximation of it. A
//! record whose conversion cannot be reproduced exactly is left out rather than
//! reported under a real ExifTool name.
//!
//! # Format structure (LNK.pm:1721 `ProcessLNK`)
//!
//! - Shell Link Header: `HeaderSize` (>= 0x4c) bytes, walked as
//!   `%Image::ExifTool::LNK::Main`
//! - `LinkTargetIDList` (Flags bit 0): int16u size then the ItemID list,
//!   walked by `ProcessItemID` (LNK.pm:1449)
//! - `LinkInfo` (Flags bit 1): int32u size then the block, walked by
//!   `ProcessLinkInfo` (LNK.pm:1589)
//! - String data (Flags bits 2-6): int16u character count then the characters,
//!   two bytes each when the Unicode flag (bit 7) is set
//! - Extra data blocks: int32u size then an int32u block signature
//!
//! # References
//!
//! - `Image::ExifTool::LNK` -- the authority for every name and conversion here
//! - [MS-SHLLINK]: Shell Link (.LNK) Binary File Format

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use chrono::{Datelike, Offset, Timelike};

/// LNK signature: the little-endian int32u header size 0x0000004C.
const LNK_MAGIC: &[u8] = &[0x4C, 0x00, 0x00, 0x00];

/// Shell Link class identifier {00021401-0000-0000-C000-000000000046}, matched
/// bytewise the way LNK.pm:1729 matches it.
const SHELL_LINK_GUID: &[u8] = &[
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

/// Minimum Shell Link Header size (LNK.pm:1737 `$len >= 0x4c`).
const LNK_HEADER_SIZE: usize = 0x4c;

/// Number of 100-ns intervals between 1601-01-01 and the Unix epoch. LNK.pm:54
/// spells the same constant as `$val/1e7-11644473600`.
const FILETIME_EPOCH_100NS: i128 = 116_444_736_000_000_000;

/// `%Image::ExifTool::LNK::Main` 0x14 `Flags` BITMASK (LNK.pm:285).
const LINK_FLAGS: &[(u32, &str)] = &[
    (0, "IDList"),
    (1, "LinkInfo"),
    (2, "Description"),
    (3, "RelativePath"),
    (4, "WorkingDir"),
    (5, "CommandArgs"),
    (6, "IconFile"),
    (7, "Unicode"),
    (8, "NoLinkInfo"),
    (9, "ExpString"),
    (10, "SeparateProc"),
    (12, "DarwinID"),
    (13, "RunAsUser"),
    (14, "ExpIcon"),
    (15, "NoPidAlias"),
    (17, "RunWithShim"),
    (18, "NoLinkTrack"),
    (19, "TargetMetadata"),
    (20, "NoLinkPathTracking"),
    (21, "NoKnownFolderTracking"),
    (22, "NoKnownFolderAlias"),
    (23, "LinkToLink"),
    (24, "UnaliasOnSave"),
    (25, "PreferEnvPath"),
    (26, "KeepLocalIDList"),
];

/// `%fileAttributes` BITMASK (LNK.pm:30), shared by `FileAttributes` and
/// `TargetFileAttributes`.
const FILE_ATTRIBUTES: &[(u32, &str)] = &[
    (0, "Read-only"),
    (1, "Hidden"),
    (2, "System"),
    (3, "Volume"),
    (4, "Directory"),
    (5, "Archive"),
    (6, "Encrypted?"),
    (7, "Normal"),
    (8, "Temporary"),
    (9, "Sparse"),
    (10, "Reparse point"),
    (11, "Compressed"),
    (12, "Offline"),
    (13, "Not indexed"),
    (14, "Encrypted"),
];

/// `%Image::ExifTool::LNK::Main` 0x3c `RunWindow` PrintConv (LNK.pm:347).
const RUN_WINDOW: &[(u32, &str)] = &[
    (0, "Hide"),
    (1, "Normal"),
    (2, "Show Minimized"),
    (3, "Show Maximized"),
    (4, "Show No Activate"),
    (5, "Show"),
    (6, "Minimized"),
    (7, "Show Minimized No Activate"),
    (8, "Show NA"),
    (9, "Restore"),
    (10, "Show Default"),
];

/// The numeric keys of the 0x40 `HotKey` PrintConv (LNK.pm:365). Anything else
/// falls through to the `OTHER` handler in [`hot_key`].
const HOT_KEY_EXACT: &[(u32, &str)] = &[
    (0x00, "(none)"),
    (0x90, "Num Lock"),
    (0x91, "Scroll Lock"),
    (0x100, "Shift"),
    (0x200, "Control"),
    (0x400, "Alt"),
];

/// `%Image::ExifTool::LNK::Beef0004` 16 `OperatingSystem` PrintConv (LNK.pm:1027).
const OPERATING_SYSTEM: &[(u32, &str)] = &[
    (0x14, "Windows XP, 2003"),
    (0x26, "Windows Vista"),
    (0x2a, "Windows 2008, 7, 8"),
    (0x2e, "Windows 8.1, 10"),
];

/// `%Image::ExifTool::LNK::LinkInfo` `DriveType` PrintConv (LNK.pm:1149).
const DRIVE_TYPE: &[(u32, &str)] = &[
    (0, "Unknown"),
    (1, "Invalid Root Path"),
    (2, "Removable Media"),
    (3, "Fixed Disk"),
    (4, "Remote Drive"),
    (5, "CD-ROM"),
    (6, "Ram Disk"),
];

/// `%Image::ExifTool::LNK::LinkInfo` `NetProviderType` PrintConv
/// (LNK.pm:1171), transcribed from ExifTool's copy of `wnnc.h`.
const NET_PROVIDER_TYPE: &[(u32, &str)] = &[
    (0x010000, "MSNET"),
    (0x020000, "SMB"),
    (0x030000, "NETWARE"),
    (0x040000, "VINES"),
    (0x050000, "10NET"),
    (0x060000, "LOCUS"),
    (0x070000, "SUN_PC_NFS"),
    (0x080000, "LANSTEP"),
    (0x090000, "9TILES"),
    (0x0a0000, "LANTASTIC"),
    (0x0b0000, "AS400"),
    (0x0c0000, "FTP_NFS"),
    (0x0d0000, "PATHWORKS"),
    (0x0e0000, "LIFENET"),
    (0x0f0000, "POWERLAN"),
    (0x100000, "BWNFS"),
    (0x110000, "COGENT"),
    (0x120000, "FARALLON"),
    (0x130000, "APPLETALK"),
    (0x140000, "INTERGRAPH"),
    (0x150000, "SYMFONET"),
    (0x160000, "CLEARCASE"),
    (0x170000, "FRONTIER"),
    (0x180000, "BMC"),
    (0x190000, "DCE"),
    (0x1a0000, "AVID"),
    (0x1b0000, "DOCUSPACE"),
    (0x1c0000, "MANGOSOFT"),
    (0x1d0000, "SERNET"),
    (0x1e0000, "RIVERFRONT1"),
    (0x1f0000, "RIVERFRONT2"),
    (0x200000, "DECORB"),
    (0x210000, "PROTSTOR"),
    (0x220000, "FJ_REDIR"),
    (0x230000, "DISTINCT"),
    (0x240000, "TWINS"),
    (0x250000, "RDR2SAMPLE"),
    (0x260000, "CSC"),
    (0x270000, "3IN1"),
    (0x290000, "EXTENDNET"),
    (0x2a0000, "STAC"),
    (0x2b0000, "FOXBAT"),
    (0x2c0000, "YAHOO"),
    (0x2d0000, "EXIFS"),
    (0x2e0000, "DAV"),
    (0x2f0000, "KNOWARE"),
    (0x300000, "OBJECT_DIRE"),
    (0x310000, "MASFAX"),
    (0x320000, "HOB_NFS"),
    (0x330000, "SHIVA"),
    (0x340000, "IBMAL"),
    (0x350000, "LOCK"),
    (0x360000, "TERMSRV"),
    (0x370000, "SRT"),
    (0x380000, "QUINCY"),
    (0x390000, "OPENAFS"),
    (0x3a0000, "AVID1"),
    (0x3b0000, "DFS"),
    (0x3c0000, "KWNP"),
    (0x3d0000, "ZENWORKS"),
    (0x3e0000, "DRIVEONWEB"),
    (0x3f0000, "VMWARE"),
    (0x400000, "RSFX"),
    (0x410000, "MFILES"),
    (0x420000, "MS_NFS"),
    (0x430000, "GOOGLE"),
    (0x440000, "NDFS"),
    (0x450000, "DOCUSHARE"),
];

/// `%Image::ExifTool::LNK::ConsoleData` 0x24 `FontFamily` PrintConv (LNK.pm:1288).
const FONT_FAMILY: &[(u32, &str)] = &[
    (0, "Don't Care"),
    (0x1, "Roman"),
    (0x2, "Swiss"),
    (0x3, "Modern"),
    (0x4, "Script"),
    (0x5, "Decorative"),
];

/// Extra data block signatures walked by this module (LNK.pm:412).
const CONSOLE_DATA_SIG: u32 = 0xa000_0002;
const TRACKER_DATA_SIG: u32 = 0xa000_0003;

/// ItemID entry whose payload is `%Image::ExifTool::LNK::TargetInfo` (LNK.pm:514).
const ITEM_TARGET_INFO: u8 = 0x31;

/// `0xbeef0004` extension block ID (LNK.pm:571).
const BEEF_0004: u32 = 0xbeef_0004;

// ---------------------------------------------------------------------------
// little-endian accessors -- all bounds-checked so a truncated directory simply
// drops the tags that fall outside it, the way ProcessBinaryData does.
// ---------------------------------------------------------------------------

/// Borrows `len` bytes at `pos`, or `None` when that range is not fully inside
/// `data`. Offsets come straight off disk, so the addition is checked.
fn slice_at(data: &[u8], pos: usize, len: usize) -> Option<&[u8]> {
    data.get(pos..pos.checked_add(len)?)
}

fn u16le(data: &[u8], pos: usize) -> Option<u16> {
    let b = slice_at(data, pos, 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn u32le(data: &[u8], pos: usize) -> Option<u32> {
    let b = slice_at(data, pos, 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn u64le(data: &[u8], pos: usize) -> Option<u64> {
    let b = slice_at(data, pos, 8)?;
    Some(u64::from_le_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}

// ---------------------------------------------------------------------------
// ExifTool print conversions
// ---------------------------------------------------------------------------

/// `Image::ExifTool::DecodeBits` (ExifTool.pm:6385) over a 32-bit value: named
/// bits joined with ", ", unnamed set bits rendered as `[n]`, and `(none)` when
/// nothing is set.
fn decode_bits(val: u32, lookup: &[(u32, &str)]) -> String {
    let mut bits: Vec<String> = Vec::new();
    for bit in 0..32u32 {
        if val & (1u32 << bit) == 0 {
            continue;
        }
        match lookup.iter().find(|(b, _)| *b == bit) {
            Some((_, name)) => bits.push((*name).to_string()),
            None => bits.push(format!("[{bit}]")),
        }
    }
    if bits.is_empty() {
        return "(none)".to_string();
    }
    bits.join(", ")
}

/// Hash PrintConv lookup with ExifTool's miss text (ExifTool.pm:3628): with
/// `PrintHex` the raw value is shown as `Unknown (0x..)`, otherwise decimal.
fn print_conv(val: u32, table: &[(u32, &str)], print_hex: bool) -> String {
    match table.iter().find(|(k, _)| *k == val) {
        Some((_, s)) => (*s).to_string(),
        None if print_hex => format!("Unknown (0x{val:x})"),
        None => format!("Unknown ({val})"),
    }
}

/// 0x40 `HotKey` (LNK.pm:361).
///
/// The `OTHER` handler reduces to "character plus modifier prefixes": its guard
/// is written `chr $ch =~ /^[A-Z0-9]$/`, which Perl parses as
/// `chr($ch =~ /^[A-Z0-9]$/)` -- a one-character string that is always true --
/// so the first branch always wins and the F-key / Num Lock / Scroll Lock arms
/// below it never run. Verified against ExifTool 13.55: 0x70 prints `p`, not
/// `F1`, and 0x0b prints U+000B.
fn hot_key(val: u32) -> String {
    if let Some((_, s)) = HOT_KEY_EXACT.iter().find(|(k, _)| *k == val) {
        return (*s).to_string();
    }
    let mut out = char::from_u32(val & 0xff)
        .map(String::from)
        .unwrap_or_default();
    if val & 0x400 != 0 {
        out = format!("Alt-{out}");
    }
    if val & 0x200 != 0 {
        out = format!("Control-{out}");
    }
    if val & 0x100 != 0 {
        out = format!("Shift-{out}");
    }
    out
}

/// `Image::ExifTool::LNK::DOSTime` (LNK.pm:1419).
fn dos_time(val: u32) -> String {
    format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
        ((val >> 9) & 0x7f) + 1980,
        (val >> 5) & 0x0f,
        val & 0x1f,
        (val >> 27) & 0x1f,
        (val >> 21) & 0x3f,
        (val >> 15) & 0x3e
    )
}

/// `%fileTime` (LNK.pm:50): a FILETIME rendered as ExifTool's local date/time.
///
/// `RawConv => '$val ? $val : undef'` drops a zero FILETIME, then
/// `ConvertUnixTime($val, 1)` (ExifTool.pm:6784) formats local time with a
/// zone suffix. That conversion runs `sprintf('%.0f', $frac)` on the fractional
/// second and carries into the integer seconds only when the result is "1", so
/// a fraction above (not at) one half rounds up.
fn file_time(filetime: u64) -> Option<String> {
    if filetime == 0 {
        return None;
    }
    let ticks = filetime as i128 - FILETIME_EPOCH_100NS;
    let mut secs = ticks.div_euclid(10_000_000);
    if ticks.rem_euclid(10_000_000) > 5_000_000 {
        secs += 1;
    }
    let secs = i64::try_from(secs).ok()?;
    // ExifTool.pm:6787 short-circuits the epoch itself.
    if secs == 0 {
        return Some("0000:00:00 00:00:00".to_string());
    }
    let utc = chrono::DateTime::from_timestamp(secs, 0)?;
    let local = utc.with_timezone(&chrono::Local);
    // Built field by field rather than with chrono's `%Y`/`%:z`: ExifTool's
    // `sprintf("%4d:...")` never prefixes a five-digit year with '+', and
    // TimeZoneString (ExifTool.pm:6764) rounds the offset to whole minutes.
    let offset_secs = local.offset().fix().local_minus_utc();
    let sign = if offset_secs < 0 { '-' } else { '+' };
    let offset_min = (offset_secs.unsigned_abs() + 30) / 60;
    Some(format!(
        "{:04}:{:02}:{:02} {:02}:{:02}:{:02}{}{:02}:{:02}",
        local.year(),
        local.month(),
        local.day(),
        local.hour(),
        local.minute(),
        local.second(),
        sign,
        offset_min / 60,
        offset_min % 60,
    ))
}

/// `$et->Decode($val, 'UTF16')` over little-endian UTF-16, ignoring a trailing
/// odd byte.
///
/// ExifTool's decoder stops at the first null character, so a padded field
/// reports only the text before the padding. Verified against ExifTool 13.55:
/// a Description of `A A NUL B B` reports `AA`, not `AA\0BB`.
fn decode_utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let decoded = String::from_utf16_lossy(&units);
    match decoded.find('\0') {
        Some(end) => decoded[..end].to_string(),
        None => decoded,
    }
}

/// `Image::ExifTool::LNK::GetString` (LNK.pm:1436) for single-byte strings:
/// bytes up to the null terminator, or the remainder when there is none.
/// `None` when the position is past the end of the buffer.
fn get_string(data: &[u8], pos: usize) -> Option<&[u8]> {
    let rest = data.get(pos..).filter(|_| pos < data.len())?;
    Some(match rest.iter().position(|&b| b == 0) {
        Some(end) => &rest[..end],
        None => rest,
    })
}

/// `GetString` with the Unicode flag set: scans for an aligned `\0\0` pair.
fn get_string_unicode(data: &[u8], pos: usize) -> Option<&[u8]> {
    let rest = data.get(pos..).filter(|_| pos < data.len())?;
    let mut i = 0;
    while i + 2 <= rest.len() {
        if rest[i] == 0 && rest[i + 1] == 0 {
            return Some(&rest[..i]);
        }
        i += 2;
    }
    Some(rest)
}

/// Renders raw single-byte string data. LNK.pm hands these through untouched,
/// so anything that is not valid UTF-8 is replaced rather than guessed at.
fn plain_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

// ---------------------------------------------------------------------------
// tag emission
// ---------------------------------------------------------------------------

/// Records a tag under ExifTool's `LNK` group.
fn put(metadata: &mut MetadataMap, name: &str, value: TagValue) {
    metadata.insert(format!("LNK:{name}"), value);
}

fn put_str(metadata: &mut MetadataMap, name: &str, value: impl Into<String>) {
    put(metadata, name, TagValue::String(value.into()));
}

fn put_int(metadata: &mut MetadataMap, name: &str, value: u32) {
    put(metadata, name, TagValue::Integer(i64::from(value)));
}

/// Windows Shortcut (LNK) parser for extracting metadata from .lnk files
pub struct LNKParser;

impl LNKParser {
    /// Verifies the LNK signature: the int32u header size 0x4c followed by the
    /// Shell Link class identifier (LNK.pm:1729).
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < LNK_HEADER_SIZE as u64 {
            return Ok(false);
        }
        if reader.read(0, 4)? != LNK_MAGIC {
            return Ok(false);
        }
        Ok(reader.read(4, 16)? == SHELL_LINK_GUID)
    }

    /// Walks `%Image::ExifTool::LNK::Main` over the Shell Link Header
    /// (LNK.pm:275).
    fn read_header(header: &[u8], metadata: &mut MetadataMap) {
        if let Some(flags) = u32le(header, 0x14) {
            put_str(metadata, "Flags", decode_bits(flags, LINK_FLAGS));
        }
        if let Some(attrs) = u32le(header, 0x18) {
            put_str(
                metadata,
                "FileAttributes",
                decode_bits(attrs, FILE_ATTRIBUTES),
            );
        }
        for (offset, name) in [
            (0x1c, "CreateDate"),
            (0x24, "AccessDate"),
            (0x2c, "ModifyDate"),
        ] {
            if let Some(raw) = u64le(header, offset)
                && let Some(formatted) = file_time(raw)
            {
                put_str(metadata, name, formatted);
            }
        }
        if let Some(size) = u32le(header, 0x34) {
            put_int(metadata, "TargetFileSize", size);
        }
        if let Some(index) = u32le(header, 0x38) {
            // LNK.pm:341 `PrintConv => '$val ? $val : "(none)"'`
            if index == 0 {
                put_str(metadata, "IconIndex", "(none)");
            } else {
                put_int(metadata, "IconIndex", index);
            }
        }
        if let Some(run) = u32le(header, 0x3c) {
            put_str(metadata, "RunWindow", print_conv(run, RUN_WINDOW, false));
        }
        if let Some(key) = u32le(header, 0x40) {
            put_str(metadata, "HotKey", hot_key(key));
        }
    }

    /// `ProcessItemID` (LNK.pm:1449): walks the ItemID list, splitting each
    /// entry at its `0xbeef` extension block when one is present.
    fn read_item_id(data: &[u8], metadata: &mut MetadataMap) {
        let len = data.len();
        let mut pos = 0usize;
        while pos + 2 <= len {
            let Some(entry_size) = u16le(data, pos) else {
                break;
            };
            if entry_size == 0 || entry_size < 4 {
                break;
            }
            let mut size = entry_size as usize;
            if pos + size > len {
                size = len - pos;
            }
            let Some(tag) = data.get(pos + 2).copied() else {
                break;
            };
            let entry = &data[pos..pos + size];
            let beef_start = find_beef_offset(entry).map(|off| pos + off);
            let payload_len = beef_start.map_or(size, |start| start - pos);
            Self::read_item(tag, &data[pos..pos + payload_len], metadata);

            // Extension blocks that follow the item payload (LNK.pm:1523).
            let end = pos + size;
            let mut cursor = beef_start;
            while let Some(start) = cursor {
                if start + 8 > end {
                    break;
                }
                let Some(block_len) = u16le(data, start) else {
                    break;
                };
                let Some(beef_id) = u32le(data, start + 4) else {
                    break;
                };
                if beef_id & 0xffff_0000 != 0xbeef_0000 {
                    break;
                }
                let block_len = block_len as usize;
                if block_len == 0 || start + block_len > end {
                    break;
                }
                if beef_id == BEEF_0004 {
                    Self::read_beef_0004(&data[start..start + block_len], metadata);
                }
                // LNK.pm:1541 assumes extensions start on 2-byte boundaries.
                let step = block_len + (block_len & 1);
                cursor = Some(start + step);
            }
            pos += size;
        }
    }

    /// Dispatches one ItemID entry. LNK.pm:1478 folds the 0x20-0x2f, 0x30-0x3f
    /// and 0x40-0x4f ID ranges onto a single table each; only the
    /// `TargetInfo` range is decoded here.
    fn read_item(tag: u8, entry: &[u8], metadata: &mut MetadataMap) {
        let target_info = tag == ITEM_TARGET_INFO
            || item_id_alias(tag & 0x71) == Some(ITEM_TARGET_INFO)
            || item_id_alias(tag & 0x70) == Some(ITEM_TARGET_INFO);
        if target_info {
            Self::read_target_info(entry, metadata);
        }
    }

    /// `%Image::ExifTool::LNK::TargetInfo` (LNK.pm:627).
    fn read_target_info(entry: &[u8], metadata: &mut MetadataMap) {
        if let Some(raw) = u32le(entry, 8)
            && raw != 0
        {
            // LNK.pm:636 `RawConv => '$val || undef'` ignores zero dates.
            put_str(metadata, "TargetFileModifyDate", dos_time(raw));
        }
        if let Some(&attrs) = entry.get(12) {
            put_str(
                metadata,
                "TargetFileAttributes",
                decode_bits(u32::from(attrs), FILE_ATTRIBUTES),
            );
        }
        // `Format => 'undef[$size-14]'` (LNK.pm:647). ProcessBinaryData skips a
        // zero-length read, so a 14-byte entry reports no name at all -- as
        // opposed to a 1-byte field holding a null, which reports "".
        if let Some(name) = entry.get(14..).filter(|n| !n.is_empty()) {
            // LNK.pm:648 allows for the possibility of Unicode here.
            let is_unicode = name.len() >= 3
                && (0x20..=0x7f).contains(&name[0])
                && name[1] == 0
                && (0x20..=0x7f).contains(&name[2]);
            let value = if is_unicode {
                decode_utf16le(get_string_unicode(name, 0).unwrap_or(name))
            } else {
                plain_string(get_string(name, 0).unwrap_or(name))
            };
            put_str(metadata, "TargetFileDOSName", value);
        }
    }

    /// `%Image::ExifTool::LNK::Beef0004` (LNK.pm:995) -- the TargetInfo
    /// extension block.
    fn read_beef_0004(block: &[u8], metadata: &mut MetadataMap) {
        let version = u16le(block, 2).unwrap_or(0);
        if let Some(raw) = u32le(block, 8) {
            put_str(metadata, "TargetFileCreateDate", dos_time(raw));
        }
        if let Some(raw) = u32le(block, 12) {
            put_str(metadata, "TargetFileAccessDate", dos_time(raw));
        }
        let Some(os) = u16le(block, 16) else {
            return;
        };
        put_str(
            metadata,
            "OperatingSystem",
            print_conv(u32::from(os), OPERATING_SYSTEM, true),
        );

        // LNK.pm:1033 Hook -- the offset of TargetFileName depends on the
        // extension version.
        let mut var_size = 0usize;
        if version >= 7 {
            var_size += 18;
        }
        if version >= 3 {
            var_size += 2;
        }
        if version >= 9 {
            var_size += 4;
        }
        if version >= 8 {
            var_size += 4;
        }

        // LNK.pm:1042 `Format => 'undef[$size - 20 - $varSize]'` drops the
        // trailing offset word.
        let start = 18 + var_size;
        let Some(name_len) = block.len().checked_sub(20 + var_size) else {
            return;
        };
        let Some(raw) = slice_at(block, start, name_len) else {
            return;
        };
        // LNK.pm:1044 splits the field into null-terminated Unicode strings.
        let mut strings: Vec<String> = Vec::new();
        let mut pos = 0usize;
        while pos + 2 <= raw.len() {
            let Some(bytes) = get_string_unicode(raw, pos) else {
                break;
            };
            let consumed = bytes.len() + 2;
            if pos + consumed > raw.len() {
                break;
            }
            strings.push(decode_utf16le(bytes));
            pos += consumed;
        }
        match strings.len() {
            0 => {}
            1 => put_str(metadata, "TargetFileName", strings.remove(0)),
            _ => put(
                metadata,
                "TargetFileName",
                TagValue::Array(strings.into_iter().map(TagValue::String).collect()),
            ),
        }
    }

    /// `ProcessLinkInfo` (LNK.pm:1589) over the whole LinkInfo block, offsets
    /// included, starting at its own size word.
    fn read_link_info(data: &[u8], metadata: &mut MetadataMap) {
        let data_len = data.len();
        if data_len < 0x24 {
            return;
        }
        let (Some(header_len), Some(flags)) = (u32le(data, 4), u32le(data, 8)) else {
            return;
        };
        let header_len = header_len as usize;

        if flags & 0x01 != 0 {
            // Volume ID
            if let Some(off) = u32le(data, 0x0c).map(|v| v as usize)
                && off != 0
                && off.saturating_add(0x20) <= data_len
            {
                if let Some(drive) = u32le(data, off + 4) {
                    put_str(metadata, "DriveType", print_conv(drive, DRIVE_TYPE, false));
                }
                if let Some(serial) = u32le(data, off + 8) {
                    // LNK.pm:1160 `join("-", unpack("A4 A4", sprintf("%08X", $val)))`
                    let hex = format!("{serial:08X}");
                    put_str(
                        metadata,
                        "DriveSerialNumber",
                        format!("{}-{}", &hex[..4], &hex[4..]),
                    );
                }
                if let Some(mut pos) = u32le(data, off + 0x0c).map(|v| v as usize) {
                    let unicode = pos == 0x14;
                    if unicode {
                        pos = u32le(data, off + 0x10).map(|v| v as usize).unwrap_or(pos);
                    }
                    pos = pos.saturating_add(off);
                    let label = if unicode {
                        get_string_unicode(data, pos).map(decode_utf16le)
                    } else {
                        get_string(data, pos).map(plain_string)
                    };
                    if let Some(label) = label {
                        put_str(metadata, "VolumeLabel", label);
                    }
                }
            }
            // Local base path
            let (pos, unicode) = if header_len >= 0x24 {
                (u32le(data, 0x1c), true)
            } else {
                (u32le(data, 0x10), false)
            };
            if let Some(pos) = pos.map(|v| v as usize) {
                let path = if unicode {
                    get_string_unicode(data, pos).map(decode_utf16le)
                } else {
                    get_string(data, pos).map(plain_string)
                };
                if let Some(path) = path {
                    put_str(metadata, "LocalBasePath", path);
                }
            }
        }

        if flags & 0x02 != 0 {
            // Common network relative link
            if let Some(off) = u32le(data, 0x14).map(|v| v as usize)
                && off != 0
                && off.saturating_add(0x14) <= data_len
                && let Some(link_size) = u32le(data, off).map(|v| v as usize)
                && off.saturating_add(link_size) <= data_len
            {
                if let Some(mut pos) = u32le(data, off + 0x08).map(|v| v as usize) {
                    let unicode = pos > 0x14 && link_size >= 0x18;
                    if unicode {
                        pos = u32le(data, off + 0x14).map(|v| v as usize).unwrap_or(pos);
                    }
                    let pos = pos.saturating_add(off);
                    let name = if unicode {
                        get_string_unicode(data, pos).map(decode_utf16le)
                    } else {
                        get_string(data, pos).map(plain_string)
                    };
                    if let Some(name) = name {
                        put_str(metadata, "NetName", name);
                    }
                }
                if let Some(link_flags) = u32le(data, off + 0x04)
                    && link_flags & 0x01 != 0
                    && let Some(mut pos) = u32le(data, off + 0x0c).map(|v| v as usize)
                {
                    let unicode = pos > 0x14 && link_size >= 0x1c;
                    if unicode {
                        pos = u32le(data, off + 0x18).map(|v| v as usize).unwrap_or(pos);
                    }
                    let pos = pos.saturating_add(off);
                    let name = if unicode {
                        get_string_unicode(data, pos).map(decode_utf16le)
                    } else {
                        get_string(data, pos).map(plain_string)
                    };
                    if let Some(name) = name {
                        put_str(metadata, "DeviceName", name);
                    }
                }
                if let Some(link_flags) = u32le(data, off + 0x04)
                    && link_flags & 0x02 != 0
                    && let Some(provider) = u32le(data, off + 0x10)
                {
                    put_str(
                        metadata,
                        "NetProviderType",
                        print_conv(provider, NET_PROVIDER_TYPE, true),
                    );
                }
            }
        }

        if let Some(off) = u32le(data, 0x18).map(|v| v as usize)
            && off != 0
            && off < data_len
            && let Some(suffix) = get_string(data, off)
        {
            put_str(metadata, "CommonPathSuffix", plain_string(suffix));
        }
        if header_len >= 0x24
            && let Some(off) = u32le(data, 0x20).map(|v| v as usize)
            && off != 0
            && off < data_len
            && let Some(suffix) = get_string_unicode(data, off)
        {
            put_str(metadata, "CommonPathSuffixUnicode", decode_utf16le(suffix));
        }
    }

    /// `%Image::ExifTool::LNK::ConsoleData` (LNK.pm:1250), keyed from the start
    /// of the extra data block.
    fn read_console_data(block: &[u8], metadata: &mut MetadataMap) {
        for (offset, name) in [(0x08, "FillAttributes"), (0x0a, "PopupFillAttributes")] {
            if let Some(v) = u16le(block, offset) {
                // LNK.pm:1256 `sprintf("0x%.2x", $val)`
                put_str(metadata, name, format!("0x{v:02x}"));
            }
        }
        for (offset, name) in [
            (0x0c, "ScreenBufferSize"),
            (0x10, "WindowSize"),
            (0x14, "WindowOrigin"),
            (0x20, "FontSize"),
        ] {
            if let (Some(a), Some(b)) = (u16le(block, offset), u16le(block, offset + 2)) {
                // LNK.pm:1266 `PrintConv => '$val=~s/ / x /; $val'`
                put_str(metadata, name, format!("{a} x {b}"));
            }
        }
        if let Some(v) = u32le(block, 0x24) {
            // LNK.pm:1287 `Mask => 0xf0` selects the family nibble.
            put_str(
                metadata,
                "FontFamily",
                print_conv((v & 0xf0) >> 4, FONT_FAMILY, true),
            );
        }
        if let Some(v) = u32le(block, 0x28) {
            put_int(metadata, "FontWeight", v);
        }
        if let Some(raw) = slice_at(block, 0x2c, 64) {
            // LNK.pm:1304 decodes UTF16 then truncates at the first null.
            let decoded = decode_utf16le(raw);
            let name = decoded.split('\0').next().unwrap_or("");
            if !name.is_empty() {
                put_str(metadata, "FontName", name);
            }
        }
        if let Some(v) = u32le(block, 0x6c) {
            put_int(metadata, "CursorSize", v);
        }
        for (offset, name) in [
            (0x70, "FullScreen"),
            (0x74, "QuickEdit"),
            (0x78, "InsertMode"),
            (0x7c, "WindowOriginAuto"),
        ] {
            if let Some(v) = u32le(block, offset) {
                put_str(metadata, name, if v != 0 { "Yes" } else { "No" });
            }
        }
        if let Some(v) = u32le(block, 0x80) {
            put_int(metadata, "HistoryBufferSize", v);
        }
        if let Some(v) = u32le(block, 0x84) {
            put_int(metadata, "NumHistoryBuffers", v);
        }
        if let Some(v) = u32le(block, 0x88) {
            put_str(
                metadata,
                "RemoveHistoryDuplicates",
                if v != 0 { "Yes" } else { "No" },
            );
        }
    }

    /// `%Image::ExifTool::LNK::TrackerData` (LNK.pm:1349).
    fn read_tracker_data(block: &[u8], metadata: &mut MetadataMap) {
        if let Some(id) = get_string(block, 0x10) {
            put_str(metadata, "MachineID", plain_string(id));
        }
    }

    /// The extra data block loop of LNK.pm:1821.
    fn read_extra_data(data: &[u8], mut pos: usize, metadata: &mut MetadataMap) {
        while let Some(size) = u32le(data, pos) {
            let size = size as usize;
            if size < 4 {
                break;
            }
            let Some(block) = slice_at(data, pos, size) else {
                break;
            };
            // LNK.pm:1826 skips blocks with no payload beyond the signature.
            if size - 4 > 4 {
                match u32le(block, 4) {
                    Some(CONSOLE_DATA_SIG) => Self::read_console_data(block, metadata),
                    Some(TRACKER_DATA_SIG) => Self::read_tracker_data(block, metadata),
                    _ => {}
                }
            }
            pos += size;
        }
    }
}

/// LNK.pm:1479 `%lkup` -- the ItemID range folding table.
fn item_id_alias(masked: u8) -> Option<u8> {
    match masked {
        0x20 => Some(0x2e),
        0x21 => Some(0x2f),
        0x30 => Some(0x31),
        0x40 => Some(0x40),
        _ => None,
    }
}

/// LNK.pm:1504 -- locates the `0xbeef` extension inside one ItemID entry. The
/// candidate offset is only accepted when it matches the back-pointer stored in
/// the entry's last two bytes.
fn find_beef_offset(entry: &[u8]) -> Option<usize> {
    let back_pointer = u16le(entry, entry.len().checked_sub(2)?)? as usize;
    let mut i = 0usize;
    while i + 8 <= entry.len() {
        if entry[i + 5] == 0 && entry[i + 6] == 0xef && entry[i + 7] == 0xbe {
            return (i == back_pointer).then_some(i);
        }
        i += 1;
    }
    None
}

impl FormatParser for LNKParser {
    /// Parses metadata from a Windows shortcut (LNK) file.
    ///
    /// Follows `ProcessLNK` (LNK.pm:1721): header, ItemID list, LinkInfo,
    /// string data, extra data blocks.
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid LNK signature"));
        }

        let file_size = usize::try_from(reader.size())
            .map_err(|_| ExifToolError::parse_error("LNK file too large"))?;
        let data = reader.read(0, file_size)?;

        let header_len = u32le(data, 0).unwrap_or(0) as usize;
        if header_len < LNK_HEADER_SIZE {
            return Err(ExifToolError::parse_error("Invalid LNK header size"));
        }
        let header = data
            .get(..header_len)
            .ok_or_else(|| ExifToolError::parse_error("Truncated LNK header"))?;

        let mut metadata = MetadataMap::new();
        Self::read_header(header, &mut metadata);

        let flags = u32le(header, 0x14).unwrap_or(0);
        let is_unicode = flags & 0x80 != 0;
        let mut pos = header_len;

        // Link target ID list (LNK.pm:1760)
        if flags & 0x01 != 0 {
            let Some(len) = u16le(data, pos).map(|v| v as usize) else {
                return Ok(metadata);
            };
            pos += 2;
            let end = pos.saturating_add(len).min(data.len());
            Self::read_item_id(&data[pos.min(end)..end], &mut metadata);
            pos = end;
        }

        // Link information (LNK.pm:1772)
        if flags & 0x02 != 0 {
            let Some(len) = u32le(data, pos).map(|v| v as usize) else {
                return Ok(metadata);
            };
            if len < 4 {
                return Ok(metadata);
            }
            let end = pos.saturating_add(len).min(data.len());
            Self::read_link_info(&data[pos.min(end)..end], &mut metadata);
            pos = end;
        }

        // String data (LNK.pm:1786)
        const STRINGS: [&str; 5] = [
            "Description",
            "RelativePath",
            "WorkingDirectory",
            "CommandLineArguments",
            "IconFileName",
        ];
        for (i, name) in STRINGS.iter().enumerate() {
            if flags & (0x04 << i) == 0 {
                continue;
            }
            let Some(chars) = u16le(data, pos).map(|v| v as usize) else {
                return Ok(metadata);
            };
            pos += 2;
            if chars == 0 {
                continue;
            }
            // Windows limits most of these strings to 259 characters despite
            // its own specification (LNK.pm:1797).
            let mut chars = chars;
            let limited = i != 3 && chars >= 260;
            if limited && chars > 260 {
                chars = 260;
            }
            let len = if is_unicode { chars * 2 } else { chars };
            // LNK.pm:1806 tests `$raf->Read(...)` for truth, not for the full
            // count: a short read still yields whatever was available and the
            // walk continues. Only a read of nothing ends it.
            let available = data.len().saturating_sub(pos);
            if available == 0 {
                return Ok(metadata);
            }
            let read = len.min(available);
            // The length limit drops the last character, which Perl's substr
            // silently ignores when the buffer is already shorter.
            let keep = if limited {
                len.saturating_sub(if is_unicode { 2 } else { 1 })
            } else {
                len
            };
            let raw = &data[pos..pos + read.min(keep)];
            let value = if is_unicode {
                decode_utf16le(raw)
            } else {
                plain_string(raw)
            };
            put_str(&mut metadata, name, value);
            pos += read;
        }

        // Extra data blocks (LNK.pm:1821)
        Self::read_extra_data(data, pos, &mut metadata);

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::LNK)
    }
}

/// Parses metadata from Windows shortcut (LNK) files.
///
/// This is the public API function for parsing LNK files.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the LNK file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
///
/// # Examples
///
/// ```no_run
/// use oxidex::parsers::specialized::lnk::parse_lnk_metadata;
/// use oxidex::io::MMapReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), String> {
/// let reader = MMapReader::new(Path::new("shortcut.lnk"))
///     .map_err(|e| e.to_string())?;
/// let metadata = parse_lnk_metadata(&reader)?;
/// println!("LNK metadata: {:?}", metadata);
/// # Ok(())
/// # }
/// ```
pub fn parse_lnk_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = LNKParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// Builds a minimal Shell Link Header with the given flags.
    fn header(flags: u32) -> Vec<u8> {
        let mut data = vec![0u8; LNK_HEADER_SIZE];
        data[0..4].copy_from_slice(&(LNK_HEADER_SIZE as u32).to_le_bytes());
        data[4..20].copy_from_slice(SHELL_LINK_GUID);
        data[20..24].copy_from_slice(&flags.to_le_bytes());
        data
    }

    #[test]
    fn test_verify_signature_valid() {
        let reader = TestReader::new(header(0));
        assert!(LNKParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_invalid_magic() {
        let mut data = header(0);
        data[0..4].copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        let reader = TestReader::new(data);
        assert!(!LNKParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_too_small() {
        let reader = TestReader::new(vec![0x4C, 0x00, 0x00, 0x00]);
        assert!(!LNKParser::verify_signature(&reader).unwrap());
    }

    /// Every emitted tag carries ExifTool's `LNK` group prefix.
    #[test]
    fn test_tags_are_group_prefixed() {
        let mut data = header(0);
        data[24..28].copy_from_slice(&0x80u32.to_le_bytes());
        let reader = TestReader::new(data);
        let metadata = LNKParser.parse(&reader).unwrap();

        assert!(metadata.iter().count() > 0);
        for (key, _) in metadata.iter() {
            assert!(key.starts_with("LNK:"), "ungrouped tag {key}");
        }
    }

    /// `%Image::ExifTool::LNK::Main` 0x14/0x18 BITMASK conversions.
    #[test]
    fn test_header_bitmask_conversions() {
        let mut data = header(0xbf);
        data[24..28].copy_from_slice(&0x80u32.to_le_bytes());
        let reader = TestReader::new(data);
        let metadata = LNKParser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("LNK:Flags"),
            Some(&TagValue::String(
                "IDList, LinkInfo, Description, RelativePath, WorkingDir, CommandArgs, Unicode"
                    .to_string()
            ))
        );
        assert_eq!(
            metadata.get("LNK:FileAttributes"),
            Some(&TagValue::String("Normal".to_string()))
        );
    }

    /// `(none)` placeholders and the RunWindow PrintConv.
    #[test]
    fn test_header_scalar_conversions() {
        let mut data = header(0);
        data[0x34..0x38].copy_from_slice(&3_988_946u32.to_le_bytes());
        data[0x3c..0x40].copy_from_slice(&1u32.to_le_bytes());
        let reader = TestReader::new(data);
        let metadata = LNKParser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("LNK:TargetFileSize"),
            Some(&TagValue::Integer(3_988_946))
        );
        assert_eq!(
            metadata.get("LNK:IconIndex"),
            Some(&TagValue::String("(none)".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:RunWindow"),
            Some(&TagValue::String("Normal".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:HotKey"),
            Some(&TagValue::String("(none)".to_string()))
        );
    }

    /// A 14-byte TargetInfo entry leaves `undef[$size-14]` zero-length, which
    /// ProcessBinaryData skips; one more byte reports an empty name. Verified
    /// against ExifTool 13.55.
    #[test]
    fn test_target_file_dos_name_zero_length() {
        let mut entry = vec![0u8; 14];
        entry[0] = 14;
        entry[2] = ITEM_TARGET_INFO;
        let mut metadata = MetadataMap::new();
        LNKParser::read_target_info(&entry, &mut metadata);
        assert!(metadata.get("LNK:TargetFileDOSName").is_none());

        entry.push(0);
        let mut metadata = MetadataMap::new();
        LNKParser::read_target_info(&entry, &mut metadata);
        assert_eq!(
            metadata.get("LNK:TargetFileDOSName"),
            Some(&TagValue::String(String::new()))
        );
    }

    /// `NetProviderType` is a `PrintHex` hash, so a value outside ExifTool's
    /// copy of `wnnc.h` renders as `Unknown (0x..)` (ExifTool.pm:3631).
    #[test]
    fn test_net_provider_type() {
        assert_eq!(NET_PROVIDER_TYPE.len(), 68);
        assert_eq!(print_conv(0x020000, NET_PROVIDER_TYPE, true), "SMB");
        assert_eq!(print_conv(0x430000, NET_PROVIDER_TYPE, true), "GOOGLE");
        // 0x280000 is the one gap in ExifTool's otherwise contiguous run.
        assert_eq!(
            print_conv(0x280000, NET_PROVIDER_TYPE, true),
            "Unknown (0x280000)"
        );
    }

    #[test]
    fn test_decode_bits_matches_exiftool() {
        assert_eq!(decode_bits(0, LINK_FLAGS), "(none)");
        assert_eq!(decode_bits(0x03, LINK_FLAGS), "IDList, LinkInfo");
        // Bit 11 has no name in %Main 0x14.
        assert_eq!(decode_bits(1 << 11, LINK_FLAGS), "[11]");
    }

    /// `Image::ExifTool::LNK::DOSTime` (LNK.pm:1419) on the values ExifTool
    /// reports for the reference sample.
    #[test]
    fn test_dos_time() {
        assert_eq!(dos_time(1_421_884_203), "2009:09:11 10:38:00");
        assert_eq!(dos_time(1_381_579_557), "2009:09:05 10:18:50");
        assert_eq!(dos_time(402_996_020), "2009:09:20 03:00:10");
    }

    /// A zero FILETIME is dropped by `RawConv => '$val ? $val : undef'`.
    #[test]
    fn test_file_time_zero_is_omitted() {
        assert_eq!(file_time(0), None);
    }

    /// `ConvertUnixTime` carries the fractional second into the integer part
    /// only when it rounds to 1.
    #[test]
    fn test_file_time_rounds_to_nearest_second() {
        // 128978881758399056 is 0.8399056 s past a second boundary.
        let rounded = file_time(128_978_881_758_399_056).unwrap();
        let truncated = file_time(128_978_881_750_000_000).unwrap();
        assert_ne!(rounded, truncated, "fractional second must round up");
        // The rounded form is exactly one second later than the truncated one.
        assert_eq!(rounded, file_time(128_978_881_760_000_000).unwrap());
    }

    /// `Decode(..., 'UTF16')` stops at the first null. Verified against
    /// ExifTool 13.55: a Description of `A A NUL B B` reports `AA`.
    #[test]
    fn test_decode_utf16le_stops_at_null() {
        let units =
            |s: &[u16]| -> Vec<u8> { s.iter().flat_map(|u| u.to_le_bytes()).collect::<Vec<u8>>() };
        assert_eq!(decode_utf16le(&units(&[0x41, 0x41, 0x42])), "AAB");
        assert_eq!(decode_utf16le(&units(&[0x41, 0x41, 0, 0x42, 0x42])), "AA");
        assert_eq!(decode_utf16le(&units(&[0x41, 0, 0, 0x42])), "A");
        assert_eq!(decode_utf16le(&units(&[0x41, 0x42, 0])), "AB");
        // A trailing odd byte is ignored rather than mangling the last unit.
        assert_eq!(decode_utf16le(&[0x41, 0x00, 0x42]), "A");
    }

    /// ExifTool's `sprintf("%4d:...")` never prefixes a five-digit year with
    /// '+', unlike chrono's `%Y`.
    #[test]
    fn test_file_time_year_has_no_sign() {
        // A FILETIME far past the year 9999.
        let value = file_time(9_000_000_000_000_000_000).expect("representable");
        assert!(
            !value.starts_with('+'),
            "year must not carry a sign: {value}"
        );
        assert!(value.len() > 25, "expected a five-digit year: {value}");
        // The zone suffix is still present and shaped like ExifTool's.
        let zone = &value[value.len() - 6..];
        assert!(zone.starts_with('+') || zone.starts_with('-'), "{value}");
        assert_eq!(&zone[3..4], ":", "{value}");
    }

    /// The `OTHER` handler of the HotKey PrintConv (LNK.pm:366), verified
    /// against ExifTool 13.55.
    #[test]
    fn test_hot_key() {
        assert_eq!(hot_key(0x00), "(none)");
        assert_eq!(hot_key(0x41), "A");
        assert_eq!(hot_key(0x35), "5");
        // Perl's precedence quirk keeps this out of the F-key branch.
        assert_eq!(hot_key(0x70), "p");
        assert_eq!(hot_key(0x90), "Num Lock");
        assert_eq!(hot_key(0x100), "Shift");
        assert_eq!(hot_key(0x241), "Control-A");
        assert_eq!(hot_key(0x630), "Control-Alt-0");
    }

    /// The extension block is only accepted when the trailing back-pointer
    /// agrees with the scanned offset (LNK.pm:1506).
    #[test]
    fn test_find_beef_offset() {
        let mut entry = vec![0u8; 32];
        entry[8 + 5] = 0x00;
        entry[8 + 6] = 0xef;
        entry[8 + 7] = 0xbe;
        entry[30..32].copy_from_slice(&8u16.to_le_bytes());
        assert_eq!(find_beef_offset(&entry), Some(8));

        // A back-pointer that disagrees rejects the candidate.
        entry[30..32].copy_from_slice(&9u16.to_le_bytes());
        assert_eq!(find_beef_offset(&entry), None);
    }

    /// LinkInfo: drive type, serial number and the ANSI local base path.
    #[test]
    fn test_link_info() {
        let mut block = vec![0u8; 0x40];
        block[0..4].copy_from_slice(&0x40u32.to_le_bytes()); // block size
        block[4..8].copy_from_slice(&0x1cu32.to_le_bytes()); // header size
        block[8..12].copy_from_slice(&0x01u32.to_le_bytes()); // VolumeIDAndLocalBasePath
        block[12..16].copy_from_slice(&0x1cu32.to_le_bytes()); // VolumeID offset
        block[16..20].copy_from_slice(&0x30u32.to_le_bytes()); // LocalBasePath offset
        block[0x1c..0x20].copy_from_slice(&0x11u32.to_le_bytes()); // VolumeID size
        block[0x20..0x24].copy_from_slice(&3u32.to_le_bytes()); // DriveType
        block[0x24..0x28].copy_from_slice(&0xc8f0_d326u32.to_le_bytes());
        block[0x28..0x2c].copy_from_slice(&0x10u32.to_le_bytes()); // VolumeLabel offset
        block[0x30..0x3b].copy_from_slice(b"C:\\test.txt");

        let mut metadata = MetadataMap::new();
        LNKParser::read_link_info(&block, &mut metadata);

        assert_eq!(
            metadata.get("LNK:DriveType"),
            Some(&TagValue::String("Fixed Disk".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:DriveSerialNumber"),
            Some(&TagValue::String("C8F0-D326".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:LocalBasePath"),
            Some(&TagValue::String("C:\\test.txt".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:VolumeLabel"),
            Some(&TagValue::String(String::new()))
        );
    }

    /// `%Image::ExifTool::LNK::TrackerData` MachineID.
    #[test]
    fn test_tracker_data_block() {
        let mut data = header(0);
        let block = vec![0u8; 96];
        data.extend_from_slice(&block);
        let base = LNK_HEADER_SIZE;
        data[base..base + 4].copy_from_slice(&96u32.to_le_bytes());
        data[base + 4..base + 8].copy_from_slice(&TRACKER_DATA_SIG.to_le_bytes());
        data[base + 0x10..base + 0x17].copy_from_slice(b"yukkypc");

        let reader = TestReader::new(data);
        let metadata = LNKParser.parse(&reader).unwrap();
        assert_eq!(
            metadata.get("LNK:MachineID"),
            Some(&TagValue::String("yukkypc".to_string()))
        );
        // The forensic GUIDs the old parser invented are not ExifTool tags.
        assert!(metadata.get("LNK:DroidFileID").is_none());
        assert!(metadata.get("MACAddress").is_none());
    }

    /// `%Image::ExifTool::LNK::ConsoleData` conversions.
    #[test]
    fn test_console_data_block() {
        let mut block = vec![0u8; 0x8c];
        block[0x08..0x0a].copy_from_slice(&7u16.to_le_bytes());
        block[0x0a..0x0c].copy_from_slice(&0xf5u16.to_le_bytes());
        block[0x0c..0x0e].copy_from_slice(&80u16.to_le_bytes());
        block[0x0e..0x10].copy_from_slice(&500u16.to_le_bytes());
        block[0x24..0x28].copy_from_slice(&0x30u32.to_le_bytes());
        block[0x28..0x2c].copy_from_slice(&400u32.to_le_bytes());
        for (i, unit) in "8514oem".encode_utf16().enumerate() {
            block[0x2c + i * 2..0x2e + i * 2].copy_from_slice(&unit.to_le_bytes());
        }
        block[0x6c..0x70].copy_from_slice(&25u32.to_le_bytes());
        block[0x78..0x7c].copy_from_slice(&1u32.to_le_bytes());
        block[0x80..0x84].copy_from_slice(&50u32.to_le_bytes());

        let mut metadata = MetadataMap::new();
        LNKParser::read_console_data(&block, &mut metadata);

        assert_eq!(
            metadata.get("LNK:FillAttributes"),
            Some(&TagValue::String("0x07".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:PopupFillAttributes"),
            Some(&TagValue::String("0xf5".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:ScreenBufferSize"),
            Some(&TagValue::String("80 x 500".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:FontFamily"),
            Some(&TagValue::String("Modern".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:FontWeight"),
            Some(&TagValue::Integer(400))
        );
        assert_eq!(
            metadata.get("LNK:FontName"),
            Some(&TagValue::String("8514oem".to_string()))
        );
        assert_eq!(metadata.get("LNK:CursorSize"), Some(&TagValue::Integer(25)));
        assert_eq!(
            metadata.get("LNK:FullScreen"),
            Some(&TagValue::String("No".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:InsertMode"),
            Some(&TagValue::String("Yes".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:HistoryBufferSize"),
            Some(&TagValue::Integer(50))
        );
    }

    /// Unicode string data uses ExifTool's own tag names.
    #[test]
    fn test_string_data_names() {
        // Description + RelativePath, Unicode.
        let mut data = header(0x04 | 0x08 | 0x80);
        let push_string = |data: &mut Vec<u8>, s: &str| {
            let units: Vec<u16> = s.encode_utf16().collect();
            data.extend_from_slice(&(units.len() as u16).to_le_bytes());
            for unit in units {
                data.extend_from_slice(&unit.to_le_bytes());
            }
        };
        push_string(&mut data, "Rename file name");
        push_string(&mut data, ".\\exiftool(-k).exe");
        data.extend_from_slice(&0u32.to_le_bytes()); // terminal block

        let reader = TestReader::new(data);
        let metadata = LNKParser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("LNK:Description"),
            Some(&TagValue::String("Rename file name".to_string()))
        );
        assert_eq!(
            metadata.get("LNK:RelativePath"),
            Some(&TagValue::String(".\\exiftool(-k).exe".to_string()))
        );
        // ExifTool has no "Name" tag for LNK.
        assert!(metadata.get("LNK:Name").is_none());
        assert!(metadata.get("Name").is_none());
    }
}
