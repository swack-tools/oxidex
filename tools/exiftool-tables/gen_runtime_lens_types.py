#!/usr/bin/env python3
"""Regenerate Canon/Pentax MakerNote base lens arrays from selected ExifTool."""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from gen_xmp_lens_maps import LENS_DATA, ROOT, rust_string, selected_source

FIXTURE = ROOT / 'tools/exiftool-tables/fixtures/canon_lens_types.json'


def base_rows(data):
    makers = {maker['make']: maker for maker in data['makers']}
    canon, pentax = [], []
    for key, label in makers['Canon']['rows']:
        if '.' in key:
            continue
        if not re.fullmatch(r'(?:0|-?[1-9]\d*)', key) or not -1 <= int(key) <= 65535:
            raise ValueError(f'unrepresentable Canon lens id: {key}')
        canon.append((int(key), label))
    for key, label in makers['Pentax']['rows']:
        if '.' in key:
            continue
        match = re.fullmatch(r'(0|[1-9]\d*) (0|[1-9]\d*)', key)
        if not match or int(match[1]) > 255 or int(match[2]) > 65535:
            raise ValueError(f'unrepresentable Pentax lens id: {key}')
        pentax.append((int(match[1]), int(match[2]), label))
    canon.sort(key=lambda row: row[0])
    pentax.sort(key=lambda row: row[:2])
    if (not canon or not pentax or len(set(k for k, _ in canon)) != len(canon)
            or len(set(row[:2] for row in pentax)) != len(pentax)):
        raise ValueError('empty or duplicate base lens registry')
    if dict(canon).get(-1) != 'n/a' or dict(canon).get(65535) != 'n/a':
        raise ValueError('Canon n/a sentinels changed; inspect runtime semantics')
    if len(canon) != makers['Canon']['base_count'] or len(pentax) != makers['Pentax']['base_count']:
        raise ValueError('exporter base counts disagree with rows')
    return canon, pentax, makers['Canon']['fractional_count']


def array_match(text, name, shape):
    pattern = (r'pub static ' + name + r': \[' + re.escape(shape)
               + r'; (\d+)\] = \[\n(.*?)\n    \];')
    matches = list(re.finditer(pattern, text, re.S))
    if len(matches) != 1:
        raise ValueError(f'expected exactly one {name} array')
    return matches[0]


def replace_array(text, name, shape, rows):
    match = array_match(text, name, shape)
    rendered = f'pub static {name}: [{shape}; {len(rows)}] = [\n'
    rendered += ''.join(f'        {row},\n' for row in rows)
    rendered += '    ];'
    return text[:match.start()] + rendered + text[match.end():]


def parsed_arrays(text):
    canon = array_match(text, 'CANON_LENS_TYPES', '(i64, &str)')
    pentax = array_match(text, 'PENTAX_LENS_TYPES', '(u8, u16, &str)')
    c = [(int(k), json.loads(v)) for k, v in re.findall(
        r'\(\s*(-?\d+)\s*,\s*("(?:\\.|[^"\\])*")\s*\)', canon[2])]
    p = [(int(a), int(b), json.loads(v)) for a, b, v in re.findall(
        r'\(\s*(\d+)\s*,\s*(\d+)\s*,\s*("(?:\\.|[^"\\])*")\s*,?\s*\)', pentax[2])]
    if len(c) != int(canon[1]) or len(p) != int(pentax[1]):
        raise ValueError('runtime lens array declaration/rows disagree')
    return c, sorted(p)


def render(data, text):
    canon, pentax, fractional = base_rows(data)
    result = replace_array(text, 'CANON_LENS_TYPES', '(i64, &str)',
                           [f'({key}, {rust_string(label)})' for key, label in canon])
    result = replace_array(result, 'PENTAX_LENS_TYPES', '(u8, u16, &str)',
                           [f'({a}, {b}, {rust_string(label)})' for a, b, label in pentax])
    fixture = json.dumps({'entries': canon, 'fractional_key_count_not_covered': fractional},
                         ensure_ascii=False, indent=1) + '\n'
    return (canon, pentax), result, fixture


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exiftool-dir', type=Path, required=True)
    parser.add_argument('--perl', default='perl')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    data = selected_source(args.perl, args.exiftool_dir.resolve())
    current = LENS_DATA.read_text()
    expected, result, fixture = render(data, current)
    same_arrays = parsed_arrays(current) == (expected[0], sorted(expected[1]))
    same_fixture = FIXTURE.is_file() and FIXTURE.read_text() == fixture
    if args.check:
        if not same_arrays or not same_fixture:
            raise ValueError('runtime lens arrays or Canon fixture drifted from selected source')
    else:
        if not same_arrays:
            LENS_DATA.write_text(result)
        if not same_fixture:
            FIXTURE.write_text(fixture)
    print(f'runtime lenses current: Canon={len(expected[0])}, Pentax={len(expected[1])}')


if __name__ == '__main__':
    try:
        main()
    except ValueError as exc:
        raise SystemExit(str(exc)) from exc
