#!/usr/bin/env perl
# Capture native FindTagInfo candidates for already-generated static row names.
use strict;
use warnings;
use JSON::PP;
use Scalar::Util qw(refaddr);

my ($lib, $rows_path) = @ARGV;
die "usage: setnewvalue_address_probe.pl EXIFTOOL_LIB rows.json\n" unless $lib && $rows_path;
unshift @INC, $lib;
require Image::ExifTool;
require 'Image/ExifTool/Writer.pl';

open(my $fh, '<:raw', $rows_path) or die "read $rows_path: $!\n";
local $/;
my $rows = decode_json(<$fh>);
close($fh) or die "close $rows_path: $!\n";
die "rows.json is not an array\n" unless ref($rows) eq 'ARRAY';

my (%selected, %names);
for my $row (@$rows) {
    die "row is not an object\n" unless ref($row) eq 'HASH';
    for my $key (qw(module table full_name raw_id name)) {
        die "row $key is not text\n" unless defined($row->{$key}) && !ref($row->{$key});
    }
    my $table = Image::ExifTool::GetTagTable($row->{full_name})
        or die "cannot load selected table $row->{full_name}\n";
    $selected{refaddr($table)} = { map { $_ => $row->{$_} } qw(module table full_name) };
    $names{lc $row->{name}} //= $row->{name};
}
my $et = Image::ExifTool->new;
my %queries;
for my $lower (sort keys %names) {
    my $name = $names{$lower};
    my @candidates;
    for my $info (Image::ExifTool::TagLookup::FindTagInfo($name)) {
        my $table = $info->{Table};
        my %candidate = (name => "$info->{Name}", raw_id => "$info->{TagID}");
        if (my $identity = $selected{refaddr($table)}) {
            @candidate{qw(module table full_name)} = @{$identity}{qw(module table full_name)};
        } else {
            $candidate{external_table} = (ref($table) eq 'HASH' && defined($table->{TABLE_NAME}))
                ? "$table->{TABLE_NAME}" : 'unidentified';
        }
        my @groups = $et->GetGroup($info);
        $candidate{groups} = { map { $_ => "$groups[$_]" } grep { defined $groups[$_] } 0 .. $#groups };
        push @candidates, \%candidate;
    }
    $queries{$lower} = { query => $name, candidates => \@candidates };
}
print JSON::PP->new->canonical->encode({ schema => 'native_setnewvalue_addressing_v1', queries => \%queries }), "\n";
