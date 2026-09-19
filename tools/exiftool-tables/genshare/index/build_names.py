"""Build class-names-eadb.json: (family-0 group, tag name) pairs each silence class can produce.

Adapted from work/build_names.py (afd3a628) for the probe tree at
/Users/allen/git/gen-share-probe (HEAD = probe commits on tip eadb5884).
Instrument: static parse of the tree (tables.json from parse_tables.py, plus
regex scrapes of the hand/vendor sources); read-only, no build, no oxidex or
ExifTool execution. Every call-site line number below is located by grep at
run time, never carried over from the afd3a628 index.

Engine class, what changed since afd3a628:
  * roots are every non-test `process_exif(` / `process_binary_data(` call
    site (asserted: an unmapped call site aborts the build);
  * the tables a root reaches are computed by walking the generated
    SubDirectory edges exactly as the engines do (ifd_engine.rs descend:
    skip `validate` / `unwalked`, IFD target first and only if walkable, else
    an enabled binary table; engine.rs descend: enabled binary tables only);
  * a caller that FENCES its rows (Exif IFD1, Canon::Main, FujiFilm::Main)
    contributes its root table's rows only;
  * a field is a pair iff the engine walk can report it: not a SubDirectory
    edge, not `flags.unknown` (ifd_engine.rs:726, ExifTool never returns an
    Unknown tag without -u), and no Omitted flag left after a resolved
    `_variants` alternative's `condition` is cleared.
"""
import collections, json, pathlib, re, subprocess, sys

# usage: build_names.py <probe tree root> <tables.json from parse_tables.py> <out class-names.json>
R = pathlib.Path(sys.argv[1])
S = R / "src"
OUT = pathlib.Path(sys.argv[3])
T = json.load(open(sys.argv[2]))
by = {(t["kind"], t["module"], t["table"]): t for t in T}


def git(*args):
    return subprocess.run(["git", "-C", str(R), *args], capture_output=True, text=True, check=True).stdout.strip()


HEAD = git("rev-parse", "--short=8", "HEAD")
HEAD_SUBJ = git("log", "-1", "--format=%s")
LOG = git("log", "--format=%h %s", "-4").splitlines()
DIRTY = [l for l in git("status", "--porcelain").splitlines() if not l.startswith("??")]
TIP = next((l.split()[0] for l in LOG if not l.split(" ", 1)[1].startswith("PROBE")), None)


def read(rel):
    return (R / rel).read_text(encoding="utf-8", errors="replace")


def test_lines(lines):
    """0-based indices inside any `#[cfg(test)]` item (rustfmt layout: the item
    closes at the next line that is its own indentation followed by `}`;
    a one-line item ends at its `;`)."""
    out = set()
    i = 0
    while i < len(lines):
        l = lines[i]
        if l.strip() == "#[cfg(test)]":
            ind = l[: len(l) - len(l.lstrip())]
            j = i + 1
            while j < len(lines) and (lines[j].strip().startswith("#[") or not lines[j].strip()):
                j += 1
            if j < len(lines) and lines[j].rstrip().endswith(";"):
                end = j
            else:
                end = j
                while end < len(lines) and lines[end].rstrip() != ind + "}":
                    end += 1
            out.update(range(i, end + 1))
            i = end + 1
            continue
        i += 1
    return out


def lines_of(rel, pattern, include_tests=False):
    ls = read(rel).splitlines()
    skip = set() if include_tests else test_lines(ls)
    rx = re.compile(pattern)
    return [i + 1 for i, l in enumerate(ls) if i not in skip and rx.search(l) and not l.strip().startswith("//")]


def one_line(rel, pattern):
    ln = lines_of(rel, pattern)
    if len(ln) != 1:
        raise SystemExit(f"{rel}: expected one non-test match for {pattern!r}, got {ln}")
    return ln[0]


def site(rel, pattern):
    ln = lines_of(rel, pattern)
    if not ln:
        raise SystemExit(f"{rel}: no non-test match for {pattern!r}")
    return f"{rel}:" + "/".join(map(str, ln))


def allow(rel, static):
    body = read(rel).split(f"pub static {static}", 1)[1].split("];", 1)[0]
    body = "\n".join(l for l in body.splitlines() if not l.strip().startswith("//"))
    return sorted(set(re.findall(r'\("([^"]+)",\s*"([^"]+)"\)', body)))


EN_B = set(allow("src/exiftool_tables/enabled.rs", "ENABLED"))
EN_I = set(allow("src/exiftool_tables/enabled_ifd.rs", "ENABLED_IFD"))


def enabled(t):
    lst = EN_B if t["kind"] == "binary" else EN_I
    return t["gate_a"] and (t["module"], t["table"]) in lst


for kind, lst in (("binary", EN_B), ("ifd", EN_I)):
    for m, tb in lst:
        t = by.get((kind, m, tb))
        if t is None or not t["gate_a"]:
            raise SystemExit(f"allowlisted {kind} {m}::{tb} missing from tables.json or blocked by Gate A")

# ---------------------------------------------------------------- engine
IFD_ENGINE = "src/exiftool_tables/ifd_engine.rs"
BIN_ENGINE = "src/exiftool_tables/engine.rs"
IFD_WALK_LINE = one_line(IFD_ENGINE, r"^\s{16}walk\($")          # descend -> Target::Ifd
IFD_TO_BIN_LINE = one_line(IFD_ENGINE, r"^\s+engine::walk\($")    # descend -> Target::Binary
IFD_PUSH = one_line(IFD_ENGINE, r"probe_silence::Class::Engine")
BIN_PUSH = one_line(BIN_ENGINE, r"probe_silence::Class::Engine")

ROOTS = [
    # kind, module, table, caller file, family-1 key prefix the caller writes, fence
    ("binary", "ICC_Profile", "Header", "src/parsers/icc/mod.rs", "ICC-header", None),
    ("binary", "H264", "RecInfo", "src/parsers/video/h264.rs", "H264", None),
    ("ifd", "Exif", "Main", "src/core/tiff_helpers.rs", "IFD1",
     "is_ifd1_exif_main_row: Exif::Main rows under group1 IFD1 only"),
    ("ifd", "Canon", "Main", "src/parsers/tiff/makernotes/canon/main_engine.rs", "Canon",
     "is_canon_main_row: Canon::Main rows under group1 Canon only"),
    ("ifd", "FujiFilm", "Main", "src/parsers/tiff/makernotes/fujifilm/main_engine.rs", "FujiFilm",
     "is_fuji_main_row: FujiFilm::Main rows under group1 FujiFilm only"),
    ("ifd", "Olympus", "Main", "src/parsers/tiff/makernotes/olympus.rs", "Olympus", None),
]
CALL_RX = r"\b(process_exif|process_binary_data)\($"
# Every non-test engine entry point must be a mapped root.
all_calls = subprocess.run(["rg", "-n", "--no-heading", r"\b(process_exif|process_binary_data)\(", "src",
                            "--glob", "!src/exiftool_tables/**"], cwd=R, capture_output=True, text=True).stdout
found = set()
for line in all_calls.splitlines():
    rel, ln, text = line.split(":", 2)
    if text.strip().startswith("//") or "use " in text:
        continue
    ls = read(rel).splitlines()
    if int(ln) - 1 in test_lines(ls):
        continue
    found.add(f"{rel}:{ln}")
root_sites = {}
for kind, m, tb, rel, pref, fence in ROOTS:
    root_sites[(m, tb)] = f"{rel}:{one_line(rel, CALL_RX)}"
if found != set(root_sites.values()):
    raise SystemExit(f"engine call sites not all mapped: found {sorted(found)} mapped {sorted(root_sites.values())}")


def targets(t):
    """(target table, via field) pairs the engine descends into from table t."""
    for f in t["fields"]:
        e = f["edge"]
        if not e or e["unwalked"] or e["validate"]:
            continue
        if t["kind"] == "ifd":
            ti = by.get(("ifd", e["module"], e["table"]))
            if ti is not None:
                if enabled(ti):
                    yield ti, f, IFD_WALK_LINE
                continue
        tb = by.get(("binary", e["module"], e["table"]))
        if tb is not None and enabled(tb):
            yield tb, f, (IFD_TO_BIN_LINE if t["kind"] == "ifd" else one_line(BIN_ENGINE, r"^\s+walk\($"))


def fam0(t, f):
    return f["g0"]


eng = collections.OrderedDict()   # (kind, module, table) -> record
fenced_away = []
for kind, m, tb, rel, pref, fence in ROOTS:
    root = by[(kind, m, tb)]
    if not enabled(root):
        raise SystemExit(f"root {m}::{tb} is not enabled")
    seen = {(kind, m, tb): []}
    queue = [root]
    while queue:
        cur = queue.pop(0)
        for tgt, f, walk_line in targets(cur):
            k = (tgt["kind"], tgt["module"], tgt["table"])
            if k in seen:
                continue
            seen[k] = seen[(cur["kind"], cur["module"], cur["table"])] + [
                f"{cur['module']}::{cur['table']} {('0x%04x' % f['id']) if 'id' in f and f['id'] is not None else 'index %s' % f.get('index')} "
                f"{f['name']} -> {IFD_ENGINE if cur['kind'] == 'ifd' else BIN_ENGINE}:{walk_line}"]
            queue.append(tgt)
    for k, path in seen.items():
        t = by[k]
        if fence and k != (kind, m, tb):
            fenced_away.append({"root": f"{m}::{tb}", "table": f"{k[1]}::{k[2]}", "reason": fence})
            continue
        rec = eng.setdefault(k, {"table": f"{k[1]}::{k[2]}", "kind": k[0], "engine_caller": [], "emit_prefix": pref,
                                 "fence": fence, "reached_via": [], "pairs": set()})
        caller = root_sites[(m, tb)] + (f" ({m}::{tb} root)" if not path else f" ({m}::{tb} root, descended)")
        rec["engine_caller"].append(caller)
        rec["reached_via"].extend(path)
        for f in t["fields"]:
            if f["reported"]:
                rec["pairs"].add((fam0(t, f), f["name"]))

# family-0 expectations (task spec): MakerNotes for vendor maker-note tables, EXIF for Exif::Main.
EXPECT_G0 = {"Exif": "EXIF", "Canon": "MakerNotes", "FujiFilm": "MakerNotes", "Olympus": "MakerNotes",
             "ICC_Profile": "ICC_Profile", "H264": "H264"}
for k, rec in eng.items():
    bad = {g for g, _ in rec["pairs"] if g != EXPECT_G0[k[1]]}
    if bad:
        raise SystemExit(f"{rec['table']}: family-0 {bad} != expected {EXPECT_G0[k[1]]}")

# Names the engine table reports that a same-vendor hand RESIDUAL arm also writes.
def names_at(t, ids):
    return {f["name"] for f in t["fields"] if f.get("id") in ids}


def parse_ids(rel, const):
    body = read(rel).split(const, 1)[1].split(";", 1)[0]
    return {int(x, 16) for x in re.findall(r"0x[0-9a-fA-F]+", body)}


residual_names = {}
residual_names[("ifd", "Canon", "Main")] = ("src/parsers/tiff/makernotes/canon/main_engine.rs CANON_MAIN_RESIDUAL_IDS",
    names_at(by[("ifd", "Canon", "Main")], parse_ids("src/parsers/tiff/makernotes/canon/main_engine.rs", "CANON_MAIN_RESIDUAL_IDS: &[u16] =")))
residual_names[("ifd", "FujiFilm", "Main")] = ("src/parsers/tiff/makernotes/fujifilm/main_engine.rs FUJI_MAIN_RESIDUAL_IDS",
    names_at(by[("ifd", "FujiFilm", "Main")], parse_ids("src/parsers/tiff/makernotes/fujifilm/main_engine.rs", "FUJI_MAIN_RESIDUAL_IDS: &[u16] =")))
# IFD1_RESIDUAL_IDS is spelled with TAG_* constants; resolve each against the tree.
ifd1_body = read("src/core/tiff_helpers.rs").split("const IFD1_RESIDUAL_IDS: &[u16] = &[", 1)[1].split("];", 1)[0]
consts = {}
for rel in ("src/core/tiff_helpers.rs", "src/core/tiff_tags.rs", "src/parsers/tiff/tags.rs"):
    p = R / rel
    if p.exists():
        consts.update({k: int(v, 16) for k, v in re.findall(r"const (TAG_\w+): u16 = (0x[0-9a-fA-F]+);", p.read_text(errors="replace"))})
missing_consts = [c for c in re.findall(r"TAG_\w+", ifd1_body) if c not in consts]
if missing_consts:
    out = subprocess.run(["rg", "-n", "--no-heading", r"const (%s): u16 = 0x" % "|".join(missing_consts), "src"],
                         cwd=R, capture_output=True, text=True).stdout
    consts.update({k: int(v, 16) for k, v in re.findall(r"const (TAG_\w+): u16 = (0x[0-9a-fA-F]+);", out)})
ifd1_ids = {consts[c] for c in re.findall(r"TAG_\w+", ifd1_body)}
# Four residual ids are absent from the static (their names come from the
# hand collector, per the doc comment above IFD1_RESIDUAL_IDS), plus the
# derived ThumbnailImage / ThumbnailTIFF / PreviewTIFF.
th = read("src/core/tiff_helpers.rs").split("const IFD1_RESIDUAL_IDS", 1)[0].splitlines()
doc = []
for l in reversed(th):
    if not l.strip().startswith("///"):
        if doc:
            break
        continue
    doc.insert(0, l.strip()[3:])
doc = " ".join(doc)
doc_names = set(re.findall(r"0x[0-9A-Fa-f]{4}\s+([A-Z]\w+)\b(?!\s+is NOT)", doc))
dm = re.search(r"derived ([\w /]+?) come from", doc)
if dm:
    doc_names |= {x.strip() for x in dm.group(1).split("/")}
if not {"ThumbnailOffset", "ThumbnailLength", "StripOffsets", "StripByteCounts"} <= doc_names:
    raise SystemExit(f"IFD1 residual doc scrape failed: {sorted(doc_names)}")
residual_names[("ifd", "Exif", "Main")] = ("src/core/tiff_helpers.rs IFD1_RESIDUAL_IDS (+ the absent-from-static names and derived thumbnails its doc comment lists)",
                                           names_at(by[("ifd", "Exif", "Main")], ifd1_ids) | doc_names)
oly_res = read("src/parsers/tiff/makernotes/olympus/tables.rs")
for statics, tb in ((("MAIN_RESIDUAL", "MAIN_INFO"), "Main"), (("EQUIPMENT_RESIDUAL",), "Equipment"),
                    (("CAMERA_SETTINGS_RESIDUAL",), "CameraSettings"), (("RAW_DEVELOPMENT_RESIDUAL",), "RawDevelopment"),
                    (("RAW_DEVELOPMENT2_RESIDUAL",), "RawDevelopment2"), (("IMAGE_PROCESSING_RESIDUAL",), "ImageProcessing"),
                    (("FOCUS_INFO_RESIDUAL",), "FocusInfo"), (("RAW_INFO_RESIDUAL",), "RawInfo")):
    names = set()
    for static in statics:
        body = oly_res.split(f"pub static {static}: &[TagDef] = &[", 1)[1].split("];", 1)[0]
        got = {a or b for a, b in re.findall(r'0x[0-9a-fA-F]+,\s*"([^"]+)"|\bname:\s*"([^"]+)"', body)}
        if not got:
            raise SystemExit(f"olympus residual {static}: scraped nothing")
        ndefs = len(re.findall(r"TagDef::\w+\(|TagDef \{", body))
        nnames = len(re.findall(r'TagDef::\w+\(0x[0-9a-fA-F]+,\s*"|\bname:\s*"', body))
        if ndefs != nnames:
            raise SystemExit(f"olympus residual {static}: {ndefs} TagDefs but {nnames} names scraped")
        names |= got
    src_desc = "src/parsers/tiff/makernotes/olympus/tables.rs " + "+".join(statics)
    if tb == "FocusInfo":
        # parse_focus_info_model_conditional: the hand arm writes the
        # alternatives of 0x0305/0x0308/0x031B the engine withholds.
        fi = read(MK_OLY := "src/parsers/tiff/makernotes/olympus.rs")
        body = fi.split("fn parse_focus_info_model_conditional(", 1)[1].split("\n}\n", 1)[0]
        extra = set(re.findall(r'emit\("([A-Za-z0-9_]+)"', body))
        if not extra:
            raise SystemExit("olympus FocusInfo model-conditional scrape found no emit() names")
        names |= extra
        src_desc += f" + {MK_OLY} parse_focus_info_model_conditional emit()s"
    residual_names[("ifd", "Olympus", tb)] = (src_desc, names)

# Approximate: engine names that also appear as a string literal in the same
# vendor's hand code (not the engine caller, not generated/by-script tables,
# not test regions). Catches hand producers the residual lists do not name
# (e.g. a hand sub-table decoder writing the same name); also catches
# comments-free mentions that are not producers -- flagged approx.
HAND_FILES = {
    "Canon": [p for p in (sorted((S / "parsers/tiff/makernotes/canon").glob("*.rs")) + [S / "parsers/tiff/makernotes/canon.rs"])
              if p.name not in ("main_engine.rs", "binary_tables.rs", "camera_info_tables.rs", "custom_functions2_tables.rs", "color_data.rs")],
    "FujiFilm": [p for p in (sorted((S / "parsers/tiff/makernotes/fujifilm").glob("*.rs")) + [S / "parsers/tiff/makernotes/fujifilm.rs"])
                 if p.name not in ("main_engine.rs", "settings_tables.rs")],
    "Olympus": [p for p in (sorted((S / "parsers/tiff/makernotes/olympus").glob("*.rs")) + [S / "parsers/tiff/makernotes/olympus.rs"])
                if p.name not in ("lookups.rs",)],
}


def hand_literal_hits(vendor, names):
    hits = collections.defaultdict(list)
    for p in HAND_FILES.get(vendor, []):
        rel = str(p.relative_to(R))
        ls = p.read_text(errors="replace").splitlines()
        skip = test_lines(ls)
        for i, l in enumerate(ls):
            if i in skip or l.strip().startswith("//"):
                continue
            for n in re.findall(r'"(?:%s:)?([A-Za-z0-9_]+)"' % vendor, l):
                if n in names:
                    hits[n].append(f"{rel}:{i + 1}")
    return {n: v[:4] for n, v in sorted(hits.items())}

engine_tables = []
for k, rec in eng.items():
    names = {n for _, n in rec["pairs"]}
    rn = residual_names.get(k)
    shared = sorted(names & rn[1]) if rn else []
    hand_hits = hand_literal_hits(k[1], names) if k[1] in HAND_FILES else None
    engine_tables.append({
        "table": rec["table"], "kind": rec["kind"],
        "engine_caller": "; ".join(dict.fromkeys(rec["engine_caller"])),
        "emit_prefix": rec["emit_prefix"], "fence": rec["fence"],
        "reached_via": list(dict.fromkeys(rec["reached_via"])),
        "row_push": f"{IFD_ENGINE}:{IFD_PUSH}" if rec["kind"] == "ifd" else f"{BIN_ENGINE}:{BIN_PUSH}",
        "residual_source": rn[0] if rn else None,
        "names_shared_with_residual_hand_arm": shared,
        "names_as_literals_in_same_vendor_hand_code_approx": hand_hits,
        "pairs": sorted(rec["pairs"]),
    })
engine_keys = set(eng)
cls = collections.OrderedDict()
cls["engine"] = {
    "tables": engine_tables,
    "enabled_but_via_legacy_api": sorted(f"{m}::{tb}" for m, tb in EN_B if ("binary", m, tb) not in engine_keys),
    "enabled_ifd_not_reached": sorted(f"{m}::{tb}" for m, tb in EN_I if ("ifd", m, tb) not in engine_keys),
    "reached_but_fenced_away": fenced_away,
}

# ---------------------------------------------------------------- legacy L1
L1 = {  # file -> [(module, table, emit prefix)]  (same tables as afd3a628; lines re-derived)
    "src/core/jpeg_helpers.rs": [("DJI", "ThermalParams2", "APP4")],
    "src/parsers/quicktime/metadata_extractor.rs": [("Pentax", "MOV", None)],
    "src/parsers/image/czi.rs": [("ZISRAW", "Main", "File")],
    "src/parsers/image/dpx.rs": [("DPX", "Main", "File")],
    "src/parsers/image/psp.rs": [("PSP", "Image", "PSP")],
    "src/parsers/image/pcx.rs": [("PCX", "Main", "File")],
    "src/parsers/image/pgf.rs": [("PGF", "Main", "File")],
    "src/parsers/image/photocd.rs": [("PhotoCD", "Main", None)],
    "src/parsers/image/jpeg2000.rs": [("Jpeg2000", x, "Jpeg2000") for x in ("FileType", "ImageHeader", "ColorSpec", "CaptureResolution", "DisplayResolution")],
    "src/parsers/image/pmp.rs": [("Sony", "PMP", "Sony")],
    "src/parsers/canon_vrd/ver2.rs": [("CanonVRD", "Ver2", "CanonVRD")],
    "src/parsers/specialized/mrc.rs": [("MRC", "Main", "File"), ("MRC", "FEI12", "File")],
    "src/parsers/tiff/makernotes/samsung/stmn.rs": [("Samsung", "Main", "Samsung")],
    "src/parsers/raw/metadata.rs": [("Canon", "CMP1", "Canon")]
        + [("CanonRaw", x, None) for x in ("ImageFormat", "ImageInfo", "FlashInfo", "ExposureInfo", "DecoderTable", "RawJpgInfo", "WhiteSample", "TimeStamp")]
        + [("Canon", "ColorBalance", None)],
    "src/parsers/raw/kyocera.rs": [("KyoceraRaw", "Main", "KyoceraRaw")],
    "src/parsers/tiff/makernotes/sony/amount.rs": [("Sony", x, "Sony") for x in ("CameraInfo", "Panorama", "ExtraInfo", "ExtraInfo2", "ExtraInfo3")],
    "src/parsers/specialized/palm.rs": [("Palm", "Main", "Palm"), ("Palm", "MOBI", "MOBI")],
    "src/parsers/specialized/itc.rs": [("ITC", "Header", "ITC"), ("ITC", "Item", "ITC")],
    "src/parsers/specialized/red.rs": [("Red", "RED1", "Red"), ("Red", "RED2", "Red")],
    "src/parsers/specialized/moi.rs": [("MOI", "Main", "MOI")],
    "src/parsers/font/pfm.rs": [("Font", "PFM", "Font")],
    "src/parsers/jpeg/app_parsers.rs": [("Casio", "QVCI", "Casio")],
    "src/parsers/audio/ape.rs": [("APE", "OldHeader", "APE"), ("APE", "NewHeader", "APE")],
    "src/parsers/audio/dss.rs": [("Olympus", "DSS", "Olympus")],
}
decode_files = set()
out = subprocess.run(["rg", "-l", r"decode_binary_table(_variants)?\(", "src", "--glob", "!src/exiftool_tables/**"],
                     cwd=R, capture_output=True, text=True).stdout.split()
decode_files = {f for f in out if lines_of(f, r"decode_binary_table(_variants)?\(")}
if decode_files != set(L1):
    raise SystemExit(f"L1 consumer set moved: new {sorted(decode_files - set(L1))} gone {sorted(set(L1) - decode_files)}")
l1 = []
for rel, tabs in L1.items():
    decode_lines = lines_of(rel, r"decode_binary_table(_variants)?\(")
    src_txt = read(rel)
    for m, tb, pref in tabs:
        # The decode call that serves this table: the first decode line after
        # its literal `find_table("M", "T")`; else after each variable-name
        # `find_table("M", name)` (a dispatcher passes the table name down);
        # else every decode line in the file.
        lit = lines_of(rel, r'find_table\(\s*"%s",\s*"%s"\s*\)' % (re.escape(m), re.escape(tb)))
        if not lit:
            lit = lines_of(rel, r'find_table\(\s*"%s",\s*[a-z_]+\s*\)' % re.escape(m))
        after = {next((d for d in decode_lines if d >= l), None) for l in lit} - {None}
        tdef = by.get(("binary", m, tb))
        if after and tdef and any(f["variant"] for f in tdef["fields"]):
            # a table with `_variants` is also read by the variants API
            after |= set(lines_of(rel, r"decode_binary_table_variants\("))
        s = f"{rel}:" + "/".join(map(str, sorted(after) or decode_lines))
        t = by.get(("binary", m, tb))
        if t is None:
            l1.append({"site": s, "table": f"{m}::{tb}", "error": "not found in binary_tables.rs"})
            continue
        if f'"{tb}"' not in src_txt:
            l1.append({"site": s, "table": f"{m}::{tb}", "error": f'no "{tb}" literal in {rel}'})
            continue
        fs = [f for f in t["fields"] if not f["subdir"]]
        l1.append({"site": s, "table": f"{m}::{tb}", "enabled": (m, tb) in EN_B, "emit_prefix": pref,
                   "emit_pairs": sorted({(f["g0"], f["name"]) for f in fs if not f["omitted"]}),
                   "rawaccess_only_pairs": sorted({(f["g0"], f["name"]) for f in fs if f["omitted"]})})

# ---------------------------------------------------------------- legacy L2 / L3 (regex scrapes)
def scrape(rel, g0, pref, approx=True, rx=r'\bname: "([A-Za-z0-9_\-]+)"', note=None):
    names = sorted(set(re.findall(rx, read("src/" + rel))))
    d = {"file": "src/" + rel, "group0": g0, "emit_prefix": pref, "approx": approx, "names": names}
    if note:
        d["note"] = note
    return d


MK = "src/parsers/tiff/makernotes/"
L2_SIL = {l: None for l in lines_of(MK + "shared/binary_subdir.rs", r"Class::LegacyL2")}
bsub = f"shared/binary_subdir.rs:{'/'.join(map(str, lines_of(MK + 'shared/binary_subdir.rs', r'Class::LegacyL2')))}"
fuji_via = "/".join(map(str, lines_of(MK + "fujifilm.rs", r"decode_binary_subdir\(")))
pana_via = "/".join(map(str, lines_of(MK + "panasonic.rs", r"decode_binary_subdir\(")))
pent_via = "/".join(map(str, lines_of(MK + "pentax.rs", r"decode_binary_subdir(_with)?\(")))
sony_bd = MK + "sony/binary_data.rs"
sony_l2 = "/".join(map(str, lines_of(sony_bd, r"Class::LegacyL2")))
sony_l3 = "/".join(map(str, lines_of(sony_bd, r"Class::LegacyL3")))
L2 = {
    f"codegen_subdirs(fujifilm) -> {bsub} via fujifilm.rs:{fuji_via}": scrape("parsers/tiff/makernotes/fujifilm/settings_tables.rs", "MakerNotes", "FujiFilm"),
    f"codegen_subdirs(panasonic) -> {bsub} via panasonic.rs:{pana_via}": scrape("parsers/tiff/makernotes/panasonic/face_tables.rs", "MakerNotes", "Panasonic"),
    f"codegen_subdirs(pentax) -> {bsub} via pentax.rs:{pent_via}": scrape("parsers/tiff/makernotes/pentax/subdir_tables.rs", "MakerNotes", "Pentax"),
    f"gen_canon_custom_functions2 -> {site(MK + 'canon/custom_functions2.rs', r'Class::LegacyL2')}": scrape("parsers/tiff/makernotes/canon/custom_functions2_tables.rs", "MakerNotes", "CanonCustom"),
    f"gen_infiray_tables -> {site('src/parsers/jpeg/app_segments/infiray.rs', r'Class::LegacyL2')} (+app8_isothermal.rs)": scrape("parsers/jpeg/app_segments/infiray_tables.rs", "APP2..APP9 (oracle -G0 on InfiRay.jpg)", "APPn:InfiRay"),
    f"gen_sony_main_extra_tables -> {site(MK + 'sony.rs', r'Class::LegacyL2')}": scrape("parsers/tiff/makernotes/sony/main_extra_tables.rs", "MakerNotes", "Sony"),
    f"gen_minolta_a100_tables -> {sony_bd}:{sony_l2} via minolta.rs:{'/'.join(map(str, lines_of(MK + 'minolta.rs', r'a100::idx::')))} (A100_SUBDIRS)": scrape("parsers/tiff/makernotes/minolta_a100_tables.rs", "MakerNotes", "Minolta"),
    f"gen_nikon_settings_tables -> {site(MK + 'nikon/settings.rs', r'Class::LegacyL2')} via nikon.rs:{'/'.join(map(str, lines_of(MK + 'nikon.rs', r'settings::parse_nikon_settings')))}":
        scrape("parsers/tiff/makernotes/nikon/settings_tables.rs", "MakerNotes", "Nikon",
               note="MOVED from L3 (afd3a628 index) to L2: settings_tables.rs is a managed tier-2 generated artifact since #744 "
                    "(tools/exiftool-tables/artifacts.py:55 Artifact('nikon-settings', 2, 'gen_nikon_settings_tables', ...)); "
                    "the probe gates it under Class::LegacyL2 since 5c96bc48 (nikon/settings.rs:303)."),
}
L3 = {
    f"sony/plain_tables.rs (no committed generator) -> {sony_bd}:{sony_l3} via sony.rs:{'/'.join(map(str, lines_of(MK + 'sony.rs', r'binary_data::process\(plain_tables')))}": scrape("parsers/tiff/makernotes/sony/plain_tables.rs", "MakerNotes", "Sony"),
    f"sony/enciphered_tables.rs (no committed generator) -> {sony_bd}:{sony_l3} via enciphered.rs / sony.rs:{'/'.join(map(str, lines_of(MK + 'sony.rs', r'enciphered::(is_root_tag|decode_root)')))}": scrape("parsers/tiff/makernotes/sony/enciphered_tables.rs", "MakerNotes", "Sony"),
    f"nikon/encrypted_tables.rs (no committed generator) -> {site(MK + 'nikon/binary_data.rs', r'Class::LegacyL3')}": scrape("parsers/tiff/makernotes/nikon/encrypted_tables.rs", "MakerNotes", "Nikon"),
    f"canon/binary_tables.rs (transcribed by script, no committed generator) -> {site(MK + 'canon/binary_tables.rs', r'Class::LegacyL3')}": scrape("parsers/tiff/makernotes/canon/binary_tables.rs", "MakerNotes", "Canon"),
    f"canon/camera_info_tables.rs (by script, no committed generator) -> {site(MK + 'canon/camera_info.rs', r'Class::LegacyL3')}, {site(MK + 'canon/filter_info.rs', r'Class::LegacyL3')}": scrape("parsers/tiff/makernotes/canon/camera_info_tables.rs", "MakerNotes", "Canon"),
    f"canon/color_data.rs (by script, no committed generator) -> {site(MK + 'canon/color_data.rs', r'Class::LegacyL3')}": scrape("parsers/tiff/makernotes/canon/color_data.rs", "MakerNotes", "Canon"),
    f"canon_vrd/ver1_table.rs (by script, no committed generator) -> {site('src/parsers/canon_vrd/mod.rs', r'Class::LegacyL3')}": scrape("parsers/canon_vrd/ver1_table.rs", "CanonVRD", "CanonVRD"),
}
cls["legacy"] = {"L1_runtime_decode_api": l1, "L2_manifest_vendor_walkers": L2, "L3_offmanifest_generated_origin_walkers": L3}

# ---------------------------------------------------------------- producers
comp = read("src/composite/tables.rs")
cnames = sorted(set(re.findall(r'Composite \{\s*name: "([^"]+)",\s*module: "([^"]+)"', comp)))
gc = sorted(set(re.findall(r'\("([^"]+)", "([^"]+)"\) =>', read("src/composite/generated_compute.rs"))))
dic = sorted(set(re.findall(r'e\("([^"]+)", b"', read("src/parsers/specialized/dicom_dict.rs"))))
gtxt = read("src/parsers/tiff/geotiff_printconv.rs")
gblock = gtxt.split("GEOKEY_NAMES", 1)[1] if "GEOKEY_NAMES" in gtxt else ""
geo = sorted(set(re.findall(r'\(\d+, "([A-Za-z0-9]+)"\)', gblock.split("];", 1)[0])))
fits = sorted(set(re.findall(r'\("[A-Z\-]+", "([A-Za-z]+)"\)', read("src/parsers/specialized/fits/tables.rs"))))
ident_sites = subprocess.run(["rg", "-n", "--no-heading", r"probe_silence::Class::Identity", "src"], cwd=R,
                             capture_output=True, text=True).stdout.splitlines()
ident_sites = sorted(":".join(l.split(":", 2)[:2]) for l in ident_sites)
cls["producers"] = {
    "file_identity": {"pairs": [["File", "FileType"], ["File", "FileTypeExtension"], ["File", "MIMEType"]],
                      "emission": ident_sites,
                      "note": "emission = every Class::Identity silence site in the probe tree; 5c96bc48 added the Mach-O and ar "
                              "File:MIMEType writes (generated filetype table via mime_for_type(\"EXE\")). Pairs unchanged."},
    "composite_declared": {"note": "generated Require/Desire/Inhibit/priority (composite/tables.rs); computation is hand (compute.rs) except the generated_compute arms",
                           "pairs": sorted({("Composite", n) for n, m in cnames}), "definitions": [f"{m}::{n}" for n, m in cnames]},
    "composite_generated_compute": {"pairs": sorted({("Composite", n) for m, n in gc}), "definitions": [f"{m}::{n}" for m, n in gc]},
    "lens_id": {"pairs": [["Composite", "LensID"]], "note": "lens_alternatives.rs (generated) consulted by lens_id.rs compute_primary; LensType label itself comes from hand/vendor lens_data"},
    "dicom": {"group0": "DICOM (oracle -G0 DICOM.dcm)", "pairs": [["DICOM", n] for n in dic]},
    "geotiff": {"group0": "GeoTiff (oracle -G0 GeoTiff.tif)", "pairs": [["GeoTiff", n] for n in geo], "note": "GeoTiffVersion is hand (geotiff_parser.rs) and excluded"},
    "fits_names": {"group0": "FITS (oracle -G0 FITS.fits)", "pairs": [["FITS", n] for n in fits], "note": "name map only; value is hand; a silenced lookup falls back to hand title-casing that yields the SAME name for single-word keywords"},
}

# ---------------------------------------------------------------- conv (not silenceable; index only)
cls["conv"] = {
    "exiftool_tables_printconv_lookups_by_hand_walkers": [
        {"site": site("src/parsers/audio/aiff.rs", r'find_table\("AIFF", "Common"\)'), "pairs": [["AIFF", "CompressionType"]]},
        {"site": site("src/parsers/audio/mp3.rs", r'find_table\("ID3", "v1"\)'), "pairs": [["ID3", "Genre"]], "note": "ID3 v1 Genre PrintConv applied to ID3v2 text frames"},
        {"site": site("src/parsers/jpeg/mpf_parser.rs", r'find_table\("MPF", "MPImage"\)'), "pairs": [["MPF", "MPImageFormat"], ["MPF", "MPImageType"]]},
        {"site": site("src/parsers/pe/metadata_extractor.rs", r'find_table\("EXE", "Main"\)'), "pairs": [["EXE", "MachineType"]]},
        {"site": site("src/parsers/image/bmp.rs", r"BMP_MAIN$"), "pairs": [["File", "BMPVersion"]]},
        {"site": site(MK + "canon.rs", r'find_table\("Canon", "FileInfo"\)') + " (decode_file_info_enum)", "pairs": [["MakerNotes", "RFLensType"]]},
        {"site": site(MK + "canon.rs", r'find_table\("Canon", "CameraSettings"\)'), "pairs": [["MakerNotes", "SRAWQuality"]]},
        {"site": site("src/composite/compute.rs", r'find_table\("Canon", "CameraSettings"\)') + " (canon_mode_print)", "pairs": [["Composite", "ShootingMode"]],
         "note": "NEW since afd3a628: Canon CameraSettings IntEnum PrintConv read by the hand Composite ShootingMode compute"}],
    "vendor_lookup_modules_by_hand_walkers": ["olympus/lookups.rs (gen_olympus_lookups)", "samsung/lookups.rs (gen_samsung_lookups)", "lens_data.rs (splice_leica, mixed)", "nikon/af_points.rs (codegen_af_points, mixed)", "geotiff_printconv.rs maps", "mac_charset/*.rs (value text decoding, no names)", "sony/main_table.rs PrintConv hashes (off-manifest)", "minolta_tables.rs PrintConv (off-manifest)", "qualcomm_tables.rs (constants + test fixture; names come from the hand port make_name)"],
    "note": "conv cannot be enumerated as a closed name set: these maps are consulted by hand walkers that also name the row; see scout.md for choke points."}


def tojson(o):
    if isinstance(o, (tuple, set)):
        return [tojson(v) for v in (sorted(o) if isinstance(o, set) else o)]
    if isinstance(o, dict):
        return {k: tojson(v) for k, v in o.items()}
    if isinstance(o, list):
        return [tojson(v) for v in o]
    return o


def count_pairs(x):
    s = set()

    def walk(o):
        if isinstance(o, dict):
            for k, v in o.items():
                if k in ("pairs", "emit_pairs", "rawaccess_only_pairs"):
                    s.update(tuple(p) for p in v)
                elif k == "names" and isinstance(v, list):
                    s.update((o.get("group0"), n) for n in v)
                else:
                    walk(v)
        elif isinstance(o, list):
            for v in o:
                walk(v)
    walk(x)
    return len(s)


out = {"_meta": {
    "commit": f"{HEAD} ({HEAD_SUBJ}) on tip {TIP}",
    "git_log": LOG,
    "tree_dirty_tracked_files": DIRTY,
    "instrument": "static parse of the probe tree's generated Rust statics (work-eadb/parse_tables.py -> tables.json, brace-matched) "
                  "and regex scrapes of hand/vendor sources (work-eadb/build_names.py); read-only, no build, no oxidex or ExifTool "
                  "execution. Call-site lines are grep-located at build time.",
    "pair_semantics": "[group0, name]: group0 is the family-0 group ExifTool reports (from the generated static's group0 / TagGroups.g0 where one exists; otherwise the family-0 observed with the pinned 13.59 oracle -G0:1 on the named t/images sample, or the MakerNotes convention for vendor MakerNote tables). emit_prefix is the key prefix oxidex's code writes (usually family-1), when it differs.",
    "engine_rule": "an engine pair is a field the walk can report: not a SubDirectory edge, not flags.unknown, no Omitted flag left after a resolved _variants alternative's condition is cleared (ifd_engine.rs walk / engine.rs walk). Fenced callers contribute the root table only. Against the plain not-Omitted/not-edge rule this drops only the Unknown-flagged tags (24 distinct pairs: Olympus::Main 13, FocusInfo 4, ImageProcessing 4, CameraSettings 1, Canon::Main 3 CRWParam/CanonFlashInfo/Flavor, Exif::Main SamsungRawUnknown), which ifd_engine.rs never pushes; no variant alternative changes.",
    "diff_vs_previous": "work-eadb/diff-afd3a628-vs-eadb.json, produced by work-eadb/diff_names.py ../class-names.json ../class-names-eadb.json (per-token counts via attribute.class_index)",
    "sanity_check": "work-eadb/sanity_check.py over oxidex-eadb5884 -j -G1 -a on t/images Canon/FujiFilm/Olympus/Nikon/ExifTool.jpg (outputs + report in work-eadb/sanity/)",
    "caveats": [
        "A name in a class is a CAN-produce bound, not an observed row: the same (group0,name) may also be produced by hand code (Canon/FujiFilm/Olympus/IFD1 residual arms -- listed per engine table as names_shared_with_residual_hand_arm -- other vendors' MakerNotes rows, Sony duplicate copies).",
        "Exif::Main is walked for IFD1 only, but its pairs are family-0 EXIF, which IFD0/ExifIFD/InteropIFD rows written by the hand TIFF walker share: a lost EXIF row is DIRECT for engine by name whichever IFD it came from.",
        "Family-0 MakerNotes cannot tell vendors apart: a (MakerNotes, X) pair from Canon::Main also matches a Nikon/Sony/Pentax MakerNotes X.",
        "Vendor generated-table name sets (legacy L2/L3) are regex scrapes of `name: \"...\"` literals and may include a few sub-table/struct names (flagged approx=true).",
        "Names reached through a generated table's SubDirectory edge from an ENABLED engine root are included only when the target is itself enabled (engine refuses others) and the caller does not fence it away."],
}}
out.update(tojson(cls))
out["_meta"]["distinct_pairs_per_class"] = {k: count_pairs(out[k]) for k in ("engine", "legacy", "producers", "conv")}
out["_meta"]["distinct_pairs_legacy_sub"] = {k: count_pairs(v) for k, v in out["legacy"].items()}
out["_meta"]["distinct_pairs_per_engine_table"] = {t["table"]: len(t["pairs"]) for t in out["engine"]["tables"]}
OUT.write_text(json.dumps(out, indent=1))
print("HEAD", HEAD, HEAD_SUBJ, "tip", TIP, "dirty", DIRTY)
print(json.dumps(out["_meta"]["distinct_pairs_per_class"]), json.dumps(out["_meta"]["distinct_pairs_legacy_sub"]))
for t in out["engine"]["tables"]:
    print(f"  {t['table']:32} {len(t['pairs']):4}  {t['engine_caller']}  via={t['reached_via']}  shared={t['names_shared_with_residual_hand_arm']}")
print("via legacy:", out["engine"]["enabled_but_via_legacy_api"], "ifd not reached:", out["engine"]["enabled_ifd_not_reached"],
      "fenced away:", out["engine"]["reached_but_fenced_away"])
print("L1 errors:", [x for x in l1 if "error" in x])
print("composites", len(cnames), "gen compute", gc, "dicom", len(dic), "geo", len(geo), "fits", len(fits))
print("identity sites", ident_sites)
