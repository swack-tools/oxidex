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
//! Exif.pm:6539       $valuePtr < 8 and not $$dirInfo{ZeroOffsetOK} and $suspect = $warnCount;
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
//!   bytes. It remains unwalked unless code generation authenticated the
//!   closed U16-size helper and native reader contract for a serial target.
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
//! # Which out-of-line values are "inside the directory"
//!
//! ExifTool refuses two kinds of out-of-line value as "Suspicious" --
//! warned and skipped (Exif.pm:6673-6678): one whose stored offset points
//! into the 8-byte TIFF header, `$valuePtr < 8 and not
//! $$dirInfo{ZeroOffsetOK}` (Exif.pm:6539, tested on the offset as stored,
//! before the `$dataPos` correction), and one that OVERLAPS the directory's
//! entry array, `$valuePtr < $dirEnd and $valuePtr+$size > $dirStart`
//! (Exif.pm:6549). Any other value that lies entirely before the directory
//! is read. [`DirectoryRule::Overlap`] carries both checks for every table
//! whose `GROUPS{0}` is not `MakerNotes` (`Exif::Main`, walked at IFD1,
//! ExifIFD and InteropIFD); `ZeroOffsetOK` is set by one caller in the
//! pinned tree, Samsung.pm:1708 (`ProcessSamsungIFD`, a maker-note
//! directory), never for an `Exif::Main` directory, so the header check is
//! unconditional here. Writers that put the ExifIFD after its value block
//! (SonyILCE-7CM2, OlympusE-M10MarkIV) store 22-30 values before the
//! directory, past the header, and ExifTool reads every one (construct K-O
//! of the `exif-ifd` slice spec).
//!
//! A `MakerNotes` table keeps `table_ifd.rs`'s floor: a value that starts
//! anywhere before the end of the directory's next-IFD link is refused.
//! ExifTool applies the overlap rule there too (`$inMakerNotes` only makes
//! the warning minor), so the floor withholds a strict superset of what
//! the overlap rule withholds; it also covers the header check whenever
//! the directory does not lie before `$base` (an offset below 8 then
//! starts before the directory), which only `ZeroOffsetOK`'s Samsung
//! directory breaks. It only ever withholds, never invents, and it is the
//! hardening the maker-note ports were measured with (Olympus, Canon,
//! FujiFilm). Widening the overlap rule to them is its own measured change.

use crate::core::TagValue;
use crate::io::ByteOrder;

use super::cond::{self, MemberValue};
use super::conv::{self, Arm};
use super::enabled_serial;
use super::engine::{self, Dir, Emitted};
use super::exprs;
use super::ifd_schema::{
    IfdByteOrder, IfdStart, IfdSubdirEdge, IfdSubdirProcessor, IfdTable, IfdTag, RawConvEffect,
};
use super::runtime::{self, DecodedValue, decode_value_of};
use super::session::{ByteOrder as SessionByteOrder, MemberVal, Session};
use super::subdir::BaseExpr;
use super::{
    Fmt, Omitted, SerialDir, SerialEmissionSink, SerialTable, SerialWalkResult, find_ifd_table,
    find_serial_table, find_table, process_serial_directory,
};

#[path = "subdirectory_adapter.rs"]
pub mod subdirectory_adapter;

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
    /// Stable identity of the enclosing file/data domain.
    /// Separate from [`Self::base`]: it identifies buffers for the processed
    /// directory guard but never participates in stored-offset correction.
    pub data_domain: u64,
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
/// refused ([`DirectoryRule::Floor`]).
fn directory_floor(ifd_start: usize, entries: usize) -> usize {
    directory_end(ifd_start, entries) + 4
}

/// `$dirEnd` (Exif.pm:6347-6348): the first byte after the entry array.
fn directory_end(ifd_start: usize, entries: usize) -> usize {
    ifd_start + 2 + 12 * entries
}

/// When an out-of-line value is "inside the directory", i.e. refused as
/// "Suspicious" (Exif.pm:6673-6678: warned, `++$warnCount`, skipped). See
/// the module doc for why the two rules coexist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DirectoryRule {
    /// ExifTool's own rules: refused iff the stored offset points into the
    /// TIFF header (Exif.pm:6539, `$valuePtr < 8`; no `Exif::Main`
    /// directory sets `ZeroOffsetOK`) or the value overlaps the entry array
    /// (Exif.pm:6549, `start < dir_end && start + size > dir_start`). Any
    /// other value entirely before the directory, or starting at or after
    /// `$dirEnd` (the next-IFD link included), is read.
    Overlap { dir_start: usize, dir_end: usize },
    /// `table_ifd.rs`'s floor ([`directory_floor`]), kept for `MakerNotes`
    /// tables (decision D-4 of the `exif-ifd` slice spec): refused iff the
    /// value starts before `$dirEnd + 4`.
    Floor(usize),
}

impl DirectoryRule {
    /// The rule for `table`'s directory at `ifd_start` with `entries`
    /// entries: [`Self::Floor`] iff the table's family-0 group is
    /// `MakerNotes` (Exif.pm:6294 `$inMakerNotes`).
    fn for_table(table: &IfdTable, ifd_start: usize, entries: usize) -> Self {
        if table.group0 == "MakerNotes" {
            Self::Floor(directory_floor(ifd_start, entries))
        } else {
            Self::Overlap {
                dir_start: ifd_start,
                dir_end: directory_end(ifd_start, entries),
            }
        }
    }

    /// Whether the `size` bytes at `start`, whose entry stores the offset
    /// `stored` (before the `base` correction), are refused.
    fn refuses(self, stored: u32, start: usize, size: usize) -> bool {
        match self {
            Self::Overlap { dir_start, dir_end } => {
                // Exif.pm:6539: "offset shouldn't point into TIFF header".
                stored < 8
                    // Exif.pm:6549: "value shouldn't overlap our directory".
                    || (start < dir_end && start.saturating_add(size) > dir_start)
            }
            Self::Floor(floor) => start < floor,
        }
    }
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
        || (code == 16
            && in_maker_notes
            && member_bytes(ctx, "Make").is_some_and(|make| make == b"Apple"));
    if accepted { entry_type(code) } else { None }
}

/// A member's original byte representation where one exists.
///
/// `ProcessBinaryData` RawConv can seed the shared map with a Perl byte
/// scalar. Do not force it through UTF-8 merely to perform IFD's ASCII
/// Make/Model guards: an invalid byte after an ASCII prefix is still part of
/// a matching Perl byte string.
fn member_bytes<'c>(ctx: &'c cond::Ctx, member: &str) -> Option<&'c [u8]> {
    match ctx.members.get(member) {
        Some(MemberValue::Bytes(bytes)) => Some(bytes),
        Some(MemberValue::Str(s)) => Some(s.as_bytes()),
        _ => None,
    }
}

/// Exif.pm:6475 -- `next if $index or $$et{Model} =~ /^ILCE/;` (Sony ILCE
/// bodies write an empty first entry, so a bad first entry is not evidence
/// of a corrupted IFD for them).
fn model_is_ilce(ctx: &cond::Ctx) -> bool {
    member_bytes(ctx, "Model").is_some_and(|model| model.starts_with(b"ILCE"))
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

/// One successfully located IFD entry at its physical traversal point.
///
/// An observer sees the entry before the generated table handles it. Native
/// sub-directory descent still happens in the engine immediately afterward,
/// and the same observer is carried into that descent. This lets adapters add
/// source-authenticated rows the generated schema intentionally withheld
/// without re-walking or sorting the directory after the fact.
pub struct IfdEntryEvent<'entry, 'data> {
    pub table: &'static IfdTable,
    pub dir: IfdDir<'data>,
    pub entry: &'entry IfdEntry,
    located: &'entry Located<'data>,
}

impl IfdEntryEvent<'_, '_> {
    /// Decode the entry exactly as its declared TIFF type, before any
    /// table-specific `Format`, `RawConv`, `ValueConv`, or `PrintConv`.
    #[must_use]
    pub fn declared_value(&self) -> Option<DecodedValue> {
        let plan = read_plan(self.located, None)?;
        decode_plan(self.located, plan, self.dir.byte_order)
    }
}

/// Adapter hook for source-authenticated rows absent from a generated table.
///
/// Implementations may append rows to `out`; their position is the entry's
/// actual traversal position. The engine itself remains responsible for all
/// generated processing and recursive descent.
pub trait IfdEntryObserver {
    fn observe(&mut self, event: IfdEntryEvent<'_, '_>, out: &mut Vec<Emitted>);
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

/// Exif.pm:6502-6680 for one entry, with the table's [`DirectoryRule`].
fn locate<'d>(
    dir: &IfdDir<'d>,
    entry: &IfdEntry,
    ty: EntryType,
    rule: DirectoryRule,
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
    // Exif.pm:6539 and 6549 (or, for a MakerNotes table, table_ifd.rs's
    // floor; the module doc explains both): a value in the TIFF header or
    // inside the directory is "Suspicious" (Exif.pm:6673-6678) and
    // skipped. Checked before the bounds, as ExifTool computes `$suspect`
    // first; either way the entry costs exactly one warning.
    if rule.refuses(entry.value_offset, start, size) {
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

/// Project the bytes selected by `ReadValue` into the stored channel before
/// its string repair or later rational rounding. Numeric IFD formats already
/// have exact typed representations; strings need the physical byte scalar
/// because `FixUTF8` belongs only to the value/display path.
fn stored_plan(
    located: &Located<'_>,
    plan: ReadPlan,
    order: ByteOrder,
    decoded: &DecodedValue,
) -> TagValue {
    let source_bytes = |bytes: &[u8]| match String::from_utf8(bytes.to_vec()) {
        Ok(text) => TagValue::String(text),
        Err(_) => TagValue::Binary(bytes.to_vec()),
    };
    match plan.kind {
        Kind::Str => {
            let end = located
                .bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(located.bytes.len());
            source_bytes(&located.bytes[..end])
        }
        Kind::Utf8 => source_bytes(located.bytes),
        Kind::Undef => TagValue::Binary(located.bytes.to_vec()),
        Kind::Num(_) => runtime::to_stored_tag_value(decoded, order),
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
        // GetTagInfo evaluates a direct tag's Condition with this entry's
        // value bytes, format and count. Do not let a parent adapter's
        // temporary context leak into the child table.
        let val_pt = &located.bytes[..located.bytes.len().min(128)];
        let mut entry_ctx = cond::Ctx {
            members: &mut *ctx.members,
            val_pt: Some(val_pt),
            format: Some(located.ty.name),
            count: Some(i64::from(entry.count)),
        };
        if tag
            .condition
            .is_some_and(|condition| !condition.eval(&mut entry_ctx))
        {
            return None;
        }
        return Some(Resolved {
            tag,
            condition_resolved: tag.condition.is_some(),
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
    session: &mut Session,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
) {
    process_exif_decoded(table, dir, session, ctx, out);
}

/// [`process_exif`] with an entry observer propagated through every native
/// IFD sub-directory reached by this walk.
pub fn process_exif_with_observer(
    table: &'static IfdTable,
    dir: IfdDir<'_>,
    session: &mut Session,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
    observer: &mut dyn IfdEntryObserver,
) {
    let mut observer = Some(observer);
    let _ = process_exif_decoded_outcome_inner(table, dir, session, ctx, out, &mut observer);
}

/// What the walk did with one entry of the ROOT directory
/// ([`process_exif_decoded`]).
///
/// A caller that walks the same directory by other means (a hand arm) needs
/// to tell an absence ExifTool shares from one only this engine makes: the
/// first must stay an absence, the second may fall back to the other reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryRead {
    /// `ReadValue` (Exif.pm:6782) produced the entry's value. It has a row
    /// in `out`, or a conversion rule withheld the tag (a `RawConv` or
    /// `ValueConv` returning undef, a withheld conversion) -- the engine's
    /// report either way.
    Decoded,
    /// Refused on a rule ExifTool applies to the same bytes, so ExifTool
    /// reports nothing for the entry either: a type code `ProcessExif`
    /// rejects (Exif.pm:6463-6478, and every entry after a bad FIRST entry,
    /// "assume corrupted IFD"), a value it warns about and skips -- an
    /// offset into the TIFF header (Exif.pm:6539) or an overlap with the
    /// directory (Exif.pm:6549; both 6673-6678), an offset outside the data (Exif.pm:6551-6552; no `RAF` here, which is
    /// ExifTool's case for a JPEG APP1 or any in-memory block), an
    /// impossible size (Exif.pm:6505-6509) --, an entry past the exhausted
    /// warning budget (Exif.pm:6455-6457), or an excessive count
    /// (Exif.pm:6763-6773).
    Refused,
    /// Not read, for a reason of this engine's own: a value it cannot
    /// locate without a base, an unmodelled `Format` override, an
    /// unresolved tag or `Condition`, a value it cannot decode. ExifTool may
    /// well read it.
    Unread,
}

/// What an authenticated serial `SubDirectory` edge did for one root entry.
///
/// `Handled` includes native no-output paths such as a false parent condition,
/// a failed source-authenticated validator, an empty child, and a selected
/// serial record that ends at a normal unmatched alternative. `Refused` is
/// an enabled route whose execution could not be proved; it publishes no
/// speculative child rows or state. `Fallback` belongs only to a table whose
/// ownership has not transferred to the shared reader.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SerialSubdirRead {
    Handled,
    Refused,
    Fallback,
}

/// What [`process_exif_decoded`] reports about the ROOT directory, beside
/// the rows themselves.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RootReads {
    /// What the walk did with each entry, in entry order.
    pub entries: Vec<EntryRead>,
    /// `(row, entry)`: for every row the root directory itself emitted, its
    /// index in `out` and the index of the entry that produced it. Rows a
    /// `SubDirectory` edge produced are not listed.
    pub rows: Vec<(usize, usize)>,
    /// `(entry, outcome)` for source-selected serial child edges from this
    /// root. This stays separate from [`Self::rows`]: emitted child rows do
    /// not declare a name under their parent IFD tag, while a caller still
    /// needs to place or suppress its legacy parent producer in entry order.
    pub serial_subdirs: Vec<(usize, SerialSubdirRead)>,
    /// `(row, entry)` for every serial-child row emitted while processing a
    /// root edge. These rows are deliberately not mixed into [`Self::rows`]:
    /// their names belong to the child table, while the index supplies the
    /// parent-entry ordering a legacy carrier needs for a safe migration.
    pub serial_rows: Vec<(usize, usize)>,
}

/// Why a decoded root walk did or did not produce per-entry reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessExifDecoded {
    /// The root was admitted and walked.
    Read(RootReads),
    /// This session already processed the same table at the same address.
    AlreadyProcessed,
    /// The root was admitted, but its directory could not be read or walked.
    Refused,
}

/// [`process_exif_decoded`] with the root guard outcome kept distinct from a
/// directory refusal. Adapters that have a legacy residual producer need the
/// distinction: an already-processed directory must suppress that producer,
/// while a genuinely unreadable directory may fall back to it.
pub fn process_exif_decoded_outcome(
    table: &'static IfdTable,
    dir: IfdDir<'_>,
    session: &mut Session,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
) -> ProcessExifDecoded {
    let mut observer = None;
    process_exif_decoded_outcome_inner(table, dir, session, ctx, out, &mut observer)
}

fn process_exif_decoded_outcome_inner(
    table: &'static IfdTable,
    dir: IfdDir<'_>,
    session: &mut Session,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
    observer: &mut Option<&mut dyn IfdEntryObserver>,
) -> ProcessExifDecoded {
    // ExifTool.pm:9065-9072: `ProcessDirectory` records the root directory's
    // address too, which is what stops a table that points at itself.
    if !session.processed().admit(
        dir.data_domain,
        ifd_addr(dir.ifd_start),
        table_key(table),
        false,
    ) {
        return ProcessExifDecoded::AlreadyProcessed;
    }
    let mut decoded = RootReads::default();
    if walk(table, dir, session, ctx, out, observer, Some(&mut decoded)).is_none() {
        ProcessExifDecoded::Refused
    } else {
        ProcessExifDecoded::Read(decoded)
    }
}

/// [`process_exif`], also reporting what the walk did with each entry of
/// the ROOT directory and which entry each of its rows came from
/// ([`RootReads`]). `None` when the directory itself was refused or the root
/// guard had already processed it. Call [`process_exif_decoded_outcome`] when
/// those cases must be distinguished.
pub fn process_exif_decoded(
    table: &'static IfdTable,
    dir: IfdDir<'_>,
    session: &mut Session,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
) -> Option<RootReads> {
    match process_exif_decoded_outcome(table, dir, session, ctx, out) {
        ProcessExifDecoded::Read(decoded) => Some(decoded),
        ProcessExifDecoded::AlreadyProcessed | ProcessExifDecoded::Refused => None,
    }
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

/// One directory. `decoded`, when given, receives the [`RootReads`] of this
/// directory (see [`process_exif_decoded`]); `None` is returned only when
/// [`read_ifd`] refuses the directory.
fn walk(
    table: &'static IfdTable,
    dir: IfdDir<'_>,
    session: &mut Session,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
    observer: &mut Option<&mut dyn IfdEntryObserver>,
    decoded: Option<&mut RootReads>,
) -> Option<()> {
    let order = match dir.byte_order {
        ByteOrder::Little => SessionByteOrder::LittleEndian,
        ByteOrder::Big => SessionByteOrder::BigEndian,
    };
    let saved_dir_name = ctx.members.get("DIR_NAME").cloned();
    let saved_compression = ctx.members.get("Compression").cloned();
    let saved_subfile_type = ctx.members.get("SubfileType").cloned();
    match dir.group1 {
        Some(name) => {
            ctx.members
                .insert("DIR_NAME", MemberValue::Str(name.to_string()));
        }
        None => {
            ctx.members.remove("DIR_NAME");
        }
    }
    ctx.members
        .insert("Compression", MemberValue::Str(String::new()));
    ctx.members
        .insert("SubfileType", MemberValue::Str(String::new()));

    let result = {
        let mut scope = session.enter_directory(order, dir.group1);
        walk_scoped(table, dir, &mut scope, ctx, out, observer, decoded)
    };

    restore_ctx_member(ctx, "DIR_NAME", saved_dir_name);
    restore_ctx_member(ctx, "Compression", saved_compression);
    restore_ctx_member(ctx, "SubfileType", saved_subfile_type);
    result
}

fn restore_ctx_member(ctx: &mut cond::Ctx<'_>, key: &'static str, saved: Option<MemberValue>) {
    match saved {
        Some(value) => {
            ctx.members.insert(key, value);
        }
        None => {
            ctx.members.remove(key);
        }
    }
}

fn walk_scoped(
    table: &'static IfdTable,
    dir: IfdDir<'_>,
    session: &mut Session,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
    observer: &mut Option<&mut dyn IfdEntryObserver>,
    mut decoded: Option<&mut RootReads>,
) -> Option<()> {
    // Exif.pm:6344-6358.
    let entries = read_ifd(dir.data, dir.ifd_start, dir.byte_order)?;
    if let Some(reads) = decoded.as_deref_mut() {
        reads.entries.clear();
        reads.entries.resize(entries.len(), EntryRead::Unread);
        reads.rows.clear();
        reads.serial_subdirs.clear();
        reads.serial_rows.clear();
    }
    // Every entry from `from` on is one ExifTool never reaches.
    let refuse_rest = |decoded: &mut Option<&mut RootReads>, from: usize| {
        if let Some(reads) = decoded.as_deref_mut() {
            for flag in &mut reads.entries[from..] {
                *flag = EntryRead::Refused;
            }
        }
    };
    // Exif.pm:6539/6549 or the maker-note floor (module doc).
    let rule = DirectoryRule::for_table(table, dir.ifd_start, entries.len());
    // Exif.pm:6294.
    let in_maker_notes = table.group0 == "MakerNotes";
    // Autogeneration v2: the table's generated conversion arms, run first
    // for every field they claim (mixed mode, `conv` module doc), over the
    // file's Session under the current directory scope.
    let generated = conv::decoder(table);

    let mut warn_count = 0u32;
    for (index, entry) in entries.iter().enumerate() {
        // Exif.pm:6455-6457.
        if warn_count > 10 {
            refuse_rest(&mut decoded, index);
            return Some(());
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
                refuse_rest(&mut decoded, index);
                return Some(());
            }
            if let Some(reads) = decoded.as_deref_mut() {
                reads.entries[index] = EntryRead::Refused;
            }
            continue;
        };
        // Exif.pm:6502-6680.
        let located = match locate(&dir, entry, ty, rule) {
            Ok(located) => located,
            Err(Refusal::Warned) => {
                warn_count += 1;
                if let Some(reads) = decoded.as_deref_mut() {
                    reads.entries[index] = EntryRead::Refused;
                }
                continue;
            }
            Err(Refusal::Silent) => continue,
        };
        session.count = Some(i64::from(entry.count));
        session.format = Some(located.ty.name.to_string());
        if let Some(observer) = observer.as_deref_mut() {
            observer.observe(
                IfdEntryEvent {
                    table,
                    dir,
                    entry,
                    located: &located,
                },
                out,
            );
        }
        // Exif.pm:6485, 6717-6720. A `$bad` entry never gets this far, which
        // is also ExifTool's order (Exif.pm:6713-6714 drops the tag before
        // the value-scoped `GetTagInfo`).
        let resolved = resolve(table, entry, &located, ctx);
        // Condition assignments mutate ExifTool's file object even when an
        // alternative loses or no alternative ultimately matches.
        sync_ctx_members(session, ctx);
        let Some(Resolved {
            tag,
            condition_resolved,
        }) = resolved
        else {
            if direct_serial_no_match(table, entry.tag_id) {
                if let Some(reads) = decoded.as_deref_mut() {
                    reads
                        .serial_subdirs
                        .push((index, SerialSubdirRead::Handled));
                }
            }
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
            let out_before = out.len();
            let outcome = descend_observed(
                table, tag, edge, &located, &dir, session, ctx, out, observer,
            );
            if let DescendOutcome::Serial(outcome) = outcome
                && let Some(reads) = decoded.as_deref_mut()
            {
                reads.serial_subdirs.push((index, outcome));
                reads
                    .serial_rows
                    .extend((out_before..out.len()).map(|row| (row, index)));
            }
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
            if let Some(reads) = decoded.as_deref_mut() {
                reads.entries[index] = EntryRead::Refused;
            }
            continue;
        }
        // Exif.pm:6782, 6915.
        let Some(raw) = decode_plan(&located, plan, dir.byte_order) else {
            continue;
        };
        let stored = stored_plan(&located, plan, dir.byte_order, &raw);
        if let Some(reads) = decoded.as_deref_mut() {
            reads.entries[index] = EntryRead::Decoded;
        }
        // ExifTool.pm:6312-6320: `ReadValue` keeps the fraction beside the
        // number (`TAG_EXTRA{Rational}`, Exif.pm:7185).
        let fraction = single_rational(&raw);
        // Mixed mode (Autogeneration v2): a generated arm takes the field
        // first; on a decline the existing path below runs for this entry.
        let mut declined = false;
        if let Some(decode) = generated.filter(|_| conv::claims(table, tag)) {
            let attempt = generated_arm(decode, session, tag, &raw);
            match resolve_generated_attempt(attempt, session, ctx) {
                Arm::Decline(_) => {
                    declined = true;
                }
                Arm::Suppress => continue,
                Arm::Report(report) => {
                    let Some(group1) = group1_of(table, tag, &dir) else {
                        continue;
                    };
                    if !super::attribution::silenced(super::attribution::Token::Engine) {
                        out.push(generated_row(
                            table,
                            tag,
                            group1,
                            &raw,
                            stored.clone(),
                            fraction,
                            report,
                        ));
                        if let Some(reads) = decoded.as_deref_mut() {
                            reads.rows.push((out.len() - 1, index));
                        }
                    }
                    continue;
                }
            }
        }
        process_residual_entry(
            table,
            tag,
            &dir,
            raw,
            stored,
            fraction,
            omitted,
            declined,
            session,
            ctx,
            out,
            &mut decoded,
            index,
        );
    }
    Some(())
}

/// The existing per-entry producer, invoked only when no generated arm owns
/// the entry or after a generated decline has discarded its staged effects.
/// Keeping this as one named residual is the production seam used by the
/// exactness tests: a declined byte run cannot bypass the same conversion,
/// emission, and decoded-row bookkeeping used by a real directory walk.
#[allow(clippy::too_many_arguments)]
fn process_residual_entry(
    table: &'static IfdTable,
    tag: &IfdTag,
    dir: &IfdDir<'_>,
    raw: DecodedValue,
    stored: TagValue,
    fraction: Option<(i64, i64)>,
    omitted: Omitted,
    generated_declined: bool,
    session: &mut Session,
    ctx: &mut cond::Ctx<'_>,
    out: &mut Vec<Emitted>,
    decoded: &mut Option<&mut RootReads>,
    index: usize,
) {
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
            return;
        };
        ctx.members.insert(member, value.clone());
        let session_value = match value {
            MemberValue::Str(s) => MemberVal::Str(s),
            MemberValue::Bytes(bytes) => MemberVal::from_bytes(bytes),
            MemberValue::Num(n) => MemberVal::Int(n),
        };
        let _ = session.set_member(member, session_value);
    }
    // A `Binary` tag with a refused `PrintConv` is withheld here too,
    // although ExifTool never runs a PrintConv on a scalar-ref value
    // (ExifTool.pm:3533): over-refusing is the safe direction.
    if omitted.any() {
        // A field whose generated arm declined this entry and whose static
        // conversion is withheld: the engine cannot vouch for the absence,
        // so the caller's hand arm runs (`EntryRead::Unread`).
        if generated_declined && let Some(reads) = decoded.as_deref_mut() {
            reads.entries[index] = EntryRead::Unread;
        }
        return;
    }
    let binary = tag.flags.binary && tag.value_conv.is_none();
    let (value, value_conv) = if binary {
        // ExifTool.pm:3535-3539: `Binary` with no `ValueConv` gets `\$val`.
        let Some(len) = perl_length(&raw) else {
            return;
        };
        (
            TagValue::String(format!(
                "(Binary data {len} bytes, use -b option to extract)"
            )),
            None,
        )
    } else {
        let Some(converted) = runtime::apply_value_conv(tag.value_conv, &raw) else {
            return;
        };
        let unconverted = || {
            if tag.flags.list {
                runtime::to_tag_value(&converted)
            } else {
                ifd_exiftool_value(&converted)
            }
        };
        match runtime::render(tag.print_conv, &converted) {
            Some(rendered) => (TagValue::String(rendered), Some(unconverted())),
            None => (unconverted(), None),
        }
    };
    let Some(group1) = group1_of(table, tag, dir) else {
        return;
    };
    let emitted_at = out.len();
    if !super::attribution::silenced(super::attribution::Token::Engine) {
        out.push(Emitted {
            module: table.module,
            table: table.table,
            group0: tag.groups.g0.unwrap_or(table.group0),
            group1,
            group2: tag.groups.g2.unwrap_or(table.group2),
            name: tag.name,
            source_id: oxidex_tags::TagId::Numeric(tag.id),
            stored,
            value,
            low_priority: effective_priority(table, tag) == Some(0),
            avoid: tag.flags.avoid,
            rational: fraction
                .filter(|_| !binary && tag.value_conv.is_none() && value_conv.is_none()),
            value_conv,
            is_list: tag.flags.list,
        });
    }
    if out.len() > emitted_at
        && let Some(reads) = decoded.as_deref_mut()
    {
        reads.rows.push((out.len() - 1, index));
    }
}

/// Every member the existing condition walk tracks in `ctx` is copied into
/// the file-scoped session after condition evaluation. This keeps arbitrary
/// assignment side effects visible across directories, including a losing
/// alternative's write, not only the historically hand-picked Make/Model and
/// file-type keys.
fn sync_ctx_members(session: &mut Session, ctx: &cond::Ctx) {
    for (&key, member) in ctx.members.iter() {
        let value = match member {
            MemberValue::Str(s) => MemberVal::Str(s.clone()),
            MemberValue::Bytes(b) => MemberVal::from_bytes(b.clone()),
            MemberValue::Num(n) => MemberVal::Int(*n),
        };
        // A non-UTF-8 Make/Model leaves the typed slot unsupplied.
        let _ = session.set_member(key, value);
    }
}

/// A generated conversion attempt's mutations, isolated from the live
/// file-scoped [`Session`] until its [`Arm`] establishes ownership.
///
/// Generated helpers are allowed to mutate `$self` (warnings, options,
/// data-members and other file state) while deciding whether to report,
/// suppress or decline an entry. Running them against the live session lets
/// a declined attempt contaminate the one residual that follows it. A clone
/// is the transaction boundary: Report and Suppress commit source-required
/// effects, while Decline drops every attempted mutation before residual
/// dispatch.
#[derive(Debug)]
pub(crate) struct StagedEffects {
    staged_session: Session,
}

impl StagedEffects {
    #[must_use]
    fn begin(session: &Session) -> Self {
        Self {
            staged_session: session.clone(),
        }
    }

    fn session_mut(&mut self) -> &mut Session {
        &mut self.staged_session
    }

    /// Publish the staged file state atomically after ownership resolves.
    fn commit(self, session: &mut Session) {
        *session = self.staged_session;
    }

    /// Explicitly abandon a generated attempt before its residual runs.
    fn discard(self) {}
}

struct GeneratedAttempt {
    arm: Arm,
    effects: StagedEffects,
}

/// Resolve a generated attempt at the production ownership boundary.
///
/// A returned [`Arm::Decline`] means the staged copy has already been
/// discarded, so the caller may immediately enter its one residual path.
/// Report writes are part of the same transaction as decoder-side effects.
fn resolve_generated_attempt(
    attempt: GeneratedAttempt,
    session: &mut Session,
    ctx: &mut cond::Ctx<'_>,
) -> Arm {
    let GeneratedAttempt { arm, mut effects } = attempt;
    match arm {
        Arm::Decline(reason) => {
            effects.discard();
            Arm::Decline(reason)
        }
        Arm::Suppress => {
            effects.commit(session);
            Arm::Suppress
        }
        Arm::Report(report) => {
            for (key, value) in &report.writes {
                let _ = effects.session_mut().set_member(key, value.clone());
                match member_of(value) {
                    Some(member) => {
                        ctx.members.insert(key, member);
                    }
                    None => {
                        ctx.members.remove(key);
                    }
                }
            }
            effects.commit(session);
            Arm::Report(report)
        }
    }
}

/// The Perl scalar `ReadValue` hands a conversion (ExifTool.pm:6297-6330):
/// an integer IV, a `float`/`double` NV, a rational's `RoundFloat` string
/// (or `inf`/`undef`), a string or byte run as its exact bytes (a
/// [`MemberVal::Bytes`] when they are not UTF-8: the arms' runtime is
/// byte-exact), a fixed-count entry as ONE space-joined string.
fn perl_scalar(raw: &DecodedValue) -> Option<MemberVal> {
    Some(match raw {
        DecodedValue::Integer(i) => MemberVal::Int(*i),
        DecodedValue::Float(f) => MemberVal::Float(*f),
        DecodedValue::UnsignedRational(..) | DecodedValue::SignedRational(..) => {
            MemberVal::Str(ifd_perl_string(raw)?)
        }
        DecodedValue::StringBytes(bytes) | DecodedValue::Undefined(bytes) => {
            MemberVal::from_bytes(bytes.clone())
        }
        DecodedValue::String(s) => MemberVal::Str(s.clone()),
        DecodedValue::Array(values) => MemberVal::Str(
            values
                .iter()
                .map(ifd_perl_string)
                .collect::<Option<Vec<_>>>()?
                .join(" "),
        ),
    })
}

/// Runs `tag`'s generated arm on the entry's `ReadValue` result. (No
/// Exif::Main arm reads the eval-site `$count`/`$format`; the backend
/// refuses any that would; and a conversion that made a `Warn` request has
/// already declined, `conv::Decode`'s contract.) The arm's report is taken
/// only when the row it becomes is exact: every string the row carries must
/// be UTF-8 -- the arm is byte-exact, but a `TagValue` holds text -- and a
/// report that leaves `$val` unconverted (`value: None`) must have UTF-8
/// `$val`, since the row then shows the value read. Otherwise the existing
/// path runs, as before.
fn generated_arm(
    decode: conv::Decode,
    session: &Session,
    tag: &IfdTag,
    raw: &DecodedValue,
) -> GeneratedAttempt {
    let mut effects = StagedEffects::begin(session);
    let Some(val) = perl_scalar(raw) else {
        return GeneratedAttempt {
            arm: Arm::Decline("value has no Perl scalar form here"),
            effects,
        };
    };
    let arm = decode(effects.session_mut(), tag.id, &val);
    if let Arm::Report(report) = &arm {
        let text = |out: &conv::Out| match out {
            conv::Out::Scalar(v) => !matches!(v, MemberVal::Bytes(_)),
            conv::Out::Binary(_) => true,
        };
        let exact = report
            .value
            .as_ref()
            .map_or(!matches!(val, MemberVal::Bytes(_)), text)
            && report.print.as_ref().is_none_or(text);
        if !exact {
            return GeneratedAttempt {
                arm: Arm::Decline("a reported value is not UTF-8 text"),
                effects,
            };
        }
    }
    GeneratedAttempt { arm, effects }
}

/// `$$self{X} = v` as the `Cond` grammar's member: an IV as `Num`, a byte
/// string as `Str`/`Bytes` exactly, anything else as its string; `undef`
/// clears it.
fn member_of(value: &MemberVal) -> Option<MemberValue> {
    match value {
        MemberVal::Int(i) => Some(MemberValue::Num(*i)),
        MemberVal::Undef => None,
        MemberVal::Bytes(b) => Some(MemberValue::Bytes(b.clone())),
        other => Some(MemberValue::Str(other.perl_string())),
    }
}

/// A generated arm's scalar as the value an IFD row carries: an IV as
/// `Integer`, an NV as `Float` (printed `%.15g` by every writer), anything
/// else as its Perl string.
fn scalar_tag_value(value: &MemberVal) -> TagValue {
    match value {
        MemberVal::Int(i) => TagValue::Integer(*i),
        MemberVal::Float(f) => TagValue::Float(*f),
        other => TagValue::String(other.perl_string()),
    }
}

fn out_tag_value(out: &conv::Out) -> TagValue {
    match out {
        conv::Out::Scalar(v) => scalar_tag_value(v),
        // ExifTool.pm:3535-3539 / exiftool:3983-3988, as the static path
        // renders a `Binary` tag.
        conv::Out::Binary(bytes) => TagValue::String(format!(
            "(Binary data {} bytes, use -b option to extract)",
            bytes.len()
        )),
    }
}

/// The row for a generated arm's report: the same shape the static path
/// builds below -- the default-mode value, the `-n` value when a `PrintConv`
/// ran, and the fraction only where the value is the rational unconverted.
fn generated_row(
    table: &'static IfdTable,
    tag: &'static IfdTag,
    group1: &'static str,
    raw: &DecodedValue,
    stored: TagValue,
    fraction: Option<(i64, i64)>,
    report: conv::Report,
) -> Emitted {
    let unconverted = match &report.value {
        Some(out) => out_tag_value(out),
        None => {
            let rounded = round_rationals(raw.clone());
            if tag.flags.list {
                runtime::to_tag_value(&rounded)
            } else {
                ifd_exiftool_value(&rounded)
            }
        }
    };
    // A `PrintConv` whose text is exactly the value it was given (ISO's
    // `s/\s+/, /g` on a single number) changes nothing ExifTool prints: the
    // row keeps its typed value, as a tag with no `PrintConv` does, so the
    // library's typed accessors see what the hand arm gave them.
    // Only for a field the static table withholds (whose previous producer
    // was the hand arm, which gave the typed value) and only where no
    // conversion stage replaced `$val` (the typed value IS what `ReadValue`
    // read): a field the static engine already reported keeps the String
    // display it always had, and a converted number is never handed to
    // name-keyed output rules as if it were the raw one.
    let print = report.print.filter(|print| {
        let before = match &report.value {
            None if tag.omitted.any() => perl_scalar(raw).map(|v| v.perl_string()),
            _ => None,
        };
        !matches!((print, before), (conv::Out::Scalar(p), Some(b)) if p.is_defined() && p.perl_bytes().as_ref() == b.as_bytes())
    });
    let untouched = report.value.is_none() && print.is_none();
    let (value, value_conv) = match &print {
        Some(print) => (out_tag_value(print), Some(unconverted)),
        None => (unconverted, None),
    };
    Emitted {
        module: table.module,
        table: table.table,
        group0: tag.groups.g0.unwrap_or(table.group0),
        group1,
        group2: tag.groups.g2.unwrap_or(table.group2),
        name: tag.name,
        source_id: oxidex_tags::TagId::Numeric(tag.id),
        stored,
        value,
        low_priority: effective_priority(table, tag) == Some(0),
        avoid: tag.flags.avoid,
        rational: fraction.filter(|_| untouched),
        value_conv,
        is_list: tag.flags.list,
    }
}

/// The `(numerator, denominator)` of a single rational `ReadValue` result
/// (either signedness; a zero denominator included, as ExifTool keeps
/// `"$ratNumer/$ratDenom"` for it too), else `None`.
fn single_rational(raw: &DecodedValue) -> Option<(i64, i64)> {
    match *raw {
        DecodedValue::UnsignedRational(numerator, denominator) => {
            Some((i64::from(numerator), i64::from(denominator)))
        }
        DecodedValue::SignedRational(numerator, denominator) => {
            Some((i64::from(numerator), i64::from(denominator)))
        }
        _ => None,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DescendOutcome {
    Native,
    Serial(SerialSubdirRead),
}

/// The IFD caller has no public Unknown-output option yet. It therefore
/// preserves the existing default projection: serial Unknown slots select and
/// consume bytes but do not call `FoundTag`. A caller that exposes that option
/// must carry it into this sink before widening the route.
struct IfdSerialSink<'a> {
    out: &'a mut Vec<Emitted>,
}

impl SerialEmissionSink for IfdSerialSink<'_> {
    fn emit(&mut self, row: Emitted) {
        self.out.push(row);
    }

    fn serial_enabled(&self, table: &'static SerialTable) -> bool {
        enabled_serial::is_enabled(table)
    }
}

/// Publish a serial child's buffered rows only when its entire native walk
/// stayed authenticated. A later unsupported state action invalidates an
/// earlier prefix as well. The ownership wrapper turns this internal fallback
/// into an explicit refusal and restores the edge's prior state and output.
fn finish_serial_child<T>(
    out: &mut Vec<T>,
    child: Vec<T>,
    result: &SerialWalkResult,
) -> SerialSubdirRead {
    if result.gate_a_blocked != 0 || result.gate_b_blocked != 0 || result.tainted {
        SerialSubdirRead::Fallback
    } else {
        out.extend(child);
        SerialSubdirRead::Handled
    }
}

/// A direct source candidate whose condition did not select an alternative.
/// It is safe to call that a handled serial omission only when the declared
/// row itself has a fully modeled condition and one source-selected serial
/// edge. Variants and unmodeled conditions remain `Unread`, preserving the
/// legacy producer rather than inferring an absence from a partial view.
fn direct_serial_no_match(table: &'static IfdTable, id: u16) -> bool {
    let Some(tag) = table.tags.iter().find(|tag| tag.id == id) else {
        return false;
    };
    tag.condition.is_some()
        && !tag.omitted.condition
        && tag.subdir.as_ref().is_some_and(|edge| {
            edge.processor == IfdSubdirProcessor::Serial
                && enabled_serial::owns(edge.module, edge.table)
        })
}

/// An enabled edge owns its output even when later source/execution facts are
/// refused. Parent effects were applied before descent, so the snapshot keeps
/// them while discarding any speculative child effects and earlier child rows.
fn owned_serial_attempt(
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
    attempt: impl FnOnce(&mut cond::Ctx, &mut Vec<Emitted>) -> DescendOutcome,
) -> DescendOutcome {
    let members = ctx.members.clone();
    let before = out.len();
    let outcome = attempt(ctx, out);
    if outcome == DescendOutcome::Serial(SerialSubdirRead::Fallback) {
        *ctx.members = members;
        out.truncate(before);
        DescendOutcome::Serial(SerialSubdirRead::Refused)
    } else {
        outcome
    }
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
    session: &mut Session,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
) -> DescendOutcome {
    let mut observer = None;
    descend_observed(
        table,
        tag,
        edge,
        located,
        dir,
        session,
        ctx,
        out,
        &mut observer,
    )
}

#[allow(clippy::too_many_arguments)]
fn descend_observed(
    table: &'static IfdTable,
    tag: &'static IfdTag,
    edge: &IfdSubdirEdge,
    located: &Located<'_>,
    dir: &IfdDir<'_>,
    session: &mut Session,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
    observer: &mut Option<&mut dyn IfdEntryObserver>,
) -> DescendOutcome {
    if edge.processor != IfdSubdirProcessor::Serial {
        return descend_inner(table, tag, edge, located, dir, session, ctx, out, observer);
    }
    if !enabled_serial::owns(edge.module, edge.table) {
        return DescendOutcome::Serial(SerialSubdirRead::Fallback);
    }
    owned_serial_attempt(ctx, out, |ctx, out| {
        descend_inner(table, tag, edge, located, dir, session, ctx, out, observer)
    })
}

#[allow(clippy::too_many_arguments)]
fn descend_inner(
    table: &'static IfdTable,
    tag: &'static IfdTag,
    edge: &IfdSubdirEdge,
    located: &Located<'_>,
    dir: &IfdDir<'_>,
    session: &mut Session,
    ctx: &mut cond::Ctx,
    out: &mut Vec<Emitted>,
    observer: &mut Option<&mut dyn IfdEntryObserver>,
) -> DescendOutcome {
    // Legacy IFD/binary Validate remains unwalked. A serial edge may proceed
    // only when codegen carried the independently authenticated primitive;
    // a schema mismatch is a carrier fallback, never an implicit approval.
    if edge.validate && edge.processor == IfdSubdirProcessor::Native {
        return DescendOutcome::Native;
    }
    if edge.validate && edge.validation.is_none() {
        return DescendOutcome::Serial(SerialSubdirRead::Fallback);
    }
    // Slice IFD1: the generator emitted the edge but marked it unwalked --
    // the enclosing table itself (no TagTable, Exif.pm:6939-6944) or a
    // ProcessProc the walk cannot run. Same outcome as `validate`: the
    // pointer marks its place and nothing behind it is read.
    if edge.unwalked.is_some() {
        return match edge.processor {
            IfdSubdirProcessor::Native => DescendOutcome::Native,
            IfdSubdirProcessor::Serial => DescendOutcome::Serial(SerialSubdirRead::Fallback),
        };
    }
    // Exif.pm:6921-6926 -- "don't process empty subdirectories".
    if located.bytes.is_empty() {
        return match edge.processor {
            IfdSubdirProcessor::Native => DescendOutcome::Native,
            IfdSubdirProcessor::Serial => DescendOutcome::Serial(SerialSubdirRead::Handled),
        };
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
        Serial(&'static SerialTable),
    }
    let target = if edge.processor == IfdSubdirProcessor::Serial {
        let Some(t) = find_serial_table(edge.module, edge.table) else {
            return DescendOutcome::Serial(SerialSubdirRead::Fallback);
        };
        Target::Serial(t)
    } else if let Some(t) = ifd_target(edge.module, edge.table) {
        if !walkable(t) {
            // Opt-in (Step 28 D1): an edge never enables its target.
            return DescendOutcome::Native;
        }
        Target::Ifd(t)
    } else if let Some(t) = find_table(edge.module, edge.table) {
        if !t.enabled() {
            return DescendOutcome::Native;
        }
        Target::Binary(t)
    } else {
        // Not a defect in the edge: the target is a table neither generator
        // transcribed (a custom `PROCESS_PROC`, a refused table).
        return DescendOutcome::Native;
    };

    // Exif.pm:6929-6938, 7100-7101: how many times the loop runs.
    let iterations: Vec<Option<i64>> = match edge.start {
        IfdStart::Val(_) => match pointer_values(tag, edge, located, dir.byte_order) {
            Some(pointers) => pointers.into_iter().map(Some).collect(),
            // Native cannot open a child when the pointer value is not
            // readable.  This is a handled no-child path, not a reason for a
            // legacy producer to invent a second interpretation of it.
            None => {
                return match edge.processor {
                    IfdSubdirProcessor::Native => DescendOutcome::Native,
                    IfdSubdirProcessor::Serial => DescendOutcome::Serial(SerialSubdirRead::Handled),
                };
            }
        },
        // A `$valuePtr` start does not depend on `$val`; with `MaxSubdirs`
        // ExifTool would loop over the value's pieces re-opening the SAME
        // start, which `$$self{PROCESSED}` (and the guard here) refuses after
        // the first, so once is the observable count.
        IfdStart::ValuePtr(_) => vec![None],
    };

    let dir_name = subdir_name(tag, edge, in_maker_notes);

    let mut serial_outcome = SerialSubdirRead::Handled;
    for pointer in iterations {
        // Exif.pm:6951-6968 -- `#### eval Start ($valuePtr, $val)`, then
        // `$newStart -= $subdirDataPos` back to data-relative.
        let start = match (edge.start, pointer) {
            (IfdStart::ValuePtr(offset), _) => value_pos.saturating_add(offset),
            (IfdStart::Val(offset), Some(pointer)) => {
                // `$val + base` is where the pointer lands in `data`; with
                // no correction the shared reader cannot locate the block.
                // This is an execution prerequisite, so retain the legacy
                // producer rather than calling a failed source evaluation a
                // native omission.
                let Some(base) = dir.base else {
                    return match edge.processor {
                        IfdSubdirProcessor::Native => DescendOutcome::Native,
                        IfdSubdirProcessor::Serial => {
                            DescendOutcome::Serial(SerialSubdirRead::Fallback)
                        }
                    };
                };
                pointer.saturating_add(offset).saturating_add(base)
            }
            (IfdStart::Val(_), None) => {
                return match edge.processor {
                    IfdSubdirProcessor::Native => DescendOutcome::Native,
                    IfdSubdirProcessor::Serial => DescendOutcome::Serial(SerialSubdirRead::Handled),
                };
            }
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
            return match edge.processor {
                IfdSubdirProcessor::Native => DescendOutcome::Native,
                IfdSubdirProcessor::Serial => DescendOutcome::Serial(SerialSubdirRead::Handled),
            };
        }
        let Ok(start_pos) = usize::try_from(start) else {
            return match edge.processor {
                IfdSubdirProcessor::Native => DescendOutcome::Native,
                IfdSubdirProcessor::Serial => DescendOutcome::Serial(SerialSubdirRead::Handled),
            };
        };
        // Exif.pm:6971-6997.
        let byte_order = match edge.byte_order {
            IfdByteOrder::Inherit => dir.byte_order,
            IfdByteOrder::Little => ByteOrder::Little,
            IfdByteOrder::Big => ByteOrder::Big,
            IfdByteOrder::Unknown => match detect_byte_order(data, start_pos, dir.byte_order) {
                Some(order) => order,
                None => {
                    return match edge.processor {
                        IfdSubdirProcessor::Native => DescendOutcome::Native,
                        IfdSubdirProcessor::Serial => {
                            DescendOutcome::Serial(SerialSubdirRead::Fallback)
                        }
                    };
                }
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
                    return match edge.processor {
                        IfdSubdirProcessor::Native => DescendOutcome::Native,
                        IfdSubdirProcessor::Serial => {
                            DescendOutcome::Serial(SerialSubdirRead::Fallback)
                        }
                    };
                }
                match dir.base {
                    Some(base) => Some(base.saturating_add(expr.eval(start - base, 0))),
                    // `$start` is unknowable without a correction; a
                    // constant leaves the correction unknown too.
                    None if matches!(expr, BaseExpr::Const(_)) => None,
                    None => {
                        return match edge.processor {
                            IfdSubdirProcessor::Native => DescendOutcome::Native,
                            IfdSubdirProcessor::Serial => {
                                DescendOutcome::Serial(SerialSubdirRead::Fallback)
                            }
                        };
                    }
                }
            }
        };

        match target {
            Target::Ifd(target) => {
                // ExifTool.pm:9065-9072: a repeat is `return 0` for THIS
                // directory; the loop moves on to the next value.
                if !session.processed().admit(
                    dir.data_domain,
                    ifd_addr(start_pos),
                    table_key(target),
                    false,
                ) {
                    continue;
                }
                session.processed().depth += 1;
                // A refused sub-directory is simply not walked (`None`).
                let _ = walk(
                    target,
                    IfdDir {
                        data,
                        data_domain: dir.data_domain,
                        ifd_start: start_pos,
                        base,
                        byte_order,
                        group1: dir_name,
                    },
                    session,
                    ctx,
                    out,
                    observer,
                    None,
                );
                session.processed().depth -= 1;
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
                if !session.processed().admit(
                    dir.data_domain,
                    binary_addr(base, start_pos),
                    table_key(target),
                    false,
                ) {
                    continue;
                }
                session.processed().depth += 1;
                engine::walk(
                    target,
                    Dir {
                        data,
                        data_domain: dir.data_domain,
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
                    session.processed(),
                    out,
                );
                session.processed().depth -= 1;
            }
            Target::Serial(target) => {
                let Ok(dir_len) = usize::try_from(dir_len) else {
                    serial_outcome = SerialSubdirRead::Handled;
                    continue;
                };
                if edge.validate {
                    let Ok(validation_size) = u32::try_from(dir_len) else {
                        serial_outcome = SerialSubdirRead::Handled;
                        continue;
                    };
                    if !edge
                        .validation
                        .expect("checked before serial descent")
                        .matches(data, start_pos, validation_size, byte_order)
                    {
                        // Native Validate false is an ordinary handled omission;
                        // it must not reactivate a legacy child reader.
                        serial_outcome = SerialSubdirRead::Handled;
                        continue;
                    }
                }
                if !session.processed().admit(
                    dir.data_domain,
                    binary_addr(base, start_pos),
                    table_key(target),
                    false,
                ) {
                    serial_outcome = SerialSubdirRead::Handled;
                    continue;
                }
                // Commit child rows only after the entire serial directory
                // is proved. The outer edge transaction also owns state
                // rollback, including earlier child iterations.
                let mut serial_rows = Vec::new();
                session.processed().depth += 1;
                let mut sink = IfdSerialSink {
                    out: &mut serial_rows,
                };
                let result = process_serial_directory(
                    target,
                    SerialDir {
                        data,
                        dir_start: start_pos,
                        dir_len,
                        base: 0,
                        data_pos: base.map_or(0, |value| -value),
                        byte_order,
                    },
                    ctx,
                    &mut sink,
                );
                session.processed().depth -= 1;
                serial_outcome = finish_serial_child(out, serial_rows, &result);
                if serial_outcome == SerialSubdirRead::Fallback {
                    // Refuse the whole edge, including prior child iterations;
                    // the ownership wrapper rolls back their state and rows.
                    return DescendOutcome::Serial(SerialSubdirRead::Fallback);
                }
            }
        }
    }
    match edge.processor {
        IfdSubdirProcessor::Native => DescendOutcome::Native,
        IfdSubdirProcessor::Serial => DescendOutcome::Serial(serial_outcome),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;
    use crate::exiftool_tables::cond::{CmpOp, Cond, Ctx, EffectSource};
    use crate::exiftool_tables::ifd_schema::IfdVariantGroup;
    use crate::exiftool_tables::{
        ExprId, GateA, IfdFlags, Omitted, PrintConv, SizeExpectation, TagGroups, U16SizeCheck,
    };

    fn process_exif(
        table: &'static IfdTable,
        dir: IfdDir<'_>,
        ctx: &mut cond::Ctx<'_>,
        out: &mut Vec<Emitted>,
    ) {
        let mut session = Session::new();
        super::process_exif(table, dir, &mut session, ctx, out);
    }

    fn process_exif_decoded(
        table: &'static IfdTable,
        dir: IfdDir<'_>,
        ctx: &mut cond::Ctx<'_>,
        out: &mut Vec<Emitted>,
    ) -> Option<RootReads> {
        let mut session = Session::new();
        super::process_exif_decoded(table, dir, &mut session, ctx, out)
    }

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
            condition: None,
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

    fn suppress_after_member_write(session: &mut Session, _: u16, _: &MemberVal) -> Arm {
        session
            .set_member("SuppressWrite", MemberVal::Int(41))
            .expect("test member write");
        Arm::Suppress
    }

    #[test]
    fn generated_suppress_commits_its_staged_session_effects() {
        static TAG: IfdTag = plain(1, "Suppressed");
        let mut session = Session::new();
        let GeneratedAttempt { arm, effects } = generated_arm(
            suppress_after_member_write,
            &session,
            &TAG,
            &DecodedValue::Integer(1),
        );
        assert_eq!(arm, Arm::Suppress);
        effects.commit(&mut session);
        assert_eq!(session.member("SuppressWrite"), MemberVal::Int(41));
    }

    fn staged_matrix_seed() -> Session {
        let mut session = Session::new();
        session
            .set_member("Make", MemberVal::Str("BeforeMake".into()))
            .unwrap();
        session
            .set_member("Model", MemberVal::Str("BeforeModel".into()))
            .unwrap();
        session.set_member("RemoveMe", MemberVal::Int(7)).unwrap();
        session.set_option("Verbose", MemberVal::Int(0));
        session.warn(MemberVal::Str("before".into()));
        session.byte_order = Some(crate::exiftool_tables::session::ByteOrder::LittleEndian);
        session.count = Some(9);
        session.format = Some("outer".into());
        session
            .set_member("DIR_NAME", MemberVal::Str("Outer".into()))
            .unwrap();
        assert!(session.processed().admit(1, 2, 3, false));
        session
    }

    fn mutate_staged_matrix(session: &mut Session) {
        session
            .set_member("Make", MemberVal::Str("AfterMake".into()))
            .unwrap();
        session
            .set_member("Model", MemberVal::Str("AfterModel".into()))
            .unwrap();
        session.remove_member("RemoveMe");
        session.set_option("Verbose", MemberVal::Int(4));
        session.warn(MemberVal::Str("after-1".into()));
        session.warn(MemberVal::Str("after-2".into()));
        assert!(session.processed().admit(4, 5, 6, false));

        // A nested directory scope must restore the staged copy before the
        // transaction resolves, just as it would on the live Session.
        {
            let mut scope = session.enter_directory(
                crate::exiftool_tables::session::ByteOrder::BigEndian,
                Some("Inner"),
            );
            scope.count = Some(99);
            scope.format = Some("inner".into());
        }
    }

    fn clear_typed_make_model(session: &mut Session) {
        session.set_member("Make", MemberVal::Undef).unwrap();
        session.set_member("Model", MemberVal::Undef).unwrap();
    }

    fn assert_outer_scope(session: &Session) {
        assert_eq!(
            session.byte_order,
            Some(crate::exiftool_tables::session::ByteOrder::LittleEndian)
        );
        assert_eq!(session.count, Some(9));
        assert_eq!(session.format.as_deref(), Some("outer"));
        assert_eq!(session.member("DIR_NAME"), MemberVal::Str("Outer".into()));
    }

    #[test]
    fn staged_effects_commit_the_complete_session_matrix_in_order() {
        let mut session = staged_matrix_seed();
        let mut effects = StagedEffects::begin(&session);
        mutate_staged_matrix(effects.session_mut());
        effects.commit(&mut session);

        assert_eq!(session.member("Make"), MemberVal::Str("AfterMake".into()));
        assert_eq!(session.member("Model"), MemberVal::Str("AfterModel".into()));
        assert!(!session.has_member("RemoveMe"));
        assert_eq!(session.option("Verbose"), MemberVal::Int(4));
        assert_eq!(
            session
                .warnings()
                .iter()
                .map(|warning| warning.message.clone())
                .collect::<Vec<_>>(),
            [
                MemberVal::Str("before".into()),
                MemberVal::Str("after-1".into()),
                MemberVal::Str("after-2".into())
            ]
        );
        assert_outer_scope(&session);
        assert!(!session.processed().admit(1, 2, 3, false));
        assert!(!session.processed().admit(4, 5, 6, false));
    }

    #[test]
    fn staged_effects_discard_the_complete_session_matrix() {
        let mut session = staged_matrix_seed();
        let mut effects = StagedEffects::begin(&session);
        mutate_staged_matrix(effects.session_mut());
        effects.discard();

        assert_eq!(session.member("Make"), MemberVal::Str("BeforeMake".into()));
        assert_eq!(
            session.member("Model"),
            MemberVal::Str("BeforeModel".into())
        );
        assert_eq!(session.member("RemoveMe"), MemberVal::Int(7));
        assert_eq!(session.option("Verbose"), MemberVal::Int(0));
        assert_eq!(session.warnings().len(), 1);
        assert_outer_scope(&session);
        assert!(!session.processed().admit(1, 2, 3, false));
        assert!(session.processed().admit(4, 5, 6, false));
    }

    #[test]
    fn staged_effects_commit_and_discard_typed_make_model_clearing() {
        let mut committed = staged_matrix_seed();
        let mut effects = StagedEffects::begin(&committed);
        clear_typed_make_model(effects.session_mut());
        effects.commit(&mut committed);
        assert!(!committed.has_member("Make"));
        assert!(!committed.has_member("Model"));

        let discarded = staged_matrix_seed();
        let mut effects = StagedEffects::begin(&discarded);
        clear_typed_make_model(effects.session_mut());
        effects.discard();
        assert_eq!(
            discarded.member("Make"),
            MemberVal::Str("BeforeMake".into())
        );
        assert_eq!(
            discarded.member("Model"),
            MemberVal::Str("BeforeModel".into())
        );
    }

    fn report_non_utf8_after_complete_matrix_mutation(
        session: &mut Session,
        _: u16,
        _: &MemberVal,
    ) -> Arm {
        mutate_staged_matrix(session);
        clear_typed_make_model(session);
        Arm::Report(conv::Report {
            value: None,
            print: None,
            writes: Vec::new(),
        })
    }

    #[test]
    fn reported_non_utf8_decline_discards_every_effect_before_residual() {
        static TAGS: [IfdTag; 1] = [plain(1, "NonUtf8")];
        static TABLE: IfdTable = table("Residual", &TAGS);
        let tag = &TAGS[0];
        let mut session = staged_matrix_seed();
        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members);
        let raw = DecodedValue::Undefined(vec![0xff]);
        let attempt = generated_arm(
            report_non_utf8_after_complete_matrix_mutation,
            &session,
            tag,
            &raw,
        );

        let arm = resolve_generated_attempt(attempt, &mut session, &mut ctx);
        assert_eq!(arm, Arm::Decline("a reported value is not UTF-8 text"));

        // These are live-session assertions at the production boundary:
        // resolve_generated_attempt has discarded before returning Decline,
        // and only now may the real residual observe or emit anything.
        assert_eq!(session.member("Make"), MemberVal::Str("BeforeMake".into()));
        assert_eq!(
            session.member("Model"),
            MemberVal::Str("BeforeModel".into())
        );
        assert_eq!(session.member("RemoveMe"), MemberVal::Int(7));
        assert_eq!(session.option("Verbose"), MemberVal::Int(0));
        assert_eq!(session.warnings().len(), 1);
        assert_outer_scope(&session);
        assert!(session.processed().admit(4, 5, 6, false));

        let dir = IfdDir {
            data: &[],
            data_domain: 0,
            ifd_start: 0,
            base: Some(0),
            byte_order: ByteOrder::Little,
            group1: Some("Test"),
        };
        let mut reads = RootReads {
            entries: vec![EntryRead::Decoded],
            ..Default::default()
        };
        let mut decoded = Some(&mut reads);
        let mut out = Vec::new();
        assert!(out.is_empty(), "the declined generated arm emitted no row");
        process_residual_entry(
            &TABLE,
            tag,
            &dir,
            raw.clone(),
            runtime::to_stored_tag_value(&raw, dir.byte_order),
            None,
            tag.omitted,
            true,
            &mut session,
            &mut ctx,
            &mut out,
            &mut decoded,
            0,
        );
        assert_eq!(out.len(), 1, "exactly one production residual row");
        assert_eq!(reads.rows, [(0, 0)], "the residual owns one occurrence");
        assert_eq!(out[0].value, TagValue::Binary(vec![0xff]));
    }

    #[test]
    fn staged_effects_clone_cost_measurement() {
        let mut session = staged_matrix_seed();
        for index in 0..32 {
            session
                .set_member(&format!("Member{index}"), MemberVal::Int(index))
                .unwrap();
            session.set_option(&format!("Option{index}"), MemberVal::Int(index));
        }
        let iterations = 20_000u32;
        let started = std::time::Instant::now();
        for _ in 0..iterations {
            std::hint::black_box(StagedEffects::begin(&session));
        }
        let elapsed = started.elapsed();
        eprintln!(
            "staged-effects-clone: iterations={iterations} elapsed_ns={} ns_per_clone={}",
            elapsed.as_nanos(),
            elapsed.as_nanos() / u128::from(iterations)
        );
        assert_eq!(session.member("Make"), MemberVal::Str("BeforeMake".into()));
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
            validation: None,
            processor: IfdSubdirProcessor::Native,
            unwalked: None,
        }
    }

    const fn pointer_edge(table: &'static str) -> IfdSubdirEdge {
        IfdSubdirEdge {
            module: "Test",
            table,
            start: IfdStart::Val(0),
            base: None,
            byte_order: IfdByteOrder::Inherit,
            fix_format: None,
            sub_ifd: true,
            max_subdirs: None,
            dir_name: None,
            validate: false,
            validation: None,
            processor: IfdSubdirProcessor::Native,
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
        let mut session = Session::new();
        let mut out = Vec::new();
        super::process_exif(
            table,
            IfdDir {
                data,
                data_domain: 0,
                ifd_start: 0,
                base,
                byte_order: order,
                group1: None,
            },
            &mut session,
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

    static OBSERVER_CHILD: IfdTable = table("ObserverChild", &[]);
    static OBSERVER_PARENT_TAGS: &[IfdTag] = &[
        plain(0x0001, "First"),
        IfdTag {
            subdir: Some(pointer_edge("ObserverChild")),
            ..plain(0x0010, "Child")
        },
    ];
    static OBSERVER_PARENT: IfdTable = table("ObserverParent", OBSERVER_PARENT_TAGS);

    struct UnknownLongObserver;

    impl IfdEntryObserver for UnknownLongObserver {
        fn observe(&mut self, event: IfdEntryEvent<'_, '_>, out: &mut Vec<Emitted>) {
            if event.entry.tag_id != 0x00f0 {
                return;
            }
            let Some(value) = event.declared_value().and_then(|value| value.as_integer()) else {
                return;
            };
            let value = TagValue::Integer(value);
            out.push(Emitted {
                module: event.table.module,
                table: event.table.table,
                group0: event.table.group0,
                group1: event.table.group1,
                group2: event.table.group2,
                name: "Observed",
                source_id: oxidex_tags::TagId::Numeric(event.entry.tag_id),
                stored: value.clone(),
                value,
                value_conv: None,
                low_priority: false,
                avoid: false,
                rational: None,
                is_list: false,
            });
        }
    }

    #[test]
    fn entry_observer_emissions_interleave_and_follow_recursive_descent() {
        let _registered = Registered::new(&[&OBSERVER_PARENT, &OBSERVER_CHILD]);
        let order = ByteOrder::Little;
        let child_start = 64usize;
        let mut data = ifd(
            order,
            &[
                int16u_entry(order, 0x0001, 1),
                entry(order, 0x00f0, 4, 1, bytes32(order, 11)),
                entry(order, 0x0010, 4, 1, bytes32(order, child_start as u32)),
                entry(order, 0x00f0, 4, 1, bytes32(order, 33)),
            ],
            &[],
        );
        data.resize(child_start, 0);
        data.extend_from_slice(&ifd(
            order,
            &[entry(order, 0x00f0, 4, 1, bytes32(order, 22))],
            &[],
        ));

        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut session = Session::new();
        let mut out = Vec::new();
        let mut observer = UnknownLongObserver;
        super::process_exif_with_observer(
            &OBSERVER_PARENT,
            IfdDir {
                data: &data,
                data_domain: 0,
                ifd_start: 0,
                base: Some(0),
                byte_order: order,
                group1: None,
            },
            &mut session,
            &mut ctx,
            &mut out,
            &mut observer,
        );

        assert_eq!(
            values(&out),
            [
                ("First", TagValue::Integer(1)),
                ("Observed", TagValue::Integer(11)),
                ("Observed", TagValue::Integer(22)),
                ("Observed", TagValue::Integer(33)),
            ]
        );
        assert_eq!(
            out.iter().map(|row| row.table).collect::<Vec<_>>(),
            [
                "ObserverParent",
                "ObserverParent",
                "ObserverChild",
                "ObserverParent",
            ]
        );
    }

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
            // `-n` (ExifTool.pm:3477): the rendered row carries its
            // pre-PrintConv value; the unconverted one needs none.
            assert_eq!(got[0].value_conv, Some(TagValue::Integer(2)), "{order:?}");
            assert_eq!(got[1].value_conv, None, "{order:?}");
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
        // A MakerNotes table (the fixture's group 0): the stored offset
        // points back into the directory itself (at the second entry's
        // bytes). Exif.pm:6549/6673-6678 calls that "Suspicious" and skips
        // it; table_ifd.rs's floor, kept for maker notes, refuses anything
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

    // -- K-O: Exif.pm:6539/6549, ExifTool's rules for non-MakerNotes tables ------

    /// [`STRINGS`]' tags in a table whose family-0 group is `EXIF`, as
    /// `Exif::Main`'s is: the overlap rule applies, not the floor.
    static EXIF_STRINGS: IfdTable = IfdTable {
        group0: "EXIF",
        ..table("ExifStrings", STRINGS_TAGS)
    };

    /// `data` with the directory at `ifd_start` (stored offsets are
    /// buffer-relative: `base: Some(0)`).
    fn run_at(table: &'static IfdTable, data: &[u8], ifd_start: usize) -> Vec<Emitted> {
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut out = Vec::new();
        process_exif(
            table,
            IfdDir {
                data,
                data_domain: 0,
                ifd_start,
                base: Some(0),
                byte_order: ByteOrder::Big,
                group1: None,
            },
            &mut ctx,
            &mut out,
        );
        out
    }

    /// A TIFF header (`MM\0*`, IFD0 at 16), `HELLO\0\0\0` at 8, then a
    /// one-entry directory at 16 whose ASCII value points at offset
    /// `value_at` (count 5).
    fn value_before_directory(value_at: u32) -> Vec<u8> {
        let order = ByteOrder::Big;
        let mut data = b"MM\0*\0\0\0\x10HELLO\0\0\0".to_vec();
        data.extend(ifd(
            order,
            &[entry(order, 0x0001, 2, 5, bytes32(order, value_at))],
            &[],
        ));
        data
    }

    /// Spec 7.1 test 13 (a) and (c): a value entirely BEFORE the directory,
    /// past the TIFF header, is read under `Exif::Main`'s rule (ExifTool
    /// reads it: Exif.pm:6549 refuses an overlap and Exif.pm:6539 an offset
    /// into the header, and this is neither; SonyILCE-7CM2.jpg's
    /// ExifIFD stores 30 such values), and still refused under a MakerNotes
    /// table's floor (D-4).
    #[test]
    fn k_o_reads_a_value_before_the_directory_outside_maker_notes_only() {
        let data = value_before_directory(8);
        assert_eq!(
            values(&run_at(&EXIF_STRINGS, &data, 16)),
            vec![("Text", TagValue::String("HELLO".to_string()))],
            "Exif::Main-shaped table: read"
        );
        assert!(
            run_at(&STRINGS, &data, 16).is_empty(),
            "MakerNotes table: the floor still refuses it"
        );
        // A value starting at `$dirEnd` -- the next-IFD link -- overlaps
        // nothing ExifTool checks: read by the overlap rule, refused by the
        // floor (`$dirEnd + 4`).
        let order = ByteOrder::Big;
        let dir_end = directory_end(0, 1);
        let mut data = ifd(
            order,
            &[entry(order, 0x0001, 2, 5, bytes32(order, dir_end as u32))],
            b"O\0",
        );
        data[dir_end..dir_end + 4].copy_from_slice(b"HELL");
        assert_eq!(
            values(&run_at(&EXIF_STRINGS, &data, 0)),
            vec![("Text", TagValue::String("HELLO".to_string()))]
        );
        assert!(run_at(&STRINGS, &data, 0).is_empty());
        // The real tables pick the rules by their own group 0.
        let exif = find_ifd_table("Exif", "Main").expect("Exif::Main");
        assert_eq!(
            DirectoryRule::for_table(exif, 8, 1),
            DirectoryRule::Overlap {
                dir_start: 8,
                dir_end: 22
            }
        );
        let olympus = find_ifd_table("Olympus", "Main").expect("Olympus::Main");
        assert_eq!(
            DirectoryRule::for_table(olympus, 8, 1),
            DirectoryRule::Floor(26)
        );
    }

    /// Spec 7.1 test 13 (b): under the overlap rule a value that overlaps
    /// the entry array -- starting inside it, or starting before the
    /// directory and running into it -- is "Suspicious" (Exif.pm:6673-6678):
    /// skipped, and it spends the directory's warning budget (Exif.pm:6455
    /// aborts once `$warnCount > 10`).
    #[test]
    fn k_o_refuses_an_overlap_and_spends_the_warning_budget() {
        let order = ByteOrder::Big;
        // Starts before the directory (at 14) and runs 3 bytes into it.
        let data = value_before_directory(14);
        assert!(run_at(&EXIF_STRINGS, &data, 16).is_empty(), "runs into it");
        // Starts inside the entry array (at the second entry, offset 14: past
        // the TIFF header, so only Exif.pm:6549 refuses it).
        let data = ifd(
            order,
            &[
                entry(order, 0x0001, 2, 5, bytes32(order, 14)),
                entry(order, 0x0003, 7, 1, [0x2a, 0, 0, 0]),
            ],
            &[],
        );
        assert_eq!(
            values(&run_at(&EXIF_STRINGS, &data, 0)),
            vec![("OneByte", TagValue::Integer(42))],
            "inside it: skipped, the next entry still read"
        );
        // Ten overlapping entries leave the budget at 10: the eleventh entry
        // is read. Eleven overlapping entries exhaust it: the walk stops.
        for (overlaps, read) in [(10usize, true), (11, false)] {
            let mut entries = vec![entry(order, 0x0001, 2, 5, bytes32(order, 14)); overlaps];
            entries.push(entry(order, 0x0003, 7, 1, [0x2a, 0, 0, 0]));
            let data = ifd(order, &entries, &[]);
            let got = values(&run_at(&EXIF_STRINGS, &data, 0));
            if read {
                assert_eq!(got, vec![("OneByte", TagValue::Integer(42))]);
            } else {
                assert!(got.is_empty(), "{overlaps} warnings: {got:?}");
            }
            // Every refusal here is ExifTool's own ("Suspicious ... offset",
            // then "Too many warnings"): `Refused`, never `Unread`, so a
            // caller with another reader for the same entries does not put
            // back what ExifTool refuses.
            let mut members = HashMap::new();
            let mut ctx = cond::Ctx::new(&mut members);
            let reads = process_exif_decoded(
                &EXIF_STRINGS,
                IfdDir {
                    data: &data,
                    data_domain: 0,
                    ifd_start: 0,
                    base: Some(0),
                    byte_order: order,
                    group1: None,
                },
                &mut ctx,
                &mut Vec::new(),
            )
            .unwrap()
            .entries;
            let mut expected = vec![EntryRead::Refused; overlaps];
            expected.push(if read {
                EntryRead::Decoded
            } else {
                EntryRead::Refused
            });
            assert_eq!(reads, expected, "{overlaps} overlaps");
        }
    }

    /// A bad FIRST entry is ExifTool's "assume corrupted IFD" (Exif.pm:
    /// 6474-6477): the whole directory is `Refused`, entry by entry.
    #[test]
    fn a_bad_first_entry_refuses_the_whole_directory() {
        let order = ByteOrder::Big;
        let data = ifd(
            order,
            &[
                entry(order, 0x0001, 99, 1, [0, 0, 0, 0]),
                entry(order, 0x0003, 7, 1, [0x2a, 0, 0, 0]),
            ],
            &[],
        );
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut out = Vec::new();
        let reads = process_exif_decoded(
            &EXIF_STRINGS,
            IfdDir {
                data: &data,
                data_domain: 0,
                ifd_start: 0,
                base: Some(0),
                byte_order: order,
                group1: None,
            },
            &mut ctx,
            &mut out,
        );
        assert!(out.is_empty());
        assert_eq!(
            reads.map(|reads| reads.entries),
            Some(vec![EntryRead::Refused, EntryRead::Refused])
        );
    }

    /// Exif.pm:6539 (`$valuePtr < 8 and not $$dirInfo{ZeroOffsetOK}`): an
    /// out-of-line value whose stored offset points into the 8-byte TIFF
    /// header is "Suspicious" (Exif.pm:6673-6678) even though it lies
    /// entirely before the directory -- skipped, one warning each. No
    /// `Exif::Main` directory sets `ZeroOffsetOK` (only Samsung.pm:1708's
    /// maker-note directory does). Offset 8, the first byte past the header,
    /// is read. A crafted JPEG whose IFD1 XResolution points at offset 0
    /// read 346409.125 without this check; pinned ExifTool 13.59 and the
    /// pre-K-O floor print nothing.
    #[test]
    fn k_o_refuses_an_offset_into_the_tiff_header() {
        let order = ByteOrder::Big;
        for at in [0u32, 1, 4, 7] {
            let data = value_before_directory(at);
            assert!(
                run_at(&EXIF_STRINGS, &data, 16).is_empty(),
                "offset {at} is in the TIFF header"
            );
            // The MakerNotes floor refuses it too (the directory does not
            // lie before the base).
            assert!(run_at(&STRINGS, &data, 16).is_empty(), "offset {at}");
        }
        assert_eq!(
            values(&run_at(&EXIF_STRINGS, &value_before_directory(8), 16)),
            vec![("Text", TagValue::String("HELLO".to_string()))]
        );
        // Each header offset spends the warning budget like an overlap: ten
        // leave the eleventh entry readable, eleven stop the walk.
        for (suspicious, read) in [(10usize, true), (11, false)] {
            let mut entries = vec![entry(order, 0x0001, 2, 5, bytes32(order, 0)); suspicious];
            entries.push(entry(order, 0x0003, 7, 1, [0x2a, 0, 0, 0]));
            let mut data = b"MM\0*\0\0\0\x08".to_vec();
            data.extend(ifd(order, &entries, &[]));
            let got = values(&run_at(&EXIF_STRINGS, &data, 8));
            if read {
                assert_eq!(got, vec![("OneByte", TagValue::Integer(42))]);
            } else {
                assert!(got.is_empty(), "{suspicious} warnings: {got:?}");
            }
            // ExifTool's own refusal: `Refused`, never `Unread`, so the
            // ExifIFD caller does not put the hand arm's reading of the
            // header bytes back (crafted `hdroff0.jpg`: control printed
            // ExposureTime 346409.1, pinned 13.59 nothing).
            let mut members = HashMap::new();
            let mut ctx = cond::Ctx::new(&mut members);
            let reads = process_exif_decoded(
                &EXIF_STRINGS,
                IfdDir {
                    data: &data,
                    data_domain: 0,
                    ifd_start: 8,
                    base: Some(0),
                    byte_order: order,
                    group1: None,
                },
                &mut ctx,
                &mut Vec::new(),
            )
            .unwrap()
            .entries;
            let mut expected = vec![EntryRead::Refused; suspicious];
            expected.push(if read {
                EntryRead::Decoded
            } else {
                EntryRead::Refused
            });
            assert_eq!(reads, expected, "{suspicious} header offsets");
        }
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

    /// `process_exif_decoded`: one [`EntryRead`] per root entry --
    /// `Decoded` once its value was read (even if nothing is reported for
    /// it), `Refused` where ExifTool refuses the same entry (a bad type code,
    /// an offset past the data), `Unread` for the engine's own gaps; `None`
    /// for a refused directory. And `Emitted::rational` keeps an unconverted
    /// single rational's fraction.
    #[test]
    fn process_exif_decoded_flags_read_entries_and_rows_keep_the_fraction() {
        let order = ByteOrder::Big;
        let floor = trailer_at(4) as u32;
        let data = ifd(
            order,
            &[
                // Read and reported: 3/2 out of line.
                entry(order, 0x0002, 5, 1, bytes32(order, floor)),
                // An entry type ProcessExif refuses (not the first entry).
                entry(order, 0x0001, 99, 1, [0, 0, 0, 0]),
                // Past the end of `data`.
                entry(order, 0x0003, 5, 2, bytes32(order, floor + 100)),
                // Read, but no tag in the table: reported by nobody.
                entry(order, 0x00ee, 3, 1, [0, 7, 0, 0]),
            ],
            &[0, 0, 0, 3, 0, 0, 0, 2],
        );
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut out = Vec::new();
        let dir = IfdDir {
            data: &data,
            data_domain: 0,
            ifd_start: 0,
            base: Some(0),
            byte_order: order,
            group1: None,
        };
        let decoded = process_exif_decoded(&NUMERIC, dir, &mut ctx, &mut out);
        let decoded = decoded.expect("an accepted directory");
        assert_eq!(
            decoded.entries,
            [
                EntryRead::Decoded,
                EntryRead::Refused,
                EntryRead::Refused,
                EntryRead::Unread,
            ]
        );
        assert_eq!(decoded.rows, [(0, 0)], "the one row came from entry 0");
        assert_eq!(values(&out), vec![("Ratio", TagValue::Float(1.5))]);
        assert_eq!(out[0].rational, Some((3, 2)));
        assert_eq!(out[0].value_conv, None);
        assert_eq!(out[0].stored, TagValue::new_rational(3, 2));
        assert_eq!(out[0].source_id, oxidex_tags::TagId::Numeric(0x0002));
        assert!(!out[0].is_list);

        let generated = generated_row(
            &NUMERIC,
            &NUMERIC_TAGS[1],
            "Numeric",
            &DecodedValue::UnsignedRational(1, 3),
            TagValue::new_rational(1, 3),
            Some((1, 3)),
            conv::Report {
                value: None,
                print: None,
                writes: Vec::new(),
            },
        );
        assert_eq!(generated.stored, TagValue::new_rational(1, 3));
        assert_eq!(generated.source_id, oxidex_tags::TagId::Numeric(0x0002));
        let refused = process_exif_decoded(
            &NUMERIC,
            IfdDir {
                data: &data[..10],
                ..dir
            },
            &mut ctx,
            &mut Vec::new(),
        );
        assert_eq!(refused, None);
    }

    #[test]
    fn real_static_and_generated_walks_preserve_physical_string_and_rational_storage() {
        fn exercise(
            table: &'static IfdTable,
            string_id: u16,
            utf8_id: u16,
            rational_id: u16,
            expected_display: &str,
        ) {
            let order = ByteOrder::Big;
            let floor = trailer_at(3) as u32;
            let string_bytes = [b'A', 0xff, b'B', 0, 0];
            let mut trailer = string_bytes.to_vec();
            trailer.extend_from_slice(&1u32.to_be_bytes());
            trailer.extend_from_slice(&3u32.to_be_bytes());
            let data = ifd(
                order,
                &[
                    entry(order, string_id, 2, 5, bytes32(order, floor)),
                    entry(order, utf8_id, 129, 3, [b'A', 0xff, b'B', 0]),
                    entry(
                        order,
                        rational_id,
                        5,
                        1,
                        bytes32(order, floor + string_bytes.len() as u32),
                    ),
                ],
                &trailer,
            );
            let mut members = HashMap::new();
            let mut ctx = cond::Ctx::new(&mut members);
            let mut session = Session::new();
            let mut rows = Vec::new();
            super::process_exif(
                table,
                IfdDir {
                    data: &data,
                    data_domain: 0,
                    ifd_start: 0,
                    base: Some(0),
                    byte_order: order,
                    group1: table.set_group1.map(|_| table.group1),
                },
                &mut session,
                &mut ctx,
                &mut rows,
            );
            let string = rows
                .iter()
                .find(|row| row.source_id == oxidex_tags::TagId::Numeric(string_id))
                .unwrap_or_else(|| {
                    panic!(
                        "string row from the real {}::{} table walk; rows={rows:?}",
                        table.module, table.table
                    )
                });
            assert_eq!(string.stored, TagValue::Binary(vec![b'A', 0xff, b'B']));
            assert_eq!(
                string.value,
                TagValue::String(expected_display.to_owned()),
                "the existing display conversion must not change"
            );

            let utf8 = rows
                .iter()
                .find(|row| row.source_id == oxidex_tags::TagId::Numeric(utf8_id))
                .expect("malformed TIFF type 129 row from the real table walk");
            assert_eq!(
                utf8.stored,
                TagValue::Binary(vec![b'A', 0xff, b'B']),
                "type 129 storage must retain the exact bytes before FixUTF8"
            );
            assert_eq!(
                utf8.value,
                TagValue::String("A?B".to_owned()),
                "the existing typed/display FixUTF8 behavior must not change"
            );

            let rational = rows
                .iter()
                .find(|row| row.source_id == oxidex_tags::TagId::Numeric(rational_id))
                .expect("rational row from the real table walk");
            assert_eq!(rational.stored, TagValue::new_rational(1, 3));
            match &rational.value {
                TagValue::Float(value) => assert_eq!(*value, 0.333_333_333_3),
                TagValue::String(value) => assert!(
                    value.contains("0.3333333333"),
                    "converted rational display kept its existing rounded quotient: {value}"
                ),
                other => panic!("unexpected rational display shape: {other:?}"),
            }
        }

        let olympus = find_ifd_table("Olympus", "Equipment").expect("Olympus::Equipment");
        assert!(conv::decoder(olympus).is_none(), "static residual control");
        exercise(olympus, 0x0100, 0x0102, 0x0103, "Unknown (A?B)");

        let exif = find_ifd_table("Exif", "Main").expect("Exif::Main");
        let image_description = exif
            .tags
            .iter()
            .find(|tag| tag.id == 0x010e)
            .expect("ImageDescription");
        let x_resolution = exif
            .tags
            .iter()
            .find(|tag| tag.id == 0x011a)
            .expect("XResolution");
        let document_name = exif
            .tags
            .iter()
            .find(|tag| tag.id == 0x010d)
            .expect("DocumentName");
        assert!(conv::claims(exif, image_description));
        assert!(conv::claims(exif, document_name));
        assert!(conv::claims(exif, x_resolution));
        exercise(exif, 0x010e, 0x010d, 0x011a, "A?B");
    }

    #[test]
    fn one_session_refuses_a_repeated_root_but_allows_a_distinct_address() {
        let order = ByteOrder::Big;
        let one = ifd(order, &[int16u_entry(order, 0x0001, 7)], &[]);
        let mut data = one.clone();
        let second_start = data.len();
        data.extend_from_slice(&one);
        let mut session = Session::new();
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut out = Vec::new();

        let first = IfdDir {
            data: &data,
            data_domain: 0,
            ifd_start: 0,
            base: Some(0),
            byte_order: order,
            group1: Some("First"),
        };
        assert!(
            super::process_exif_decoded(&NUMERIC, first, &mut session, &mut ctx, &mut out)
                .is_some()
        );
        assert_eq!(out.len(), 1);

        assert_eq!(
            super::process_exif_decoded(&NUMERIC, first, &mut session, &mut ctx, &mut out),
            None,
            "the file-scoped processed set must reject the same table/address"
        );

        let distinct = IfdDir {
            ifd_start: second_start,
            group1: Some("Second"),
            ..first
        };
        assert!(
            super::process_exif_decoded(&NUMERIC, distinct, &mut session, &mut ctx, &mut out)
                .is_some(),
            "the same table at a distinct address is a legitimate duplicate"
        );
        assert_eq!(out.len(), 2);
    }

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
        let mut members = HashMap::new();
        members.insert("Model", MemberValue::Bytes(b"ILCE-7M4\xe9".to_vec()));
        assert_eq!(
            values(&run_with(&PLAIN, &data, order, Some(0), &mut members)),
            vec![("Raw", TagValue::Integer(7))],
            "an invalid byte after the ASCII model prefix must not erase it"
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
        let mut members = HashMap::new();
        members.insert("Make", MemberValue::Bytes(b"Apple".to_vec()));
        assert_eq!(
            values(&run_with(&PLAIN, &data, order, Some(0), &mut members)),
            vec![
                ("Raw", TagValue::Integer(7)),
                ("Mode", TagValue::String("Unknown (9)".to_string()))
            ],
            "an ASCII byte scalar is the same Make value native Perl compares"
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

    const TEST_VERSION_IS_TWO: Cond = Cond::MemberCmp {
        member: "TestVersion",
        op: CmpOp::Eq,
        value: 2,
    };
    const COMPRESSION_IS_CLEAR: Cond = Cond::MemberStrEq {
        member: "Compression",
        value: "",
        negate: false,
    };
    static FILE_SESSION_CHILD_GROUPS: &[IfdVariantGroup] = &[IfdVariantGroup {
        id: 0x0001,
        alternatives: &[
            (TEST_VERSION_IS_TWO, plain(0x0001, "ChildSawParent")),
            (Cond::Always, plain(0x0001, "ChildMissedParent")),
        ],
    }];
    static FILE_SESSION_CHILD_TAGS: &[IfdTag] = &[IfdTag {
        raw_conv: Some(RawConvEffect::SetMember {
            member: "Compression",
        }),
        ..plain(0x0002, "TemporaryCompression")
    }];
    static FILE_SESSION_CHILD: IfdTable = IfdTable {
        variants: FILE_SESSION_CHILD_GROUPS,
        ..table("FileSessionChild", FILE_SESSION_CHILD_TAGS)
    };
    static FILE_SESSION_PARENT_GROUPS: &[IfdVariantGroup] = &[IfdVariantGroup {
        id: 0x0003,
        alternatives: &[
            (COMPRESSION_IS_CLEAR, plain(0x0003, "SiblingIsClean")),
            (Cond::Always, plain(0x0003, "SiblingSawChildState")),
        ],
    }];
    static FILE_SESSION_PARENT_TAGS: &[IfdTag] = &[
        IfdTag {
            raw_conv: Some(RawConvEffect::SetMember {
                member: "TestVersion",
            }),
            ..plain(0x0001, "Version")
        },
        IfdTag {
            subdir: Some(edge("FileSessionChild")),
            ..plain(0x0002, "ToChild")
        },
    ];
    static FILE_SESSION_PARENT: IfdTable = IfdTable {
        variants: FILE_SESSION_PARENT_GROUPS,
        ..table("FileSessionParent", FILE_SESSION_PARENT_TAGS)
    };

    #[test]
    fn child_and_sibling_share_file_members_but_not_child_directory_state() {
        let order = ByteOrder::Little;
        let child = ifd(
            order,
            &[
                int16u_entry(order, 0x0001, 7),
                int16u_entry(order, 0x0002, 9),
            ],
            &[],
        );
        let child_at = trailer_at(3) as u32;
        let data = ifd(
            order,
            &[
                int16u_entry(order, 0x0001, 2),
                entry(
                    order,
                    0x0002,
                    7,
                    child.len() as u32,
                    bytes32(order, child_at),
                ),
                int16u_entry(order, 0x0003, 8),
            ],
            &child,
        );
        let _registered = Registered::new(&[&FILE_SESSION_CHILD]);
        let mut members = HashMap::new();

        assert_eq!(
            values(&run_with(
                &FILE_SESSION_PARENT,
                &data,
                order,
                Some(0),
                &mut members,
            )),
            vec![
                ("Version", TagValue::Integer(2)),
                ("ChildSawParent", TagValue::Integer(7)),
                ("TemporaryCompression", TagValue::Integer(9)),
                ("SiblingIsClean", TagValue::Integer(8)),
            ]
        );
        assert_eq!(members.get("TestVersion"), Some(&MemberValue::Num(2)));
        assert!(
            !members.contains_key("Compression"),
            "the child's directory-local Compression must be restored"
        );
    }

    const MISSING_MEMBER_IS_ONE: Cond = Cond::MemberCmp {
        member: "NeverSet",
        op: CmpOp::Eq,
        value: 1,
    };
    const LOSING_ASSIGNMENT: Cond = Cond::SetMember {
        member: "LosingWrite",
        source: EffectSource::Const(5),
        then: Some(&MISSING_MEMBER_IS_ONE),
    };
    const LOSING_WRITE_IS_FIVE: Cond = Cond::MemberCmp {
        member: "LosingWrite",
        op: CmpOp::Eq,
        value: 5,
    };
    static LOSING_ASSIGNMENT_GROUPS: &[IfdVariantGroup] = &[IfdVariantGroup {
        id: 0x0001,
        alternatives: &[
            (LOSING_ASSIGNMENT, plain(0x0001, "MustLose")),
            (LOSING_WRITE_IS_FIVE, plain(0x0001, "LaterSeesWrite")),
        ],
    }];
    static LOSING_ASSIGNMENT_TABLE: IfdTable = IfdTable {
        variants: LOSING_ASSIGNMENT_GROUPS,
        ..table("LosingAssignment", &[])
    };

    #[test]
    fn losing_condition_assignment_reaches_later_condition_and_file_session() {
        let order = ByteOrder::Little;
        let data = ifd(order, &[int16u_entry(order, 0x0001, 7)], &[]);
        let mut members = HashMap::new();
        let mut ctx = cond::Ctx::new(&mut members);
        let mut session = Session::new();
        let mut out = Vec::new();

        super::process_exif(
            &LOSING_ASSIGNMENT_TABLE,
            IfdDir {
                data: &data,
                data_domain: 0,
                ifd_start: 0,
                base: Some(0),
                byte_order: order,
                group1: None,
            },
            &mut session,
            &mut ctx,
            &mut out,
        );

        assert_eq!(values(&out), vec![("LaterSeesWrite", TagValue::Integer(7))]);
        assert_eq!(members.get("LosingWrite"), Some(&MemberValue::Num(5)));
        assert_eq!(session.member("LosingWrite"), MemberVal::Int(5));
    }

    // A standalone IFD condition is not a variant: Sony::Main's 0x202a
    // parent edge has this exact shape. It must be evaluated before the
    // adapter follows SubDirectory, with this entry's bytes rather than a
    // stale ancestor context.
    const STARTS_WITH_ONE: Cond = Cond::ValPtRegex {
        pattern: r"^\x01",
        negate: false,
    };
    static DIRECT_CONDITION_TAGS: &[IfdTag] = &[IfdTag {
        condition: Some(STARTS_WITH_ONE),
        ..plain(0x002a, "Child")
    }];
    static DIRECT_CONDITION: IfdTable = IfdTable {
        variants: &[],
        ..table("DirectCondition", DIRECT_CONDITION_TAGS)
    };

    #[test]
    fn a_direct_ifd_condition_uses_its_own_value_bytes() {
        let order = ByteOrder::Little;
        let mut members = HashMap::new();
        let yes = ifd(order, &[int16u_entry(order, 0x002a, 1)], &[]);
        assert_eq!(
            values(&run_with(
                &DIRECT_CONDITION,
                &yes,
                order,
                Some(0),
                &mut members
            )),
            vec![("Child", TagValue::Integer(1))]
        );
        let no = ifd(order, &[int16u_entry(order, 0x002a, 2)], &[]);
        assert!(
            run_with(&DIRECT_CONDITION, &no, order, Some(0), &mut members).is_empty(),
            "a false direct Condition drops the parent tag before an adapter could descend"
        );
    }

    // A source-selected serial child is keyed only by generated module/table
    // facts. The parent fixture supplies a TIFF `undef[16]` entry whose first
    // child word is its declared byte size, the closed U16 validator shape.
    static SERIAL_EDGE_TAGS: &[IfdTag] = &[IfdTag {
        subdir: Some(IfdSubdirEdge {
            module: "Canon",
            table: "AFInfo2",
            validation: Some(U16SizeCheck {
                offset: 0,
                expected: &[SizeExpectation::Relative(0)],
                expression: "Test::Validate($dirData,$subdirStart,$size)",
                callee: "Test::Validate",
                source_file: "Image/ExifTool/Test.pm",
                source_sha256: "test",
                reader_contract_sha256: Some("test"),
            }),
            processor: IfdSubdirProcessor::Serial,
            validate: true,
            ..edge("AFInfo2")
        }),
        ..plain(0x0026, "SerialChild")
    }];
    static SERIAL_EDGE: IfdTable = table("SerialEdge", SERIAL_EDGE_TAGS);

    fn serial_child_parent(order: ByteOrder, declared_size: u16) -> Vec<u8> {
        let floor = trailer_at(1);
        let mut child = Vec::new();
        for word in [declared_size, 2, 0, 1, 2, 3, 4, 5] {
            child.extend_from_slice(&bytes16(order, word));
        }
        ifd(
            order,
            &[entry(
                order,
                0x0026,
                7,
                child.len() as u32,
                bytes32(order, floor as u32),
            )],
            &child,
        )
    }

    #[test]
    fn authenticated_serial_edge_checks_size_then_uses_generated_child_table() {
        for order in [ByteOrder::Big, ByteOrder::Little] {
            let data = serial_child_parent(order, 16);
            let mut members = HashMap::new();
            let mut ctx = Ctx::new(&mut members);
            let mut out = Vec::new();
            let reads = process_exif_decoded(
                &SERIAL_EDGE,
                IfdDir {
                    data: &data,
                    data_domain: 0,
                    ifd_start: 0,
                    base: Some(0),
                    byte_order: order,
                    group1: None,
                },
                &mut ctx,
                &mut out,
            )
            .expect("the parent IFD is valid");
            assert_eq!(
                reads.serial_subdirs,
                vec![(0, SerialSubdirRead::Handled)],
                "{order:?}: a supported child is a handled source route"
            );
            assert_eq!(
                reads.serial_rows,
                (0..7).map(|row| (row, 0)).collect::<Vec<_>>(),
                "{order:?}: child rows retain their parent-entry position"
            );
            assert_eq!(
                values(&out),
                vec![
                    (
                        "AFAreaMode",
                        TagValue::String("Single-point AF".to_string())
                    ),
                    ("NumAFPoints", TagValue::Integer(0)),
                    ("ValidAFPoints", TagValue::Integer(1)),
                    ("CanonImageWidth", TagValue::Integer(2)),
                    ("CanonImageHeight", TagValue::Integer(3)),
                    ("AFImageWidth", TagValue::Integer(4)),
                    ("AFImageHeight", TagValue::Integer(5)),
                ],
                "{order:?}: child selection, cursor and source enums come from SerialTable"
            );
        }
    }

    #[test]
    fn failed_serial_size_check_is_a_handled_native_omission() {
        let data = serial_child_parent(ByteOrder::Little, 15);
        let mut members = HashMap::new();
        let mut ctx = Ctx::new(&mut members);
        let mut out = Vec::new();
        let reads = process_exif_decoded(
            &SERIAL_EDGE,
            IfdDir {
                data: &data,
                data_domain: 0,
                ifd_start: 0,
                base: Some(0),
                byte_order: ByteOrder::Little,
                group1: None,
            },
            &mut ctx,
            &mut out,
        )
        .expect("the parent IFD is valid");
        assert!(
            out.is_empty(),
            "native Validate false produces no child rows"
        );
        assert_eq!(reads.serial_subdirs, vec![(0, SerialSubdirRead::Handled)]);
        assert!(reads.serial_rows.is_empty());
    }

    #[test]
    fn tainted_serial_child_discards_an_earlier_buffered_prefix() {
        let mut out = vec!["parent"];
        let outcome = finish_serial_child(
            &mut out,
            vec!["serial-prefix"],
            &SerialWalkResult {
                tainted: true,
                ..SerialWalkResult::default()
            },
        );
        assert_eq!(outcome, SerialSubdirRead::Fallback);
        assert_eq!(out, vec!["parent"]);
    }

    #[test]
    fn refused_owned_serial_walk_restores_child_state_but_keeps_parent_effects() {
        let source = find_serial_table("Canon", "AFInfo2").unwrap();
        let mut entries = source.entries.to_vec();
        let mut later = entries[3].alternatives.to_vec();
        // Deliberately inconsistent artifact: the earlier NumAFPoints
        // SetMember executes before this later unsupported source hook.
        later[0].omitted.hook = true;
        entries[3].alternatives = Box::leak(later.into_boxed_slice());
        let refused = Box::leak(Box::new(SerialTable {
            entries: Box::leak(entries.into_boxed_slice()),
            ..*source
        }));
        let data: Vec<u8> = [16u16, 2, 9, 1, 2, 3, 4, 5]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        let mut members = HashMap::from([
            ("AFInfo3", MemberValue::Num(1)),
            ("NumAFPoints", MemberValue::Num(7)),
        ]);
        let before = members.clone();
        let mut ctx = Ctx::new(&mut members);
        let mut out = Vec::new();
        let outcome = owned_serial_attempt(&mut ctx, &mut out, |ctx, out| {
            let mut child = Vec::new();
            let result = process_serial_directory(
                refused,
                SerialDir {
                    data: &data,
                    dir_start: 0,
                    dir_len: data.len(),
                    base: 0,
                    data_pos: 0,
                    byte_order: ByteOrder::Little,
                },
                ctx,
                &mut IfdSerialSink { out: &mut child },
            );
            assert!(result.tainted);
            assert_eq!(ctx.members.get("NumAFPoints"), Some(&MemberValue::Num(9)));
            assert_eq!(child.len(), 2, "a real generated prefix was read");
            DescendOutcome::Serial(finish_serial_child(out, child, &result))
        });
        assert_eq!(outcome, DescendOutcome::Serial(SerialSubdirRead::Refused));
        assert!(out.is_empty());
        assert_eq!(*ctx.members, before);
        assert!(
            Cond::MemberCmp {
                member: "NumAFPoints",
                op: CmpOp::Eq,
                value: 7,
            }
            .eval(&mut ctx)
        );
        assert!(
            Cond::MemberTruthy {
                member: "AFInfo3",
                negate: false,
            }
            .eval(&mut ctx)
        );
    }

    #[test]
    fn missing_proof_on_owned_serial_edge_is_refusal_not_legacy_fallback() {
        let mut tags = SERIAL_EDGE_TAGS.to_vec();
        tags[0].subdir.as_mut().unwrap().validation = None;
        let parent = Box::leak(Box::new(IfdTable {
            tags: Box::leak(tags.into_boxed_slice()),
            ..SERIAL_EDGE
        }));
        let data = serial_child_parent(ByteOrder::Little, 16);
        let mut members = HashMap::from([("AFInfo3", MemberValue::Num(1))]);
        let mut ctx = Ctx::new(&mut members);
        let mut out = Vec::new();
        let reads = process_exif_decoded(
            parent,
            IfdDir {
                data: &data,
                data_domain: 0,
                ifd_start: 0,
                base: Some(0),
                byte_order: ByteOrder::Little,
                group1: None,
            },
            &mut ctx,
            &mut out,
        )
        .unwrap();
        assert_eq!(reads.serial_subdirs, vec![(0, SerialSubdirRead::Refused)]);
        assert!(out.is_empty());
        assert_eq!(ctx.members.get("AFInfo3"), Some(&MemberValue::Num(1)));
    }

    // FujiFilm.pm:709-714 and Olympus.pm:809-822 supply direct (not
    // `_variants`) member conditions. `process_exif` must use the caller's
    // file-level state for both string predicate forms; a missing or wrong
    // member is a false condition, not permission to report the tag.
    const GENERAL_IMAGING_MAKE: Cond = Cond::MemberRegex {
        member: "Make",
        pattern: "^GENERAL IMAGING",
        ignore_case: false,
        negate: false,
    };
    const ERF_TIFF_TYPE: Cond = Cond::MemberStrEq {
        member: "TIFF_TYPE",
        value: "ERF",
        negate: false,
    };
    static DIRECT_MEMBER_CONDITION_TAGS: &[IfdTag] = &[
        IfdTag {
            condition: Some(GENERAL_IMAGING_MAKE),
            ..plain(0x002b, "MakeScoped")
        },
        IfdTag {
            condition: Some(ERF_TIFF_TYPE),
            ..plain(0x002c, "TypeScoped")
        },
    ];
    static DIRECT_MEMBER_CONDITIONS: IfdTable = IfdTable {
        variants: &[],
        ..table("DirectMemberConditions", DIRECT_MEMBER_CONDITION_TAGS)
    };

    #[test]
    fn direct_ifd_member_conditions_use_the_supplied_file_state() {
        let order = ByteOrder::Little;
        let data = ifd(
            order,
            &[
                int16u_entry(order, 0x002b, 1),
                int16u_entry(order, 0x002c, 2),
            ],
            &[],
        );
        for (make, tiff_type, expected) in [
            (
                Some("GENERAL IMAGING CO."),
                Some("TIFF"),
                vec![("MakeScoped", TagValue::Integer(1))],
            ),
            (
                Some("FUJIFILM"),
                Some("ERF"),
                vec![("TypeScoped", TagValue::Integer(2))],
            ),
            (Some("FUJIFILM"), Some("TIFF"), vec![]),
            (None, None, vec![]),
        ] {
            let mut members = HashMap::new();
            if let Some(make) = make {
                members.insert("Make", MemberValue::Str(make.to_string()));
            }
            if let Some(tiff_type) = tiff_type {
                members.insert("TIFF_TYPE", MemberValue::Str(tiff_type.to_string()));
            }
            assert_eq!(
                values(&run_with(
                    &DIRECT_MEMBER_CONDITIONS,
                    &data,
                    order,
                    Some(0),
                    &mut members,
                )),
                expected,
                "Make={make:?}, TIFF_TYPE={tiff_type:?}"
            );
        }
    }

    // Canon.pm:1598-1604 (pinned 13.59) uses a direct condition on
    // CanonAFInfo, `$$self{AFInfoCount} = $count`, immediately before its
    // SubDirectory. Canon.pm:6484-6489 then reads AFInfoCount when choosing
    // a child layout. Keep the state write and the descent in one test: a
    // direct condition that is evaluated after descent, or not at all, would
    // make the child take its fallback branch.
    const SET_AF_INFO_COUNT: Cond = Cond::SetMember {
        member: "AFInfoCount",
        source: EffectSource::Count,
        then: None,
    };
    const AF_INFO_COUNT_IS_TWO: Cond = Cond::MemberCmp {
        member: "AFInfoCount",
        op: CmpOp::Eq,
        value: 2,
    };
    static AF_COUNT_CHILD_GROUPS: &[IfdVariantGroup] = &[IfdVariantGroup {
        id: 0x0002,
        alternatives: &[
            (AF_INFO_COUNT_IS_TWO, plain(0x0002, "CountTwo")),
            (Cond::Always, plain(0x0002, "CountOther")),
        ],
    }];
    static AF_COUNT_CHILD: IfdTable = IfdTable {
        variants: AF_COUNT_CHILD_GROUPS,
        ..table("AFCountChild", &[])
    };
    static AF_COUNT_PARENT_TAGS: &[IfdTag] = &[IfdTag {
        condition: Some(SET_AF_INFO_COUNT),
        omitted: Omitted {
            value_conv: false,
            raw_conv: false,
            condition: false,
            hook: false,
            subdirectory: true,
            print_conv: false,
        },
        subdir: Some(IfdSubdirEdge {
            start: IfdStart::Val(0),
            sub_ifd: true,
            max_subdirs: Some(1),
            ..edge("AFCountChild")
        }),
        ..plain(0x0012, "CanonAFInfo")
    }];
    static AF_COUNT_PARENT: IfdTable = table("AFCountParent", AF_COUNT_PARENT_TAGS);

    #[test]
    fn a_direct_condition_stores_count_before_a_child_variant_is_selected() {
        let order = ByteOrder::Little;
        let _registered = Registered::new(&[&AF_COUNT_CHILD]);
        let child = ifd(order, &[int16u_entry(order, 0x0002, 7)], &[]);

        // A count of two needs two out-of-line int32u pointer values. The
        // child is the first (MaxSubdirs limits the native loop to it), so
        // the direct condition must store this entry's count before descent.
        let pointers_at = trailer_at(1) as u32;
        let child_at = pointers_at + 8;
        let mut two_trailer = Vec::new();
        two_trailer.extend_from_slice(&bytes32(order, child_at));
        two_trailer.extend_from_slice(&bytes32(order, 0));
        two_trailer.extend_from_slice(&child);
        let two = ifd(
            order,
            &[entry(order, 0x0012, 4, 2, bytes32(order, pointers_at))],
            &two_trailer,
        );
        let mut members = HashMap::new();
        assert_eq!(
            values(&run_with(
                &AF_COUNT_PARENT,
                &two,
                order,
                Some(0),
                &mut members,
            )),
            vec![("CountTwo", TagValue::Integer(7))],
            "the direct Condition writes AFInfoCount before the child resolves"
        );
        assert_eq!(members.get("AFInfoCount"), Some(&MemberValue::Num(2)));

        // The negative control is deliberately a valid single-pointer entry,
        // not a malformed count: it must select the fallback child layout.
        // A stale/late count write, or a constant instead of `$count`, cannot
        // satisfy both this and the two-pointer case.
        let child_at = trailer_at(1) as u32;
        let one = ifd(
            order,
            &[entry(order, 0x0012, 4, 1, bytes32(order, child_at))],
            &child,
        );
        let mut members = HashMap::new();
        assert_eq!(
            values(&run_with(
                &AF_COUNT_PARENT,
                &one,
                order,
                Some(0),
                &mut members,
            )),
            vec![("CountOther", TagValue::Integer(7))],
            "a count of one reaches the same child but selects its fallback"
        );
        assert_eq!(members.get("AFInfoCount"), Some(&MemberValue::Num(1)));
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
                data_domain: 0,
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
