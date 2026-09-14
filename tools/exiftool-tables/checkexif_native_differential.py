#!/usr/bin/env python3
"""Record actual pinned-native CheckExif behavior for generated-route inputs.

This is a differential fixture producer, not a writer.  Every result comes
from the loaded Perl helper operating on a clone of Exif::Main HostComputer's
real tag-info hash.  It records callable/source authentication alongside the
raw scalar state so a Rust CheckExif executor can be compared later.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile


SOURCE_RELATIVE = Path("Image/ExifTool/WriteExif.pl")
CHECK_VALUE_RELATIVE = Path("Image/ExifTool/Writer.pl")
MUTATION = (
    (
        "my $format = $$tagInfo{Format} || $$tagInfo{Writable} || $$tagInfo{Table}{WRITABLE};",
        "my $format = $$tagInfo{Writable} || $$tagInfo{Format} || $$tagInfo{Table}{WRITABLE};",
    ),
    ("return 'No writable format';", "return 'Changed no writable format';"),
)

PERL_HARNESS = r'''
BEGIN { no warnings 'once'; $Image::ExifTool::configFile = ''; }
use strict; use warnings; use JSON::PP; use Encode (); use B (); use B::Deparse;
use Digest::SHA qw(sha256_hex); use Image::ExifTool; use Image::ExifTool::Exif;
require 'Image/ExifTool/Writer.pl'; require 'Image/ExifTool/WriteExif.pl'; local $/;
my $rows=JSON::PP->new->utf8->decode(<STDIN>); my @results;
my $cv=\&Image::ExifTool::Exif::CheckExif;
my $body=B::Deparse->new('-p','-sC')->coderef2text($cv);
my $cv_file=B::svref_2object($cv)->FILE;
open my $source, '<:raw', $cv_file or die "open $cv_file: $!";
my $source_sha=sha256_hex(<$source>); close $source;
my $check_value=\&Image::ExifTool::CheckValue;
my $check_value_body=B::Deparse->new('-p','-sC')->coderef2text($check_value);
my $check_value_file=B::svref_2object($check_value)->FILE;
open my $check_value_source, '<:raw', $check_value_file or die "open $check_value_file: $!";
my $check_value_source_sha=sha256_hex(<$check_value_source>); close $check_value_source;
my $et=Image::ExifTool->new;
my $host=$et->GetTagInfo(\%Image::ExifTool::Exif::Main, 0x013c)
    or die 'HostComputer tag-info unavailable';
sub property {
    my ($property) = @_;
    return $property unless ref $property eq 'HASH';
    my $kind=$property->{kind};
    return undef if $kind eq 'undefined';
    return $property->{value} if $kind eq 'integer';
    return pack('H*', $property->{hex}) if $kind eq 'bytes';
    return $property->{text} if $kind eq 'utf8';
    die "unsupported property kind $kind";
}
for my $row (@$rows) {
    my %tag=%$host; my %table=%{$$host{Table}};
    my %groups=%{$$host{Groups} || {}};
    $tag{Table}=\%table; $tag{Groups}=\%groups;
    for my $key (qw(format writable count)) {
        my $target=$key eq 'format' ? 'Format' : $key eq 'writable' ? 'Writable' : 'Count';
        exists $row->{$key} ? $tag{$target}=property($row->{$key}) : delete $tag{$target};
    }
    $table{WRITABLE}=property($row->{table_writable}); $groups{0}=property($row->{group0});
    my $scalar=$row->{scalar};
    my $value=$scalar->{kind} eq 'undefined' ? undef :
        $scalar->{kind} eq 'bytes' ? pack('H*',$scalar->{hex}) : $scalar->{text};
    utf8::upgrade($value) if $scalar->{kind} eq 'utf8';
    my $error=Image::ExifTool::Exif::CheckExif(undef,\%tag,\$value);
    my $utf8=defined($value) && utf8::is_utf8($value) ? JSON::PP::true : JSON::PP::false;
    my $bytes=defined($value) ? ($utf8 ? Encode::encode('UTF-8',$value) : $value) : undef;
    push @results, {name=>$row->{name}, error=>$error, defined=>defined($value)?JSON::PP::true:JSON::PP::false,
        utf8=>$utf8, hex=>defined($bytes)?unpack('H*',$bytes):undef,
        character_length=>defined($value)?length($value):undef};
}
print JSON::PP->new->canonical->utf8->encode({
    callable=>'Image::ExifTool::Exif::CheckExif', callable_file=>$cv_file,
    source_sha256=>$source_sha, body_sha256=>sha256_hex($body),
    check_value=>{ callable=>'Image::ExifTool::CheckValue', callable_file=>$check_value_file,
        source_sha256=>$check_value_source_sha, body_sha256=>sha256_hex($check_value_body) },
    results=>\@results,
});
'''


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def resolve_perl(supplied: Path) -> Path:
    """Resolve a deliberate bare executable through this process's PATH only."""
    raw = str(supplied)
    if os.sep not in raw and (not os.altsep or os.altsep not in raw):
        located = shutil.which(raw, path=os.environ.get("PATH"))
        if located is None:
            raise ValueError(f"selected Perl executable is not on PATH: {raw}")
        return Path(located).resolve()
    return supplied.expanduser().resolve()


def resolve_library(supplied: Path) -> Path:
    library = supplied.expanduser().resolve()
    if (library / "lib").is_dir():
        library = (library / "lib").resolve()
    return library


def run_native(
    perl: Path, library: Path, cases: list[dict[str, object]], fallback_library: Path | None = None
) -> dict[str, object]:
    env = dict()
    for key, value in os.environ.items():
        if key not in {"PERL5LIB", "PERLLIB", "PERL5OPT", "PERL_MM_OPT", "PERL_MB_OPT"}:
            env[key] = value
    command = [str(perl), "-I" + str(library)]
    if fallback_library is not None:
        command.append("-I" + str(fallback_library))
    result = subprocess.run(
        [*command, "-e", PERL_HARNESS],
        input=json.dumps(cases, ensure_ascii=False).encode("utf-8"), env=env,
        capture_output=True, timeout=30, check=False,
    )
    if result.returncode:
        raise RuntimeError(
            f"native CheckExif harness failed ({result.returncode}): "
            f"{result.stderr.decode(errors='replace')}"
        )
    return json.loads(result.stdout)


def mutate_library(source_library: Path) -> tempfile.TemporaryDirectory[str]:
    temporary = tempfile.TemporaryDirectory(prefix="oxidex-checkexif-mutated-")
    root = Path(temporary.name)
    changed = root / SOURCE_RELATIVE
    changed.parent.mkdir(parents=True)
    source = (source_library / SOURCE_RELATIVE).read_text(encoding="utf-8")
    for before, after in MUTATION:
        if source.count(before) != 1:
            temporary.cleanup()
            raise RuntimeError(f"mutation anchor is not unique: {before!r}")
        source = source.replace(before, after)
    changed.write_text(source, encoding="utf-8")
    return temporary


def write_evidence(
    directory: Path, payload: dict[str, object], *, source: Path,
    check_value_source: Path, cases: list[dict[str, object]],
) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "checkexif-cases.json").write_text(json.dumps(cases, indent=2) + "\n", encoding="utf-8")
    (directory / "checkexif-native.json").write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (directory / "WriteExif.pl").write_bytes(source.read_bytes())
    (directory / "Writer.pl").write_bytes(check_value_source.read_bytes())
    (directory / "source-sha256.txt").write_text(sha256(source) + "\n", encoding="utf-8")
    (directory / "scope.json").write_text(json.dumps({
        "scope": "direct Image::ExifTool::Exif::CheckExif helper invocation",
        "not_covered": "SetNewValue/Sanitize and public writer admission",
    }, indent=2) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--perl", type=Path, default=os.environ.get("EXIFTOOL_PERL"))
    parser.add_argument("--lib", type=Path, default=os.environ.get("OXIDEX_PINNED_EXIFTOOL"))
    parser.add_argument("--cases", type=Path, default=Path(__file__).with_name("testdata") / "checkexif_native_cases.json")
    parser.add_argument("--evidence-root", type=Path, required=True)
    args = parser.parse_args()
    if args.perl is None or args.lib is None:
        parser.error("--perl/--lib are required (or set EXIFTOOL_PERL/OXIDEX_PINNED_EXIFTOOL)")
    try:
        args.perl = resolve_perl(args.perl)
    except ValueError as error:
        parser.error(str(error))
    args.lib = resolve_library(args.lib)
    if (not args.perl.is_file() or not os.access(args.perl, os.X_OK)
            or not (args.lib / SOURCE_RELATIVE).is_file()
            or not (args.lib / CHECK_VALUE_RELATIVE).is_file()):
        raise SystemExit("selected Perl or ExifTool library is unavailable")
    cases = json.loads(args.cases.read_text(encoding="utf-8"))
    if not isinstance(cases, list):
        raise SystemExit("case fixture must be an array")

    canonical = run_native(args.perl, args.lib, cases)
    source = args.lib / SOURCE_RELATIVE
    check_value_source = args.lib / CHECK_VALUE_RELATIVE
    if canonical["callable_file"] != str(source) or canonical["source_sha256"] != sha256(source):
        raise SystemExit("canonical CheckExif callable/source authentication failed")
    if (canonical["check_value"]["callable_file"] != str(check_value_source)
            or canonical["check_value"]["source_sha256"] != sha256(check_value_source)):
        raise SystemExit("canonical CheckValue callable/source authentication failed")
    write_evidence(args.evidence_root / "canonical", canonical, source=source,
                   check_value_source=check_value_source, cases=cases)

    temporary = mutate_library(args.lib)
    try:
        changed_library = Path(temporary.name)
        changed = run_native(args.perl, changed_library, cases, args.lib)
        changed_source = changed_library / SOURCE_RELATIVE
        if changed["callable_file"] != str(changed_source) or changed["source_sha256"] != sha256(changed_source):
            raise SystemExit("mutated CheckExif callable/source authentication failed")
        if (changed["check_value"]["callable_file"] != str(check_value_source)
                or changed["check_value"]["source_sha256"] != sha256(check_value_source)):
            raise SystemExit("mutated CheckValue callable/source authentication failed")
        if changed["results"] == canonical["results"]:
            raise SystemExit("copied-source mutation did not change native results")
        write_evidence(args.evidence_root / "changed", changed, source=changed_source,
                       check_value_source=check_value_source, cases=cases)
    finally:
        temporary.cleanup()
    print(json.dumps({"evidence_root": str(args.evidence_root), "cases": len(cases)}, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
