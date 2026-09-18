#!/usr/bin/env python3
"""Fill benches/spike/DISPATCH_PERF.md's @@PLACEHOLDERS@@ from the evidence
directory, so every number in the report is traceable to a file the
instrument wrote (no hand transcription).

Usage: fill_report.py <evidence-dir> [--template benches/spike/DISPATCH_PERF.md]
Writes the filled report in place and prints unfilled placeholders, if any.
"""
import json, os, re, sys
from collections import OrderedDict

E = sys.argv[1]
ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "..", ".."))
TPL = sys.argv[sys.argv.index("--template") + 1] if "--template" in sys.argv else os.path.join(ROOT, "benches/spike/DISPATCH_PERF.md")

def hf(name):
    with open(os.path.join(E, name + ".json")) as f:
        return json.load(f)["results"]

def ms(x): return x * 1000

def short(cmd):
    cmd = re.sub(r"/tmp/oxidex-perl538-build-\S+/perl5\.38\.2 -I\S+ \S+/exiftool", "exiftool", cmd)
    cmd = re.sub(r"/tmp/oxidex-exiftool-cache/exiftool/exiftool", "exiftool", cmd)
    cmd = re.sub(r"\S+/target/release/oxidex", "oxidex", cmd)
    cmd = re.sub(r"/tmp/oxidex-exiftool-cache/exiftool/t/images/", "", cmd)
    if cmd.count(" ") > 8:
        cmd = " ".join(cmd.split()[:4]) + " <194 files>"
    return cmd

# ---- hyperfine table -------------------------------------------------------
hft = open(os.path.join(E, "hyperfine.txt")).read()
loads = {}
for name, when, l1, l5, l15 in re.findall(r"^\[(\w+) (before|after)\] .*load averages?: ([0-9.]+) ([0-9.]+) ([0-9.]+)", hft, re.M):
    loads.setdefault(name, {})[when] = l1
rows = ["| Scenario | Command | Median | Min | Mean ± σ | Max | Runs | load1 before -> after | Ratio ExifTool/oxidex (median; min) |", "|---|---|---:|---:|---:|---:|---:|---:|---:|"]
ratios = {}
for scen, label in [("canon_jpg", "(a) Canon.jpg `-j -a -G1`"), ("nikon_nef", "(b) Nikon.nef `-j -a -G1`"),
                    ("corpus_parallel", "(c) 194-file t/images `-j -a -G1`, oxidex rayon 10 cores"),
                    ("corpus_1thread", "(c') same, oxidex `RAYON_NUM_THREADS=1`"), ("noop", "process floor")]:
    res = hf(scen)
    ox = next((r for r in res if "target/release/oxidex" in r["command"]), None)
    et = next((r for r in res if "/exiftool " in r["command"] + " " and "target/release/oxidex" not in r["command"]), None)
    if scen == "corpus_1thread" and et is None:
        et = next(r for r in hf("corpus_parallel") if "target/release/oxidex" not in r["command"])
    ratio = et["median"] / ox["median"] if (ox and et) else None
    ratio_min = et["min"] / ox["min"] if (ox and et) else None
    ratios[scen] = ratio
    ld = loads.get(scen, {})
    for r in res:
        rr = f"**{ratio:.2f}x**; {ratio_min:.2f}x" if (ratio and "target/release/oxidex" in r["command"]) else ""
        cv = 100 * r["stddev"] / r["mean"]
        rows.append(f"| {label} | `{short(r['command'])}` | {ms(r['median']):.1f} ms | {ms(r['min']):.1f} ms | {ms(r['mean']):.1f} ± {ms(r['stddev']):.1f} ({cv:.0f} %) | {ms(r['max']):.1f} ms | {len(r['times'])} | {ld.get('before', '?')} -> {ld.get('after', '?')} | {rr} |")
        label = ""
HF_TABLE = "\n".join(rows)
noop_ox = next(r for r in hf("noop") if "target/release/oxidex" in r["command"])["median"]

# ---- stages ----------------------------------------------------------------
def tsv(name):
    lines = [l.rstrip("\n").split("\t") for l in open(os.path.join(E, name))]
    return lines[0], {r[0]: dict(zip(lines[0], r)) for r in lines[1:]}
hdr, two = tsv("stages-two-files.tsv"); _, corp = tsv("stages-corpus.tsv")
def stage_table(rowsd, names):
    out = ["| file | tags | fs µs | mmap µs | detect µs | **read µs** | json µs | read allocs | read reallocs | read bytes | retained allocs | json allocs |", "|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|"]
    for n in names:
        r = rowsd[n]
        out.append(f"| {n} | {r['tags']} | {r['fs_us']} | {r['mmap_us']} | {r['detect_us']} | **{r['read_us']}** | {r['json_us']} | {r['read_allocs']} | {r['read_reallocs']} | {r['read_bytes']} | {r['retained_allocs']} | {r['json_allocs']} |")
    return "\n".join(out)
STAGES = stage_table(two, ["Canon.jpg", "Nikon.nef"]) + "\n" + stage_table(corp, ["TOTAL"]).split("\n")[2].replace("| TOTAL |", "| corpus TOTAL (194 files, one pass) |")
canon, nef, tot = two["Canon.jpg"], two["Nikon.nef"], corp["TOTAL"]
read_canon_ms = float(canon["read_us"]) / 1000; read_nef_ms = float(nef["read_us"]) / 1000

# ---- criterion -------------------------------------------------------------
crit = open(os.path.join(E, "criterion.txt")).read()
res = OrderedDict()
for m in re.finditer(r"^(\w+)/(\w+)/(\w+)/(\d+)\s*\n\s*time:\s*\[([0-9.]+) (\w+) ([0-9.]+) (\w+) ([0-9.]+) (\w+)\]", crit, re.M):
    unit = {"ns": 1, "µs": 1e3, "ms": 1e6}[m.group(8)]
    res[(m.group(1), m.group(2), m.group(3))] = (float(m.group(7)) * unit, int(m.group(4)))
crows = ["| table / workload | n | linear ns/lookup | bsearch ns/lookup | matcharm ns/lookup | hashmap ns/lookup |", "|---|---:|---:|---:|---:|---:|"]
seen = OrderedDict()
for (t, w, i), (ns, n) in res.items():
    seen.setdefault((t, w), {})[i] = ns / n; seen[(t, w)]["n"] = n
for (t, w), d in seen.items():
    crows.append(f"| {t} / {w} | {d['n']} | {d.get('linear', float('nan')):.1f} | {d.get('bsearch', float('nan')):.1f} | {d.get('matcharm', float('nan')):.1f} | {d.get('hashmap', float('nan')):.1f} |")
CRITERION = "\n".join(crows)
def per(impl):
    a = seen[("exif_main", "canon_jpg")][impl]; b = seen[("canon_main", "canon_jpg")][impl]
    return (a * 44 + b * 26) / 70  # weighted by the Canon.jpg replay mix
bs, ma, li = per("bsearch"), per("matcharm"), per("linear")
bs_nef = seen[("exif_main", "nikon_nef")]["bsearch"]

# ---- profiles --------------------------------------------------------------
def buckets(name):
    txt = open(os.path.join(E, name)).read()
    stage = dict((k, float(p)) for p, _, k in re.findall(r"^\s*([0-9.]+)%\s+(\d+)\s+(.+)$", txt.split("== allocator/memcpy")[0], re.M))
    alloc = dict((k, float(p)) for p, _, k in re.findall(r"^\s*([0-9.]+)%\s+(\d+)\s+(.+)$", txt.split("== allocator/memcpy leaf share")[1].split("\n==")[0], re.M))
    by_stage = dict((k, float(p)) for p, _, k in re.findall(r"^\s*([0-9.]+)%\s+(\d+)\s+(.+)$", txt.split("by owning stage ==")[1].split("\n==")[0], re.M))
    incl = dict((k.strip(), float(p)) for p, _, k in re.findall(r"^\s*([0-9.]+)%\s+(\d+)\s+(.+)$", txt.split("== inclusive time per oxidex function")[1].split("== by library")[0], re.M))
    return stage, alloc, by_stage, incl, txt
cs, ca, cb, ci, ctxt = buckets("samply-stages-corpus-locked.buckets.txt")
ls, la, lb, li_, ltxt = buckets("samply-cli-canon-locked.buckets.txt")
def incl(d, needle):
    return sum(v for k, v in d.items() if needle in k) if needle.startswith("+") else next((v for k, v in d.items() if needle in k), 0.0)
def section(txt, start, end):
    return txt.split(start)[1].split(end)[0].strip()
def profile_block(txt):
    return "```\n" + section(txt, "== stage (inclusive, outermost owner) ==", "== allocator+memmove leaf time, by owning stage ==").replace("== allocator/memcpy leaf share (overlay, any stage) ==", "\n== allocator/memcpy leaf share (overlay, any stage) ==") + "\n```\n\nTop inclusive functions:\n```\n" + "\n".join(section(txt, "== inclusive time per oxidex function", "== by library").split("\n")[1:23]) + "\n```"
eng = lambda s: sum(s.get(k, 0) for k in ("engine:ifd", "engine:binary", "engine:serial", "engine:cond/exprs"))

V = {
 "BSEARCH_NS": f"{bs:.1f}", "MATCH_NS": f"{ma:.1f}", "LINEAR_NS": f"{li:.1f}",
 "BSEARCH_FILE_US": f"{bs*81/1000:.2f}", "BSEARCH_NEF_US": f"{bs_nef*189/1000:.2f}", "LINEAR_FILE_US": f"{li*81/1000:.1f}",
 "MATCH_SAVING_US": f"{(bs-ma)*81/1000:.2f}", "MATCH_SAVING_NEF_US": f"{(bs_nef-seen[('exif_main','nikon_nef')]['matcharm'])*189/1000:.2f}",
 "MATCH_SAVING_PCT": f"{(bs-ma)*81/1000/ (read_canon_ms*1000)*100:.4f}",
 "READ_CANON_MS": f"{read_canon_ms:.1f}", "BSEARCH_SHARE_PCT": f"{bs*81/1000/(read_canon_ms*1000)*100:.3f}",
 "BSEARCH_NEF_SHARE_PCT": f"{bs_nef*189/1000/(read_nef_ms*1000)*100:.3f}",
 "ENGINE_INCL_PCT": f"{eng(cs):.2f}", "ENGINE_CLI_PCT": f"{eng(ls):.2f}",
 "READ_ALLOCS_CANON": canon["read_allocs"], "READ_REALLOCS_CANON": canon["read_reallocs"], "READ_BYTES_CANON": f"{int(canon['read_bytes'])/1e6:.1f}",
 "READ_ALLOCS_NEF": nef["read_allocs"], "READ_REALLOCS_NEF": nef["read_reallocs"], "JSON_ALLOCS_CANON": canon["json_allocs"],
 "READ_ALLOCS_CORPUS": tot["read_allocs"], "ALLOCS_PER_TAG": f"{int(tot['read_allocs'])/int(tot['tags']):.0f}",
 "COMPOSITE_PCT": f"{cs.get('composite',0):.1f}", "COMPOSITE_CLI_PCT": f"{ls.get('composite',0):.1f}",
 "FILETYPE_PCT": f"{incl(li_, 'filetype::COMPILED'):.1f}", "TAGDB_PCT": f"{incl(li_, 'tag_db::lookup_tag_name'):.1f}",
 "REGEX_PCT": f"{incl(li_, 'cond::regex_match_str'):.1f}",
 "IO_PCT": f"{cs.get('fs+mmap+detect',0):.1f}", "HAND_PCT": f"{cs.get('hand parsers',0):.1f}", "HAND_CLI_PCT": f"{ls.get('hand parsers',0):.1f}",
 "CONV_PCT": f"{cs.get('value/printconv',0):.2f}", "SINK_PCT": f"{cs.get('map/sink insert',0):.2f}", "SINK_CLI_PCT": f"{ls.get('map/sink insert',0):.2f}",
 "JSON_PCT": f"{cs.get('json-output',0):.2f}", "JSON_CLI_PCT": f"{ls.get('json-output',0):.2f}",
 "ALLOC_PCT": f"{sum(ca.values()):.1f}", "ALLOC_MALLOC_PCT": f"{next((v for k, v in ca.items() if 'malloc' in k), 0):.0f}", "ALLOC_MEMMOVE_PCT": f"{next((v for k, v in ca.items() if 'memmove' in k), 0):.0f}", "ALLOC_CLI_PCT": f"{sum(la.values()):.1f}", "ALLOC_COMPOSITE_PCT": f"{cb.get('composite',0):.1f}",
 "RATIO_CANON": f"{ratios['canon_jpg']:.2f}", "RATIO_NEF": f"{ratios['nikon_nef']:.2f}", "RATIO_CORPUS_1T": f"{ratios['corpus_1thread']:.2f}", "RATIO_CORPUS_PAR": f"{ratios['corpus_parallel']:.2f}",
 "NOOP_MS": f"{ms(noop_ox):.1f}", "STUB_MS": "@@STUB_MS@@",
 "HF_TABLE": HF_TABLE, "STAGES": STAGES, "CRITERION": CRITERION,
 "PROFILE_CORPUS": profile_block(ctxt), "PROFILE_CLI": profile_block(ltxt),
 "STAGES_LOAD": re.search(r"=== stages \(load1 ([0-9.]+)\)", open(os.path.join(E, "locked.log")).read()).group(1),
 "SNAPSHOTS": "```\n" + "\n".join(l for l in open(os.path.join(E, "locked.log")).read().splitlines() if re.match(r"^\[\w[\w ]* (before|after)\] ", l)) + "\n```",
 "LOCK_WAIT_S": str(json.loads(open(os.path.join(E, "locked.log.status.jsonl")).readline())["lock_wait_s"]),
 "OXIDEX_SHA": re.search(r"sha256=([0-9a-f]+)", open(os.path.join(E, "hyperfine.txt")).read()).group(1),
}
# stub ms comes from the benchmark refresh run if present
try:
    V["STUB_MS"] = f"{ms(next(r for r in json.load(open(os.path.join(ROOT, 'benches/benchmark_results.json')))['single_file']['results'] if 'target/release/oxidex' in r['command'])['median']):.1f}"
except Exception:
    pass
s = open(TPL).read()
for k, v in V.items():
    s = s.replace("@@" + k + "@@", str(v))
open(TPL, "w").write(s)
left = sorted(set(re.findall(r"@@\w+@@", s)))
print("unfilled:", left if left else "none")
