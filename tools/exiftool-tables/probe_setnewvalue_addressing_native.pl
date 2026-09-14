#!/usr/bin/env perl
# Observe the public native SetNewValue address selection without writing a file.
use strict;
use warnings;
use JSON::PP;
use Scalar::Util qw(refaddr);

my ($lib, $tags_path) = @ARGV;
die "usage: probe_setnewvalue_addressing_native.pl EXIFTOOL_LIB tags.json\n" unless $lib && $tags_path;
unshift @INC, $lib;
require Image::ExifTool;
require 'Image/ExifTool/Writer.pl';
open(my $fh, '<:raw', $tags_path) or die "read $tags_path: $!\n";
local $/;
my $tags = decode_json(<$fh>);
close($fh) or die "close $tags_path: $!\n";
die "tags.json is not an array of text\n" unless ref($tags) eq 'ARRAY' and !grep { !defined($_) || ref($_) } @$tags;

my @out;
for my $tag (@$tags) {
    my $et = Image::ExifTool->new;
    my ($count, $error) = $et->SetNewValue($tag, 'probe', NoShortcut => 1);
    my @rows;
    for my $value (values %{ $et->{NEW_VALUE} || {} }) {
        my $info = $value->{TagInfo};
        my $table = $info->{Table};
        push @rows, {
            name => "$info->{Name}", raw_id => "$info->{TagID}", write_group => "$value->{WriteGroup}",
            table_name => (ref($table) eq 'HASH' && defined($table->{TABLE_NAME})) ? "$table->{TABLE_NAME}" : 'unidentified',
        };
    }
    @rows = sort { $a->{table_name} cmp $b->{table_name} || $a->{raw_id} cmp $b->{raw_id} } @rows;
    push @out, { tag => $tag, count => $count, error => $error, rows => \@rows };
}
print JSON::PP->new->canonical->encode(\@out), "\n";
