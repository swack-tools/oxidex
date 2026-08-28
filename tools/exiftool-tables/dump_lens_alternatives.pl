#!/usr/bin/env perl
#
# Emit the FRACTIONAL ("$id.$n") alternatives of the manufacturer LensType
# PrintConv hashes, as the Rust body of `src/composite/lens_alternatives.rs`.
#
# WHY THIS EXISTS
#
# `src/parsers/tiff/makernotes/lens_data.rs` carries only the *integer* keys of
# `%Image::ExifTool::Canon::canonLensTypes` and friends -- the entries a plain
# `<Maker>:LensType` lookup needs.  Its own doc comment records that the
# fractional keys were deliberately left out because "they belong with
# `Composite:LensID`, which oxidex does not implement".  This script is what
# closes that half, now that it does.
#
# WHY THE OUTPUT IS KEYED BY A STRING, NOT BY THE NUMERIC ID
#
# Both ExifTool routines that consult the fractional keys build their candidate
# list identically -- Canon.pm:10191-10196 and Exif.pm:5963-5970:
#
#     $lens =~ s/ or .*//s;    # remove everything after "or"
#     my @lenses = ( $lens );
#     for ($i=1; $$printConv{"$lensType.$i"}; ++$i) {
#         push @lenses, $$printConv{"$lensType.$i"};
#     }
#
# so the only handle either needs on the integer key is the string stored
# there, which is exactly what oxidex already emits as `<Maker>:LensType`.
# Keying by that string therefore needs no numeric plumbing through the
# makernote parsers (whose insert shim stores the print form only), and this
# script HARD-ERRORS if two ambiguous ids in one table ever share a string, at
# which point the string would stop determining the alternative list and the
# shortcut would no longer be exact.
#
# USAGE
#     perl tools/exiftool-tables/dump_lens_alternatives.pl \
#         --exiftool-dir /tmp/oxidex-exiftool-cache/exiftool
#
# Never point this at a bare `exiftool` on PATH: the pinned release named by
# `.exiftool-version` is the only source of truth (AGENTS.md).
use strict;
use warnings;
use Getopt::Long;

my $dir = '/tmp/oxidex-exiftool-cache/exiftool';
GetOptions('exiftool-dir=s' => \$dir) or die "bad options\n";
unshift @INC, "$dir/lib";

require Image::ExifTool;
require Image::ExifTool::Canon;
require Image::ExifTool::Pentax;
require Image::ExifTool::Olympus;
require Image::ExifTool::Panasonic;

printf STDERR "ExifTool %s from %s\n", $Image::ExifTool::VERSION, $dir;

sub rows {
    my ($label, $h) = @_;
    my %base;
    for my $k (keys %$h) {
        next if ref $h->{$k};
        $base{$k} = $h->{$k} if $k !~ /\./;
    }
    my (@rows, %seen);
    for my $id (sort { ($a =~ /^-?[\d.]+$/ && $b =~ /^-?[\d.]+$/) ? $a <=> $b : $a cmp $b }
                keys %base)
    {
        next unless exists $h->{"$id.1"};
        die "$label: two ambiguous ids share the string '$base{$id}' -- the "
          . "string no longer determines the alternative list, see this "
          . "script's header\n"
            if $seen{$base{$id}}++;
        my @alt;
        for (my $i = 1; exists $h->{"$id.$i"}; ++$i) { push @alt, $h->{"$id.$i"} }
        push @rows, [ $id, $base{$id}, \@alt ];
    }
    return @rows;
}

sub rs { my $s = shift; $s =~ s/([\\"])/\\$1/g; return "\"$s\"" }

sub emit {
    my ($const, $doc, @rows) = @_;
    print "$doc\n";
    printf "pub static %s: [(&str, &[&str]); %d] = [\n", $const, scalar(@rows);
    for my $r (@rows) {
        printf "    // id %s\n", $r->[0];
        printf "    (%s,\n     &[%s]),\n", rs($r->[1]), join(', ', map { rs($_) } @{$r->[2]});
    }
    print "];\n\n";
}

emit('CANON_LENS_ALTERNATIVES',
     "/// `%Image::ExifTool::Canon::canonLensTypes` (Canon.pm:97): the integer\n"
   . "/// ids that carry at least one `.N` alternative, keyed by the string at\n"
   . "/// the integer key, with the alternatives in ExifTool's `.1 .. .N` order.",
     rows('canon', \%Image::ExifTool::Canon::canonLensTypes));

emit('PENTAX_LENS_ALTERNATIVES',
     "/// `%Image::ExifTool::Pentax::pentaxLensTypes` (Pentax.pm:118): the ids\n"
   . "/// that carry at least one `.N` alternative, same shape as Canon's.",
     rows('pentax', \%Image::ExifTool::Pentax::pentaxLensTypes));

# Not emitted, but asserted: %olympusLensTypes (reached through the Equipment
# table's tag 0x0201 PrintConv, since the hash itself is a lexical) has NO
# fractional keys at all, which is why `LensTable::Olympus` can return an empty
# alternatives slice instead of a table.
my $oly = $Image::ExifTool::Olympus::Equipment{0x0201}{PrintConv};
my @olyfrac = grep { /\./ } keys %$oly;
die "olympusLensTypes grew " . scalar(@olyfrac) . " fractional keys; "
  . "LensTable::Olympus's empty table is no longer correct\n" if @olyfrac;
printf STDERR "olympusLensTypes: %d keys, 0 fractional (asserted)\n", scalar(keys %$oly);
