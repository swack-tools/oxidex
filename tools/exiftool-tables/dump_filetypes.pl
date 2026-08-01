#!/usr/bin/env perl
#
# Dump ExifTool's file-identification tables to JSON.
#
# These are ordinary package hashes rather than tag tables, so they are not
# reachable through dump_tables.pl's tag-table heuristics -- but they are just
# as much pure data:
#
#   %magicNumber     file type -> Perl regex matched against the file header
#   %fileTypeLookup  extension -> file type (or an alias to another entry)
#   %mimeType        file type -> MIME type
#
# Together they are how ExifTool answers FileType / FileTypeExtension /
# MIMEType, which in the comparison corpus were the three most-missed tags
# after the composites. Transcribing them is strictly cheaper than
# reimplementing format sniffing by hand, and it cannot drift from ExifTool
# because it *is* ExifTool's table.
#
# Magic numbers are emitted as raw bytes hex-encoded, because they are byte
# patterns rather than text and any encoding step would corrupt them.

use strict;
use warnings;
use JSON::PP;

my $LIB = shift @ARGV or die "usage: $0 <exiftool-lib-dir>\n";
unshift @INC, $LIB;
require Image::ExifTool;

no strict 'refs';
no warnings 'once';   # these globals are read-only accesses into ExifTool

my %magic;
{
    my $h = \%Image::ExifTool::magicNumber;
    for my $k (keys %$h) {
        my $v = $h->{$k};
        next unless defined $v && !ref $v;
        # Hex-encode: these are byte-level regexes containing NULs and
        # arbitrary high bytes.
        $magic{$k} = unpack('H*', $v);
    }
}

my %lookup;
{
    my $h = \%Image::ExifTool::fileTypeLookup;
    for my $k (keys %$h) {
        my $v = $h->{$k};
        if (!ref $v) {
            # A plain string is an alias to another extension.
            $lookup{$k} = { alias => $v };
        } elsif (ref $v eq 'ARRAY') {
            # [ file type(s), description ]; the type may itself be an array
            # when one extension maps to several formats.
            my ($type, $desc) = @$v;
            my @types = ref $type eq 'ARRAY' ? @$type : ($type);
            $lookup{$k} = { types => \@types, desc => (defined $desc ? "$desc" : undef) };
        }
    }
}

my %mime;
{
    my $h = \%Image::ExifTool::mimeType;
    for my $k (keys %$h) {
        my $v = $h->{$k};
        $mime{$k} = $v if defined $v && !ref $v;
    }
}

# %fileTypeExt overrides the default FileTypeExtension, which is otherwise just
# the lowercased file type: DICOM -> dcm, JPEG -> jpg, GZIP -> gz.
#
# It is a lexical `my` hash, so unlike everything else here it is not reachable
# through the symbol table. Rather than retype nine pairs by hand -- the exact
# manual transcription this tooling exists to avoid -- slice the literal out of
# the source and let Perl eval it. The parsing is Perl's, not a regex's; the
# regex only finds the block boundaries, and a malformed result is a hard error
# rather than a silent empty table.
my %file_type_ext;
{
    my $src_path = "$LIB/Image/ExifTool.pm";
    open(my $fh, '<', $src_path) or die "open $src_path: $!";
    my $src = do { local $/; <$fh> };
    close $fh;
    if ($src =~ /^my\s+%fileTypeExt\s*=\s*\((.*?)^\);/ms) {
        my $body = $1;
        # The literal is plain key => value pairs; anything else means the
        # upstream shape changed and we must not guess.
        die "unexpected content in %fileTypeExt\n" if $body =~ /[\$\@\&]/;
        my %h = eval "($body)";
        die "failed to eval %fileTypeExt: $@\n" if $@;
        die "%fileTypeExt came out empty\n" unless keys %h;
        %file_type_ext = %h;
    } else {
        die "could not locate %fileTypeExt in $src_path\n";
    }
}

# Order matters: ExifTool tests magic numbers in a fixed sequence and takes the
# first hit, so a generator that iterated a hash would produce a different
# answer for any file matching two patterns.
my @order = @Image::ExifTool::fileTypes;

print JSON::PP->new->utf8->canonical->pretty->encode({
    exiftool_version => $Image::ExifTool::VERSION,
    magic_number     => \%magic,
    file_type_lookup => \%lookup,
    mime_type        => \%mime,
    file_type_ext    => \%file_type_ext,
    test_order       => \@order,
});
