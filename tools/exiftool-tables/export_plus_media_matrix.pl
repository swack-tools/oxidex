#!/usr/bin/env perl
# Export the PLUS XMP MediaSummaryCode PrintConv selected by GetTagTable.
use strict;
use warnings;
BEGIN {
    for my $name (qw(PERL5OPT PERL5LIB PERLLIB)) {
        die "refusing ambient $name\n" if defined $ENV{$name} && length $ENV{$name};
    }
}
use B ();
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use Encode qw(decode FB_CROAK);
use FindBin;
use Getopt::Long qw(GetOptions);
use JSON::PP ();

my $root = abs_path("$FindBin::Bin/../..");
my $tree;
GetOptions('exiftool-dir=s' => \$tree) or die "usage: $0 --exiftool-dir TREE\n";
die "usage: $0 --exiftool-dir TREE\n" unless defined $tree && !@ARGV;
$tree = abs_path($tree) or die "selected ExifTool tree does not exist\n";
my $lib = "$tree/lib";
open my $pin_fh, '<', "$root/.exiftool-version" or die "cannot read version pin: $!\n";
my $pin = do { local $/; <$pin_fh> };
close $pin_fh;
$pin =~ s/\s+\z//;
die "invalid version pin\n" unless $pin =~ /^\d+\.\d+\z/;

unshift @INC, $lib;
require Image::ExifTool;
my $table = Image::ExifTool::GetTagTable('Image::ExifTool::PLUS::XMP')
    or die "selected PLUS::XMP table unavailable\n";
die "selected ExifTool version disagrees with pin\n"
    unless $Image::ExifTool::VERSION eq $pin;
for my $loaded_module ('Image/ExifTool.pm', 'Image/ExifTool/XMP.pm',
                       'Image/ExifTool/PLUS.pm') {
    my $expected_module = abs_path("$lib/$loaded_module")
        or die "selected module missing: $loaded_module\n";
    my $actual_module = defined($INC{$loaded_module}) ? abs_path($INC{$loaded_module}) : undef;
    die "$loaded_module loaded outside selected tree\n"
        unless defined($actual_module) && $actual_module eq $expected_module;
}
my $module = 'Image/ExifTool/PLUS.pm';
my $expected = abs_path("$lib/$module") or die "selected PLUS.pm missing\n";
my %supported_plus_versions = (
    '11.78' => '1.00',
    '12.64' => '1.00',
    '13.59' => '1.02',
);
die "unsupported PLUS.pm version\n"
    unless defined($supported_plus_versions{$pin})
        && $Image::ExifTool::PLUS::VERSION eq $supported_plus_versions{$pin};
my $tag = $table->{MediaSummaryCode};
die "MediaSummaryCode not selected from PLUS::XMP\n"
    unless ref($tag) eq 'HASH' && $tag->{SeparateTable} eq 'MediaMatrix';
my $conv = $tag->{PrintConv};
die "MediaSummaryCode PrintConv is not a hash\n" unless ref($conv) eq 'HASH';
die "MediaSummaryCode OTHER is not a CODE reference\n"
    unless ref($conv->{OTHER}) eq 'CODE';
die "MediaSummaryCode Notes is not a string\n"
    unless defined($conv->{Notes}) && !ref($conv->{Notes});

my @rows;
for my $key (sort keys %$conv) {
    next if $key eq 'OTHER' || $key eq 'Notes';
    die "unsupported MediaSummaryCode key '$key'\n" unless $key =~ /^[0-9][A-Z]{3}\z/;
    my $value = $conv->{$key};
    die "unsupported MediaSummaryCode value for '$key'\n"
        unless defined($value) && !ref($value)
            && (B::svref_2object(\$value)->FLAGS & B::SVp_POK());
    if (!utf8::is_utf8($value)) {
        my $decoded = eval { decode('UTF-8', $value, FB_CROAK) };
        $value = defined($decoded) ? $decoded : decode('ISO-8859-1', $value);
    }
    push @rows, [$key, $value];
}
die "unexpected MediaSummaryCode row count: " . scalar(@rows) . "\n"
    unless @rows == 2143;
open my $source_fh, '<:raw', $expected or die "cannot read PLUS.pm: $!\n";
my $source = do { local $/; <$source_fh> };
close $source_fh;
for my $fragment ('OTHER => sub {', '$val = uc $val;',
                  '$val =~ /^\\|PLUS\\|(.*?)\\|(.*?)\\|(.*)/s',
                  '$code =~ tr/0-9A-Z|//dc;',
                  '$code =~ /(\\d[A-Z]{3})/g',
                  'defined $$conv{$mmid}') {
    die "PLUS.pm OTHER contract changed: $fragment\n"
        unless index($source, $fragment) >= 0;
}
binmode STDOUT, ':encoding(UTF-8)';
print JSON::PP->new->canonical->encode({
    schema => 'plus_media_matrix_v1', exiftool_version => $pin,
    module => $module, module_version => $Image::ExifTool::PLUS::VERSION,
    source_sha256 => sha256_hex($source), rows => \@rows,
    exceptions => { OTHER => 'CODE', Notes => 'STRING' },
});
