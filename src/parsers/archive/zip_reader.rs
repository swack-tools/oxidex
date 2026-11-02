//! Minimal read-only ZIP container reader.
//!
//! This replaces the `zip` crate. OxiDex needs about five percent of what that
//! crate does -- open an in-memory archive, find an entry by name, and inflate
//! a few small XML files -- and it was inheriting that crate's entire
//! dependency tree and advisory surface for the privilege:
//!
//! - RUSTSEC-2026-0367/0368/0369 (`zip`): SUID/SGID preservation, symlink
//!   escape, and permissions applied through symlinks, all during
//!   `ZipArchive::extract`. OxiDex never extracts anything. This reader cannot
//!   extract, cannot write, and cannot touch the filesystem, so that whole
//!   class is inapplicable by construction rather than merely patched.
//! - RUSTSEC-2026-0391/0398/0403/0410 (`zstd`, `zstd-safe`, `zstd-sys`): pulled
//!   in transitively by `zip` purely so it could decompress Zstd entries.
//!   OxiDex only ever reports the method name of such an entry as a tag value.
//!
//! What it deliberately does NOT do: write archives, decrypt, decompress
//! anything other than Stored and Deflate, or extract to disk. An entry in an
//! unsupported method is still *enumerated* with its sizes, CRC and method name
//! -- only reading its bytes fails. That matters because the metadata OxiDex
//! publishes for such an archive is unchanged.
//!
//! # Hardening
//!
//! Every field is read through bounds-checked helpers, so a truncated or
//! malicious archive yields `Err`, never a panic or an out-of-bounds read.
//! Specifically:
//!
//! - The central directory is the single source of truth for sizes and offsets.
//!   Local headers are only checked for their signature; their size fields are
//!   ignored, because a streamed entry writes zeros there and defers the real
//!   values to a data descriptor after the payload.
//! - Inflation is capped at the declared uncompressed size, so a zip bomb that
//!   lies about its size produces a short read and an error rather than an
//!   unbounded allocation.
//! - The entry count from the end-of-central-directory record is treated as a
//!   hint, not a promise: it is clamped against what the buffer could actually
//!   hold, since each central directory entry needs at least 46 bytes.
//!
//! See `fuzz/fuzz_targets/fuzz_zip.rs`, and the differential test against the
//! `zip` crate in `tests/zip_reader_differential.rs`.

use std::borrow::Cow;

/// Local file header signature, `PK\x03\x04`.
const LOCAL_HEADER_SIG: u32 = 0x0403_4b50;
/// Central directory file header signature, `PK\x01\x02`.
const CENTRAL_HEADER_SIG: u32 = 0x0201_4b50;
/// End of central directory signature, `PK\x05\x06`.
const EOCD_SIG: u32 = 0x0605_4b50;
/// ZIP64 end of central directory record signature, `PK\x06\x06`.
const ZIP64_EOCD_SIG: u32 = 0x0606_4b50;
/// ZIP64 end of central directory locator signature, `PK\x06\x07`.
const ZIP64_LOCATOR_SIG: u32 = 0x0706_4b50;

/// Fixed size of a central directory file header, before the variable-length
/// name, extra and comment fields.
const CENTRAL_HEADER_LEN: usize = 46;
/// Fixed size of a local file header, before name and extra.
const LOCAL_HEADER_LEN: usize = 30;
/// Fixed size of an end-of-central-directory record, before its comment.
const EOCD_LEN: usize = 22;

/// The EOCD comment field is a `u16` length, so the record can start at most
/// this far from the end of the file.
const MAX_EOCD_SEARCH: usize = EOCD_LEN + u16::MAX as usize;

/// Ceiling on a single entry's inflated size, independent of what the archive
/// claims. OxiDex reads small XML and plist members; anything larger is not
/// metadata and is not worth the allocation. A caller that hits this gets an
/// error rather than a truncated value, because a silently short XML document
/// would parse into wrong tags, which is worse than no tags at all.
const MAX_INFLATED_SIZE: u64 = 256 * 1024 * 1024;

/// Why an archive could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ZipReadError {
    /// No end-of-central-directory record was found.
    NotAZip,
    /// A structure extended past the end of the buffer.
    Truncated,
    /// A signature was not what the format requires at that offset.
    BadSignature,
    /// An offset or length in the archive does not address the buffer.
    BadOffset,
    /// No entry with the requested name exists.
    EntryNotFound,
    /// The entry's compression method has no decompressor here.
    UnsupportedCompression(u16),
    /// Decompression failed, or produced a different size than declared.
    DecompressionFailed,
    /// The entry's declared size exceeds [`MAX_INFLATED_SIZE`].
    EntryTooLarge(u64),
}

impl std::fmt::Display for ZipReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAZip => write!(f, "no ZIP end-of-central-directory record found"),
            Self::Truncated => write!(f, "ZIP structure extends past end of data"),
            Self::BadSignature => write!(f, "bad ZIP signature"),
            Self::BadOffset => write!(f, "ZIP offset does not address the archive"),
            Self::EntryNotFound => write!(f, "entry not found in archive"),
            Self::UnsupportedCompression(m) => {
                write!(f, "unsupported ZIP compression method {m}")
            }
            Self::DecompressionFailed => write!(f, "ZIP entry decompression failed"),
            Self::EntryTooLarge(n) => write!(f, "ZIP entry declares {n} bytes, over the limit"),
        }
    }
}

impl std::error::Error for ZipReadError {}

type Result<T> = std::result::Result<T, ZipReadError>;

/// A DOS date/time, as stored in a ZIP header.
///
/// Kept as the raw packed fields rather than converted to a calendar type: the
/// values are frequently nonsense in real archives (zero, or a month of 0) and
/// callers render them as-is. Interpreting them further would invent data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DosDateTime {
    date: u16,
    time: u16,
}

impl DosDateTime {
    /// Returns `None` when both fields are zero, which is how archivers mark
    /// "no timestamp" -- distinct from a real timestamp that happens to be
    /// 1980-00-00, which this still reports.
    fn from_parts(date: u16, time: u16) -> Option<Self> {
        if date == 0 && time == 0 {
            None
        } else {
            Some(Self { date, time })
        }
    }

    pub fn year(&self) -> u16 {
        1980 + (self.date >> 9)
    }
    pub fn month(&self) -> u8 {
        ((self.date >> 5) & 0x0f) as u8
    }
    pub fn day(&self) -> u8 {
        (self.date & 0x1f) as u8
    }
    pub fn hour(&self) -> u8 {
        (self.time >> 11) as u8
    }
    pub fn minute(&self) -> u8 {
        ((self.time >> 5) & 0x3f) as u8
    }
    /// DOS stores seconds in two-second units, so odd seconds are not
    /// representable and this is always even.
    pub fn second(&self) -> u8 {
        ((self.time & 0x1f) * 2) as u8
    }
}

/// One entry, as described by its central directory header.
#[derive(Debug, Clone)]
pub struct ZipEntry {
    name: String,
    compression: u16,
    crc32: u32,
    compressed_size: u64,
    uncompressed_size: u64,
    local_header_offset: u64,
    external_attributes: u32,
    version_made_by: u16,
    flags: u16,
    last_modified: Option<DosDateTime>,
}

impl ZipEntry {
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Raw compression method code, e.g. 0 Stored, 8 Deflate, 93 Zstd.
    pub fn compression(&self) -> u16 {
        self.compression
    }
    pub fn crc32(&self) -> u32 {
        self.crc32
    }
    pub fn compressed_size(&self) -> u64 {
        self.compressed_size
    }
    /// Uncompressed size, named `size` to match the call sites this replaced.
    pub fn size(&self) -> u64 {
        self.uncompressed_size
    }
    pub fn last_modified(&self) -> Option<DosDateTime> {
        self.last_modified
    }

    /// `(host system, zip specification version)`, as the central directory
    /// packs them into one `u16`.
    pub fn version_made_by(&self) -> (u8, u8) {
        (
            (self.version_made_by >> 8) as u8,
            (self.version_made_by & 0xff) as u8,
        )
    }

    /// Unix permission bits, present only when the archive was made on a Unix
    /// host. The high 16 bits of the external attributes carry `st_mode`.
    pub fn unix_mode(&self) -> Option<u32> {
        // Host 3 is UNIX; 19 is OS X. Other hosts put DOS attributes here, and
        // reporting those as a Unix mode would be a plausible wrong value.
        match self.version_made_by >> 8 {
            3 | 19 => Some(self.external_attributes >> 16),
            _ => None,
        }
    }

    /// True when bit 0 of the general purpose flags is set.
    pub fn is_encrypted(&self) -> bool {
        self.flags & 1 != 0
    }

    /// Directory entries are conventionally marked by a trailing separator.
    pub fn is_dir(&self) -> bool {
        self.name.ends_with('/') || self.name.ends_with('\\')
    }

    /// Human-readable compression method, covering the methods APPNOTE assigns
    /// even where this reader cannot decompress them -- the name is a tag value
    /// and is reported regardless.
    pub fn compression_name(&self) -> &'static str {
        match self.compression {
            0 => "Stored",
            1 => "Shrunk",
            6 => "Imploded",
            8 => "Deflate",
            9 => "Deflate64",
            12 => "Bzip2",
            14 => "LZMA",
            93 => "Zstd",
            95 => "XZ",
            96 => "Jpeg",
            97 => "WavPack",
            98 => "PPMd",
            99 => "AES",
            _ => "Unknown",
        }
    }
}

/// A parsed, read-only view over an in-memory ZIP archive.
#[derive(Debug, Clone)]
pub struct ZipReader<'a> {
    data: &'a [u8],
    entries: Vec<ZipEntry>,
    comment: Vec<u8>,
    /// Bytes of non-ZIP prefix (a self-extracting stub) that every offset in
    /// the central directory is implicitly relative to. Zero for ordinary
    /// archives. See `locate_central_directory`.
    offset_shift: u64,
}

/// Reads a little-endian `u16` at `off`, or `Truncated`.
fn u16_at(data: &[u8], off: usize) -> Result<u16> {
    data.get(off..off + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(ZipReadError::Truncated)
}

/// Reads a little-endian `u32` at `off`, or `Truncated`.
fn u32_at(data: &[u8], off: usize) -> Result<u32> {
    data.get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(ZipReadError::Truncated)
}

/// Reads a little-endian `u64` at `off`, or `Truncated`.
fn u64_at(data: &[u8], off: usize) -> Result<u64> {
    data.get(off..off + 8)
        .map(|b| u64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        .ok_or(ZipReadError::Truncated)
}

/// Converts a `u64` from the archive into a buffer index, rejecting anything
/// that cannot address `data`. This is the only place archive-supplied offsets
/// become indices, so every such conversion is checked.
fn offset_to_index(value: u64, data: &[u8]) -> Result<usize> {
    let idx = usize::try_from(value).map_err(|_| ZipReadError::BadOffset)?;
    if idx > data.len() {
        return Err(ZipReadError::BadOffset);
    }
    Ok(idx)
}

impl<'a> ZipReader<'a> {
    /// Parses the central directory of `data`.
    ///
    /// Reads no entry payloads, so the cost is proportional to the number of
    /// entries rather than the size of the archive.
    pub fn new(data: &'a [u8]) -> Result<Self> {
        let eocd = Self::find_eocd(data).ok_or(ZipReadError::NotAZip)?;

        let mut entry_count = u16_at(data, eocd + 10)? as u64;
        let mut cd_size = u32_at(data, eocd + 12)? as u64;
        let mut cd_offset = u32_at(data, eocd + 16)? as u64;

        let comment_len = u16_at(data, eocd + 20)? as usize;
        let comment_start = eocd + EOCD_LEN;
        // A comment length longer than the remaining bytes is a malformed
        // archive rather than a fatal one: take what is actually there.
        let comment_end = comment_start.saturating_add(comment_len).min(data.len());
        let comment = data
            .get(comment_start..comment_end)
            .unwrap_or_default()
            .to_vec();

        // ZIP64 only when the 32-bit fields are saturated, which is the signal
        // the format uses. Looking for the ZIP64 records unconditionally would
        // misread a plain archive whose bytes happen to contain the signature.
        if entry_count == u16::MAX as u64
            || cd_offset == u32::MAX as u64
            || cd_size == u32::MAX as u64
        {
            if let Some((z64_entries, z64_size, z64_offset)) = Self::read_zip64(data, eocd) {
                entry_count = z64_entries;
                cd_size = z64_size;
                cd_offset = z64_offset;
            }
        }

        // A self-extracting archive carries an executable stub before the first
        // local header, so every offset the central directory records is
        // relative to where the ZIP data starts rather than to the file. The
        // declared offset then points into the stub.
        //
        // The central directory always ends at the EOCD, so `eocd - cd_size`
        // is its true position; the difference from the declared offset is the
        // prefix length, and the same shift applies to every local header
        // offset. Trusting the declared offset instead reads zero entries,
        // which is how this was caught -- by the differential test against the
        // `zip` crate, on a stub-prefixed archive.
        let (cd_start, offset_shift) =
            Self::locate_central_directory(data, eocd, cd_size, cd_offset)?;

        let mut cursor = cd_start;

        // The declared count is a hint. Each central directory header occupies
        // at least CENTRAL_HEADER_LEN bytes, so an archive claiming more
        // entries than could fit is lying and would otherwise have us
        // preallocate on its word.
        let max_possible = data.len().saturating_sub(cursor) / CENTRAL_HEADER_LEN;
        let capacity = (entry_count as usize).min(max_possible);
        let mut entries = Vec::with_capacity(capacity);

        for _ in 0..entry_count {
            if u32_at(data, cursor)? != CENTRAL_HEADER_SIG {
                // A short central directory is common in damaged archives.
                // Stop and keep what parsed rather than discarding all of it;
                // the entries already read are individually well-formed.
                break;
            }
            let (entry, next) = Self::parse_central_entry(data, cursor)?;
            entries.push(entry);
            cursor = next;
        }

        Ok(Self {
            data,
            entries,
            comment,
            offset_shift,
        })
    }

    /// Scans backwards for the end-of-central-directory signature.
    ///
    /// Backwards because the record sits at the end, and an archive may carry a
    /// self-extracting stub or trailing junk; the *last* valid-looking record
    /// is the operative one.
    fn find_eocd(data: &[u8]) -> Option<usize> {
        if data.len() < EOCD_LEN {
            return None;
        }
        let search_start = data.len().saturating_sub(MAX_EOCD_SEARCH);
        let last_possible = data.len() - EOCD_LEN;
        for off in (search_start..=last_possible).rev() {
            if u32_at(data, off).ok()? == EOCD_SIG {
                return Some(off);
            }
        }
        None
    }

    /// Follows the ZIP64 locator that precedes the EOCD to the ZIP64 EOCD
    /// record. Returns `None` on any inconsistency, leaving the 32-bit values
    /// in force, since a partially-understood ZIP64 archive should degrade to
    /// what the classic record says rather than guess.
    fn read_zip64(data: &[u8], eocd: usize) -> Option<(u64, u64, u64)> {
        // The locator is a fixed 20 bytes immediately before the EOCD.
        let locator = eocd.checked_sub(20)?;
        if u32_at(data, locator).ok()? != ZIP64_LOCATOR_SIG {
            return None;
        }
        let z64_eocd = offset_to_index(u64_at(data, locator + 8).ok()?, data).ok()?;
        if u32_at(data, z64_eocd).ok()? != ZIP64_EOCD_SIG {
            return None;
        }
        let entries = u64_at(data, z64_eocd + 32).ok()?;
        let cd_size = u64_at(data, z64_eocd + 40).ok()?;
        let cd_offset = u64_at(data, z64_eocd + 48).ok()?;
        Some((entries, cd_size, cd_offset))
    }

    /// Resolves where the central directory really begins, and by how much
    /// the archive's internal offsets are shifted.
    ///
    /// Returns `(index of first central header, shift)`. The shift is added to
    /// each entry's local header offset when reading its payload.
    ///
    /// Order matters: the declared offset is tried first, so an ordinary
    /// archive is unaffected and cannot be mis-shifted by a coincidence. Only
    /// when that does not hold a central header is the `eocd - cd_size`
    /// position consulted.
    fn locate_central_directory(
        data: &[u8],
        eocd: usize,
        cd_size: u64,
        cd_offset: u64,
    ) -> Result<(usize, u64)> {
        // Ordinary archive: the declared offset addresses a central header.
        if let Ok(idx) = offset_to_index(cd_offset, data) {
            if u32_at(data, idx) == Ok(CENTRAL_HEADER_SIG) {
                return Ok((idx, 0));
            }
        }

        // Prefixed archive: the directory ends at the EOCD, so back up by its
        // size. The shift is how far the real position is past the declared.
        let computed = (eocd as u64).checked_sub(cd_size);
        if let Some(computed) = computed {
            if let Ok(idx) = offset_to_index(computed, data) {
                if u32_at(data, idx) == Ok(CENTRAL_HEADER_SIG) {
                    let shift = computed.saturating_sub(cd_offset);
                    return Ok((idx, shift));
                }
            }
        }

        // Neither position holds a central header. An archive with zero
        // entries legitimately has no central header at all, so accept the
        // declared offset and let the (empty) entry loop confirm it.
        Ok((offset_to_index(cd_offset, data)?, 0))
    }

    /// Parses one central directory header at `off`, returning it and the
    /// offset of the next header.
    fn parse_central_entry(data: &[u8], off: usize) -> Result<(ZipEntry, usize)> {
        let version_made_by = u16_at(data, off + 4)?;
        let flags = u16_at(data, off + 8)?;
        let compression = u16_at(data, off + 10)?;
        let mod_time = u16_at(data, off + 12)?;
        let mod_date = u16_at(data, off + 14)?;
        let crc32 = u32_at(data, off + 16)?;
        let mut compressed_size = u32_at(data, off + 20)? as u64;
        let mut uncompressed_size = u32_at(data, off + 24)? as u64;
        let name_len = u16_at(data, off + 28)? as usize;
        let extra_len = u16_at(data, off + 30)? as usize;
        let comment_len = u16_at(data, off + 32)? as usize;
        let external_attributes = u32_at(data, off + 38)?;
        let mut local_header_offset = u32_at(data, off + 42)? as u64;

        let name_start = off + CENTRAL_HEADER_LEN;
        let name_bytes = data
            .get(name_start..name_start + name_len)
            .ok_or(ZipReadError::Truncated)?;

        // Flag bit 11 promises UTF-8. Without it the name is officially CP437,
        // but in practice it is usually UTF-8 anyway, so decode as UTF-8 and
        // fall back to lossy rather than mis-transcoding. Entry names are used
        // for lookup and reporting, never as filesystem paths.
        let name = match std::str::from_utf8(name_bytes) {
            Ok(s) => s.to_string(),
            Err(_) => String::from_utf8_lossy(name_bytes).into_owned(),
        };

        let extra_start = name_start + name_len;
        let extra = data
            .get(extra_start..extra_start + extra_len)
            .ok_or(ZipReadError::Truncated)?;

        // ZIP64 extended information (header id 0x0001) replaces whichever
        // 32-bit fields are saturated, in a fixed order, each present only if
        // its 32-bit counterpart was 0xFFFFFFFF.
        if compressed_size == u32::MAX as u64
            || uncompressed_size == u32::MAX as u64
            || local_header_offset == u32::MAX as u64
        {
            if let Some(z64) = find_extra_field(extra, 0x0001) {
                let mut p = 0usize;
                if uncompressed_size == u32::MAX as u64 {
                    if let Ok(v) = u64_at(z64, p) {
                        uncompressed_size = v;
                        p += 8;
                    }
                }
                if compressed_size == u32::MAX as u64 {
                    if let Ok(v) = u64_at(z64, p) {
                        compressed_size = v;
                        p += 8;
                    }
                }
                if local_header_offset == u32::MAX as u64 {
                    if let Ok(v) = u64_at(z64, p) {
                        local_header_offset = v;
                    }
                }
            }
        }

        let next = extra_start
            .checked_add(extra_len)
            .and_then(|v| v.checked_add(comment_len))
            .ok_or(ZipReadError::Truncated)?;

        Ok((
            ZipEntry {
                name,
                compression,
                crc32,
                compressed_size,
                uncompressed_size,
                local_header_offset,
                external_attributes,
                version_made_by,
                flags,
                last_modified: DosDateTime::from_parts(mod_date, mod_time),
            },
            next,
        ))
    }

    /// Number of entries parsed. May be fewer than the archive declared when
    /// its central directory is damaged.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The archive comment, lossily decoded. Comments are free text with no
    /// declared encoding.
    pub fn comment(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(&self.comment)
    }

    pub fn entries(&self) -> &[ZipEntry] {
        &self.entries
    }

    pub fn by_index(&self, index: usize) -> Option<&ZipEntry> {
        self.entries.get(index)
    }

    pub fn by_name(&self, name: &str) -> Option<&ZipEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.by_name(name).is_some()
    }

    /// Decompresses the entry named `name`.
    pub fn read_by_name(&self, name: &str) -> Result<Vec<u8>> {
        let entry = self.by_name(name).ok_or(ZipReadError::EntryNotFound)?;
        self.read_entry(entry)
    }

    /// Decompresses `name` and decodes it as UTF-8, lossily.
    ///
    /// Lossy because the alternative is failing a whole document over one bad
    /// byte in an XML comment, and these inputs are metadata parts whose
    /// encoding declarations are not always honest.
    pub fn read_to_string(&self, name: &str) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.read_by_name(name)?).into_owned())
    }

    /// Decompresses one entry's payload.
    ///
    /// Sizes come from the central directory, never the local header: a
    /// streamed entry stores zeros there and writes the real values to a data
    /// descriptor after the payload.
    pub fn read_entry(&self, entry: &ZipEntry) -> Result<Vec<u8>> {
        if entry.uncompressed_size > MAX_INFLATED_SIZE {
            return Err(ZipReadError::EntryTooLarge(entry.uncompressed_size));
        }

        let local = offset_to_index(
            entry.local_header_offset.saturating_add(self.offset_shift),
            self.data,
        )?;
        if u32_at(self.data, local)? != LOCAL_HEADER_SIG {
            return Err(ZipReadError::BadSignature);
        }
        // Only the variable-length counts are taken from the local header; the
        // payload begins after them.
        let local_name_len = u16_at(self.data, local + 26)? as usize;
        let local_extra_len = u16_at(self.data, local + 28)? as usize;

        let data_start = local
            .checked_add(LOCAL_HEADER_LEN)
            .and_then(|v| v.checked_add(local_name_len))
            .and_then(|v| v.checked_add(local_extra_len))
            .ok_or(ZipReadError::BadOffset)?;

        let compressed_len =
            usize::try_from(entry.compressed_size).map_err(|_| ZipReadError::BadOffset)?;
        let data_end = data_start
            .checked_add(compressed_len)
            .ok_or(ZipReadError::BadOffset)?;
        let payload = self
            .data
            .get(data_start..data_end)
            .ok_or(ZipReadError::Truncated)?;

        match entry.compression {
            0 => Ok(payload.to_vec()),
            8 => inflate(payload, entry.uncompressed_size),
            other => Err(ZipReadError::UnsupportedCompression(other)),
        }
    }
}

/// Finds an extra-field record by header id within an extra-field block.
///
/// The block is a sequence of `(u16 id, u16 len, len bytes)`. A malformed
/// length terminates the walk rather than running past the block.
fn find_extra_field(extra: &[u8], want_id: u16) -> Option<&[u8]> {
    let mut off = 0usize;
    while off + 4 <= extra.len() {
        let id = u16::from_le_bytes([extra[off], extra[off + 1]]);
        let len = u16::from_le_bytes([extra[off + 2], extra[off + 3]]) as usize;
        let body_start = off + 4;
        let body_end = body_start.checked_add(len)?;
        if body_end > extra.len() {
            return None;
        }
        if id == want_id {
            return Some(&extra[body_start..body_end]);
        }
        off = body_end;
    }
    None
}

/// Inflates a raw deflate stream, refusing to produce more than
/// `declared_size` bytes.
///
/// The cap is what makes a zip bomb harmless: an entry claiming a small
/// uncompressed size cannot expand past it, and one claiming a large size is
/// rejected earlier by [`MAX_INFLATED_SIZE`]. Producing fewer bytes than
/// declared is also an error -- a short XML document would parse into wrong
/// tags, and AGENTS.md is explicit that a plausible wrong value is worse than
/// an absent one.
fn inflate(payload: &[u8], declared_size: u64) -> Result<Vec<u8>> {
    use std::io::Read;

    let cap = usize::try_from(declared_size).map_err(|_| ZipReadError::BadOffset)?;
    let mut out = Vec::new();
    out.try_reserve_exact(cap)
        .map_err(|_| ZipReadError::EntryTooLarge(declared_size))?;

    // take(cap + 1) so an over-long stream is detected rather than silently
    // truncated to the declared size.
    let mut decoder = flate2::read::DeflateDecoder::new(payload).take(declared_size + 1);
    decoder
        .read_to_end(&mut out)
        .map_err(|_| ZipReadError::DecompressionFailed)?;

    if out.len() as u64 != declared_size {
        return Err(ZipReadError::DecompressionFailed);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a single-entry Stored archive by hand, so the tests do not
    /// depend on a ZIP writer.
    fn stored_archive(name: &str, body: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        let crc = crc32(body);
        let n = name.as_bytes();

        let local_off = v.len() as u32;
        v.extend_from_slice(&LOCAL_HEADER_SIG.to_le_bytes());
        v.extend_from_slice(&10u16.to_le_bytes()); // version needed
        v.extend_from_slice(&0u16.to_le_bytes()); // flags
        v.extend_from_slice(&0u16.to_le_bytes()); // method: stored
        v.extend_from_slice(&0u16.to_le_bytes()); // time
        v.extend_from_slice(&0x2821u16.to_le_bytes()); // date: 2000-01-01
        v.extend_from_slice(&crc.to_le_bytes());
        v.extend_from_slice(&(body.len() as u32).to_le_bytes());
        v.extend_from_slice(&(body.len() as u32).to_le_bytes());
        v.extend_from_slice(&(n.len() as u16).to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes()); // extra len
        v.extend_from_slice(n);
        v.extend_from_slice(body);

        let cd_off = v.len() as u32;
        v.extend_from_slice(&CENTRAL_HEADER_SIG.to_le_bytes());
        v.extend_from_slice(&0x031eu16.to_le_bytes()); // made by: unix
        v.extend_from_slice(&10u16.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes()); // flags
        v.extend_from_slice(&0u16.to_le_bytes()); // method
        v.extend_from_slice(&0u16.to_le_bytes()); // time
        v.extend_from_slice(&0x2821u16.to_le_bytes()); // date
        v.extend_from_slice(&crc.to_le_bytes());
        v.extend_from_slice(&(body.len() as u32).to_le_bytes());
        v.extend_from_slice(&(body.len() as u32).to_le_bytes());
        v.extend_from_slice(&(n.len() as u16).to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes()); // extra
        v.extend_from_slice(&0u16.to_le_bytes()); // comment
        v.extend_from_slice(&0u16.to_le_bytes()); // disk
        v.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
        v.extend_from_slice(&0o100644u32.wrapping_shl(16).to_le_bytes()); // external
        v.extend_from_slice(&local_off.to_le_bytes());
        v.extend_from_slice(n);

        let cd_len = v.len() as u32 - cd_off;
        v.extend_from_slice(&EOCD_SIG.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes()); // disk
        v.extend_from_slice(&0u16.to_le_bytes()); // cd disk
        v.extend_from_slice(&1u16.to_le_bytes()); // entries this disk
        v.extend_from_slice(&1u16.to_le_bytes()); // entries total
        v.extend_from_slice(&cd_len.to_le_bytes());
        v.extend_from_slice(&cd_off.to_le_bytes());
        v.extend_from_slice(&0u16.to_le_bytes()); // comment len
        v
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn reads_a_stored_entry() {
        let zip = stored_archive("hello.txt", b"hello world");
        let r = ZipReader::new(&zip).expect("should parse");
        assert_eq!(r.len(), 1);
        let e = r.by_index(0).expect("entry 0");
        assert_eq!(e.name(), "hello.txt");
        assert_eq!(e.size(), 11);
        assert_eq!(e.compression(), 0);
        assert_eq!(e.compression_name(), "Stored");
        assert!(!e.is_dir());
        assert!(!e.is_encrypted());
        assert_eq!(r.read_to_string("hello.txt").expect("read"), "hello world");
    }

    #[test]
    fn reports_unix_mode_only_for_unix_hosts() {
        let zip = stored_archive("a.txt", b"x");
        let r = ZipReader::new(&zip).expect("parse");
        // The fixture declares host 3 (UNIX), so the mode is meaningful.
        assert_eq!(r.by_index(0).expect("entry").unix_mode(), Some(0o100644));
    }

    #[test]
    fn decodes_dos_datetime() {
        let zip = stored_archive("a.txt", b"x");
        let r = ZipReader::new(&zip).expect("parse");
        let dt = r.by_index(0).expect("entry").last_modified().expect("dt");
        assert_eq!(dt.year(), 2000);
        assert_eq!(dt.month(), 1);
        assert_eq!(dt.day(), 1);
    }

    #[test]
    fn missing_entry_is_an_error_not_a_panic() {
        let zip = stored_archive("a.txt", b"x");
        let r = ZipReader::new(&zip).expect("parse");
        assert_eq!(r.read_by_name("nope"), Err(ZipReadError::EntryNotFound));
        assert!(!r.contains("nope"));
    }

    #[test]
    fn rejects_non_zip_input() {
        assert_eq!(
            ZipReader::new(b"not a zip").unwrap_err(),
            ZipReadError::NotAZip
        );
        assert_eq!(ZipReader::new(b"").unwrap_err(), ZipReadError::NotAZip);
    }

    /// Every truncation of a valid archive must be an error, never a panic.
    /// This is the property the fuzz target explores more widely.
    #[test]
    fn every_truncation_fails_cleanly() {
        let zip = stored_archive("hello.txt", b"hello world");
        for n in 0..zip.len() {
            if let Ok(r) = ZipReader::new(&zip[..n]) {
                // Parsing a prefix may succeed when the EOCD survives; reading
                // must then still not panic.
                for i in 0..r.len() {
                    if let Some(e) = r.by_index(i) {
                        let _ = r.read_entry(e);
                    }
                }
            }
        }
    }

    /// A single flipped byte must not panic, at any position.
    #[test]
    fn single_byte_corruption_never_panics() {
        let zip = stored_archive("hello.txt", b"hello world");
        for i in 0..zip.len() {
            let mut c = zip.clone();
            c[i] ^= 0xff;
            if let Ok(r) = ZipReader::new(&c) {
                for idx in 0..r.len() {
                    if let Some(e) = r.by_index(idx) {
                        let _ = r.read_entry(e);
                    }
                }
                let _ = r.comment();
            }
        }
    }

    #[test]
    fn unsupported_method_still_enumerates() {
        let mut zip = stored_archive("a.txt", b"x");
        // Rewrite the central directory method to Zstd (93). The entry must
        // still be listed with its name and sizes; only reading it fails.
        let cd = zip
            .windows(4)
            .position(|w| w == CENTRAL_HEADER_SIG.to_le_bytes())
            .expect("central header");
        zip[cd + 10..cd + 12].copy_from_slice(&93u16.to_le_bytes());
        let r = ZipReader::new(&zip).expect("parse");
        let e = r.by_index(0).expect("entry");
        assert_eq!(e.compression_name(), "Zstd");
        assert_eq!(e.name(), "a.txt");
        assert_eq!(
            r.read_entry(e),
            Err(ZipReadError::UnsupportedCompression(93))
        );
    }

    #[test]
    fn absurd_entry_count_does_not_preallocate() {
        let mut zip = stored_archive("a.txt", b"x");
        let eocd = ZipReader::find_eocd(&zip).expect("eocd");
        // Claim 65534 entries in an archive that holds one.
        zip[eocd + 10..eocd + 12].copy_from_slice(&65534u16.to_le_bytes());
        // Must stop at the first non-signature rather than trusting the count.
        let r = ZipReader::new(&zip).expect("parse");
        assert_eq!(r.len(), 1);
    }
}
