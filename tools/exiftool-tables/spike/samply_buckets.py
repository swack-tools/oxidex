#!/usr/bin/env python3
"""Bucket a samply (Firefox processed-profile JSON, gzip) by pipeline stage.

Three views over every sample of every thread:
  stage   -- inclusive ownership: a sample with ANY frame inside a generated
             engine (ifd/binary/serial/keyed, cond/exprs) is the engine's, since
             the engines are always reached through a hand parser and the
             question is how much the engines cost; otherwise, walking the
             stack ROOT->LEAF, the first frame matching a stage regex claims
             the sample (a malloc under Composite counts as "composite", JSON
             formatting under the CLI counts as "json").
  leaf    -- self time: the leaf frame's function (top N).
  alloc   -- share of samples whose LEAF is inside the allocator (malloc/free/
             realloc/_nanov2_/szone) regardless of stage.

Usage: samply_buckets.py profile.json.gz [--top N]
"""
import bisect, gzip, json, os, re, sys
from collections import Counter

STAGES = [
    ("json-output",   r"output_formatter|JsonNode|JsonFormatter|to_pretty_string|json_array_with"),
    ("composite",     r"oxidex::composite::"),
    ("fs+mmap+detect",r"file_metadata::|MMapReader|detection::|detect_format|filetype::|extract_file_metadata"),
    ("engine:ifd",    r"exiftool_tables::ifd_engine|exif_dir_engine|main_engine|ifd1_engine"),
    ("engine:binary", r"exiftool_tables::engine::|process_binary_data|subdirectory_adapter"),
    ("engine:serial", r"exiftool_tables::serial_engine|exiftool_tables::keyed_engine|exiftool_tables::fit"),
    ("engine:cond/exprs", r"exiftool_tables::cond|exiftool_tables::exprs|exiftool_tables::runtime"),
    ("tag_resolution",r"cli::tag_resolution|tag_resolution::"),
    ("map/sink insert", r"tag_sink::|metadata_map::|TagOccurrence|tag_occurrence::|interner"),
    ("hand parsers",  r"oxidex::parsers::|tiff_helpers|makernotes"),
    ("value/printconv", r"tag_value::|print_conv|value_conv|conversion|tag_db::|oxidex_tags"),
]
# Tier 2: claimed only when no tier-1 stage matched anywhere on the stack.
FALLBACK = [
    ("read pipeline (core::operations etc.)", r"oxidex::core::|read_metadata"),
    ("cli/main", r"oxidex::main|handle_|batch_processor|stages::main"),
]
FALLBACK_RE = [(n, re.compile(p)) for n, p in FALLBACK]
ALLOC_LIBS = ("libsystem_malloc.dylib",)
ALLOC_LEAF = re.compile(r"malloc|free|realloc|nanov2|szone|_platform_memmove|_platform_memset|memcpy", re.I)
STAGE_RE = [(n, re.compile(p)) for n, p in STAGES]

class Syms:
    """`samply record --unstable-presymbolicate` sidecar: per-lib symbol table."""
    def __init__(self, path):
        self.by_code_id = {}
        side = path[:-3] + ".syms.json" if path.endswith(".gz") else path + ".syms.json"
        if not os.path.exists(side):
            print(f"(no sidecar {side}; names will be raw addresses)", file=sys.stderr)
            self.strings = []
            return
        d = json.load(open(side))
        self.strings = d["string_table"]
        for lib in d["data"]:
            tab = sorted(lib["symbol_table"], key=lambda e: e["rva"])
            self.by_code_id[lib["code_id"].upper()] = ([e["rva"] for e in tab], tab)
    def name(self, code_id, rva, fallback):
        ent = self.by_code_id.get((code_id or "").upper())
        if not ent or rva is None: return fallback
        rvas, tab = ent
        i = bisect.bisect_right(rvas, rva) - 1
        if i >= 0 and rva < tab[i]["rva"] + tab[i]["size"]:
            return self.strings[tab[i]["symbol"]]
        return fallback

def main():
    path = sys.argv[1]
    top = int(sys.argv[sys.argv.index("--top") + 1]) if "--top" in sys.argv else 40
    d = json.load(gzip.open(path) if path.endswith(".gz") else open(path))
    syms = Syms(path)
    stage_c, leaf_c, alloc_c, total = Counter(), Counter(), Counter(), 0
    lib_c, incl_c, alloc_by_stage = Counter(), Counter(), Counter()
    for t in d["threads"]:
        strings = t["stringArray"]
        st, ft, fn = t["stackTable"], t["frameTable"], t["funcTable"]
        ress = fn["resource"]
        resources = t.get("resourceTable", {})
        libs = d.get("libs", [])
        def lib_of(func_idx):
            r = ress[func_idx]
            if r is None or r < 0: return None
            li = resources["lib"][r]
            return libs[li] if li is not None and li >= 0 else None
        def libname(func_idx):
            l = lib_of(func_idx); return l["name"] if l else "?"
        # Symbolicate each FRAME (not func): frame address is the lib-relative rva.
        frame_name = []
        for fi in range(ft["length"]):
            func = ft["func"][fi]
            raw = strings[fn["name"][func]]
            l = lib_of(func)
            frame_name.append(syms.name(l.get("codeId") or l.get("breakpadId", "")[:32] if l else None, ft["address"][fi], raw))
        cache = {}
        def frames_of(stack):
            if stack in cache: return cache[stack]
            chain = []
            s = stack
            while s is not None:
                chain.append(st["frame"][s]); s = st["prefix"][s]
            chain.reverse()  # root -> leaf
            cache[stack] = chain
            return chain
        cpu = t["samples"].get("threadCPUDelta")
        weights = t["samples"]["weight"] or [1] * t["samples"]["length"]
        for i, s in enumerate(t["samples"]["stack"]):
            if s is None: continue
            # Weight by CPU time (threadCPUDelta, us) so blocked/idle samples do
            # not count; fall back to sample weight.
            w = (cpu[i] if cpu and cpu[i] is not None else None)
            if w is None: w = weights[i] or 1
            if w <= 0: continue
            total += w
            chain = frames_of(s)
            funcs = [frame_name[f] for f in chain]
            leaf = funcs[-1]
            leaf_c[leaf] += w
            leaf_lib = libname(ft["func"][chain[-1]])
            lib_c[leaf_lib] += w
            is_alloc = leaf_lib in ALLOC_LIBS or ALLOC_LEAF.search(leaf) is not None
            if leaf_lib in ALLOC_LIBS: alloc_c["allocator (libsystem_malloc leaf)"] += w
            elif "memmove" in leaf or "memcpy" in leaf or "memset" in leaf: alloc_c["memmove/memcpy/memset leaf"] += w
            owner = None
            for f in reversed(funcs):  # leaf -> root: an engine frame anywhere wins
                hit = next((n for n, r in STAGE_RE if n.startswith("engine:") and r.search(f)), None)
                if hit: owner = hit; break
            for f in (funcs if owner is None else []):  # root -> leaf: outermost tier-1 stage claims it
                hit = next((n for n, r in STAGE_RE if r.search(f)), None)
                if hit: owner = hit; break
            if owner is None:
                for f in funcs:
                    hit = next((n for n, r in FALLBACK_RE if r.search(f)), None)
                    if hit: owner = hit; break
            owner = owner or "other/unattributed"
            stage_c[owner] += w
            if is_alloc: alloc_by_stage[owner] += w
            for f in set(funcs):  # inclusive time per distinct function
                if "oxidex" in f or "stages" in f: incl_c[f] += w
    print(f"total weight (thread CPU us if available, else samples): {total}")
    print("\n== stage (inclusive, outermost owner) ==")
    for k, v in stage_c.most_common(): print(f"{100*v/total:6.2f}%  {v:9.0f}  {k}")
    print("\n== allocator/memcpy leaf share (overlay, any stage) ==")
    for k, v in alloc_c.most_common(): print(f"{100*v/total:6.2f}%  {v:9.0f}  {k}")
    print("\n== allocator+memmove leaf time, by owning stage ==")
    for k, v in alloc_by_stage.most_common(): print(f"{100*v/total:6.2f}%  {v:9.0f}  {k}")
    print(f"\n== inclusive time per oxidex function, top {top} (a sample counts once per distinct function on its stack) ==")
    for k, v in incl_c.most_common(top): print(f"{100*v/total:6.2f}%  {v:9.0f}  {k[:160]}")
    print("\n== by library (leaf) ==")
    for k, v in lib_c.most_common(8): print(f"{100*v/total:6.2f}%  {v:9.0f}  {k}")
    print(f"\n== leaf self time, top {top} ==")
    for k, v in leaf_c.most_common(top): print(f"{100*v/total:6.2f}%  {v:9.0f}  {k[:150]}")

if __name__ == "__main__":
    main()
