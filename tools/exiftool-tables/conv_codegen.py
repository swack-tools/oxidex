#!/usr/bin/env python3
r"""Autogeneration v2 backend: ExifTool conversions -> Rust `match` arms.

    python3 tools/exiftool-tables/conv_codegen.py <dump.json> \
        [--table Exif::Main] [-o src/exiftool_tables/conv/exif_main.rs] \
        [--ledger tools/exiftool-tables/conv_exif_main_ledger.json]

`<dump.json>` is `dump_tables.pl`'s output for the pinned tree (the same
document `codegen.py` reads). For every plain tag of the table this compiles
the field's `RawConv`, `ValueConv` and `PrintConv` -- strings, deparsed code
refs, hashes (with `BITMASK` / `OTHER`) and conversion lists -- into one Rust
arm over a `Session` (`docs/AUTOGENERATION-V2-DESIGN.md` sections 1-5).

Front end: the spike's grammar, `spike/perl_subset.py`, imported unchanged --
there is no second parser. Back end: this file, against the hand-written
runtime `src/exiftool_tables/conv/rt.rs` and the #824 helper library
`src/exiftool_tables/helpers.rs`.

Doctrine (AGENTS.md "Never approximate a conversion"): a field is GENERATED
only when every slot it has compiles; anything outside what this backend
models -- a production it does not emit, a helper with no proven port, an
eval-site lexical (`$tag`, `$tagInfo`), session state the caller cannot
supply -- REFUSES the whole field with the reason, recorded in the ledger
and in the emitted `REFUSED` table, and the field stays on the existing
path. Nothing is guessed. Every generated arm is then proven against the
pinned ExifTool by `conv_oracle.py`.

Regexes compile to `LazyLock<regex::bytes::Regex>` in `(?-u)` mode; no fold
to a native string operation is made (the design allows one only where
`verify_exprs` proves it, and none is claimed here).
"""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE / "spike"))
import perl_subset as P  # noqa: E402

LEDGER_SCHEMA = "oxidex_conv_ledger_v1"
DEFAULT_TABLE = "Exif::Main"
READ_SLOTS = ("RawConv", "ValueConv", "PrintConv")
REGISTRY_BEGIN = "// BEGIN GENERATED CONVERSION REGISTRY"
REGISTRY_END = "// END GENERATED CONVERSION REGISTRY"
_IFD_KEY = re.compile(r"^(?:0|[1-9][0-9]*)$")


@dataclass(frozen=True, order=True)
class RegistryEntry:
    module: str
    table: str
    stem: str

    @property
    def identity(self):
        return f"{self.module}::{self.table}"


def table_stem(module, table):
    """Stable Rust/file identity derived only from the ExifTool table identity."""
    if not module or not table:
        raise ValueError(f"conversion table identity requires module and table: {module!r}::{table!r}")
    stem = re.sub(r"[^a-z0-9]+", "_", f"{module}_{table}".lower()).strip("_")
    if not stem:
        raise ValueError(f"conversion table has no usable identity: {module!r}::{table!r}")
    return stem


def ifd_tag_id(key):
    """A typed IFD key, never a coercion of a named/indexed record key."""
    if not isinstance(key, str) or not _IFD_KEY.fullmatch(key):
        raise Refuse(f"non-IFD tag key {key!r}: typed key interface required")
    value = int(key)
    if value > 0xFFFF:
        raise Refuse(f"non-IFD tag key {key!r}: outside u16 IFD domain")
    return value


def _validated_registry(entries):
    entries = sorted(entries, key=lambda e: (e.module, e.table))
    identities = [e.identity for e in entries]
    duplicate_identities = sorted({x for x in identities if identities.count(x) > 1})
    if duplicate_identities:
        raise ValueError(f"duplicate conversion table identity: {duplicate_identities}")
    stems = [e.stem for e in entries]
    duplicate_stems = sorted({x for x in stems if stems.count(x) > 1})
    if duplicate_stems:
        raise ValueError(f"duplicate conversion module stem: {duplicate_stems}")
    return entries


def registry_with_candidate(entries, candidate):
    """Validate a generated table before its stable paths can overwrite an
    existing table with a colliding stem."""
    retained = [entry for entry in entries if entry.identity != candidate.identity]
    return _validated_registry([*retained, candidate])


def render_registry(entries):
    entries = _validated_registry(entries)
    lines = [REGISTRY_BEGIN]
    lines += [f"pub mod {entry.stem};" for entry in entries]
    if len(entries) == 1:
        entry = entries[0]
        lines += [
            "",
            "pub static REGISTRY: &[Entry] = &[Entry {",
            f"    module: {rust_str(entry.module)},",
            f"    table: {rust_str(entry.table)},",
            f"    decode: {entry.stem}::decode,",
            f"    claims: {entry.stem}::claims,",
            "}];",
            REGISTRY_END,
        ]
        return "\n".join(lines)
    lines += ["", "pub static REGISTRY: &[Entry] = &["]
    for entry in entries:
        lines += [
            "    Entry {",
            f"        module: {rust_str(entry.module)},",
            f"        table: {rust_str(entry.table)},",
            f"        decode: {entry.stem}::decode,",
            f"        claims: {entry.stem}::claims,",
            "    },",
        ]
    lines += ["];", REGISTRY_END]
    return "\n".join(lines)


def replace_registry(text, entries):
    generated = render_registry(entries)
    if REGISTRY_BEGIN not in text or REGISTRY_END not in text:
        raise ValueError("conv/mod.rs has no generated registry markers")
    before, rest = text.split(REGISTRY_BEGIN, 1)
    _old, after = rest.split(REGISTRY_END, 1)
    return before + generated + after


def discover_registry(root=REPO):
    """Discover enabled conversion outputs from per-table ledgers."""
    root = Path(root)
    conv_dir = root / "src/exiftool_tables/conv"
    ledger_dir = root / "tools/exiftool-tables"
    entries = []
    for path in sorted(ledger_dir.glob("conv_*_ledger.json")):
        data = json.loads(path.read_text(encoding="utf-8"))
        if data.get("schema") != LEDGER_SCHEMA:
            continue
        identity = data.get("table", "")
        if identity.count("::") != 1:
            raise ValueError(f"invalid conversion table identity in {path}: {identity!r}")
        module, table = identity.split("::")
        stem = table_stem(module, table)
        expected_ledger = ledger_dir / f"conv_{stem}_ledger.json"
        if path != expected_ledger:
            raise ValueError(f"stale conversion ledger name {path.name}; expected {expected_ledger.name}")
        if not (conv_dir / f"{stem}.rs").is_file():
            raise ValueError(f"missing conversion module {stem}.rs for {identity}")
        entries.append(RegistryEntry(module, table, stem))
    entries = _validated_registry(entries)
    owned = {e.stem for e in entries}
    reserved = {"mod", "rt", "tests"}
    on_disk = {p.stem for p in conv_dir.glob("*.rs")} - reserved
    orphan = sorted(on_disk - owned)
    if orphan:
        raise ValueError(f"orphan conversion module(s): {orphan}")
    return entries


def verify_emitted_registry(root=REPO):
    """Require the emitted generated block to be exactly the deterministic
    ledger-derived registry before any consumer trusts its identities."""
    root = Path(root)
    entries = discover_registry(root)
    if not entries:
        raise ValueError("generated conversion registry requires a nonempty ledger set")
    hub = root / "src/exiftool_tables/conv/mod.rs"
    if not hub.is_file():
        raise ValueError("generated conversion registry hub is missing")
    text = hub.read_text(encoding="utf-8")
    if text.count(REGISTRY_BEGIN) != 1 or text.count(REGISTRY_END) != 1:
        raise ValueError("generated conversion registry markers are missing or duplicated")
    start = text.index(REGISTRY_BEGIN)
    end = text.index(REGISTRY_END, start) + len(REGISTRY_END)
    actual = text[start:end]
    expected = render_registry(entries)
    if actual != expected:
        raise ValueError("generated conversion registry differs from ledger-derived rendering")
    return entries

# The #824 helper library, by fully qualified Perl sub: how the backend calls
# each port. Only subs listed in helpers.rs's PORTS may appear here (checked
# in `ported_helpers`). `kind` is the Rust call shape.
HELPER_CALLS = {
    "Image::ExifTool::Exif::ConvertExifText": "exif_text",
    "Image::ExifTool::Exif::DecodeCFAPattern": "self_val_mut",
    "Image::ExifTool::Exif::PrintCFAPattern": "unary_result",
    "Image::ExifTool::Exif::PrintSFR": "session_unary",
    "Image::ExifTool::ASF::GetGUID": "pure_scalar",
    "Image::ExifTool::Decode": "decode",
    "Image::ExifTool::Encode": "encode",
    "Image::ExifTool::Printable": "printable",
    "Image::ExifTool::IsFloat": "is_float",
    "Image::ExifTool::IsInt": "is_int",
    "Image::ExifTool::Exif::PrintExposureTime": "unary_result",
    "Image::ExifTool::Exif::PrintFNumber": "unary_result",
    "Image::ExifTool::Exif::PrintFraction": "unary_result",
    "Image::ExifTool::Exif::ConvertFraction": "unary_result",
    "Image::ExifTool::ConvertDuration": "unary_result",
    "Image::ExifTool::ConvertBitrate": "unary_result",
    "Image::ExifTool::XMP::ConvertXMPDate": "xmp_date",
    "Image::ExifTool::ConvertDateTime": "session_result",
}
RUST_NAMES = {
    "Image::ExifTool::Exif::ConvertExifText": "convert_exif_text",
    "Image::ExifTool::Exif::DecodeCFAPattern": "decode_cfa_pattern",
    "Image::ExifTool::Exif::PrintCFAPattern": "print_cfa_pattern",
    "Image::ExifTool::Exif::PrintSFR": "print_sfr",
    "Image::ExifTool::ASF::GetGUID": "asf_get_guid",
    "Image::ExifTool::Decode": "decode",
    "Image::ExifTool::Encode": "encode",
    "Image::ExifTool::Printable": "printable",
    "Image::ExifTool::IsFloat": "is_float",
    "Image::ExifTool::IsInt": "is_int",
    "Image::ExifTool::Exif::PrintExposureTime": "print_exposure_time",
    "Image::ExifTool::Exif::PrintFNumber": "print_f_number",
    "Image::ExifTool::Exif::PrintFraction": "print_fraction",
    "Image::ExifTool::Exif::ConvertFraction": "convert_fraction",
    "Image::ExifTool::ConvertDuration": "convert_duration",
    "Image::ExifTool::ConvertBitrate": "convert_bitrate",
    "Image::ExifTool::XMP::ConvertXMPDate": "convert_xmp_date",
    "Image::ExifTool::ConvertDateTime": "convert_date_time",
}
# Engine subs an expression may call that need no port: `GetByteOrder()`
# reads the session's byte order.
ENGINE_CALLS = {"Image::ExifTool::GetByteOrder"}
# `use Image::ExifTool qw(:DataAccess :Utils)` in the modules whose code refs
# appear here imports these into the caller's package (ExifTool.pm
# %EXPORT_TAGS); a bare call to one resolves to Image::ExifTool::<name>.
EXPORTED_UTILS = {"IsFloat", "IsInt", "GetByteOrder"}
# `$self->Options(NAME)` with one literal NAME, for these names only: each
# is an `@availableOptions` key, so `exists $$options{NAME}` holds and
# Options returns `$$options{NAME}` without its case-fixing search
# (ExifTool.pm `sub Options`). The arm reads `Session::option`, whose
# defaults `helpers::tests::session_option_defaults_match_the_pinned_perl`
# holds to `Image::ExifTool->new`.
OPTION_READS = {"CharsetEXIF"}

# Fields whose arm compiles and is proven but whose ownership cannot move in
# this backend alone: another producer reads the same id, and handing it to
# the engine means changing that producer in the same commit
# (`core::tiff_helpers::ifd1_tests::ifd1_residual_is_exactly_the_remainder`).
HAND_OWNED = {
    0x8298: "hand-owned: the IFD1 residual (core::tiff_helpers IFD1_RESIDUAL_IDS) also reads "
            "Copyright; generating it would insert it twice until that list moves with it",
}

# Why each field that stays refused stays refused, beyond the generator's
# first-hit reason: what it would take, checked against the pinned 13.59
# source and the static IFD table (`src/exiftool_tables/ifd/exif.rs`).
# Recorded in the ledger as `note`; a note for a field that is no longer
# refused is an error (`ledger`), so this cannot go stale silently.
REFUSAL_NOTES = {
    0x00FE: "RawConv calls $self->SetPriorityDir() (writes PRIORITY_DIR, the duplicate-tag "
            "priority directory, ExifTool.pm:9636) and reads/writes PageCount and MultiPage, "
            "per-file members no IFD walk carries across directories",
    0x8298: "its arm (Options('CharsetEXIF'), Decode, s/ *\\0/\\n/, s/\\n$//) compiles and "
            "matched the pinned Perl on every probe before it was withheld; it moves with "
            "the IFD1 residual list, IFD0-on-the-engine work",
    0x00FF: "same as SubfileType: SetPriorityDir, PageCount, MultiPage",
    0x0103: "RawConv calls Exif::IdentifyRawFile, which reads FILE_TYPE and IdentifiedRawFile "
            "and, for a TIFF file, calls OverrideFileType($$et{TIFF_TYPE} = ...): a file-type "
            "side effect. FILE_TYPE is not supplied to the walk yet (Session::for_ifd takes it "
            "from the caller's Condition members, which no caller seeds)",
    0x0111: "no IFD entry: codegen.py emits nothing for this id in ifd/exif.rs (every "
            "alternative is an offset/length pointer, IsOffset/OffsetPair; their Conditions "
            "read TIFF_TYPE/DIR_NAME/Compression/SubfileType), so the walk never resolves an "
            "alternative to convert",
    0x0117: "no IFD entry: as StripOffsets (0x0111), the byte-count half of the offset pairs",
    0x014A: "no IFD entry: SubIFD's Condition calls Image::ExifTool::Sony::SetARW and reads "
            "DIR_NAME/FILE_TYPE/Make/SubfileType/Compression; A100DataOffset is an offset",
    0x0201: "no IFD entry: offset alternatives whose Conditions read DIR_NAME/TIFF_TYPE/"
            "FILE_TYPE/PATH[-2] and call OverrideFileType (NRW detection)",
    0x0202: "no IFD entry: the length half of 0x0201's offset pairs",
    0x927C: "no IFD entry: the MakerNotes dispatch array (SubDirectory alternatives chosen by "
            "$$valPt/Make/Model; MakerNotePhaseOne's Condition calls OverrideFileType)",
    0x9287: "PrintConv reads Exif.pm's file-scoped lexical hashes %use and %ind (absent from "
            "the dump) and uses shift and a C-style for loop",
    0xA462: "RawConv loops over the value by byte offset with Get16u/GetRational64u/"
            "GetRational64s (while loop, declarations): grammar work, not a helper",
    0xC634: "the static table's variant group has no alternative with a conversion: all are "
            "SubDirectory edges except DNGPrivateData (Binary, no ValueConv/PrintConv), which "
            "the static path already reports",
    0xC740: "%opcodeInfo sets ConvertBinary => 1 (recorded only by name in the dump's "
            "_extra_keys): the PrintConv runs on the SCALAR reference, and its OTHER sub "
            "Exif::PrintOpcode dereferences $$val and unpacks x${pos}N4 records",
    0xC741: "as OpcodeList1: ConvertBinary, PrintOpcode over $$val",
    0xC74E: "as OpcodeList1: ConvertBinary, PrintOpcode over $$val",
    0xC763: "PrintConv maps hex over split, then a while loop of sprintf('%.2x') BCD and "
            "timezone arithmetic (declarations, while): grammar work, not a helper",
}


class Refuse(Exception):
    def __init__(self, reason):
        super().__init__(reason)
        self.reason = reason


def ascii_lit(s: str) -> str:
    """A Perl source literal as Rust text. ExifTool's modules have no `use
    utf8`: a non-ASCII literal is a byte string whose bytes the dump's JSON
    no longer pins down, so it refuses the field."""
    if any(ord(ch) > 0x7F for ch in s):
        raise Refuse("non-ASCII literal (its Perl bytes are not pinned by the dump)")
    return s


def rust_str(s: str) -> str:
    out = ['"']
    for ch in s:
        o = ord(ch)
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\r":
            out.append("\\r")
        elif o < 0x20 or o == 0x7F:
            out.append(f"\\x{o:02x}")
        elif o < 0x80:
            out.append(ch)
        else:
            out.append(f"\\u{{{o:x}}}")
    out.append('"')
    return "".join(out)


def rust_bytes(b: bytes) -> str:
    return 'b"' + "".join(
        chr(c) if 0x20 <= c < 0x7F and c not in (0x22, 0x5C) else f"\\x{c:02x}" for c in b
    ) + '"'


# ---------------------------------------------------------------------------
# Perl literals
# ---------------------------------------------------------------------------

def perl_num_literal(text: str) -> str:
    t = text.replace("_", "")
    if re.fullmatch(r"0[xX][0-9a-fA-F]+", t):
        v = int(t, 16)
    elif re.fullmatch(r"0[bB][01]+", t):
        v = int(t[2:], 2)
    elif re.fullmatch(r"0[0-7]+", t):
        v = int(t, 8)
    elif re.fullmatch(r"\d+", t):
        v = int(t)
    else:
        f = float(t)
        return f"rt::float({f!r}_f64)"
    if v > 2**63 - 1:
        raise Refuse(f"integer literal {text} beyond IV range")
    return f"rt::int({v})"


_ESC = {"n": "\n", "t": "\t", "r": "\r", "f": "\f", "e": "\x1b", "a": "\a", "0": "\0",
        "\\": "\\", '"': '"', "$": "$", "@": "@", "/": "/", "'": "'"}


def unescape_dq(body: str) -> str:
    """A double-quoted literal with NO interpolation: escapes resolved."""
    out, i = [], 0
    while i < len(body):
        c = body[i]
        if c == "\\" and i + 1 < len(body):
            n = body[i + 1]
            if n == "x":
                m = re.match(r"\\x\{([0-9a-fA-F]+)\}|\\x([0-9a-fA-F]{1,2})", body[i:])
                if not m:
                    raise Refuse("bad \\x escape")
                out.append(chr(int(m.group(1) or m.group(2), 16)))
                i += m.end()
                continue
            if n in _ESC:
                out.append(_ESC[n])
                i += 2
                continue
            if n.isalnum():
                raise Refuse(f"escape \\{n} not modelled")
            out.append(n)
            i += 2
            continue
        if c in "$@":
            raise Refuse("interpolation in a literal the backend treats as constant")
        out.append(c)
        i += 1
    return "".join(out)


def interp_pieces(body: str):
    """Split an interpolating string into ('lit', text) / ('var', name)."""
    pieces, lit, i = [], [], 0
    while i < len(body):
        c = body[i]
        if c == "\\" and i + 1 < len(body):
            j = i + 2
            if body[i + 1] == "x":
                m = re.match(r"\\x\{([0-9a-fA-F]+)\}|\\x([0-9a-fA-F]{1,2})", body[i:])
                if not m:
                    raise Refuse("bad \\x escape")
                lit.append(chr(int(m.group(1) or m.group(2), 16)))
                i += m.end()
                continue
            n = body[i + 1]
            if n in _ESC:
                lit.append(_ESC[n])
            elif n.isalnum():
                raise Refuse(f"escape \\{n} not modelled")
            else:
                lit.append(n)
            i = j
            continue
        if c == "@":
            raise Refuse("array interpolation")
        if c == "$":
            m = re.match(r"\$\{(\w+)\}|\$(\w+)", body[i:])
            if not m:
                raise Refuse("interpolation form not modelled")
            name = m.group(1) or m.group(2)
            nxt = body[i + m.end():i + m.end() + 2]
            sub = re.match(r"\[(\d+)\]", body[i + m.end():])
            if m.group(2) and sub:
                if lit:
                    pieces.append(("lit", "".join(lit)))
                    lit = []
                pieces.append(("elem", (name, int(sub.group(1)))))
                i += m.end() + sub.end()
                continue
            if m.group(2) and (nxt[:1] in ("[", "{") or nxt == "->" or nxt[:2] == "::"):
                raise Refuse("subscripted interpolation")
            if lit:
                pieces.append(("lit", "".join(lit)))
                lit = []
            pieces.append(("var", name))
            i += m.end()
            continue
        lit.append(c)
        i += 1
    if lit:
        pieces.append(("lit", "".join(lit)))
    return pieces


# ---------------------------------------------------------------------------
# Regex translation (Perl -> Rust regex crate, `(?-u)` bytes)
# ---------------------------------------------------------------------------

_CLASS_ESC_OK = set("sSdDwWtnrfe\\]-^[./()|*+?{}$@&~#: '\"")


def translate_regex(pat: str, flags: str, eol_ok: bool = False):
    """(rust pattern, uses `$` as `\\z`). Refuses what the regex crate cannot
    express identically. With `eol_ok`, a `$` that ENDS the pattern becomes
    the capture `(?P<eol>\\n?)\\z` (exact for a match and for a single
    substitution: `conv/rt.rs` module doc); any other `$` is `\\z`, exact for
    a subject that does not end in a newline (rt declines one that does)."""
    ascii_lit(pat)
    for f in flags:
        if f not in "isg":
            raise Refuse(f"regex flag /{f} not modelled")
    out, i, n, dollar, depth = [], 0, len(pat), False, 0
    while i < n:
        c = pat[i]
        if c == "\\":
            if i + 1 >= n:
                raise Refuse("trailing backslash in regex")
            e = pat[i + 1]
            if e == "0":
                if i + 2 < n and pat[i + 2] in "01234567":
                    raise Refuse("octal escape")
                out.append(r"\x00")
            elif e == "x":
                m = re.match(r"\\x\{([0-9a-fA-F]+)\}|\\x([0-9a-fA-F]{1,2})", pat[i:])
                if not m:
                    raise Refuse("bad \\x escape in regex")
                out.append("\\x%02x" % int(m.group(1) or m.group(2), 16))
                i += m.end()
                continue
            elif e in "sSdDwWbBtnrfAz":
                out.append("\\" + e)
            elif e.isdigit():
                raise Refuse("backreference")
            elif e.isalpha():
                raise Refuse(f"regex escape \\{e} not modelled")
            else:
                out.append("\\" + e if e in r".\/^$|()[]{}*+?-" else re.escape(e))
            i += 2
            continue
        if c == "[":
            j = i + 1
            cls = ["["]
            if j < n and pat[j] == "^":
                cls.append("^")
                j += 1
            if j < n and pat[j] == "]":
                cls.append(r"\]")
                j += 1
            while j < n and pat[j] != "]":
                d = pat[j]
                if d == "\\":
                    e = pat[j + 1] if j + 1 < n else ""
                    if e == "0":
                        cls.append(r"\x00")
                    elif e == "x":
                        m = re.match(r"\\x\{([0-9a-fA-F]+)\}|\\x([0-9a-fA-F]{1,2})", pat[j:])
                        if not m:
                            raise Refuse("bad \\x escape in class")
                        cls.append("\\x%02x" % int(m.group(1) or m.group(2), 16))
                        j += m.end()
                        continue
                    elif e in "sSdDwWtnrf":
                        cls.append("\\" + e)
                    elif e.isalnum():
                        raise Refuse(f"class escape \\{e} not modelled")
                    else:
                        cls.append("\\x%02x" % ord(e))
                    j += 2
                    continue
                if d == "[" and pat[j:j + 2] == "[:":
                    raise Refuse("POSIX class")
                if d in "[&~":
                    cls.append("\\" + d)
                elif d == "-" and cls[-1] == "-":
                    raise Refuse("'--' in class")
                else:
                    cls.append(d)
                j += 1
            if j >= n:
                raise Refuse("unterminated class")
            cls.append("]")
            out.append("".join(cls))
            i = j + 1
            continue
        if c == "$":
            rest = pat[i + 1:]
            if rest == "" and eol_ok:
                out.append(r"(?P<eol>\n?)\z")
                i += 1
                continue
            if rest == "" or rest[0] in ")|":
                out.append(r"\z")
                dollar = True
                i += 1
                continue
            raise Refuse("variable interpolation in regex")
        if c == "@":
            raise Refuse("array interpolation in regex")
        if c == "(":
            if pat[i + 1:i + 2] == "?":
                if pat[i + 2:i + 3] == ":":
                    out.append("(?:")
                    i += 3
                    depth += 1
                    continue
                raise Refuse("extended group (lookaround / inline flags)")
            out.append("(")
            depth += 1
            i += 1
            continue
        if c == "{":
            m = re.match(r"\{(\d+)(,(\d*))?\}", pat[i:])
            if m:
                out.append(m.group(0))
                i += m.end()
                continue
            out.append(r"\{")
            i += 1
            continue
        if c == "}":
            out.append(r"\}")
            i += 1
            continue
        if c == "#" or c.isspace():
            out.append(re.escape(c) if c != " " else " ")
            i += 1
            continue
        out.append(c)
        i += 1
    on = ("i" if "i" in flags else "") + ("s" if "s" in flags else "")
    prefix = f"(?{on}-u)"
    rust = prefix + "".join(out)
    return rust, dollar


# ---------------------------------------------------------------------------
# sprintf formats
# ---------------------------------------------------------------------------

def parse_format(fmt: str):
    pieces, i = [], 0
    for m in re.finditer(r"%([-0]*)(\d+)?(?:\.(\d+))?([a-zA-Z%])", fmt):
        if m.start() > i:
            pieces.append(("lit", fmt[i:m.start()]))
        flags, width, prec, conv = m.groups()
        if conv == "%":
            if flags or width or prec:
                raise Refuse("flags on %%")
            pieces.append(("lit", "%"))
        else:
            if conv == "i":
                conv = "d"
            if conv not in "sdfxX":
                raise Refuse(f"sprintf %{conv} not modelled")
            if "0" in flags and conv == "s":
                raise Refuse("zero flag on %s")
            pieces.append(("spec", "-" in flags, "0" in flags,
                           int(width) if width else None, int(prec) if prec else None, conv))
        i = m.end()
    if "%" in fmt[i:]:
        raise Refuse("sprintf format piece not modelled")
    if i < len(fmt):
        pieces.append(("lit", fmt[i:]))
    return pieces


def fmt_rust(pieces):
    parts = []
    for p in pieces:
        if p[0] == "lit":
            parts.append(f"rt::Fmt::Lit({rust_str(p[1])})")
        else:
            _, minus, zero, width, prec, conv = p
            w = f"Some({width})" if width is not None else "None"
            pr = f"Some({prec})" if prec is not None else "None"
            parts.append(f"rt::Fmt::Spec {{ minus: {str(minus).lower()}, zero: {str(zero).lower()}, "
                         f"width: {w}, prec: {pr}, conv: b'{conv}' }}")
    return "&[" + ", ".join(parts) + "]"


# ---------------------------------------------------------------------------
# The emitter
# ---------------------------------------------------------------------------

class Module:
    """Per generated file: shared statics (regexes, hash tables) and fns."""

    def __init__(self):
        self.statics = []          # rust source lines
        self.regex_ids = {}        # (pattern) -> static name
        self.fns = []              # rust fn sources
        self.helpers_used = set()  # qualified Perl subs called
        self.counter = 0

    def regex(self, rust_pat: str) -> str:
        if rust_pat not in self.regex_ids:
            name = f"RE_{len(self.regex_ids)}"
            self.regex_ids[rust_pat] = name
            self.statics.append(
                f"static {name}: LazyLock<Regex> = LazyLock::new(|| "
                f"Regex::new({rust_str(rust_pat)}).expect(\"generated regex compiles\"));")
        return self.regex_ids[rust_pat]

    def fresh(self, stem):
        self.counter += 1
        return f"{stem}_{self.counter}"


class Fn:
    """One compiled Perl body -> one Rust fn.

    `ret` is "out" (a conversion slot: returns `R<Out>`) or "scalar" (a hash
    `OTHER` sub or a list item: returns `R<MemberVal>`)."""

    def __init__(self, mod: Module, pkg: str, ret: str, slot: str, tag_name=None, form=None):
        self.mod, self.pkg, self.ret, self.slot = mod, pkg, ret, slot
        self.tag_name, self.form = tag_name, form
        self.uses_session = False
        self.lines = []
        self.locals = {}       # perl scalar name -> rust ident
        self.undef_locals = set()
        self.lists = {}        # perl array name -> rust ident
        self.reads = set()     # session members read
        self.writes = set()    # session members written
        self.params = None
        self.tmp = 0
        self.alias = {}        # perl scalar name -> rust lvalue (foreach aliasing)
        self.loops = 0         # enclosing foreach depth (`next`/`last` legal inside)
        self.conv_map = None   # the hash an OTHER sub's `$conv` names

    # -- helpers ----------------------------------------------------------
    def t(self):
        self.tmp += 1
        return f"t{self.tmp}"

    def sess(self):
        """The session parameter, marking the fn as reading it."""
        self.uses_session = True
        return "s"

    def wrap_ret(self, expr_out: str) -> str:
        return f"Ok({expr_out})"

    def qualify(self, name: str) -> str:
        if "::" in name:
            return name
        if self.pkg != "Image::ExifTool" and name in EXPORTED_UTILS:
            return "Image::ExifTool::" + name
        return f"{self.pkg}::{name}"

    # -- statements -------------------------------------------------------
    def body(self, stmts):
        stmts = [s for s in stmts if s[0] not in ("use",)]
        if not stmts:
            raise Refuse("empty body")
        for k, st in enumerate(stmts):
            last = k == len(stmts) - 1
            self.statement(st, last)

    def statement(self, st, last):
        while st[0] == "paren":
            st = st[1]
        kind = st[0]
        if kind == "package":
            self.pkg = st[1]
            if last:
                raise Refuse("package as the value")
            return
        if kind == "block":
            self.body(st[1])
            return
        if kind == "require_module":
            if last:
                raise Refuse("require as the value")
            return
        if kind == "return":
            self.lines.append(f"return {self.ret_value(st[1])};")
            return
        if kind == "if":
            self.if_stmt(st, last)
            return
        if kind == "modifier":
            _, mk, inner, cond = st
            if last or mk not in ("if", "unless"):
                raise Refuse(f"statement modifier {mk}")
            test = self.ex(cond)
            if mk == "unless":
                test = f"rt::not(&{test})"
            self.lines.append(f"if rt::truthy(&{test}) {{")
            self.statement(inner, False)
            self.lines.append("}")
            return
        if kind == "foreach":
            if last:
                raise Refuse("foreach as the value")
            self.foreach(st)
            return
        if kind == "bind" and st[3][0] in ("subst", "tr"):
            if last:
                raise Refuse("s/// or tr/// count as a value")
            self.mutate(st)
            return
        if kind == "assign" and st[2][0] == "decl":
            self.decl(st)
            if last:
                raise Refuse("declaration as the value")
            return
        if last:
            self.lines.append(f"{self.ret_value(st)}")
        else:
            self.lines.append(f"let _ = {self.ex(st)};")

    def ret_value(self, node):
        if node is None:
            raise Refuse("bare return")
        if self.ret == "out":
            return f"Ok({self.out(node)})"
        return f"Ok({self.ex(node)})"

    def if_stmt(self, st, last):
        if last:
            raise Refuse("if as the value")
        _, clauses, els = st
        # `if ($inv) {...}` where `$inv` is a parameter ExifTool always passes
        # as undef (GetValue calls OTHER as `($val, undef, $conv)`): the
        # clause is dead in every read, so only the rest is compiled.
        live = []
        for ckind, cond, block in clauses:
            c = cond
            while c[0] == "paren":
                c = c[1]
            if ckind in ("if", "elsif") and c[0] == "var" and c[1] == "$" \
                    and c[2] in self.undef_locals and self.locals.get(c[2], "x") is not None:
                continue
            live.append((ckind, cond, block))
        if not live:
            if els is not None:
                self.body_nested(els)
            return
        clauses = live
        first = True
        for ckind, cond, block in clauses:
            kw = "if" if first else "} else if"
            test = self.ex(cond)
            if ckind == "unless":
                test = f"rt::not(&{test})"
            elif ckind not in ("if", "elsif"):
                raise Refuse(f"{ckind} clause")
            self.lines.append(f"{kw} rt::truthy(&{test}) {{")
            self.body_nested(block)
            first = False
        if els is not None:
            self.lines.append("} else {")
            self.body_nested(els)
        self.lines.append("}")

    def body_nested(self, block):
        stmts = block[1] if block[0] == "block" else [block]
        for s in stmts:
            self.statement(s, False)

    def foreach(self, st):
        _, loopvar, init, block = st
        if loopvar != ("var", "$", "_"):
            raise Refuse("foreach with a named loop variable")
        src = init
        while src[0] == "paren":
            src = src[1]
        if not (src[0] == "var" and src[1] == "@" and src[2] in self.lists):
            raise Refuse("foreach over something other than a declared array")
        arr = self.lists[src[2]]
        i = self.t()
        saved = self.alias.get("_")
        self.alias["_"] = f"{arr}[{i}]"
        self.loops += 1
        self.lines.append(f"for {i} in 0..{arr}.len() {{")
        self.body_nested(block)
        self.lines.append("}")
        self.loops -= 1
        if saved is None:
            del self.alias["_"]
        else:
            self.alias["_"] = saved

    def target(self, node):
        """The rust ident a mutating operator writes back to."""
        if node[0] == "var" and node[1] == "$" and node[2] in self.alias:
            return self.alias[node[2]]
        if node[0] == "var" and node[1] == "$":
            name = node[2]
            if name == "val":
                return "val"
            if name in self.locals:
                return self.locals[name]
        raise Refuse("mutation of something other than a scalar local")

    def mutate(self, st):
        _, op, lhs, rhs = st
        if op != "=~":
            raise Refuse("!~ with s/// or tr///")
        dst = self.target(lhs)
        if rhs[0] == "subst":
            _, pat, repl, flags, _parsed = rhs
            if "e" in flags or "r" in flags:
                raise Refuse("s///e or s///r")
            if repl[0] != "interp" or repl[2]:
                raise Refuse("replacement with interpolation")
            text = unescape_dq(repl[1])
            self.lines.append(f"{dst} = {self.subst_call(dst, pat, flags, text)}.0;")
            return
        _, frm, to, flags = rhs
        if flags:
            raise Refuse("tr/// flags")
        f, t = ascii_lit(unescape_dq(frm)), ascii_lit(unescape_dq(to))
        if "-" in f.strip("-") or "-" in t.strip("-") or len(f) != len(t):
            raise Refuse("tr/// with a range or unequal lists")
        self.lines.append(
            f"{dst} = rt::tr(&{dst}, {rust_bytes(f.encode('latin-1'))}, "
            f"{rust_bytes(t.encode('latin-1'))})?;")

    def subst_call(self, dst, pat, flags, text):
        """`rt::subst(...)?`: an `R<(new value, substituted)>` expression."""
        g = "g" in flags
        rust_pat, dollar = translate_regex(pat, flags.replace("g", ""), eol_ok=not g)
        name = self.mod.regex(rust_pat)
        return (f"rt::subst(&{name}, {str(dollar).lower()}, &{dst}, {rust_str(ascii_lit(text))}, "
                f"{str(g).lower()})?")

    def decl(self, st):
        _, op, lhs, rhs = st
        if op != "=":
            raise Refuse("compound declaration")
        names = ["".join(n) if isinstance(n, (tuple, list)) else n for n in lhs[2]]
        if len(names) == 1 and names[0].startswith("@"):
            nm = names[0][1:]
            ident = f"l_{nm}"
            self.lists[nm] = ident
            self.lines.append(f"let mut {ident}: Vec<MemberVal> = {self.list_ex(rhs)};")
            return
        # parameter binding: my($val, ...) = @_ / my($val) = shift
        is_args = rhs == ("var", "@", "_") or (
            rhs[0] == "paren" and rhs[1] == ("call", "shift", [], "core")) or \
            rhs == ("call", "shift", [], "core")
        if is_args:
            if self.params is None:
                raise Refuse("argument binding outside a sub")
            scal = [n for n in names]
            for k, n in enumerate(scal):
                if not n.startswith("$"):
                    raise Refuse("non-scalar parameter")
                nm = n[1:]
                if rhs != ("var", "@", "_") and k > 0:
                    raise Refuse("shift binding more than one name")
                arg = self.params[k] if k < len(self.params) else None
                if arg == "val":
                    if nm != "val":
                        raise Refuse("value parameter not named $val")
                elif arg == "self":
                    if nm != "self":
                        raise Refuse("session parameter not named $self")
                elif arg == "conv":
                    self.undef_locals.add(nm)  # use refused in ex()
                    self.locals[nm] = None
                else:
                    self.undef_locals.add(nm)
            return
        if len(names) != 1 or not names[0].startswith("$"):
            raise Refuse("declaration form not modelled")
        nm = names[0][1:]
        ident = f"v_{nm}"
        self.lines.append(f"let mut {ident} = {self.ex(rhs)};")
        self.locals[nm] = ident

    # -- values -----------------------------------------------------------
    def out(self, node):
        """An `Out`-typed value: the one place a `\\$val` is allowed."""
        if node[0] == "paren":
            return self.out(node[1])
        if node[0] == "ternary":
            return (f"if rt::truthy(&{self.ex(node[1])}) {{ {self.out(node[2])} }} "
                    f"else {{ {self.out(node[3])} }}")
        if node[0] == "unop" and node[1] == "\\":
            inner = node[2]
            if inner[0] == "var" and inner[1] == "$":
                return f"Out::Binary({self.ex(inner)}.perl_bytes().into_owned())"
            raise Refuse("reference to something other than a scalar")
        return f"Out::Scalar({self.ex(node)})"

    def var(self, node):
        _, sig, name = node
        if sig == "@" and name in self.lists:
            # an array in scalar (numeric) context: its element count
            return f"rt::int({self.lists[name]}.len() as i64)"
        if sig != "$":
            raise Refuse(f"{sig}{name} in scalar context")
        if name in self.alias:
            return f"{self.alias[name]}.clone()"
        if name == "val":
            return "val.clone()"
        if name in self.undef_locals:
            if self.locals.get(name, "x") is None:
                raise Refuse("$conv (the conversion hash) read in OTHER")
            return "MemberVal::Undef"
        if name in self.locals:
            return f"{self.locals[name]}.clone()"
        if name == "_" and "_" in self.locals:
            return f"{self.locals['_']}.clone()"
        if name == "tag" and self.slot == "RawConv" and self.form == "str" and self.tag_name:
            # FoundTag's `my $tag = $$tagInfo{Name}` (ExifTool.pm), in scope
            # for a string RawConv's eval.
            return f"rt::string({rust_str(ascii_lit(self.tag_name))})"
        raise Refuse(f"lexical ${name} (eval-site context) not modelled")

    def member(self, key):
        if key in self.writes:
            raise Refuse("member read after write in one arm")
        self.reads.add(key)
        return f"member({self.sess()}, {rust_str(ascii_lit(key))})?"

    def ex(self, node):
        k = node[0]
        if k == "var":
            return self.var(node)
        if k == "num":
            return perl_num_literal(node[1])
        if k == "str":
            return f"rt::string({rust_str(ascii_lit(node[1]))})"
        if k == "interp":
            pieces = interp_pieces(node[1])
            if not pieces:
                return 'rt::string("")'
            acc = None
            for kind, v in pieces:
                if kind == "elem":
                    name, idx = v
                    if name not in self.lists:
                        raise Refuse("interpolated element of an undeclared array")
                    e = f"rt::index(&{self.lists[name]}, &rt::int({idx}))"
                    acc = e if acc is None else f"rt::concat(&{acc}, &{e})"
                    continue
                e = (f"rt::string({rust_str(ascii_lit(v))})" if kind == "lit"
                     else self.var(("var", "$", v)))
                acc = e if acc is None else f"rt::concat(&{acc}, &{e})"
            if len(pieces) == 1 and pieces[0][0] == "var":
                acc = f"MemberVal::from_bytes({acc}.perl_bytes().into_owned())"
            return acc
        if k == "paren":
            return self.ex(node[1])
        if k == "binop":
            return self.binop(node)
        if k == "unop":
            op, x = node[1], node[2]
            if op == "-":
                return f"rt::neg(&{self.ex(x)})?"
            if op in ("!", "not"):
                return f"rt::not(&{self.ex(x)})"
            raise Refuse(f"unary {op} not modelled")
        if k == "ternary":
            return (f"(if rt::truthy(&{self.ex(node[1])}) {{ {self.ex(node[2])} }} "
                    f"else {{ {self.ex(node[3])} }})")
        if k == "call":
            return self.call(node)
        if k == "method":
            return self.method(node)
        if k == "elem":
            base, br, key, _arrow = node[1], node[2], node[3], node[4]
            if br == "{" and base == ("deref", "%", ("var", "$", "self")) and key[0] == "str":
                return self.member(key[1])
            if br == "[" and base[0] == "var" and base[1] == "@" and base[2] in self.lists:
                return f"rt::index(&{self.lists[base[2]]}, &{self.ex(key)})"
            if br == "{" and base == ("var", "$", "conv") and self.conv_map is not None \
                    and "conv" in self.undef_locals:
                return f"rt::hash_get({self.conv_map}, &{self.ex(key)})"
            raise Refuse("subscript form not modelled")
        if k == "bind":
            _, op, lhs, rhs = node
            if rhs[0] == "regex":
                _, mk, pat, flags, _parsed = rhs
                if mk != "m" and mk != "":
                    raise Refuse(f"quote-like {mk} as a match")
                if "g" in flags:
                    raise Refuse("m//g")
                rust_pat, dollar = translate_regex(pat, flags, eol_ok=True)
                name = self.mod.regex(rust_pat)
                m = f"rt::re_match(&{name}, {str(dollar).lower()}, &{self.ex(lhs)})?"
                return m if op == "=~" else f"rt::not(&{m})"
            if rhs[0] == "subst" and op == "=~":
                # s/// for its value: 1 or PL_sv_no (non-global only).
                _, pat, repl, flags, _parsed = rhs
                if "e" in flags or "r" in flags or "g" in flags:
                    raise Refuse("s///e, s///r or s///g used for its value")
                if repl[0] != "interp" or repl[2]:
                    raise Refuse("replacement with interpolation")
                dst = self.target(lhs)
                nv, hit = self.t(), self.t()
                call = self.subst_call(dst, pat, flags, unescape_dq(repl[1]))
                return (f"{{ let ({nv}, {hit}) = {call}; {dst} = {nv}; "
                        f"rt::subst_count({hit}) }}")
            raise Refuse("s/// or tr/// used for its value")
        if k == "assign" and node[1] in (".=", "+=", "-=", "*="):
            _, op, lhs, rhs = node
            dst = self.target(lhs)
            fn = {".=": "concat", "+=": "add", "-=": "sub", "*=": "mul"}[op]
            v = self.t()
            return f"{{ let {v} = rt::{fn}(&{dst}, &{self.ex(rhs)}); {dst} = {v}.clone(); {v} }}"
        if k == "assign":
            _, op, lhs, rhs = node
            if op != "=":
                raise Refuse(f"assignment {op}")
            if lhs[0] == "elem" and lhs[1] == ("deref", "%", ("var", "$", "self")) \
                    and lhs[2] == "{" and lhs[3][0] == "str":
                if self.slot != "RawConv":
                    raise Refuse("member write outside RawConv")
                key = lhs[3][1]
                if key in self.reads:
                    raise Refuse("member write after read in one arm")
                self.writes.add(key)
                v = self.t()
                return (f"{{ let {v} = {self.ex(rhs)}; w.push(({rust_str(key)}, {v}.clone())); "
                        f"{v} }}")
            dst = self.target(lhs)
            v = self.t()
            return f"{{ let {v} = {self.ex(rhs)}; {dst} = {v}.clone(); {v} }}"
        if k == "return":
            return f"(return {self.ret_value(node[1])})"
        if k in ("next", "last"):
            if not self.loops or node[1] is not None:
                raise Refuse(f"{k} outside a foreach (or with a label)")
            return "(continue)" if k == "next" else "(break)"
        if k == "list":
            items = node[1]
            if not items:
                raise Refuse("empty list in scalar context")
            parts = [f"let _ = {self.ex(x)};" for x in items[:-1]]
            return "{ " + " ".join(parts) + f" {self.ex(items[-1])} }}"
        if k == "preinc":
            op, x = node[1], node[2]
            if op != "++":
                raise Refuse(f"pre{op}")
            dst = self.target(x)
            v = self.t()
            return f"{{ let {v} = rt::preinc(&{dst})?; {dst} = {v}.clone(); {v} }}"
        raise Refuse(f"AST node {k} not modelled")

    def binop(self, node):
        _, op, a, b = node
        arith = {"+": "add", "-": "sub", "*": "mul", "**": "pow", ".": "concat"}
        if op in arith:
            return f"rt::{arith[op]}(&{self.ex(a)}, &{self.ex(b)})"
        if op == "/":
            return f"rt::div(&{self.ex(a)}, &{self.ex(b)})?"
        cmps = {"<": "Lt", ">": "Gt", "<=": "Le", ">=": "Ge", "==": "Eq", "!=": "Ne"}
        if op in cmps:
            return f"rt::num_cmp(rt::Cmp::{cmps[op]}, &{self.ex(a)}, &{self.ex(b)})"
        scmps = {"lt": "Lt", "gt": "Gt", "le": "Le", "ge": "Ge", "eq": "Eq", "ne": "Ne"}
        if op in scmps:
            return f"rt::str_cmp(rt::Cmp::{scmps[op]}, &{self.ex(a)}, &{self.ex(b)})"
        bits = {"&": "band", "|": "bor", ">>": "shr", "<<": "shl"}
        if op in bits:
            if not (a[0] == "num" or b[0] == "num"
                    or (a[0] == "paren" and a[1][0] == "num")
                    or (b[0] == "paren" and b[1][0] == "num")):
                raise Refuse("bitwise op without a numeric-literal operand")
            return f"rt::{bits[op]}(&{self.ex(a)}, &{self.ex(b)})?"
        if op in ("&&", "and"):
            v = self.t()
            return f"{{ let {v} = {self.ex(a)}; if {v}.is_truthy() {{ {self.ex(b)} }} else {{ {v} }} }}"
        if op in ("||", "or"):
            v = self.t()
            return f"{{ let {v} = {self.ex(a)}; if {v}.is_truthy() {{ {v} }} else {{ {self.ex(b)} }} }}"
        raise Refuse(f"binary {op} not modelled")

    def list_ex(self, node):
        """A Perl list-context expression -> `Vec<MemberVal>`."""
        k = node[0]
        if k == "list":
            items = node[1]
            if len(items) == 1:
                return self.list_ex(items[0])
            parts = [self.list_ex(x) for x in items]
            return "{ let mut v = Vec::new(); " + "".join(f"v.extend({p}); " for p in parts) + "v }"
        if k == "paren":
            return self.list_ex(node[1])
        if k == "call" and node[1] == "split":
            args = node[2]
            if len(args) == 3 and args[2] == ("num", "0"):
                args = args[:2]
            if len(args) == 2 and args[0][0] == "regex" and args[0][1] in ("m", ""):
                _, _mk, pat, flags, _parsed = args[0]
                rust_pat, dollar = translate_regex(pat, flags)
                if dollar or re.search(r"(?<!\\)\((?!\?:)", pat):
                    raise Refuse("split pattern with an anchor or a capture group")
                name = self.mod.regex(rust_pat)
                return f"rt::split_re(&{name}, &{self.ex(args[1])})?"
            if len(args) != 2 or args[0] != ("str", " "):
                raise Refuse("split other than awk-mode ' '")
            return f"rt::split_ws(&{self.ex(args[1])})"
        if k == "mapgrep" and node[1] == "map":
            _, _, block, rest = node
            src = self.list_ex(rest)
            stmts = block[1]
            if len(stmts) != 1:
                raise Refuse("map block with statements")
            saved = self.locals.get("_")
            ident = self.t()
            self.locals["_"] = ident
            body = self.ex(stmts[0])
            if saved is None:
                del self.locals["_"]
            else:
                self.locals["_"] = saved
            return (f"{src}.into_iter().map(|{ident}: MemberVal| -> R<MemberVal> {{ Ok({body}) }})"
                    f".collect::<R<Vec<MemberVal>>>()?")
        if k == "var" and node[1] == "@" and node[2] in self.lists:
            return f"{self.lists[node[2]]}.clone()"
        if k == "list" or (k == "call" and node[1] == "qw"):
            raise Refuse("list form not modelled")
        return f"vec![{self.ex(node)}]"

    def call(self, node):
        _, name, args, kind = node
        if kind == "core":
            return self.core(name, args)
        q = self.qualify(name)
        if q in ENGINE_CALLS:
            if args:
                raise Refuse("GetByteOrder with arguments")
            return f"byte_order({self.sess()})?"
        if q not in HELPER_CALLS:
            raise Refuse(helper_refusal(q))
        return self.helper(q, args)

    def session_call(self, rust, args, n):
        """`helpers::<rust>(s, a0, .., a{n-1})` with `undef` for a missing
        trailing argument. Each argument is bound first, left to right as
        Perl evaluates them, so none borrows the session the call takes."""
        binds, refs = [], []
        for k in range(n):
            v = self.t()
            e = self.ex(args[k]) if k < len(args) else "MemberVal::Undef"
            binds.append(f"let {v} = {e};")
            refs.append(f"&{v}")
        return "{ " + " ".join(binds) + f" h(helpers::{rust}({self.sess()}, {', '.join(refs)}))? }}"

    def helper(self, q, args, with_self=False):
        shape = HELPER_CALLS[q]
        rust = RUST_NAMES[q]
        self.mod.helpers_used.add(q)
        if shape == "session_result":
            if not with_self or len(args) != 1:
                raise Refuse(f"{q} not called as a method with one argument")
            return f"h(helpers::{rust}({self.sess()}, &{self.ex(args[0])}))?"
        # `$self->Decode($val, $from, $fromOrder, $to, $toOrder)`,
        # `$self->Encode($val, $to, $toOrder)`, `$self->Printable($val, $max)`
        methods = {"decode": 5, "encode": 3, "printable": 2}
        if shape in methods:
            if not with_self:
                raise Refuse(f"{q} not called as a method")
            if not 1 <= len(args) <= methods[shape]:
                raise Refuse(f"{q} arity")
            return self.session_call(rust, args, methods[shape])
        if with_self:
            raise Refuse(f"{q} called as a method")
        is_self = lambda a: a == ("var", "$", "self")  # noqa: E731
        if shape == "exif_text":
            # ConvertExifText($self, $val [, $asciiFlex [, $tag]])
            if not 2 <= len(args) <= 4 or not is_self(args[0]):
                raise Refuse(f"{q} not called with $self first")
            return self.session_call(rust, args[1:], 3)
        if shape == "self_val_mut":
            if len(args) != 2 or not is_self(args[0]):
                raise Refuse(f"{q} not called as ($self, $val)")
            return self.session_call(rust, args[1:], 1)
        if shape == "session_unary":
            if len(args) != 1:
                raise Refuse(f"{q} arity")
            return self.session_call(rust, args, 1)
        if shape == "pure_scalar":
            if len(args) != 1:
                raise Refuse(f"{q} arity")
            return f"helpers::{rust}(&{self.ex(args[0])})"
        if shape == "is_float":
            if len(args) != 1:
                raise Refuse("IsFloat arity")
            a = args[0]
            if a[0] == "var" and a[1] == "$" and (
                    a[2] == "val" or a[2] in self.alias
                    or (a[2] in self.locals and self.locals[a[2]])):
                dst = self.target(a)
                r, nv = self.t(), self.t()
                return f"{{ let ({r}, {nv}) = helpers::is_float(&{dst}); {dst} = {nv}; {r} }}"
            if a[0] == "var" and a[1] == "$" and a[2] == "_" and "_" in self.locals:
                return f"helpers::is_float(&{self.ex(a)}).0"
            raise Refuse("IsFloat of an expression (its in-place rewrite has no target)")
        if shape == "is_int":
            if len(args) != 1:
                raise Refuse("IsInt arity")
            return f"helpers::is_int(&{self.ex(args[0])})"
        if shape == "unary_result":
            if len(args) != 1:
                raise Refuse(f"{q} arity")
            return f"h(helpers::{rust}(&{self.ex(args[0])}))?"
        if shape == "xmp_date":
            if len(args) == 1:
                return f"helpers::{rust}(&{self.ex(args[0])}, &MemberVal::Undef)"
            if len(args) == 2:
                return f"helpers::{rust}(&{self.ex(args[0])}, &{self.ex(args[1])})"
            raise Refuse("ConvertXMPDate arity")
        raise Refuse(f"helper shape {shape}")

    def method(self, node):
        _, obj, name, args = node
        if obj != ("var", "$", "self"):
            raise Refuse("method on something other than $self")
        if name == "Options":
            if len(args) != 1 or args[0][0] != "str" or args[0][1] not in OPTION_READS:
                raise Refuse("Options other than a read of " + "/".join(sorted(OPTION_READS)))
            return f"{self.sess()}.option({rust_str(args[0][1])})"
        q = "Image::ExifTool::" + name
        if q not in HELPER_CALLS:
            raise Refuse(helper_refusal(q))
        return self.helper(q, args, with_self=True)

    def core(self, name, args):
        one = {"length": "length", "abs": "abs", "uc": "uc", "lc": "lc", "defined": "defined"}
        if name in one:
            if len(args) != 1:
                raise Refuse(f"{name} arity")
            return f"rt::{one[name]}(&{self.ex(args[0])})"
        if name == "int":
            if len(args) != 1:
                raise Refuse("int arity")
            return f"rt::int_of(&{self.ex(args[0])})?"
        if name == "undef" and not args:
            return "MemberVal::Undef"
        if name == "hex":
            if len(args) != 1:
                raise Refuse("hex arity")
            return f"rt::hex(&{self.ex(args[0])})?"
        if name == "unpack":
            if len(args) == 2 and args[0] == ("str", "H*"):
                return f"rt::unpack_hex(&{self.ex(args[1])})"
            raise Refuse("unpack template not modelled")
        if name == "sprintf":
            if not args or args[0][0] not in ("str", "interp"):
                raise Refuse("sprintf with a non-literal format")
            f = args[0]
            text = f[1] if f[0] == "str" else unescape_dq(f[1])
            pieces = parse_format(text)
            nspec = sum(1 for p in pieces if p[0] == "spec")
            if nspec != len(args) - 1:
                raise Refuse("sprintf argument count differs from its format")
            argv = ", ".join(self.ex(a) for a in args[1:])
            return f"rt::sprintf({fmt_rust(pieces)}, &[{argv}])?"
        if name == "join":
            if len(args) < 2:
                raise Refuse("join arity")
            rest = args[1] if len(args) == 2 else ("list", args[1:])
            return f"rt::join(&{self.ex(args[0])}, &{self.list_ex(rest)})"
        raise Refuse(f"builtin {name} not modelled")


# ---------------------------------------------------------------------------
# Fields
# ---------------------------------------------------------------------------

def parse_slot_source(v):
    """(form, text) of a string/code slot."""
    if isinstance(v, str):
        return "str", v
    if isinstance(v, dict) and v.get("kind") == "expr" and isinstance(v.get("expr"), str):
        return "str", v["expr"]
    if isinstance(v, dict) and v.get("kind") == "code" and isinstance(v.get("deparse"), str):
        return "code", v["deparse"]
    return None, None


def compile_body(mod, fn_name, form, text, slot, ret, params, conv_map=None, tag_name=None):
    fn = Fn(mod, "Image::ExifTool", ret, slot, tag_name=tag_name, form=form)
    fn.conv_map = conv_map
    try:
        if form == "str":
            ast, _feat = P.parse_string_expr(text)
        else:
            ast, _feat = P.parse_code_ref(text)
            fn.params = params
    except P.Refuse as e:
        raise Refuse(f"outside the grammar: {e.reason}")
    stmts = ast[1]
    if form == "code":
        if len(stmts) != 1 or stmts[0][0] != "block":
            raise Refuse("code ref shape")
        stmts = stmts[0][1]
    fn.body(stmts)
    rtype = "R<Out>" if ret == "out" else "R<MemberVal>"
    uses_w = any("w.push" in ln for ln in fn.lines)
    wparam = "w" if uses_w else "_w"
    sparam = "s" if fn.uses_session else "_s"
    mut = "mut " if any(re.search(r"\bval = ", ln) for ln in fn.lines) else ""
    if ret == "out":
        sig = (f"fn {fn_name}({sparam}: &mut Session, {mut}val: MemberVal, "
               f"{wparam}: &mut Vec<(&'static str, MemberVal)>) -> {rtype}")
    else:
        if fn.uses_session or uses_w:
            raise Refuse("session access inside an OTHER / list-item sub")
        sig = f"fn {fn_name}({mut}val: MemberVal) -> {rtype}"
    src = sig + " {\n" + "\n".join(fn.lines) + "\n}\n"
    return src, fn


def compile_hash(mod, name, conv, tag, print_conv):
    """A hash conversion -> a `static rt::HashConv` and an adapter fn."""
    mp = conv.get("map") or {}
    for k, v in mp.items():
        if not isinstance(v, str):
            raise Refuse("hash value that is not a string")
    directives = conv.get("directives") or {}
    extra = sorted(set(directives) - {"BITMASK", "OTHER"})
    if extra:
        raise Refuse(f"hash directives not modelled: {extra}")
    for k, v in mp.items():
        ascii_lit(k)
        ascii_lit(v)
    entries = sorted(mp.items(), key=lambda kv: kv[0].encode("utf-8"))
    lines = [f"static {name}_MAP: &[(&str, &str)] = &["]
    lines += [f"    ({rust_str(k)}, {rust_str(v)})," for k, v in entries]
    lines.append("];")
    bitmask = "None"
    if "BITMASK" in directives:
        bm = directives["BITMASK"]
        if not isinstance(bm, dict):
            raise Refuse("BITMASK that is not a hash")
        pairs = []
        for k, v in bm.items():
            if not re.fullmatch(r"\d+", str(k)) or not isinstance(v, str):
                raise Refuse("BITMASK entry not modelled")
            pairs.append((int(k), ascii_lit(v)))
        pairs.sort()
        lines.append(f"static {name}_BITS: &[(i64, &str)] = &[" +
                     ", ".join(f"({k}, {rust_str(v)})" for k, v in pairs) + "];")
        bitmask = f"Some({name}_BITS)"
    other = "None"
    if "OTHER" in directives:
        o = directives["OTHER"]
        body = o.get("__deparse") if isinstance(o, dict) else None
        if not isinstance(body, str):
            raise Refuse("OTHER without a deparsed body")
        src, _fn = compile_body(mod, f"{name.lower()}_other", "code", body, "OTHER",
                                "scalar", ["val", "inv", "conv"], conv_map=f"{name}_MAP")
        mod.fns.append(src)
        mod.fns.append(f"fn {name.lower()}_other_ref(val: &MemberVal) -> R<MemberVal> {{ "
                       f"{name.lower()}_other(val.clone()) }}\n")
        other = f"Some({name.lower()}_other_ref)"
    bpw = tag.get("BitsPerWord")
    bpw_s = f"Some({int(bpw)})" if bpw is not None else "None"
    ph = "true" if print_hex(tag) else "false"
    lines.append(f"static {name}: rt::HashConv = rt::HashConv {{ map: {name}_MAP, "
                 f"bitmask: {bitmask}, bits_per_word: {bpw_s}, other: {other}, print_hex: {ph} }};")
    mod.statics.append("\n".join(lines))
    return f"rt::hash_conv(&val, &{name}, {str(print_conv).lower()})"


def flags_of(tag):
    f = tag.get("Flags")
    if f is None:
        return set()
    return {f} if isinstance(f, str) else set(f)


def truthy_key(tag, key):
    v = tag.get(key)
    return v not in (None, "", "0", 0) or key in flags_of(tag)


def print_hex(tag):
    return truthy_key(tag, "PrintHex")


def compile_slot(mod, tid, tag, slot, v):
    """The Rust fn for one slot; returns (fn name, description)."""
    base = f"{slot_prefix(slot)}_{tid:04x}"
    print_conv = slot == "PrintConv"
    if isinstance(v, dict) and v.get("kind") in ("enum", "enum_partial"):
        if slot == "RawConv":
            raise Refuse("hash RawConv")
        call = compile_hash(mod, base.upper(), v, tag, print_conv)
        mod.fns.append(
            f"fn {base}(_s: &mut Session, val: MemberVal, _w: &mut Vec<(&'static str, MemberVal)>) "
            f"-> R<Out> {{\nOk(Out::Scalar({call}?))\n}}\n")
        return base, "hash"
    if isinstance(v, dict) and v.get("kind") == "list":
        if slot == "RawConv":
            raise Refuse("list RawConv")
        items = v.get("items") or []
        names = []
        for i, item in enumerate(items):
            iname = f"{base}_item{i}"
            if item is None:
                names.append("None")
                continue
            if item == "REPEAT":
                raise Refuse("REPEAT in a conversion list")
            if isinstance(item, dict) and "kind" not in item:
                call = compile_hash(mod, iname.upper(), {"map": item}, tag, print_conv)
                mod.fns.append(f"fn {iname}(val: &MemberVal) -> R<MemberVal> {{\n"
                               f"let val = val.clone();\n{call}\n}}\n")
            else:
                form, text = parse_slot_source(item)
                if form is None:
                    raise Refuse("conversion-list item not modelled")
                src, _fn = compile_body(mod, f"{iname}_body", form, text, slot, "scalar", ["val"])
                mod.fns.append(src)
                mod.fns.append(f"fn {iname}(val: &MemberVal) -> R<MemberVal> {{ {iname}_body(val.clone()) }}\n")
            names.append(f"Some({iname} as fn(&MemberVal) -> R<MemberVal>)")
        mod.fns.append(
            f"fn {base}(_s: &mut Session, val: MemberVal, _w: &mut Vec<(&'static str, MemberVal)>) "
            f"-> R<Out> {{\nOk(Out::Scalar(rt::list_conv(&val, &[{', '.join(names)}], "
            f"{str(print_conv).lower()})?.unwrap_or(MemberVal::Undef)))\n}}\n")
        return base, "list"
    form, text = parse_slot_source(v)
    if form is None:
        raise Refuse(f"{slot} form not modelled")
    params = ["val", "self"]
    src, fn = compile_body(mod, base, form, text, slot, "out", params, tag_name=tag.get("Name"))
    mod.fns.append(src)
    return base, form


def slot_prefix(slot):
    return {"RawConv": "raw", "ValueConv": "vc", "PrintConv": "pc"}[slot]


def source_text(v):
    return json.dumps(v, sort_keys=True, ensure_ascii=False)


def compile_field(mod, tid, tag):
    """Rust source of one arm, or Refuse."""
    for key in ("Relist", "RawJoin", "ConvertBinary", "List"):
        # The dump records some keys only BY NAME in `_extra_keys` (their
        # value not serialized): `%opcodeInfo`'s `ConvertBinary => 1` reaches
        # OpcodeList1-3 that way. Presence there refuses as well.
        if truthy_key(tag, key) or key in (tag.get("_extra_keys") or []):
            raise Refuse(f"{key} not modelled")
    slots = {s: tag[s] for s in READ_SLOTS if tag.get(s) is not None}
    binary = truthy_key(tag, "Binary")
    fns = {}
    before = (len(mod.fns), len(mod.statics), dict(mod.regex_ids), set(mod.helpers_used))
    try:
        for s, v in slots.items():
            fns[s] = compile_slot(mod, tid, tag, s, v)
    except Refuse:
        # roll back anything the partial field emitted
        del mod.fns[before[0]:]
        del mod.statics[before[1]:]
        mod.regex_ids = before[2]
        mod.helpers_used = before[3]
        raise
    lines = [f"fn arm_{tid:04x}(s: &mut Session, raw: &MemberVal) -> R<Arm> {{"]
    lines.append("let mut w: Vec<(&'static str, MemberVal)> = Vec::new();")
    lines.append("let val = raw.clone();")
    changed = False
    if "RawConv" in fns:
        lines.append(f"let val = match {fns['RawConv'][0]}(s, val, &mut w)? {{")
        lines.append("Out::Scalar(MemberVal::Undef) => return Ok(Arm::Suppress),")
        lines.append("Out::Scalar(v) => v,")
        lines.append('Out::Binary(_) => return Err(Decline("RawConv returned a reference")),')
        lines.append("};")
        changed = True
    if "ValueConv" in fns:
        lines.append(f"let value = {fns['ValueConv'][0]}(s, val, &mut w)?;")
        changed = True
    elif binary:
        lines.append("let value = Out::Binary(val.perl_bytes().into_owned());")
        changed = True
    else:
        lines.append("let value = Out::Scalar(val);")
    lines.append("if value == Out::Scalar(MemberVal::Undef) { return Ok(Arm::Suppress); }")
    if "PrintConv" in fns:
        lines.append("let print = match &value {")
        lines.append(f"Out::Scalar(v) => match {fns['PrintConv'][0]}(s, v.clone(), &mut w)? {{")
        lines.append('Out::Scalar(MemberVal::Undef) => return Err(Decline("PrintConv returned undef")),')
        lines.append("p => Some(p),")
        lines.append("},")
        lines.append("Out::Binary(_) => None,")
        lines.append("};")
    else:
        lines.append("let print = None;")
    lines.append("Ok(Arm::Report(Report { value: " + ("Some(value)" if changed else "None")
                 + ", print, writes: w }))")
    lines.append("}")
    if not changed:
        lines[2] = "let val = raw.clone();"
    return "\n".join(lines) + "\n", {s: d for s, (_n, d) in fns.items()}, binary


def helper_refusal(q):
    """Why a call to `q` refuses the field: no proven port at all, or a port
    (helpers.rs PORTS) this backend has no call shape for."""
    if q in ported_helpers():
        return f"{q} is ported (helpers.rs) but has no call shape in HELPER_CALLS"
    return f"{q} has no proven port"


def ported_helpers():
    text = (REPO / "src" / "exiftool_tables" / "helpers.rs").read_text()
    block = text[text.index("pub const PORTS"):text.index("pub const REFUSED_HELPERS")]
    return set(re.findall(r'perl: "([^"]+)"', block))


def generate(dump_path, table_name):
    doc = json.loads(Path(dump_path).read_text(encoding="utf-8"))
    module, table = table_name.split("::")
    tbl = doc["modules"][module]["tables"][table]
    version = doc.get("exiftool_version")
    ported = ported_helpers()
    missing = sorted(set(HELPER_CALLS) - ported)
    if missing:
        raise SystemExit(f"HELPER_CALLS names subs helpers.rs does not port: {missing}")
    keyed_tags = []
    for key, tag in tbl["tags"].items():
        keyed_tags.append((ifd_tag_id(key), tag))
    mod = Module()
    arms, generated, refused, skipped = [], [], [], []
    hand_owned = HAND_OWNED if table_name == DEFAULT_TABLE else {}
    refusal_notes = REFUSAL_NOTES if table_name == DEFAULT_TABLE else {}
    for tid, tag in sorted(keyed_tags, key=lambda row: row[0]):
        if not isinstance(tag, dict):
            continue
        name = tag.get("Name") or "/".join(
            dict.fromkeys(v.get("Name", "?") for v in tag.get("_variants") or []))
        if tag.get("_variants"):
            refused.append(dict(id=tid, name=name, reason=(
                "_variants group: alternatives are chosen by the walker's compiled Condition "
                "(offset/pointer, SubDirectory and MakerNote dispatch); not a conversion arm")))
            continue
        if tag.get("SubDirectory"):
            skipped.append(dict(id=tid, name=name, reason="SubDirectory edge: walked, never reported"))
            continue
        if truthy_key(tag, "Unknown"):
            skipped.append(dict(id=tid, name=name, reason="Unknown => 1: reported only under -u"))
            continue
        if tid in hand_owned:
            refused.append(dict(id=tid, name=name, reason=hand_owned[tid],
                                slots=sorted(s for s in READ_SLOTS if tag.get(s) is not None)))
            continue
        try:
            src, slots, binary = compile_field(mod, tid, tag)
        except Refuse as e:
            refused.append(dict(id=tid, name=name, reason=e.reason,
                                slots=sorted(s for s in READ_SLOTS if tag.get(s) is not None)))
            continue
        arms.append((tid, name, src))
        generated.append(dict(id=tid, name=name, slots=slots, binary=binary,
                              print_hex=print_hex(tag),
                              source=({s: tag[s] for s in READ_SLOTS if tag.get(s) is not None}),
                              flags=sorted(k for k in ("Binary", "PrintHex") if truthy_key(tag, k))))
    table_sha = hashlib.sha256(
        json.dumps(tbl, sort_keys=True, ensure_ascii=False).encode("utf-8")).hexdigest()
    return dict(version=version, module=module, table=table, mod=mod, arms=arms, table_sha=table_sha,
                generated=generated, refused=refused, skipped=skipped,
                refusal_notes=refusal_notes)


def render_rust(g, dump_sha):
    mod = g["mod"]
    ids = [tid for tid, _n, _s in g["arms"]]
    out = [
        f"//! `Image::ExifTool::{g['module']}::{g['table']}` conversions, generated by",
        "//! `tools/exiftool-tables/conv_codegen.py` from the pinned ExifTool's own",
        f"//! source (ExifTool {g['version']}, table sha256 `{dump_sha[:16]}...`). Do not edit by",
        "//! hand. See `super` (conv/mod.rs) for what an arm is and how it is proven.",
        "//!",
        f"//! {len(g['generated'])} fields generated, {len(g['refused'])} refused "
        f"(`REFUSED`, with reasons), {len(g['skipped'])} not conversion fields",
        "//! (SubDirectory edges / `Unknown`).",
        "#![allow(clippy::all, clippy::pedantic, unused_parens, unused_braces, unused_mut, unused_variables, unreachable_code)]",
        "",
        "use std::sync::LazyLock;",
        "",
        "use regex::bytes::Regex;",
        "",
        "use super::rt::{self, Decline, Out, R};",
        "use super::{Arm, Report, finish};",
        "use crate::exiftool_tables::helpers::{self, HelperError};",
        "use crate::exiftool_tables::session::{MemberVal, Session};",
        "",
        f"/// ExifTool {g['version']}: the release these arms were generated from.",
        f"pub const EXIFTOOL_VERSION: &str = {rust_str(g['version'])};",
        "",
        "/// Tag ids with a generated arm, sorted.",
        "pub static CLAIMED: &[u16] = &[" + ", ".join(f"0x{i:04x}" for i in ids) + "];",
        "",
        "/// Fields refused by the backend: `(id, name, reason)`. The existing path",
        "/// keeps producing them.",
        "pub static REFUSED: &[(u16, &str, &str)] = &[",
    ]
    for r in g["refused"]:
        out.append(f"    (0x{r['id']:04x}, {rust_str(r['name'])}, {rust_str(r['reason'])}),")
    out += [
        "];",
        "",
        "/// Whether `id` has a generated arm.",
        "#[must_use]",
        "pub fn claims(id: u16) -> bool {",
        "    CLAIMED.binary_search(&id).is_ok()",
        "}",
        "",
        "/// Runs the arm for `id` on `$val` in the directory's session (a helper",
        "/// may set members there, as ExifTool's subs set them on `$self`).",
        "/// `Arm::Decline` for an unclaimed id, and for an entry whose conversion",
        "/// made a `$self->Warn` request: ExifTool then also reports a `Warning`",
        "/// tag -- and, for a Perl warning raised inside the conversion's eval,",
        "/// a second `\"ValueConv <tag>: ...\"` one -- which an arm does not model.",
        "#[must_use]",
        "pub fn decode(s: &mut Session, id: u16, val: &MemberVal) -> Arm {",
        "    let warned = s.warnings().len();",
        "    let arm = finish(match id {",
    ]
    for tid, _name, _src in g["arms"]:
        out.append(f"        0x{tid:04x} => arm_{tid:04x}(s, val),")
    out += [
        '        _ => Err(Decline("no generated arm for this id")),',
        "    });",
        "    if s.warnings().len() > warned {",
        '        return Arm::Decline("a conversion made a Warn request (the Warning tag is not modelled)");',
        "    }",
        "    arm",
        "}",
        "",
        "fn h(r: Result<MemberVal, HelperError>) -> R<MemberVal> {",
        "    r.map_err(|e| match e {",
        "        HelperError::Refused(why) => Decline(why),",
        '        HelperError::Dies(_) => Decline("helper dies in Perl (eval returns undef)"),',
        "    })",
        "}",
        "",
        "/// `$$self{key}`: a member the caller did not supply declines.",
        "fn member(s: &Session, key: &'static str) -> R<MemberVal> {",
        "    if s.has_member(key) {",
        "        Ok(s.member(key))",
        "    } else {",
        '        Err(Decline("session member not supplied by the caller"))',
        "    }",
        "}",
        "",
        "/// `GetByteOrder()`: the byte order of the directory being read.",
        "fn byte_order(s: &Session) -> R<MemberVal> {",
        "    s.byte_order",
        "        .map(|b| rt::string(b.as_perl()))",
        '        .ok_or(Decline("byte order not supplied by the caller"))',
        "}",
        "",
    ]
    out += mod.statics
    out.append("")
    for tid, name, src in g["arms"]:
        out.append(f"// 0x{tid:04x} {name}")
        out.append(src)
    out += mod.fns
    return "\n".join(out) + "\n"


def rustfmt(text):
    with tempfile.NamedTemporaryFile("w", suffix=".rs", delete=False) as fh:
        fh.write(text)
        path = fh.name
    subprocess.run(["rustfmt", "--edition", "2024", "--config-path", str(REPO / "rustfmt.toml"), path],
                   check=True)
    return Path(path).read_text()


def hash_keys(source):
    """Every key of every hash conversion a field carries: the oracle probes
    each one (a hash arm is otherwise only exercised on its Unknown path)."""
    keys = set()
    for v in source.values():
        if isinstance(v, dict) and v.get("kind") in ("enum", "enum_partial"):
            keys |= set((v.get("map") or {}).keys())
        if isinstance(v, dict) and v.get("kind") == "list":
            for item in v.get("items") or []:
                if isinstance(item, dict) and "kind" not in item:
                    keys |= set(item.keys())
    return sorted(keys)


def ledger(g, dump_sha, rust_sha):
    refusal_notes = g["refusal_notes"]
    stale = sorted(set(refusal_notes) - {r["id"] for r in g["refused"]})
    if stale:
        raise SystemExit(f"REFUSAL_NOTES names fields that are no longer refused: "
                         f"{[hex(i) for i in stale]}")
    return {
        "schema": LEDGER_SCHEMA,
        "tool": "tools/exiftool-tables/conv_codegen.py",
        "exiftool_version": g["version"],
        "table_sha256": dump_sha,
        "rust_sha256": rust_sha,
        "table": f"{g['module']}::{g['table']}",
        "counts": {"generated": len(g["generated"]), "refused": len(g["refused"]),
                   "not_conversion_fields": len(g["skipped"])},
        "helpers_called": sorted(g["mod"].helpers_used),
        "generated": [dict(id=f"0x{r['id']:04x}", name=r["name"], slots=r["slots"],
                           flags=r["flags"], hash_keys=hash_keys(r["source"]),
                           source_sha256=hashlib.sha256(
                               source_text(r["source"]).encode()).hexdigest())
                      for r in g["generated"]],
        "refused": [dict(id=f"0x{r['id']:04x}", name=r["name"], reason=r["reason"],
                         **({"note": refusal_notes[r["id"]]} if r["id"] in refusal_notes else {}))
                    for r in g["refused"]],
        "not_conversion_fields": [dict(id=f"0x{r['id']:04x}", name=r["name"], reason=r["reason"])
                                  for r in g["skipped"]],
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("dump")
    ap.add_argument("--table", default=DEFAULT_TABLE)
    ap.add_argument("-o", "--output")
    ap.add_argument("--ledger")
    ap.add_argument("--registry", default=str(REPO / "src/exiftool_tables/conv/mod.rs"))
    ap.add_argument("--check", action="store_true",
                    help="regenerate in memory and require the committed files to match")
    args = ap.parse_args()
    module, table = args.table.split("::")
    stem = table_stem(module, table)
    candidate = RegistryEntry(module, table, stem)
    registry_with_candidate(discover_registry(REPO), candidate)
    output = Path(args.output or REPO / f"src/exiftool_tables/conv/{stem}.rs")
    ledger_path = Path(args.ledger or HERE / f"conv_{stem}_ledger.json")
    registry_path = Path(args.registry)
    g = generate(args.dump, args.table)
    dump_sha = g["table_sha"]
    text = rustfmt(render_rust(g, dump_sha))
    rust_sha = hashlib.sha256(text.encode()).hexdigest()
    led = json.dumps(ledger(g, dump_sha, rust_sha), indent=1, sort_keys=True, ensure_ascii=False) + "\n"
    if args.check:
        entries = discover_registry(REPO)
        expected_registry = replace_registry(registry_path.read_text(), entries)
        ok = (output.read_text() == text and ledger_path.read_text() == led
              and registry_path.read_text() == expected_registry)
        print("PASS" if ok else "MISMATCH: committed conversion arms differ from a regeneration")
        return 0 if ok else 1
    output.write_text(text)
    ledger_path.write_text(led)
    entries = discover_registry(REPO)
    registry_path.write_text(replace_registry(registry_path.read_text(), entries))
    print(f"{args.table}: {len(g['generated'])} generated, {len(g['refused'])} refused, "
          f"{len(g['skipped'])} not conversion fields -> {output}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
