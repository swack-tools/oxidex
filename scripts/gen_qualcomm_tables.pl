#!/usr/bin/env perl
#
# Emit Rust source for the Qualcomm APP7 reader straight from ExifTool's own
# in-memory %Image::ExifTool::Qualcomm::Main hash, so the Rust copy cannot
# drift from the Perl one through transcription.
#
#   perl scripts/gen_qualcomm_tables.pl > src/parsers/jpeg/app_segments/qualcomm_tables.rs
#
# Design rules, following scripts/gen_infiray_tables.pl and the generators
# behind #241 / #249 / #319 / #330:
#
#  * Dump the LOADED hash, never the .pm text. A hash that failed to load, or
#    whose key moved, disappears here instead of being half-parsed.
#  * Hard-error on any construct this script has not seen -- a table entry that
#    carries anything beyond the Name/Description pair MakeNameAndDesc writes,
#    a stray table-level key, a format list of unexpected length or content, or
#    a dispatch gate whose shape is not recognised. A silent skip becomes a
#    false explanation downstream, so every refusal aborts and names the exact
#    construct.
#  * Qualcomm::Main is unusual: it declares VARS => { ID_FMT => 'none',
#    NO_LOOKUP => 1 } and every one of its entries is an EMPTY hash in the
#    source. The Name and Description are not written by a human at all -- they
#    are generated at module load by Qualcomm::MakeNameAndDesc. So there is no
#    static table worth transcribing: the content is the ALGORITHM. This script
#    therefore emits the algorithm's inputs (signature, directory offset,
#    format codes) plus a fixture of all 1188 key/Name/Description triples that
#    the Rust port of MakeNameAndDesc must reproduce exactly. That fixture is
#    ExifTool's own output, so a divergence in the Rust port fails the build.
#  * ExifTool extracts tags from this segment even when they are absent from
#    the table (Qualcomm.pm's AddTagToTable branch), which is the second reason
#    a static 1188-row table would be the wrong shape.

use strict;
use warnings;

my $EXIFTOOL_LIB = $ENV{OXIDEX_EXIFTOOL_LIB} || '/tmp/oxidex-exiftool-cache/exiftool/lib';
use lib map { $_ } ($ENV{OXIDEX_EXIFTOOL_LIB} || '/tmp/oxidex-exiftool-cache/exiftool/lib');

require Image::ExifTool;
require Image::ExifTool::Qualcomm;

my $QUALCOMM_PM = "$EXIFTOOL_LIB/Image/ExifTool/Qualcomm.pm";
my $EXIFTOOL_PM = "$EXIFTOOL_LIB/Image/ExifTool.pm";

sub slurp {
    my ($path) = @_;
    open(my $fh, '<', $path) or die "cannot read $path: $!\n";
    local $/;
    my $text = <$fh>;
    close $fh;
    return $text;
}

my $QUALCOMM_TEXT = slurp($QUALCOMM_PM);
my $EXIFTOOL_TEXT = slurp($EXIFTOOL_PM);

sub line_of {
    my ($text, $path, $re, $what) = @_;
    my @hits;
    my @lines = split /\n/, $text, -1;
    for my $i (0 .. $#lines) {
        push @hits, $i + 1 if $lines[$i] =~ $re;
    }
    die "REFUSING: $what is not declared exactly once in $path "
      . "(found " . scalar(@hits) . " matches, expected 1)\n"
      unless @hits == 1;
    return $hits[0];
}

# ---------------------------------------------------------------------------
# Table-level shape. Anything outside this set means the table grew a construct
# this generator has never seen, so it must not silently emit a stale reader.
# ---------------------------------------------------------------------------
no warnings 'once';   # the table is reached only through this one reference
my $TABLE = \%Image::ExifTool::Qualcomm::Main;
use warnings 'once';

my %KNOWN_TABLE_KEYS = map { $_ => 1 } qw(GROUPS NOTES PROCESS_PROC VARS);
for my $k (sort keys %$TABLE) {
    next unless $k =~ /^[A-Z][A-Z_0-9]*$/;
    die "REFUSING: unrecognised table-level key '$k' in Qualcomm::Main\n"
        unless $KNOWN_TABLE_KEYS{$k};
}

# GROUPS drives the family-0/1 names oxidex must emit.
my $g = $TABLE->{GROUPS} or die "REFUSING: Qualcomm::Main has no GROUPS\n";
die "REFUSING: unexpected GROUPS in Qualcomm::Main: "
  . join(',', map { "$_=$$g{$_}" } sort keys %$g) . "\n"
    unless ($g->{0} // '') eq 'MakerNotes'
       and ($g->{2} // '') eq 'Camera'
       and !exists $g->{1};

# VARS: ID_FMT 'none' + NO_LOOKUP is what makes the string tag IDs legal.
my $vars = $TABLE->{VARS} or die "REFUSING: Qualcomm::Main has no VARS\n";
die "REFUSING: unexpected VARS in Qualcomm::Main: "
  . join(',', map { "$_=$$vars{$_}" } sort keys %$vars) . "\n"
    unless ($vars->{ID_FMT} // '') eq 'none' and ($vars->{NO_LOOKUP} // 0);

# ---------------------------------------------------------------------------
# Format codes. Qualcomm.pm's @qualcommFormat is lexical (`my`), so it cannot be
# read out of the symbol table; it is parsed from the source and every element
# is checked against the ExifTool format names oxidex already knows how to read.
# ---------------------------------------------------------------------------
my %RUST_FMT = (
    int8u  => 'Int8u',  int8s  => 'Int8s',
    int16u => 'Int16u', int16s => 'Int16s',
    int32u => 'Int32u', int32s => 'Int32s',
    float  => 'Float',  double => 'Double',
);

my ($fmt_block) = $QUALCOMM_TEXT =~ /my\s+\@qualcommFormat\s*=\s*\(\s*(.*?)\s*\)\s*;/s
    or die "REFUSING: cannot find \@qualcommFormat in $QUALCOMM_PM\n";
my @FORMATS = ($fmt_block =~ /'([a-z0-9]+)'/g);
die "REFUSING: \@qualcommFormat has " . scalar(@FORMATS) . " entries, expected 8\n"
    unless @FORMATS == 8;
for my $f (@FORMATS) {
    die "REFUSING: unknown ExifTool format '$f' in \@qualcommFormat\n"
        unless $RUST_FMT{$f};
}
my $FMT_LINE = line_of($QUALCOMM_TEXT, $QUALCOMM_PM,
    qr/my\s+\@qualcommFormat\s*=/, '\@qualcommFormat');

# ---------------------------------------------------------------------------
# Dispatch gate, read out of ExifTool.pm rather than restated. Both the APP7
# signature and the 27-byte DirStart come from the same `elsif` branch.
# ---------------------------------------------------------------------------
my $SIG_LINE = line_of($EXIFTOOL_TEXT, $EXIFTOOL_PM,
    qr/\$\$segDataPt =~ \/\^\\x1aQualcomm Camera Attributes\//,
    'the APP7 Qualcomm signature test');

my ($dirstart) = $EXIFTOOL_TEXT =~
    /\$\$segDataPt =~ \/\^\\x1aQualcomm Camera Attributes\/.*?DirStart\(\\%dirInfo,\s*(\d+)\s*\)/s
    or die "REFUSING: cannot find the DirStart offset for the Qualcomm APP7 branch "
         . "in $EXIFTOOL_PM; its dispatch shape has changed\n";
my $DIRSTART_LINE = line_of($EXIFTOOL_TEXT, $EXIFTOOL_PM,
    qr/DirStart\(\\%dirInfo,\s*27\s*\)\s*;/, 'the Qualcomm DirStart call');

# The signature literal and the offset must agree: DirStart skips exactly the
# signature, no more and no less. If ExifTool ever changes one without the
# other this check is what catches it.
my $SIGNATURE = "\x1aQualcomm Camera Attributes";
die "REFUSING: DirStart offset $dirstart does not equal the "
  . length($SIGNATURE) . "-byte signature length; the branch has changed\n"
    unless $dirstart == length($SIGNATURE);

# ---------------------------------------------------------------------------
# The 1188 generated names. Every entry must be exactly the {Name, Description}
# pair MakeNameAndDesc writes over an empty hash -- anything else means a human
# added a Format/PrintConv this reader would silently ignore.
# ---------------------------------------------------------------------------
my @FIXTURE;
for my $key (sort keys %$TABLE) {
    next if $key =~ /^[A-Z][A-Z_0-9]*$/;   # table-level control keys
    my $info = $TABLE->{$key};
    die "REFUSING: Qualcomm::Main entry '$key' is a " . (ref($info) || 'scalar')
      . ", expected a HASH\n" unless ref($info) eq 'HASH';
    my @k = sort keys %$info;
    die "REFUSING: Qualcomm::Main entry '$key' carries [" . join(',', @k)
      . "], expected exactly [Description,Name]. A Format or PrintConv here "
      . "would be silently dropped by this reader.\n"
        unless @k == 2 and $k[0] eq 'Description' and $k[1] eq 'Name';
    # The key must appear literally in the source, which is what makes a
    # fabricated tag id impossible to emit.
    die "REFUSING: tag id '$key' does not appear literally in $QUALCOMM_PM\n"
        unless index($QUALCOMM_TEXT, "'$key'") >= 0;
    push @FIXTURE, [$key, $info->{Name}, $info->{Description}];
}
die "REFUSING: Qualcomm::Main yielded no entries\n" unless @FIXTURE;

sub rs_str {
    my ($s) = @_;
    $s =~ s/\\/\\\\/g;
    $s =~ s/"/\\"/g;
    return "\"$s\"";
}

my $ET_VERSION = $Image::ExifTool::VERSION;
my $Q_VERSION  = $Image::ExifTool::Qualcomm::VERSION;
my $N = scalar @FIXTURE;

# ---------------------------------------------------------------------------
print <<"HDR";
//! Qualcomm APP7 constants and name fixture, GENERATED by
//! `scripts/gen_qualcomm_tables.pl` from ExifTool's in-memory
//! `%Image::ExifTool::Qualcomm::Main` hash. Do not edit by hand.
//!
//! Source: ExifTool $ET_VERSION, Qualcomm.pm $Q_VERSION. Every `ExifTool.pm:` and
//! `Qualcomm.pm:` line number below is a line of THAT release; they move
//! between releases.
//!
//! `%Image::ExifTool::Qualcomm::Main` is not a transcribable table. Every one
//! of its $N entries is an EMPTY hash in Qualcomm.pm; the `Name` and
//! `Description` are written at module load by `Qualcomm::MakeNameAndDesc`
//! (Qualcomm.pm), and ExifTool extracts tags from this segment even when they
//! are absent from the table. The content is therefore the ALGORITHM, ported
//! in `qualcomm.rs`, and what is generated here is the fixture that proves the
//! port reproduces ExifTool's own output for all $N known ids.
//!
//! The generator aborts rather than guessing at a table entry carrying
//! anything beyond that Name/Description pair, a stray table-level key, an
//! unknown format code, or an unrecognised APP7 dispatch gate.
//!
//! The output is already rustfmt-stable: regenerating and running
//! `cargo fmt` leaves the file byte-identical.

/// Numeric value formats, indexed by the format byte of each APP7 entry.
///
/// `Qualcomm.pm:$FMT_LINE` (`\@qualcommFormat`). A format byte above the end of
/// this list is not an error: ExifTool falls back to the raw bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fmt {
HDR

my %emitted;
for my $f (@FORMATS) {
    next if $emitted{ $RUST_FMT{$f} }++;
    print "    /// ExifTool `$f`.\n";
    print "    $RUST_FMT{$f},\n";
}
print "}\n\n";

print "/// The eight format codes, in the order ExifTool indexes them.\n";
print "///\n";
print "/// `Qualcomm.pm:$FMT_LINE`.\n";
print "pub(crate) const FORMATS: [Fmt; 8] = [\n";
print "    Fmt::$RUST_FMT{$_},\n" for @FORMATS;
print "];\n\n";

print <<"CONST";
/// The APP7 payload prefix that selects this reader.
///
/// `ExifTool.pm:$SIG_LINE` (`\$\$segDataPt =~ /^\\x1aQualcomm Camera Attributes/`).
pub(crate) const SIGNATURE: &[u8] = b"\\x1aQualcomm Camera Attributes";

/// Bytes of the APP7 payload that precede the first entry.
///
/// `ExifTool.pm:$DIRSTART_LINE` (`DirStart(\\%dirInfo, $dirstart)`), which is exactly
/// the length of [`SIGNATURE`]; the generator refuses to emit these two if
/// they ever disagree.
pub(crate) const DIR_START: usize = $dirstart;

CONST

print <<"FIX";
/// Every tag id listed in `%Image::ExifTool::Qualcomm::Main`, paired with the
/// `Name` and `Description` ExifTool's `MakeNameAndDesc` generated for it.
///
/// This is ExifTool's own output, not a transcription, and it exists so the
/// Rust port of that function can be checked against all $N of them.
///
/// `rustfmt::skip` keeps one id per line. Without it rustfmt explodes the
/// longer rows across four lines each, which would make the generated file and
/// the formatted file differ and leave `cargo fmt --check` failing after every
/// regeneration.
#[cfg(test)]
#[rustfmt::skip]
pub(crate) const NAME_FIXTURE: [(&str, &str, &str); $N] = [
FIX

for my $row (@FIXTURE) {
    print "    (" . join(', ', map { rs_str($_) } @$row) . "),\n";
}
print "];\n";
