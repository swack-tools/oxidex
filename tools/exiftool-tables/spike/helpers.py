#!/usr/bin/env python3
"""SPIKE (measurement only): index the pinned ExifTool 13.59 Perl library's
subs, so every helper an expression calls can be answered with: does it exist,
in which module, is it a pure function of its arguments, does it read the
ExifTool object, does it drive the reader engine -- and does oxidex already
have a Rust port of it?

No `exiftool` process is run: this reads the pinned tree's .pm/.pl SOURCE.
"""

import pathlib
import re

SUB_RE = re.compile(r"^sub\s+([A-Za-z_]\w*)\s*(\([^)]*\))?\s*\{", re.M)
PKG_RE = re.compile(r"^package\s+([\w:]+)\s*;", re.M)

SESSION_RX = re.compile(
    r"\$\$(?:et|self|exifTool)\{|\$(?:et|self|exifTool)->\{|->Options\(|"
    r"\$(?:et|self|exifTool)->[A-Z]")
ENGINE_RX = re.compile(
    r"ProcessDirectory|ProcessBinaryData|HandleTag|FoundTag|ExtractInfo|"
    r"ProcessTIFF|ProcessExif|->Read\(|RawConvInv|WriteInfo|\$raf|"
    r"SetNewValue|ProcessJpeg")
ENV_RX = re.compile(r"\blocaltime\b|\btime\(\)|\$ENV\{|%ENV|TimeZoneString|"
                    r"static_vars")
CALL_RX = re.compile(r"\b([A-Za-z_]\w*)\s*\(")


class Lib:
    def __init__(self, libdir):
        self.libdir = pathlib.Path(libdir)
        self.bodies = {}        # "Pkg::Sub" -> body text
        self.by_short = {}      # "Sub" -> [qualified, ...]
        self._analysis = {}
        self._scan()

    def _scan(self):
        files = sorted(self.libdir.rglob("*.pm")) + sorted(self.libdir.rglob("*.pl"))
        for path in files:
            text = path.read_text(encoding="utf-8", errors="replace")
            packages = [(m.start(), m.group(1)) for m in PKG_RE.finditer(text)]
            for m in SUB_RE.finditer(text):
                name = m.group(1)
                pkg = "main"
                for start, p in packages:
                    if start < m.start():
                        pkg = p
                    else:
                        break
                body = self._balanced_body(text, m.end() - 1)
                qual = f"{pkg}::{name}"
                self.bodies.setdefault(qual, body)
                self.by_short.setdefault(name, []).append(qual)

    @staticmethod
    def _balanced_body(text, i):
        depth = 0
        start = i
        n = len(text)
        while i < n:
            c = text[i]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    return text[start:i + 1]
            elif c == "#":
                j = text.find("\n", i)
                i = n if j < 0 else j
                continue
            i += 1
        return text[start:start + 20000]

    def resolve(self, canonical):
        """Map a classifier helper name to a qualified sub name, or None."""
        if canonical.startswith("ET->"):
            short = canonical[4:]
            for qual in self.by_short.get(short, []):
                if qual.startswith("Image::ExifTool::") and \
                        qual.count("::") == 2 or qual == "Image::ExifTool::" + short:
                    return qual
            cands = self.by_short.get(short, [])
            return cands[0] if cands else None
        if canonical.startswith("<"):
            return None
        for cand in (f"Image::ExifTool::{canonical}", canonical):
            if cand in self.bodies:
                return cand
        short = canonical.split("::")[-1]
        cands = self.by_short.get(short, [])
        return cands[0] if len(cands) == 1 else (cands[0] if cands else None)

    def analyze(self, canonical, _depth=0, _seen=None):
        """{'found','qualified','session','engine','env','calls'}"""
        if canonical in self._analysis:
            return self._analysis[canonical]
        qual = self.resolve(canonical)
        if qual is None:
            result = {"found": False, "qualified": None, "session": False,
                      "engine": False, "env": False}
            self._analysis[canonical] = result
            return result
        body = self.bodies[qual]
        session = bool(SESSION_RX.search(body))
        engine = bool(ENGINE_RX.search(body))
        env = bool(ENV_RX.search(body))
        seen = set(_seen or ())
        seen.add(qual)
        # Transitivity is deliberately shallow and does NOT propagate the
        # "engine" flag: a one-line `$et->Warn` deep in a callee would
        # otherwise mark every arithmetic helper above it as engine-coupled,
        # which is exactly the kind of instrument that lies in the direction
        # you were already leaning (AGENTS.md).
        if _depth < 1:
            for called in set(CALL_RX.findall(body)):
                if called in ("if", "unless", "while", "for", "foreach",
                              "return", "sprintf", "printf", "join", "split",
                              "push", "pack", "unpack", "defined", "length",
                              "substr", "int", "abs", "hex", "sprintf", "my"):
                    continue
                cands = self.by_short.get(called, [])
                if len(cands) != 1 or cands[0] in seen:
                    continue
                sub = self.analyze(cands[0], _depth + 1, seen)
                session = session or sub["session"]
                env = env or sub["env"]
        result = {"found": True, "qualified": qual, "session": session,
                  "engine": engine, "env": env}
        if _depth == 0:
            self._analysis[canonical] = result
        return result


# ---------------------------------------------------------------------------
# oxidex's existing Rust ports
# ---------------------------------------------------------------------------

RUST_FN_RE = re.compile(r"^pub fn ([a-z0-9_]+)", re.M)

# Perl helper -> the Rust fn in src/exiftool_tables/exprs.rs that ports it.
# Every entry is checked against the file at run time; a stale one is an error,
# not a silently-inflated port count.
RUST_PORTS = {
    "Exif::PrintExposureTime": "print_exposure_time",
    "Exif::PrintFNumber": "print_f_number",
    "Exif::PrintFraction": "print_fraction",
    "GPS::ToDMS": "gps_to_dms",
    "Canon::CanonEv": "canon_ev",
    "Nikon::PrintPC": "nikon_print_pc",
    "ICC_Profile::HexID": "icc_hex_id",
    "ConvertUnixTime": "convert_unix_time",
    "ConvertDuration": "convert_duration",
    "ConvertBitrate": "convert_bitrate",
    "ConvertFileSize": "convert_file_size",
    "ASF::GetGUID": "asf_get_guid",
    "Canon::PrintAFPointsLeftRight": "print_af_points_left_right",
    "Canon::PrintAFPointsUpDown": "print_af_points_up_down",
    "ET->Decode": "decode_ucs2",
    "CanonCustom::ConvertPfn": "convert_pfn",
    "CanonCustom::ConvertPFn": "convert_pfn",
}
# Ports that cover only the branch oxidex runs under, not the whole Perl sub.
# Counting these as complete would be exactly the flattering measurement
# AGENTS.md warns about, so they are reported separately as "partial":
#   ET->ConvertDateTime  identity, valid only with no -d/DateFormat option
#   ET->Decode           only the UCS2 charset (decode_ucs2)
#   ConvertFileSize      only the default ByteUnit branch
#   GPS::ToDMS           only the default CoordFormat
PARTIAL_PORTS = {"ET->ConvertDateTime", "ConvertDateTime", "ET->Decode",
                 "ConvertFileSize", "GPS::ToDMS"}


def rust_ported(exprs_rs_path):
    """({helper: "full"|"partial"}, set of rust fn names in exprs.rs)"""
    text = pathlib.Path(exprs_rs_path).read_text(encoding="utf-8")
    fns = set(RUST_FN_RE.findall(text))
    ported = {}
    missing = []
    for helper, fn in RUST_PORTS.items():
        if fn in fns:
            ported[helper] = "partial" if helper in PARTIAL_PORTS else "full"
        else:
            missing.append((helper, fn))
    if missing:
        raise SystemExit(f"stale RUST_PORTS entries (fn absent from exprs.rs): {missing}")
    for helper in PARTIAL_PORTS:
        ported.setdefault(helper, "partial")
    return ported, fns
