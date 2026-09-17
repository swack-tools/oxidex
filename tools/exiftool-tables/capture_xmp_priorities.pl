#!/usr/bin/env perl
#
# Record the FoundTag priority ExifTool's READ path gives each XMP tag, so a
# reader can arbitrate same-named XMP tags the way ExifTool.pm FoundTag does:
#
#   my $priority = $$tagInfo{Priority};
#   unless (defined $priority) {
#       $priority = $$tbl{PRIORITY};
#       $priority = 0 if not defined $priority and $$tagInfo{Avoid};
#   }
#   ... (undefined after that -> 1, outside a PRIORITY_DIR/LOW_PRIORITY_DIR)
#
# FoundXMP (XMP.pm) picks the tag table by the property's namespace prefix
# AFTER %stdXlatNS, as a key of %Image::ExifTool::XMP::Main, so that is the
# key recorded here (`iptcCore`, `photomech`, `Device`). A property that is
# not an entry of its table -- or whose prefix names no table -- is minted
# with `Priority => 0` (XMP.pm ~3595) and is not recorded: absent means 0.
#
# Each table is flattened first (Image::ExifTool::XMP::AddFlattenedTags), so
# the structure fields FoundXMP reports by flattened name are entries too.
# Table AVOID is applied by GetTagTable/SetupTagTable and AddTagToTable.
#
# Output, per Main key: the table name, the table PRIORITY (or null), and
# tag Name -> effective priority. A Name that occurs twice in one table with
# different effective priorities cannot be keyed by Name, so it dies.
#
# Usage: capture_xmp_priorities.pl <exiftool-lib>
use strict;
use warnings;
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
use JSON::PP ();

my $lib = shift @ARGV;
die "usage: capture_xmp_priorities.pl <exiftool-lib>\n" unless defined $lib && !@ARGV;
unshift @INC, $lib;
require Image::ExifTool;
require Image::ExifTool::XMP;
no warnings 'once';

my $main = Image::ExifTool::GetTagTable('Image::ExifTool::XMP::Main');
my %namespaces;
for my $key (sort(Image::ExifTool::TagTableKeys($main))) {
    my $info = $$main{$key};
    next unless ref $info eq 'HASH' and $$info{SubDirectory} and $$info{SubDirectory}{TagTable};
    my $name = $$info{SubDirectory}{TagTable};
    my $table = Image::ExifTool::GetTagTable($name) or die "cannot load $name\n";
    Image::ExifTool::XMP::AddFlattenedTags($table);
    my %tags;
    for my $id (sort(Image::ExifTool::TagTableKeys($table))) {
        for my $tagInfo (Image::ExifTool::GetTagInfoList($table, $id)) {
            next unless ref $tagInfo eq 'HASH';
            my $tagName = $$tagInfo{Name};
            die "$name $id has no Name\n" unless defined $tagName;
            my $priority = $$tagInfo{Priority};
            unless (defined $priority) {
                $priority = $$table{PRIORITY};
                $priority = 0 if not defined $priority and $$tagInfo{Avoid};
            }
            $priority = 1 unless defined $priority;
            $priority = 0 + $priority;
            die "$name: tag $tagName has effective priorities $tags{$tagName} and $priority\n"
                if exists $tags{$tagName} and $tags{$tagName} != $priority;
            $tags{$tagName} = $priority;
        }
    }
    die "Main key $key maps to two tables\n" if exists $namespaces{$key};
    $namespaces{$key} = {
        table => $name,
        table_priority => defined $$table{PRIORITY} ? 0 + $$table{PRIORITY} : undef,
        tags => \%tags,
    };
}
print JSON::PP->new->canonical->pretty->utf8->encode({
    exiftool_version => "$Image::ExifTool::VERSION",
    namespaces => \%namespaces,
});
