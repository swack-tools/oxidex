#!/usr/bin/env perl
# Differential oracle for the generated conversion arms
# (src/exiftool_tables/conv/<module>.rs). Driven by conv_oracle.py -- run it
# through that script, which pins the interpreter, the ExifTool tree, TZ and
# the probe set; this file only asks the pinned ExifTool itself.
#
#   conv_oracle.pl <exiftool lib dir> <Module::Table> < cases.json > outputs.json
#
# Input: a JSON array of cases
#   { "id": <tag id>, "val": ARG, "byte_order": "II"|"MM",
#     "members": { "<$$self key>": ARG, ... } }
# where ARG is {"t":"undef"} | {"t":"s","hex":"<bytes>"} |
# {"t":"i","v":"<integer>"} | {"t":"f","v":"<decimal>"}.
#
# For each case a fresh ExifTool object takes the members, the byte order is
# set, and the tag's own tagInfo (GetTagInfo on the table) goes through the
# real pipeline: FoundTag($tagInfo, $val) -- RawConv, data members -- then
# GetValue($key, 'ValueConv') and GetValue($key, 'PrintConv'), each in scalar
# context, exactly as the CLI asks for the -n and default values.
#
# Output: one entry per case:
#   {"suppressed":1}                      FoundTag returned undef (RawConv undef)
#   {"vc":OUT,"pc":OUT,"set":{k:OUT,...}} otherwise
# OUT is {"t":"undef"} | {"t":"s","hex":..} (a scalar, stringified by Perl) |
# {"t":"ref","hex":..} (a SCALAR ref: its referent) | {"t":"other","ref":..}.
# "set" lists every top-level non-reference member of $self that FoundTag
# changed (a RawConv's `$$self{X} = ...`), except FoundTag's own NUM_FOUND.
use strict;
use warnings;
use JSON::PP;

my $lib = shift or die "usage: $0 <exiftool lib dir> <Module::Table>\n";
my $tbl = shift or die "usage: $0 <exiftool lib dir> <Module::Table>\n";
unshift @INC, $lib;
require Image::ExifTool;
my ($module) = split /::/, $tbl;
eval "require Image::ExifTool::$module; 1" or die $@;

$SIG{__WARN__} = sub { };    # numeric/uninitialized warnings are not values

sub arg {
    my $a = shift;
    my $t = $$a{t};
    return undef if $t eq 'undef';
    return pack('H*', $$a{hex}) if $t eq 's';
    return 0 + $$a{v} if $t eq 'i';
    return unpack('d', pack('d', $$a{v})) if $t eq 'f';
    die "bad arg type $t\n";
}

sub out {
    my $v = shift;
    return { t => 'undef' } unless defined $v;
    if (ref $v eq 'SCALAR') {
        my $s = $$v;
        utf8::encode($s) if utf8::is_utf8($s);
        return { t => 'ref', hex => unpack('H*', $s) };
    }
    return { t => 'other', ref => ref $v } if ref $v;
    my $s = "$v";
    utf8::encode($s) if utf8::is_utf8($s);
    return { t => 's', hex => unpack('H*', $s) };
}

# FoundTag's own bookkeeping, not a conversion's effect.
my %BOOKKEEPING = (NUM_FOUND => 1);

sub scalars {
    my $et = shift;
    my %h;
    foreach (keys %$et) {
        next if $BOOKKEEPING{$_};
        my $v = $$et{$_};
        next if ref $v;
        $h{$_} = defined $v ? "d:$v" : 'u';
    }
    return \%h;
}

my $table = Image::ExifTool::GetTagTable("Image::ExifTool::$tbl");
my $json = JSON::PP->new->canonical;
local $/;
my $cases = $json->decode(<STDIN>);
my @results;
for my $case (@$cases) {
    my $et = Image::ExifTool->new;
    my $m = $$case{members} || {};
    $$et{$_} = arg($$m{$_}) foreach sort keys %$m;
    Image::ExifTool::SetByteOrder($$case{byte_order} || 'II');
    my $val = arg($$case{val});
    my $tagInfo = $et->GetTagInfo($table, $$case{id});
    die "no tagInfo for $$case{id}\n" unless ref $tagInfo eq 'HASH';
    my $before = scalars($et);
    my $key = $et->FoundTag($tagInfo, $val);
    unless (defined $key) {
        push @results, { suppressed => 1 };
        next;
    }
    my $after = scalars($et);
    my %set;
    foreach my $k (sort keys %$after) {
        next if defined $$before{$k} and $$before{$k} eq $$after{$k};
        $set{$k} = out($$et{$k});
    }
    my $vc = $et->GetValue($key, 'ValueConv');
    my $pc = $et->GetValue($key, 'PrintConv');
    push @results, { vc => out($vc), pc => out($pc), set => \%set };
}
print $json->encode(\@results), "\n";
