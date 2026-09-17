#!/usr/bin/env perl
#
# Record, for every tag pinned ExifTool extracts from each file, the exact
# source coordinate that produced it: family-1 group, tag name, full table
# name, tag ID and variant index within GetTagInfoList.  The extraction uses
# the command line's -a (Duplicates) behavior with no user configuration.
#
# Usage: capture_corpus_sources.pl <exiftool-lib> <file>...
# Output: one JSON object {file: [[group1, name, table, tag_id, variant], ...]}
# with a null coordinate when the tag has no table row (e.g. a runtime-built
# table entry absent from GetTagInfoList).
use strict;
use warnings;
BEGIN { no warnings "once"; $Image::ExifTool::configFile = ""; }
use JSON::PP ();

my $lib = shift @ARGV;
die "usage: capture_corpus_sources.pl <exiftool-lib> <file>...\n" unless defined $lib && @ARGV;
unshift @INC, $lib;
require Image::ExifTool;

my %result;
for my $file (@ARGV) {
    my $et = Image::ExifTool->new;
    $et->Options(Duplicates => 1);
    my $info = $et->ImageInfo($file);
    my @rows;
    for my $key (sort keys %$info) {
        my $tag_info = $$et{TAG_INFO}{$key};
        my $group1 = $et->GetGroup($key, 1);
        my $name = Image::ExifTool::GetTagName($key);
        my ($table_name, $tag_id, $variant);
        my $table = ref $tag_info eq 'HASH' ? $$tag_info{Table} : undef;
        if ($table && defined $$tag_info{TagID}) {
            $table_name = $$table{TABLE_NAME};
            $tag_id = "$$tag_info{TagID}";
            my @variants = Image::ExifTool::GetTagInfoList($table, $$tag_info{TagID});
            for my $index (0 .. $#variants) {
                if ($variants[$index] == $tag_info) { $variant = $index; last }
            }
        }
        push @rows, [$group1, $name, $table_name, (defined $variant ? $tag_id : undef), $variant];
    }
    $result{$file} = \@rows;
}
print JSON::PP->new->canonical->utf8->encode(\%result), "\n";
