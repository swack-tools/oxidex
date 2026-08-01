#!/usr/bin/env perl
#
# Emit Rust source for every %Image::ExifTool::InfiRay::* binary-data table
# straight from ExifTool's own in-memory Perl hashes, so the Rust copy cannot
# drift from the Perl one through transcription.
#
#   perl scripts/gen_infiray_tables.pl > src/parsers/jpeg/app_segments/infiray_tables.rs
#
# Design rules, following scripts/gen_olympus_lookups.pl and the generators
# behind #241 / #249 / #319:
#
#  * Dump the LOADED hash, never the .pm text. A hash that failed to load, or
#    whose key moved, disappears here instead of being half-parsed.
#  * Hard-error on any construct this script has not seen -- an unknown Format,
#    an unknown PrintConv expression, a PrintConv hash or code ref, a stray
#    table-level key. A silent skip becomes a false explanation downstream, so
#    every refusal aborts the run and names the exact construct.
#  * Trace every emitted Name to a literal line of InfiRay.pm. A name that is
#    not in the source file byte-for-byte, exactly once, aborts the run. This
#    is what makes a fabricated tag name impossible to emit.
#  * Derive the per-APPn dispatch gates from ExifTool.pm's own source lines
#    rather than restating them, and abort if a gate's shape is not recognised.

use strict;
use warnings;

my $EXIFTOOL_LIB = $ENV{OXIDEX_EXIFTOOL_LIB} || '/tmp/oxidex-exiftool-cache/exiftool/lib';
use lib map { $_ } ($ENV{OXIDEX_EXIFTOOL_LIB} || '/tmp/oxidex-exiftool-cache/exiftool/lib');

require Image::ExifTool;
require Image::ExifTool::InfiRay;

my $INFIRAY_PM  = "$EXIFTOOL_LIB/Image/ExifTool/InfiRay.pm";
my $EXIFTOOL_PM = "$EXIFTOOL_LIB/Image/ExifTool.pm";

# ---------------------------------------------------------------------------
# Source text, used only to prove each emitted name exists literally.
# ---------------------------------------------------------------------------
open(my $fh, '<', $INFIRAY_PM) or die "cannot read $INFIRAY_PM: $!\n";
my @INFIRAY_LINES = <$fh>;
close $fh;

# Returns the 1-based line of InfiRay.pm that literally declares this name.
# Dies unless exactly one line does.
sub source_line_of_name {
    my ($name) = @_;
    my @hits;
    for my $i (0 .. $#INFIRAY_LINES) {
        my $line = $INFIRAY_LINES[$i];
        next if $line =~ /^\s*#/;          # commented-out tags are not emitted
        push @hits, $i + 1 if $line =~ /Name\s*=>\s*'\Q$name\E'/;
    }
    die "REFUSING: name '$name' is not declared literally in $INFIRAY_PM "
        . "(found " . scalar(@hits) . " matches, expected 1)\n"
        unless @hits == 1;
    return $hits[0];
}

# ---------------------------------------------------------------------------
# Formats. Keys are ExifTool format names; values are the Rust `Fmt` variant.
# Anything outside this map aborts the run.
# ---------------------------------------------------------------------------
my %FORMAT = (
    int8u  => 'Int8u',
    int8s  => 'Int8s',
    int16u => 'Int16u',
    int16s => 'Int16s',
    int32u => 'Int32u',
    int32s => 'Int32s',
    int64u => 'Int64u',
    float  => 'Float',
    string => 'Str',
);

# ---------------------------------------------------------------------------
# PrintConv expressions, matched as literal Perl source. InfiRay.pm builds
# these from four shared hashes (%convFloat2, %convPercent, %convMeters,
# %convCelsius, InfiRay.pm:21-24); by the time the module is loaded they are
# plain strings, which is what arrives here.
# ---------------------------------------------------------------------------
my %PRINT_CONV = (
    'sprintf("%.2f", $val)'         => 'Float2',
    'sprintf("%.1f %%", $val * 100)' => 'Percent',
    'sprintf("%.2f m", $val)'       => 'Meters',
    'sprintf("%.2f C", $val)'       => 'Celsius',
);

# Table-level keys this script understands. Anything else aborts: a new key
# could change how the record is read (FORMAT, FIRST_ENTRY, DATAMEMBER, ...).
my %TABLE_KEYS_OK = map { $_ => 1 } qw(
    GROUPS PROCESS_PROC VARS NOTES WRITE_PROC CHECK_PROC
    TABLE_NAME SHORT_NAME PARENT PRIORITY WRITABLE
);

# Per-tag keys this script understands.
my %TAG_KEYS_OK = map { $_ => 1 } qw(Name Format PrintConv);

# ---------------------------------------------------------------------------
# The tables, in APPn order. `rust` is the emitted static's name.
# ---------------------------------------------------------------------------
my @TABLES = (
    { app => 2, perl => 'Version',    rust => 'VERSION' },
    { app => 4, perl => 'Factory',    rust => 'FACTORY' },
    { app => 5, perl => 'Picture',    rust => 'PICTURE' },
    { app => 6, perl => 'MixMode',    rust => 'MIX_MODE' },
    { app => 7, perl => 'OpMode',     rust => 'OP_MODE' },
    { app => 8, perl => 'Isothermal', rust => 'ISOTHERMAL' },
    { app => 9, perl => 'Sensor',     rust => 'SENSOR' },
);

# ---------------------------------------------------------------------------
# Dispatch gates, read out of ExifTool.pm's own source.
#
# The InfiRay records carry no identifier of their own, so ExifTool gates each
# one on `$$self{HasIJPEG}` plus a minimum segment length. Those numbers live
# in ExifTool.pm, not InfiRay.pm, so they are lifted from the literal lines
# that set $dumpType.
# ---------------------------------------------------------------------------
sub read_gates {
    open(my $g, '<', $EXIFTOOL_PM) or die "cannot read $EXIFTOOL_PM: $!\n";
    my @lines = <$g>;
    close $g;

    my %gates;
    for my $i (0 .. $#lines) {
        next unless $lines[$i] =~ /\$dumpType\s*=\s*'InfiRay\s+(\w+)'/;
        my $table = $1;
        # The gate is the `if`/`elsif` immediately above the $dumpType line.
        my $cond = $lines[$i - 1];
        my $line_no = $i;   # 1-based line number of the condition

        if ($cond =~ /\$\$self\{HasIJPEG\}\s+and\s+\$length\s*>=\s*(\d+)/) {
            $gates{$table} = { min => $1, line => $line_no, kind => 'len' };
        } elsif ($cond =~ /\$\$segDataPt\s*=~\s*\/\^\.\.\.\.IJPEG\\0\/s/) {
            # APP2 is the header that SETS HasIJPEG; its gate is the signature
            # itself: four bytes of version, then "IJPEG\0".
            $gates{$table} = { min => 0, line => $line_no, kind => 'sig' };
        } elsif ($cond =~ /\$\$self\{HasIJPEG\}\s+or\s+\$\$self\{Make\}\s+eq\s+'DJI'/) {
            # APP3 ImagingData: no length gate at all.
            $gates{$table} = { min => 0, line => $line_no, kind => 'none' };
        } else {
            chomp(my $shown = $cond);
            $shown =~ s/^\s+//;
            die "REFUSING: unrecognised dispatch gate for 'InfiRay $table' at "
                . "$EXIFTOOL_PM:$line_no -- $shown\n";
        }
    }
    return \%gates;
}

my $GATES = read_gates();

# ---------------------------------------------------------------------------
# Emit
# ---------------------------------------------------------------------------
sub esc {
    my $s = shift;
    $s =~ s/\\/\\\\/g;
    $s =~ s/"/\\"/g;
    return $s;
}

my @out;
my $total = 0;

for my $t (@TABLES) {
    my $perl_name = $t->{perl};
    no strict 'refs';
    my $table = \%{"Image::ExifTool::InfiRay::$perl_name"};
    use strict 'refs';
    die "REFUSING: %Image::ExifTool::InfiRay::$perl_name is empty or absent\n"
        unless %$table;

    # A FORMAT key would make the numeric keys index units rather than byte
    # offsets, and every offset below would silently move.
    die "REFUSING: %InfiRay::$perl_name declares FORMAT => '$$table{FORMAT}'; "
        . "this generator assumes the int8u default, i.e. byte offsets\n"
        if defined $table->{FORMAT};

    for my $k (sort keys %$table) {
        next if $k =~ /^\d+$/ || $k =~ /^0x[0-9a-f]+$/i;
        die "REFUSING: unhandled table-level key '$k' in %InfiRay::$perl_name\n"
            unless $TABLE_KEYS_OK{$k};
    }

    my $gate = $GATES->{$perl_name}
        or die "REFUSING: no dispatch gate found in $EXIFTOOL_PM for "
             . "'InfiRay $perl_name'\n";

    my @numeric = sort { $a <=> $b } grep { /^\d+$/ } keys %$table;
    die "REFUSING: %InfiRay::$perl_name has no numeric tag keys\n" unless @numeric;

    my @rows;
    for my $idx (@numeric) {
        my $info = $table->{$idx};
        die "REFUSING: %InfiRay::$perl_name key $idx is a "
            . (ref($info) || 'plain scalar') . ", not a HASH; a Condition list "
            . "or code ref needs handling this generator does not have\n"
            unless ref($info) eq 'HASH';

        for my $k (sort keys %$info) {
            die "REFUSING: unhandled tag key '$k' on %InfiRay::$perl_name"
                . sprintf(" 0x%x", $idx) . "\n"
                unless $TAG_KEYS_OK{$k};
        }

        my $name = $info->{Name}
            or die "REFUSING: %InfiRay::$perl_name" . sprintf(" 0x%x", $idx)
                 . " has no Name\n";
        my $src_line = source_line_of_name($name);

        # Format. Absent means the table default, which is int8u (count 1).
        my ($fmt, $count) = ('int8u', 1);
        if (defined(my $f = $info->{Format})) {
            die "REFUSING: Format on %InfiRay::$perl_name $name is a "
                . ref($f) . " ref, not a string\n" if ref $f;
            if ($f =~ /^(\w+)\[(\d+)\]$/) {
                ($fmt, $count) = ($1, $2);
            } elsif ($f =~ /^(\w+)$/) {
                ($fmt, $count) = ($1, 1);
            } else {
                die "REFUSING: unparsable Format '$f' on "
                    . "%InfiRay::$perl_name $name\n";
            }
            die "REFUSING: unknown Format '$fmt' on %InfiRay::$perl_name $name "
                . "(InfiRay.pm:$src_line)\n" unless $FORMAT{$fmt};
        }

        # PrintConv.
        my $conv = 'Raw';
        if (defined(my $pc = $info->{PrintConv})) {
            if (ref($pc) eq 'CODE') {
                die "REFUSING: PrintConv on %InfiRay::$perl_name $name is a code "
                    . "ref; a hash dump cannot see its body. Probe it with "
                    . "sentinel values and add an explicit arm.\n";
            }
            if (ref($pc) eq 'HASH') {
                die "REFUSING: PrintConv on %InfiRay::$perl_name $name is a lookup "
                    . "hash; this generator only handles sprintf expressions.\n";
            }
            die "REFUSING: PrintConv on %InfiRay::$perl_name $name is a "
                . ref($pc) . " ref\n" if ref $pc;
            $conv = $PRINT_CONV{$pc}
                or die "REFUSING: unknown PrintConv expression on "
                     . "%InfiRay::$perl_name $name (InfiRay.pm:$src_line):\n"
                     . "    $pc\n";
        }

        push @rows, {
            idx   => $idx,
            name  => $name,
            fmt   => $FORMAT{$fmt},
            count => $count,
            conv  => $conv,
            line  => $src_line,
        };
        $total++;
    }

    push @out, {
        %$t,
        rows => \@rows,
        gate => $gate,
        notes => scalar(@rows),
    };
}

# ---------------------------------------------------------------------------
# Print the Rust module.
# ---------------------------------------------------------------------------
print <<"VERSIONED";
//! InfiRay IJPEG binary-data tables, GENERATED by
//! `scripts/gen_infiray_tables.pl` from ExifTool's in-memory
//! `%Image::ExifTool::InfiRay::*` hashes. Do not edit by hand.
//!
//! Source: ExifTool $Image::ExifTool::VERSION. Every `ExifTool.pm:` and
//! `InfiRay.pm:` line number below is a line of THAT release; they move
//! between releases.
VERSIONED

print <<'HEADER';
//!
//! Every `name` below is proven to appear literally in ExifTool's
//! `InfiRay.pm`; the trailing comment on each row is the line it came from.
//! The generator aborts rather than guessing at an unknown `Format`, an
//! unknown `PrintConv` expression, a `PrintConv` hash or code ref, or an
//! unrecognised APPn dispatch gate.
//!
//! The output is already rustfmt-stable: regenerating and running
//! `cargo fmt` leaves the file byte-identical.
//!
//! The tables declare no `FORMAT`, so ExifTool's `ProcessBinaryData` default
//! of `int8u` applies and the numeric keys are plain byte offsets.

/// One field of an InfiRay binary-data record.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Field {
    /// Byte offset of the field within the record.
    pub offset: usize,
    /// ExifTool tag name.
    pub name: &'static str,
    /// ExifTool `Format`, absent meaning the table's `int8u` default.
    pub format: Fmt,
    /// Element count from `Format`'s `[n]` suffix, else 1.
    pub count: usize,
    /// ExifTool `PrintConv`, or [`Conv::Raw`] when the table declares none.
    pub conv: Conv,
}

/// The ExifTool formats these tables use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fmt {
    Int8u,
    Int8s,
    Int16u,
    Int16s,
    Int32u,
    Int32s,
    Int64u,
    Float,
    Str,
}

/// The `PrintConv` expressions these tables use (`InfiRay.pm:21-24`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Conv {
    /// No `PrintConv`: ExifTool prints the raw value.
    Raw,
    /// `sprintf("%.2f", $val)`
    Float2,
    /// `sprintf("%.1f %%", $val * 100)`
    Percent,
    /// `sprintf("%.2f m", $val)`
    Meters,
    /// `sprintf("%.2f C", $val)`
    Celsius,
}

HEADER

for my $t (@out) {
    my $gate = $t->{gate};
    my $app  = $t->{app};
    my $perl = $t->{perl};
    my $rust = $t->{rust};
    my $n    = scalar @{$t->{rows}};

    if ($gate->{kind} eq 'len') {
        printf("/// Minimum APP%d payload before ExifTool reads an InfiRay %s\n", $app, $perl);
        printf("/// record (`ExifTool.pm:%d`: `\$\$self{HasIJPEG} and \$length >= %d`).\n",
               $gate->{line}, $gate->{min});
        printf("pub(crate) const %s_MIN_LENGTH: usize = %d;\n\n", $rust, $gate->{min});
    } elsif ($gate->{kind} eq 'sig') {
        printf("/// The APP%d %s header carries its own signature and no length\n", $app, $perl);
        printf("/// gate (`ExifTool.pm:%d`: `\$\$segDataPt =~ /^....IJPEG\\0/s`),\n", $gate->{line});
        printf("/// so the ten signature bytes are the only minimum.\n");
        printf("pub(crate) const %s_MIN_LENGTH: usize = 10;\n\n", $rust);
    } else {
        printf("/// ExifTool gates APP%d %s on `\$\$self{HasIJPEG}` alone\n", $app, $perl);
        printf("/// (`ExifTool.pm:%d`), with no minimum length.\n", $gate->{line});
        printf("pub(crate) const %s_MIN_LENGTH: usize = 0;\n\n", $rust);
    }

    printf("/// `%%Image::ExifTool::InfiRay::%s` -- JPEG APP%d, %d field%s.\n",
           $perl, $app, $n, $n == 1 ? '' : 's');
    # One row per source line is the whole point of the trailing citations;
    # rustfmt would explode each row across seven lines and lose that.
    printf("#[rustfmt::skip]\n");
    printf("pub(crate) static %s: &[Field] = &[\n", $rust);
    for my $r (@{$t->{rows}}) {
        printf(
            "    Field { offset: 0x%03x, name: \"%s\", format: Fmt::%s, count: %d, conv: Conv::%s }, // InfiRay.pm:%d\n",
            $r->{idx}, esc($r->{name}), $r->{fmt}, $r->{count}, $r->{conv}, $r->{line},
        );
    }
    print "];\n\n";
}

printf("/// Total fields across every generated table.\n");
printf("#[cfg(test)]\n");
printf("pub(crate) const GENERATED_FIELD_COUNT: usize = %d;\n", $total);

print STDERR "gen_infiray_tables.pl: emitted $total fields across "
    . scalar(@out) . " tables\n";
