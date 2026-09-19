#!/usr/bin/env python3
"""Attribute oxidex's MATCHED rows to generated-data classes from silence censuses.

Instrument: conformance.py --json-out (tools/exiftool-tables/conformance.py, pinned
ExifTool 13.59 oracle) run once with the unmodified binary (CONTROL) and once per
class token (PROBE, OXIDEX_PROBE_SILENCE=<token>) against a build carrying
probe.patch; census.sh drives both. conformance.py spawns oxidex with the
inherited environment, so the switch reaches the binary. Method and caveats:
README.md in this directory.

What it computes, per class:

  matched_lost   = control matched - probe matched, summed from per_format
                   (the same counters the TOTAL line prints). Authoritative.
  per-row split  = rebuilt from per_file: for every (file, name) the change in
                   MISSING rows plus the change in VALUE rows is the number of
                   that name's matched rows lost (E = matched + value + missing
                   + renames is fixed by the oracle). Renames are not listed per
                   file, so the rows that moved in or out of RENAME are the
                   reconciliation residual, which must equal the per_format
                   rename delta exactly; the script checks and prints it.
  DIRECT         = lost (group, name) is in class-names.json for the class
                   (or in a group the class owns outright: DICOM, GeoTiff);
  DIRECT_NAME    = the name is in the class index under another group;
  CASCADE        = anything else (Composite inputs, readers of the dropped rows,
                   revealed/overwritten same-key hand copies scored differently).
  gained         = (file, name) whose matched count ROSE under the silence (a
                   hand copy or Composite revealed by the drop), reported apart.

A lost row's family-0 group comes from the newly-MISSING occurrence (the oracle
side's group); a row that moved matched->VALUE carries no group in the JSON, so
it takes the class index's group for that name when unique, else '?'.

Usage:
  attribute.py --class-names class-names.json [--add-pairs addendum.json] \\
      --control control.json --probe engine=engine.json ... [--json-out out.json]

Guard: conformance.py ignores oxidex's exit status. A probe run whose binary
refused the token (exit 2) scores every file as all-MISSING and still prints a
TOTAL. This script refuses a probe whose matched total falls below
--min-kept (default 25%) of control unless --allow-collapse is given.
"""
import argparse, collections, json, re, sys

SUFFIX = re.compile(r' \((\d+)\)$')

def load(path):
    with open(path, encoding='utf-8') as fh:
        return json.load(fh)

def groups_of(g0):
    if not g0:
        return set()
    head = g0.split(' ')[0]
    m = re.fullmatch(r'APP(\d+)\.\.APP(\d+)', head)
    if m:
        return {f'APP{i}' for i in range(int(m.group(1)), int(m.group(2)) + 1)}
    return {head}

def class_index(cn):
    """token -> {'pairs': {(g0, name)}, 'names': {name}}; unions included."""
    idx = collections.defaultdict(lambda: {'pairs': set(), 'names': set()})
    def add(tok, gs, name):
        idx[tok]['names'].add(name)
        for g in gs:
            idx[tok]['pairs'].add((g, name))
    for t in cn['engine']['tables']:
        for g, n in t['pairs']:
            add('engine', {g}, n)
    for s in cn['legacy']['L1_runtime_decode_api']:
        for g, n in s['emit_pairs']:
            add('legacy-l1', {g}, n)
    for sub, tok in (('L2_manifest_vendor_walkers', 'legacy-l2'),
                     ('L3_offmanifest_generated_origin_walkers', 'legacy-l3')):
        for d in cn['legacy'][sub].values():
            for n in d['names']:
                add(tok, groups_of(d['group0']), n)
    p = cn['producers']
    for key, tok in (('file_identity', 'producers-identity'),
                     ('composite_generated_compute', 'producers-composite'),
                     ('dicom', 'producers-dicom'), ('geotiff', 'producers-geotiff'),
                     ('fits_names', 'producers-fits')):
        for g, n in p[key]['pairs']:
            add(tok, groups_of(g) or {g}, n)
    for g, n in p['composite_declared']['pairs'] + p['lens_id']['pairs']:
        add('composite-declared', {'Composite'}, n)
    unions = {'legacy': ('legacy-l1', 'legacy-l2', 'legacy-l3'),
              'producers': ('producers-identity', 'producers-composite',
                            'producers-dicom', 'producers-geotiff', 'producers-fits')}
    for u, parts in unions.items():
        for q in parts:
            idx[u]['pairs'] |= idx[q]['pairs']
            idx[u]['names'] |= idx[q]['names']
    return idx, unions

def group_owned(tok, g, n):
    """Every row of these groups is written at the silenced site (scout 3c/3d);
    covers the class-names DICOM scrape gap (8 eb()/ep() names)."""
    if g == 'DICOM' and tok in ('producers', 'producers-dicom'):
        return True
    if g == 'GeoTiff' and n != 'GeoTiffVersion' and tok in ('producers', 'producers-geotiff'):
        return True
    return False

def missing_multiset(pf):
    """Counter over (group, name) of one file's MISSING occurrences."""
    c = collections.Counter()
    for key, gv in pf.get('missing', {}).items():
        base = SUFFIX.sub('', key)
        name = base.split(':')[-1]
        group = gv[0] if isinstance(gv, list) and gv else (base.split(':')[0] if ':' in base else '')
        c[(group, name)] += 1
    return c

def vd_by_name(pf):
    c = collections.Counter()
    for row in pf.get('value_diff', []):
        c[row[0]] += 1
    return c

def totals(doc):
    t = collections.Counter()
    for c in doc['per_format'].values():
        t.update(c)
    return t

def resolve(tok, idx):
    """A comma-joined token (what OXIDEX_PROBE_SILENCE accepts) is the union
    of its parts' indexes."""
    parts = [t.strip() for t in tok.split(',') if t.strip()]
    for t in parts:
        if t not in idx:
            sys.exit(f'unknown class token {t!r} (class-names has no index for it)')
    e = {'pairs': set(), 'names': set()}
    for t in parts:
        e['pairs'] |= idx[t]['pairs']
        e['names'] |= idx[t]['names']
    return parts, e

def attribute(tok, ctl, prb, idx):
    parts, e = resolve(tok, idx)
    composite_names = {n for _g, n in idx['composite-declared']['pairs']}
    name_groups = collections.defaultdict(set)
    for g, n in e['pairs']:
        name_groups[n].add(g)
    rows = collections.Counter()          # (bucket, group, name) -> lost rows
    gained = collections.Counter()         # (group, name) -> matched rows gained
    per_format_rows = collections.Counter()
    top_files = collections.Counter()
    sum_delta = 0
    only_in_control = [f for f in ctl['per_file'] if f not in prb['per_file']]
    for f, pc in ctl['per_file'].items():
        pp = prb['per_file'].get(f)
        if pp is None:
            continue
        mc, mp = missing_multiset(pc), missing_multiset(pp)
        vc, vp = vd_by_name(pc), vd_by_name(pp)
        names = {n for _g, n in mc} | {n for _g, n in mp} | set(vc) | set(vp)
        for n in names:
            dmiss = {g: mp[(g, n)] - mc[(g, n)] for g in {g for g, m in mp if m == n} | {g for g, m in mc if m == n}}
            delta = sum(dmiss.values()) + (vp[n] - vc[n])
            sum_delta += delta
            if delta == 0:
                continue
            # the group of the rows: from the newly-missing side when there is one
            grp_counts = collections.Counter({g: d for g, d in dmiss.items() if d > 0})
            if grp_counts:
                # Largest delta wins; a tie goes to the lexically first group.
                # Counter.most_common breaks ties by insertion order, which
                # here is set-iteration order and so moved with PYTHONHASHSEED
                # (up to 32 rows between DIRECT and CASCADE at 72eae8a5).
                grp = min(grp_counts.items(), key=lambda kv: (-kv[1], kv[0]))[0]
            else:
                gs = name_groups.get(n)
                if gs and len(gs) == 1:
                    grp = next(iter(gs))
                elif n in composite_names:
                    grp = 'Composite(inferred)'
                else:
                    grp = '?'
            if delta < 0:
                gained[(grp, n)] += -delta
                continue
            if grp.startswith('Composite') and ('Composite', n) not in e['pairs']:
                # Only producers-composite / composite-declared write Composite
                # rows, and only the names they index; any other lost Composite
                # row is an input cascade (scout 5.2).
                bucket = 'CASCADE'
            elif (grp, n) in e['pairs'] or any(group_owned(t, grp, n) for t in parts):
                bucket = 'DIRECT'
            elif n in e['names']:
                bucket = 'DIRECT_NAME'
            else:
                bucket = 'CASCADE'
            rows[(bucket, grp, n)] += delta
            per_format_rows[pc.get('format', '?')] += delta
            top_files[f] += delta
    tc, tp = totals(ctl), totals(prb)
    matched_lost = tc['matched'] - tp['matched']
    rename_delta = tp['renames'] - tc['renames']
    residual = matched_lost - sum_delta - rename_delta
    by_bucket = collections.Counter()
    by_group = collections.defaultdict(collections.Counter)
    by_name = collections.defaultdict(collections.Counter)
    for (b, g, n), c in rows.items():
        by_bucket[b] += c
        by_group[b][g] += c
        by_name[b][f'{g}:{n}'] += c
    return {
        'class': tok,
        'control_matched': tc['matched'], 'probe_matched': tp['matched'],
        'matched_lost': matched_lost,
        'share_of_control_matched': (matched_lost / tc['matched']) if tc['matched'] else None,
        'lost_rows_rebuilt': sum(rows.values()), 'gained_rows': sum(gained.values()),
        'rename_delta': rename_delta,
        'reconciliation_residual': residual,
        'buckets': dict(by_bucket),
        'by_group': {b: dict(c.most_common()) for b, c in by_group.items()},
        'top_names': {b: dict(c.most_common(25)) for b, c in by_name.items()},
        'gained_top': {f'{g}:{n}': c for (g, n), c in gained.most_common(15)},
        'lost_by_format': dict(per_format_rows.most_common()),
        'top_files': {k: v for k, v in top_files.most_common(10)},
        'files_only_in_control': only_in_control,
        'probe_value_diff_delta': tp['value_diff'] - tc['value_diff'],
        'probe_missing_delta': tp['missing'] - tc['missing'],
        'probe_extra_delta': tp['extra'] - tc['extra'],
    }

def main():
    ap = argparse.ArgumentParser(description=__doc__.split('\n\n')[0])
    ap.add_argument('--class-names', required=True)
    ap.add_argument('--add-pairs', action='append', default=[], metavar='JSON',
                    help='addendum {token: [[group0, name], ...]} for rows a route gained after '
                         'the class index was built (e.g. class-names-addendum-838.json)')
    ap.add_argument('--control', required=True)
    ap.add_argument('--probe', action='append', default=[], metavar='TOKEN=PATH')
    ap.add_argument('--json-out')
    ap.add_argument('--min-kept', type=float, default=0.25)
    ap.add_argument('--allow-collapse', action='store_true')
    args = ap.parse_args()
    idx, unions = class_index(load(args.class_names))
    for path in args.add_pairs:
        for tok, pairs in load(path).items():
            if tok.startswith('_'):
                continue
            if tok not in idx:
                sys.exit(f'{path}: unknown class token {tok!r}')
            for g, n in pairs:
                idx[tok]['pairs'].add((g, n))
                idx[tok]['names'].add(n)
    ctl = load(args.control)
    tc = totals(ctl)
    print(f"=== instrument: attribute.py over conformance.py --json-out ===")
    print(f"control: {args.control}  files={len(ctl['per_file'])} matched={tc['matched']} "
          f"value={tc['value_diff']} missing={tc['missing']} renames={tc['renames']}")
    results = {}
    for spec in args.probe:
        tok, _, path = spec.partition('=')
        resolve(tok, idx)
        prb = load(path)
        tp = totals(prb)
        if tc['matched'] and tp['matched'] < args.min_kept * tc['matched'] and not args.allow_collapse:
            sys.exit(f'{tok}: probe matched {tp["matched"]} < {args.min_kept:.0%} of control '
                     f'{tc["matched"]} -- a refused token or a dead binary scores as all-MISSING; '
                     'check the probe binary ran (rc 0) before trusting this')
        r = attribute(tok, ctl, prb, idx)
        results[tok] = r
        share = r['share_of_control_matched']
        print(f"\n== {tok}: matched {r['control_matched']} -> {r['probe_matched']}  "
              f"LOST {r['matched_lost']} ({share:.2%} of control matched)")
        print(f"   rebuilt per-row: lost {r['lost_rows_rebuilt']}  gained {r['gained_rows']}  "
              f"rename delta {r['rename_delta']}  residual {r['reconciliation_residual']}"
              + ('' if r['reconciliation_residual'] == 0 else '   <-- RECONCILIATION FAILED'))
        print(f"   buckets: {r['buckets']}")
        for b in ('DIRECT', 'DIRECT_NAME', 'CASCADE'):
            if b in r['by_group']:
                print(f"   {b} by group: {dict(list(r['by_group'][b].items())[:8])}")
                print(f"   {b} top names: {dict(list(r['top_names'][b].items())[:12])}")
        if r['gained_top']:
            print(f"   gained (revealed by the drop): {r['gained_top']}")
        if r['files_only_in_control']:
            print(f"   files scored in control but not probe: {len(r['files_only_in_control'])}")
    for u, parts in unions.items():
        if u in results and all(p in results for p in parts):
            s = sum(results[p]['matched_lost'] for p in parts)
            print(f"\n== overlap {u}: union lost {results[u]['matched_lost']} vs sum of parts {s} "
                  f"(union - parts = {results[u]['matched_lost'] - s}: rows two sub-classes both "
                  "produce under one key count only in the union; negative = cascade double-count in parts)")
    if args.json_out:
        with open(args.json_out, 'w', encoding='utf-8') as fh:
            json.dump(results, fh, indent=1, sort_keys=True)
        print(f"\nwrote {args.json_out}")

if __name__ == '__main__':
    main()
