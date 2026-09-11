#!/usr/bin/env perl
# Transcribe the pinned Canon/Pentax LensType alternatives. With no output
# option, retain the historical Rust-body stdout interface. --full-file,
# --out FILE and --check FILE use the complete, rustfmt-normalized module.
# Unsupported table shapes are refused before emitting or replacing anything.
use strict;
use warnings;
BEGIN {
    for my $name (qw(PERL5OPT PERL5LIB PERLLIB)) {
        die "refusing ambient $name; select the source with --exiftool-dir\n"
            if defined $ENV{$name} && length $ENV{$name};
    }
}
use B ();
use Cwd qw(abs_path);
use Encode qw(decode encode FB_CROAK);
use File::Basename qw(dirname);
use File::Temp qw(tempfile);
use FindBin;
use Getopt::Long qw(GetOptions);

my $root = abs_path("$FindBin::Bin/../..");
my $dir = '/tmp/oxidex-exiftool-cache/exiftool';
my ($full, $out, $check, $help);
my $rustfmt = $ENV{RUSTFMT} || 'rustfmt';
GetOptions('exiftool-dir=s' => \$dir, 'full-file' => \$full,
           'out=s' => \$out, 'check=s' => \$check, 'rustfmt=s' => \$rustfmt,
           'help' => \$help) or die "bad options; use --help\n";
if ($help) {
    print "usage: $0 --exiftool-dir TREE [--full-file | --out FILE | --check FILE] [--rustfmt PROGRAM]\n";
    exit 0;
}
die "unexpected positional arguments\n" if @ARGV;
die "--out and --check are mutually exclusive\n" if defined $out && defined $check;
$full ||= defined $out || defined $check;
open my $pin_fh, '<', "$root/.exiftool-version" or die "cannot read repository pin: $!\n";
my $pin = do { local $/; <$pin_fh> }; close $pin_fh;
$pin =~ s/\s+\z//;
die "invalid repository pin\n" unless $pin =~ /^\d+\.\d+\z/;
$dir = abs_path($dir) or die "selected ExifTool tree does not exist\n";
my $lib = "$dir/lib";
my @modules = qw(Image/ExifTool.pm Image/ExifTool/Canon.pm Image/ExifTool/Pentax.pm
                 Image/ExifTool/Olympus.pm Image/ExifTool/Panasonic.pm);
for my $module (@modules) {
    die "selected source missing $module\n" unless -f "$lib/$module";
}
unshift @INC, $lib;
for my $module (@modules) {
    require $module;
    my $loaded = defined $INC{$module} ? abs_path($INC{$module}) : undef;
    die "module $module loaded outside the selected source\n"
        unless defined $loaded && $loaded eq abs_path("$lib/$module");
}
my $version = $Image::ExifTool::VERSION;
die "selected ExifTool version '$version' disagrees with repository pin '$pin'\n"
    unless defined $version && $version eq $pin;
printf STDERR "ExifTool %s from %s; Perl %s\n", $version, $dir, $^V;

sub string_value {
    my ($label, $key, $value) = @_;
    die "$label key '$key': expected a truthy string scalar\n"
        unless defined($value) && !ref($value)
        && (B::svref_2object(\$value)->FLAGS & B::SVp_POK()) && $value;
    # ExifTool modules contain both Latin-1 bytes and Unicode strings. Match
    # dump_tables.pl's text decoding, without letting interpolation convert a
    # numeric/ref value into an apparently valid string before validation.
    return $value if utf8::is_utf8($value);
    my $copy = $value;
    my $decoded = eval { decode('UTF-8', $copy, FB_CROAK) };
    return defined $decoded ? $decoded : decode('ISO-8859-1', $value);
}

sub rows {
    my ($label, $table) = @_;
    die "$label: expected a hash\n" unless ref($table) eq 'HASH';
    my (%base, %fractional, %labels);
    my $id_pattern = $label =~ /^canon(?:_rf)?$/ ? qr/(?:0|-?[1-9]\d*)/ : qr/-?\d+(?: \d+)*/;
    for my $key (sort keys %$table) {
        # Pentax's known unknown-ID PrintConv hook is not a lens ID. No other
        # metadata or nonliteral row is silently dropped.
        if (($label eq 'pentax' || $label eq 'olympus') && $key eq 'Notes') {
            string_value($label, $key, $table->{$key});
            next;
        }
        if ($label eq 'pentax' && $key eq 'OTHER') {
            die "pentax OTHER: expected CODE directive\n" unless ref($table->{$key}) eq 'CODE';
            next;
        }
        if ($key =~ /^($id_pattern)\.([1-9]\d*)\z/) {
            $fractional{$1}{$2} = string_value($label, $key, $table->{$key});
        } elsif ($key =~ /^$id_pattern\z/) {
            if ($label =~ /^canon(?:_rf)?$/) {
                my $digits = $key; $digits =~ s/^-//;
                my $limit = $key =~ /^-/ ? '9223372036854775808' : '9223372036854775807';
                die "canon: raw ID outside i64 range\n"
                    if length($digits) > length($limit) || (length($digits) == length($limit) && $digits gt $limit);
            }
            $base{$key} = string_value($label, $key, $table->{$key});
            push @{$labels{$base{$key}}}, $key;
        } else {
            die "$label: unsupported key '$key'\n";
        }
    }
    die "$label: empty base lens table\n" unless keys %base;
    die "$label: empty alternative population\n"
        if $label ne 'olympus' && $label ne 'canon_rf' && !keys %fractional;
    for my $id (sort keys %fractional) {
        die "$label: orphan alternatives for '$id'\n" unless exists $base{$id};
        my @indices = sort { $a <=> $b } keys %{$fractional{$id}};
        for my $i (0 .. $#indices) {
            die "$label: noncontiguous alternatives for '$id'\n" unless $indices[$i] == $i + 1;
        }
        die "$label: ambiguous label '$base{$id}' is shared by multiple base IDs\n"
            if $label !~ /^canon(?:_rf)?$/ && @{$labels{$base{$id}}} != 1;
    }
    # Both upstream PrintLensID loops stop on Perl false values, not exists().
    # Requiring every fractional value to be a truthy string and every chain
    # to be contiguous makes the emitted complete chain exactly that loop.
    my @rows;
    for my $id (sort { ($a =~ /^-?\d+\z/ && $b =~ /^-?\d+\z/) ? $a <=> $b : $a cmp $b } keys %{ $label =~ /^canon(?:_rf)?$/ ? \%base : \%fractional }) {
        push @rows, [$id, $base{$id}, [map { $fractional{$id}{$_} }
                                    sort { $a <=> $b } keys %{$fractional{$id} || {}}]];
    }
    return (\@rows, scalar(keys %base), scalar(grep { /\./ } keys %$table));
}

sub rs {
    my ($value) = @_;
    $value =~ s/([\\"])/\\$1/g;
    $value =~ s/([\x00-\x1f\x7f])/sprintf('\\u{%x}', ord($1))/ge;
    return qq{"$value"};
}
sub emit {
    my ($constant, $doc, $rows) = @_;
    my $canon = $constant =~ /^CANON(?:_RF)?_LENS_ALTERNATIVES$/;
    my $shape = $canon ? '(i64, &str, &[&str])' : '(&str, &[&str])';
    my $text = "$doc\n" . sprintf("pub static %s: [%s; %d] = [\n", $constant, $shape, scalar @$rows);
    for my $row (@$rows) {
        $text .= sprintf("    // id %s\n    (%s%s,\n     &[%s]),\n", $row->[0], $canon ? "$row->[0], " : '', rs($row->[1]), join(', ', map { rs($_) } @{$row->[2]}));
    }
    return $text . "];\n\n";
}

no warnings 'once';
my ($canon, $canon_base, $canon_frac) = rows('canon', \%Image::ExifTool::Canon::canonLensTypes);
my ($rf, $rf_base, $rf_frac) = rows('canon_rf', $Image::ExifTool::Canon::FileInfo{61}{PrintConv});
die "Canon RF grew fractional alternatives; explicit runtime support required\n" if $rf_frac;
my ($pentax) = rows('pentax', \%Image::ExifTool::Pentax::pentaxLensTypes);
my $olympus = $Image::ExifTool::Olympus::Equipment{0x0201}{PrintConv};
my ($oly_rows, $oly_base, $oly_frac) = rows('olympus', $olympus);
die "olympusLensTypes grew fractional keys; its empty runtime alternatives table is no longer correct\n" if $oly_frac;
printf STDERR "Canon: %d rows / %d alternatives; Pentax: %d rows / %d alternatives; Olympus: %d keys / 0 fractional\n",
    scalar(@$canon), $canon_frac, scalar(@$pentax), scalar(map { @{$_->[2]} } @$pentax), $oly_base;
my $body = emit('CANON_LENS_ALTERNATIVES',
    "/// `%Image::ExifTool::Canon::canonLensTypes`: the " . scalar(@$canon) . " integer\n"
  . "/// ids, keyed by raw ID and retaining the exact base label. Empty slices\n"
  . "/// mean no alternatives; nonempty slices follow ExifTool's `.1 .. .N` order.", $canon)
  . emit('CANON_RF_LENS_ALTERNATIVES',
    "/// Canon FileInfo RFLensType: all raw IDs and labels; no fractional alternatives.", $rf)
  . emit('PENTAX_LENS_ALTERNATIVES',
    "/// `%Image::ExifTool::Pentax::pentaxLensTypes`: the " . scalar(@$pentax) . " ids\n"
  . "/// that carry at least one `.N` alternative, keyed by validated unique base label.", $pentax);
my $header = <<'HEADER';
//! The ambiguous half of the manufacturer `LensType` lookups: for each id that
//! several lenses share, the alternatives ExifTool files under its fractional
//! keys.
//!
//! DO NOT EDIT BY HAND. Transcribed from the pinned ExifTool tree's own
//! in-memory Perl hashes (`.exiftool-version`, @VERSION@) by
//! `tools/exiftool-tables/dump_lens_alternatives.pl --full-file`.
//!
//! # Why this is a separate table, and how its keys preserve identity
//!
//! [`super::super::parsers::tiff::makernotes::lens_data`] carries the *integer*
//! keys of `%Image::ExifTool::Canon::canonLensTypes` and friends -- the @BASE@
//! entries a plain `Canon:LensType` lookup needs. Its @FRACTIONAL@ fractional keys
//! belong to `Composite:LensID`; this file carries that missing half.
//!
//! Both upstream `PrintLensID` routines retain the raw maker lens ID:
//!
//! ```text
//!     $lens =~ s/ or .*//s;    # remove everything after "or"
//!     my @lenses = ( $lens );
//!     for ($i=1; $$printConv{"$lensType.$i"}; ++$i) {
//!         push @lenses, $$printConv{"$lensType.$i"};
//!     }
//! ```
//!
//! Canon must therefore keep the raw ID, base label and alternatives together.
//! Distinct IDs can share a label while only one has alternatives. Every base
//! row is included so a caller lacking the raw ID can still determine whether
//! its label has one unambiguous candidate set; it must omit when it cannot.
//!
//! Canon RF has a separate PrintConv table. Its complete raw-ID/label rows
//! are retained independently; the producer refuses any fractional RF growth.
//!
//! Pentax retains its existing label keys only after the producer checks every
//! base label, including IDs without alternatives: each ambiguous label must
//! identify exactly one base ID. Fractional chains must be contiguous truthy
//! strings, matching the upstream loop. Unsupported shapes are refused.
HEADER
$header =~ s/\@VERSION\@/$version/g;
$header =~ s/\@BASE\@/$canon_base/g;
$header =~ s/\@FRACTIONAL\@/$canon_frac/g;
my $result = encode('UTF-8', ($full ? $header : '') . $body);
if ($full) {
    my ($fh, $path) = tempfile('oxidex-lens-XXXXXX', SUFFIX => '.rs', TMPDIR => 1, UNLINK => 1);
    binmode $fh; print {$fh} $result or die "cannot write formatter input: $!\n"; close $fh or die "cannot close formatter input: $!\n";
    system {$rustfmt} $rustfmt, '--edition', '2024', '--config-path', "$root/rustfmt.toml", $path;
    die "rustfmt failed; output unchanged\n" if $? != 0;
    open my $formatted, '<:raw', $path or die "cannot read formatted output: $!\n";
    $result = do { local $/; <$formatted> }; close $formatted;
}
if (defined $check) {
    die "check target is not a regular file\n" unless -f $check && !-l $check;
    open my $fh, '<:raw', $check or die "cannot read check target: $!\n";
    my $actual = do { local $/; <$fh> }; close $fh;
    die "lens alternatives differ: $check\n" unless $actual eq $result;
    print STDERR "lens alternatives are current: $check\n";
} elsif (defined $out) {
    die "output target must be a regular file or absent\n" if -l $out || (-e $out && !-f $out);
    my $mode = -e $out ? (stat($out))[2] & 0777 : 0644;
    my ($fh, $temp) = tempfile('.oxidex-lens-XXXXXX', DIR => dirname($out), UNLINK => 1);
    binmode $fh; print {$fh} $result or die "cannot write output: $!\n"; close $fh or die "cannot close output: $!\n";
    chmod $mode, $temp or die "cannot set output mode: $!\n";
    rename $temp, $out or die "cannot replace output: $!\n";
} else {
    binmode STDOUT; print $result or die "cannot write stdout: $!\n";
}
