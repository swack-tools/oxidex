#!/usr/bin/env python3
"""Verify the complete NikonSettings Rust projection against live Perl facts.

No generator or dump JSON is used. A strict reader consumes the emitted Rust
DSL and compares its meaning with freshly loaded NikonSettings::Main. This is
declaration verification, not proof of the bespoke directory walker. Unknown
rows are excluded for normal native extraction. The existing AFAreaMode state
assignment is deliberately reported as unpropagated by the Rust interpreter.
The existing BracketProgram Mask is reported separately: the native custom
processor does not apply it, while the Rust interpreter does. PASS describes
this bounded declaration projection, not runtime equivalence for those rows.
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
from rust_source import lexical_source  # noqa: E402

PERL_FACTS = r'''
use strict; use warnings;
use B (); use Cwd qw(abs_path); use JSON::PP;
use Image::ExifTool; use Image::ExifTool::NikonSettings;
my $table = \%Image::ExifTool::NikonSettings::Main;
my %meta = map { $_ => 1 } qw(PROCESS_PROC GROUPS NOTES);
for my $k (keys %$table) {
    die "unmodeled table metadata $k\n" unless $meta{$k} or $k =~ /^(0|[1-9][0-9]*)$/;
}
my $proc = $table->{PROCESS_PROC};
die "missing process procedure\n" unless ref($proc) eq 'CODE';
my $gv = B::svref_2object($proc)->GV;
my $proc_name = $gv->STASH->NAME . '::' . $gv->NAME;
die "unmodeled process procedure\n" unless $proc_name eq 'Image::ExifTool::NikonSettings::ProcessNikonSettings';
my $groups = $table->{GROUPS};
die "unmodeled groups\n" unless ref($groups) eq 'HASH' and keys(%$groups) == 2
    and defined($groups->{0}) and $groups->{0} eq 'MakerNotes'
    and defined($groups->{2}) and $groups->{2} eq 'Camera';
my %allowed = map { $_ => 1 } qw(Name Condition Mask Notes PrintConv RawConv ValueConv Unknown PrintConvInv ValueConvInv);
my @rows;
for my $id (sort grep { !$meta{$_} } keys %$table) {
    die "tag id outside u16\n" if $id > 65535;
    my $entry = $table->{$id};
    my @variants = ref($entry) eq 'ARRAY' ? @$entry : ($entry);
    die "empty variants\n" unless @variants;
    my %unknown_states;
    for my $r (@variants) {
        die "non-hash row $id\n" unless ref($r) eq 'HASH';
        $unknown_states{$r->{Unknown} ? 1 : 0} = 1;
    }
    die "mixed Unknown/normal variants cannot preserve native veto $id\n" if keys(%unknown_states) > 1;
    my $ordinal = 0;
    for my $row (@variants) {
        die "non-hash row $id\n" unless ref($row) eq 'HASH';
        my %copy;
        for my $key (keys %$row) {
            die "unmodeled row field $id/$key\n" unless $allowed{$key};
            my $value = $row->{$key};
            if ($key eq 'PrintConv' and ref($value) eq 'HASH') {
                my %map;
                for my $k (keys %$value) {
                    die "unmodeled PrintConv key $id/$k\n" unless $k =~ /^(0|[1-9][0-9]*)$/ and $k <= 4294967295;
                    die "unmodeled PrintConv value $id/$k\n" unless defined($value->{$k}) and !ref($value->{$k});
                    $map{$k} = "$value->{$k}";
                }
                die "empty PrintConv map $id\n" unless keys %map;
                $copy{$key} = \%map;
            } else {
                die "unmodeled value $id/$key\n" unless defined($value) and !ref($value);
                $copy{$key} = "$value";
            }
        }
        die "empty name $id\n" unless defined($copy{Name}) and length($copy{Name});
        die "unmodeled Unknown $id\n" if exists($copy{Unknown}) and $copy{Unknown} ne '0' and $copy{Unknown} ne '1';
        die "unmodeled Mask $id\n" if exists($copy{Mask}) and ($copy{Mask} !~ /^(0|[1-9][0-9]*)$/ or $copy{Mask} > 4294967295);
        push @rows, { id => 0+$id, variant => $ordinal++, facts => \%copy };
    }
}
die "empty NikonSettings table\n" unless @rows;
print JSON::PP->new->canonical->utf8->encode({
    version => $Image::ExifTool::VERSION,
    core => abs_path($INC{'Image/ExifTool.pm'}),
    settings => abs_path($INC{'Image/ExifTool/NikonSettings.pm'}), rows => \@rows,
});
'''


class VerificationError(ValueError):
    pass


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def native_facts(source, perl):
    source = Path(source).resolve()
    paths = [source / 'lib/Image/ExifTool.pm', source / 'lib/Image/ExifTool/NikonSettings.pm']
    if any(not p.is_file() for p in paths):
        raise VerificationError('selected source lacks the core or NikonSettings module')
    executable = shutil.which(str(perl))
    if not executable:
        raise VerificationError('selected Perl is unavailable')
    executable = Path(executable).resolve()
    paths.append(executable)
    hashes = {str(p.resolve()): sha256(p) for p in paths}
    env = {k: v for k, v in os.environ.items() if not k.startswith('PERL')}
    result = subprocess.run([str(executable), '-I', str(source / 'lib'), '-e', PERL_FACTS],
                            capture_output=True, text=True, encoding='utf-8', env=env, timeout=60)
    if result.returncode:
        raise VerificationError('native NikonSettings oracle failed: ' + result.stderr.strip())
    try:
        doc = json.loads(result.stdout)
    except ValueError as exc:
        raise VerificationError('native oracle returned malformed JSON') from exc
    if not isinstance(doc, dict) or doc.get('version') != (ROOT / '.exiftool-version').read_text().strip():
        raise VerificationError('native source version does not match the repository pin')
    if doc.get('core') != str(paths[0].resolve()) or doc.get('settings') != str(paths[1].resolve()):
        raise VerificationError('loaded modules do not belong to the selected source')
    if any(sha256(p) != h for p, h in hashes.items()):
        raise VerificationError('source or interpreter changed during the native probe')
    doc['identity'] = {'source': str(source), 'perl': str(executable), 'sha256': hashes}
    return doc


class Reader:
    """Read only this file's small constructor language, consuming all tokens."""
    def __init__(self, text):
        code, strings = lexical_source(text)
        # Validate trivia the shared lexical mask deliberately erases. Incomplete
        # comments and standalone character literals are outside this DSL.
        at = 0
        while at < len(text):
            if at in strings:
                at += len(strings[at])
            elif text.startswith('//', at):
                end = text.find('\n', at)
                at = len(text) if end < 0 else end
            elif text.startswith('/*', at):
                depth = 1
                at += 2
                while depth and at < len(text):
                    if text.startswith('/*', at):
                        depth += 1
                        at += 2
                    elif text.startswith('*/', at):
                        depth -= 1
                        at += 2
                    else:
                        at += 1
                if depth:
                    raise VerificationError('unterminated Rust comment')
            elif code[at].isspace() and not text[at].isspace():
                raise VerificationError('unsupported erased Rust literal')
            else:
                at += 1
        self.tokens = []
        pos = 0
        token = re.compile(r'\s+|@|0[xX][0-9a-fA-F]+|[0-9]+|[A-Za-z_][A-Za-z_0-9]*|::|[^\s]')
        for match in token.finditer(code):
            if match.start() != pos:
                raise VerificationError('unreadable Rust token')
            pos = match.end()
            value = match.group()
            if value.isspace():
                continue
            if value == '@':
                raw = strings.get(match.start(), '')
                # These are the exact string forms used by this producer. New
                # Rust escapes are refused, never decoded with Python semantics.
                if not re.fullmatch(r'"(?:[^"\\\x00-\x1f]|\\["\\nrt])*"', raw):
                    raise VerificationError('unsupported Rust string literal')
                value = ('string', json.loads(raw))
            self.tokens.append(value)
        self.i = 0

    def take(self):
        if self.i == len(self.tokens):
            raise VerificationError('unexpected end of Rust output')
        token = self.tokens[self.i]
        self.i += 1
        return token

    def expect(self, *tokens):
        for token in tokens:
            found = self.take()
            if found != token:
                raise VerificationError(f'unrecognized Rust shape: expected {token!r}, got {found!r}')

    def peek(self, token):
        return self.i < len(self.tokens) and self.tokens[self.i] == token

    def integer(self, maximum=4294967295, signed=False):
        negative = signed and self.peek('-')
        if negative:
            self.take()
        token = self.take()
        if not isinstance(token, str) or not re.fullmatch(r'(?:0[xX][0-9a-fA-F]+|[0-9]+)', token):
            raise VerificationError('expected integer literal')
        value = int(token, 16 if token.lower().startswith('0x') else 10)
        if value > maximum:
            raise VerificationError('Rust integer outside declared domain')
        return -value if negative else value

    def string(self):
        token = self.take()
        if not isinstance(token, tuple) or token[0] != 'string':
            raise VerificationError('expected string literal')
        return token[1]

    def enum(self, typ):
        self.expect(typ, '::')
        name = self.take()
        if not isinstance(name, str) or not re.fullmatch(r'[A-Za-z][A-Za-z0-9]*', name):
            raise VerificationError('invalid enum variant')
        args = []
        if self.peek('('):
            self.take()
            while not self.peek(')'):
                args.append(self.take() if name == 'Map' else self.integer(2**63-1, signed=True))
                if not self.peek(','):
                    break
                self.take()
            self.expect(')')
        return name, tuple(args)


def rust_facts(text):
    r = Reader(text)
    r.expect('use', 'super', '::', 'settings', '::', '{', 'Cond', ',', 'Conv', ',', 'Dm', ',',
             'SettingsTag', 'as', 'E', '}', ';')
    maps, rows = {}, None
    while r.i < len(r.tokens):
        if r.peek('#'):
            r.expect('#', '[', 'rustfmt', '::', 'skip', ']')
            if r.i == len(r.tokens):
                raise VerificationError('Rust attribute without a declaration')
            continue
        if r.peek('const'):
            r.take()
            name = r.take()
            if not isinstance(name, str) or not re.fullmatch(r'PC_[0-9]+', name) or name in maps:
                raise VerificationError('invalid/duplicate Rust map declaration')
            r.expect(':', '&', '[', '(', 'u32', ',', '&', 'str', ')', ']', '=', '&', '[')
            pairs = {}
            while not r.peek(']'):
                r.expect('(')
                key = r.integer()
                r.expect(',')
                value = r.string()
                r.expect(')')
                if key in pairs:
                    raise VerificationError('duplicate Rust map key')
                pairs[key] = value
                if not r.peek(','):
                    break
                r.take()
            r.expect(']', ';')
            if not pairs:
                raise VerificationError('empty Rust map')
            maps[name] = pairs
        else:
            if rows is not None:
                raise VerificationError('extra Rust declarations after SETTINGS_TAGS')
            r.expect('pub', '(', 'super', ')', 'const', 'SETTINGS_TAGS', ':', '&', '[', 'E', ']', '=', '&', '[')
            rows = []
            while not r.peek(']'):
                r.expect('E', '{', 'id', ':')
                tag_id = r.integer(65535)
                r.expect(',', 'name', ':')
                name = r.string()
                r.expect(',', 'cond', ':')
                cond = r.enum('Cond')
                r.expect(',', 'mask', ':')
                mask = r.integer()
                r.expect(',', 'conv', ':')
                conv = r.enum('Conv')
                r.expect(',', 'dm', ':')
                dm = r.enum('Dm')
                if r.peek(','):
                    r.take()
                r.expect('}')
                rows.append({'id': tag_id, 'Name': name, 'Condition': cond, 'Mask': mask,
                             'Conversion': conv, 'State': dm})
                if not r.peek(','):
                    break
                r.take()
            r.expect(']', ';')
    if not rows:
        raise VerificationError('missing/empty SETTINGS_TAGS')
    used = {row['Conversion'][1][0] for row in rows
            if row['Conversion'][0] == 'Map' and len(row['Conversion'][1]) == 1}
    if used != set(maps):
        raise VerificationError('missing or unreferenced Rust maps')
    return rows, maps


# Reverse contracts describe existing Rust variants, not a forward generator.
# Literal regex/string whitespace stays exact; unfamiliar source refuses.
CONDITIONS = {
    'Always': None,
    'ModelD6': r'$$self{Model} =~ /^NIKON D6\b/i',
    'ModelZ7': r'$$self{Model} =~ /^NIKON Z (7|7_2)\b/i',
    'ModelZSeries': r'$$self{Model} =~ /^NIKON Z (5|50|6|6_2|7|7_2|fc)\b/i',
    'ModelZ6Or7': r'$$self{Model} =~ /^NIKON Z [67]\b/',
    'HdmiBitDepthIs2': '$$self{HDMIBitDepth}  == 2',
    'CmdDialsReverseRotExposureCompIs1': '$$self{CmdDialsReverseRotExposureComp} and $$self{CmdDialsReverseRotExposureComp} == 1',
    'BracketSetLt4': '$$self{BracketSet} < 4',
    'PlaybackFlickUpIs1': '$$self{PlaybackFlickUp} and $$self{PlaybackFlickUp} == 1',
    'PlaybackFlickDownIs1': '$$self{PlaybackFlickDown} and $$self{PlaybackFlickDown} == 1',
}
MEMBERS = {
    'HdmiBitDepth': 'HDMIBitDepth', 'HdmiOutputHdr': 'HDMIOutputHDR',
    'BracketSet': 'BracketSet', 'BracketProgram': 'BracketProgram',
    'PlaybackFlickUp': 'PlaybackFlickUp', 'PlaybackFlickDown': 'PlaybackFlickDown',
    'CmdDialsReverseRotExposureComp': 'CmdDialsReverseRotExposureComp',
    'CmdDialsChangeMainSubExposure': 'CmdDialsChangeMainSubExposure',
}


def condition_contract(variant):
    name, args = variant
    if any(not isinstance(arg, int) or not 0 <= arg <= 4294967295 for arg in args):
        raise VerificationError('condition parameter outside u32')
    if name in CONDITIONS and not args:
        return CONDITIONS[name]
    if len(args) == 1:
        n = args[0]
        if name == 'CmdDialsChangeMainSubExposureIs':
            return f'$$self{{CmdDialsChangeMainSubExposure}} and $$self{{CmdDialsChangeMainSubExposure}} == {n}'
        if name == 'BracketSetIs':
            return f'$$self{{BracketSet}} and $$self{{BracketSet}} == {n}'
        if name == 'BracketSetLt4AndProgramNe':
            return f'$$self{{BracketSet}} < 4 and $$self{{BracketProgram}} ne {n}'
    if name == 'BracketSetEqAndProgramNe' and len(args) == 2:
        return f'$$self{{BracketSet}} == {args[0]} and $$self{{BracketProgram}} ne {args[1]}'
    raise VerificationError(f'unknown Rust condition {variant}')


def known_native_condition(value):
    if value in CONDITIONS.values():
        return True
    templates = [
        ('CmdDialsChangeMainSubExposureIs', 1), ('BracketSetIs', 1),
        ('BracketSetLt4AndProgramNe', 1), ('BracketSetEqAndProgramNe', 2),
    ]
    for name, arity in templates:
        # Only replace distinct numeric argument markers in an exact reverse
        # contract. Regex literals and every other source character stay exact.
        markers = tuple(range(987654320, 987654320 + arity))
        pattern = re.escape(condition_contract((name, markers)))
        for marker in markers:
            pattern = pattern.replace(str(marker), r'(0|[1-9][0-9]*)')
        found = re.fullmatch(pattern, value or '')
        if found and all(int(n) <= 4294967295 for n in found.groups()):
            return True
    return False


def conversion_matches(variant, maps, facts):
    name, args = variant
    vc, pc = facts.get('ValueConv'), facts.get('PrintConv')
    if name == 'Map' and len(args) == 1:
        return vc is None and isinstance(pc, dict) and maps.get(args[0]) == {int(k): v for k, v in pc.items()}
    if name == 'Raw' and not args:
        return vc is None and pc is None
    if name == 'FineTune' and not args:
        return vc == '($val - 7) / 6' and pc == '$val ? sprintf("%+.2f", $val) : 0'
    if len(args) == 1:
        n = args[0]
        if name == 'Fps':
            return vc == f'{n} - $val' and pc == '"$val fps"'
        if name == 'Negate':
            return vc == f'{n} - $val' and pc is None
        if name == 'Offset':
            expressions = {f'$val + {n}', f'$val{n:+d}'}
            if n < 0:
                expressions.add(f'$val - {-n}')
            return (vc in expressions and pc is None) or (vc is None and isinstance(pc, str) and pc in expressions)
    return False


def verify_text(text, native):
    rows, maps = rust_facts(text)
    for entry in native['rows']:
        if (entry['facts'].get('Unknown') == '1'
                and not known_native_condition(entry['facts'].get('Condition'))):
            raise VerificationError(f"unmodeled condition on omitted Unknown tag {entry['id']}")
    expected = [r for r in native['rows'] if r['facts'].get('Unknown') != '1']
    if len(rows) != len(expected):
        raise VerificationError(f'row population differs: Rust={len(rows)} native normal={len(expected)}')
    residual, masks = [], []
    for row, entry in zip(rows, expected):
        facts = entry['facts']
        context = f"tag {entry['id']} variant {entry['variant']} ({facts['Name']})"
        if row['id'] != entry['id'] or row['Name'] != facts['Name']:
            raise VerificationError(f'identity/variant order differs at {context}')
        if condition_contract(row['Condition']) != facts.get('Condition') or row['Mask'] != int(facts.get('Mask', '0')):
            raise VerificationError(f'condition or mask differs at {context}')
        if row['Mask']:
            # ProcessNikonSettings calls HandleTag, not ProcessBinaryData's
            # masking path. Keep the one existing projected declaration visible
            # as runtime debt; never certify additional masks as native behavior.
            if not (row['id'] == 266 and row['Name'] == 'BracketProgram'
                    and row['Condition'] == ('BracketSetIs', (5,)) and row['Mask'] == 15
                    and row['State'] == ('BracketProgram', ())
                    and facts.get('RawConv') == '$$self{BracketProgram} = $val'):
                raise VerificationError(f'new mask is not a native NikonSettings read operation at {context}')
            masks.append({'id': 266, 'name': 'BracketProgram', 'mask': 15})
        if not conversion_matches(row['Conversion'], maps, facts):
            raise VerificationError(f'conversion differs at {context}')
        member, args = row['State']
        raw = facts.get('RawConv')
        if args:
            raise VerificationError(f'unmodeled state parameters at {context}')
        if member == 'None' and raw is None:
            continue
        if member in MEMBERS and raw == '$$self{' + MEMBERS[member] + '} = $val':
            continue
        if (member == 'None' and row['id'] == 366 and row['Name'] == 'AFAreaMode'
                and raw == '$$self{AFAreaMode} = $val'):
            residual.append({'id': 366, 'name': 'AFAreaMode', 'member': 'AFAreaMode'})
            continue
        raise VerificationError(f'state assignment differs at {context}')
    return {'status': 'PASS', 'rows': len(rows), 'maps': len(maps),
            'unknown_omitted': len(native['rows']) - len(expected), 'unpropagated_state': residual,
            'runtime_mask_residual': masks}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exiftool-dir', required=True, type=Path)
    parser.add_argument('--perl', required=True, help='exact native Perl interpreter')
    parser.add_argument('--input', required=True, type=Path, help='complete emitted settings_tables.rs')
    args = parser.parse_args()
    print('=== instrument: verify_nikon_settings.py ===')
    try:
        before = args.input.read_bytes()
        native = native_facts(args.exiftool_dir, args.perl)
        result = verify_text(before.decode('utf-8'), native)
        if args.input.read_bytes() != before:
            raise VerificationError('input changed during verification')
        result['input_sha256'] = hashlib.sha256(before).hexdigest()
        result['identity'] = native['identity']
        result['instrument'] = 'verify_nikon_settings.py'
        result['version'] = native['version']
        print(json.dumps(result, sort_keys=True))
        return 0
    except (VerificationError, OSError, ValueError, subprocess.SubprocessError) as exc:
        print(f'Nikon settings verification refused: {exc}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main())
