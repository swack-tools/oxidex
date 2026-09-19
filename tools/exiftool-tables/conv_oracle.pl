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
#     "members": { "<$$self key>": ARG, ... },
#     "options": { "<OPTIONS key>": ARG, ... } }        (options optional)
# where ARG is {"t":"undef"} | {"t":"s","hex":"<bytes>"} |
# {"t":"i","v":"<integer>"} | {"t":"f","v":"<decimal>"}.
#
# Each case runs the tag's own tagInfo (GetTagInfo on the table) through the
# real pipeline TWICE, each time on a fresh ExifTool object with the members
# and options set and the byte order set:
#   A. FoundTag($tagInfo, $val) -- RawConv, data members -- then
#      GetValue($key, 'ValueConv'): the -n value;
#   B. FoundTag($tagInfo, $val) then GetValue($key, 'PrintConv'): the default
#      value. B runs every conversion stage exactly once (RawConv, ValueConv,
#      PrintConv), as the CLI does for one tag, so B's side effects are the
#      ones recorded -- a second GetValue on one object would re-run
#      ValueConv and repeat its Warn requests.
#
# Output: one entry per case:
#   {"suppressed":1,"set":..,"warnings":..} FoundTag returned undef (RawConv
#                                         undef); set/warnings from that call
#   {"vc":OUT,"pc":OUT,"set":{k:OUT,...},"warnings":[W...]} otherwise
# OUT is {"t":"undef"} | {"t":"s","hex":..} (a scalar, stringified by Perl) |
# {"t":"ref","hex":..} (a SCALAR ref: its referent) | {"t":"other","ref":..}
# | {"t":"deleted"} (in "set" only); a scalar Perl held as a CHARACTER
# string (UTF-8 flag on) also carries "u8":1 -- the Rust runtime models
# byte strings only, so the replay counts that as a disagreement.
# "set" lists every top-level non-reference member of $self that run B
# created, changed or deleted (a RawConv's `$$self{X} = ...`, a helper's
# DecodeWarn/WrongByteOrder/...), except FoundTag's own NUM_FOUND.
# "warnings" lists each $self->Warn request made during run B, in order, as
# OUT with "ignorable" set to Warn's second argument when one was passed.
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
    my ($t, $s);
    if (ref $v eq 'SCALAR') {
        ($t, $s) = ('ref', $$v);
    } elsif (ref $v) {
        return { t => 'other', ref => ref $v };
    } else {
        ($t, $s) = ('s', "$v");
    }
    my %o = (t => $t);
    if (utf8::is_utf8($s)) {
        utf8::encode($s);
        $o{u8} = 1;
    }
    $o{hex} = unpack('H*', $s);
    return \%o;
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

# A fresh object with the case's members, options and byte order.
sub fresh {
    my $case = shift;
    my $et = Image::ExifTool->new;
    my $m = $$case{members} || {};
    $$et{$_} = arg($$m{$_}) foreach sort keys %$m;
    my $o = $$case{options} || {};
    $$et{OPTIONS}{$_} = arg($$o{$_}) foreach sort keys %$o;
    Image::ExifTool::SetByteOrder($$case{byte_order} || 'II');
    return $et;
}

my $table = Image::ExifTool::GetTagTable("Image::ExifTool::$tbl");
my $json = JSON::PP->new->canonical;
local $/;
my $cases = $json->decode(<STDIN>);
my @results;
# The members of $et changed since $before, and the recorded Warn requests.
sub effects {
    my ($et, $before, $warned) = @_;
    my $after = scalars($et);
    my %set;
    foreach my $k (sort keys %$after) {
        next if defined $$before{$k} and $$before{$k} eq $$after{$k};
        $set{$k} = out($$et{$k});
    }
    foreach my $k (sort keys %$before) {
        $set{$k} = { t => 'deleted' } unless exists $$et{$k};
    }
    my @w;
    foreach (@$warned) {
        my $o = out($$_[0]);
        $$o{ignorable} = "$$_[1]" if defined $$_[1];
        push @w, $o;
    }
    return (set => \%set, warnings => \@w);
}

for my $case (@$cases) {
    # run A: the -n value
    my $et = fresh($case);
    my $tagInfo = $et->GetTagInfo($table, $$case{id});
    die "no tagInfo for $$case{id}\n" unless ref $tagInfo eq 'HASH';
    my $key = $et->FoundTag($tagInfo, arg($$case{val}));
    my $vc = defined $key ? $et->GetValue($key, 'ValueConv') : undef;
    # run B: the default value, every stage once, side effects recorded
    $et = fresh($case);
    $tagInfo = $et->GetTagInfo($table, $$case{id});
    my $before = scalars($et);
    my @warned;
    my ($pc, $keyB);
    {
        no warnings qw(redefine once);
        local *Image::ExifTool::Warn = sub { push @warned, [ $_[1], $_[2] ]; return 1 };
        $keyB = $et->FoundTag($tagInfo, arg($$case{val}));
        die "FoundTag differs between runs A and B\n" if defined $keyB != defined $key;
        $pc = $et->GetValue($keyB, 'PrintConv') if defined $keyB;
    }
    my %fx = effects($et, $before, \@warned);
    if (defined $key) {
        push @results, { vc => out($vc), pc => out($pc), %fx };
    } else {
        push @results, { suppressed => 1, %fx };
    }
}
print $json->encode(\@results), "\n";
