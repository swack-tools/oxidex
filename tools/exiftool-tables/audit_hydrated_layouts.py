#!/usr/bin/env python3
"""Audit a complete hydrated source graph; this does not measure runtime support."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path


def audit(document):
    h = document['hydrated_layouts']
    if h['schema'] != 'oxidex_hydrated_layout_projection_v1' or h['selection'] != 'full_hydrated_catalog':
        raise ValueError('a complete hydrated catalog projection is required')
    tables = h['tables']
    if not tables or any(h[key] != len(tables) for key in ['table_count', 'requested_table_count', 'available_table_count']):
        raise ValueError('table conservation failed')
    objects = h['shared_reference_objects']
    refs = Counter()
    def visit(value):
        if isinstance(value, dict):
            if '__ref' in value:
                kind = value['__ref']
                refs[kind] += 1
                if kind == 'tag_table':
                    names = value.get('table_full_names', [])
                    if not names or any(name not in tables for name in names):
                        raise ValueError('unresolved table reference')
                else:
                    target = objects.get(value.get('object_id'))
                    if target is None or target.get('kind') != kind:
                        raise ValueError('unresolved or incorrectly typed object reference')
            for item in value.values():
                visit(item)
        elif isinstance(value, list):
            for item in value:
                visit(item)
    def variant_count(row):
        if isinstance(row, dict) and '_variants' in row:
            return sum(variant_count(child) for child in row['_variants'])
        return 1
    for name, table in tables.items():
        if table['full_name'] != name or table['tag_count'] != len(table['tags']):
            raise ValueError('table row identity/count mismatch')
    visit(tables)
    visit(objects)
    provenance = h['source_provenance']
    if not provenance['sources'] or len(provenance['producer_sha256']) != 64:
        raise ValueError('missing source provenance')
    return {
        'schema': 'oxidex_hydrated_layout_audit_v1',
        'scope': 'source graph only; generated and observed read/write support unmeasured',
        'exiftool_version': document['exiftool_version'],
        'source_provenance': provenance,
        'tables': len(tables),
        'raw_keys': sum(len(t['tags']) for t in tables.values()),
        'variants': sum(variant_count(row) for t in tables.values() for row in t['tags'].values()),
        'catalog_native_entries': h['catalog_counts']['total_tag_entries'],
        'catalog_native_legacy_unique_counter': h['catalog_counts']['unique_tag_names'],
        'shared_objects': len(objects),
        'reference_occurrences_by_kind': dict(sorted(refs.items())),
        'unresolved_references': 0,
        'shortcuts': h['helpers']['shortcuts']['entry_count'],
    }



def check_profile(result, expected):
    """Compare semantic totals and provenance, independent of JSON ordering."""
    fields = ('schema', 'exiftool_version', 'tables', 'raw_keys', 'variants',
              'catalog_native_entries', 'catalog_native_legacy_unique_counter',
              'shared_objects', 'reference_occurrences_by_kind',
              'unresolved_references', 'shortcuts', 'source_provenance')
    for field in fields:
        if result[field] != expected[field]:
            raise ValueError(f'committed audit profile mismatch: {field}')


def check_catalog(document, catalog):
    """Reconcile the independent native catalog, not producer-owned tag counts."""
    h = document['hydrated_layouts']
    entries = catalog['entries']
    if (catalog['exiftool_version'] != document['exiftool_version'] or
            len(entries) != catalog['counts']['catalog_total_tag_entries'] or
            len(entries) != h['catalog_counts']['total_tag_entries']):
        raise ValueError('catalog denominator mismatch')
    sources = catalog['producer']['sources']
    if not sources or any(h['source_provenance']['sources'].get(k) != v
                          for k, v in sources.items()):
        raise ValueError('catalog source provenance mismatch')
    seen = set()
    for entry in entries:
        identity = (entry['table'], entry['raw_key'], entry['variant_index'])
        if identity in seen:
            raise ValueError('duplicate catalog identity')
        seen.add(identity)
        table = h['tables'].get(entry['table'], {})
        row = table.get('tags', {}).get(entry['raw_key'])
        variants = row.get('_variants', [row]) if isinstance(row, dict) else [row]
        index = entry['variant_index']
        if (not isinstance(index, int) or index < 0 or index >= len(variants) or
                not isinstance(variants[index], dict) or
                variants[index].get('Name') != entry['name']):
            raise ValueError(f'catalog coordinate/name mismatch: {identity!r}')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--dump', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--replace', action='store_true')
    parser.add_argument('--expected-audit', type=Path)
    parser.add_argument('--catalog', type=Path)
    args = parser.parse_args()
    inputs = [args.dump, args.expected_audit, args.catalog]
    if any(path and args.output.resolve() == path.resolve() for path in inputs):
        parser.error('output aliases an input')
    if bool(args.expected_audit) != bool(args.catalog):
        parser.error('--expected-audit and --catalog must be supplied together')
    if args.output.exists() and not args.replace:
        parser.error('output exists; use --replace for intentional regeneration')
    raw = args.dump.read_bytes()
    document = json.loads(raw)
    result = audit(document)
    if args.expected_audit:
        check_profile(result, json.loads(args.expected_audit.read_text()))
        check_catalog(document, json.loads(args.catalog.read_text()))
    result['dump_sha256'] = hashlib.sha256(raw).hexdigest()
    result['dump_bytes'] = len(raw)
    result['instrument_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
    print(f"=== instrument: audit_hydrated_layouts.py ===\ntables: {result['tables']}; variants: {result['variants']}; unresolved references: 0")


if __name__ == '__main__':
    main()
