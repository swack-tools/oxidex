#!/usr/bin/env python3
"""Adversarial matrix: does an EXIF edit keep maker-note data that lies
OUTSIDE the MakerNote entry's byte count?

A MakerNote is one ExifIFD entry (0x927C) with a declared byte count, but
many vendors point from inside it to bytes outside it: Casio Type2's
PreviewImage (0x2000) and its 0x0003/0x0004 Start/Length pair, Nikon's
PreviewIFD, Pentax/Olympus/Samsung/Canon PreviewImageStart, Nikon's
NEFBitDepth running past the end of the note.  ExifTool rebuilds the maker
note and relocates that data (WriteExif.pl RebuildMakerNotes/PREVIEW_INFO);
a writer that re-lays the TIFF block out around a pinned MakerNote without
knowing about it overwrites the data with whatever it puts there.

Subcommands
-----------
``survey``  -- for each JPEG, every maker-note value the pinned oracle reads
               (``-v3`` hex-dump offsets) plus every ``IsOffset`` Start/Length
               target, classified against the TIFF block's own structures:
               ``hole`` (bytes no standard structure owns), ``claimed:<s>``,
               ``beyond-tiff``, ``straddles-tiff-end``, ``mn-overlap``.
``select``  -- a deterministic source list from a survey: every file with a
               known out-of-blob reference, capped per (vendor, class).
``matrix``  -- for each source x byte order (as-is, and flipped by an oracle
               EXIF rebuild) x carrier (JPEG; PNG with the EXIF wrapped into
               ``eXIf`` by the oracle) x edit (grow/shrink/same), apply the
               edit with the oracle and with oxidex and compare the oracle's
               read-back of EVERY maker-note tag (``-MakerNotes:all -b``,
               binary values hashed).  Offsets that ExifTool itself relocates
               (its ``IsOffset`` tags) are compared by the data they locate,
               not by value.

Cell verdicts: ``match`` (oxidex == oracle edit), ``match-orig`` (oxidex ==
the unedited source while the oracle's own edit changed a maker-note value),
``refused-untouched`` (oxidex exit != 0 and the file is byte-identical),
``refused-modified`` and ``corrupt`` (the two failures).  The goal is
``corrupt == 0`` and ``refused-modified == 0``.

The oracle is resolved and capability-probed through
``scripts/exiftool_oracle.py`` and the header names the binary, the commit and
the corpus (``scripts/instrument.py``), per AGENTS.md "Name the instrument".
"""

from __future__ import annotations

import argparse
import collections
import hashlib
import json
import re
import shutil
import struct
import subprocess  # nosec B404 -- list-argv only
import sys
import tempfile
import zlib
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts"))
import exiftool_oracle  # noqa: E402
import instrument  # noqa: E402

# ExifTool 13.59's IsOffset tags (every table, found by walking the loaded
# tag tables for `IsOffset`/`Flags => 'IsOffset'`): values ExifTool rewrites
# when it relocates the data they point at, so they are compared by the data
# they locate, not by value.  ZoomedPreviewStart is NOT IsOffset in 13.59
# (Olympus.pm:895 "not updated properly when the image is rewritten").
ISOFFSET = frozenset(
    "A100DataOffset AlphaOffset FreeOffsets HiddenDataOffset IDCPreviewStart "
    "ImageOffset JPEGACTables JPEGDCTables JPEGQTables JpgFromRawStart "
    "MPImageStart OriginalDecisionDataOffset OtherImageStart PreviewImageStart "
    "PreviewJXLStart RawDataOffset SamsungRawPointersOffset SharedData "
    "StripOffsets ThumbnailOffset TileOffsets".split()
)
TYPE_SIZE = {1: 1, 2: 1, 3: 2, 4: 4, 5: 8, 6: 1, 7: 1, 8: 2, 9: 4, 10: 8, 11: 4, 12: 8, 13: 4}
GROW = "D" * 3000
SAME_DATE = "2001:02:03 04:05:06"
SHRINK_CANDIDATES = (
    "IFD0:ImageDescription", "ExifIFD:UserComment", "IFD0:Software", "IFD0:Artist",
    "IFD0:Copyright", "ExifIFD:DateTimeOriginal", "ExifIFD:CreateDate", "IFD0:ModifyDate",
)


# --------------------------------------------------------------- oracle ---

class Oracle:
    def __init__(self, argv: list[str]):
        self.argv = argv

    def run(self, args: list[str], timeout: int = 180) -> subprocess.CompletedProcess:
        return subprocess.run(  # nosec B603
            [*self.argv, *args], capture_output=True, text=True, errors="replace", timeout=timeout
        )

    def json(self, args: list[str], path: str) -> dict:
        r = self.run(["-j", *args, path])
        try:
            return json.loads(r.stdout)[0]
        except (ValueError, IndexError):
            return {}


def resolve_oracle() -> tuple[Oracle, object]:
    oracle = exiftool_oracle.resolve_or_exit()
    pinned = exiftool_oracle.repo_pin()
    if oracle.version != pinned:
        sys.exit(f"❌ oracle is ExifTool {oracle.version}, .exiftool-version pins {pinned}")
    docx = Path(oracle.argv[-1]).parent / "t" / "images" / "OOXML.docx"
    try:
        oracle.check_container_support(docx)
    except exiftool_oracle.OracleError as exc:
        sys.exit(f"❌ oracle capability probe failed: {exc}")
    return Oracle([*oracle.argv, "-config", ""]), oracle


# ---------------------------------------------------------- tiff helpers ---

def jpeg_exif(data: bytes) -> tuple[int | None, bytes | None]:
    """(file offset of the TIFF header, TIFF bytes) of the first Exif APP1."""
    i = 2
    while i + 4 <= len(data) and data[i] == 0xFF:
        marker = data[i + 1]
        if marker in (0xD9, 0xDA):
            break
        length = struct.unpack(">H", data[i + 2:i + 4])[0]
        if marker == 0xE1 and data[i + 4:i + 10] == b"Exif\0\0":
            return i + 10, data[i + 10:i + 2 + length]
        i += 2 + length
    return None, None


def png_exif(data: bytes) -> tuple[int | None, bytes | None]:
    i = 8
    while i + 8 <= len(data):
        length = struct.unpack(">I", data[i:i + 4])[0]
        if data[i + 4:i + 8] == b"eXIf":
            return i + 8, data[i + 8:i + 8 + length]
        i += 12 + length
    return None, None


def carrier_exif(path: str) -> tuple[int | None, bytes | None]:
    data = Path(path).read_bytes()
    return png_exif(data) if data.startswith(b"\x89PNG") else jpeg_exif(data)


def structure(tiff: bytes) -> tuple[list[tuple[str, int, int]], tuple[int, int] | None]:
    """Byte ranges the standard TIFF structures own, and the (first
    ExifIFD) MakerNote range. :func:`makernotes_of` lists every note."""
    owned, notes = _walk_structure(tiff)
    exif = [r for d, r in notes if d == "ExifIFD"]
    return owned, (exif[0] if exif else None)


def makernotes_of(tiff: bytes) -> list[tuple[str, tuple[int, int]]]:
    """Every MakerNote (0x927C) range of the block, in any directory the
    reader accepts one in (IFD0 before ExifIFD, as ``-v3`` prints them)."""
    return _walk_structure(tiff)[1]


def _walk_structure(tiff: bytes) -> tuple[list[tuple[str, int, int]], list[tuple[str, tuple[int, int]]]]:
    e = "<" if tiff[:2] == b"II" else ">"
    u16 = lambda o: struct.unpack(e + "H", tiff[o:o + 2])[0]  # noqa: E731
    u32 = lambda o: struct.unpack(e + "I", tiff[o:o + 4])[0]  # noqa: E731
    owned: list[tuple[str, int, int]] = [("header", 0, 8)]
    notes: list[tuple[str, tuple[int, int]]] = []

    def walk(off: int, name: str) -> tuple[dict, int | None] | None:
        if off + 2 > len(tiff):
            return None
        count = u16(off)
        owned.append((name + "-table", off, off + 2 + 12 * count + 4))
        ptrs: dict = {}
        for k in range(count):
            p = off + 2 + 12 * k
            if p + 12 > len(tiff):
                break
            tag, typ, cnt, val = u16(p), u16(p + 2), u32(p + 4), u32(p + 8)
            if tag in (0x8769, 0x8825, 0xA005) and name in ("IFD0", "ExifIFD"):
                ptrs[tag] = val
                continue
            size = TYPE_SIZE.get(typ, 1) * cnt
            if size > 4:
                owned.append((f"{name}:0x{tag:04x}", val, val + size))
                if tag == 0x927C and not any(d == name for d, _ in notes):
                    notes.append((name, (val, val + size)))  # the first per directory
            if name == "IFD1" and tag in (0x201, 0x202):
                ptrs[tag] = val
        nxt = u32(off + 2 + 12 * count) if off + 2 + 12 * count + 4 <= len(tiff) else None
        return ptrs, nxt

    first = walk(u32(4), "IFD0")
    if first:
        ptrs, nxt = first
        if 0x8769 in ptrs:
            exif = walk(ptrs[0x8769], "ExifIFD")
            if exif and 0xA005 in exif[0]:
                walk(exif[0][0xA005], "InteropIFD")
        if 0x8825 in ptrs:
            walk(ptrs[0x8825], "GPS")
        if nxt:
            ifd1 = walk(nxt, "IFD1")
            if ifd1 and 0x201 in ifd1[0] and 0x202 in ifd1[0]:
                owned.append(("thumbnail", ifd1[0][0x201], ifd1[0][0x201] + ifd1[0][0x202]))
    order = {"IFD0": 0, "ExifIFD": 1, "InteropIFD": 2, "GPS": 3, "IFD1": 4}
    notes.sort(key=lambda n: order.get(n[0], 9))
    return owned, notes


# ----------------------------------------------------------------- survey ---

TAG_LINE = re.compile(r"^[\s|]*- Tag (0x[0-9a-f]+) \((\d+) bytes")
HEX_LINE = re.compile(r"^[\s|]*([0-9a-f]{4,8}): ")


def value_targets(et: Oracle, path: str) -> list[tuple[str, str, int, int]]:
    """(name, tag, file offset, size) of every >4-byte maker-note value -v3 dumps."""
    lines = et.run(["-v3", path]).stdout.splitlines()
    out, depth, seen_header, i = [], None, False, 0
    while i < len(lines):
        line = lines[i]
        if depth is None:
            if ("(SubDirectory) -->" in line and "MakerNote" in line and i + 1 < len(lines)
                    and "Tag 0x927c" in lines[i + 1]):
                depth = line.count("|")
                seen_header = False
                i += 2
                continue
        else:
            # the subtree ends: look on for another MakerNote directory
            ended = line.count("|") <= depth and re.match(r"^[\s|]*\d+\)", line)
            if line.count("|") <= depth and re.match(r"^[\s|]*\+ \[", line):
                if seen_header:
                    ended = True
                seen_header = True
            if ended:
                depth = None
                continue
            m = TAG_LINE.match(line)
            if m and line.count("|") > depth and i + 1 < len(lines):
                h = HEX_LINE.match(lines[i + 1])
                if h and int(m.group(2)) > 4:
                    name = re.sub(r"^[\s|]*\d+\)\s*", "", lines[i - 1]).split("=")[0].strip()
                    out.append((name, m.group(1), int(h.group(1), 16), int(m.group(2))))
        i += 1
    return out


def offset_targets(et: Oracle, path: str) -> list[tuple[str, str, int, int]]:
    """IsOffset Start/Offset tags with a paired Length (ExifTool absolutises them)."""
    # -G1:4: every instance keeps its own key (`Casio:Copy1:PreviewImageStart`),
    # where -G1 alone keeps one per group and drops the duplicates
    rows = et.json(["-n", "-a", "-G1:4", "-MakerNotes:all"], path)
    out = []
    for key, value in rows.items():
        parts = key.split(":")
        name = parts[-1]
        if name not in ISOFFSET or not isinstance(value, int):
            continue
        # plural pairs too (StripOffsets/StripByteCounts, FreeOffsets,
        # TileOffsets), stemmed and suffixed as makernote_offset_pairs.pl
        # pairs them for the generated Rust inventory
        stem = re.sub(r"(Offsets?|Start)$", "", name)
        prefix = ":".join(parts[:-1])
        for suffix in ("Length", "Size", "ByteCount", "ByteCounts"):
            length = rows.get(f"{prefix}:{stem}{suffix}")
            if isinstance(length, int) and length > 0:
                out.append((name, "isoffset", value, length))
                break
    return out


def survey_one(et: Oracle, path: str) -> dict | None:
    base, tiff = carrier_exif(path)
    if tiff is None or len(tiff) < 8 or tiff[:2] not in (b"II", b"MM"):
        return None
    try:
        owned, _ = structure(tiff)
        notes = [r for _, r in makernotes_of(tiff)]
    except struct.error:
        return None
    if not notes:
        return None
    note_keys = {f"{d}:0x927c" for d, _ in makernotes_of(tiff)}
    refs = []
    for name, tag, off, size in value_targets(et, path) + offset_targets(et, path):
        start, end = off - base, off - base + size
        if any(start >= a and end <= b for a, b in notes):
            continue
        if start < 0:
            cls = "before-tiff"
        elif start >= len(tiff):
            cls = "beyond-tiff"
        elif end > len(tiff):
            cls = "straddles-tiff-end"
        elif any(start < b and end > a for a, b in notes):
            cls = "mn-overlap"
        else:
            hits = sorted({o[0] for o in owned if o[1] < end and start < o[2] and o[0] not in note_keys})
            cls = "claimed:" + ",".join(hits) if hits else "hole"
        refs.append({"name": name, "tag": tag, "tiff_off": start, "len": size, "class": cls})
    return {"file": path, "order": tiff[:2].decode(), "tiff_len": len(tiff), "mn": notes[0],
            "mns": notes, "out": refs}


def known_ref(ref: dict) -> bool:
    """A named tag the oracle reports by default (not an Unknown `_0xNNNN`)."""
    if re.search(r"_0x[0-9a-f]{4}$", ref["name"]):
        return False
    return ref["tag"] != "isoffset" or ref["name"] in ISOFFSET


# ----------------------------------------------------------------- matrix ---

def png_bytes() -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    ihdr = struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(b"\0\0\0\0")) + chunk(b"IEND", b"")


def readback(et: Oracle, path: str) -> dict:
    """Every maker-note tag, every instance (family-4 ``Copy N`` keeps
    repeats), binary values hashed; IsOffset values dropped (compared through
    the data tags they locate).  Instances are collapsed to a sorted list per
    ``group1:name`` because the Copy numbering follows the carrier's read
    order, which differs between JPEG and PNG for the same EXIF block."""
    rows = et.json(["-a", "-G1:4", "-b", "-n", "-MakerNotes:all"], path)
    out: dict = collections.defaultdict(list)
    for key, value in rows.items():
        if key == "SourceFile":
            continue
        parts = key.split(":")
        name = parts[-1]
        if name in ISOFFSET:
            continue
        text = json.dumps(value, sort_keys=True)
        out[f"{parts[0]}:{name}"].append(
            text if len(text) <= 80 else "sha256:" + hashlib.sha256(text.encode()).hexdigest()[:24])
    return {k: sorted(v) for k, v in out.items()}


def flip_tiff(tiff: bytes) -> bytes | None:
    """The same TIFF block in the other byte order, every structure in place.

    Only the standard directories (IFD0, ExifIFD, GPS, InteropIFD, IFD1) and
    their numeric values are re-encoded; UNDEFINED/ASCII/BYTE values -- the
    MakerNote among them -- keep their bytes and their offsets, so a maker
    note's own offsets stay valid.  Whether the maker note still reads the
    same (it may inherit the outer order) is checked by the caller."""
    src = "<" if tiff[:2] == b"II" else ">"
    dst = ">" if src == "<" else "<"
    out = bytearray(tiff)
    out[0:2] = b"MM" if src == "<" else b"II"

    def swap(off: int, fmt: str) -> None:
        size = struct.calcsize(fmt)
        out[off:off + size] = struct.pack(dst + fmt, *struct.unpack(src + fmt, tiff[off:off + size]))

    unit = {3: "H", 4: "I", 5: "I", 8: "h", 9: "i", 10: "i", 11: "f", 12: "d", 13: "I"}
    seen: set[int] = set()

    def walk(off: int, name: str) -> None:
        if off in seen or off + 2 > len(tiff):
            return
        seen.add(off)
        count = struct.unpack(src + "H", tiff[off:off + 2])[0]
        swap(off, "H")
        children = []
        for k in range(count):
            p = off + 2 + 12 * k
            if p + 12 > len(tiff):
                raise ValueError("truncated directory")
            tag, typ, cnt = struct.unpack(src + "HHI", tiff[p:p + 8])
            swap(p, "H"); swap(p + 2, "H"); swap(p + 4, "I")
            size = TYPE_SIZE.get(typ, 1) * cnt
            at = p + 8 if size <= 4 else struct.unpack(src + "I", tiff[p + 8:p + 12])[0]
            if size > 4:
                swap(p + 8, "I")
            if typ in unit:
                n = size // struct.calcsize(unit[typ])
                if at + size > len(tiff):
                    raise ValueError("value out of range")
                for i in range(n):
                    swap(at + i * struct.calcsize(unit[typ]), unit[typ])
            if size <= 4 and typ not in unit:
                pass  # inline bytes: order-free
            if (name, tag) in (("IFD0", 0x8769), ("IFD0", 0x8825), ("ExifIFD", 0xA005)):
                child = struct.unpack(src + "I", tiff[p + 8:p + 12])[0]
                children.append((child, {0x8769: "ExifIFD", 0x8825: "GPS", 0xA005: "InteropIFD"}[tag]))
        nxt_at = off + 2 + 12 * count
        nxt = struct.unpack(src + "I", tiff[nxt_at:nxt_at + 4])[0] if nxt_at + 4 <= len(tiff) else 0
        if nxt_at + 4 <= len(tiff):
            swap(nxt_at, "I")
        for child, child_name in children:
            walk(child, child_name)
        if name == "IFD0" and nxt:
            walk(nxt, "IFD1")

    swap(2, "H")
    ifd0 = struct.unpack(src + "I", tiff[4:8])[0]
    swap(4, "I")
    walk(ifd0, "IFD0")
    return bytes(out)


def sha(path: str) -> str:
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def diff(a: dict, b: dict) -> dict:
    return {k: [a.get(k), b.get(k)] for k in sorted(set(a) | set(b)) if a.get(k) != b.get(k)}


GROUP_EDITS = {
    # block-shrinking group removals that keep the maker note ...
    "ifd1_all": ("IFD1", ["-IFD1:All="]),
    "interop_all": ("InteropIFD", ["-InteropIFD:All="]),
    "gps_all": ("GPS", ["-GPS:All="]),
    # ... and the two that delete it (ExifIFD with its note; the whole block)
    "exififd_all": (None, ["-ExifIFD:All="]),
    "ifd0_all": (None, ["-IFD0:All="]),
}


def edits_for(et: Oracle, src: str, sets: str = "basic") -> dict[str, list[str]]:
    """The edits of one source. ``basic``: grow / shrink / same. ``groups``:
    the group removals of :data:`GROUP_EDITS` (a group only when the source
    holds it). ``all``: both."""
    rows = et.json(["-a", "-G1", "-n", "-IFD0:all", "-ExifIFD:all", "-IFD1:all",
                    "-InteropIFD:all", "-GPS:all"], src)
    edits: dict[str, list[str]] = {}
    if sets in ("basic", "all", "survey"):
        edits["grow"] = [f"-IFD0:ImageDescription={GROW}"]
        present = [(len(str(rows[k])), k) for k in SHRINK_CANDIDATES if k in rows]
        if present:
            edits["shrink"] = [f"-{max(present)[1]}="]
    if sets in ("basic", "all"):
        if "IFD0:ModifyDate" in rows:
            edits["same"] = [f"-IFD0:ModifyDate={SAME_DATE}"]
        elif "ExifIFD:DateTimeOriginal" in rows:
            edits["same"] = [f"-ExifIFD:DateTimeOriginal={SAME_DATE}"]
    if sets in ("groups", "all", "survey"):
        groups = {k.split(":")[0] for k in rows}
        for name, (group, args) in GROUP_EDITS.items():
            if group is None or group in groups:
                edits[name] = args
    return edits


def preview_state(et: Oracle, path: str) -> dict:
    """``-m -b -PreviewImage`` sha (every instance), the raw maker-note
    ``PreviewImageStart`` pointers, and the ``-validate`` warnings."""
    rows = et.json(["-a", "-G1", "-n", "-m", "-b", "-PreviewImage", "-MakerNotes:PreviewImageStart"], path)
    # without -m: a [minor] warning ("PreviewImageStart is past end of file")
    # is exactly what this has to see
    rows.update({f"W{k}": v for k, v in et.json(["-a", "-G1", "-validate", "-Warning"], path).items()
                 if k.split(":")[-1] == "Warning"})
    shas, starts, warnings = [], [], []
    for key, value in rows.items():
        name = key.split(":")[-1]
        if name == "PreviewImage":
            text = value if isinstance(value, str) else json.dumps(value)
            shas.append(hashlib.sha256(text.encode()).hexdigest()[:16])
        elif name == "PreviewImageStart":
            starts.append(value)
        elif name == "Warning":
            warnings.extend(value if isinstance(value, list) else [value])
    return {"preview": sorted(shas), "start": sorted(map(str, starts)),
            "warnings": sorted(str(w) for w in warnings)}


def preview_verdict(orig: dict, et_after: dict | None, ox_after: dict) -> str:
    """kept / lost / mis-pointed against the original preview, where the
    oracle's own edit kept it; as-oracle / differs-from-oracle where the
    oracle's edit changed it (a deleted maker note); n/a without one."""
    if not orig["preview"]:
        return "n/a"
    if et_after is None or et_after["preview"] == orig["preview"]:
        if ox_after["preview"] == orig["preview"]:
            return "kept"
        return "lost" if not ox_after["preview"] else "mis-pointed"
    return "as-oracle" if ox_after["preview"] == et_after["preview"] else "differs-from-oracle"


def variants(et: Oracle, src: str, work: Path, flip: bool = True) -> tuple[list[dict], list[dict]]:
    """(order, carrier, path) sources for one input JPEG, each checked to carry
    the original's maker notes unchanged before any cell relies on it."""
    reference = readback(et, src)
    out, notes = [], []
    as_is = work / "src.jpg"
    shutil.copy(src, as_is)
    jpegs = [("as-is", as_is)]
    flipped = work / "flip.jpg"
    data = Path(src).read_bytes()
    at, tiff = jpeg_exif(data)
    other = None
    try:
        other = flip_tiff(tiff) if flip else None
    except (ValueError, struct.error) as exc:
        notes.append({"variant": "flipped", "skipped": f"cannot flip byte order: {exc}"})
    if other is not None:
        flipped.write_bytes(data[:at] + other + data[at + len(tiff):])
        got = readback(et, str(flipped))
        if got == reference:
            jpegs.append(("flipped", flipped))
        else:
            notes.append({"variant": "flipped", "skipped": "maker notes read differently in the other order",
                          "diff": dict(list(diff(reference, got).items())[:4])})
    base_png = work / "base.png"
    base_png.write_bytes(png_bytes())
    for label, jpg in jpegs:
        tiff_order = carrier_exif(str(jpg))[1][:2].decode()
        out.append({"variant": label, "order": tiff_order, "carrier": "jpg", "path": jpg})
        exif_bin = work / f"{label}.exif"
        # binary stdout: not through Oracle.run, which decodes text
        exif_bin.write_bytes(subprocess.run([*et.argv, "-b", "-EXIF", str(jpg)],  # nosec B603
                                            capture_output=True, timeout=180).stdout)
        png = work / f"{label}.png"
        et.run(["-q", f"-EXIF<={exif_bin}", "-o", str(png), str(base_png)])
        if png.is_file() and readback(et, str(png)) == reference:
            out.append({"variant": label, "order": tiff_order, "carrier": "png", "path": png})
        else:
            got = readback(et, str(png)) if png.is_file() else {}
            notes.append({"variant": label, "carrier": "png", "skipped": "eXIf wrap changed maker notes",
                          "diff": dict(list(diff(reference, got).items())[:4])})
    return out, notes


def edit_readback(et: Oracle, path: str, args: list[str]) -> dict:
    """What the requested edit addresses, read back by the oracle: the tag
    (`-IFD0:ImageDescription=...` -> `-IFD0:ImageDescription`) or the whole
    group (`-IFD1:All=` -> `-IFD1:All`), every instance. Equal for oxidex's
    and the oracle's output of the same edit, or the edit did not happen as
    requested (a silent no-op reports success and changes nothing)."""
    wanted = [a.split("=", 1)[0] for a in args if a.startswith("-") and "=" in a]
    rows = et.json(["-a", "-G1:4", "-n", *wanted], path)
    rows.pop("SourceFile", None)
    return rows


def run_cells(et: Oracle, binaries: dict[str, str], src: str, root: Path, sets: str = "basic",
              flip: bool = True) -> list[dict]:
    """Every cell of one source; the oracle edits each cell once and every
    oxidex binary (``label -> path``) is judged against that same edit."""
    # collision-free: the readable tail plus a hash of the full source path
    # (two corpora can hold the same relative name)
    tail = re.sub(r"[^A-Za-z0-9._-]", "_", src.split("combined-samples/")[-1].split("t/images/")[-1])
    work = root / f"{tail}-{hashlib.sha256(src.encode()).hexdigest()[:12]}"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    try:
        sources, notes = variants(et, src, work, flip)
    except Exception as exc:  # a broken source is reported, never silently dropped
        return [{"file": src, "error": f"variant preparation failed: {exc!r}"}]
    records = [{"file": src, **n} for n in notes]
    edits = edits_for(et, src, sets)
    for s in sources:
        original = str(s["path"])
        before = readback(et, original)
        orig_preview = preview_state(et, original)
        for edit, args in edits.items():
            stem = f"{s['variant']}-{s['carrier']}-{edit}"
            ext = "." + s["carrier"]
            et_path = work / f"{stem}-et{ext}"
            shutil.copy(original, et_path)
            er = et.run(["-overwrite_original", *args, str(et_path)])
            et_ok = er.returncode == 0 and "1 image files updated" in er.stdout
            et_rb = readback(et, str(et_path)) if et_ok else None
            et_preview = preview_state(et, str(et_path)) if et_ok else None
            for label, oxidex in binaries.items():
                ox_path = work / f"{stem}-{label}{ext}"
                shutil.copy(original, ox_path)
                orx = subprocess.run([oxidex, *args, str(ox_path)], capture_output=True, text=True,  # nosec B603
                                     errors="replace", timeout=180)
                rec = {"file": src, "binary": label, "variant": s["variant"], "order": s["order"],
                       "carrier": s["carrier"], "edit": edit, "args": [a[:60] for a in args],
                       "et_exit": er.returncode, "et_ok": et_ok, "ox_exit": orx.returncode,
                       "orig_preview": orig_preview}
                if orx.returncode != 0:
                    untouched = sha(str(ox_path)) == sha(original)
                    rec["verdict"] = "refused-untouched" if untouched else "refused-modified"
                    rec["preview_verdict"] = "refused"
                    rec["ox_msg"] = (orx.stdout + orx.stderr).strip()[-240:]
                else:
                    ox_rb = readback(et, str(ox_path))
                    ox_preview = preview_state(et, str(ox_path))
                    rec["preview_verdict"] = preview_verdict(orig_preview, et_preview, ox_preview)
                    rec["ox_preview"], rec["et_preview"] = ox_preview, et_preview
                    rec["validate_parity"] = et_preview is None or ox_preview["warnings"] == et_preview["warnings"]
                    rec["mn_rows"] = sum(len(v) for v in before.values())
                    if et_ok and edit_readback(et, str(ox_path), args) != edit_readback(et, str(et_path), args):
                        # the requested edit did not happen (or not as the
                        # oracle's did): never counted as a match
                        rec["verdict"] = "edit-not-applied"
                        rec["edit_ox"] = edit_readback(et, str(ox_path), args)
                        rec["edit_et"] = edit_readback(et, str(et_path), args)
                    elif et_ok:
                        if ox_rb == et_rb:
                            rec["verdict"] = "match"
                        elif ox_rb == before:
                            rec["verdict"] = "match-orig"
                            rec["et_vs_orig"] = dict(list(diff(before, et_rb).items())[:6])
                        else:
                            rec["verdict"] = "corrupt"
                            rec["diff_vs_et"] = dict(list(diff(et_rb, ox_rb).items())[:8])
                    else:
                        rec["verdict"] = "match-orig" if ox_rb == before else "corrupt"
                        rec["et_msg"] = (er.stdout + er.stderr).strip()[-200:]
                        if rec["verdict"] == "corrupt":
                            rec["diff_vs_orig"] = dict(list(diff(before, ox_rb).items())[:8])
                records.append(rec)
    shutil.rmtree(work, ignore_errors=True)
    return records


VERDICTS = ("match", "match-orig", "refused-untouched", "refused-modified", "corrupt", "edit-not-applied")


def summarize(records: list[dict]) -> None:
    cells = [r for r in records if "verdict" in r]
    labels = list(dict.fromkeys(r.get("binary", "ox") for r in cells))
    for label in labels:
        mine = [r for r in cells if r.get("binary", "ox") == label]
        verdicts = collections.Counter(r["verdict"] for r in mine)
        previews = collections.Counter(r.get("preview_verdict", "n/a") for r in mine)
        parity = sum(1 for r in mine if r.get("validate_parity") is False)
        print(f"[{label}] cells={len(mine)}  " + "  ".join(f"{k}={verdicts[k]}" for k in VERDICTS)
              + f"  | preview: " + "  ".join(f"{k}={v}" for k, v in sorted(previews.items()))
              + f"  | -validate differs from oracle: {parity}")
    by = collections.defaultdict(collections.Counter)
    for r in cells:
        key = (vendor_of(r["file"]), r["edit"], r["carrier"])
        by[key]["cells:" + r.get("binary", "ox")] += 1
        pv = r.get("preview_verdict")
        if pv in ("lost", "mis-pointed", "differs-from-oracle"):
            by[key][f"{pv}:{r.get('binary', 'ox')}"] += 1
        if r["verdict"] in ("corrupt", "refused-modified", "edit-not-applied"):
            by[key][f"corrupt:{r.get('binary', 'ox')}"] += 1
        if r["verdict"] == "refused-untouched":
            by[key][f"refused:{r.get('binary', 'ox')}"] += 1
    head = f"{'vendor':<14}{'edit':<12}{'carrier':<8}{'cells':>6}"
    for label in labels:
        head += f" | {label[:10]:>10} lost mis corrupt refused"
    print(head)
    for key, c in sorted(by.items()):
        line = f"{key[0]:<14}{key[1]:<12}{key[2]:<8}{c['cells:' + labels[0]]:>6}"
        for label in labels:
            line += (f" | {'':>10} {c['lost:' + label]:>4} {c['mis-pointed:' + label] + c['differs-from-oracle:' + label]:>3}"
                     f" {c['corrupt:' + label]:>7} {c['refused:' + label]:>7}")
        print(line)
    skipped = [r for r in records if "skipped" in r or "error" in r]
    print(f"variants skipped (wrap/flip changed maker notes, or preparation error): {len(skipped)}")


def vendor_of(path: str) -> str:
    tail = path.split("combined-samples/")[-1].split("t/images/")[-1]
    if "/" in tail:
        return tail.split("/")[0]
    return re.sub(r"[0-9]*\.jpe?g$", "", Path(tail).name, flags=re.I).rstrip("_") or tail


# ------------------------------------------------------------------- main ---

def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("survey")
    s.add_argument("files", help="file listing JPEG paths, one per line")
    s.add_argument("--jobs", type=int, default=2, help="workers (AGENTS.md: at most ~2 heavy waves)")
    s.add_argument("--out", required=True)
    s.add_argument("--min-files", type=int, default=1)
    sel = sub.add_parser("select")
    sel.add_argument("survey")
    sel.add_argument("--per-group", type=int, default=4)
    sel.add_argument("--include", nargs="*", default=[], help="always include these paths")
    m = sub.add_parser("matrix")
    m.add_argument("sources", help="file listing source JPEG paths, one per line")
    m.add_argument("--oxidex", required=True, action="append",
                   help="PATH, or LABEL=PATH; repeat to judge several binaries on the same oracle edits")
    m.add_argument("--edits", choices=["basic", "groups", "all", "survey"], default="basic",
                   help="basic: grow/shrink/same; groups: <group>:All removals; survey: grow, shrink "
                        "and the group removals")
    m.add_argument("--no-flip", action="store_true", help="skip the byte-order-flipped variant")
    m.add_argument("--out", required=True, help="JSONL of every cell")
    m.add_argument("--work", default=None)
    m.add_argument("--jobs", type=int, default=2, help="workers (AGENTS.md: at most ~2 heavy waves)")
    m.add_argument("--min-mn-tags", type=int, default=1000,
                   help="floor on maker-note rows compared (summed over the first binary's cells): "
                        "a degraded oracle reads none and every cell would 'match'")
    m.add_argument("--min-cells", type=int, default=1)
    args = ap.parse_args()

    if args.cmd == "select":
        rows = [json.loads(line) for line in open(args.survey) if line.startswith("{")]
        groups = collections.defaultdict(list)
        for r in rows:
            for cls in sorted({o["class"].split(":")[0] for o in r["out"] if known_ref(o)}):
                groups[(vendor_of(r["file"]), cls)].append(r["file"])
        chosen = list(dict.fromkeys(args.include))
        for key in sorted(groups):
            for f in sorted(set(groups[key]))[: args.per_group]:
                if f not in chosen:
                    chosen.append(f)
        print("\n".join(chosen))
        return 0

    et, oracle = resolve_oracle()
    git = instrument.git_state()
    dirty_overridden = instrument.refuse_if_dirty(git, "makernote_outofblob_matrix.py")
    files = [line.strip() for line in open(args.files if args.cmd == "survey" else args.sources) if line.strip()]
    for f in files:
        if not Path(f).is_file():
            sys.exit(f"❌ source missing: {f}")
    binaries: dict[str, str] = {}
    binary = None
    if args.cmd == "matrix":
        for spec in args.oxidex:
            label, _, path = spec.rpartition("=")
            ident = instrument.resolve_binary(path, kind="oxidex")
            binaries[label or "ox"] = str(ident.path)
            binary = binary or ident
        if len(binaries) != len(args.oxidex):
            sys.exit("❌ --oxidex labels must be distinct")
    extra = [f"oxidex[{k}]: {v}  sha256 {sha(v)[:16]}" for k, v in binaries.items()]
    instrument.print_header(tool=f"makernote_outofblob_matrix.py {args.cmd}", git=git, binary=binary,
                            extra=extra,
                            dirty_overridden=dirty_overridden, oracle=oracle, corpus_paths=[args.files if args.cmd == "survey" else args.sources],
                            file_count=len(files))
    sys.stdout.flush()

    if args.cmd == "survey":
        rows = []
        with ThreadPoolExecutor(args.jobs) as ex, open(args.out, "w") as out:
            # streamed: a row is on disk as soon as its file is surveyed
            for r in ex.map(lambda f: survey_one(et, f), files):
                if r is not None:
                    out.write(json.dumps(r) + "\n")
                    out.flush()
                    rows.append(r)
        if len(rows) < args.min_files:
            sys.exit(f"❌ only {len(rows)} maker-note files surveyed (< --min-files {args.min_files})")
        hit = [r for r in rows if any(known_ref(o) for o in r["out"])]
        classes = collections.Counter(c for r in hit for c in {o["class"].split(":")[0] for o in r["out"] if known_ref(o)})
        print(f"maker-note files={len(rows)}  with a named out-of-blob reference={len(hit)}")
        for cls, n in classes.most_common():
            print(f"  {cls:<20} files={n}")
        return 0

    root = Path(args.work or tempfile.mkdtemp(prefix="mnob-matrix-"))
    root.mkdir(parents=True, exist_ok=True)
    records: list[dict] = []
    with ThreadPoolExecutor(args.jobs) as ex, open(args.out, "w") as out:
        for recs in ex.map(lambda f: run_cells(et, binaries, f, root, args.edits, not args.no_flip), files):
            for r in recs:
                out.write(json.dumps(r) + "\n")
                out.flush()
            records.extend(recs)
    cells = sum(1 for r in records if "verdict" in r) // max(1, len(binaries))
    summarize(records)
    if cells < args.min_cells:
        sys.exit(f"❌ only {cells} cells ran (< --min-cells {args.min_cells}); refusing to report a number")
    first = next(iter(binaries), None)
    mn_rows = sum(r.get("mn_rows", 0) for r in records if r.get("binary") == first)
    print(f"maker-note rows compared ({first}): {mn_rows}")
    if mn_rows < args.min_mn_tags:
        sys.exit(f"❌ only {mn_rows} maker-note rows compared (< --min-mn-tags {args.min_mn_tags}); "
                 "the oracle read-back looks degraded -- refusing to report a number")
    bad = sum(1 for r in records if r.get("verdict") in ("corrupt", "refused-modified", "edit-not-applied"))
    return 1 if bad else 0


if __name__ == "__main__":
    raise SystemExit(main())
