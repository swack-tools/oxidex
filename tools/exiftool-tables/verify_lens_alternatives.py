#!/usr/bin/env python3
"""Compare complete Rust lens-alternative facts with independent native Perl.

The oracle dumps every selected source-table entry, not producer output. This
also checks whether labels preserve the upstream raw-ID distinction. A matching
projection can still FAIL because the runtime string key loses that distinction.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
ORACLE = r'''
use strict; use warnings; use B (); use Cwd qw(abs_path); use JSON::PP;
my $root=abs_path(shift @ARGV); die "no source tree\n" unless defined $root;
my @modules=qw(Image/ExifTool.pm Image/ExifTool/Canon.pm Image/ExifTool/Pentax.pm Image/ExifTool/Olympus.pm Image/ExifTool/Panasonic.pm);
for my $m (@modules) { die "selected source missing $m\n" unless -f "$root/lib/$m"; }
unshift @INC,"$root/lib";
for my $m (@modules) { require $m; die "foreign module $m\n" unless abs_path($INC{$m}) eq abs_path("$root/lib/$m"); }
no warnings 'once';
my %tables=(canon=>\%Image::ExifTool::Canon::canonLensTypes,canon_rf=>$Image::ExifTool::Canon::FileInfo{61}{PrintConv},pentax=>\%Image::ExifTool::Pentax::pentaxLensTypes,olympus=>$Image::ExifTool::Olympus::Equipment{0x0201}{PrintConv});
my %facts;
for my $name (keys %tables) {
    my $h=$tables{$name}; die "$name is not a hash\n" unless ref($h) eq 'HASH';
    for my $key (keys %$h) {
        my $v=$h->{$key};
        my $type=!defined($v)?'undef':ref($v)?ref($v):(B::svref_2object(\$v)->FLAGS & B::SVp_POK())?'string':'nonstring';
        my $row={type=>$type,truthy=>$v?JSON::PP::true:JSON::PP::false};
        if ($type eq 'string') {
            if (utf8::is_utf8($v)) { $row->{unicode}=$v; }
            else { $row->{bytes_hex}=unpack('H*',$v); }
        }
        $facts{$name}{$key}=$row;
    }
}
print JSON::PP->new->utf8->canonical->encode({version=>$Image::ExifTool::VERSION,tables=>\%facts});
'''
STRING = r'"(?:\\.|[^"\\])*"'
TOKEN = re.compile(STRING + r'|//[^\n]*')
DECL = re.compile(r'pub\s+static\s+(CANON_RF|CANON|PENTAX)_LENS_ALTERNATIVES\s*:\s*\[\((i64,\s*)?&str,\s*&\[&str\]\);\s*(\d+)\]\s*=\s*\[(.*?)\];', re.S)
ROW = re.compile(r'\(\s*(?:(-?\d+)\s*,\s*)?(' + STRING + r')\s*,\s*&\[\s*((?:' + STRING + r'\s*,?\s*)*)\]\s*,?\s*\)\s*,?', re.S)


def rust_string(token):
    out, i = [], 1
    escapes = {'"':'"', '\\':'\\', 'n':'\n', 'r':'\r', 't':'\t', '0':'\0'}
    while i < len(token)-1:
        char = token[i]
        i += 1
        if char != '\\':
            out.append(char)
            continue
        char = token[i]
        i += 1
        if char in escapes:
            out.append(escapes[char])
        elif char == 'u' and token[i] == '{':
            end = token.index('}', i)
            value = int(token[i+1:end], 16)
            if 0xD800 <= value <= 0xDFFF:
                raise ValueError('invalid Rust Unicode scalar')
            out.append(chr(value))
            i = end+1
        else:
            raise ValueError('unsupported Rust string escape')
    return ''.join(out)


def read_rust(text):
    clean = TOKEN.sub(lambda m: '' if m[0].startswith('//') else m[0], text)
    result = {}
    pos = 0
    for match in DECL.finditer(clean):
        if clean[pos:match.start()].strip():
            raise ValueError('unexpected content outside generated tables')
        family = match[1].lower()
        if family in result:
            raise ValueError('duplicate table declaration')
        if bool(match[2]) != (family in ('canon', 'canon_rf')):
            raise ValueError('incorrect table key type')
        rows, cursor = {}, 0
        for row in ROW.finditer(match[4]):
            if match[4][cursor:row.start()].strip():
                raise ValueError('unparsed table content')
            label = rust_string(row[2])
            if bool(row[1]) != (family in ('canon', 'canon_rf')):
                raise ValueError('incorrect row key type')
            key = row[1] if family in ('canon', 'canon_rf') else label
            if key in rows:
                raise ValueError('duplicate Rust key')
            alts = [rust_string(m[0]) for m in re.finditer(STRING, row[3])]
            rows[key] = {'label':label, 'alternatives':alts} if family in ('canon', 'canon_rf') else alts
            cursor = row.end()
        if match[4][cursor:].strip() or len(rows) != int(match[3]):
            raise ValueError('unparsed rows or incorrect array length')
        result[family] = rows
        pos = match.end()
    if clean[pos:].strip() or set(result) != {'canon', 'canon_rf', 'pentax'}:
        raise ValueError('expected exactly three complete alternative tables')
    return result


def text_value(row):
    if row['type'] != 'string' or not row['truthy']:
        raise ValueError('expected truthy string scalar')
    if 'unicode' in row:
        return row['unicode']
    data = bytes.fromhex(row['bytes_hex'])
    try:
        return data.decode('utf-8')
    except UnicodeDecodeError:
        return data.decode('latin-1')


def compare(facts, rust):
    errors, counts, expected = [], {}, {}
    for family, entries in facts['tables'].items():
        bases, chains, labels = {}, {}, {}
        id_pattern = r'(?:0|-?[1-9]\d*)' if family in ('canon', 'canon_rf') else r'-?\d+(?: \d+)*'
        for key, value in entries.items():
            if family in ('pentax', 'olympus') and key == 'Notes':
                try:
                    text_value(value)
                except (ValueError, KeyError) as exc:
                    errors.append(f'{family} Notes: {exc}')
                continue
            if family == 'pentax' and key == 'OTHER':
                if value['type'] != 'CODE':
                    errors.append('pentax OTHER is not CODE')
                continue
            fractional = re.fullmatch('('+id_pattern+r')\.([1-9]\d*)', key)
            try:
                text = text_value(value)
            except (ValueError, KeyError) as exc:
                errors.append(f'{family} {key}: {exc}')
                continue
            if fractional:
                chains.setdefault(fractional[1], {})[int(fractional[2])] = text
            elif re.fullmatch(id_pattern, key):
                if family in ('canon', 'canon_rf') and not -(2**63) <= int(key) < 2**63:
                    errors.append('canon: raw ID outside i64 range')
                bases[key] = text
                labels.setdefault(text, []).append(key)
            else:
                errors.append(f'{family}: unsupported key {key}')
        counts[family] = {'source_entries':len(entries), 'base_entries':len(bases),
                          'ambiguous_ids':len(chains), 'alternatives':sum(map(len, chains.values()))}
        if not bases or (family not in ('olympus', 'canon_rf') and not chains):
            errors.append(f'{family}: vacuous source population')
        projected = {}
        for raw_id, chain in chains.items():
            if raw_id not in bases:
                errors.append(f'{family}: orphan {raw_id}')
                continue
            if sorted(chain) != list(range(1, len(chain)+1)):
                errors.append(f'{family}: noncontiguous {raw_id}')
            label = bases[raw_id]
            if family not in ('canon', 'canon_rf') and len(labels[label]) != 1:
                errors.append(f'{family}: ambiguous label {label!r} belongs to raw IDs {sorted(labels[label])}')
            projected[label] = [chain[n] for n in sorted(chain)]
        if family in ('canon', 'canon_rf'):
            projected = {raw_id:{'label':label,'alternatives':[chains[raw_id][n] for n in sorted(chains.get(raw_id, {}))]}
                         for raw_id,label in bases.items()}
        if family == 'canon_rf' and chains:
            errors.append('canon_rf: runtime assumes no alternatives')
        if family == 'olympus':
            if chains:
                errors.append('olympus: runtime assumes no alternatives')
        else:
            expected[family] = projected
    for family in ('canon', 'canon_rf', 'pentax'):
        if rust.get(family) != expected.get(family):
            errors.append(f'{family}: complete Rust alternatives differ from native Perl projection')
    return {'errors':errors, 'counts':counts, 'projection_matches':rust == expected}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('rust_file', type=Path)
    parser.add_argument('--exiftool-dir', type=Path, required=True)
    parser.add_argument('--perl', required=True)
    parser.add_argument('--json-out', type=Path)
    args = parser.parse_args()
    report = {'instrument':'verify_lens_alternatives.py', 'rust_file':str(args.rust_file),
              'source':str(args.exiftool_dir.resolve()), 'perl':args.perl, 'status':'FAIL'}
    try:
        for name in ('PERL5OPT', 'PERL5LIB', 'PERLLIB'):
            if os.environ.get(name):
                raise ValueError('refusing ambient '+name)
        result = subprocess.run([args.perl, '-e', ORACLE, str(args.exiftool_dir)],
                                capture_output=True, text=True, timeout=60)
        if result.returncode:
            raise ValueError('native oracle failed: '+result.stderr.strip())
        facts = json.loads(result.stdout)
        pin = (ROOT/'.exiftool-version').read_text().strip()
        if facts['version'] != pin:
            raise ValueError(f"source version {facts['version']!r} disagrees with pin {pin!r}")
        source = args.rust_file.read_text()
        if not source.startswith('//!') or f'`.exiftool-version`, {pin}' not in source:
            raise ValueError('expected complete file header with selected pin')
        report.update(compare(facts, read_rust(source)))
        report.update(version=pin, source_facts=facts,
                      rust_sha256=hashlib.sha256(args.rust_file.read_bytes()).hexdigest())
        if not report['errors']:
            report['status'] = 'PASS'
    except (ValueError, OSError, subprocess.SubprocessError, KeyError) as exc:
        report['errors'] = [str(exc)]
    if args.json_out:
        args.json_out.write_text(json.dumps(report, indent=2, ensure_ascii=False)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k != 'source_facts'}, indent=2))
    return 0 if report['status'] == 'PASS' else 1


if __name__ == '__main__':
    sys.exit(main())
