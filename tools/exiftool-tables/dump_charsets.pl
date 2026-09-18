#!/usr/bin/env perl
#
# Dump ExifTool's character-set tables (Image::ExifTool::Charset and every
# Image::ExifTool::Charset::* module) to JSON, for codegen_charsets.py.
#
#   dump_charsets.pl <exiftool lib dir>  > charsets.json
#
# Everything is read from the pinned tree by Perl itself, never retyped:
#
#   csType        %Image::ExifTool::Charset::csType (a package variable)
#   unicode2byte  Charset.pm's file-scoped `my %unicode2byte` (the pre-loaded
#                 inverse Latin table). A `my` hash is not reachable from
#                 outside the file, so its one statement is cut out of the
#                 pinned Charset.pm text and evaluated here -- it is a pure
#                 data literal, and the dump fails if the cut is not exactly
#                 one `my %unicode2byte = ( ... );` statement.
#   tables        each %Image::ExifTool::Charset::<Name> hash, loaded with
#                 Charset::LoadCharset exactly as Decompose/Recompose load it.
#
# A table value is emitted as one of
#   {"u": N}                       a single code point
#   {"a": [N, ...]}                several code points (an ARRAY ref)
#   {"h": {"<byte>": VALUE, ...}}  a lead byte of a 2-byte code (a HASH ref)
# and anything else fails the dump (never guessed at).
use strict;
use warnings;
use JSON::PP;

my $lib = shift or die "usage: $0 <exiftool lib dir>\n";
unshift @INC, $lib;
require Image::ExifTool;
require Image::ExifTool::Charset;

sub is_int { my $v = shift; return defined $v && !ref $v && $v =~ /^\d+\z/ }

sub value {
    my ($v, $where, $depth) = @_;
    return { u => 0 + $v } if is_int($v);
    if (ref $v eq 'ARRAY') {
        is_int($_) or die "$where: non-integer array element\n" foreach @$v;
        return { a => [ map { 0 + $_ } @$v ] };
    }
    if (ref $v eq 'HASH' and not $depth) {
        my %h;
        foreach my $k (keys %$v) {
            is_int($k) or die "$where: non-integer key $k\n";
            $h{$k} = value($$v{$k}, "$where\{$k}", 1);
        }
        return { h => \%h };
    }
    die "$where: unsupported value " . (defined $v ? $v : 'undef') . "\n";
}

my %cs = do { no warnings 'once'; %Image::ExifTool::Charset::csType };

# The pre-loaded inverse table, from the pinned source text.
my $pm = "$lib/Image/ExifTool/Charset.pm";
open my $fh, '<', $pm or die "$pm: $!\n";
my $text = do { local $/; <$fh> };
close $fh;
my @stmts = ($text =~ /^(my %unicode2byte = \(.*?^\);)$/msg);
@stmts == 1 or die "expected one `my %unicode2byte = (...);` statement in $pm, found "
    . scalar(@stmts) . "\n";
my %unicode2byte;
{
    my $code = $stmts[0];
    $code =~ s/^my //;
    no strict 'vars';
    eval "\%unicode2byte = do { my $code };" ; # (the literal evaluated as data)
    die "evaluating %unicode2byte: $@" if $@;
}
my %u2b;
foreach my $set (keys %unicode2byte) {
    ref $unicode2byte{$set} eq 'HASH' or die "unicode2byte{$set} is not a hash\n";
    foreach my $k (keys %{$unicode2byte{$set}}) {
        my $v = $unicode2byte{$set}{$k};
        is_int($k) && is_int($v) or die "unicode2byte{$set}: non-integer entry\n";
        $u2b{$set}{$k} = 0 + $v;
    }
}

my %tables;
foreach my $name (sort keys %cs) {
    next unless $cs{$name} & 0x001;     # only these load a translation module
    my $conv = Image::ExifTool::Charset::LoadCharset($name)
        or die "LoadCharset($name) failed\n";
    my %t;
    foreach my $k (keys %$conv) {
        is_int($k) or die "$name: non-integer key $k\n";
        $t{$k} = value($$conv{$k}, "$name\{$k}", 0);
    }
    $tables{$name} = \%t;
}

print JSON::PP->new->canonical->encode({
    csType => { map { $_ => 0 + $cs{$_} } keys %cs },
    unicode2byte => \%u2b,
    tables => \%tables,
}), "\n";
