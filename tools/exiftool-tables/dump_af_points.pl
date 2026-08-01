#!/usr/bin/env perl
# Slices the ten `afPoints*` point-name tables out of Nikon.pm and evals
# them with real Perl, mirroring the %fileTypeExt precedent documented in
# docs/TRANSCRIPTION.md ("One table is not reachable this way" -- these are
# `my` lexicals, invisible to dump_tables.pl's symbol-table walk).
use strict;
use warnings;
use JSON::PP;

my $nikon_pm = $ARGV[0] or die "usage: dump_af_points.pl <path/to/Nikon.pm> <out.json>\n";
my $out_path = $ARGV[1] or die "usage: dump_af_points.pl <path/to/Nikon.pm> <out.json>\n";

open my $fh, '<', $nikon_pm or die "open $nikon_pm: $!\n";
local $/;
my $src = <$fh>;
close $fh;

# Each hash-shaped table: `my %afPointsNNN = ( ... );`
my @hash_tables = qw(afPoints51 afPoints39 afPoints105 afPoints135 afPoints153 afPoints81);
# Each array-shaped table: `my @afPointsNNN = ( ... );` (231/299/405 are
# `qw()` lists; afPoints11 is hash-shaped in the source but semantically an
# 11-slot ordered list once the BITMASK/0/0x7ff special keys are stripped --
# handled separately below).
my @array_tables = qw(afPoints231 afPoints299 afPoints405);

my %result;

for my $name (@hash_tables) {
    $src =~ /my \s+ \%\Q$name\E \s* = \s* \( (.*?) \) \s* ; /sx
        or die "shape changed: could not find 'my \%$name = ( ... );' in $nikon_pm\n";
    my $literal = "\%tmp = ($1);";
    my %tmp;
    { no strict 'vars'; eval $literal; die "eval \%$name failed: $@" if $@; }
    $result{$name} = { kind => 'hash', points => { %tmp } };
}

for my $name (@array_tables) {
    $src =~ /my \s+ \@\Q$name\E \s* = \s* \( \s* qw\( (.*?) \) \s* \) \s* ; /sx
        or die "shape changed: could not find 'my \@$name = (qw(...));' in $nikon_pm\n";
    my @tmp = split ' ', $1;
    $result{$name} = { kind => 'array', points => [ @tmp ] };
}

# afPoints11: `my %afPoints11 = ( 0 => '(none)', 0x7ff => 'All 11 Points',
# BITMASK => { 0 => 'Center', ..., 10 => 'Far Right' } );` -- extract the
# BITMASK sub-hash, ordered 0..10, as a plain 11-slot array (see af_info2.rs
# Task 4 for how the '(none)'/'All 11 Points' literals are handled in Rust).
{
    $src =~ /my \s+ \%afPoints11 \s* = \s* \( (.*?) \) \s* ; /sx
        or die "shape changed: could not find 'my \%afPoints11 = ( ... );'\n";
    my $literal = "\%tmp = ($1);";
    my %tmp;
    { no strict 'vars'; eval $literal; die "eval \%afPoints11 failed: $@" if $@; }
    my $bitmask = $tmp{BITMASK} or die "afPoints11 has no BITMASK key\n";
    my @ordered = map { $bitmask->{$_} } 0 .. 10;
    die "afPoints11 BITMASK is not 0..10\n" if grep { !defined } @ordered;
    $result{afPoints11} = { kind => 'array', points => [ @ordered ] };
}

for my $name (@hash_tables, @array_tables, 'afPoints11') {
    die "missing table: $name\n" unless exists $result{$name};
}

open my $out, '>', $out_path or die "open $out_path: $!\n";
print $out JSON::PP->new->canonical->pretty->encode(\%result);
close $out;
print "wrote $out_path: " . join(', ', map { "$_=" . (ref $result{$_}{points} eq 'ARRAY' ? scalar(@{$result{$_}{points}}) : scalar(keys %{$result{$_}{points}})) } sort keys %result) . "\n";
