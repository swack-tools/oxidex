//! The `ProcessExif` engine (slice I-1): one walk over an IFD-style
//! [`IfdTable`] and everything its `SubDirectory` edges reach.
//!
//! # Why this module exists
//!
//! `Image::ExifTool::Exif::ProcessExif` (Exif.pm:6278-7240, pinned 13.59) is
//! the function behind EXIF, GPS and most MakerNotes `Main` tables -- 495
//! tables and 17,986 tag entries the binary-table generator never
//! transcribed (design spec section 1). Its reader already exists in this
//! repository as `parsers::tiff::makernotes::shared::table_ifd`'s `read_ifd`
//! / `decode_entry_with_floor`, hand-wired to a private `TagDef` vocabulary
//! for five vendors. This module ports that reader down into the generated-
//! table layer (`exiftool_tables` is the lower layer and must not depend on
//! `parsers::`), keys it on an [`IfdTable`], and reuses the conversion
//! machinery the binary engine already proves against the oracle:
//! [`runtime::apply_value_conv`], [`runtime::render`], `cond::first_match`.
//! `table_ifd.rs` is made to delegate to this in a later slice.
//!
//! # The Perl this reproduces, line by line (pinned 13.59)
//!
//! ```text
//! Exif.pm:6294   my $inMakerNotes = $$tagTablePtr{GROUPS}{0} eq 'MakerNotes';
//! Exif.pm:6346   $numEntries = Get16u($dataPt, $dirStart);
//! Exif.pm:6447   $$et{Compression} = $$et{SubfileType} = '';
//! Exif.pm:6455   if ($warnCount > 10) { ... "parsing aborted" ... return 0 }
//! Exif.pm:6458   my $entry = $dirStart + 2 + 12 * $index;
//! Exif.pm:6459   my $tagID = Get16u($dataPt, $entry);
//! Exif.pm:6460   my $format = Get16u($dataPt, $entry+2);
//! Exif.pm:6461   my $count = Get32u($dataPt, $entry+4);
//! Exif.pm:6463   if (($format < 1 or $format > 13) and $format != 129 and not ($format == 16 and Make eq 'Apple' and $inMakerNotes))
//! Exif.pm:6475       next if $index or $$et{Model} =~ /^ILCE/;  return 0;
//! Exif.pm:6480   my $formatStr = $formatName[$format];
//! Exif.pm:6484   my $valuePtr = $entry + 8;
//! Exif.pm:6485   my $tagInfo = $et->GetTagInfo($tagTablePtr, $tagID);
//! Exif.pm:6502   my $size = $count * $formatSize[$format];
//! Exif.pm:6504   if ($size > 4) {
//! Exif.pm:6505       if ($size > 0x7fffffff ...) { Warn; ++$warnCount; next }
//! Exif.pm:6510       $valuePtr = Get32u($dataPt, $valuePtr);
//! Exif.pm:6546       $valuePtr -= $dataPos;
//! Exif.pm:6549       $suspect = $warnCount if $valuePtr < $dirEnd and $valuePtr+$size > $dirStart;
//! Exif.pm:6551       if ($valuePtr < 0 or $valuePtr+$size > $dataLen) { ... "Bad offset" ... $bad = 1 }
//! Exif.pm:6673       if (defined $suspect and $suspect == $warnCount) { Warn("Suspicious ..."); ++$warnCount; next }
//! Exif.pm:6682   $formatStr = 'int8u' if $format == 7 and $count == 1;
//! Exif.pm:6717   my $tmpVal = substr($$valueDataPt, $valuePtr, $readSize < 128 ? $readSize : 128);
//! Exif.pm:6719   $tagInfo = $et->GetTagInfo($tagTablePtr, $tagID, \$tmpVal, $formatName[$format], $count);
//! Exif.pm:6729   my $readFormat = $$tagInfo{Format};
//! Exif.pm:6733   $readFormat = 'undef' if $subdir and not $$tagInfo{SubIFD} and not $readFormat;
//! Exif.pm:6735   if ($readFormat) { $formatStr = $readFormat; $newNum = $formatNumber{$formatStr};
//! Exif.pm:6738       if ($newNum and $newNum != $format) { $format = $newNum; $count = int($size / $formatSize[$format]) } }
//! Exif.pm:6747   if (($$tagInfo{IsOffset} or $$tagInfo{SubIFD}) and not $intFormat{$formatStr}) { ... next }
//! Exif.pm:6763   if ($count > 100000 and $formatStr !~ /^(undef|string|binary)$/) { ... next }
//! Exif.pm:6782   $val = ReadValue($valueDataPt,$valuePtr,$formatStr,$count,$readSize,\$rational);
//! Exif.pm:6915   next unless defined $val;
//! Exif.pm:6919   if ($subdir) { ... }                       # see `descend`
//! Exif.pm:7180   $tagKey = $et->FoundTag($tagInfo, $val);
//! Exif.pm:7183   $et->SetGroup($tagKey, $dirName) if $$tagTablePtr{SET_GROUP1};
//! ```
//!
//! and, reached from `FoundTag`/`GetValue`/`GetTagInfo` in ExifTool.pm:
//!
//! ```text
//! ExifTool.pm:9164   foreach $tagInfo (@infoArray) { ... eval $condition ... }   # first match wins
//! ExifTool.pm:9180   if ($$tagInfo{Unknown} and not $$options{Unknown} ...) { return undef }
//! ExifTool.pm:9469   my $priority = $$tagInfo{Priority};
//! ExifTool.pm:9471   $priority = $$tbl{PRIORITY}; $priority = 0 if not defined $priority and $$tagInfo{Avoid};
//! ExifTool.pm:9484   if ($$tagInfo{RawConv}) { ... $value = eval $conv ... }
//! ExifTool.pm:3538   next unless $$tagInfo{Binary}; $conv = '\$val';   # scalar ref -> "(Binary data N bytes, use -b option to extract)" (exiftool:3983-3988)
//! ExifTool.pm:6330   return join(' ', @vals) if @vals > 1;               # ReadValue: one space-joined scalar per entry
//! ```
//!
//! # What is deliberately NOT here
//!
//! Every refusal below withholds the tag (or the sub-directory) rather than
//! guessing -- `AGENTS.md`, "never approximate". Each is a rule the schema
//! cannot carry or a Perl path this port does not walk:
//!
//! * `Format` overrides outside `%formatNumber` (Exif.pm:6737): an override
//!   whose spelling ExifTool cannot map to a format *number* (`int16uRev`,
//!   `rational32u`, the `fixed*` family, a sized `string[N]`) takes
//!   `ReadValue`'s fallback arms with the ORIGINAL count -- a different
//!   arithmetic from the `int($size / $formatSize)` recount. Refused, not
//!   modelled ([`read_plan`]).
//! * a zero-count entry: `ReadValue` returns `''` (ExifTool.pm:6297) and
//!   ExifTool reports an empty value (`Unknown ()` through a hash
//!   `PrintConv`). Refused ([`read_plan`]).
//! * `$$dirInfo{EntryBased}`, `FixOffsets`, `ChangeBase`, `FixCount`,
//!   `FixedSize`, `LongBinary`, `ReadFromRAF`, `LeicaTrailer`, `IsOffset`,
//!   `MAP_FORMAT`, `OffsetPt`, `BadOffset`: per-tag / per-table keys the
//!   schema does not carry. The generator refuses the ones that change what
//!   a tag means; the rest are inert for the makernote tables slice I-2
//!   enables.
//! * `Validate` (Exif.pm:7081-7085) is Perl evaluated against the directory
//!   bytes; an edge carrying it is never walked.
//! * `Start => '$val'` on a tag WITHOUT `Flags => 'SubIFD'`: its value is
//!   read as `undef` (Exif.pm:6733), so `eval('$val')` yields a byte string
//!   and `IsInt` fails (Exif.pm:6957-6959) unless the bytes happen to spell
//!   an integer. Refused ([`pointer_values`]).
//! * `Base` expressions naming `$base` (only `MakerNotes.pm:699`): the
//!   enclosing directory's absolute base is not part of [`IfdDir`] (its
//!   `base` is the signed correction, ExifTool's `-$dataPos`), so the
//!   expression cannot be evaluated. Refused.
//! * the file-seek (`$raf`) paths: a value or sub-directory outside `data`
//!   is skipped exactly as ExifTool skips it when no `RAF` is available
//!   (Exif.pm:6616-6670, 7017-7036).
//! * the entry-count truncation ExifTool applies to a makernote directory
//!   that overruns its data (`$numEntries = int(($dirSize - 2) / 12)`,
//!   Exif.pm:6384-6388) and the `$bytesFromEnd` legality check
//!   (Exif.pm:6394-6400): both depend on `DirLen`/`DataLen`, which are the
//!   caller's buffer shape, not this directory's. [`read_ifd`] keeps the
//!   `table_ifd.rs` rule -- a directory that runs off the end of `data` is
//!   refused whole.
//! * `IfdSubdirEdge::fix_format` is writer-side data (`WriteExif.pl:1760`
//!   is its only reader in the pinned tree; `ProcessExif` never consults
//!   it) and is ignored by this walk.
//!
//! # Where this differs from `table_ifd.rs` on purpose
//!
//! The floor (values that start before the end of the directory are
//! refused) is `table_ifd.rs`'s rule, kept as the design spec asks. ExifTool
//! refuses a narrower set -- values that OVERLAP the directory
//! (Exif.pm:6549) -- and reads a value that lies entirely before it. The
//! difference only withholds, never invents.

use crate::core::TagValue;
use crate::io::ByteOrder;

use super::cond::{self, MemberValue};
use super::engine::{self, Dir, Emitted, Guard};
use super::exprs;
use super::ifd_schema::{IfdByteOrder, IfdStart, IfdSubdirEdge, IfdTable, IfdTag, RawConvEffect};
use super::runtime::{self, DecodedValue, decode_value_of};
use super::subdir::BaseExpr;
use super::{Fmt, find_ifd_table, find_table};

// ---------------------------------------------------------------------------
// The directory
// ---------------------------------------------------------------------------

/// The `%dirInfo` a `ProcessExif` call receives, reduced to what the walk
/// reads (Exif.pm:6281-6287).
#[derive(Clone, Copy, Debug)]
pub struct IfdDir<'a> {
    /// `$$dirInfo{DataPt}` -- the whole buffer, not just this directory: an
    /// entry's out-of-line value and every `SubDirectory` start are offsets
    /// into *this*.
    pub data: &'a [u8],
    /// `$$dirInfo{DirStart}`: where the 2-byte entry count sits in `data`.
    pub ifd_start: usize,
    /// The signed correction added to an entry's stored `value_offset` to
    /// land inside `data` -- ExifTool's `$valuePtr -= $dataPos`
    /// (Exif.pm:6546), i.e. `-$dataPos` after the caller's own base fixing.
    /// `None` means the caller could not establish where stored offsets
    /// point (the old `OLYMP\0` maker notes with TIFF-relative offsets and
    /// only the maker-note slice in hand): every out-of-line value is then
    /// left unread rather than decoded from whatever the raw offset lands
    /// on. Inline values (four bytes or fewer, Exif.pm:6504) never use it.
    pub base: Option<i64>,
    /// `GetByteOrder()` for this directory (Exif.pm:7078 sets it per
    /// sub-directory).
    pub byte_order: ByteOrder,
    /// `$$dirInfo{DirName}` (Exif.pm:6286): the name ExifTool gives this
    /// directory. It reaches a reported tag only through a `SET_GROUP1`
    /// table (Exif.pm:7183 `SetGroup($tagKey, $dirName)`); every other
    /// table's family-1 group comes from the tag or the table itself. A
    /// nested directory's name is derived at the edge (Exif.pm:7053,
    /// 7073-7077, 7087-7088), see [`subdir_name`].
    pub group1: Option<&'static str>,
}

// ---------------------------------------------------------------------------
// Reading the directory -- ported from table_ifd.rs::{RawEntry, read_ifd}
// ---------------------------------------------------------------------------

/// One raw 12-byte IFD entry (Exif.pm:6458-6461, 6484).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IfdEntry {
    pub tag_id: u16,
    /// The declared TIFF type code (`$format`), unvalidated: see
    /// [`entry_type`] for which codes `ProcessExif` accepts.
    pub field_type: u16,
    pub count: u32,
    /// The entry's 4-byte value field read as an `int32u` -- an offset when
    /// the value does not fit inline (Exif.pm:6510).
    pub value_offset: u32,
    /// Byte offset of the entry's 4-byte value field inside `data`
    /// (ExifTool's initial `$valuePtr = $entry + 8`, Exif.pm:6484).
    pub value_field_pos: usize,
}

/// The largest entry count [`read_ifd`] accepts. ExifTool has no such bound
/// (it reads whatever the `int16u` says and lets each entry fail on its
/// own); `table_ifd.rs` added it as the guard that keeps a random buffer
/// from being walked as a directory, and no real maker-note IFD in the
/// corpus comes within a factor of two of it.
pub const MAX_IFD_ENTRIES: usize = 512;

/// Parse an IFD header at `ifd_start` and return its entries.
///
/// Ported from `table_ifd.rs::read_ifd` (its rules, in its order): `None`
/// when the entry count does not fit in `data`, when it is 0 or above
/// [`MAX_IFD_ENTRIES`], or when the entry array runs off the end of `data`.
/// A zero count is legal to ExifTool (Exif.pm:6346 reads it and the loop at
/// Exif.pm:6454 simply does not run), so refusing it changes no observable
/// output; the upper bound and the whole-array requirement are stricter than
/// ExifTool's (see the module doc) in the withholding direction only.
#[must_use]
pub fn read_ifd(data: &[u8], ifd_start: usize, order: ByteOrder) -> Option<Vec<IfdEntry>> {
    let count = usize::from(get16(data, ifd_start, order)?);
    if count == 0 || count > MAX_IFD_ENTRIES {
        return None;
    }
    let end = ifd_start
        .checked_add(2)?
        .checked_add(count.checked_mul(12)?)?;
    if end > data.len() {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        // Exif.pm:6458-6461, 6484.
        let off = ifd_start + 2 + i * 12;
        out.push(IfdEntry {
            tag_id: get16(data, off, order)?,
            field_type: get16(data, off + 2, order)?,
            count: get32(data, off + 4, order)?,
            value_offset: get32(data, off + 8, order)?,
            value_field_pos: off + 8,
        });
    }
    Some(out)
}

/// `$dirEnd + 4`: the first byte after the directory's next-IFD pointer
/// (Exif.pm:6347-6348 `$dirSize = 2 + 12 * $numEntries; $dirEnd = $dirStart +
/// $dirSize`, then the 4-byte link ExifTool reads at `$dirEnd`, Exif.pm:6425).
/// `table_ifd.rs`'s floor: an out-of-line value that starts before it is
/// refused.
fn directory_floor(ifd_start: usize, entries: usize) -> usize {
    ifd_start + 2 + 12 * entries + 4
}

fn get16(data: &[u8], at: usize, order: ByteOrder) -> Option<u16> {
    let bytes = data.get(at..at.checked_add(2)?)?;
    Some(match order {
        ByteOrder::Big => u16::from_be_bytes([bytes[0], bytes[1]]),
        ByteOrder::Little => u16::from_le_bytes([bytes[0], bytes[1]]),
    })
}

fn get32(data: &[u8], at: usize, order: ByteOrder) -> Option<u32> {
    let bytes = data.get(at..at.checked_add(4)?)?;
    Some(match order {
        ByteOrder::Big => u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        ByteOrder::Little => u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    })
}

// ---------------------------------------------------------------------------
// Entry types -- Exif.pm:82-93 (@formatSize, @formatName), 6463, 6682
// ---------------------------------------------------------------------------

/// How one element of an entry is read, after ExifTool's own remapping of
/// the TIFF type: `%readValueProc` (ExifTool.pm:6243-6268) names a reader
/// for every numeric spelling (`ifd` reads as `Get32u`), and the three
/// spellings with no reader -- `string`, `undef`, `utf8` -- take
/// `ReadValue`'s `substr` arm (ExifTool.pm:6307-6311), differing only in
/// whether the run is NUL-truncated (`string`) and whether it is a byte
/// string or text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// A numeric element decoded by [`decode_value_of`].
    Num(Fmt),
    /// `string`: one value spanning every byte, truncated at the first NUL
    /// (ExifTool.pm:6311).
    Str,
    /// `undef`: one value spanning every byte, verbatim.
    Undef,
    /// `utf8` (Exif 3.0, code 129): the byte run, then
    /// `$et->Decode($val, 'UTF8')` (Exif.pm:6786-6787), which with the
    /// default `Charset` is the identity (ExifTool.pm:6348 `if ($from ne
    /// $to ...)`). Not NUL-truncated -- only `string` is.
    Utf8,
}

/// One accepted entry type: ExifTool's `$formatName[$format]` and
/// `$formatSize[$format]` (Exif.pm:82-93), plus how it is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EntryType {
    /// The spelling a `Condition` sees as `$format` (Exif.pm:6720 passes
    /// `$formatName[$format]`, the ORIGINAL name, to `GetTagInfo`).
    name: &'static str,
    /// `$formatSize[$format]`.
    size: usize,
    kind: Kind,
}

/// `@formatName` / `@formatSize` (Exif.pm:82-93) for every code
/// `ProcessExif` names, whether or not it accepts it -- see
/// [`accepted_type`] for the acceptance rule.
fn entry_type(code: u16) -> Option<EntryType> {
    let (name, size, kind) = match code {
        1 => ("int8u", 1, Kind::Num(Fmt::Int8u)),
        2 => ("string", 1, Kind::Str),
        3 => ("int16u", 2, Kind::Num(Fmt::Int16u)),
        4 => ("int32u", 4, Kind::Num(Fmt::Int32u)),
        5 => ("rational64u", 8, Kind::Num(Fmt::Rational64u)),
        6 => ("int8s", 1, Kind::Num(Fmt::Int8s)),
        7 => ("undef", 1, Kind::Undef),
        8 => ("int16s", 2, Kind::Num(Fmt::Int16s)),
        9 => ("int32s", 4, Kind::Num(Fmt::Int32s)),
        10 => ("rational64s", 8, Kind::Num(Fmt::Rational64s)),
        11 => ("float", 4, Kind::Num(Fmt::Float)),
        12 => ("double", 8, Kind::Num(Fmt::Double)),
        // `ifd` reads as `Get32u` (ExifTool.pm:6266).
        13 => ("ifd", 4, Kind::Num(Fmt::Int32u)),
        // `unicode` / `complex`: named and sized (Exif.pm:82, 89) but
        // "not yet properly supported by ExifTool" (Exif.pm:117-120) and
        // rejected by the code range at Exif.pm:6463. Reading them here would
        // require a decoder ExifTool itself does not have; the acceptance
        // rule keeps them out, and they are listed so the name/size census
        // stays complete.
        14 => ("unicode", 2, Kind::Undef),
        15 => ("complex", 8, Kind::Undef),
        16 => ("int64u", 8, Kind::Num(Fmt::Int64u)),
        17 => ("int64s", 8, Kind::Num(Fmt::Int64s)),
        // `ifd64` reads as `Get64u` (ExifTool.pm:6267).
        18 => ("ifd64", 8, Kind::Num(Fmt::Int64u)),
        129 => ("utf8", 1, Kind::Utf8),
        _ => return None,
    };
    Some(EntryType { name, size, kind })
}

/// Exif.pm:6463 -- which type codes `ProcessExif` reads:
///
/// ```perl
/// if (($format < 1 or $format > 13) and $format != 129 and
///     not ($format == 16 and $$et{Make} eq 'Apple' and $inMakerNotes))
/// ```
///
/// So 14-15 and 17-18 are "Bad format" despite having names, and the
/// BigTIFF `int64u` (16) is accepted only inside Apple maker notes (their
/// ProRaw DNGs write `LivePhotoVideoIndex` in it). `$$et{Make}` is the
/// `Make` data member the caller seeds (slice I-2); with no member set the
/// code is refused, exactly as ExifTool refuses it for a non-Apple file.
fn accepted_type(code: u16, in_maker_notes: bool, ctx: &cond::Ctx) -> Option<EntryType> {
    let accepted = (1..=13).contains(&code)
        || code == 129
        || (code == 16 && in_maker_notes && member_str(ctx, "Make") == Some("Apple"));
    if accepted { entry_type(code) } else { None }
}

fn member_str<'c>(ctx: &'c cond::Ctx, member: &str) -> Option<&'c str> {
    match ctx.members.get(member) {
        Some(MemberValue::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// Exif.pm:6475 -- `next if $index or $$et{Model} =~ /^ILCE/;` (Sony ILCE
/// bodies write an empty first entry, so a bad first entry is not evidence
/// of a corrupted IFD for them).
fn model_is_ilce(ctx: &cond::Ctx) -> bool {
    member_str(ctx, "Model").is_some_and(|m| m.starts_with("ILCE"))
}

// ---------------------------------------------------------------------------
// Locating an entry's value -- Exif.pm:6502-6680, table_ifd.rs:600-645
// ---------------------------------------------------------------------------

/// An entry whose `$size` bytes have been found inside `data`.
#[derive(Clone, Copy, Debug)]
struct Located<'d> {
    /// `$valuePtr` relative to `data` after Exif.pm:6484 (inline) or
    /// Exif.pm:6510/6546 (out of line): the absolute position of the value
    /// bytes, which `SubDirectory` `Start` expressions call `$valuePtr`.
    value_pos: usize,
    /// The `$size = $count * $formatSize[$format]` bytes (Exif.pm:6502).
    bytes: &'d [u8],
    ty: EntryType,
}

/// Why an entry's value could not be located.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refusal {
    /// ExifTool would have issued a warning here and `++$warnCount`
    /// (Exif.pm:6507, 6661, 6676): the directory's "too many warnings" budget
    /// (Exif.pm:6455) moves.
    Warned,
    /// No ExifTool analogue counts: the caller's `base: None` says the block
    /// cannot be located at all.
    Silent,
}

/// Exif.pm:6502-6680 for one entry, with `table_ifd.rs`'s floor.
fn locate<'d>(
    dir: &IfdDir<'d>,
    entry: &IfdEntry,
    ty: EntryType,
    floor: usize,
) -> Result<Located<'d>, Refusal> {
    // Exif.pm:6502.
    let size = u64::from(entry.count) * (ty.size as u64);
    // Exif.pm:6505-6509.
    if size > 0x7fff_ffff {
        return Err(Refusal::Warned);
    }
    let size = size as usize;
    if size <= 4 {
        // Exif.pm:6484: the value sits in the entry's own value field.
        // `read_ifd` proved the entry is inside `data`.
        let bytes = dir
            .data
            .get(entry.value_field_pos..entry.value_field_pos + size)
            .ok_or(Refusal::Silent)?;
        return Ok(Located {
            value_pos: entry.value_field_pos,
            bytes,
            ty,
        });
    }
    // Exif.pm:6510, 6546: an offset, corrected into `data`.
    let Some(base) = dir.base else {
        return Err(Refusal::Silent);
    };
    let start = i64::from(entry.value_offset)
        .checked_add(base)
        .ok_or(Refusal::Warned)?;
    // Exif.pm:6551 `$valuePtr < 0` -- "Bad offset".
    let Ok(start) = usize::try_from(start) else {
        return Err(Refusal::Warned);
    };
    // table_ifd.rs's floor (the module doc explains how it relates to
    // Exif.pm:6549's overlap rule): a value inside the directory is
    // "Suspicious" (Exif.pm:6673-6678) and skipped.
    if start < floor {
        return Err(Refusal::Warned);
    }
    // Exif.pm:6551 `$valuePtr+$size > $dataLen` -- "Bad offset".
    let end = start.checked_add(size).ok_or(Refusal::Warned)?;
    let bytes = dir.data.get(start..end).ok_or(Refusal::Warned)?;
    Ok(Located {
        value_pos: start,
        bytes,
        ty,
    })
}

// ---------------------------------------------------------------------------
// Reading the value -- Exif.pm:6682, 6729-6745, 6782; ExifTool.pm:6286-6332
// ---------------------------------------------------------------------------

/// What `ReadValue` will be asked for: the format and element count after
/// Exif.pm:6682 (a single `undef` byte reads as `int8u`) and after the
/// tag's `Format` override recomputed `$count` (Exif.pm:6735-6744).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReadPlan {
    kind: Kind,
    count: usize,
}

/// Exif.pm:6682 + 6729-6745: which format and count the value is read with.
///
/// `override_fmt` is the tag's `Format` (or `Some(Fmt::Undef(0))` for a
/// non-SubIFD `SubDirectory` tag with none, Exif.pm:6733). An override is
/// honoured only when ExifTool would map it through `%formatNumber`
/// (Exif.pm:6737), because only then does the count become `int($size /
/// $formatSize[$format])`; the other spellings take `ReadValue`'s fallback
/// arms with the original count (module doc) and are refused here. `None`
/// also for a zero element count, whose `''` value this walk does not
/// report.
fn read_plan(located: &Located<'_>, override_fmt: Option<Fmt>) -> Option<ReadPlan> {
    let size = located.bytes.len();
    let mut kind = located.ty.kind;
    let mut count = size / located.ty.size;
    // Exif.pm:6682 -- "treat single unknown byte as int8u".
    if kind == Kind::Undef && count == 1 {
        kind = Kind::Num(Fmt::Int8u);
    }
    if let Some(fmt) = override_fmt {
        // Exif.pm:6736-6744. `%formatNumber` (Exif.pm:96-116) names exactly
        // these spellings among the ones the schema can spell; `binary` is
        // `undef`'s alias and `ifd`/`ifd64`/`unicode`/`complex`/`utf8` have
        // no `Fmt`.
        let (new_kind, new_size) = match fmt {
            Fmt::Int8u
            | Fmt::Int8s
            | Fmt::Int16u
            | Fmt::Int16s
            | Fmt::Int32u
            | Fmt::Int32s
            | Fmt::Int64u
            | Fmt::Int64s
            | Fmt::Float
            | Fmt::Double
            | Fmt::Rational64u
            | Fmt::Rational64s => (Kind::Num(fmt), usize::try_from(fmt.size()).ok()?),
            Fmt::Str(0) => (Kind::Str, 1),
            Fmt::Undef(0) => (Kind::Undef, 1),
            _ => return None,
        };
        kind = new_kind;
        // `$count = int($size / $formatSize[$format])` -- the same result
        // when the number does not change (`$size` is `count * size`), so it
        // is safe to recompute unconditionally. Note the `int8u` patch above
        // is undone by an explicit `Format => 'undef'`: ExifTool's
        // `$formatStr = $readFormat` (Exif.pm:6736) runs after Exif.pm:6682.
        count = size / new_size;
    }
    // ExifTool.pm:6296-6297: `return '' if defined $count` -- a zero count is
    // not a refusal, it is the empty value, which `FoundTag` records like any
    // other (OlympusXZ-1.jpg RawDevelopment2 0x0108, a 0-byte `int16s`,
    // prints `""` and overwrites the RawDevelopment copy's `0`). `decode_plan`
    // turns the zero-count plan into that empty string.
    Some(ReadPlan { kind, count })
}

/// `ReadValue` (ExifTool.pm:6286-6332) over the located bytes with the
/// plan's format and count. Never shortens: `$readSize` is the entry's own
/// `$size` (Exif.pm:6503), so `$len * $count <= $size` by construction.
fn decode_plan(located: &Located<'_>, plan: ReadPlan, order: ByteOrder) -> Option<DecodedValue> {
    let bytes = located.bytes;
    if plan.count == 0 {
        // ExifTool.pm:6296-6297, whatever the format.
        return Some(DecodedValue::String(String::new()));
    }
    match plan.kind {
        Kind::Num(fmt) => {
            let elem = usize::try_from(fmt.size()).ok()?;
            if plan.count == 1 {
                return decode_value_of(bytes.get(..elem)?, fmt, order);
            }
            let values = bytes
                .get(..plan.count.checked_mul(elem)?)?
                .chunks_exact(elem)
                .map(|chunk| decode_value_of(chunk, fmt, order))
                .collect::<Option<Vec<_>>>()?;
            Some(DecodedValue::Array(values))
        }
        // ExifTool.pm:6308-6311: one value spanning every byte, NUL-truncated
        // for `string`. `decode_value_of`'s `Str` arm is that rule, and both
        // it and the `utf8` arm print malformed UTF-8 the way the exiftool
        // application does: `XMP::FixUTF8`'s `?` per bad byte
        // (`runtime::fix_utf8`), not a refusal and not a lossy re-encode.
        Kind::Str => decode_value_of(bytes, Fmt::Str(u32::try_from(bytes.len()).ok()?), order),
        Kind::Undef => Some(DecodedValue::Undefined(bytes.to_vec())),
        Kind::Utf8 => runtime::fix_utf8(bytes).map(DecodedValue::String),
    }
}

/// `%intFormat` (Exif.pm:125-136): the formats a `SubIFD` pointer may be
/// read in (Exif.pm:6747, else "Wrong format" and the entry is skipped).
fn is_int_format(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Num(
            Fmt::Int8u
                | Fmt::Int8s
                | Fmt::Int16u
                | Fmt::Int16s
                | Fmt::Int32u
                | Fmt::Int32s
                | Fmt::Int64u
                | Fmt::Int64s
        )
    )
}

// ---------------------------------------------------------------------------
// Resolving the tag -- Exif.pm:6485, 6717-6720; ExifTool.pm:9155-9217
// ---------------------------------------------------------------------------

/// The table entry ExifTool's `GetTagInfo` picks for one IFD entry.
struct Resolved {
    tag: &'static IfdTag,
    /// True when the tag came from a `_variants` group whose `Condition`
    /// `cond::first_match_ifd` has resolved, so `Omitted::condition` on it
    /// is not an outstanding refusal (as `engine::Entry::condition_resolved`).
    condition_resolved: bool,
}

/// Exif.pm:6485 / 6717-6720: `GetTagInfo` with `$$valPt` (the first 128
/// bytes of the value), `$format` (the entry's declared name) and `$count`
/// in scope for a `Condition`.
fn resolve(
    table: &'static IfdTable,
    entry: &IfdEntry,
    located: &Located<'_>,
    ctx: &mut cond::Ctx,
) -> Option<Resolved> {
    if let Some(tag) = table.tag(entry.tag_id) {
        return Some(Resolved {
            tag,
            condition_resolved: false,
        });
    }
    let group = table.variant_group(entry.tag_id)?;
    // Exif.pm:6717: `substr($$valueDataPt, $valuePtr, $readSize < 128 ? $readSize : 128)`.
    let val_pt = &located.bytes[..located.bytes.len().min(128)];
    let mut entry_ctx = cond::Ctx {
        members: &mut *ctx.members,
        val_pt: Some(val_pt),
        format: Some(located.ty.name),
        count: Some(i64::from(entry.count)),
    };
    // ExifTool.pm:9164-9188: first match wins; `SetMember` side effects of
    // every alternative visited fire, winner or not (cond.rs).
    let tag = cond::first_match_ifd(group.alternatives, &mut entry_ctx)?;
    Some(Resolved {
        tag,
        condition_resolved: true,
    })
}

// ---------------------------------------------------------------------------
// The walk
// ---------------------------------------------------------------------------

/// Walk one `ProcessExif` table at `dir` and everything its `SubDirectory`
/// edges reach, appending every tag ExifTool would report to `out`.
///
/// Like [`engine::process_binary_data`], this does not check the root
/// table's own gates -- the call site does (`table.enabled()`), which is
/// what `reachability.py`'s census counts. Every table reached through an
/// edge IS checked (`descend`), so an edge never enables its target by the
/// back door. `ctx` carries `$$self{...}` data members across the whole walk
/// (they are per-file state in ExifTool); the caller seeds `Make`/`Model`.
pub fn process_exif(
    table: &'static IfdTable,
    dir: IfdDir<'_>,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
) {
    let mut guard = Guard::new();
    // ExifTool.pm:9065-9072: `ProcessDirectory` records the root directory's
    // address too, which is what stops a table that points at itself.
    if !guard.admit(ifd_addr(dir.ifd_start), table_key(table), false) {
        return;
    }
    walk(table, dir, ctx, &mut guard, out);
}

/// The `$$self{PROCESSED}` key for an IFD walked at `start`.
///
/// ExifTool.pm:9066 keys on `DirStart + DataPos + Base (+ $$self{BASE})`.
/// `DataPos + Base` is invariant across a `SubDirectory` `Base` override
/// (Exif.pm:7040 `$subdirDataPos += $base - $subdirBase`), so within one
/// walk over one buffer two directories share an address exactly when they
/// share a data-relative start -- which is therefore the key, independent
/// of whatever correction each level carries.
fn ifd_addr(start: usize) -> i64 {
    i64::try_from(start).unwrap_or(i64::MAX)
}

/// The key `engine::descend` would compute for the same `ProcessBinaryData`
/// table reached from inside the binary engine with the [`Dir`] built in
/// [`descend`] (`start + data_pos + base` with `data_pos` the negated
/// correction and `base` 0), so a binary table reached either way keys the
/// same.
fn binary_addr(base: Option<i64>, start: usize) -> i64 {
    i64::try_from(start).unwrap_or(i64::MAX) - base.unwrap_or(0)
}

fn table_key<T>(table: &'static T) -> usize {
    std::ptr::from_ref(table) as usize
}

fn walk(
    table: &'static IfdTable,
    dir: IfdDir<'_>,
    ctx: &mut cond::Ctx,
    guard: &mut Guard,
    out: &mut Vec<Emitted>,
) {
    // Exif.pm:6344-6358.
    let Some(entries) = read_ifd(dir.data, dir.ifd_start, dir.byte_order) else {
        return;
    };
    let floor = directory_floor(dir.ifd_start, entries.len());
    // Exif.pm:6294.
    let in_maker_notes = table.group0 == "MakerNotes";
    // Exif.pm:6446-6447: "make sure that Compression and SubfileType are
    // defined for this IFD (for Condition's)".
    ctx.members
        .insert("Compression", MemberValue::Str(String::new()));
    ctx.members
        .insert("SubfileType", MemberValue::Str(String::new()));

    let mut warn_count = 0u32;
    for (index, entry) in entries.iter().enumerate() {
        // Exif.pm:6455-6457.
        if warn_count > 10 {
            return;
        }
        // Exif.pm:6463-6478.
        let Some(ty) = accepted_type(entry.field_type, in_maker_notes, ctx) else {
            // Exif.pm:6470-6473: "warn unless the IFD was just padded with
            // zeros" -- a zero code does not spend the warning budget.
            if entry.field_type != 0 {
                warn_count += 1;
            }
            // Exif.pm:6474-6477: "assume corrupted IFD if this is our first
            // entry (except Sony ILCE which have an empty first entry)".
            if index == 0 && !model_is_ilce(ctx) {
                return;
            }
            continue;
        };
        // Exif.pm:6502-6680.
        let located = match locate(&dir, entry, ty, floor) {
            Ok(located) => located,
            Err(Refusal::Warned) => {
                warn_count += 1;
                continue;
            }
            Err(Refusal::Silent) => continue,
        };
        // Exif.pm:6485, 6717-6720. A `$bad` entry never gets this far, which
        // is also ExifTool's order (Exif.pm:6713-6714 drops the tag before
        // the value-scoped `GetTagInfo`).
        let Some(Resolved {
            tag,
            condition_resolved,
        }) = resolve(table, entry, &located, ctx)
        else {
            continue;
        };
        // ExifTool.pm:9180-9186: an `Unknown` tag is not returned unless
        // `-u` (not in v1).
        if tag.flags.unknown {
            continue;
        }
        let mut omitted = tag.omitted;
        if condition_resolved {
            omitted.condition = false;
        }
        // A plain tag with a `Condition` the generator could not compile:
        // ExifTool decides here whether the tag applies at all, so neither
        // its value nor its sub-directory can be honoured.
        if omitted.condition {
            continue;
        }
        // Exif.pm:6919-7153 -- a SubDirectory's value is never reported
        // (Exif.pm:7103-7104 `next unless $doMaker ...`, and the `MakerNotes`
        // option is off), so the edge is the whole of the tag.
        if let Some(edge) = &tag.subdir {
            descend(table, tag, edge, &located, &dir, ctx, guard, out);
            continue;
        }
        // Exif.pm:6729-6745.
        let Some(plan) = read_plan(&located, tag.format) else {
            continue;
        };
        // Exif.pm:6763-6773: "Ignoring ... with excessive count" (the
        // warning is minor, so the default options take the `next`).
        if plan.count > 100_000
            && !matches!(plan.kind, Kind::Str | Kind::Undef)
            && !(tag.name == "TransferFunction" && plan.count == 196_608)
        {
            continue;
        }
        // Exif.pm:6782, 6915.
        let Some(raw) = decode_plan(&located, plan, dir.byte_order) else {
            continue;
        };
        // ExifTool.pm:6107-6120: the rational `ReadValue` hands on is already
        // `RoundFloat($n / $d, 10)`, so everything below -- the `RawConv`
        // member, `ValueConv`, `PrintConv`, the unconverted report -- sees
        // that number, not the exact quotient. See `round_rationals`.
        let raw = round_rationals(raw);
        // ExifTool.pm:9484-9505: `FoundTag` runs `RawConv` before any
        // conversion; the one shape carried as data stores the raw value
        // and returns it unchanged (the assignment's value).
        if let Some(RawConvEffect::SetMember { member }) = tag.raw_conv {
            let Some(value) = member_value(&raw) else {
                continue;
            };
            ctx.members.insert(member, value);
        }
        // A `Binary` tag with a refused `PrintConv` is withheld here too,
        // although ExifTool never runs a PrintConv on a scalar-ref value
        // (ExifTool.pm:3533): over-refusing is the safe direction.
        if omitted.any() {
            continue;
        }
        let value = if tag.flags.binary && tag.value_conv.is_none() {
            // ExifTool.pm:3535-3539: a `Binary` tag with no `ValueConv` gets
            // `\$val`, and the CLI prints the placeholder with `length($$val)`
            // (exiftool:3983-3988). A `ValueConv`, when present, runs
            // normally and the result is an ordinary scalar.
            let Some(len) = perl_length(&raw) else {
                continue;
            };
            TagValue::String(format!(
                "(Binary data {len} bytes, use -b option to extract)"
            ))
        } else {
            let Some(converted) = runtime::apply_value_conv(tag.value_conv, &raw) else {
                // A verified ValueConv may faithfully return Perl undef:
                // tag suppression, not permission to emit the raw value.
                continue;
            };
            match runtime::render(tag.print_conv, &converted) {
                Some(rendered) => TagValue::String(rendered),
                // ExifTool.pm:6330: the entry's value is ONE space-joined
                // scalar unless the tag is a `List`.
                None if tag.flags.list => runtime::to_tag_value(&converted),
                None => ifd_exiftool_value(&converted),
            }
        };
        let Some(group1) = group1_of(table, tag, &dir) else {
            continue;
        };
        out.push(Emitted {
            module: table.module,
            table: table.table,
            // ExifTool.pm:9236-9244 (`AddTagToTable`) / 3832-3835: the tag's
            // own family else the table's.
            group0: tag.groups.g0.unwrap_or(table.group0),
            group1,
            group2: tag.groups.g2.unwrap_or(table.group2),
            name: tag.name,
            value,
            low_priority: effective_priority(table, tag) == Some(0),
            avoid: tag.flags.avoid,
        });
    }
}

/// ExifTool.pm:9469-9473:
///
/// ```perl
/// my $priority = $$tagInfo{Priority};
/// unless (defined $priority) {
///     $priority = $$tbl{PRIORITY};
///     $priority = 0 if not defined $priority and $$tagInfo{Avoid};
/// }
/// ```
///
/// The tag's own `Priority` wins outright -- a `Priority => 1` tag in a
/// `PRIORITY => 0` table is NOT low priority -- and `Avoid` only supplies the
/// default when neither declares one.
fn effective_priority(table: &IfdTable, tag: &IfdTag) -> Option<i64> {
    tag.flags
        .priority
        .or(table.priority)
        .or(if tag.flags.avoid { Some(0) } else { None })
}

/// The family-1 group `GetGroup` (ExifTool.pm:3810-3860) reports:
///
/// * a `SET_GROUP1` table sets it to the DIRECTORY NAME for every tag it
///   reports (Exif.pm:7183 `SetGroup($tagKey, $dirName) if
///   $$tagTablePtr{SET_GROUP1}`; the key's value is a flag -- Exif.pm:416
///   declares `SET_GROUP1 => 1` -- so [`IfdTable::set_group1`]'s payload is
///   never a group name). That `TAG_EXTRA{G1}` beats the tag's own
///   `Groups{1}` (ExifTool.pm:3860). A `SET_GROUP1` table walked with no
///   directory name cannot be reported faithfully and is withheld;
/// * otherwise the tag's `Groups{1}`, else the table's effective group 1
///   (ExifTool.pm:3832-3835 fills the tag's groups from the table).
fn group1_of(table: &IfdTable, tag: &IfdTag, dir: &IfdDir<'_>) -> Option<&'static str> {
    if table.set_group1.is_some() {
        return dir.group1;
    }
    Some(tag.groups.g1.unwrap_or(table.group1))
}

/// `$$self{Member} = $val` (ExifTool.pm:9497-9500 evaluates the `RawConv`
/// with `$val` bound to the value `ReadValue` returned). `Num` for an
/// integer, else the Perl string of the value ([`DecodedValue::perl_string`];
/// a multi-count entry is the space-joined string ExifTool.pm:6330 built).
/// A rational arrives here already rounded ([`round_rationals`]), so the
/// member holds the `RoundFloat($n / $d, 10)` digits ExifTool's `RawConv`
/// would have stored -- `0.3333333333` for 1/3, never `0.333333333333333`.
/// `None` when the value has no faithful Perl string (an `undef` run that is
/// not UTF-8): the member is left unset and the tag withheld, so a later
/// `Condition` sees "undefined" rather than an invented value.
fn member_value(raw: &DecodedValue) -> Option<MemberValue> {
    match raw {
        DecodedValue::Integer(n) => Some(MemberValue::Num(*n)),
        DecodedValue::Undefined(bytes) => {
            String::from_utf8(bytes.clone()).ok().map(MemberValue::Str)
        }
        DecodedValue::Array(values) => values
            .iter()
            .map(ifd_perl_string)
            .collect::<Option<Vec<_>>>()
            .map(|parts| MemberValue::Str(parts.join(" "))),
        other => ifd_perl_string(other).map(MemberValue::Str),
    }
}

/// The string Perl holds for one IFD scalar once `ReadValue` has produced it
/// -- the element `join(' ', @vals)` (ExifTool.pm:6330) concatenates when an
/// entry carries more than one value, and what `$$self{X} = $val` stores.
///
/// [`DecodedValue::perl_string`] (shared with the binary engine) refuses a
/// rational because its digit string depends on the rational's WIDTH, which
/// a `DecodedValue` does not carry. Here that is known: an IFD entry's
/// rational is always TIFF type 5 or 10, i.e. what `GetRational64u`/
/// `GetRational64s` return -- `RoundFloat($n / $d, 10)`, or `inf` / `undef`
/// for a zero denominator (ExifTool.pm:6107-6120, 5960-5964) -- which
/// [`runtime::perl_rational64`] renders. From `walk` only the zero-
/// denominator pairs still reach these arms: [`round_rationals`] has turned
/// every other rational into the `Float` that string numifies to, which the
/// `Float` arm prints back as the same digits. The rational arms stay so the
/// rule is total over the type (and so the `inf`/`undef` spelling has one
/// home). An `undef` byte run is not a Perl number and a nested array cannot
/// occur as an element (callers join the outer array themselves): both are
/// `None`, keeping this a scalar rule.
fn ifd_perl_string(value: &DecodedValue) -> Option<String> {
    match value {
        DecodedValue::UnsignedRational(numerator, denominator) => Some(runtime::perl_rational64(
            f64::from(*numerator),
            f64::from(*denominator),
        )),
        DecodedValue::SignedRational(numerator, denominator) => Some(runtime::perl_rational64(
            f64::from(*numerator),
            f64::from(*denominator),
        )),
        DecodedValue::Undefined(_) | DecodedValue::Array(_) => None,
        other => other.perl_string(),
    }
}

/// The `ReadValue` step applied to every decoded entry before anything
/// consumes it: a rational element is neither the pair nor the exact
/// quotient but what `GetRational64u`/`GetRational64s` return --
/// `RoundFloat($n / $d, 10)` (ExifTool.pm:6112, 6119), and `RoundFloat` is
/// literally `sprintf("%.10g", $val)` (ExifTool.pm:5960-5964) -- a STRING
/// that every later consumer numifies: the `RawConv` `$$self{X} = $val`
/// (ExifTool.pm:9497-9500) stores those digits, a `ValueConv`/`PrintConv`
/// expression computes on that number (ExifTool.pm:3530-3664), a hash
/// `PrintConv` keys `$$conv{$val}` by it (ExifTool.pm:3616), `"$val mm"`
/// interpolates it, and an unconverted tag reports it. Handing on the exact
/// quotient instead printed `1.73382372803657e-07 mm` for Olympus
/// `FocalPlaneDiagonal` 256/1476505344 where the pinned 13.59 oracle prints
/// `1.733823728e-07 mm` (5 corpus files under `scripts/compare_file.py`;
/// the census is in `olympus/tables.rs` above `ENGINE_MISRENDERS`).
///
/// So each nonzero-denominator rational becomes the `Float`
/// [`exprs::round_float`]`(n / d, 10)`: the double Perl parses from that
/// string, whose `%.15g` is the string again (see there), so it prints the
/// same digits wherever `perl_num` runs. Element-wise inside a fixed-count
/// entry, as `ReadValue` rounds each element before joining (ExifTool.pm:
/// 6312-6321). An integral quotient is still the integer key the integer-
/// keyed hash arms look up (`DecodedValue::integer` reads an integral
/// `Float`, so `$$conv{2}` hits for 2/1). A zero denominator returns
/// `inf`/`undef` BEFORE `RoundFloat` (ExifTool.pm:6111, 6118) and is left as
/// the pair for [`ifd_perl_string`]/[`runtime::to_tag_value`] to spell. The
/// 64-bit width is this engine's to assume: an entry's rational is TIFF type
/// 5 or 10; the binary engine's `rational32u` (seven digits, ExifTool.pm:
/// 6100-6106) never comes through here.
fn round_rationals(raw: DecodedValue) -> DecodedValue {
    match raw {
        DecodedValue::UnsignedRational(numerator, denominator) if denominator != 0 => {
            DecodedValue::Float(exprs::round_float(
                f64::from(numerator) / f64::from(denominator),
                10,
            ))
        }
        DecodedValue::SignedRational(numerator, denominator) if denominator != 0 => {
            DecodedValue::Float(exprs::round_float(
                f64::from(numerator) / f64::from(denominator),
                10,
            ))
        }
        DecodedValue::Array(values) => {
            DecodedValue::Array(values.into_iter().map(round_rationals).collect())
        }
        other => other,
    }
}

/// The unconverted value in the form ExifTool reports an IFD entry: a
/// fixed-count entry is ONE space-joined string (ExifTool.pm:6330) with each
/// element rendered by [`ifd_perl_string`] (so a `rational64u[3]` GPS
/// coordinate reads `"41 24.2 0"` exactly as `-j` prints it); everything
/// else is [`runtime::to_tag_value`]. An array whose elements have no Perl
/// string (an `undef` run inside a list) keeps the [`TagValue::Array`] form
/// rather than print text Perl would not.
fn ifd_exiftool_value(value: &DecodedValue) -> TagValue {
    match value {
        DecodedValue::Array(values) => match values
            .iter()
            .map(ifd_perl_string)
            .collect::<Option<Vec<_>>>()
        {
            Some(parts) => TagValue::String(parts.join(" ")),
            None => runtime::to_tag_value(value),
        },
        other => runtime::to_tag_value(other),
    }
}

/// `length($$val)` for the `(Binary data N bytes, ...)` placeholder
/// (exiftool:3987): the byte length of the Perl scalar `ReadValue` returned
/// -- the run itself for `undef`, the NUL-truncated run for `string`, and the
/// space-joined digits for a numeric entry.
fn perl_length(raw: &DecodedValue) -> Option<usize> {
    match raw {
        DecodedValue::Undefined(bytes) => Some(bytes.len()),
        DecodedValue::String(s) => Some(s.len()),
        DecodedValue::Array(values) => values
            .iter()
            .map(ifd_perl_string)
            .collect::<Option<Vec<_>>>()
            .map(|parts| parts.join(" ").len()),
        other => ifd_perl_string(other).map(|s| s.len()),
    }
}

// ---------------------------------------------------------------------------
// SubDirectory -- Exif.pm:6919-7102
// ---------------------------------------------------------------------------

/// The `$val`s a `Flags => 'SubIFD'` edge walks (Exif.pm:6929-6938,
/// 6946-6968, 7100-7101):
///
/// * the pointer is read with the tag's `Format` if any, else the entry's
///   own type, and must be an integer format (Exif.pm:6747, else "Wrong
///   format" and the entry is skipped);
/// * with `MaxSubdirs`, `@values = split ' ', $val` and at most that many
///   are walked (Exif.pm:6929-6937), one per loop iteration;
/// * without it, `$val` is used whole: a multi-count entry's space-joined
///   string fails `IsInt` (Exif.pm:6957-6959, "Bad value") and nothing is
///   walked. (`'$val + n'` would numify the leading integer with a Perl
///   warning -- an accident this port refuses rather than reproduces.)
///
/// A `Start => '$val'` edge WITHOUT `SubIFD` is refused outright (module
/// doc): its value is `undef` bytes (Exif.pm:6733) and `IsInt` fails.
fn pointer_values(
    tag: &IfdTag,
    edge: &IfdSubdirEdge,
    located: &Located<'_>,
    order: ByteOrder,
) -> Option<Vec<i64>> {
    if !edge.sub_ifd {
        return None;
    }
    let plan = read_plan(located, tag.format)?;
    if !is_int_format(plan.kind) {
        return None;
    }
    match decode_plan(located, plan, order)? {
        DecodedValue::Integer(pointer) => Some(vec![pointer]),
        DecodedValue::Array(values) => {
            let max = usize::try_from(edge.max_subdirs?).ok()?;
            values
                .iter()
                .take(max)
                .map(DecodedValue::as_integer)
                .collect()
        }
        _ => None,
    }
}

/// Exif.pm:6982-6993 -- `ByteOrder => 'Unknown'`: read the sub-directory's
/// entry count in the enclosing order and flip when it "looks wrong":
///
/// ```perl
/// my $num = Get16u($subdirDataPt, $subdirStart);
/// if ($num & 0xff00 and ($num>>8) > ($num&0xff)) {
///     $newByteOrder = $otherOrder{$oldByteOrder};
/// } else {
///     $newByteOrder = $oldByteOrder;
/// }
/// ```
///
/// (Not "0 or > 512": a count of 0x0100 flips, 0x012C does not.) `None`
/// when the two bytes are not inside `data` -- Exif.pm:7017 then refuses the
/// start anyway.
fn detect_byte_order(data: &[u8], start: usize, old: ByteOrder) -> Option<ByteOrder> {
    let num = get16(data, start, old)?;
    Some(if num & 0xff00 != 0 && (num >> 8) > (num & 0xff) {
        match old {
            ByteOrder::Big => ByteOrder::Little,
            ByteOrder::Little => ByteOrder::Big,
        }
    } else {
        old
    })
}

/// `$$subdirInfo{DirName}` for the nested directory: `SubDirectory.DirName`
/// (Exif.pm:7053), overridden by the tag's own `Groups{1}` when the tag
/// declares groups and is not writable (Exif.pm:7073-7077 -- including to
/// nothing, when the tag declares only families 0/2), and finally the tag's
/// name inside maker notes (Exif.pm:7087-7088). The `$dirNum` suffix
/// (Exif.pm:7076) for a second and later `MaxSubdirs` directory is not
/// reproduced (it would need a runtime-built name); it matters only to a
/// `SET_GROUP1` target, of which the pinned tree has one (`Exif::Main`).
fn subdir_name(tag: &IfdTag, edge: &IfdSubdirEdge, in_maker_notes: bool) -> Option<&'static str> {
    let mut name = edge.dir_name;
    let has_groups = tag.groups.g0.is_some() || tag.groups.g1.is_some() || tag.groups.g2.is_some();
    if has_groups && tag.writable.is_none() {
        name = tag.groups.g1;
    }
    if name.is_none() && in_maker_notes {
        name = Some(tag.name);
    }
    name
}

/// Whether a `Base` expression reads `$base` -- the enclosing directory's
/// absolute base, which [`IfdDir`] does not carry (module doc).
fn mentions_base(expr: &BaseExpr) -> bool {
    match expr {
        BaseExpr::Base => true,
        BaseExpr::Const(_) | BaseExpr::Start => false,
        BaseExpr::Add(a, b) | BaseExpr::Sub(a, b) | BaseExpr::Mul(a, b) => {
            mentions_base(a) || mentions_base(b)
        }
        BaseExpr::Neg(a) => mentions_base(a),
    }
}

/// Whether the walk may report `table` when an edge leads to it: Gate A and
/// Gate B ([`IfdTable::enabled`]), plus -- under `cfg(test)` only -- the
/// tables a unit test has registered (see `tests::Registered`), so the
/// descent path can be exercised while the real allowlist is still empty.
fn walkable(table: &'static IfdTable) -> bool {
    table.enabled() || test_hook_enabled(table)
}

/// `find_ifd_table` for an edge's target, plus the test registry under
/// `cfg(test)` (the generated set is empty on this branch).
fn ifd_target(module: &str, table: &str) -> Option<&'static IfdTable> {
    test_hook_table(module, table).or_else(|| find_ifd_table(module, table))
}

#[cfg(test)]
fn test_hook_enabled(table: &'static IfdTable) -> bool {
    tests::registered_enabled(table)
}

#[cfg(not(test))]
fn test_hook_enabled(_table: &'static IfdTable) -> bool {
    false
}

#[cfg(test)]
fn test_hook_table(module: &str, table: &str) -> Option<&'static IfdTable> {
    tests::registered_table(module, table)
}

#[cfg(not(test))]
fn test_hook_table(_module: &str, _table: &str) -> Option<&'static IfdTable> {
    None
}

/// Exif.pm:6919-7102 -- open the directory (or directories) an entry points
/// at and process each with the right table.
#[allow(clippy::too_many_arguments)]
fn descend(
    table: &'static IfdTable,
    tag: &'static IfdTag,
    edge: &IfdSubdirEdge,
    located: &Located<'_>,
    dir: &IfdDir<'_>,
    ctx: &mut cond::Ctx,
    guard: &mut Guard,
    out: &mut Vec<Emitted>,
) {
    // Exif.pm:7081-7085: `Validate` is Perl over the directory bytes.
    if edge.validate {
        return;
    }
    // Slice IFD1: the generator emitted the edge but marked it unwalked --
    // the enclosing table itself (no TagTable, Exif.pm:6939-6944) or a
    // ProcessProc the walk cannot run. Same outcome as `validate`: the
    // pointer marks its place and nothing behind it is read.
    if edge.unwalked.is_some() {
        return;
    }
    // Exif.pm:6921-6926 -- "don't process empty subdirectories".
    if located.bytes.is_empty() {
        return;
    }
    let in_maker_notes = table.group0 == "MakerNotes";
    let data = dir.data;
    let data_len = i64::try_from(data.len()).unwrap_or(i64::MAX);
    let value_pos = i64::try_from(located.value_pos).unwrap_or(i64::MAX);
    let size = i64::try_from(located.bytes.len()).unwrap_or(i64::MAX);

    // Exif.pm:6939-6944: resolve the target table first (an unknown table
    // is `next` before the loop; nothing is recorded for it).
    enum Target {
        Ifd(&'static IfdTable),
        Binary(&'static super::BinaryTable),
    }
    let target = if let Some(t) = ifd_target(edge.module, edge.table) {
        if !walkable(t) {
            // Opt-in (Step 28 D1): an edge never enables its target.
            return;
        }
        Target::Ifd(t)
    } else if let Some(t) = find_table(edge.module, edge.table) {
        if !t.enabled() {
            return;
        }
        Target::Binary(t)
    } else {
        // Not a defect in the edge: the target is a table neither generator
        // transcribed (a custom `PROCESS_PROC`, a refused table).
        return;
    };

    // Exif.pm:6929-6938, 7100-7101: how many times the loop runs.
    let iterations: Vec<Option<i64>> = match edge.start {
        IfdStart::Val(_) => match pointer_values(tag, edge, located, dir.byte_order) {
            Some(pointers) => pointers.into_iter().map(Some).collect(),
            None => return,
        },
        // A `$valuePtr` start does not depend on `$val`; with `MaxSubdirs`
        // ExifTool would loop over the value's pieces re-opening the SAME
        // start, which `$$self{PROCESSED}` (and the guard here) refuses after
        // the first, so once is the observable count.
        IfdStart::ValuePtr(_) => vec![None],
    };

    let dir_name = subdir_name(tag, edge, in_maker_notes);

    for pointer in iterations {
        // Exif.pm:6951-6968 -- `#### eval Start ($valuePtr, $val)`, then
        // `$newStart -= $subdirDataPos` back to data-relative.
        let start = match (edge.start, pointer) {
            (IfdStart::ValuePtr(offset), _) => value_pos.saturating_add(offset),
            (IfdStart::Val(offset), Some(pointer)) => {
                // `$val + base` is where the pointer lands in `data`; with
                // no correction the block cannot be located.
                let Some(base) = dir.base else {
                    return;
                };
                pointer.saturating_add(offset).saturating_add(base)
            }
            (IfdStart::Val(_), None) => return,
        };
        // Exif.pm:6964-6966: `$size -= $newStart - $subdirStart` unless
        // SubIFD (or BadOffset, not modelled) -- the DirLen a binary target
        // receives.
        let dir_len = if edge.sub_ifd {
            size
        } else {
            size - (start - value_pos)
        };
        // Exif.pm:7017-7037: "Bad SubDirectory start" ends the loop (`last`).
        if start < 0 || start.saturating_add(2) > data_len {
            return;
        }
        let Ok(start_pos) = usize::try_from(start) else {
            return;
        };
        // Exif.pm:6971-6997.
        let byte_order = match edge.byte_order {
            IfdByteOrder::Inherit => dir.byte_order,
            IfdByteOrder::Little => ByteOrder::Little,
            IfdByteOrder::Big => ByteOrder::Big,
            IfdByteOrder::Unknown => match detect_byte_order(data, start_pos, dir.byte_order) {
                Some(order) => order,
                None => return,
            },
        };
        // Exif.pm:6999-7004 -- `#### eval Base ($start,$base)` with `$start`
        // the SUB-DIRECTORY's start relative to the base (`$subdirStart +
        // $subdirDataPos`), then Exif.pm:7040's `$subdirDataPos += $base -
        // $subdirBase`: the new correction is the old one plus the
        // expression's value.
        let base = match edge.base {
            None => dir.base,
            Some(expr) => {
                if mentions_base(expr) {
                    return;
                }
                match dir.base {
                    Some(base) => Some(base.saturating_add(expr.eval(start - base, 0))),
                    // `$start` is unknowable without a correction; a
                    // constant leaves the correction unknown too.
                    None if matches!(expr, BaseExpr::Const(_)) => None,
                    None => return,
                }
            }
        };

        match target {
            Target::Ifd(target) => {
                // ExifTool.pm:9065-9072: a repeat is `return 0` for THIS
                // directory; the loop moves on to the next value.
                if !guard.admit(ifd_addr(start_pos), table_key(target), false) {
                    continue;
                }
                guard.depth += 1;
                walk(
                    target,
                    IfdDir {
                        data,
                        ifd_start: start_pos,
                        base,
                        byte_order,
                        group1: dir_name,
                    },
                    ctx,
                    guard,
                    out,
                );
                guard.depth -= 1;
            }
            Target::Binary(target) => {
                // ProcessBinaryData with `$size <= 0` reads nothing
                // (ExifTool.pm:9964 `last if $more <= 0` on the first key).
                let Ok(dir_len) = usize::try_from(dir_len) else {
                    continue;
                };
                if dir_len == 0 {
                    continue;
                }
                if !guard.admit(binary_addr(base, start_pos), table_key(target), false) {
                    continue;
                }
                guard.depth += 1;
                engine::walk(
                    target,
                    Dir {
                        data,
                        dir_start: start_pos,
                        dir_len: Some(dir_len),
                        // The binary engine reads at data-relative offsets;
                        // `base`/`data_pos` only feed its `Base` expressions
                        // and guard keys. `data_pos` is ExifTool's
                        // `$dataPos` (the negated correction); the absolute
                        // `$base` is not known here (module doc).
                        base: 0,
                        data_pos: base.map_or(0, |b| -b),
                        byte_order,
                    },
                    ctx,
                    guard,
                    out,
                );
                guard.depth -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;
    use crate::exiftool_tables::cond::{CmpOp, Cond};
    use crate::exiftool_tables::ifd_schema::IfdVariantGroup;
    use crate::exiftool_tables::{ExprId, GateA, IfdFlags, Omitted, PrintConv, TagGroups};

    // -- Test-only enablement/lookup registry ----------------------------------
    //
    // `ENABLED_IFD` is empty and `ALL_IFD_TABLES` is the stub on this branch,
    // so a hand-built target could never be found nor enabled through the real
    // gates. `Registered` makes the tables it names both findable (by their
    // `module`/`table`) and walkable for the duration of a test, and nothing
    // else: the production `walkable`/`ifd_target` consult it only under
    // `cfg(test)`, and only for the registered pointers.

    thread_local! {
        static REGISTERED: RefCell<Vec<&'static IfdTable>> = const { RefCell::new(Vec::new()) };
    }

    pub(super) fn registered_enabled(table: &'static IfdTable) -> bool {
        REGISTERED.with(|r| r.borrow().iter().any(|t| std::ptr::eq(*t, table)))
    }

    pub(super) fn registered_table(module: &str, table: &str) -> Option<&'static IfdTable> {
        REGISTERED.with(|r| {
            r.borrow()
                .iter()
                .copied()
                .find(|t| t.module == module && t.table == table)
        })
    }

    struct Registered(usize);

    impl Registered {
        fn new(tables: &[&'static IfdTable]) -> Self {
            REGISTERED.with(|r| r.borrow_mut().extend_from_slice(tables));
            Self(tables.len())
        }
    }

    impl Drop for Registered {
        fn drop(&mut self) {
            REGISTERED.with(|r| {
                let mut r = r.borrow_mut();
                let keep = r.len() - self.0;
                r.truncate(keep);
            });
        }
    }

    // -- Fixture helpers -------------------------------------------------------

    const fn plain(id: u16, name: &'static str) -> IfdTag {
        IfdTag {
            id,
            name,
            format: None,
            count: None,
            writable: None,
            groups: TagGroups::NONE,
            flags: IfdFlags::NONE,
            omitted: Omitted::NONE,
            raw_conv: None,
            value_conv: None,
            print_conv: PrintConv::None,
            subdir: None,
        }
    }

    const fn table(name: &'static str, tags: &'static [IfdTag]) -> IfdTable {
        IfdTable {
            module: "Test",
            table: name,
            group0: "MakerNotes",
            group1: "Test",
            group2: "Camera",
            set_group1: None,
            priority: None,
            gate_a: GateA { blocked_by: &[] },
            tags,
            variants: &[],
        }
    }

    const fn edge(table: &'static str) -> IfdSubdirEdge {
        IfdSubdirEdge {
            module: "Test",
            table,
            start: IfdStart::ValuePtr(0),
            base: None,
            byte_order: IfdByteOrder::Inherit,
            fix_format: None,
            sub_ifd: false,
            max_subdirs: None,
            dir_name: None,
            validate: false,
            unwalked: None,
        }
    }

    fn be16(v: u16) -> [u8; 2] {
        v.to_be_bytes()
    }

    fn bytes16(order: ByteOrder, v: u16) -> [u8; 2] {
        match order {
            ByteOrder::Big => v.to_be_bytes(),
            ByteOrder::Little => v.to_le_bytes(),
        }
    }

    fn bytes32(order: ByteOrder, v: u32) -> [u8; 4] {
        match order {
            ByteOrder::Big => v.to_be_bytes(),
            ByteOrder::Little => v.to_le_bytes(),
        }
    }

    /// One 12-byte entry with a literal 4-byte value field.
    fn entry(order: ByteOrder, tag: u16, ty: u16, count: u32, value: [u8; 4]) -> [u8; 12] {
        let mut e = [0u8; 12];
        e[..2].copy_from_slice(&bytes16(order, tag));
        e[2..4].copy_from_slice(&bytes16(order, ty));
        e[4..8].copy_from_slice(&bytes32(order, count));
        e[8..].copy_from_slice(&value);
        e
    }

    /// An int16u entry whose value sits inline (padded to four bytes).
    fn int16u_entry(order: ByteOrder, tag: u16, v: u16) -> [u8; 12] {
        let mut value = [0u8; 4];
        value[..2].copy_from_slice(&bytes16(order, v));
        entry(order, tag, 3, 1, value)
    }

    /// A directory: count, entries, a zero next-IFD link, then `trailer`.
    fn ifd(order: ByteOrder, entries: &[[u8; 12]], trailer: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&bytes16(order, entries.len() as u16));
        for e in entries {
            data.extend_from_slice(e);
        }
        data.extend_from_slice(&[0, 0, 0, 0]);
        data.extend_from_slice(trailer);
        data
    }

    /// Where `ifd`'s trailer starts: the floor.
    fn trailer_at(entries: usize) -> usize {
        directory_floor(0, entries)
    }

    fn run_with(
        table: &'static IfdTable,
        data: &[u8],
        order: ByteOrder,
        base: Option<i64>,
        members: &mut HashMap<&'static str, MemberValue>,
    ) -> Vec<Emitted> {
        let mut ctx = cond::Ctx::new(members);
        let mut out = Vec::new();
        process_exif(
            table,
            IfdDir {
                data,
                ifd_start: 0,
                base,
                byte_order: order,
                group1: None,
            },
            &mut ctx,
            &mut out,
        );
        out
    }

    fn run(
        table: &'static IfdTable,
        data: &[u8],
        order: ByteOrder,
        base: Option<i64>,
    ) -> Vec<Emitted> {
        let mut members = HashMap::new();
        run_with(table, data, order, base, &mut members)
    }

    fn values(out: &[Emitted]) -> Vec<(&'static str, TagValue)> {
        out.iter().map(|e| (e.name, e.value.clone())).collect()
    }

    // -- Reading the directory -------------------------------------------------

    #[test]
    fn read_ifd_rejects_a_zero_count_an_implausible_count_and_a_short_array() {
        // table_ifd.rs:549-573's three refusals, in its order.
        assert_eq!(read_ifd(&[0, 0], 0, ByteOrder::Big), None, "count 0");
        assert_eq!(
            read_ifd(&[0x02, 0x01], 0, ByteOrder::Big),
            None,
            "count 513 > MAX_IFD_ENTRIES"
        );
        assert_eq!(
            read_ifd(&[0, 1, 0, 0, 0, 0], 0, ByteOrder::Big),
            None,
            "one entry needs 12 bytes after the count"
        );
        assert_eq!(read_ifd(&[0], 0, ByteOrder::Big), None, "no count at all");
    }

    #[test]
    fn read_ifd_reads_entries_in_either_byte_order() {
        for order in [ByteOrder::Big, ByteOrder::Little] {
            let data = ifd(
                order,
                &[entry(order, 0x0102, 3, 2, bytes32(order, 0x1234_5678))],
                &[],
            );
            let got = read_ifd(&data, 0, order).expect("one entry");
            assert_eq!(
                got,
                vec![IfdEntry {
                    tag_id: 0x0102,
                    field_type: 3,
                    count: 2,
                    value_offset: 0x1234_5678,
                    value_field_pos: 2 + 8,
                }],
                "{order:?}"
            );
        }
    }

    // -- A plain tag, both byte orders -----------------------------------------

    static PLAIN_TAGS: &[IfdTag] = &[
        IfdTag {
            print_conv: PrintConv::IntEnum(&[(1, "One"), (2, "Two")]),
            ..plain(0x0001, "Mode")
        },
        plain(0x0002, "Raw"),
    ];
    static PLAIN: IfdTable = table("Plain", PLAIN_TAGS);

    #[test]
    fn a_plain_int16u_tag_renders_an_enum_hit_and_reports_a_miss_raw() {
        for order in [ByteOrder::Big, ByteOrder::Little] {
            let data = ifd(
                order,
                &[
                    int16u_entry(order, 0x0001, 2),
                    int16u_entry(order, 0x0002, 7),
                ],
                &[],
            );
            let got = run(&PLAIN, &data, order, Some(0));
            assert_eq!(
                values(&got),
                vec![
                    ("Mode", TagValue::String("Two".to_string())),
                    ("Raw", TagValue::Integer(7)),
                ],
                "{order:?}"
            );
            assert_eq!(got[0].group0, "MakerNotes");
            assert_eq!(got[0].group1, "Test", "the table's effective group 1");
            assert_eq!(got[0].group2, "Camera");
            assert!(!got[0].low_priority);
            assert!(!got[0].avoid);
        }
        // An enum MISS renders ExifTool's own `Unknown (9)` (ExifTool.pm:
        // 3624-3631) -- `runtime::render` does that for every hash miss
        // since 4b-i (`b7797fa2`); the engine does not special-case it.
        let data = ifd(
            ByteOrder::Big,
            &[int16u_entry(ByteOrder::Big, 0x0001, 9)],
            &[],
        );
        let got = run(&PLAIN, &data, ByteOrder::Big, Some(0));
        assert_eq!(
            values(&got),
            vec![("Mode", TagValue::String("Unknown (9)".to_string()))]
        );
    }

    #[test]
    fn a_tag_the_table_does_not_declare_is_skipped() {
        let data = ifd(
            ByteOrder::Big,
            &[int16u_entry(ByteOrder::Big, 0x0099, 1)],
            &[],
        );
        assert!(run(&PLAIN, &data, ByteOrder::Big, Some(0)).is_empty());
    }

    // -- Format override (Exif.pm:6735-6744) -----------------------------------

    static OVERRIDE_TAGS: &[IfdTag] = &[
        IfdTag {
            format: Some(Fmt::Int16u),
            ..plain(0x0001, "Pair")
        },
        IfdTag {
            format: Some(Fmt::Int16u),
            flags: IfdFlags {
                list: true,
                ..IfdFlags::NONE
            },
            ..plain(0x0002, "PairList")
        },
        IfdTag {
            format: Some(Fmt::Rational32u),
            ..plain(0x0003, "Refused")
        },
        IfdTag {
            format: Some(Fmt::Str(0)),
            ..plain(0x0004, "AsString")
        },
        IfdTag {
            format: Some(Fmt::Undef(0)),
            ..plain(0x0005, "AsUndef")
        },
    ];
    static OVERRIDE: IfdTable = table("Override", OVERRIDE_TAGS);

    #[test]
    fn a_format_override_reinterprets_the_entry_bytes_with_a_recomputed_count() {
        // int32u x1 (4 bytes) read as int16u: `$count = int(4 / 2)` = 2, so
        // ExifTool's `ReadValue` returns the joined string "1 2"
        // (ExifTool.pm:6330); with `List => 1` the array shape is reported.
        let order = ByteOrder::Big;
        let data = ifd(
            order,
            &[
                entry(order, 0x0001, 4, 1, [0, 1, 0, 2]),
                entry(order, 0x0002, 4, 1, [0, 3, 0, 4]),
            ],
            &[],
        );
        let got = run(&OVERRIDE, &data, order, Some(0));
        assert_eq!(
            values(&got),
            vec![
                ("Pair", TagValue::String("1 2".to_string())),
                (
                    "PairList",
                    TagValue::Array(vec![TagValue::Integer(3), TagValue::Integer(4)])
                ),
            ]
        );
    }

    #[test]
    fn a_format_override_outside_format_number_is_refused_not_approximated() {
        // `rational32u` is not in `%formatNumber` (Exif.pm:96-116); ExifTool
        // would take ReadValue's fallback arm with the ORIGINAL count. Not
        // modelled, so the tag is withheld.
        let order = ByteOrder::Big;
        let data = ifd(order, &[entry(order, 0x0003, 4, 1, [0, 1, 0, 2])], &[]);
        assert!(run(&OVERRIDE, &data, order, Some(0)).is_empty());
    }

    #[test]
    fn string_and_undef_overrides_reread_the_run_and_undo_the_int8u_patch() {
        // `Format => 'string'` on an undef x4 entry: NUL-truncated text.
        // `Format => 'undef'` on an undef x1 entry: Exif.pm:6682 would have
        // read it as int8u, but the explicit override (Exif.pm:6736) puts
        // `undef` back, so one raw byte is reported, not an integer.
        let order = ByteOrder::Big;
        let data = ifd(
            order,
            &[
                entry(order, 0x0004, 7, 4, *b"AB\0Z"),
                entry(order, 0x0005, 7, 1, [0x41, 0, 0, 0]),
            ],
            &[],
        );
        let got = run(&OVERRIDE, &data, order, Some(0));
        assert_eq!(
            values(&got),
            vec![
                ("AsString", TagValue::String("AB".to_string())),
                ("AsUndef", TagValue::Binary(vec![0x41])),
            ]
        );
    }

    // -- Strings, undef, inline vs out of line, base, floor ----------------------

    static STRINGS_TAGS: &[IfdTag] = &[
        plain(0x0001, "Text"),
        plain(0x0002, "Blob"),
        plain(0x0003, "OneByte"),
    ];
    static STRINGS: IfdTable = table("Strings", STRINGS_TAGS);

    #[test]
    fn a_string_is_truncated_at_the_first_nul_and_an_undef_is_verbatim() {
        // ExifTool.pm:6308-6311. Both values are out of line (> 4 bytes) and
        // live in the trailer, so `base: Some(0)` maps stored offsets 1:1.
        let order = ByteOrder::Big;
        let floor = trailer_at(3);
        let text_off = floor as u32;
        let blob_off = text_off + 6;
        let data = ifd(
            order,
            &[
                entry(order, 0x0001, 2, 6, bytes32(order, text_off)),
                entry(order, 0x0002, 7, 5, bytes32(order, blob_off)),
                // Exif.pm:6682: a single undef byte reads as int8u.
                entry(order, 0x0003, 7, 1, [0x2a, 0, 0, 0]),
            ],
            b"AB\0XY\0\x01\x02\x00\x03\x04",
        );
        let got = run(&STRINGS, &data, order, Some(0));
        assert_eq!(
            values(&got),
            vec![
                ("Text", TagValue::String("AB".to_string())),
                ("Blob", TagValue::Binary(vec![1, 2, 0, 3, 4])),
                ("OneByte", TagValue::Integer(42)),
            ]
        );
    }

    #[test]
    fn an_out_of_line_value_is_read_through_the_base_correction() {
        // The stored offset is TIFF-relative (say the maker note begins 100
        // bytes into the TIFF); the caller's correction is -100.
        let order = ByteOrder::Little;
        let floor = trailer_at(1) as u32;
        let data = ifd(
            order,
            &[entry(order, 0x0001, 2, 5, bytes32(order, floor + 100))],
            b"HELLO",
        );
        let got = run(&STRINGS, &data, order, Some(-100));
        assert_eq!(
            values(&got),
            vec![("Text", TagValue::String("HELLO".to_string()))]
        );
        // The wrong correction lands outside the buffer: skipped, not read.
        assert!(run(&STRINGS, &data, order, Some(0)).is_empty());
    }

    #[test]
    fn base_none_suppresses_out_of_line_reads_but_not_inline_ones() {
        let order = ByteOrder::Big;
        let floor = trailer_at(2) as u32;
        let data = ifd(
            order,
            &[
                entry(order, 0x0001, 2, 5, bytes32(order, floor)),
                entry(order, 0x0003, 7, 1, [0x2a, 0, 0, 0]),
            ],
            b"HELLO",
        );
        let got = run(&STRINGS, &data, order, None);
        assert_eq!(
            values(&got),
            vec![("OneByte", TagValue::Integer(42))],
            "the inline byte is reported; the out-of-line string is not"
        );
    }

    #[test]
    fn a_value_that_starts_before_the_floor_is_rejected() {
        // The stored offset points back into the directory itself (at the
        // second entry's bytes). Exif.pm:6549/6673-6678 calls that
        // "Suspicious" and skips it; table_ifd.rs's floor refuses anything
        // before `dirEnd + 4`.
        let order = ByteOrder::Big;
        let data = ifd(
            order,
            &[
                entry(order, 0x0001, 2, 5, bytes32(order, 2 + 12)),
                entry(order, 0x0003, 7, 1, [0x2a, 0, 0, 0]),
            ],
            b"HELLO",
        );
        let got = run(&STRINGS, &data, order, Some(0));
        assert_eq!(values(&got), vec![("OneByte", TagValue::Integer(42))]);
        // Exactly AT the floor is fine.
        let floor = trailer_at(1) as u32;
        let data = ifd(
            order,
            &[entry(order, 0x0001, 2, 5, bytes32(order, floor))],
            b"HELLO",
        );
        assert_eq!(
            values(&run(&STRINGS, &data, order, Some(0))),
            vec![("Text", TagValue::String("HELLO".to_string()))]
        );
    }

    // -- Multi-count values, rationals, utf8 ---------------------------------------

    static NUMERIC_TAGS: &[IfdTag] = &[
        plain(0x0001, "Triple"),
        plain(0x0002, "Ratio"),
        plain(0x0003, "Ratios"),
        plain(0x0004, "Utf8"),
        plain(0x0005, "Floats"),
        plain(0x0006, "Empty"),
    ];
    static NUMERIC: IfdTable = table("Numeric", NUMERIC_TAGS);

    #[test]
    fn a_zero_count_entry_reads_as_the_empty_value() {
        // ExifTool.pm:6296-6297: `return '' if defined $count` -- a zero-count
        // entry is reported with the empty value, not skipped. OlympusXZ-1.jpg's
        // RawDevelopment2 0x0108 (a 0-byte `int16s`) prints `""` and, walked
        // after RawDevelopment, overwrites that table's `0` under the same name.
        let order = ByteOrder::Big;
        let data = ifd(order, &[entry(order, 0x0006, 8, 0, [0, 0, 0, 0])], &[]);
        let got = run(&NUMERIC, &data, order, Some(0));
        assert_eq!(
            values(&got),
            vec![("Empty", TagValue::String(String::new()))]
        );
    }

    #[test]
    fn a_multi_count_numeric_entry_is_one_space_joined_string() {
        // ExifTool.pm:6330 `join(' ', @vals)`, rendered per element as Perl
        // would: integers as digits, rational64u as `%.10g` of the quotient
        // (ExifTool.pm:6114-6120, 5960-5964), floats as `%.15g`.
        let order = ByteOrder::Big;
        let floor = trailer_at(5) as u32;
        let mut trailer = Vec::new();
        trailer.extend_from_slice(&[0, 1, 0, 2, 0, 3]); // int16u x3
        trailer.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 3, 0, 0, 0, 5, 0, 0, 0, 0]); // 1/3, 5/0
        trailer.extend_from_slice("caf\u{e9}!".as_bytes()); // utf8 x6
        trailer.extend_from_slice(&1.5f32.to_be_bytes());
        trailer.extend_from_slice(&(1.0f32 / 3.0).to_be_bytes());
        let data = ifd(
            order,
            &[
                entry(order, 0x0001, 3, 3, bytes32(order, floor)),
                entry(order, 0x0002, 5, 1, bytes32(order, floor + 6)),
                entry(order, 0x0003, 5, 2, bytes32(order, floor + 6)),
                entry(order, 0x0004, 129, 6, bytes32(order, floor + 22)),
                entry(order, 0x0005, 11, 2, bytes32(order, floor + 28)),
            ],
            &trailer,
        );
        let got = run(&NUMERIC, &data, order, Some(0));
        assert_eq!(
            values(&got),
            vec![
                ("Triple", TagValue::String("1 2 3".to_string())),
                // `decode_plan` still yields the pair (design spec section
                // 3); the `ReadValue` step (`round_rationals`) then makes it
                // the number `RoundFloat(1 / 3, 10)` numifies to (ExifTool.pm:
                // 6119), which is what a single unconverted rational reports.
                ("Ratio", TagValue::Float(0.333_333_333_3)),
                ("Ratios", TagValue::String("0.3333333333 inf".to_string())),
                ("Utf8", TagValue::String("caf\u{e9}!".to_string())),
                (
                    "Floats",
                    TagValue::String("1.5 0.333333343267441".to_string())
                ),
            ]
        );
    }

    // -- ReadValue's rational is RoundFloat(.., 10) (ExifTool.pm:6107-6120) --------

    static ROUNDED_TAGS: &[IfdTag] = &[
        // Olympus.pm:755-760 0x0205 `FocalPlaneDiagonal`, `rational64u`,
        // `PrintConv => '"$val mm"'` -- the generated expression itself.
        IfdTag {
            print_conv: PrintConv::Expr(ExprId::ValMm18ABDF),
            ..plain(0x0001, "Diagonal")
        },
        IfdTag {
            print_conv: PrintConv::IntEnum(&[(2, "Two")]),
            ..plain(0x0002, "Mode")
        },
        IfdTag {
            raw_conv: Some(RawConvEffect::SetMember {
                member: "TestRatio",
            }),
            ..plain(0x0003, "Ratio")
        },
        IfdTag {
            print_conv: PrintConv::IntEnum(&[(1, "One")]),
            ..plain(0x0004, "Signed")
        },
        plain(0x0005, "Zero"),
        IfdTag {
            print_conv: PrintConv::StrEnum(&[("0.5 0.25", "Quarter")]),
            ..plain(0x0006, "Pair")
        },
        plain(0x0007, "Big"),
    ];
    static ROUNDED: IfdTable = table("Rounded", ROUNDED_TAGS);

    /// Every consumer of a rational sees `RoundFloat($n / $d, 10)`
    /// (ExifTool.pm:6112/6119, `sprintf("%.10g")` at 5960-5964): the
    /// `"$val mm"` interpolation, the integer-keyed hash, the `RawConv`
    /// member store, the miss text, the joined key of a fixed-count entry
    /// and the unconverted report. Expected strings are perl 5.34.1's own
    /// `printf("%.10g", $n / $d)`; the `1.733823728e-07 mm` line is what
    /// the pinned oracle prints for `OlympusFE-120.jpg` (`olympus/tables.rs`,
    /// `ENGINE_MISRENDERS`).
    #[test]
    fn a_rational_is_read_as_round_float_10_before_any_conversion() {
        let order = ByteOrder::Big;
        let floor = trailer_at(7) as u32;
        let rational = |n: u32, d: u32| {
            let mut bytes = Vec::with_capacity(8);
            bytes.extend_from_slice(&bytes32(order, n));
            bytes.extend_from_slice(&bytes32(order, d));
            bytes
        };
        let mut trailer = Vec::new();
        trailer.extend(rational(256, 1_476_505_344)); // floor + 0
        trailer.extend(rational(2, 1)); // floor + 8
        trailer.extend(rational(1, 3)); // floor + 16
        trailer.extend(rational((-1i32) as u32, 2)); // floor + 24, rational64s
        trailer.extend(rational(4, 0)); // floor + 32
        trailer.extend(rational(1, 2)); // floor + 40
        trailer.extend(rational(1, 4)); // floor + 48
        trailer.extend(rational(4_294_967_295, 1)); // floor + 56
        let data = ifd(
            order,
            &[
                entry(order, 0x0001, 5, 1, bytes32(order, floor)),
                entry(order, 0x0002, 5, 1, bytes32(order, floor + 8)),
                entry(order, 0x0003, 5, 1, bytes32(order, floor + 16)),
                entry(order, 0x0004, 10, 1, bytes32(order, floor + 24)),
                entry(order, 0x0005, 5, 1, bytes32(order, floor + 32)),
                entry(order, 0x0006, 5, 2, bytes32(order, floor + 40)),
                entry(order, 0x0007, 5, 1, bytes32(order, floor + 56)),
            ],
            &trailer,
        );
        let mut members = HashMap::new();
        let got = run_with(&ROUNDED, &data, order, Some(0), &mut members);
        assert_eq!(
            values(&got),
            vec![
                // `%.10g`, not `%.15g`: `1.73382372803657e-07 mm` was the defect.
                (
                    "Diagonal",
                    TagValue::String("1.733823728e-07 mm".to_string())
                ),
                // An exact quotient is still the integer key: `$$conv{2}` hits.
                ("Mode", TagValue::String("Two".to_string())),
                // Unconverted: the number the ten-digit string numifies to.
                ("Ratio", TagValue::Float(0.333_333_333_3)),
                // A hash miss interpolates the rounded `$val` (ExifTool.pm:3633).
                ("Signed", TagValue::String("Unknown (-0.5)".to_string())),
                // A zero denominator is `inf` before `RoundFloat` (ExifTool.pm:
                // 6118): the pair is kept for the caller to spell.
                (
                    "Zero",
                    TagValue::Rational {
                        numerator: 4,
                        denominator: 0
                    }
                ),
                // Element-wise, then `join(' ', @vals)` keys the hash.
                ("Pair", TagValue::String("Quarter".to_string())),
                // The full `u32` range survives (no `i32` wrap).
                ("Big", TagValue::Float(4_294_967_295.0)),
            ]
        );
        // `$$self{TestRatio} = $val` stored the `RoundFloat` digits.
        assert_eq!(
            members.get("TestRatio"),
            Some(&MemberValue::Str("0.3333333333".to_string()))
        );
    }

    #[test]
    fn round_rationals_rounds_each_element_and_keeps_zero_denominators() {
        assert_eq!(
            round_rationals(DecodedValue::UnsignedRational(1, 3)),
            DecodedValue::Float(0.333_333_333_3)
        );
        assert_eq!(
            round_rationals(DecodedValue::SignedRational(-1, 2)),
            DecodedValue::Float(-0.5)
        );
        assert_eq!(
            round_rationals(DecodedValue::UnsignedRational(4, 0)),
            DecodedValue::UnsignedRational(4, 0)
        );
        assert_eq!(
            round_rationals(DecodedValue::SignedRational(0, 0)),
            DecodedValue::SignedRational(0, 0)
        );
        assert_eq!(
            round_rationals(DecodedValue::Array(vec![
                DecodedValue::UnsignedRational(1, 3),
                DecodedValue::UnsignedRational(5, 0),
                DecodedValue::Integer(7),
            ])),
            DecodedValue::Array(vec![
                DecodedValue::Float(0.333_333_333_3),
                DecodedValue::UnsignedRational(5, 0),
                DecodedValue::Integer(7),
            ])
        );
        // The rounded number prints the ten-digit string, and the joined
        // form is what ExifTool.pm:6330 builds from the element strings.
        assert_eq!(
            ifd_perl_string(&DecodedValue::Float(0.333_333_333_3)).as_deref(),
            Some("0.3333333333")
        );
        let other = DecodedValue::String("s".to_string());
        assert_eq!(round_rationals(other.clone()), other);
    }

    // -- A hash PrintConv keyed by a fixed-count value (ExifTool.pm:6330, 3616) ---

    static WB_MODE_TAGS: &[IfdTag] = &[IfdTag {
        // Olympus.pm:1020-1041 0x1015 `WBMode`, `int16u[2]`, keyed by the
        // space-joined value; the bare `'1'` key is ExifTool's own.
        count: Some(2),
        print_conv: PrintConv::StrEnum(&[("1", "Auto"), ("1 0", "Auto"), ("3 0", "One-touch")]),
        ..plain(0x1015, "WBMode")
    }];
    static WB_MODE: IfdTable = table("WbMode", WB_MODE_TAGS);

    /// `ReadValue` returns `join(' ', @vals)` for a count above one
    /// (ExifTool.pm:6330) and `GetValue` looks `$$conv{$val}` up with it
    /// (ExifTool.pm:3616), `Unknown ($val)` on a miss (ExifTool.pm:3633).
    /// The pinned oracle prints `Auto` for `OlympusBrioD100.jpg`'s `1 0`,
    /// `One-touch` for `OlympusE10.jpg`'s `3 0` and `Unknown (1 1)` for
    /// `OlympusC160.jpg` (`olympus/tables.rs`, `ENGINE_MISRENDERS`); the walk
    /// used to fall back to the raw `1 0`.
    #[test]
    fn a_fixed_count_entry_keys_a_hash_print_conv_by_its_joined_value() {
        let order = ByteOrder::Little;
        let pair = |a: u16, b: u16| {
            let mut value = [0u8; 4];
            value[..2].copy_from_slice(&bytes16(order, a));
            value[2..].copy_from_slice(&bytes16(order, b));
            entry(order, 0x1015, 3, 2, value)
        };
        for (entry, expected) in [
            (pair(1, 0), "Auto"),
            (pair(3, 0), "One-touch"),
            (pair(1, 1), "Unknown (1 1)"),
            // A one-count entry is `$vals[0]` (ExifTool.pm:6331): the bare key.
            (int16u_entry(order, 0x1015, 1), "Auto"),
            (int16u_entry(order, 0x1015, 9), "Unknown (9)"),
        ] {
            let data = ifd(order, &[entry], &[]);
            assert_eq!(
                values(&run(&WB_MODE, &data, order, Some(0))),
                vec![("WBMode", TagValue::String(expected.to_string()))]
            );
        }
    }

    // -- Bad format codes (Exif.pm:6463-6478) --------------------------------------

    #[test]
    fn a_bad_format_on_the_first_entry_aborts_the_directory_unless_ilce() {
        let order = ByteOrder::Big;
        let data = ifd(
            order,
            &[
                entry(order, 0x0001, 14, 1, [0; 4]), // unicode: named, not accepted
                int16u_entry(order, 0x0002, 7),
            ],
            &[],
        );
        assert!(
            run(&PLAIN, &data, order, Some(0)).is_empty(),
            "Exif.pm:6477 `return 0` on a bad first entry"
        );
        let mut members = HashMap::new();
        members.insert("Model", MemberValue::Str("ILCE-7M4".to_string()));
        assert_eq!(
            values(&run_with(&PLAIN, &data, order, Some(0), &mut members)),
            vec![("Raw", TagValue::Integer(7))],
            "Exif.pm:6475: Sony ILCE bodies get `next` instead"
        );
        // Not the first entry: `next`, the rest of the directory is read.
        let data = ifd(
            order,
            &[
                int16u_entry(order, 0x0002, 7),
                entry(order, 0x0001, 17, 1, [0; 4]), // int64s: never accepted
            ],
            &[],
        );
        assert_eq!(
            values(&run(&PLAIN, &data, order, Some(0))),
            vec![("Raw", TagValue::Integer(7))]
        );
    }

    #[test]
    fn int64u_entries_are_accepted_only_inside_apple_maker_notes() {
        let order = ByteOrder::Big;
        let floor = trailer_at(2) as u32;
        let data = ifd(
            order,
            &[
                int16u_entry(order, 0x0002, 7),
                entry(order, 0x0001, 16, 1, bytes32(order, floor)),
            ],
            &[0, 0, 0, 0, 0, 0, 0, 9],
        );
        assert_eq!(
            values(&run(&PLAIN, &data, order, Some(0))),
            vec![("Raw", TagValue::Integer(7))],
            "no Make member: code 16 is a bad format (Exif.pm:6463)"
        );
        let mut members = HashMap::new();
        members.insert("Make", MemberValue::Str("Apple".to_string()));
        assert_eq!(
            values(&run_with(&PLAIN, &data, order, Some(0), &mut members)),
            vec![
                ("Raw", TagValue::Integer(7)),
                // 9 misses PLAIN's IntEnum: ExifTool's `Unknown (9)` (4b-i).
                ("Mode", TagValue::String("Unknown (9)".to_string()))
            ]
        );
    }

    #[test]
    fn more_than_ten_warnings_abort_the_rest_of_the_directory() {
        // Exif.pm:6455-6457. Eleven out-of-range offsets, then a good inline
        // entry ExifTool would never reach.
        let order = ByteOrder::Big;
        let mut entries = Vec::new();
        for _ in 0..11 {
            entries.push(entry(order, 0x0001, 2, 5, bytes32(order, 0xffff_0000)));
        }
        entries.push(entry(order, 0x0003, 7, 1, [0x2a, 0, 0, 0]));
        let data = ifd(order, &entries, &[]);
        assert!(run(&STRINGS, &data, order, Some(0)).is_empty());
        // Ten warnings do not.
        let data = ifd(order, &entries[1..], &[]);
        assert_eq!(
            values(&run(&STRINGS, &data, order, Some(0))),
            vec![("OneByte", TagValue::Integer(42))]
        );
    }

    // -- _variants (Exif.pm:6719-6720, ExifTool.pm:9164-9188) ---------------------

    const FMT_IS_UNDEF: Cond = Cond::FormatEq {
        value: "undef",
        negate: false,
    };
    const COUNT_GT_2: Cond = Cond::CountCmp {
        op: CmpOp::Gt,
        value: 2,
    };
    static VARIANT_GROUPS: &[IfdVariantGroup] = &[IfdVariantGroup {
        id: 0x0010,
        alternatives: &[
            (FMT_IS_UNDEF, plain(0x0010, "Bytes")),
            (COUNT_GT_2, plain(0x0010, "Many")),
            (Cond::Always, plain(0x0010, "Other")),
        ],
    }];
    static VARIANTS: IfdTable = IfdTable {
        variants: VARIANT_GROUPS,
        ..table("Variants", &[])
    };

    #[test]
    fn a_variant_group_resolves_by_format_and_by_count() {
        let order = ByteOrder::Big;
        // undef x2 -> `$format eq "undef"` wins first.
        let data = ifd(order, &[entry(order, 0x0010, 7, 2, [1, 2, 0, 0])], &[]);
        assert_eq!(
            values(&run(&VARIANTS, &data, order, Some(0))),
            vec![("Bytes", TagValue::Binary(vec![1, 2]))]
        );
        // int8u x3 -> `$count > 2` wins.
        let data = ifd(order, &[entry(order, 0x0010, 1, 3, [1, 2, 3, 0])], &[]);
        assert_eq!(
            values(&run(&VARIANTS, &data, order, Some(0))),
            vec![("Many", TagValue::String("1 2 3".to_string()))]
        );
        // int16u x1 -> neither; the unconditional alternative.
        let data = ifd(order, &[int16u_entry(order, 0x0010, 5)], &[]);
        assert_eq!(
            values(&run(&VARIANTS, &data, order, Some(0))),
            vec![("Other", TagValue::Integer(5))]
        );
    }

    // -- the entry's count reaching a CountCmp variant (slice I-3) -----------------

    static E1_OR_EM5: Cond = Cond::MemberRegex {
        member: "Model",
        pattern: r"E-(1|M5)\b",
        ignore_case: false,
        negate: false,
    };
    static COUNT_NE_1: Cond = Cond::CountCmp {
        op: CmpOp::Ne,
        value: 1,
    };
    static COUNT_GROUPS: &[IfdVariantGroup] = &[IfdVariantGroup {
        id: 0x1500,
        alternatives: &[
            (
                Cond::Or(&E1_OR_EM5, &COUNT_NE_1),
                plain(0x1500, "SensorTemperatureRaw"),
            ),
            (Cond::Always, plain(0x1500, "SensorTemperatureCalibrated")),
        ],
    }];
    static COUNTS: IfdTable = IfdTable {
        variants: COUNT_GROUPS,
        ..table("Counts", &[])
    };

    #[test]
    fn the_entry_count_is_in_scope_for_a_variant_condition() {
        // Olympus.pm:3580-3590 (FocusInfo 0x1500: `$$self{Model} =~
        // /E-(1|M5)\b/ || $count != 1`) through Exif.pm:6719-6720, which
        // hands `GetTagInfo` the entry's own count (Exif.pm:6461).
        let order = ByteOrder::Little;
        let names = |got: &[Emitted]| {
            values(got)
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
        };

        let mut members = HashMap::new();
        members.insert("Model", MemberValue::Str("E-510".to_string()));
        let one = ifd(order, &[int16u_entry(order, 0x1500, 534)], &[]);
        assert_eq!(
            names(&run_with(&COUNTS, &one, order, Some(0), &mut members)),
            vec!["SensorTemperatureCalibrated"],
            "E-510, count 1: the model regex misses and `$count != 1` is false"
        );
        // int16u[2] still fits the 4-byte value field, so the count is the
        // only thing that differs from the entry above.
        let mut inline = [0u8; 4];
        inline[..2].copy_from_slice(&bytes16(order, 34));
        let two = ifd(order, &[entry(order, 0x1500, 3, 2, inline)], &[]);
        assert_eq!(
            names(&run_with(&COUNTS, &two, order, Some(0), &mut members)),
            vec!["SensorTemperatureRaw"],
            "E-510, count 2: `$count != 1` selects the first alternative"
        );
        members.insert("Model", MemberValue::Str("E-M5".to_string()));
        assert_eq!(
            names(&run_with(&COUNTS, &one, order, Some(0), &mut members)),
            vec!["SensorTemperatureRaw"],
            "E-M5, count 1: the model regex alone selects the first alternative"
        );
    }

    // -- RawConv SetMember feeding a later MemberCmp variant -----------------------

    const VER_IS_2: Cond = Cond::MemberCmp {
        member: "TestVersion",
        op: CmpOp::Eq,
        value: 2,
    };
    static MEMBER_GROUPS: &[IfdVariantGroup] = &[IfdVariantGroup {
        id: 0x0002,
        alternatives: &[
            (VER_IS_2, plain(0x0002, "SettingsV2")),
            (Cond::Always, plain(0x0002, "SettingsV1")),
        ],
    }];
    static MEMBER_TAGS: &[IfdTag] = &[IfdTag {
        raw_conv: Some(RawConvEffect::SetMember {
            member: "TestVersion",
        }),
        ..plain(0x0001, "Version")
    }];
    static MEMBERS: IfdTable = IfdTable {
        variants: MEMBER_GROUPS,
        ..table("Members", MEMBER_TAGS)
    };

    #[test]
    fn a_set_member_raw_conv_is_stored_reported_and_read_by_a_later_condition() {
        let order = ByteOrder::Little;
        let data = ifd(
            order,
            &[
                int16u_entry(order, 0x0001, 2),
                int16u_entry(order, 0x0002, 77),
            ],
            &[],
        );
        let mut members = HashMap::new();
        let got = run_with(&MEMBERS, &data, order, Some(0), &mut members);
        assert_eq!(
            values(&got),
            vec![
                ("Version", TagValue::Integer(2)),
                ("SettingsV2", TagValue::Integer(77)),
            ],
            "ExifTool.pm:9500: the assignment's value is `$val`, so the tag is still reported"
        );
        assert_eq!(members.get("TestVersion"), Some(&MemberValue::Num(2)));

        let data = ifd(
            order,
            &[
                int16u_entry(order, 0x0001, 1),
                int16u_entry(order, 0x0002, 77),
            ],
            &[],
        );
        assert_eq!(
            values(&run(&MEMBERS, &data, order, Some(0))),
            vec![
                ("Version", TagValue::Integer(1)),
                ("SettingsV1", TagValue::Integer(77)),
            ]
        );
    }

    // -- Flags and omissions -----------------------------------------------------

    static FLAG_TAGS: &[IfdTag] = &[
        IfdTag {
            flags: IfdFlags {
                binary: true,
                ..IfdFlags::NONE
            },
            print_conv: PrintConv::IntEnum(&[(1, "never rendered")]),
            ..plain(0x0001, "Blob")
        },
        IfdTag {
            flags: IfdFlags {
                unknown: true,
                ..IfdFlags::NONE
            },
            ..plain(0x0002, "Unknown")
        },
        IfdTag {
            omitted: Omitted {
                raw_conv: true,
                ..Omitted::NONE
            },
            ..plain(0x0003, "RawConvRefused")
        },
        IfdTag {
            flags: IfdFlags {
                binary: true,
                ..IfdFlags::NONE
            },
            ..plain(0x0004, "BinaryText")
        },
        IfdTag {
            flags: IfdFlags {
                binary: true,
                ..IfdFlags::NONE
            },
            ..plain(0x0005, "BinaryNumbers")
        },
        IfdTag {
            flags: IfdFlags {
                avoid: true,
                ..IfdFlags::NONE
            },
            ..plain(0x0006, "Avoided")
        },
        IfdTag {
            flags: IfdFlags {
                priority: Some(1),
                ..IfdFlags::NONE
            },
            ..plain(0x0007, "Insistent")
        },
        IfdTag {
            omitted: Omitted {
                condition: true,
                ..Omitted::NONE
            },
            ..plain(0x0008, "Conditional")
        },
    ];
    static FLAGS: IfdTable = IfdTable {
        priority: Some(0),
        ..table("Flags", FLAG_TAGS)
    };

    #[test]
    fn binary_reports_the_placeholder_with_perls_length_of_the_value() {
        let order = ByteOrder::Big;
        let floor = trailer_at(3) as u32;
        let data = ifd(
            order,
            &[
                entry(order, 0x0001, 7, 6, bytes32(order, floor)),
                // string x6 = "AB\0XYZ": NUL-truncated, so length 2.
                entry(order, 0x0004, 2, 6, bytes32(order, floor)),
                // int16u x3 = "1 2 3": length 5, not 6 bytes.
                entry(order, 0x0005, 3, 3, bytes32(order, floor + 6)),
            ],
            b"AB\0XYZ\0\x01\0\x02\0\x03",
        );
        let got = run(&FLAGS, &data, order, Some(0));
        assert_eq!(
            values(&got),
            vec![
                (
                    "Blob",
                    TagValue::String("(Binary data 6 bytes, use -b option to extract)".to_string())
                ),
                (
                    "BinaryText",
                    TagValue::String("(Binary data 2 bytes, use -b option to extract)".to_string())
                ),
                (
                    "BinaryNumbers",
                    TagValue::String("(Binary data 5 bytes, use -b option to extract)".to_string())
                ),
            ]
        );
    }

    #[test]
    fn unknown_omitted_raw_conv_and_unresolved_condition_tags_are_withheld() {
        let order = ByteOrder::Big;
        let data = ifd(
            order,
            &[
                int16u_entry(order, 0x0002, 1),
                int16u_entry(order, 0x0003, 1),
                int16u_entry(order, 0x0008, 1),
            ],
            &[],
        );
        assert!(run(&FLAGS, &data, order, Some(0)).is_empty());
    }

    #[test]
    fn priority_follows_found_tags_precedence_and_avoid_is_carried() {
        // ExifTool.pm:9469-9473: the tag's own Priority beats the table's
        // PRIORITY => 0; Avoid rides along as a flag.
        let order = ByteOrder::Big;
        let data = ifd(
            order,
            &[
                int16u_entry(order, 0x0006, 1),
                int16u_entry(order, 0x0007, 1),
            ],
            &[],
        );
        let got = run(&FLAGS, &data, order, Some(0));
        assert_eq!(got.len(), 2);
        assert!(got[0].low_priority, "table PRIORITY => 0");
        assert!(got[0].avoid);
        assert!(!got[1].low_priority, "Priority => 1 on the tag wins");
        assert!(!got[1].avoid);
    }

    #[test]
    fn avoid_alone_defaults_the_priority_to_zero() {
        // ExifTool.pm:9472: `$priority = 0 if not defined $priority and Avoid`.
        static AVOID_TAGS: &[IfdTag] = &[IfdTag {
            flags: IfdFlags {
                avoid: true,
                ..IfdFlags::NONE
            },
            ..plain(0x0001, "Avoided")
        }];
        static AVOID: IfdTable = table("Avoid", AVOID_TAGS);
        let order = ByteOrder::Big;
        let data = ifd(order, &[int16u_entry(order, 0x0001, 1)], &[]);
        let got = run(&AVOID, &data, order, Some(0));
        assert!(got[0].low_priority && got[0].avoid);
    }

    // -- Group 1 precedence ------------------------------------------------------

    static GROUP_TAGS: &[IfdTag] = &[
        IfdTag {
            groups: TagGroups {
                g0: None,
                g1: Some("TagOwn"),
                g2: None,
            },
            ..plain(0x0001, "WithOwn")
        },
        plain(0x0002, "Plain"),
    ];
    static GROUPS: IfdTable = table("Groups", GROUP_TAGS);
    static SET_GROUP1: IfdTable = IfdTable {
        // Exif.pm:416 declares `SET_GROUP1 => 1`; the payload is a flag.
        set_group1: Some("1"),
        ..table("SetGroup1", GROUP_TAGS)
    };

    fn run_named(
        table: &'static IfdTable,
        data: &[u8],
        group1: Option<&'static str>,
    ) -> Vec<Emitted> {
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut out = Vec::new();
        process_exif(
            table,
            IfdDir {
                data,
                ifd_start: 0,
                base: Some(0),
                byte_order: ByteOrder::Big,
                group1,
            },
            &mut ctx,
            &mut out,
        );
        out
    }

    #[test]
    fn group1_is_the_tags_own_else_the_tables_unless_set_group1_names_the_directory() {
        let order = ByteOrder::Big;
        let data = ifd(
            order,
            &[
                int16u_entry(order, 0x0001, 1),
                int16u_entry(order, 0x0002, 1),
            ],
            &[],
        );
        // No SET_GROUP1: the directory name plays no part (ExifTool.pm:3860
        // reads only TAG_EXTRA{G1}, which nothing set).
        let got = run_named(&GROUPS, &data, Some("DirName"));
        assert_eq!(got[0].group1, "TagOwn");
        assert_eq!(got[1].group1, "Test");
        // SET_GROUP1 (Exif.pm:7183): the directory name for every tag, over
        // the tag's own Groups{1}.
        let got = run_named(&SET_GROUP1, &data, Some("IFD0"));
        assert_eq!(got[0].group1, "IFD0");
        assert_eq!(got[1].group1, "IFD0");
        // ... and with no directory name to set, nothing can be reported
        // faithfully.
        assert!(run_named(&SET_GROUP1, &data, None).is_empty());
    }

    // -- SubDirectory: ValuePtr into an IFD table, gated ---------------------------

    static INNER_TAGS: &[IfdTag] = &[plain(0x0001, "Inner")];
    static INNER_OFF: IfdTable = table("InnerOff", INNER_TAGS);
    static INNER_ON: IfdTable = table("InnerOn", INNER_TAGS);
    static OUTER_TAGS: &[IfdTag] = &[
        IfdTag {
            subdir: Some(edge("InnerOff")),
            ..plain(0x0010, "ToOff")
        },
        IfdTag {
            subdir: Some(IfdSubdirEdge {
                dir_name: Some("InnerDir"),
                ..edge("InnerOn")
            }),
            ..plain(0x0011, "ToOn")
        },
        plain(0x0012, "After"),
    ];
    static OUTER: IfdTable = table("Outer", OUTER_TAGS);

    /// A nested directory holding one `Inner` int16u tag, as the trailer.
    fn inner_ifd(order: ByteOrder, v: u16) -> Vec<u8> {
        ifd(order, &[int16u_entry(order, 0x0001, v)], &[])
    }

    #[test]
    fn a_value_ptr_edge_walks_an_enabled_ifd_table_and_refuses_a_disabled_one() {
        let order = ByteOrder::Big;
        let floor = trailer_at(3) as u32;
        let inner = inner_ifd(order, 5);
        let data = ifd(
            order,
            &[
                entry(order, 0x0010, 7, inner.len() as u32, bytes32(order, floor)),
                entry(order, 0x0011, 7, inner.len() as u32, bytes32(order, floor)),
                int16u_entry(order, 0x0012, 9),
            ],
            &inner,
        );
        assert!(!INNER_ON.enabled() && !INNER_OFF.enabled());
        let _reg = Registered::new(&[&INNER_ON]);
        let got = run(&OUTER, &data, order, Some(0));
        assert_eq!(
            values(&got),
            vec![
                ("Inner", TagValue::Integer(5)),
                ("After", TagValue::Integer(9))
            ],
            "InnerOff is not enabled and not walked; InnerOn is walked once; the \
             SubDirectory tags themselves are never reported"
        );
        assert_eq!(got[0].module, "Test");
        assert_eq!(got[0].table, "InnerOn");
        assert_eq!(
            got[0].group1, "Test",
            "no SET_GROUP1, so the DirName stays out of it"
        );
    }

    #[test]
    fn without_the_test_registry_nothing_is_walkable() {
        let order = ByteOrder::Big;
        let floor = trailer_at(1) as u32;
        let inner = inner_ifd(order, 5);
        let data = ifd(
            order,
            &[entry(
                order,
                0x0011,
                7,
                inner.len() as u32,
                bytes32(order, floor),
            )],
            &inner,
        );
        assert!(run(&OUTER, &data, order, Some(0)).is_empty());
    }

    #[test]
    fn a_validate_edge_is_never_walked() {
        static V_TAGS: &[IfdTag] = &[IfdTag {
            subdir: Some(IfdSubdirEdge {
                validate: true,
                ..edge("InnerOn")
            }),
            ..plain(0x0011, "ToOn")
        }];
        static V: IfdTable = table("Validated", V_TAGS);
        let order = ByteOrder::Big;
        let floor = trailer_at(1) as u32;
        let inner = inner_ifd(order, 5);
        let data = ifd(
            order,
            &[entry(
                order,
                0x0011,
                7,
                inner.len() as u32,
                bytes32(order, floor),
            )],
            &inner,
        );
        let _reg = Registered::new(&[&INNER_ON]);
        assert!(run(&V, &data, order, Some(0)).is_empty());
    }

    #[test]
    fn an_unwalked_edge_is_never_walked() {
        // Slice IFD1: the target is registered and walkable -- only the
        // `unwalked` marker stops the descent, the way `validate` does.
        static U_TAGS: &[IfdTag] = &[
            IfdTag {
                subdir: Some(IfdSubdirEdge {
                    unwalked: Some("same-table recursion (TagTable absent)"),
                    ..edge("InnerOn")
                }),
                ..plain(0x0011, "ToOn")
            },
            IfdTag {
                subdir: Some(IfdSubdirEdge {
                    unwalked: Some("ProcessProc Image::ExifTool::ProcessSubTIFF"),
                    ..edge("InnerOn")
                }),
                ..plain(0x0012, "ToOn2")
            },
            plain(0x0013, "After"),
        ];
        static U: IfdTable = table("Unwalked", U_TAGS);
        let order = ByteOrder::Big;
        let floor = trailer_at(3) as u32;
        let inner = inner_ifd(order, 5);
        let data = ifd(
            order,
            &[
                entry(order, 0x0011, 7, inner.len() as u32, bytes32(order, floor)),
                entry(order, 0x0012, 7, inner.len() as u32, bytes32(order, floor)),
                int16u_entry(order, 0x0013, 9),
            ],
            &inner,
        );
        let _reg = Registered::new(&[&INNER_ON]);
        let got = run(&U, &data, order, Some(0));
        assert_eq!(
            values(&got),
            vec![("After", TagValue::Integer(9))],
            "both unwalked edges mark their place and read nothing; the walk continues"
        );
        // The same edge with the marker cleared IS walked: the marker, not
        // the shape, is what refuses.
        static W_TAGS: &[IfdTag] = &[IfdTag {
            subdir: Some(edge("InnerOn")),
            ..plain(0x0011, "ToOn")
        }];
        static W: IfdTable = table("Walked", W_TAGS);
        let got = run(&W, &data, order, Some(0));
        assert_eq!(values(&got), vec![("Inner", TagValue::Integer(5))]);
    }

    // -- SubDirectory: SubIFD pointers, byte order, base ---------------------------

    static SUBIFD_TAGS: &[IfdTag] = &[
        IfdTag {
            subdir: Some(IfdSubdirEdge {
                start: IfdStart::Val(0),
                sub_ifd: true,
                ..edge("InnerOn")
            }),
            ..plain(0x0020, "SubIFD")
        },
        IfdTag {
            subdir: Some(IfdSubdirEdge {
                start: IfdStart::Val(0),
                sub_ifd: true,
                max_subdirs: Some(2),
                ..edge("InnerOn")
            }),
            ..plain(0x0021, "SubIFDs")
        },
        IfdTag {
            subdir: Some(IfdSubdirEdge {
                byte_order: IfdByteOrder::Unknown,
                ..edge("InnerOn")
            }),
            ..plain(0x0022, "Detected")
        },
        IfdTag {
            subdir: Some(IfdSubdirEdge {
                start: IfdStart::Val(0),
                // No SubIFD flag: `$val` is undef bytes (Exif.pm:6733).
                ..edge("InnerOn")
            }),
            ..plain(0x0023, "ValWithoutSubIfd")
        },
    ];
    static SUBIFD: IfdTable = table("SubIfd", SUBIFD_TAGS);

    #[test]
    fn a_val_edge_follows_the_pointer_through_the_base_correction() {
        let order = ByteOrder::Little;
        let floor = trailer_at(1) as u32;
        let inner = inner_ifd(order, 6);
        // The pointer is TIFF-relative: 100 bytes before our buffer.
        let data = ifd(
            order,
            &[entry(order, 0x0020, 4, 1, bytes32(order, floor + 100))],
            &inner,
        );
        let _reg = Registered::new(&[&INNER_ON]);
        assert_eq!(
            values(&run(&SUBIFD, &data, order, Some(-100))),
            vec![("Inner", TagValue::Integer(6))]
        );
        assert!(
            run(&SUBIFD, &data, order, None).is_empty(),
            "a pointer cannot be followed without a correction"
        );
        // Exif.pm:6747: a SubIFD pointer in a non-integer format is skipped.
        let data = ifd(
            order,
            &[entry(order, 0x0020, 11, 1, bytes32(order, floor))],
            &inner,
        );
        assert!(run(&SUBIFD, &data, order, Some(0)).is_empty());
    }

    #[test]
    fn a_sub_ifd_loop_walks_each_offset_up_to_max_subdirs() {
        let order = ByteOrder::Big;
        let floor = trailer_at(1) as u32;
        let first = inner_ifd(order, 1);
        let second = inner_ifd(order, 2);
        let third = inner_ifd(order, 3);
        let step = first.len() as u32;
        let mut trailer = Vec::new();
        // Three pointers (12 bytes, out of line), then three directories.
        let ptrs_len = 12u32;
        for i in 0..3u32 {
            trailer.extend_from_slice(&bytes32(order, floor + ptrs_len + i * step));
        }
        trailer.extend_from_slice(&first);
        trailer.extend_from_slice(&second);
        trailer.extend_from_slice(&third);
        let _reg = Registered::new(&[&INNER_ON]);
        // MaxSubdirs => 2: the third is "Ignoring 1 SubIFDs directories".
        let data = ifd(
            order,
            &[entry(order, 0x0021, 4, 3, bytes32(order, floor))],
            &trailer,
        );
        assert_eq!(
            values(&run(&SUBIFD, &data, order, Some(0))),
            vec![
                ("Inner", TagValue::Integer(1)),
                ("Inner", TagValue::Integer(2))
            ]
        );
        // No MaxSubdirs and three values: `eval('$val')` is "a b c", IsInt
        // fails (Exif.pm:6957-6959), nothing is walked.
        let data = ifd(
            order,
            &[entry(order, 0x0020, 4, 3, bytes32(order, floor))],
            &trailer,
        );
        assert!(run(&SUBIFD, &data, order, Some(0)).is_empty());
    }

    #[test]
    fn byte_order_unknown_flips_on_an_implausible_entry_count() {
        // Outer directory little-endian; the nested one big-endian with one
        // entry: its count reads as 0x0100 in LE, high byte set and larger
        // than the low byte (Exif.pm:6987), so the walk flips to BE.
        let order = ByteOrder::Little;
        let floor = trailer_at(1) as u32;
        let inner = inner_ifd(ByteOrder::Big, 0x0102);
        let data = ifd(
            order,
            &[entry(
                order,
                0x0022,
                7,
                inner.len() as u32,
                bytes32(order, floor),
            )],
            &inner,
        );
        let _reg = Registered::new(&[&INNER_ON]);
        assert_eq!(
            values(&run(&SUBIFD, &data, order, Some(0))),
            vec![("Inner", TagValue::Integer(0x0102))]
        );
        // A plausible count in the enclosing order stays in it.
        let inner = inner_ifd(ByteOrder::Little, 0x0102);
        let data = ifd(
            order,
            &[entry(
                order,
                0x0022,
                7,
                inner.len() as u32,
                bytes32(order, floor),
            )],
            &inner,
        );
        assert_eq!(
            values(&run(&SUBIFD, &data, order, Some(0))),
            vec![("Inner", TagValue::Integer(0x0102))]
        );
    }

    #[test]
    fn a_val_edge_without_sub_ifd_is_refused() {
        let order = ByteOrder::Big;
        let floor = trailer_at(1) as u32;
        let inner = inner_ifd(order, 6);
        let data = ifd(
            order,
            &[entry(order, 0x0023, 4, 1, bytes32(order, floor))],
            &inner,
        );
        let _reg = Registered::new(&[&INNER_ON]);
        assert!(run(&SUBIFD, &data, order, Some(0)).is_empty());
    }

    #[test]
    fn a_base_expression_moves_the_correction_for_the_nested_directory() {
        // `Base => '$start'` (the shape Olympus's binary MovableInfo uses):
        // the nested directory's stored offsets become relative to its own
        // start. The nested IFD holds one out-of-line string at "offset 0 +
        // 18" relative to itself.
        static BASE_INNER_TAGS: &[IfdTag] = &[plain(0x0001, "Name")];
        static BASE_INNER: IfdTable = table("BaseInner", BASE_INNER_TAGS);
        static START: BaseExpr = BaseExpr::Start;
        static BASE_OUTER_TAGS: &[IfdTag] = &[IfdTag {
            subdir: Some(IfdSubdirEdge {
                base: Some(&START),
                ..edge("BaseInner")
            }),
            ..plain(0x0010, "Nested")
        }];
        static BASE_OUTER: IfdTable = table("BaseOuter", BASE_OUTER_TAGS);

        let order = ByteOrder::Big;
        let floor = trailer_at(1) as u32;
        // Nested: one string x5 at self-relative offset 18 (= its own floor).
        let inner = ifd(
            order,
            &[entry(order, 0x0001, 2, 5, bytes32(order, 18))],
            b"HELLO",
        );
        let data = ifd(
            order,
            &[entry(
                order,
                0x0010,
                7,
                inner.len() as u32,
                bytes32(order, floor),
            )],
            &inner,
        );
        let _reg = Registered::new(&[&BASE_INNER]);
        assert_eq!(
            values(&run(&BASE_OUTER, &data, order, Some(0))),
            vec![("Name", TagValue::String("HELLO".to_string()))]
        );
    }

    // -- The guard ------------------------------------------------------------

    static SELF_TAGS: &[IfdTag] = &[
        IfdTag {
            subdir: Some(IfdSubdirEdge {
                start: IfdStart::Val(0),
                sub_ifd: true,
                ..edge("Cyclic")
            }),
            ..plain(0x0001, "Next")
        },
        plain(0x0002, "Level"),
    ];
    static CYCLIC: IfdTable = table("Cyclic", SELF_TAGS);

    #[test]
    fn a_table_pointing_at_itself_is_stopped_by_the_guard() {
        // The directory's SubIFD pointer is its own start.
        let order = ByteOrder::Big;
        let data = ifd(
            order,
            &[
                entry(order, 0x0001, 4, 1, bytes32(order, 0)),
                int16u_entry(order, 0x0002, 1),
            ],
            &[],
        );
        let _reg = Registered::new(&[&CYCLIC]);
        assert_eq!(
            values(&run(&CYCLIC, &data, order, Some(0))),
            vec![("Level", TagValue::Integer(1))],
            "ExifTool.pm:9067: the second visit to the same address is refused"
        );
    }

    #[test]
    fn a_chain_of_distinct_directories_is_cut_at_the_depth_cap() {
        // Twelve directories each pointing at the next: the root plus
        // MAX_SUBDIR_DEPTH nested ones are walked.
        let order = ByteOrder::Big;
        let block = ifd(
            order,
            &[
                entry(order, 0x0001, 4, 1, [0; 4]),
                int16u_entry(order, 0x0002, 1),
            ],
            &[],
        );
        let step = block.len() as u32;
        let mut data = Vec::new();
        for i in 0..12u32 {
            let mut b = block.clone();
            // Patch the pointer to the next block.
            b[2 + 8..2 + 12].copy_from_slice(&bytes32(order, (i + 1) * step));
            data.extend_from_slice(&b);
        }
        let _reg = Registered::new(&[&CYCLIC]);
        let got = run(&CYCLIC, &data, order, Some(0));
        assert_eq!(
            got.len(),
            1 + engine::MAX_SUBDIR_DEPTH as usize,
            "root + MAX_SUBDIR_DEPTH nested directories"
        );
    }

    // -- ValueConv suppression and a binary-table target ---------------------------

    #[test]
    fn an_omitted_value_conv_withholds_the_tag() {
        // `Omitted::value_conv` is the schema's spelling of "the generator
        // refused this ValueConv"; reporting the raw value under the tag's
        // name would be the confident-wrong-value failure AGENTS.md names.
        static VC_TAGS: &[IfdTag] = &[IfdTag {
            omitted: Omitted {
                value_conv: true,
                ..Omitted::NONE
            },
            ..plain(0x0001, "Refused")
        }];
        static VC: IfdTable = table("ValueConv", VC_TAGS);
        let order = ByteOrder::Big;
        let data = ifd(order, &[int16u_entry(order, 0x0001, 4)], &[]);
        assert!(run(&VC, &data, order, Some(0)).is_empty());
    }

    #[test]
    fn an_edge_to_a_binary_table_that_is_not_enabled_is_refused() {
        // `Canon::CameraSettings` is a real generated ProcessBinaryData table
        // that is not on the Step 28 allowlist; the IFD engine must refuse it
        // exactly as `engine::descend` would.
        static TO_BINARY_TAGS: &[IfdTag] = &[IfdTag {
            subdir: Some(IfdSubdirEdge {
                module: "Canon",
                ..edge("CameraSettings")
            }),
            ..plain(0x0001, "CanonCameraSettings")
        }];
        static TO_BINARY: IfdTable = table("ToBinary", TO_BINARY_TAGS);
        let target = find_table("Canon", "CameraSettings").expect("generated");
        assert!(!target.enabled());
        let order = ByteOrder::Big;
        let floor = trailer_at(1) as u32;
        let data = ifd(
            order,
            &[entry(order, 0x0001, 3, 8, bytes32(order, floor))],
            &[0u8; 16],
        );
        assert!(run(&TO_BINARY, &data, order, Some(0)).is_empty());
    }

    // -- Helpers pinned on their own -----------------------------------------------

    #[test]
    fn ifd_perl_string_renders_elements_as_read_value_would() {
        assert_eq!(
            ifd_perl_string(&DecodedValue::Integer(-7)).as_deref(),
            Some("-7")
        );
        assert_eq!(
            ifd_perl_string(&DecodedValue::UnsignedRational(1, 3)).as_deref(),
            Some("0.3333333333")
        );
        assert_eq!(
            ifd_perl_string(&DecodedValue::SignedRational(-1, 2)).as_deref(),
            Some("-0.5")
        );
        assert_eq!(
            ifd_perl_string(&DecodedValue::UnsignedRational(4, 0)).as_deref(),
            Some("inf")
        );
        assert_eq!(
            ifd_perl_string(&DecodedValue::UnsignedRational(0, 0)).as_deref(),
            Some("undef")
        );
        assert_eq!(
            ifd_perl_string(&DecodedValue::Float(1.0 / 3.0)).as_deref(),
            Some("0.333333333333333")
        );
        assert_eq!(ifd_perl_string(&DecodedValue::Undefined(vec![1])), None);
        // The shared rule refuses a rational (width unknown there); the IFD
        // rule above is the one that knows the width.
        assert_eq!(DecodedValue::UnsignedRational(1, 3).perl_string(), None);
    }

    #[test]
    fn detect_byte_order_follows_exif_pm_6987_exactly() {
        // 0x0100 read big-endian: high byte 1 > low byte 0 -> flip.
        assert_eq!(
            detect_byte_order(&[1, 0], 0, ByteOrder::Big),
            Some(ByteOrder::Little)
        );
        // 0x012C (300): high byte 1, low byte 0x2C -> keep, even though 300
        // is an implausible count -- the rule is the byte comparison, not a
        // size threshold.
        assert_eq!(
            detect_byte_order(&be16(0x012c), 0, ByteOrder::Big),
            Some(ByteOrder::Big)
        );
        // 0: no high byte -> keep.
        assert_eq!(
            detect_byte_order(&[0, 0], 0, ByteOrder::Big),
            Some(ByteOrder::Big)
        );
        assert_eq!(detect_byte_order(&[0], 0, ByteOrder::Big), None);
    }
}
