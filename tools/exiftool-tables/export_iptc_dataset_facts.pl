#!/usr/bin/env perl
# Export the two IPTC IIM record tables used by ProcessIPTC from pinned Perl.
use strict;
use warnings;
BEGIN {
    for my $name (qw(PERL5OPT PERL5LIB PERLLIB)) {
        die "refusing ambient $name\n" if defined $ENV{$name} && length $ENV{$name};
    }
}
use Cwd qw(abs_path);
use Digest::SHA qw(sha256_hex);
use FindBin;
use Getopt::Long qw(GetOptions);
use JSON::PP ();

my $root = abs_path("$FindBin::Bin/../..");
my $tree;
GetOptions('exiftool-dir=s' => \$tree) or die "usage: $0 --exiftool-dir TREE\n";
die "usage: $0 --exiftool-dir TREE\n" unless defined($tree) && !@ARGV;
$tree = abs_path($tree) or die "selected ExifTool tree does not exist\n";
open my $pin_fh, '<', "$root/.exiftool-version" or die "cannot read pin\n";
my $pin = <$pin_fh>;
chomp $pin;
close $pin_fh;
unshift @INC, "$tree/lib";
require Image::ExifTool;
require Image::ExifTool::IPTC;
die "ExifTool release does not match pin\n" unless $Image::ExifTool::VERSION eq $pin;
my $module = 'Image/ExifTool/IPTC.pm';
my $source_path = abs_path("$tree/lib/$module") or die "missing IPTC.pm\n";
my $loaded_path = defined($INC{$module}) ? abs_path($INC{$module}) : undef;
die "IPTC.pm loaded outside selected tree\n"
    unless defined($loaded_path) && $loaded_path eq $source_path;
open my $source_fh, '<:raw', $source_path or die "cannot read IPTC.pm\n";
my $source = do { local $/; <$source_fh> };
close $source_fh;

my @rows;
for my $pair ([1, 'EnvelopeRecord', 14], [2, 'ApplicationRecord', 70]) {
    my ($record, $table, $expected_count) = @$pair;
    no strict 'refs';
    my $tags = \%{"Image::ExifTool::IPTC::${table}"};
    use strict 'refs';
    my @ids = sort { $a <=> $b } grep { /^[0-9]+\z/ } keys %$tags;
    die "unexpected $table row count\n" unless @ids == $expected_count;
    for my $id (@ids) {
        my $tag = $tags->{$id};
        die "unsupported $table row $id\n"
            unless ref($tag) eq 'HASH' && defined($tag->{Name})
                && !ref($tag->{Name}) && defined($tag->{Format}) && !ref($tag->{Format});
        my $flags = $tag->{Flags} // '';
        die "unsupported $table Flags $id\n" if ref($flags);
        my $list = ($flags =~ /(?:^|\s)List(?:\s|\z)/ || $tag->{List}) ? JSON::PP::true : JSON::PP::false;
        my $conversion = (defined($tag->{PrintConv}) || defined($tag->{ValueConv}) || defined($tag->{RawConv}))
            ? JSON::PP::true : JSON::PP::false;
        push @rows, {
            record => 0 + $record, dataset => 0 + $id, name => $tag->{Name},
            format => $tag->{Format}, list => $list, conversion => $conversion,
        };
    }
}
print JSON::PP->new->canonical->utf8->encode({
    schema => 'iptc_dataset_facts_v1', exiftool_version => $pin,
    module => $module, source_sha256 => sha256_hex($source), rows => \@rows,
});
