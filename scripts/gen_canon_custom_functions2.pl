#!/usr/bin/env perl
#
# Emit Rust source for `%Image::ExifTool::CanonCustom::Functions2` straight from
# ExifTool's own in-memory Perl hash.
#
# The table is a list of 100 tag ids, 26 of which ExifTool models as an ARRAY of
# `Condition` arms rather than a single hash. The arms are ordered and ExifTool
# takes the first whose `Condition` passes -- an arm with no `Condition` always
# passes (ExifTool.pm `GetTagInfo`: `if ($condition) { ... }`), so it terminates
# the list. Conditions here are drawn from a closed language:
#
#     $$self{Model} =~ /\bALT\b/            # word-bounded alternation
#     $$self{Model} !~ /\bALT\b/
#     <term> or <term>                      # two of the above
#     $count == N                           # element count of the record entry
#
# and this script HARD-ERRORS on any Condition, ValueConv or PrintConv construct
# it has not seen before rather than silently skipping the entry. A silent skip
# is what produced the "31 model-conditional entries" claim that this table
# replaces: the generator logged "not-a-hash" and three commit messages
# downstream reported it as a deliberate decision about model-specific labels.
#
# Usage: perl scripts/gen_canon_custom_functions2.pl \
#            > src/parsers/tiff/makernotes/canon/custom_functions2_tables.rs

use strict;
use warnings;
use FindBin;
use lib "$FindBin::Bin/lib";
use ExiftoolPin;

# Resolves the pinned ExifTool tree via ExiftoolPin (never a PATH or Homebrew
# fallback -- see scripts/lib/ExiftoolPin.pm).
my $EXIFTOOL_LIB = ExiftoolPin::resolve();
eval "use lib '$EXIFTOOL_LIB'; use Image::ExifTool; use Image::ExifTool::CanonCustom; 1"
    or die $@;

my $TABLE = \%Image::ExifTool::CanonCustom::Functions2;

sub bail { die "gen_canon_custom_functions2: $_[0]\n" }

sub esc {
    my $s = shift;
    $s =~ s/\\/\\\\/g;
    $s =~ s/"/\\"/g;
    return $s;
}

# ---------------------------------------------------------------------------
# Condition parsing
# ---------------------------------------------------------------------------

# A single `\b...\b` alternative must be plain text so the Rust matcher can use
# the simple word-boundary rule (a boundary exists where exactly one side is a
# word character). ExifTool's alternatives all start and end with a word
# character; anything else is a construct this script has not seen.
sub check_alt {
    my ($alt, $ctx) = @_;
    bail "$ctx: empty alternative" if $alt eq '';
    bail "$ctx: alternative '$alt' has regex metacharacters"
        unless $alt =~ /^[A-Za-z0-9 -]+$/;
    bail "$ctx: alternative '$alt' does not start with a word character"
        unless $alt =~ /^\w/;
    bail "$ctx: alternative '$alt' does not end with a word character"
        unless $alt =~ /\w$/;
    return $alt;
}

# Expand a single trailing-optional character (`1Ds?` -> `1D`, `1Ds`). The
# optional character sits immediately before a literal, so expanding it into two
# alternatives is exactly equivalent under `\b...\b`.
sub expand_optional {
    my ($body, $ctx) = @_;
    my $n = () = $body =~ /\?/g;
    return ($body) if $n == 0;
    bail "$ctx: more than one '?' in '$body'" if $n > 1;
    bail "$ctx: '?' does not follow a single literal character in '$body'"
        unless $body =~ /^(.*?)(\w)\?(.*)$/;
    my ($pre, $opt, $post) = ($1, $2, $3);
    return ("$pre$post", "$pre$opt$post");
}

# Parse `/\b(a|b)\b/`-shaped bodies into alternatives, or `/\b1D/` into a
# left-bounded prefix.
sub parse_regex_body {
    my ($body, $ctx) = @_;
    bail "$ctx: regex '$body' does not start with \\b" unless $body =~ s/^\\b//;
    my $right_bounded = ($body =~ s/\\b$//) ? 1 : 0;
    my @raw;
    if ($body =~ /^\((.*)\)$/) {
        @raw = split /\|/, $1;
        bail "$ctx: nested parentheses in '$body'" if grep { /[()]/ } @raw;
    } else {
        bail "$ctx: unparenthesised alternation in '$body'" if $body =~ /[()|]/;
        @raw = ($body);
    }
    my @alts;
    for my $r (@raw) {
        push @alts, map { check_alt($_, $ctx) } expand_optional($r, $ctx);
    }
    return ($right_bounded, \@alts);
}

# Returns a Rust `Cf2Cond` expression.
sub parse_condition {
    my ($cond, $ctx) = @_;
    my $c = $cond;
    $c =~ s/\s+/ /g;
    $c =~ s/^ | $//g;

    if ($c =~ /^\$count == (\d+)$/) {
        return "Cf2Cond::Count($1)";
    }

    my @terms = split / or /, $c;
    bail "$ctx: condition '$c' has more than two terms" if @terms > 2;

    my (@alts, $negated, $prefix);
    for my $term (@terms) {
        bail "$ctx: term '$term' is not a \$\$self{Model} match"
            unless $term =~ m{^\$\$self\{Model\} (=~|!~) /(.+)/$};
        my ($op, $body) = ($1, $2);
        if ($op eq '!~') {
            bail "$ctx: negated term combined with 'or'" if @terms > 1;
            $negated = 1;
        }
        my ($right_bounded, $list) = parse_regex_body($body, $ctx);
        if (not $right_bounded) {
            bail "$ctx: unbounded regex combined with 'or'" if @terms > 1;
            bail "$ctx: unbounded regex with alternation" if @$list > 1;
            bail "$ctx: unbounded regex negated" if $negated;
            $prefix = $list->[0];
            next;
        }
        push @alts, @$list;
    }

    return sprintf('Cf2Cond::ModelPrefix("%s")', esc($prefix)) if defined $prefix;
    my $set = join(', ', map { '"' . esc($_) . '"' } @alts);
    # `A or B` over two `=~` alternations is the union of their alternatives.
    return $negated ? "Cf2Cond::ModelNot(&[$set])" : "Cf2Cond::Model(&[$set])";
}

# ---------------------------------------------------------------------------
# ValueConv
# ---------------------------------------------------------------------------

# The five ValueConv expressions ExifTool uses in this table, matched verbatim.
my %VALUE_CONV = (
    '$val < 2 ? $val : ($val < 1000 ? exp(($val/8-9)*log(2))*100 : 0)' => 'Iso',
    'exp(-($val/8-7)*log(2))'                                         => 'ShutterSpeedStops',
    'exp(-$val/(1600*log(2)))'                                        => 'ShutterSpeedLinear',
    'exp(($val/8-1)*log(2)/2)'                                        => 'ApertureStops',
    'exp($val/2400)'                                                  => 'ApertureLinear',
);

sub value_conv_slot {
    my ($expr, $ctx) = @_;
    return 'Cf2ValueConv::None' unless defined $expr;
    bail "$ctx: ValueConv slot is a " . ref($expr) . " ref" if ref $expr;
    my $name = $VALUE_CONV{$expr}
        or bail "$ctx: unknown ValueConv expression [$expr]";
    return "Cf2ValueConv::$name";
}

# ---------------------------------------------------------------------------
# PrintConv
# ---------------------------------------------------------------------------

# The scalar PrintConv expressions, each mapped to a Rust converter. Every one
# is `<literal> <numeric rendering of $val> <literal>`, so they collapse onto
# four generic forms plus the Flags special case.
my %PRINT_CONV = (
    'sprintf("Flags 0x%x",$val)' => 'Cf2PrintConv::Flags',

    'sprintf("Max %.0f",$val)'   => 'Cf2PrintConv::Round0("Max ")',
    'sprintf("Min %.0f",$val)'   => 'Cf2PrintConv::Round0("Min ")',

    '"Hi " . Image::ExifTool::Exif::PrintExposureTime($val)'
        => 'Cf2PrintConv::ExposureTime("Hi ")',
    '"Lo " . Image::ExifTool::Exif::PrintExposureTime($val)'
        => 'Cf2PrintConv::ExposureTime("Lo ")',
    '"Manual: Hi " . Image::ExifTool::Exif::PrintExposureTime($val)'
        => 'Cf2PrintConv::ExposureTime("Manual: Hi ")',
    '"Auto: Hi " . Image::ExifTool::Exif::PrintExposureTime($val)'
        => 'Cf2PrintConv::ExposureTime("Auto: Hi ")',

    'sprintf("Closed %.2g",$val)'         => 'Cf2PrintConv::TwoSigFigs("Closed ")',
    'sprintf("Open %.2g",$val)'           => 'Cf2PrintConv::TwoSigFigs("Open ")',
    'sprintf("Manual: Closed %.2g",$val)' => 'Cf2PrintConv::TwoSigFigs("Manual: Closed ")',
    'sprintf("Auto: Closed %.2g",$val)'   => 'Cf2PrintConv::TwoSigFigs("Auto: Closed ")',

    '"Hi $val"'              => 'Cf2PrintConv::Interpolate("Hi ", "")',
    '"Cont $val"'            => 'Cf2PrintConv::Interpolate("Cont ", "")',
    '"Lo $val"'              => 'Cf2PrintConv::Interpolate("Lo ", "")',
    '"Soft $val"'            => 'Cf2PrintConv::Interpolate("Soft ", "")',
    '"Soft LS $val"'         => 'Cf2PrintConv::Interpolate("Soft LS ", "")',
    '"6 s: $val"'            => 'Cf2PrintConv::Interpolate("6 s: ", "")',
    '"16 s: $val"'           => 'Cf2PrintConv::Interpolate("16 s: ", "")',
    '"After release: $val"'  => 'Cf2PrintConv::Interpolate("After release: ", "")',
    '"$val shots"'           => 'Cf2PrintConv::Interpolate("", " shots")',
);

sub print_conv_hash {
    my ($pc, $ctx) = @_;
    # ExifTool looks the *string* form of the value up in this hash, and for a
    # multi-element record that string is the space-joined list -- which is why
    # `AEBShotCount`'s two-value arm is keyed '3 0', '2 1', '5 2' and '7 3'
    # (CanonCustom.pm:1345). Keys stay strings here so both cases are one lookup.
    my @unknown = grep { !/^-?[\d ]+$/ && $_ ne 'BITMASK' } keys %$pc;
    bail "$ctx: PrintConv hash carries [" . join(',', sort @unknown) . "]" if @unknown;

    if (exists $pc->{BITMASK}) {
        bail "$ctx: BITMASK hash also carries plain keys"
            if grep { $_ ne 'BITMASK' } keys %$pc;
        my $bm = $pc->{BITMASK};
        bail "$ctx: BITMASK is not a hash" unless ref $bm eq 'HASH';
        my @bad = grep { !/^\d+$/ } keys %$bm;
        bail "$ctx: BITMASK carries non-numeric key [" . join(',', @bad) . "]" if @bad;
        my $body = join('', map { sprintf("(%d, \"%s\"), ", $_, esc($bm->{$_})) }
                             sort { $a <=> $b } keys %$bm);
        $body =~ s/, $//;
        return "Cf2PrintConv::Bitmask(&[$body])";
    }

    my @keys = sort { ($a =~ /^-?\d+$/ && $b =~ /^-?\d+$/) ? $a <=> $b : $a cmp $b }
                    keys %$pc;
    my $body = join('', map { sprintf("(\"%s\", \"%s\"), ", esc($_), esc($pc->{$_})) } @keys);
    $body =~ s/, $//;
    return "Cf2PrintConv::Map(&[$body])";
}

sub print_conv_slot {
    my ($pc, $ctx) = @_;
    return 'Cf2PrintConv::None' unless defined $pc;
    my $r = ref $pc;
    return print_conv_hash($pc, $ctx) if $r eq 'HASH';
    bail "$ctx: PrintConv slot is a $r ref" if $r;
    my $rust = $PRINT_CONV{$pc}
        or bail "$ctx: unknown PrintConv expression [$pc]";
    return $rust;
}

# ---------------------------------------------------------------------------
# Emit
# ---------------------------------------------------------------------------

my @ARM_KEYS = qw(Name Condition Notes Count Description
                  PrintConv PrintConvInv ValueConv ValueConvInv);
my %ARM_KEY = map { $_ => 1 } @ARM_KEYS;

my @ids = sort { $a <=> $b } grep { /^\d+$/ } keys %$TABLE;
my ($n_arms, $n_arrays, $n_cond) = (0, 0, 0);

print <<'HEADER';
// GENERATED by scripts/gen_canon_custom_functions2.pl from ExifTool's
// Image::ExifTool::CanonCustom::Functions2 table. Do not edit by hand.
//
// The generator hard-errors on any Condition, ValueConv or PrintConv construct
// it has not seen, so an entry here either mirrors ExifTool exactly or the
// generator refused to run.
use super::custom_functions2::{
    Cf2Arm, Cf2Cond, Cf2Entry, Cf2Print, Cf2PrintConv, Cf2ValueConv,
};

HEADER

printf("/// All %d `%%CanonCustom::Functions2` tag ids, in ExifTool's key order.\n", scalar @ids);
print "///\n";
print "/// Each entry holds its `Condition` arms in ExifTool's own order; the first arm\n";
print "/// whose condition passes wins, and an arm with no condition always passes.\n";
print "pub(super) static CUSTOM_FUNCTIONS2: &[Cf2Entry] = &[\n";

for my $id (@ids) {
    my $entry = $TABLE->{$id};
    my @arms = ref $entry eq 'ARRAY' ? @$entry : ($entry);
    bail(sprintf("0x%04x: entry is a %s", $id, ref($entry) || 'scalar'))
        unless ref $entry eq 'ARRAY' or ref $entry eq 'HASH';
    $n_arrays++ if ref $entry eq 'ARRAY';

    printf("    Cf2Entry {\n        tag: 0x%04x,\n        arms: &[\n", $id);
    for my $i (0 .. $#arms) {
        my $arm = $arms[$i];
        my $ctx = sprintf("0x%04x arm %d", $id, $i);
        bail "$ctx: arm is not a hash" unless ref $arm eq 'HASH';
        my @unknown = grep { !$ARM_KEY{$_} } keys %$arm;
        bail "$ctx: unknown key [" . join(',', sort @unknown) . "]" if @unknown;
        my $name = $arm->{Name} or bail "$ctx: no Name";
        $n_arms++;

        my $cond = 'Cf2Cond::Always';
        if (defined $arm->{Condition}) {
            $cond = parse_condition($arm->{Condition}, $ctx);
            $n_cond++;
        }

        # ValueConv: absent, or a Perl ARRAY of per-slot expressions.
        my $vc = '&[]';
        if (defined $arm->{ValueConv}) {
            bail "$ctx: ValueConv is not an ARRAY" unless ref $arm->{ValueConv} eq 'ARRAY';
            $vc = '&[' . join(', ', map { value_conv_slot($_, $ctx) } @{$arm->{ValueConv}}) . ']';
        }

        # PrintConv: absent, a single conversion applied to the whole (possibly
        # space-joined) value, or a Perl ARRAY paired slot-for-slot with it.
        my $pc;
        my $pcv = $arm->{PrintConv};
        if (not defined $pcv) {
            $pc = 'None';
        } elsif (ref $pcv eq 'ARRAY') {
            my @slots = map { print_conv_slot($pcv->[$_], "$ctx slot $_") } 0 .. $#$pcv;
            # A lookup hash is keyed on the raw stored number, so a slot that is
            # both ValueConv'd and hash-looked-up would be a construct this
            # script has not seen (and could not key correctly).
            for my $s (0 .. $#slots) {
                next unless $slots[$s] =~ /^Cf2PrintConv::(Map|Bitmask)\(/;
                my $v = ref $arm->{ValueConv} eq 'ARRAY' ? $arm->{ValueConv}[$s] : undef;
                bail "$ctx slot $s: lookup hash on a ValueConv'd slot" if defined $v;
            }
            $pc = 'List(&[' . join(', ', @slots) . '])';
        } else {
            bail "$ctx: scalar PrintConv on a ValueConv'd entry" if defined $arm->{ValueConv};
            $pc = 'Scalar(' . print_conv_slot($pcv, $ctx) . ')';
        }

        printf("            Cf2Arm {\n");
        printf("                name: \"%s\",\n", esc($name));
        printf("                cond: %s,\n", $cond);
        printf("                value_convs: %s,\n", $vc);
        printf("                print_conv: Cf2Print::%s,\n", $pc);
        printf("            },\n");
    }
    print "        ],\n    },\n";
}
print "];\n";

printf STDERR "ids %d, arms %d (%d arrays of arms), conditions %d\n",
    scalar @ids, $n_arms, $n_arrays, $n_cond;
