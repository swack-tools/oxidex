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

my $timeout = 60;
if (@ARGV && $ARGV[0] eq '--timeout') {
    shift @ARGV;
    $timeout = shift @ARGV // die "missing timeout seconds\n";
    die "invalid timeout seconds: $timeout\n"
        unless $timeout =~ /\A[1-9]\d*\z/;
}
my $EXIFTOOL_LIB = shift @ARGV or die "usage: $0 [--timeout seconds] <exiftool-lib-dir>\n";
die "unexpected argument: $ARGV[0]\n" if @ARGV;
unshift @INC, $EXIFTOOL_LIB;
my $EXIFTOOL_LIB_ABS = abs_path($EXIFTOOL_LIB)
    or die "invalid exiftool lib: $EXIFTOOL_LIB\n";
# The extractor is deliberately a disposable process. Bound loading and the
# exhaustive native probe so a changed upstream primitive cannot hang a normal
# dump regeneration; the caller records this non-zero child as unresolved.
$SIG{ALRM} = sub { die "native_reader_contract_timeout\n" };
alarm $timeout;
require Image::ExifTool;
my $contract = capture_in_process($EXIFTOOL_LIB_ABS);
alarm 0;

my $json = JSON::PP->new->utf8->canonical->pretty;
print $json->encode($contract);
