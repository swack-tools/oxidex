#!/usr/bin/env python3
"""Capture and validate the native catalog; no reading/writing claims."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile
import sys

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import instrument


def validate(document: dict, pin: str) -> None:
    if document.get('schema') != 'oxidex_hydrated_catalog_universe_v2':
        raise ValueError('unsupported catalog schema')
    if document.get('exiftool_version') != pin:
        raise ValueError('catalog version differs from repository pin')
    environment = document.get('capture_environment')
    if (not isinstance(environment, dict)
            or set(environment) != {'perl_version', 'perl_executable_basename'}
            or not re.fullmatch(r'v\d+\.\d+\.\d+', str(environment.get('perl_version', '')))
            or not isinstance(environment.get('perl_executable_basename'), str)
            or not environment['perl_executable_basename']
            or '/' in environment['perl_executable_basename']
            or '\\' in environment['perl_executable_basename']):
        raise ValueError('Perl capture environment is missing or malformed')
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
        validate_native_writable(entry.get('native_writable'))
    classes, states, writable_names = {}, {}, set()
    for entry in entries:
        names.add(entry['normalized_name'])
        fact = entry['native_writable']
        classes[fact['class']] = classes.get(fact['class'], 0) + 1
        states[fact['state']] = states.get(fact['state'], 0) + 1
        if fact['class'] == 'writable':
            writable_names.add(entry['normalized_name'])
    if (counts.get('catalog_native_writable_classes') != classes
            or counts.get('catalog_native_writable_states') != states
            or counts.get('distinct_case_insensitive_writable_names') != len(writable_names)):
        raise ValueError('native Writable classes are unaccounted')
    if sorted(names) != document['unique_names'] or len(names) != counts['distinct_case_insensitive_entry_names']:
        raise ValueError('unique public names are duplicated or unaccounted')
    if len(document['container_rows_outside_total']) != counts['catalog_container_rows_outside_total']:
        raise ValueError('container rows are unaccounted')
    if type(counts['catalog_unique_tag_names']) is not int or counts['catalog_unique_tag_names'] < 0:
        raise ValueError('native unique-name counter is invalid')
    sources = document['producer']['sources']
    if 'Image/ExifTool/BuildTagLookup.pm' not in sources or 'Image/ExifTool.pm' not in sources:
        raise ValueError('native source provenance is missing')


WRITABLE_CLASSES = {'writable', 'writable_protected', 'not_writable', 'not_listed'}


def writable_class(column: str, in_write_lookup: bool | None = None) -> str:
    """BuildTagLookup POD semantics for one native Writable column value.

    Anything but ``no`` is writable; a trailing ``*`` flag marks a Protected
    tag that ExifTool writes only indirectly; a leading ``-`` links a separate
    table.  Natively ``=struct`` replaces the whole column, even ``no``, so a
    struct's class comes from BuildTagLookup's writable-tag lookup.
    """
    value = column[1:] if column.startswith('-') else column
    if value.startswith('=struct'):
        if type(in_write_lookup) is not bool:
            raise ValueError('struct Writable column lacks its native write-lookup fact')
        return 'writable' if in_write_lookup else 'not_writable'
    if value == '' or re.match(r'no(?![A-Za-z0-9\[])', value):
        return 'not_writable'
    flags = re.search(r'[+/~!*:_^]*\Z', value).group(0)
    return 'writable_protected' if '*' in flags else 'writable'


def validate_native_writable(fact: object) -> None:
    if not isinstance(fact, dict) or set(fact) != {'state', 'column', 'candidates', 'class', 'in_write_lookup'}:
        raise ValueError('catalog row is missing its native Writable column')
    state, column, candidates, klass = fact['state'], fact['column'], fact['candidates'], fact['class']
    lookup = fact['in_write_lookup']
    columns = [column] if isinstance(column, str) else candidates if isinstance(candidates, list) else []
    is_struct = any(isinstance(c, str) and c.lstrip('-').startswith('=struct') for c in columns)
    if (type(lookup) is bool) != is_struct:
        raise ValueError('native write-lookup fact is present only for struct Writable columns')
    if klass not in WRITABLE_CLASSES or not isinstance(candidates, list):
        raise ValueError('native Writable class is malformed')
    if state == 'determined':
        if not isinstance(column, str) or candidates or writable_class(column, lookup) != klass:
            raise ValueError('native Writable column and class disagree')
    elif state == 'format_ambiguous':
        if (column is not None or len(candidates) < 2 or candidates != sorted(set(candidates))
                or not all(isinstance(c, str) for c in candidates)
                or {writable_class(c, lookup) for c in candidates} != {klass}):
            raise ValueError('ambiguous native Writable candidates are malformed')
    elif state == 'not_listed':
        if column is not None or candidates or klass != 'not_listed':
            raise ValueError('unlisted native Writable fact is malformed')
    else:
        raise ValueError('native Writable state is unknown')


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
    classes = counts['catalog_native_writable_classes']
    writable_rows = [
        ('Entries writable by ExifTool', classes.get('writable', 0), 'Native Writable column is anything but `no` and not Protected; the write-parity denominator'),
        ('Entries ExifTool writes only indirectly', classes.get('writable_protected', 0), 'Protected (`*`): written automatically, never directly'),
        ('Entries not writable by ExifTool', classes.get('not_writable', 0), 'Native Writable column is `no`'),
        ('Entries omitted from TagNames', classes.get('not_listed', 0), 'Counted by BuildTagLookup but not documented, so no Writable column'),
        ('Entries whose exact write format is ambiguous', counts['catalog_native_writable_states'].get('format_ambiguous', 0), 'Collapsed native columns; writability is still determined'),
        ('Distinct case-insensitive writable names', counts['distinct_case_insensitive_writable_names'], 'Distinct `lc(Name)` over writable entries'),
    ]
    return '\n'.join(['| Measurement | Count | Meaning |', '| --- | ---: | --- |'] +
                     [f'| {label} | {counts[key]:,} | {meaning} |' for label, key, meaning in rows] +
                     [f'| {label} | {count:,} | {meaning} |' for label, count, meaning in writable_rows])


def rendered_report(document: dict, template: str) -> str:
    begin, end = '<!-- catalog-counts:start -->', '<!-- catalog-counts:end -->'
    if template.count(begin) != 1 or template.count(end) != 1:
        raise ValueError('report must have exactly one generated-counts region')
    prefix, rest = template.split(begin)
    _, suffix = rest.split(end)
    return prefix + begin + '\n' + render_counts(document) + '\n' + end + suffix


def semantic_document(document: dict) -> dict:
    # Runtime identity is provenance. Cross-Perl checking is allowed only when
    # all source fingerprints and all catalog facts still match exactly.
    return {key: value for key, value in document.items() if key != 'capture_environment'}


def validate_destinations(output: Path, report: Path, source: Path, root: Path = ROOT) -> None:
    output, report, source = output.resolve(), report.resolve(), source.resolve()
    protected = {(root / 'tools/exiftool-tables/catalog_snapshot.py').resolve(),
                 (root / 'tools/exiftool-tables/dump_hydrated_catalog.pl').resolve(),
                 (root / '.exiftool-version').resolve()}
    if output == report:
        raise ValueError('snapshot and report destinations alias each other')
    for destination in (output, report):
        if destination == source or source in destination.parents or destination in protected:
            raise ValueError('output or report aliases a source input')


def write_staged(documents: list[tuple[Path, str]]) -> None:
    # Stage both files before changing either destination. Each replace is
    # atomic; this is deliberately not described as a filesystem transaction.
    staged = []
    try:
        for destination, content in documents:
            destination.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(mode='w', encoding='utf-8', dir=destination.parent,
                                             prefix='.catalog-', delete=False) as handle:
                temporary = Path(handle.name)
                staged.append((temporary, destination))
                handle.write(content)
            temporary.chmod(destination.stat().st_mode & 0o777 if destination.exists() else 0o644)
        for temporary, destination in staged:
            os.replace(temporary, destination)
    finally:
        for temporary, _ in staged:
            temporary.unlink(missing_ok=True)


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
    validate_destinations(output, args.report, source)
    if output.exists() and not (args.check or args.replace):
        raise ValueError('output exists; use --check or --replace')
    print('=== instrument: catalog_snapshot.py ===', file=sys.stderr)
    print(f'commit: {state.commit}; dirty_override: {overridden}', file=sys.stderr)
    print(f'scope: native catalog only; no oxidex binary, fixtures or write operations', file=sys.stderr)
    print(f'pin: {(ROOT / ".exiftool-version").read_text().strip()}; source: {source}; perl: {args.perl}', file=sys.stderr)
    document = capture(source, args.perl)
    print('capture_environment: ' + json.dumps(document['capture_environment'], sort_keys=True), file=sys.stderr)
    rendered = json.dumps(document, sort_keys=True, ensure_ascii=True, separators=(',', ':')) + '\n'
    report_before = args.report.read_text()
    report_after = rendered_report(document, report_before)
    if args.check:
        if report_before != report_after:
            raise ValueError('report counts are stale; regenerate from the pinned source')
        recorded = json.loads(output.read_text())
        validate(recorded, document['exiftool_version'])
        print('recorded_capture_environment: ' + json.dumps(recorded.get('capture_environment'), sort_keys=True), file=sys.stderr)
        if semantic_document(recorded) != semantic_document(document):
            raise ValueError('catalog snapshot is stale; regenerate from the pinned source')
    else:
        documents = [(output, rendered)]
        if report_before != report_after:
            documents.append((args.report, report_after))
        write_staged(documents)
    print(json.dumps(document['counts'], sort_keys=True))
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as exc:
        raise SystemExit(f'catalog snapshot refused: {exc}')
