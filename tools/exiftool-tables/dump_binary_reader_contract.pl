#!/usr/bin/env perl
# Capture the unsigned-reader primitive contract in a fresh process. The
# caller may compare this output to its own loaded CODE refs without allowing
# SetByteOrder to mutate the caller's table-walking process.
use strict;
use warnings;
use Cwd qw(abs_path);
use FindBin;
use JSON::PP;
use lib $FindBin::Bin;
use OxiDex::NativeReaderContract qw(capture_in_process);

my $EXIFTOOL_LIB = shift @ARGV or die "usage: $0 <exiftool-lib-dir>\n";
unshift @INC, $EXIFTOOL_LIB;
my $EXIFTOOL_LIB_ABS = abs_path($EXIFTOOL_LIB)
    or die "invalid exiftool lib: $EXIFTOOL_LIB\n";
require Image::ExifTool;

my $json = JSON::PP->new->utf8->canonical->pretty;
print $json->encode(capture_in_process($EXIFTOOL_LIB_ABS));
