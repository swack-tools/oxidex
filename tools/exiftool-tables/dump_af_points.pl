#!/usr/bin/env perl
# Slices the ten `afPoints*` point-name tables out of Nikon.pm and evals
# them with real Perl, mirroring the %fileTypeExt precedent documented in
# docs/TRANSCRIPTION.md ("One table is not reachable this way" -- these are
# `my` lexicals, invisible to dump_tables.pl's symbol-table walk).
#
# Older releases legitimately lack some of these tables (ExifTool 11.78's
# Nikon.pm declares only afPoints51/39/135/153). A table is recorded as
# `kind => 'absent'` only when that is proven from the release's own source:
# its name occurs nowhere in Nikon.pm -- not declared, not referenced, not
# even in a comment -- and nowhere in any other `.pm` file of the same
# release `lib/` tree. The release's version label is never consulted.
#
# Any other mismatch still refuses loudly. If the name occurs anywhere in
# Nikon.pm but the expected declaration shape does not match, the table is
# present in a changed shape, and guessing at it would silently mis-parse
# it ("shape changed"). If the name is gone from Nikon.pm but occurs in
# another module, the table moved, and "absent" would be false ("moved").
use strict;
use warnings;
use JSON::PP;
use Digest::SHA qw(sha256_hex);
use File::Find qw(find);
use File::Spec;

my $nikon_pm = $ARGV[0] or die "usage: dump_af_points.pl <path/to/Nikon.pm> <out.json>\n";
my $out_path = $ARGV[1] or die "usage: dump_af_points.pl <path/to/Nikon.pm> <out.json>\n";

sub slurp {
    my ($path) = @_;
    open my $fh, '<', $path or die "open $path: $!\n";
    local $/;
    my $text = <$fh>;
    close $fh;
    return $text;
}

my $src = slurp($nikon_pm);

# Each hash-shaped table: `my %afPointsNNN = ( ... );`
my @hash_tables = qw(afPoints51 afPoints39 afPoints105 afPoints135 afPoints153 afPoints81);
# Each array-shaped table: `my @afPointsNNN = ( ... );` (231/299/405 are
# `qw()` lists; afPoints11 is hash-shaped in the source but semantically an
# 11-slot ordered list once the BITMASK/0/0x7ff special keys are stripped --
# handled separately below).
my @array_tables = qw(afPoints231 afPoints299 afPoints405);

my %result;

sub occurrences {
    my ($text, $name) = @_;
    my $count = () = $text =~ /(?<![A-Za-z0-9_])\Q$name\E(?![A-Za-z0-9_])/g;
    return $count;
}

# Prove the release-wide absence of $name, or die. Called only once
# Nikon.pm itself holds zero occurrences of the name.
my @release_modules;
sub release_modules {
    return @release_modules if @release_modules;
    my $abs = File::Spec->rel2abs($nikon_pm);
    $abs =~ m{^(.*)/Image/ExifTool/Nikon\.pm\z}
        or die "cannot prove absence: $nikon_pm is not <lib>/Image/ExifTool/Nikon.pm, "
             . "so the rest of its release cannot be scanned\n";
    my $lib = $1;
    -r "$lib/Image/ExifTool.pm"
        or die "cannot prove absence: $lib/Image/ExifTool.pm is missing, "
             . "so $lib is not a complete release lib/ tree\n";
    find({ no_chdir => 1, wanted => sub {
        push @release_modules, $File::Find::name if -f $_ && /\.pm\z/;
    } }, "$lib/Image");
    @release_modules = sort @release_modules;
    return @release_modules;
}

sub absent_or_die {
    my ($name, $expected_kind) = @_;
    my @modules = release_modules();
    for my $pm (@modules) {
        next if File::Spec->rel2abs($pm) eq File::Spec->rel2abs($nikon_pm);
        my $n = occurrences(slurp($pm), $name);
        die "moved: '$name' no longer occurs in $nikon_pm but occurs $n time(s) in $pm\n"
            if $n;
    }
    return {
        kind => 'absent',
        expected_kind => $expected_kind,
        proof => {
            source => 'Image/ExifTool/Nikon.pm',
            source_sha256 => sha256_hex($src),
            occurrences_in_source => 0,
            release_modules_scanned => scalar(@modules),
            occurrences_in_release => 0,
        },
    };
}

for my $name (@hash_tables) {
    if (!occurrences($src, $name)) { $result{$name} = absent_or_die($name, 'hash'); next }
    $src =~ /my \s+ \%\Q$name\E \s* = \s* \( (.*?) \) \s* ; /sx
        or die "shape changed: could not find 'my \%$name = ( ... );' in $nikon_pm\n";
    my $literal = "\%tmp = ($1);";
    my %tmp;
    { no strict 'vars'; eval $literal; die "eval \%$name failed: $@" if $@; }
    $result{$name} = { kind => 'hash', points => { %tmp } };
}

for my $name (@array_tables) {
    if (!occurrences($src, $name)) { $result{$name} = absent_or_die($name, 'array'); next }
    $src =~ /my \s+ \@\Q$name\E \s* = \s* \( \s* qw\( (.*?) \) \s* \) \s* ; /sx
        or die "shape changed: could not find 'my \@$name = (qw(...));' in $nikon_pm\n";
    my @tmp = split ' ', $1;
    $result{$name} = { kind => 'array', points => [ @tmp ] };
}

# afPoints11: `my %afPoints11 = ( 0 => '(none)', 0x7ff => 'All 11 Points',
# BITMASK => { 0 => 'Center', ..., 10 => 'Far Right' } );` -- extract the
# BITMASK sub-hash, ordered 0..10, as a plain 11-slot array (see af_info2.rs
# Task 4 for how the '(none)'/'All 11 Points' literals are handled in Rust).
if (!occurrences($src, 'afPoints11')) {
    $result{afPoints11} = absent_or_die('afPoints11', 'array');
} else {
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

sub summary {
    my ($entry) = @_;
    return 'absent' if $entry->{kind} eq 'absent';
    my $points = $entry->{points};
    return ref $points eq 'ARRAY' ? scalar(@$points) : scalar(keys %$points);
}

open my $out, '>', $out_path or die "open $out_path: $!\n";
print $out JSON::PP->new->canonical->pretty->encode(\%result);
close $out;
print "wrote $out_path: " . join(', ', map { "$_=" . summary($result{$_}) } sort keys %result) . "\n";
