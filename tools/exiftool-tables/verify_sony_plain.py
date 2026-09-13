#!/usr/bin/env python3
"""Independently verify the complete Sony plain Rust declaration projection.

Fresh native Perl hashes are the oracle; no generator or dump is imported.
PASS covers this finite DSL, its ordered declarations and exact RAW_TAG_IDS
sidecar, not the interpreter or container walk. Distinct native raw keys remain
distinct even at equal byte indices. PrintInt is documentation metadata, checked
natively even though the general dump does not retain its value.
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass
from decimal import Decimal
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

NAMES = ('CameraSettings', 'CameraSettings2', 'CameraSettings3', 'FaceInfo1', 'FaceInfo2', 'ShotInfo')
RAW_KEY = re.compile(r'(0|[1-9][0-9]*)(?:\.([0-9]*[1-9]))?')
PERL_FACTS = r'''
use strict; use warnings; use B (); use B::Deparse (); use Cwd qw(abs_path); use JSON::PP; use Config ();
use Image::ExifTool; use Image::ExifTool::Sony;
sub copy_fact {
    my ($v)=@_;
    die "undefined native fact\n" unless defined($v);
    return "$v" unless ref($v);
    return [map {copy_fact($_)} @$v] if ref($v) eq 'ARRAY';
    return {map {$_=>copy_fact($v->{$_})} keys %$v} if ref($v) eq 'HASH';
    if (ref($v) eq 'CODE') {
        my $g=B::svref_2object($v)->GV;
        my %code=(code_name=>$g->STASH->NAME.'::'.$g->NAME);
        $code{body}=B::Deparse->new->coderef2text($v) if $g->NAME eq '__ANON__';
        return \%code;
    }
    die "unmodeled native reference ".ref($v)."\n";
}
my %tables;
{ no strict 'refs';
  for my $name (qw(CameraSettings CameraSettings2 CameraSettings3 FaceInfo1 FaceInfo2 ShotInfo)) {
    $tables{$name}=copy_fact(\%{'Image::ExifTool::Sony::'.$name});
  }
}
my %loaded = map {$_=>abs_path($INC{$_})}
    grep {$_ eq 'Image/ExifTool.pm' or m{^Image/ExifTool/}} keys %INC;
print JSON::PP->new->canonical->utf8->encode({version=>$Image::ExifTool::VERSION,
    tables=>\%tables, loaded=>\%loaded,
    numeric=>{nvtype=>$Config::Config{nvtype}, nvsize=>0+$Config::Config{nvsize}}});
'''


class VerificationError(ValueError):
    pass


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def native_facts(source, perl):
    source = Path(source).resolve()
    lib = source / 'lib'
    required = ('Image/ExifTool.pm', 'Image/ExifTool/Sony.pm')
    if any(not (lib / name).is_file() for name in required):
        raise VerificationError('selected source lacks core or Sony module')
    found = shutil.which(str(perl))
    if not found:
        raise VerificationError('selected Perl is unavailable')
    executable = Path(found).resolve()
    # Snapshot before loading, including shared enum suppliers such as Minolta.
    before = {str(p.resolve()): digest(p) for p in lib.rglob('*.pm') if p.is_file()}
    before[str(executable)] = digest(executable)
    env = {k: v for k, v in os.environ.items() if not k.startswith('PERL')}
    p = subprocess.run([str(executable), '-I', str(lib), '-e', PERL_FACTS],
                       capture_output=True, text=True, encoding='utf-8', env=env, timeout=60)
    if p.returncode:
        raise VerificationError('native oracle failed: ' + p.stderr.strip())
    doc = json.loads(p.stdout)
    if not isinstance(doc, dict) or doc.get('version') != (ROOT / '.exiftool-version').read_text().strip():
        raise VerificationError('native version does not match repository pin')
    if doc.get('numeric') != {'nvtype': 'double', 'nvsize': 8}:
        raise VerificationError('native numeric configuration is not the reviewed binary64 model')
    loaded = doc.get('loaded')
    if not isinstance(loaded, dict) or any(name not in loaded for name in required):
        raise VerificationError('missing loaded-module identities')
    selected = {str(executable): before[str(executable)]}
    for name, path in loaded.items():
        expected = (lib / name).resolve()
        if not expected.is_relative_to(lib.resolve()) or path != str(expected) or path not in before:
            raise VerificationError('loaded module does not belong to selected source: ' + name)
        selected[path] = before[path]
    if any(digest(path) != sha for path, sha in selected.items()):
        raise VerificationError('native source or interpreter changed during probe')
    doc['identity'] = {'source': str(source), 'perl': str(executable), 'sha256': selected,
                       'numeric': doc['numeric']}
    return doc


@dataclass(frozen=True)
class Node:
    name: str
    args: tuple = ()


class Number(Decimal):
    def __new__(cls, value, kind):
        result = super().__new__(cls, value)
        result.kind = kind
        return result


class Reader:
    """Consume the whole small Rust DSL, including all declaration tails."""
    def __init__(self, text):
        code, strings = lexical_source(text)
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
                        depth += 1; at += 2
                    elif text.startswith('*/', at):
                        depth -= 1; at += 2
                    else:
                        at += 1
                if depth:
                    raise VerificationError('unterminated Rust comment')
            elif code[at].isspace() and not text[at].isspace():
                raise VerificationError('unsupported erased Rust literal')
            else:
                at += 1
        self.tokens = []
        for m in re.finditer(r'\s+|@|0x[0-9a-fA-F]+(?:u32)?|[0-9]+(?:\.[0-9]+)?(?:_f64|u32)?|[A-Za-z_][A-Za-z_0-9]*|::|[^\s]', code):
            token = m.group()
            if token.isspace():
                continue
            if token == '@':
                raw = strings.get(m.start(), '')
                if re.fullmatch(r'r"[^"\x00-\x1f]*"', raw):
                    token = ('string', raw[2:-1])
                elif re.fullmatch(r'"(?:[^"\\\x00-\x1f]|\\["\\nrt])*"', raw):
                    token = ('string', json.loads(raw))
                else:
                    raise VerificationError('unsupported Rust string literal')
            self.tokens.append(token)
        self.i = 0

    def peek(self, token):
        return self.i < len(self.tokens) and self.tokens[self.i] == token

    def take(self):
        if self.i == len(self.tokens):
            raise VerificationError('unexpected end of Rust output')
        value = self.tokens[self.i]; self.i += 1
        return value

    def expect(self, *tokens):
        for expected in tokens:
            actual = self.take()
            if actual != expected:
                raise VerificationError(f'Rust shape: expected {expected!r}, got {actual!r}')

    def expression(self):
        if self.peek('&'):
            self.take()
            if not self.peek('['):
                raise VerificationError('only literal array references are supported')
        if self.peek('[') or self.peek('('):
            closing = ']' if self.take() == '[' else ')'
            values = []
            while not self.peek(closing):
                values.append(self.expression())
                if not self.peek(','):
                    break
                self.take()
            self.expect(closing)
            return values
        negative = self.peek('-')
        if negative:
            self.take()
        token = self.take()
        if isinstance(token, tuple):
            if negative:
                raise VerificationError('negative string')
            return token[1]
        if re.fullmatch(r'(?:0x[0-9a-fA-F]+|[0-9]+(?:\.[0-9]+)?)(?:_f64|u32)?', token):
            value = re.sub(r'(?:_f64|u32)$', '', token)
            number = Decimal(int(value, 16)) if value.startswith('0x') else Decimal(value)
            kind = 'float' if '.' in token or token.endswith('_f64') else 'integer'
            return Number(-number if negative else number, kind)
        if negative or not re.fullmatch(r'[A-Za-z_][A-Za-z_0-9]*', token):
            raise VerificationError('unsupported Rust expression')
        if token in ('true', 'false'):
            return token == 'true'
        if self.peek('::'):
            self.take(); token += '::' + self.take()
        if self.peek('('):
            return Node(token, tuple(self.expression()))
        if self.peek('{'):
            self.take(); fields = {}
            while not self.peek('}'):
                key = self.take(); self.expect(':')
                if not isinstance(key, str) or key in fields:
                    raise VerificationError('duplicate or unreadable Rust field')
                fields[key] = self.expression()
                if not self.peek(','):
                    break
                self.take()
            self.expect('}')
            return token, fields
        return Node(token)


def rust_facts(text):
    r = Reader(text)
    r.expect('use', 'super', '::', 'binary_data', '::', '{')
    for i, name in enumerate(('BinTable', 'BinTag', 'Cond', 'Dm', 'Fmt', 'Hook', 'NumCmp', 'Other', 'Pc', 'Raw', 'Vc')):
        if i:
            r.expect(',')
        r.expect(name)
    r.expect('}', ';')
    declarations, types, indices = {}, {}, None
    while r.i < len(r.tokens):
        if r.peek('#'):
            r.expect('#', '[')
            if r.peek('rustfmt'):
                r.expect('rustfmt', '::', 'skip', ']')
            else:
                r.expect('allow', '(', 'dead_code', ')', ']')
            if r.i == len(r.tokens):
                raise VerificationError('orphan Rust attribute')
            continue
        public = r.peek('pub')
        if public:
            r.take()
        if r.peek('mod'):
            if not public or indices is not None:
                raise VerificationError('unmodeled or duplicate Rust module')
            r.expect('mod', 'idx', '{'); indices = {}
            while not r.peek('}'):
                r.expect('pub', 'const'); name = r.take(); r.expect(':', 'usize', '=')
                if name in indices:
                    raise VerificationError('duplicate index constant')
                indices[name] = r.expression(); r.expect(';')
            r.expect('}')
            continue
        r.expect('static'); name = r.take(); r.expect(':', '&', '[')
        typ = []
        depth = 0
        while not (r.peek(']') and depth == 0):
            token = r.take()
            if token == '[':
                depth += 1
            elif token == ']':
                depth -= 1
            typ.append(token)
        r.expect(']', '=')
        if name in declarations:
            raise VerificationError('duplicate Rust declaration')
        expected_type = {'M': ['(', '&', 'str', ',', '&', 'str', ')'],
                         'B': ['(', 'u32', ',', '&', 'str', ')'], 'T': ['BinTag']}
        if name in ('TABLES', 'RAW_TAG_IDS'):
            wanted = ['BinTable'] if name == 'TABLES' else ['&', '[', '&', 'str', ']']
            if not public:
                raise VerificationError(name + ' must remain public')
        elif isinstance(name, str) and re.fullmatch(r'[MBT][0-9]+', name) and not public:
            wanted = expected_type[name[0]]
        else:
            raise VerificationError('unmodeled Rust declaration name')
        if typ != wanted:
            raise VerificationError('Rust declaration type differs')
        declarations[name] = r.expression(); types[name] = typ; r.expect(';')
    if indices is None or any(uint(value, 32) != i for i, value in enumerate(indices.values())) or indices != {name.upper(): Decimal(i) for i, name in enumerate(NAMES)}:
        raise VerificationError('public table index constants differ')
    tables = declarations.get('TABLES')
    if not isinstance(tables, list) or len(tables) != len(NAMES):
        raise VerificationError('missing or extra Rust tables')
    raw_ids = declarations.get('RAW_TAG_IDS')
    if not isinstance(raw_ids, list) or len(raw_ids) != len(NAMES):
        raise VerificationError('missing or extra RAW_TAG_IDS tables')
    used = {'TABLES', 'RAW_TAG_IDS'}

    def resolve(value):
        if isinstance(value, Node):
            if value.name in declarations:
                if value.args:
                    raise VerificationError('called a Rust static')
                used.add(value.name)
                return declarations[value.name]
            return Node(value.name, tuple(resolve(a) for a in value.args))
        if isinstance(value, list):
            return [resolve(v) for v in value]
        return value

    output = []
    for expected_name, table, ids in zip(NAMES, tables, raw_ids):
        if not isinstance(table, tuple) or table[0] != 'BinTable' or set(table[1]) != {'name', 'fmt', 'tags'}:
            raise VerificationError('unreadable BinTable')
        fields = table[1]
        if fields['name'] != expected_name:
            raise VerificationError('table ordering/name differs')
        if not isinstance(fields['tags'], Node) or not re.fullmatch('T[0-9]+', fields['tags'].name):
            raise VerificationError('table does not reference a tag array')
        rows = resolve(fields['tags'])
        if not isinstance(rows, list) or not rows:
            raise VerificationError('empty Rust tag table')
        if not isinstance(ids, list) or len(ids) != len(rows):
            raise VerificationError('RAW_TAG_IDS row alignment differs')
        for key in ids:
            raw_key(key)
        checked = []
        row_fields = {'index', 'name', 'cond', 'fmt', 'count', 'mask', 'raw', 'vc', 'pc', 'hook', 'print_hex', 'low_priority', 'subdir'}
        for row in rows:
            if not isinstance(row, tuple) or row[0] != 'BinTag' or set(row[1]) != row_fields:
                raise VerificationError('unreadable BinTag')
            checked.append({k: resolve(v) for k, v in row[1].items()})
        output.append({'name': expected_name, 'fmt': fields['fmt'], 'rows': checked, 'raw_ids': ids})
    if used != set(declarations):
        raise VerificationError('unused or unresolved Rust declarations')
    for name, values in declarations.items():
        if name.startswith(('M', 'B')) and name != 'TABLES':
            if not isinstance(values, list) or not values:
                raise VerificationError('empty Rust map')
            keys = []
            for pair in values:
                if not isinstance(pair, list) or len(pair) != 2 or not isinstance(pair[1], str):
                    raise VerificationError('unreadable Rust map pair')
                key = pair[0]
                if name.startswith('M') and not isinstance(key, str):
                    raise VerificationError('non-string map key')
                if name.startswith('B'):
                    uint(key, 32)
                keys.append(key)
            if len(keys) != len(set(keys)):
                raise VerificationError('duplicate Rust map key')
    return output, sum(name.startswith('M') for name in declarations), sum(name.startswith('B') for name in declarations)


def uint(value, bits):
    if isinstance(value, bool) or not isinstance(value, (Decimal, str)):
        raise VerificationError('non-numeric unsigned field')
    if isinstance(value, Number) and value.kind != 'integer':
        raise VerificationError('floating-point literal in unsigned field')
    if isinstance(value, str) and not re.fullmatch(r'0|[1-9][0-9]*', value):
        raise VerificationError('invalid unsigned native field')
    n = Decimal(value)
    if n != n.to_integral_value() or not 0 <= n < 2 ** bits:
        raise VerificationError('unsigned field outside domain')
    return int(n)


def scalar(value):
    if not isinstance(value, str):
        raise VerificationError('non-scalar native metadata')
    value.encode('utf-8')
    return value


def raw_key(value):
    if not isinstance(value, str) or not (match := RAW_KEY.fullmatch(value)):
        raise VerificationError('noncanonical native raw key')
    integer = uint(match[1], 32)
    exact = Decimal(value)
    # ProcessBinaryData uses native numeric comparison and int(index). Refuse
    # keys for which binary64 would select a different physical integer offset.
    if int(float(exact)) != integer:
        raise VerificationError('raw key crosses integer boundary under binary64')
    return exact


def ordered_native_keys(keys):
    by_float = {}
    exact = {}
    for key in keys:
        exact[key] = raw_key(key)
        number = float(exact[key])
        if number in by_float and by_float[number] != key:
            raise VerificationError('distinct raw keys collide under binary64 comparison')
        by_float[number] = key
    return sorted(keys, key=exact.__getitem__)


def perl_tokens(text):
    """Ignore code spacing only; strings, regex bodies and word boundaries stay exact."""
    if text is None:
        return None
    scalar(text)
    tokens = []; at = 0
    while at < len(text):
        if text[at].isspace():
            at += 1; continue
        quote = text[at]
        regex = quote == '/' and tokens[-1:] in [['=~'], ['!~']]
        if quote in ('"', "'") or regex:
            end = at + 1
            while end < len(text):
                if text[end] == '\\':
                    end += 2; continue
                if text[end] == quote:
                    break
                end += 1
            if end >= len(text):
                raise VerificationError('unterminated native literal')
            end += 1
            if regex:
                while end < len(text) and text[end].isalpha():
                    end += 1
            tokens.append(text[at:end]); at = end; continue
        m = re.match(r'0x[0-9a-fA-F]+|[0-9]+(?:\.[0-9]+)?|[A-Za-z_][A-Za-z_0-9]*|=~|!~|==|!=|<=|>=|<<|>>|\*\*|::|->|.', text[at:])
        token = m.group(); at += len(token)
        if re.fullmatch(r'0x[0-9a-fA-F]+|[0-9]+(?:\.[0-9]+)?', token):
            # Perl interprets leading-zero integers as octal (and rejects 8/9),
            # unlike Decimal. No such source form is in this audited subset.
            if re.match(r'0[0-9]', token):
                raise VerificationError('unreviewed leading-zero native numeric literal')
            token = Decimal(int(token, 16)) if token.startswith('0x') else Decimal(token)
        tokens.append(token)
    return tokens


def same_expression(native, expected):
    return perl_tokens(native) == perl_tokens(expected)


FORMATS = {'Default': None, 'U8': 'int8u', 'I8': 'int8s', 'U16': 'int16u',
           'U32': 'int32u', 'U16Rev': 'int16uRev', 'Str': 'string'}
MEMBERS = {'FaceInfoLength', 'FaceInfoOffset', 'FacesDetected', 'LensMount', 'MetaVersion'}


def member(node):
    if not isinstance(node, Node) or node.args or node.name not in {'Dm::' + x for x in MEMBERS}:
        raise VerificationError('unsupported data member')
    return node.name[4:]


def float_number(value):
    if not isinstance(value, Number) or value.kind != 'float' or not value.is_finite():
        raise VerificationError('expected a finite Rust floating-point literal')
    return Decimal(value)


def cond_contract(node):
    if not isinstance(node, Node):
        raise VerificationError('invalid Rust condition')
    name, a = node.name, node.args
    if name == 'Cond::Always' and not a:
        return None
    if name == 'Cond::ModelRe' and len(a) == 2 and type(a[0]) is bool and isinstance(a[1], str):
        if a[1] not in ('^NEX-', '^DSLR-(A450|A500|A550)$', '^DSLR-(A450|A500|A550)', '^(NEX-|DSLR-(A450|A500|A550)$)'):
            raise VerificationError('unaudited model regex')
        return '$$self{Model} ' + ('!~' if a[0] else '=~') + ' /' + a[1] + '/'
    if name == 'Cond::DmTruthy' and len(a) == 1:
        return '$$self{' + member(a[0]) + '}'
    if name == 'Cond::DmCmp' and len(a) == 3 and a[1] in (Node('NumCmp::Eq'), Node('NumCmp::Ne')):
        n = uint(float_number(a[2]), 32)
        return '$$self{' + member(a[0]) + '} ' + ('==' if a[1].name.endswith('Eq') else '!=') + f' {n}'
    if name == 'Cond::All' and len(a) == 1 and isinstance(a[0], list) and a[0]:
        return ' and '.join(cond_contract(part) for part in a[0])
    raise VerificationError('unmodeled Rust condition variant')


def condition_matches(node, native):
    expected = cond_contract(node)
    if same_expression(native, expected):
        return True
    # The audited NEX source adds redundant parentheses around these atoms.
    if expected == '$$self{Model} =~ /^NEX-/':
        return same_expression(native, '(' + expected + ')')
    if expected == '$$self{Model} =~ /^NEX-/ and $$self{LensMount} != 1':
        return same_expression(native, '($$self{Model} =~ /^NEX-/) and ($$self{LensMount} != 1)')
    return False


def raw_contract(node):
    if node == Node('Raw::None'):
        return None
    if isinstance(node, Node) and node.name == 'Raw::Store' and len(node.args) == 1:
        return '$$self{' + member(node.args[0]) + '} = $val'
    if isinstance(node, Node) and node.name == 'Raw::DropIfDmLess' and len(node.args) == 2:
        return '$$self{' + member(node.args[0]) + f'}} < {uint(float_number(node.args[1]), 32)} ? undef : $val'
    raise VerificationError('unmodeled Rust RawConv')


def vc_contract(node):
    if not isinstance(node, Node):
        raise VerificationError('invalid ValueConv')
    name, a = node.name, node.args
    noargs = {'Vc::None': None, 'Vc::Signed8Above128': '$val > 128 ? $val - 256 : $val',
              'Vc::IsoExp': '$val ? exp(($val/8-6)*log(2))*100 : $val',
              'Vc::IsoExpBelow254': '($val and $val < 254) ? exp(($val/8-6)*log(2))*100 : $val'}
    if name in noargs and not a:
        return noargs[name]
    a = tuple(float_number(x) for x in a)
    if name == 'Vc::Add' and len(a) == 1:
        return f'$val {"-" if a[0] < 0 else "+"} {abs(a[0])}'
    if name == 'Vc::Mul' and len(a) == 1:
        return f'$val * {a[0]}'
    if name == 'Vc::SubDiv' and len(a) == 2 and a[1]:
        return f'($val - {a[0]}) / {a[1]}'
    if name == 'Vc::ExpTime' and len(a) == 2 and a[1]:
        return f'$val ? 2 ** ({a[0]} - $val/{a[1]}) : 0'
    if name == 'Vc::Pow2DivSubHalf' and len(a) == 2 and a[0]:
        return f'2 ** (($val/{a[0]} - {a[1]}) / 2)'
    raise VerificationError('unmodeled Rust ValueConv')


def other_matches(node, native):
    if node == Node('Other::None'):
        return native is None
    if not isinstance(native, dict) or set(native) != {'code_name', 'body'} or native['code_name'] != 'Image::ExifTool::Sony::__ANON__':
        return False
    tokens = perl_tokens(native['body'])
    prefix = perl_tokens('{ package Image::ExifTool::Sony;')
    if tokens[:len(prefix)] != prefix or tokens[-1:] != ['}']:
        return False
    tokens = tokens[len(prefix):-1]
    pragmas = set()
    while tokens[:1] == ['use'] and len(tokens) >= 3 and tokens[2] == ';' and tokens[1] in ('strict', 'warnings'):
        if tokens[1] in pragmas:
            return False
        pragmas.add(tokens[1]); tokens = tokens[3:]
    if 'strict' not in pragmas:
        return False
    contracts = {
        Node('Other::Identity'): ['shift();'],
        Node('Other::RoundHalfUp'): [
            'my($val, $inv) = @_; return int $val + 0.5 unless $inv; return &Image::ExifTool::IsFloat($val) ? $val : undef;',
            'my($val, $inv) = @_; return int $val + 0.5 unless $inv; return Image::ExifTool::IsFloat($val) ? $val : undef;',
        ],
    }
    return any(tokens == perl_tokens(contract) for contract in contracts.get(node, []))


def pc_matches(node, native):
    if not isinstance(node, Node):
        raise VerificationError('invalid PrintConv')
    name, a = node.name, node.args
    if name in ('Pc::Map', 'Pc::Bitmask'):
        if not isinstance(native, dict):
            return False
        allowed = {'Notes', 'BITMASK', 'BitsPerWord', 'OTHER'}
        if 'Notes' in native:
            scalar(native['Notes'])
        normal = {k: scalar(v) for k, v in native.items() if k not in allowed}
        if any(not re.fullmatch(r'-?[0-9]+(?:\.[0-9]+)?', k) for k in normal):
            raise VerificationError('unmodeled PrintConv map key')
        if name == 'Pc::Map' and len(a) == 2 and other_matches(a[1], native.get('OTHER')) and 'BITMASK' not in native and 'BitsPerWord' not in native:
            return a[0] == [[k, normal[k]] for k in sorted(normal)]
        if name == 'Pc::Bitmask' and len(a) == 4 and other_matches(a[3], native.get('OTHER')) and isinstance(native.get('BITMASK'), dict):
            bits = {uint(k, 32): scalar(v) for k, v in native['BITMASK'].items()}
            return (a[0] == [[k, normal[k]] for k in sorted(normal)]
                    and a[1] == [[Decimal(k), bits[k]] for k in sorted(bits)]
                    and uint(a[2], 32) == uint(native.get('BitsPerWord', '32'), 32))
        return False
    simple = {'Pc::None': None, 'Pc::ExposureTimeOrBulb': '$val ? Image::ExifTool::Exif::PrintExposureTime($val) : "Bulb"',
              'Pc::FNumber': 'Image::ExifTool::Exif::PrintFNumber($val)',
              'Pc::Fixed0OrAuto': '$val ? sprintf("%.0f",$val) : "Auto"',
              'Pc::Signed1OrZero': '$val ? sprintf("%+.1f",$val) : 0',
              'Pc::PlusOrVal': '$val > 0 ? "+$val" : $val',
              'Pc::HexDotHex': 'sprintf("%x.%.2x",$val>>8,$val&0xff)',
              'Pc::VerHex': 'sprintf("Ver.%.2x.%.3d",$val>>8,$val&0xff)',
              'Pc::DateTime': '$self->ConvertDateTime($val)'}
    if name in simple and not a:
        return same_expression(native, simple[name])
    if name == 'Pc::Suffix' and len(a) == 1 and a[0] in (' K', '%'):
        return same_expression(native, '"$val' + a[0] + '"')
    if name == 'Pc::ZeroPad' and len(a) == 1 and uint(a[0], 32) in (3, 4):
        return same_expression(native, f'sprintf("%.{int(a[0])}d",$val)')
    raise VerificationError('unmodeled Rust PrintConv')


def verify_text(text, native):
    tables, maps, bitmaps = rust_facts(text)
    if not isinstance(native.get('tables'), dict) or set(native['tables']) != set(NAMES):
        raise VerificationError('native table scope differs')
    count = 0; shared = []; documentation = []
    meta_fields = {'PROCESS_PROC', 'CHECK_PROC', 'WRITE_PROC', 'WRITABLE', 'FIRST_ENTRY', 'FORMAT', 'GROUPS', 'NOTES', 'PRIORITY', 'DATAMEMBER', 'IS_SUBDIR'}
    row_fields = {'Name', 'Condition', 'Format', 'Mask', 'RawConv', 'ValueConv', 'PrintConv', 'SubDirectory', 'DataMember', 'PrintHex',
                  'Notes', 'Description', 'PrintConvInv', 'ValueConvInv', 'PrintConvColumns', 'SeparateTable', 'Writable', 'Groups', 'Shift', 'PrintInt'}
    for emitted in tables:
        name = emitted['name']; table = native['tables'][name]
        if not isinstance(table, dict) or not table:
            raise VerificationError('empty native table ' + name)
        meta = {k: v for k, v in table.items() if not re.fullmatch(r'(0|[1-9][0-9]*)(?:\.[0-9]+)?', k)}
        if set(meta) - meta_fields:
            raise VerificationError('unmodeled table metadata ' + name)
        for field, proc in [('PROCESS_PROC', 'ProcessBinaryData'), ('CHECK_PROC', 'CheckBinaryData'), ('WRITE_PROC', 'WriteBinaryData')]:
            if meta.get(field) != {'code_name': 'Image::ExifTool::' + proc}:
                raise VerificationError('native processing contract differs: ' + name + '/' + field)
        if meta.get('FIRST_ENTRY') != '0' or meta.get('WRITABLE') != '1':
            raise VerificationError('unmodeled table layout/write metadata')
        expected_group = 'Camera' if name.startswith('CameraSettings') else 'Image'
        if meta.get('GROUPS') != {'0': 'MakerNotes', '2': expected_group}:
            raise VerificationError('table groups differ')
        if meta.get('PRIORITY') not in (None, '0'):
            raise VerificationError('unmodeled table priority')
        fmt = emitted['fmt']
        if not isinstance(fmt, Node) or fmt.args or not fmt.name.startswith('Fmt::') or fmt.name[5:] not in FORMATS or meta.get('FORMAT') != FORMATS[fmt.name[5:]]:
            raise VerificationError('table FORMAT differs')
        rows = []
        for key in ordered_native_keys(set(table) - set(meta)):
            variants = table[key] if isinstance(table[key], list) else [table[key]]
            if not variants:
                raise VerificationError('empty native variants')
            for variant in variants:
                row = {'Name': variant} if isinstance(variant, str) else variant
                if not isinstance(row, dict) or set(row) - row_fields:
                    raise VerificationError('unmodeled native row fields ' + name + '/' + key)
                if not scalar(row.get('Name')):
                    raise VerificationError('empty tag name')
                for field in set(row) - {'PrintConv', 'SubDirectory', 'Groups'}:
                    scalar(row[field])
                rows.append((key, row))
        if not rows or len(rows) != len(emitted['rows']):
            raise VerificationError('row population differs ' + name)
        if emitted['raw_ids'] != [key for key, _ in rows]:
            raise VerificationError('exact raw key identity or variant repetition differs ' + name)
        if meta.get('DATAMEMBER', []) != sorted({key for key, row in rows if 'DataMember' in row}, key=Decimal):
            raise VerificationError('table DATAMEMBER differs from rows')
        if meta.get('IS_SUBDIR', []) != sorted({key for key, row in rows if 'SubDirectory' in row}, key=Decimal):
            raise VerificationError('table IS_SUBDIR differs from rows')
        by_index = {}
        for key, _ in rows:
            keys = by_index.setdefault(int(Decimal(key)), [])
            if key not in keys:
                keys.append(key)
        shared.extend({'table': name, 'index': index, 'raw_keys': keys}
                      for index, keys in by_index.items() if len(keys) > 1)
        for (key, row), rust in zip(rows, emitted['rows']):
            context = name + '/' + key + '/' + row['Name']
            if uint(rust['index'], 32) != int(Decimal(key)) or rust['name'] != row['Name']:
                raise VerificationError('ordered identity differs ' + context)
            native_fmt = row.get('Format')
            fm = re.fullmatch(r'(int8u|int8s|int16u|int32u|int16uRev|string)(?:\[([1-9][0-9]*)\])?', native_fmt or '')
            expected_fmt, expected_count = (fm[1], int(fm[2] or '1')) if fm else (None, 1)
            f = rust['fmt']
            if (native_fmt is not None and fm is None) or not isinstance(f, Node) or f.args or not f.name.startswith('Fmt::') or FORMATS.get(f.name[5:], 'invalid') != expected_fmt:
                raise VerificationError('field format differs ' + context)
            if uint(rust['count'], 32) != expected_count or uint(rust['mask'], 32) != uint(row.get('Mask', '0'), 32):
                raise VerificationError('count/mask differs ' + context)
            if not condition_matches(rust['cond'], row.get('Condition')):
                raise VerificationError('condition differs ' + context)
            if not same_expression(row.get('RawConv'), raw_contract(rust['raw'])) or not same_expression(row.get('ValueConv'), vc_contract(rust['vc'])) or not pc_matches(rust['pc'], row.get('PrintConv')):
                raise VerificationError('conversion differs ' + context)
            if rust['hook'] != Node('Hook::None') or type(rust['print_hex']) is not bool or type(rust['low_priority']) is not bool:
                raise VerificationError('unmodeled hook or flag type')
            if row.get('PrintHex') not in (None, '0', '1') or rust['print_hex'] != (row.get('PrintHex') == '1') or rust['low_priority'] != (meta.get('PRIORITY') == '0'):
                raise VerificationError('output priority/hex flag differs ' + context)
            if 'DataMember' in row and raw_contract(rust['raw']) != '$$self{' + row['DataMember'] + '} = $val':
                raise VerificationError('data member declaration differs ' + context)
            sub = row.get('SubDirectory')
            if isinstance(rust['subdir'], Node) and rust['subdir'].name == 'Some':
                if len(rust['subdir'].args) != 1:
                    raise VerificationError('unmodeled subdirectory parameters')
                uint(rust['subdir'].args[0], 32)
            if sub is None:
                if rust['subdir'] != Node('None'):
                    raise VerificationError('unexpected Rust subdirectory')
            elif not isinstance(sub, dict) or set(sub) != {'TagTable'} or sub['TagTable'] not in {'Image::ExifTool::Sony::FaceInfo1', 'Image::ExifTool::Sony::FaceInfo2'} or rust['subdir'] != Node('Some', (Decimal(NAMES.index(sub['TagTable'].split('::')[-1])),)):
                raise VerificationError('subdirectory edge differs ' + context)
            if 'PrintInt' in row:
                if (name, key, row['Name'], native_fmt, row['PrintInt']) != ('CameraSettings3', '1015', 'LensType2', 'int16u', '1'):
                    raise VerificationError('unmodeled PrintInt documentation fact')
                documentation.append({'table': name, 'raw_key': key, 'field': 'PrintInt', 'value': 1})
            if 'Groups' in row and (name, key, row['Name'], row['Groups']) != ('ShotInfo', '6', 'SonyDateTime', {'2': 'Time'}):
                raise VerificationError('unmodeled field group')
            if 'Shift' in row and (name, key, row['Name'], row['Shift']) != ('ShotInfo', '6', 'SonyDateTime', 'Time'):
                raise VerificationError('unmodeled time shift metadata')
            count += 1
    return {'instrument': 'verify_sony_plain.py', 'status': 'PASS', 'scope': 'declaration projection; not runtime equivalence',
            'version': native['version'], 'tables': len(tables), 'rows': count, 'maps': maps, 'bitmaps': bitmaps,
            'raw_tag_id_tables': len(tables), 'raw_tag_id_rows': count, 'shared_byte_indices': shared,
            'native_documentation_facts': documentation}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input', required=True, type=Path)
    p.add_argument('--exiftool-dir', required=True, type=Path)
    p.add_argument('--perl', required=True, help='exact native Perl interpreter')
    args = p.parse_args()
    print('=== instrument: verify_sony_plain.py ===')
    try:
        before = args.input.read_bytes()
        native = native_facts(args.exiftool_dir, args.perl)
        report = verify_text(before.decode('utf-8'), native)
        if args.input.read_bytes() != before:
            raise VerificationError('input changed during verification')
        report['input_sha256'] = hashlib.sha256(before).hexdigest()
        report['identity'] = native['identity']
        print(json.dumps(report, sort_keys=True))
        return 0
    except (VerificationError, OSError, ValueError, TypeError, subprocess.SubprocessError) as exc:
        print(f'Sony plain verification refused: {exc}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
