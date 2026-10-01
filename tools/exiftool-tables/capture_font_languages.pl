#!/usr/bin/env perl
# Capture Font.pm's runtime %ttLang lookup, not its tag declarations.
# Usage: capture_font_languages.pl <exiftool-lib>
use strict;
use warnings;
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
use Digest::SHA qw(sha256_hex);
use JSON::PP ();

my $lib = shift @ARGV;
die "usage: capture_font_languages.pl <exiftool-lib>\n" unless defined $lib && !@ARGV && -d $lib;
unshift @INC, $lib;
require Image::ExifTool;
require Image::ExifTool::Font;
no warnings 'once';

my $source = "$lib/Image/ExifTool/Font.pm";
open my $fh, '<:raw', $source or die "$source: $!\n";
local $/;
my $source_bytes = <$fh>;
close $fh;

my %platforms;
for my $platform (sort keys %Image::ExifTool::Font::ttLang) {
    my $entries = $Image::ExifTool::Font::ttLang{$platform};
    die "non-hash ttLang platform $platform\n" unless ref $entries eq 'HASH';
    my %values;
    for my $id (keys %$entries) {
        die "non-numeric ttLang ID $platform/$id\n" unless $id =~ /\A(?:0|[1-9][0-9]*)\z/ && $id <= 65535;
        my $value = $$entries{$id};
        die "non-scalar ttLang value $platform/$id\n" if !defined($value) || ref($value);
        $values{$id} = "$value";
    }
    $platforms{$platform} = \%values;
}

print JSON::PP->new->canonical->pretty->utf8->encode({
    exiftool_version => "$Image::ExifTool::VERSION",
    source_sha256 => sha256_hex($source_bytes),
    platforms => \%platforms,
});
