#!/usr/bin/env python3
"""Generate `src/exiftool_tables/charset_tables.rs` from the pinned ExifTool
character-set modules (`Image/ExifTool/Charset.pm` and `Charset/*.pm`).

The v2 `Decode`/`Encode` port (`src/exiftool_tables/helpers.rs`,
`src/exiftool_tables/charset.rs`) reads these tables; they are never typed by
hand. `dump_charsets.pl` loads them with the pinned perl through
`Charset::LoadCharset` -- exactly how `Decompose`/`Recompose` load them -- and
this script renders the result, recording the sha256 of every source file so
the Rust test can tie the tables to the tree the oracle capture came from.

It also checks, and refuses to generate past, the two properties the Rust
port relies on instead of copying Perl's run-time behaviour:

- `Recompose` builds a destination's inverse table with
  `foreach $char (keys %$conv) { $inv{$$conv{$char}} = $char }`. Two bytes
  mapping to one code point would make that inverse depend on Perl's
  per-process hash order. Every destination-capable table (a type with 0x001
  and without 0x802) must therefore be injective on its scalar values.
- Charset.pm pre-loads the Latin inverse (`my %unicode2byte`) instead of
  building it; it must equal the inverse the generic rule would build, since
  the Rust port builds every inverse the generic way.

    python3 tools/exiftool-tables/codegen_charsets.py --write
    python3 tools/exiftool-tables/codegen_charsets.py --check

Instrument: the pinned perl (`$EXIFTOOL_PERL`) and tree
(`$OXIDEX_PINNED_EXIFTOOL`), asserted by `helper_oracle.instrument` (perl
v5.38.2, `-ver` against `.exiftool-version`, OOXML.docx -> DOCX).
"""
import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
DUMPER = HERE / "dump_charsets.pl"
OUT = REPO / "src" / "exiftool_tables" / "charset_tables.rs"


def charset_sources(et_lib):
    """{relative module path: sha256 of its raw bytes} for Charset.pm and every
    Charset/*.pm in the tree at `et_lib` (the directory holding `Image/`)."""
    base = Path(et_lib) / "Image" / "ExifTool"
    files = [base / "Charset.pm"] + sorted((base / "Charset").glob("*.pm"))
    return {str(f.relative_to(et_lib)): hashlib.sha256(f.read_bytes()).hexdigest()
            for f in files}


def perl_core(perl, env):
    """The pinned perl's installed CORE header directory."""
    arch = subprocess.run([perl, "-MConfig", "-e", "print $Config{archlibexp}"],
                          capture_output=True, text=True, env=env, check=True).stdout
    return Path(arch) / "CORE"


def perl_sources(core):
    """{header: sha256} for the pinned perl headers the UTF-8 port reads:
    perl.h (the DFA table) and inline.h (`Perl_utf8n_to_uvchr_msgs`)."""
    return {f"CORE/{h}": hashlib.sha256((core / h).read_bytes()).hexdigest()
            for h in ("perl.h", "inline.h")}


def strict_utf8_dfa(core):
    """`PL_strict_utf8_dfa_tab` (perl.h) as a list of ints: 256 byte classes,
    then the transition rows. `unpack('C0U*')` decodes through
    `Perl_utf8n_to_uvchr_msgs` (inline.h), whose fast path walks this table
    -- including from its reject state, which the Rust port must reproduce."""
    import re
    text = (core / "perl.h").read_text(encoding="latin-1")
    start = text.index("EXTCONST U8 PL_strict_utf8_dfa_tab[] = {")
    body = text[start:text.index("};", start)]
    body = body[body.index("{") + 1:]
    # The table's own `#define NUM_CLASSES` and `N0 0`, `Nk ((Nk-1) +
    # NUM_CLASSES)` sit between its class bytes and its transition rows.
    [classes] = [int(n) for n in re.findall(r"#\s*define NUM_CLASSES (\d+)", body)]
    nodes = dict(re.findall(r"#\s*define (N\d+)\s+(.+?)\s*$", body, re.M))
    if nodes.get("N0") != "0" or len(nodes) != 12 or any(
            not re.fullmatch(rf"\(\(N{k - 1}\)\s*\+ NUM_CLASSES\)", nodes[f"N{k}"])
            for k in range(1, 12)):
        sys.exit(f"perl.h: unexpected node definitions {nodes}")
    body = re.sub(r"/\*.*?\*/", " ", body, flags=re.S)
    body = "\n".join(l for l in body.splitlines() if not l.lstrip().startswith("#"))
    vals = []
    for tok in (t.strip() for t in body.split(",")):
        if not tok:
            continue
        m = re.fullmatch(r"N(\d+)", tok)
        vals.append(int(m.group(1)) * classes if m else int(tok))
    if len(vals) != 256 + 12 * classes or max(vals) > 255:
        sys.exit(f"perl.h: PL_strict_utf8_dfa_tab has {len(vals)} entries")
    return vals


def dump(perl, et_lib, env):
    proc = subprocess.run([perl, str(DUMPER), str(et_lib)], capture_output=True,
                          text=True, env=env)
    if proc.returncode != 0:
        sys.exit(f"dump_charsets.pl failed:\n{proc.stderr}")
    return json.loads(proc.stdout)


def destination_capable(cs_type):
    """Recompose builds an inverse for `$csType & 0x801` unless `& 0x802`."""
    return bool(cs_type & 0x001) and not cs_type & 0x802


def validate(d):
    for name, table in d["tables"].items():
        if not destination_capable(d["csType"][name]):
            continue
        seen = {}
        for key, val in table.items():
            if "u" not in val:
                sys.exit(f"{name}{{{key}}}: a destination table holds a non-scalar value")
            if val["u"] in seen:
                sys.exit(f"{name}: bytes {seen[val['u']]} and {key} both map to "
                         f"U+{val['u']:04X}; Recompose's inverse would depend on hash order")
            seen[val["u"]] = key
    for name, pre in d["unicode2byte"].items():
        inv = {str(v["u"]): int(k) for k, v in d["tables"][name].items()}
        if {k: int(v) for k, v in pre.items()} != inv:
            sys.exit(f"Charset.pm's pre-loaded %unicode2byte{{{name}}} differs from the "
                     f"inverse of %Image::ExifTool::Charset::{name}")


def rust_ident(name):
    return "CS_" + name.upper()


def conv(v):
    if "u" in v:
        return f"Conv::One({v['u']:#x})"
    if "a" in v:
        return "Conv::Many(&[" + ", ".join(f"{x:#x}" for x in v["a"]) + "])"
    inner = ", ".join(f"({int(k):#x}, {conv(x)})"
                      for k, x in sorted(v["h"].items(), key=lambda kv: int(kv[0])))
    return f"Conv::Lead(&[{inner}])"


def render(d, sources, exiftool_version, perl_srcs, dfa):
    lines = [
        "// @generated by tools/exiftool-tables/codegen_charsets.py; DO NOT EDIT.",
        "//! ExifTool's character-set tables (`%Image::ExifTool::Charset::csType` and each",
        "//! `%Image::ExifTool::Charset::<Name>` translation hash), loaded from the pinned",
        f"//! {exiftool_version} tree by `tools/exiftool-tables/dump_charsets.pl` and read by",
        "//! [`super::charset`]. Keys are sorted for binary search.",
        "",
        "use super::charset::{CharsetTable, Conv};",
        "",
        f"pub const EXIFTOOL_VERSION: &str = {json.dumps(exiftool_version)};",
        "",
        "/// sha256 of each source file's raw bytes, relative to the pinned `lib/`.",
        "#[rustfmt::skip]",
        "pub const SOURCES: &[(&str, &str)] = &[",
    ]
    lines += [f"    ({json.dumps(p)}, {json.dumps(h)})," for p, h in sorted(sources.items())]
    lines += ["];", "",
              "/// sha256 of the pinned perl's installed headers the UTF-8 decoder port reads.",
              "#[rustfmt::skip]",
              "pub const PERL_SOURCES: &[(&str, &str)] = &["]
    lines += [f"    ({json.dumps(p)}, {json.dumps(h)})," for p, h in sorted(perl_srcs.items())]
    lines += ["];", "",
              "/// `PL_strict_utf8_dfa_tab` (pinned perl `CORE/perl.h`): 256 byte classes,",
              "/// then the transition rows (`N<k>` = k * NUM_CLASSES).",
              "#[rustfmt::skip]",
              "pub const PERL_STRICT_UTF8_DFA_TAB: &[u8] = &["]
    lines += ["    " + " ".join(f"{v}," for v in dfa[i:i + 16]) for i in range(0, len(dfa), 16)]
    lines += ["];", "",
              "/// `%Image::ExifTool::Charset::csType`, sorted by name.",
              "#[rustfmt::skip]",
              "pub const CS_TYPE: &[(&str, u32)] = &["]
    lines += [f"    ({json.dumps(n)}, {t:#05x})," for n, t in sorted(d["csType"].items())]
    lines += ["];", ""]
    idents = {}
    for name in sorted(d["tables"]):
        ident = rust_ident(name)
        idents[name] = ident
        entries = sorted(d["tables"][name].items(), key=lambda kv: int(kv[0]))
        lines.append("#[rustfmt::skip]")
        lines.append(f"static {ident}: &[(u32, Conv)] = &[")
        row = []
        for k, v in entries:
            item = f"({int(k):#x}, {conv(v)}),"
            if "h" in v:
                # a lead byte and all its trail bytes: one line of its own
                if row:
                    lines.append("    " + " ".join(row))
                    row = []
                lines.append("    " + item)
                continue
            row.append(item)
            if sum(len(x) + 1 for x in row) > 88:
                lines.append("    " + " ".join(row))
                row = []
        if row:
            lines.append("    " + " ".join(row))
        lines.append("];")
        lines.append("")
    lines += ["/// Every translation table, sorted by charset name.",
              "#[rustfmt::skip]",
              "pub const TABLES: &[CharsetTable] = &["]
    lines += [f"    CharsetTable {{ name: {json.dumps(n)}, entries: {idents[n]} }},"
              for n in sorted(idents)]
    lines += ["];", ""]
    for name, pre in sorted(d["unicode2byte"].items()):
        lines += [f"/// Charset.pm's pre-loaded `%unicode2byte{{{name}}}` (code point -> byte);",
                  "/// the generator proves it equals the generic inverse of the table.",
                  "#[rustfmt::skip]",
                  f"pub const UNICODE2BYTE_{name.upper()}: &[(u32, u32)] = &["]
        items = sorted((int(k), v) for k, v in pre.items())
        for i in range(0, len(items), 6):
            lines.append("    " + " ".join(f"({k:#x}, {v:#x})," for k, v in items[i:i + 6]))
        lines += ["];", ""]
    return "\n".join(lines).rstrip("\n") + "\n"


def generated_tables(path=OUT):
    """Parse the generated file back into {name: (cs_type, {key: kind or
    {trail: kind}})} with kind "One"/"Many", plus {name: [(u, key)]} scalar
    values -- what the helper oracle's table-coverage probes are built from,
    so every generated entry is exercised against the pinned Perl."""
    import re
    text = Path(path).read_text(encoding="utf-8")
    cs = {n: int(t, 16) for n, t in
          re.findall(r'^    \("(\w+)", (0x[0-9a-f]+)\),$', text, re.M)
          if not n.startswith("Image/")}
    idents = dict(re.findall(r'CharsetTable \{ name: "(\w+)", entries: (\w+) \}', text))
    out, scalars = {}, {}
    for name, ident in idents.items():
        body = text[text.index(f"static {ident}: "):]
        body = body[:body.index("\n];")]
        entries, vals = {}, []
        for line in body.splitlines()[1:]:
            lead = re.match(r"    \((0x[0-9a-f]+), Conv::Lead\(&\[", line)
            items = re.findall(r"\((0x[0-9a-f]+), Conv::(One|Many)\(([^)]*)",
                               line[lead.end():] if lead else line)
            if lead:
                entries[int(lead.group(1), 16)] = {int(k, 16): kind for k, kind, _ in items}
            else:
                for k, kind, arg in items:
                    entries[int(k, 16)] = kind
                    if kind == "One":
                        vals.append((int(arg, 16), int(k, 16)))
        out[name] = (cs[name], entries)
        scalars[name] = vals
    return cs, out, scalars


def generate(perl, et_dir, env, exiftool_version):
    et_lib = Path(et_dir) / "lib"
    d = dump(perl, et_lib, env)
    validate(d)
    core = perl_core(perl, env)
    return render(d, charset_sources(et_lib), exiftool_version, perl_sources(core),
                  strict_utf8_dfa(core))


def main():
    import helper_oracle as H
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    mode = ap.add_mutually_exclusive_group(required=True)
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    ap.add_argument("--perl", default=os.environ.get("EXIFTOOL_PERL"))
    ap.add_argument("--exiftool-dir", default=os.environ.get("OXIDEX_PINNED_EXIFTOOL"))
    args = ap.parse_args()
    if not args.perl or not args.exiftool_dir:
        sys.exit("need --perl/--exiftool-dir (or EXIFTOOL_PERL/OXIDEX_PINNED_EXIFTOOL)")
    _, ver = H.instrument(args.perl, args.exiftool_dir)
    text = generate(args.perl, args.exiftool_dir, H.oracle_env(), ver)
    if args.write:
        OUT.write_text(text, encoding="utf-8")
        print(f"wrote {OUT.relative_to(REPO)}")
        return 0
    if OUT.read_text(encoding="utf-8") != text:
        print(f"MISMATCH: {OUT.relative_to(REPO)} is not what the pinned tree generates",
              file=sys.stderr)
        return 1
    print(f"PASS: pinned tree regenerates {OUT.relative_to(REPO)} byte for byte")
    return 0


if __name__ == "__main__":
    sys.exit(main())
