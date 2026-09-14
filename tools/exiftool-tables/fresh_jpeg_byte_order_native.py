#!/usr/bin/env python3
"""Capture the loaded native preferred-byte-order branch for fresh JPEG EXIF."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess

HARNESS = r'''
BEGIN { $Image::ExifTool::configFile = ''; }
use strict; use warnings; use B (); use B::Deparse; use Digest::SHA qw(sha256_hex); use JSON::PP;
use Cwd qw(abs_path); use File::Spec;
use Image::ExifTool; require 'Image/ExifTool/Writer.pl';
my $lib=abs_path($ARGV[0]);
sub fact {
    my ($name) = @_; no strict 'refs';
    my $cv=*{$name}{CODE} or die "missing $name";
    my $file=abs_path(B::svref_2object($cv)->FILE) or die "no file $name";
    my $prefix=$lib . '/'; index($file,$prefix)==0 or die "outside selected lib $file";
    open my $fh, '<:raw', $file or die $!; local $/; my $bytes=<$fh>; close $fh;
    return { __perl=>'CODE', __name=>$name, resolved=>JSON::PP::true,
      source_file=>File::Spec->abs2rel($file,$lib), source_sha256=>sha256_hex($bytes),
      __deparse=>B::Deparse->new('-p','-sC')->coderef2text($cv) };
}
sub selected {
    my ($kind) = @_; my $et=Image::ExifTool->new;
    if ($kind eq 'option') { $et->Options(ByteOrder=>'II'); }
    elsif ($kind eq 'new') { $et->SetNewValue('ExifByteOrder'=>'II'); }
    elsif ($kind eq 'maker') { $et->{MAKER_NOTE_BYTE_ORDER}='II'; }
    my $got=$et->SetPreferredByteOrder(undef);
    return { selected=>$got, reported=>Image::ExifTool::GetByteOrder() };
}
my %loaded;
for my $inc (sort keys %INC) {
  next unless $inc eq 'Image/ExifTool.pm' || $inc =~ m{^Image/ExifTool/};
  my $path=abs_path($INC{$inc}) or die "unresolved $inc";
  my $prefix=$lib . '/'; index($path,$prefix)==0 or die "ambient $path";
  open my $fh, '<:raw', $path or die $!; local $/; $loaded{$inc}=sha256_hex(<$fh>); close $fh;
}
my @closure=map {{ file=>$_, sha256=>$loaded{$_} }} sort keys %loaded;
print JSON::PP->new->canonical->encode({
 capture_context=>{kind=>'fresh_jpeg_byte_order_probe_v1',resolved=>JSON::PP::true,
  loaded_modules=>\%loaded, loaded_closure_sha256=>sha256_hex(JSON::PP->new->canonical->encode(\@closure)),
  exiftool_version=>"$Image::ExifTool::VERSION", perl_version=>"$]"},
 set_preferred_byte_order=>fact('Image::ExifTool::SetPreferredByteOrder'),
 new_jpeg_caller=>fact('Image::ExifTool::DoProcessTIFF'),
 observations=>{fresh_ifd0_no_overrides=>selected('default'), byte_order_option=>selected('option'),
                exif_byte_order=>selected('new'), maker_note_byte_order=>selected('maker')},
});
'''


def capture(perl: Path, library: Path) -> dict[str, object]:
    library = library / "lib" if (library / "lib").is_dir() else library
    result = subprocess.run([str(perl), "-I" + str(library), "-e", HARNESS, str(library)],
                            text=True, capture_output=True, check=False, timeout=30)
    if result.returncode:
        raise RuntimeError(f"native preferred-byte-order probe failed ({result.returncode}): {result.stderr}")
    return json.loads(result.stdout)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perl", type=Path, required=True)
    parser.add_argument("--exiftool-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.write_text(json.dumps(capture(args.perl, args.exiftool_dir), sort_keys=True, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
