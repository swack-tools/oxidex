#!/usr/bin/env python3
"""Compare every emitted DICOM dictionary fact with the selected live Perl hashes.

The oracle loads ExifTool's tables, never the generator's parsed data. The Rust
reader interprets the emitted constructors and rows without importing code from
the generator. This verifies table facts, not the DICOM file-walking runtime.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))
from rust_source import lexical_source
PERL_FACTS = r'''
use strict;
use warnings;
use Cwd qw(abs_path);
use JSON::PP;
use Image::ExifTool;
use Image::ExifTool::DICOM;
my (%main, %uid);
for my $key (keys %Image::ExifTool::DICOM::Main) {
    next if $key eq 'GROUPS' or $key eq 'VARS' or $key eq 'NOTES';
    die "unmodeled Main key '$key'\n" unless $key =~ /^[0-9A-Fa-fx]{4},[0-9A-Fa-fx]{4}$/;
    my $row = $Image::ExifTool::DICOM::Main{$key};
    if (!ref $row) {
        die "invalid bare name '$key'\n" unless defined $row and length $row;
        $main{$key} = [$row, undef, 0, 0];
        next;
    }
    die "non-hash Main row '$key'\n" unless ref($row) eq 'HASH';
    for my $field (keys %$row) {
        die "unmodeled Main field '$key/$field'\n"
            unless $field eq 'Name' or $field eq 'VR' or $field eq 'Binary' or $field eq 'PrintConv';
    }
    die "invalid name '$key'\n" unless defined($row->{Name}) and !ref($row->{Name}) and length($row->{Name});
    die "invalid VR '$key'\n" unless defined($row->{VR}) and !ref($row->{VR}) and $row->{VR} =~ /^[A-Z]{2}$/;
    my ($binary, $pc) = (0, 0);
    if (exists $row->{Binary}) {
        die "unmodeled Binary '$key'\n" unless !ref($row->{Binary}) and $row->{Binary} eq '1';
        $binary = 1;
    }
    if (exists $row->{PrintConv}) {
        my $map = $row->{PrintConv};
        die "unmodeled PrintConv '$key'\n" unless ref($map) eq 'HASH' and keys(%$map) == 2
            and defined($map->{0}) and !ref($map->{0}) and $map->{0} eq 'Unsigned'
            and defined($map->{1}) and !ref($map->{1}) and $map->{1} eq 'Signed';
        $pc = 1;
    }
    $main{$key} = [$row->{Name}, $row->{VR}, $binary, $pc];
}
for my $key (keys %Image::ExifTool::DICOM::uid) {
    my $value = $Image::ExifTool::DICOM::uid{$key};
    die "invalid/falsy UID name '$key'\n" unless defined($value) and !ref($value) and $value;
    $uid{$key} = $value;
}
die "empty DICOM Main or UID dictionary\n" unless keys(%main) and keys(%uid);
print JSON::PP->new->canonical->utf8->encode({
    version => $Image::ExifTool::VERSION,
    core => abs_path($INC{'Image/ExifTool.pm'}),
    dicom => abs_path($INC{'Image/ExifTool/DICOM.pm'}),
    main => \%main, uid => \%uid,
});
'''


class VerificationError(ValueError):
    pass


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def native_facts(exiftool_dir, perl=None, expected_version=None):
    """Load exact selected modules after removing ambient Perl preload paths."""
    source = Path(exiftool_dir).resolve()
    core = source / 'lib/Image/ExifTool.pm'
    dicom = source / 'lib/Image/ExifTool/DICOM.pm'
    for path in (core, dicom):
        if not path.is_file():
            raise VerificationError(f'required selected source is missing: {path}')
    choice = perl or os.environ.get('EXIFTOOL_PERL')
    if not choice:
        choice = 'perl'
    executable = shutil.which(str(choice or ''))
    if not executable:
        raise VerificationError('no usable explicit Perl interpreter')
    executable = str(Path(executable).resolve())
    identities = {str(path.resolve()): sha256(path) for path in (core, dicom, Path(executable))}
    env = {k: v for k, v in os.environ.items() if not k.startswith('PERL')}
    result = subprocess.run([executable, '-I', str(source / 'lib'), '-e', PERL_FACTS],
                            env=env, capture_output=True, text=True, encoding='utf-8')
    if result.returncode:
        raise VerificationError(f'Perl DICOM oracle failed: {result.stderr.strip()}')
    try:
        doc = json.loads(result.stdout)
    except (ValueError, TypeError) as exc:
        raise VerificationError('Perl oracle did not return valid JSON') from exc
    if not isinstance(doc, dict):
        raise VerificationError('Perl oracle JSON is not an object')
    pin = expected_version if expected_version is not None else (ROOT / '.exiftool-version').read_text().strip()
    if not pin or doc.get('version') != pin:
        raise VerificationError(f"source version {doc.get('version')!r} differs from pin {pin!r}")
    if doc.get('core') != str(core.resolve()) or doc.get('dicom') != str(dicom.resolve()):
        raise VerificationError('loaded Perl modules do not belong to the selected source')
    if any(sha256(path) != value for path, value in identities.items()):
        raise VerificationError('selected source or Perl changed during the oracle probe')
    doc['identity'] = {'source': str(source), 'perl': executable, 'sha256': identities}
    return doc


# Only the literal encoding actually emitted by this generator is supported.
# Reject any new constructor/escape/row shape rather than silently skip it.
STRING = r'"(?:[^"\\\x00-\x1f]|\\["\\])*"'
MAIN_ROW = re.compile(r'\s*\((' + STRING + r'),\s*(\w+)\((' + STRING + r')(?:,\s*b"([A-Z]{2})")?\)\),\s*')
UID_ROW = re.compile(r'\s*\((' + STRING + r'),\s*(' + STRING + r')\),\s*')


def rust_facts(text):
    code, _ = lexical_source(text)
    constructors = {}
    constructor_pattern = re.compile(
        r'const fn (\w+)\([^\n]*\) -> DicomDictEntry \{\s*DicomDictEntry \{(.*?)\}\s*\}', re.S)
    for match in constructor_pattern.finditer(code):
        name, body = match.groups()
        body = re.sub(r'\s+', '', body)
        fields = re.fullmatch(r'name,vr:(Some\(\*vr\)|None),binary:(true|false),unsigned_signed:(true|false),', body)
        if not fields or name in constructors:
            raise VerificationError(f'unmodeled/duplicate Rust constructor {name}')
        constructors[name] = (fields[1] != 'None', fields[2] == 'true', fields[3] == 'true')
    declarations = re.findall(r'\bconst\s+fn\s+(\w+)', code)
    if sorted(declarations) != sorted(constructors) or set(constructors) != {'e', 'eb', 'ep', 'bare'}:
        raise VerificationError('missing/unmodeled Rust dictionary constructors')

    def body(name):
        match = re.search(r'pub\(crate\) static ' + name + r':[^\n]*= &\[\n(.*?)\n\];', code, re.S)
        if not match or len(re.findall(r'\bstatic\s+' + name + r'\b', code)) != 1:
            raise VerificationError(f'missing/duplicate Rust array {name}')
        return text[match.start(1):match.end(1)].splitlines()

    main, uid = {}, {}
    previous = None
    for line in body('DICOM_MAIN'):
        match = MAIN_ROW.fullmatch(line)
        if not match:
            raise VerificationError(f'unmodeled Rust Main row: {line!r}')
        key, constructor, name, vr = match.groups()
        key, name = json.loads(key), json.loads(name)
        if previous is not None and key <= previous:
            raise VerificationError('Rust Main keys are duplicated or unsorted')
        previous = key
        if constructor not in constructors:
            raise VerificationError(f'unknown Rust constructor {constructor}')
        has_vr, binary, pc = constructors[constructor]
        if has_vr != (vr is not None):
            raise VerificationError('Rust constructor VR argument disagrees with its body')
        main[key] = [name, vr, int(binary), int(pc)]
    previous = None
    for line in body('DICOM_UID'):
        match = UID_ROW.fullmatch(line)
        if not match:
            raise VerificationError(f'unmodeled Rust UID row: {line!r}')
        key, name = map(json.loads, match.groups())
        if previous is not None and key <= previous:
            raise VerificationError('Rust UID keys are duplicated or unsorted')
        previous = key
        uid[key] = name
    return {'main': main, 'uid': uid}


def compare_facts(actual, expected):
    differences = []
    for table in ('main', 'uid'):
        observed, oracle = actual[table], expected[table]
        for key in sorted(observed.keys() | oracle.keys()):
            if observed.get(key) != oracle.get(key):
                differences.append(f'{table}/{key}: emitted={observed.get(key)!r}, oracle={oracle.get(key)!r}')
    if differences:
        raise VerificationError(f'{len(differences)} DICOM fact mismatches: ' + '; '.join(differences[:10]))


def verify_text(text, oracle):
    stamp = re.search(r'^//! pinned ExifTool ([^ ]+) ', text, re.M)
    if not stamp or stamp[1] != oracle['version']:
        raise VerificationError('generated DICOM source stamp differs from the selected oracle')
    compare_facts(rust_facts(text), oracle)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--exiftool-dir', type=Path, required=True)
    ap.add_argument('--perl', help='exact interpreter; otherwise EXIFTOOL_PERL or PATH Perl')
    ap.add_argument('--input', type=Path, default=ROOT / 'src/parsers/specialized/dicom_dict.rs')
    ap.add_argument('--json-out', type=Path)
    args = ap.parse_args()
    print('=== instrument: verify_dicom_dict.py ===')
    try:
        oracle = native_facts(args.exiftool_dir, args.perl)
        verify_text(args.input.read_text(encoding='utf-8'), oracle)
        report = {'status': 'PASS', 'version': oracle['version'], 'main_rows': len(oracle['main']),
                  'uid_rows': len(oracle['uid']), 'input_sha256': sha256(args.input), 'identity': oracle['identity']}
        if args.json_out:
            args.json_out.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
        print(json.dumps(report, sort_keys=True))
        return 0
    except (OSError, ValueError, subprocess.SubprocessError) as exc:
        print(f'DICOM verification refused: {exc}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
