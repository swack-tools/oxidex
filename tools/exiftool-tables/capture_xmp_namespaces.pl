#!/usr/bin/env perl
#
# Record the XMP namespace facts ExifTool's READ path uses to name a tag's
# family-1 group (XMP.pm FoundXMP: SetGroup "XMP-$ns", then %stdXlatNS):
#   static   %nsURI as XMP.pm loads it. With the 'http://ns.exiftool.ca/1.0/'
#            => 'et' seed, this is the %uri2ns reverse lookup (XMP.pm
#            ~215-221) every read starts from.
#   tables   namespaces a tag table registers only when GetTagTable loads it
#            (ExifTool.pm ~8995 -> RegisterNamespace): known to a read only
#            after that table has loaded in the process, never before.
#   std_xlat %stdXlatNS, applied to the resolved prefix in FoundXMP.
# Structure-table NAMESPACE declarations are registered only on the write
# path (WriteXMP.pl SetPropertyPath) and are deliberately not recorded.
#
# Usage: capture_xmp_namespaces.pl <exiftool-lib>
use strict;
use warnings;
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
use JSON::PP ();

my $lib = shift @ARGV;
die "usage: capture_xmp_namespaces.pl <exiftool-lib>\n" unless defined $lib && !@ARGV;
unshift @INC, $lib;
require Image::ExifTool;
require Image::ExifTool::XMP;
no warnings 'once';

my %static = %Image::ExifTool::XMP::nsURI;
Image::ExifTool::LoadAllTables();
my %tables;
for my $name (sort keys %Image::ExifTool::allTables) {
    my $table = Image::ExifTool::GetTagTable($name);
    my $ns = $$table{NAMESPACE};
    next unless defined $ns && !ref $ns;   # GetTagTable already registered it
    my $uri = $Image::ExifTool::XMP::nsURI{$ns};
    next unless defined $uri;   # a prefix with no URI names no read lookup
    next if exists $static{$ns} && $static{$ns} eq $uri;
    die "table $name changes the URI of static prefix $ns\n" if exists $static{$ns};
    die "table namespace $ns is registered twice with different URIs\n"
        if exists $tables{$ns} && $tables{$ns}{uri} ne $uri;
    $tables{$ns} = { uri => $uri, table => $name };
}
print JSON::PP->new->canonical->pretty->utf8->encode({
    exiftool_version => "$Image::ExifTool::VERSION",
    static => \%static,
    tables => \%tables,
    std_xlat => \%Image::ExifTool::XMP::stdXlatNS,
    uri2ns_seed => { 'http://ns.exiftool.ca/1.0/' => 'et' },
});
