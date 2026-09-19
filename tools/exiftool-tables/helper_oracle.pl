#!/usr/bin/env perl
# Differential oracle for the Autogeneration v2 helper library
# (src/exiftool_tables/helpers.rs). Driven by helper_oracle.py -- run it
# through that script, which pins the interpreter, the ExifTool tree, TZ and
# the probe set; this file only calls the pinned tree's own subs.
#
#   helper_oracle.pl <exiftool lib dir>  < cases.json  > outputs.json
#
# Input: a JSON array of cases
#   { "helper": "<Perl sub, fully qualified>", "args": [ARG...],
#     "options": { "<OPTIONS key>": ARG, ... }, "with_session": 0|1 }
# where ARG is {"t":"undef"} | {"t":"s","hex":"<bytes>"} |
# {"t":"i","v":"<integer>"} | {"t":"f","v":"<decimal>"}.
#
# Output: a JSON array, one entry per case, of either {"die": 1} or
# {"out": [OUT...]} where OUT is {"t":"undef"} or {"hex":"<bytes>"} -- the
# helper's return value (and, for IsFloat, its possibly-rewritten argument)
# STRINGIFIED BY PERL. The Rust side must reproduce these bytes exactly.
#
# Truthiness cases ({"truthy": ARG}) answer {"out":[{"hex":"31"|""}]} from
# Perl's own boolean context, so Session::is_truthy is pinned to perl, not
# to anyone's memory of perlsyn.
#
# An option-defaults case ({"option_defaults": [NAME...]}) answers
# {"out": [OUT...]}: each option's value in a fresh Image::ExifTool->new.
#
# Decode/Encode cases may also carry "byte_order" ("II"/"MM": SetByteOrder
# before the call; absent: the module default 'MM', which the Rust side
# never reads -- it refuses whatever would consult GetByteOrder) and
# "members" ({KEY: ARG}, set in $$et before the call). Their result adds
# "set_members" ({KEY: OUT} for every non-reference member the call created
# or changed; {"t":"deleted"} for one it deleted) and "warnings" ([OUT...],
# each message the call passed to $self->Warn, recorded instead of issued --
# every caller ignores Warn's return -- with "ignorable" set to Warn's second
# argument when one was passed). ConvertExifText and DecodeCFAPattern are
# side-effect cases too. A pure case that carries "byte_order" runs after
# SetByteOrder (PrintSFR reads GetByteOrder through Get16u/GetRational64u).
# They run with $^W = 1, as the exiftool script sets it: Charset.pm has no
# `use warnings`, so its unpack/pack see the global flag.
use strict;
use warnings;
use JSON::PP;

my $lib = shift or die "usage: $0 <exiftool lib dir>\n";
unshift @INC, $lib;
require Image::ExifTool;
require Image::ExifTool::Exif;
require Image::ExifTool::GPS;
require Image::ExifTool::Canon;
require Image::ExifTool::XMP;
require Image::ExifTool::ASF;

$SIG{__WARN__} = sub { };    # numeric warnings are not part of the value

sub arg {
    my $a = shift;
    my $t = $$a{t};
    return undef if $t eq 'undef';
    return pack('H*', $$a{hex}) if $t eq 's';
    return 0 + $$a{v} if $t eq 'i';
    # A genuine NV: `0 + "1e15"` would come back an IV (pp_add preserves
    # integers), and an IV prints its digits where an NV prints %.15g.
    return unpack('d', pack('d', $$a{v})) if $t eq 'f';
    die "bad arg type $t\n";
}

sub out {
    my $v = shift;
    return { t => 'undef' } unless defined $v;
    my $s = "$v";
    utf8::encode($s) if utf8::is_utf8($s);
    return { hex => unpack('H*', $s) };
}

# Every call bypasses prototypes (&sub(...)) so the argument list is passed
# exactly as given -- including trailing undefs -- and @_ aliases @a, which
# is how IsFloat's in-place `tr/,/./` becomes visible.
my %CALL = (
    'Image::ExifTool::ConvertDateTime' => sub { my ($et, @a) = @_; (&Image::ExifTool::ConvertDateTime($et, @a)) },
    'Image::ExifTool::GPS::ToDMS'      => sub { my ($et, @a) = @_; (&Image::ExifTool::GPS::ToDMS($et, @a)) },
    'Image::ExifTool::ConvertFileSize' => sub { my ($et, @a) = @_; $main::WITH_SESSION ? (&Image::ExifTool::ConvertFileSize(@a, $et)) : (&Image::ExifTool::ConvertFileSize(@a)) },
    'Image::ExifTool::ConvertUnixTime' => sub { my ($et, @a) = @_; (&Image::ExifTool::ConvertUnixTime(@a)) },
    'Image::ExifTool::IsFloat'         => sub { my ($et, @a) = @_; my $r = &Image::ExifTool::IsFloat(@a); ($r, $a[0]) },
    'Image::ExifTool::IsInt'           => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::IsInt(@a)) },
    'Image::ExifTool::GetUnixTime'     => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::GetUnixTime(@a)) },
    'Image::ExifTool::ConvertDuration' => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::ConvertDuration(@a)) },
    'Image::ExifTool::ConvertBitrate'  => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::ConvertBitrate(@a)) },
    'Image::ExifTool::Exif::ConvertFraction'   => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::Exif::ConvertFraction(@a)) },
    'Image::ExifTool::Exif::PrintExposureTime' => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::Exif::PrintExposureTime(@a)) },
    'Image::ExifTool::Exif::PrintFraction'     => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::Exif::PrintFraction(@a)) },
    'Image::ExifTool::Exif::PrintFNumber'      => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::Exif::PrintFNumber(@a)) },
    'Image::ExifTool::GPS::ToDegrees'          => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::GPS::ToDegrees(@a)) },
    'Image::ExifTool::Canon::CanonEv'          => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::Canon::CanonEv(@a)) },
    'Image::ExifTool::Canon::CanonEvInv'       => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::Canon::CanonEvInv(@a)) },
    'Image::ExifTool::XMP::ConvertXMPDate'     => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::XMP::ConvertXMPDate(@a)) },
    'Image::ExifTool::Decode' => sub { my ($et, @a) = @_; local $^W = 1; (scalar &Image::ExifTool::Decode($et, @a)) },
    'Image::ExifTool::Encode' => sub { my ($et, @a) = @_; local $^W = 1; (scalar &Image::ExifTool::Encode($et, @a)) },
    'Image::ExifTool::Exif::ConvertExifText'  => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::Exif::ConvertExifText($et, @a)) },
    'Image::ExifTool::Exif::DecodeCFAPattern' => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::Exif::DecodeCFAPattern($et, @a)) },
    'Image::ExifTool::Exif::PrintCFAPattern'  => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::Exif::PrintCFAPattern(@a)) },
    'Image::ExifTool::Exif::PrintSFR'         => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::Exif::PrintSFR(@a)) },
    'Image::ExifTool::ASF::GetGUID'           => sub { my ($et, @a) = @_; (scalar &Image::ExifTool::ASF::GetGUID(@a)) },
    'Image::ExifTool::Printable'              => sub { my ($et, @a) = @_; (scalar $et->Printable(@a)) },
);
my %SIDE_EFFECTS = map { $_ => 1 } qw(Image::ExifTool::Decode Image::ExifTool::Encode
    Image::ExifTool::Exif::ConvertExifText Image::ExifTool::Exif::DecodeCFAPattern);

my $json = JSON::PP->new->canonical;
local $/;
my $cases = $json->decode(<STDIN>);
my @results;
for my $case (@$cases) {
    if (exists $$case{truthy}) {
        my $v = arg($$case{truthy});
        push @results, { out => [ out($v ? '1' : '') ] };
        next;
    }
    if (exists $$case{option_defaults}) {
        my $et = Image::ExifTool->new;
        push @results, { out => [ map { out($$et{OPTIONS}{$_}) } @{$$case{option_defaults}} ] };
        next;
    }
    my $fn = $CALL{$$case{helper}} or die "no call for $$case{helper}\n";
    my $et = Image::ExifTool->new;
    my $opts = $$case{options} || {};
    $$et{OPTIONS}{$_} = arg($$opts{$_}) foreach sort keys %$opts;
    # ExifTool::Init copies these two options into %static_vars before any
    # tag is read; ConvertUnixTime reads them from there.
    %Image::ExifTool::static_vars = (
        KeepUTCTime   => $$et{OPTIONS}{KeepUTCTime},
        SystemTimeRes => $$et{OPTIONS}{SystemTimeRes},
    );
    my @a = map { arg($_) } @{$$case{args}};
    # ConvertFileSize reads OPTIONS only through an optional trailing $et.
    $main::WITH_SESSION = $$case{with_session};
    unless ($SIDE_EFFECTS{$$case{helper}}) {
        Image::ExifTool::SetByteOrder($$case{byte_order}) if $$case{byte_order};
        my @r = eval { $fn->($et, @a) };
        if ($@) {
            push @results, { die => 1 };
        } else {
            push @results, { out => [ map { out($_) } @r ] };
        }
        next;
    }
    Image::ExifTool::SetByteOrder($$case{byte_order} || 'MM');
    my $mem = $$case{members} || {};
    $$et{$_} = arg($$mem{$_}) foreach sort keys %$mem;
    my %before = map { $_ => $$et{$_} } keys %$et;
    my @warned;
    my @r = eval {
        no warnings 'redefine';
        local *Image::ExifTool::Warn = sub { push @warned, [ $_[1], $_[2] ]; return 1 };
        $fn->($et, @a);
    };
    if ($@) {
        push @results, { die => 1 };
        next;
    }
    my %set;
    foreach my $k (sort keys %$et) {
        my $v = $$et{$k};
        next if exists $before{$k} and ((defined $v ? "$v" : "\0undef") eq
            (defined $before{$k} ? "$before{$k}" : "\0undef"));
        die "$$case{helper}: reference-valued member $k changed\n" if ref $v;
        $set{$k} = out($v);
    }
    foreach my $k (sort keys %before) {
        $set{$k} = { t => 'deleted' } unless exists $$et{$k};
    }
    my @w;
    foreach (@warned) {
        my $o = out($$_[0]);
        $$o{ignorable} = "$$_[1]" if defined $$_[1];
        push @w, $o;
    }
    push @results, { out => [ map { out($_) } @r ], set_members => \%set, warnings => \@w };
}
print $json->encode(\@results), "\n";
