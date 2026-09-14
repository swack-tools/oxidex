#!/usr/bin/env python3
"""Capture and validate the native catalog; no reading/writing claims."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import instrument


def validate(document: dict, pin: str) -> None:
    if document.get('schema') != 'oxidex_hydrated_catalog_universe_v1':
        raise ValueError('unsupported catalog schema')
    if document.get('exiftool_version') != pin:
        raise ValueError('catalog version differs from repository pin')
    counts = document['counts']
    tables = document['families']['hydrated_tables']
    table_names = [t['full_name'] for t in tables]
    if len(set(table_names)) != len(table_names) or len(tables) != counts['hydrated_tables']:
        raise ValueError('table identities are duplicated or unaccounted')
    table_set = set(table_names)
    entries = document['entries']
    if len(entries) != counts['catalog_total_tag_entries']:
        raise ValueError('catalog entries do not conserve the native denominator')
    names, identities = set(), set()
    for entry in entries + document['container_rows_outside_total']:
        identity = (entry['table'], entry['raw_key'], entry['variant_index'])
        if identity in identities:
            raise ValueError('duplicate catalog row identity')
        identities.add(identity)
        if entry['table'] not in table_set:
            raise ValueError('catalog row has no table identity')
        if not isinstance(entry['raw_key'], str) or type(entry['variant_index']) is not int or entry['variant_index'] < 0:
            raise ValueError('invalid source key or variant identity')
        # ExifTool public names are ASCII; refusing a new alphabet keeps the
        # validator from silently substituting Python Unicode casing for Perl.
        name = entry['name']
        if not isinstance(name, str) or not name or not name.isascii() or entry['normalized_name'] != name.lower():
            raise ValueError('invalid case-insensitive public name identity')
        if set(entry['groups']) != {'0', '1', '2'} or any(not isinstance(g, str) for g in entry['groups'].values()):
            raise ValueError('catalog row is missing group identity')
    for entry in entries:
        names.add(entry['normalized_name'])
    if sorted(names) != document['unique_names'] or len(names) != counts['distinct_case_insensitive_entry_names']:
        raise ValueError('unique public names are duplicated or unaccounted')
    if len(document['container_rows_outside_total']) != counts['catalog_container_rows_outside_total']:
        raise ValueError('container rows are unaccounted')
    if type(counts['catalog_unique_tag_names']) is not int or counts['catalog_unique_tag_names'] < 0:
        raise ValueError('native unique-name counter is invalid')
    sources = document['producer']['sources']
    if 'Image/ExifTool/BuildTagLookup.pm' not in sources or 'Image/ExifTool.pm' not in sources:
        raise ValueError('native source provenance is missing')


def source_fingerprint(library: Path) -> dict[str, str]:
    return {str(path.relative_to(library)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted((library / 'Image').rglob('*'))
            if path.is_file() and path.suffix in {'.pm', '.pl'}}


def capture(source: Path, perl: str, root: Path = ROOT) -> dict:
    library = source / 'lib' if (source / 'lib').is_dir() else source
    producer = root / 'tools/exiftool-tables/dump_hydrated_catalog.pl'
    env = {k: v for k, v in os.environ.items() if k not in {'PERL5LIB', 'PERLLIB', 'PERL5OPT'}}
    before = source_fingerprint(library)
    run = subprocess.run([perl, str(producer), '--repo-root', str(root), str(library)],
                         check=True, capture_output=True, text=True, env=env)
    if before != source_fingerprint(library):
        raise ValueError('pinned source changed during native capture')
    document = json.loads(run.stdout)
    document['producer']['script_sha256'] = hashlib.sha256(producer.read_bytes()).hexdigest()
    validate(document, (root / '.exiftool-version').read_text().strip())
    return document


def render_counts(document: dict) -> str:
    counts = document['counts']
    rows = [
        ('Native table-entry denominator', 'catalog_total_tag_entries', 'Non-hidden tag variants excluding subdirectory navigation rows, shortcuts, and plug-ins'),
        ('Native “unique tag names” counter', 'catalog_unique_tag_names', "ExifTool's own counter, retained without alteration"),
        ('Distinct case-insensitive entry names', 'distinct_case_insensitive_entry_names', 'Distinct Perl `lc(Name)` identities over the enumerated entries'),
        ('Hydrated table identities', 'hydrated_tables', 'Fully qualified native table names'),
        ('Container/navigation rows outside the entry denominator', 'catalog_container_rows_outside_total', 'Preserved separately, not counted as additional ordinary tags'),
        ('Shortcut helper entries', 'shortcut_entries', 'Macro helpers, not fabricated file layouts'),
    ]
    return '\n'.join(['| Measurement | Count | Meaning |', '| --- | ---: | --- |'] +
                     [f'| {label} | {counts[key]:,} | {meaning} |' for label, key, meaning in rows])


def rendered_report(document: dict, template: str) -> str:
    begin, end = '<!-- catalog-counts:start -->', '<!-- catalog-counts:end -->'
    if template.count(begin) != 1 or template.count(end) != 1:
        raise ValueError('report must have exactly one generated-counts region')
    prefix, rest = template.split(begin)
    _, suffix = rest.split(end)
    return prefix + begin + '\n' + render_counts(document) + '\n' + end + suffix


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exiftool-dir', type=Path, required=True)
    parser.add_argument('--perl', default=os.environ.get('EXIFTOOL_PERL', 'perl'))
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--report', type=Path, default=ROOT / 'docs/reference/catalog-baseline.md')
    action = parser.add_mutually_exclusive_group()
    action.add_argument('--check', action='store_true')
    action.add_argument('--replace', action='store_true')
    args = parser.parse_args()
    state = instrument.git_state(ROOT)
    overridden = instrument.refuse_if_dirty(state, 'catalog_snapshot.py')
    output = args.output.resolve()
    source = args.exiftool_dir.resolve()
    protected = {Path(__file__).resolve(), (HERE / 'dump_hydrated_catalog.pl').resolve(),
                 (ROOT / '.exiftool-version').resolve(), args.report.resolve()}
    if output == source or source in output.parents or output in protected:
        raise ValueError('output aliases a source input')
    if output.exists() and not (args.check or args.replace):
        raise ValueError('output exists; use --check or --replace')
    print('=== instrument: catalog_snapshot.py ===', file=sys.stderr)
    print(f'commit: {state.commit}; dirty_override: {overridden}', file=sys.stderr)
    print(f'scope: native catalog only; no oxidex binary, fixtures or write operations', file=sys.stderr)
    print(f'pin: {(ROOT / ".exiftool-version").read_text().strip()}; source: {source}; perl: {args.perl}', file=sys.stderr)
    document = capture(source, args.perl)
    rendered = json.dumps(document, sort_keys=True, ensure_ascii=True, separators=(',', ':')) + '\n'
    report_before = args.report.read_text()
    report_after = rendered_report(document, report_before)
    if args.check:
        if report_before != report_after:
            raise ValueError('report counts are stale; regenerate from the pinned source')
        if output.read_text() != rendered:
            raise ValueError('catalog snapshot is stale; regenerate from the pinned source')
    else:
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(rendered)
        if report_before != report_after:
            args.report.write_text(report_after)
    print(json.dumps(document['counts'], sort_keys=True))
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as exc:
        raise SystemExit(f'catalog snapshot refused: {exc}')
