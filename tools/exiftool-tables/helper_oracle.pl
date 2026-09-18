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
);

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
    my @r = eval { $fn->($et, @a) };
    if ($@) {
        push @results, { die => 1 };
    } else {
        push @results, { out => [ map { out($_) } @r ] };
    }
}
print $json->encode(\@results), "\n";
