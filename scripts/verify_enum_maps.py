#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Diff a Rust commit's numeric-key -> display-string pairs against ExifTool's own lookup tables.

WHY THIS EXISTS (measured 2026-07-26, three fleet fixes, two of them wrong):

  TTF 5249a506 added `const LANGUAGE_SPANISH_MACINTOSH: u16 = 12;` and
  `const LANGUAGE_ITALIAN_MACINTOSH: u16 = 4;`, then match arms mapping those
  to Some("es") / Some("it"). ExifTool's %ttLang{Macintosh} (Font.pm:86) says
  12 => 'ar' and 4 => 'nl-NL'; Spanish is 6 and Italian is 3. Font.ttf holds a
  REAL Dutch record at 4, so FontSubfamily-it would have carried Dutch text.

  RAR a998b8fc added `2 => "MacOS", 3 => "BeOS", 4 => "OS/2"` plus a catch-all
  `_ => "Unknown"` to a RAR5 host-OS map. ExifTool's %Image::ExifTool::ZIP::RAR5
  OperatingSystem PrintConv (ZIP.pm:271) is exactly {0: Win32, 1: Unix}.

  RW2 e0900a27 was clean: every added value matched Exif.pm exactly.

Two existing gates both missed the two bad ones:

  * The worker's own `Verified: recheck-pass gaps=6->4` trailer cannot see a
    constant the sample does not exercise. Both fabrications sat beside correct
    values the sample DID hit, so the gap count dropped and it looked like a win.

  * validate_fix_commit.check_printconv (validate_fix_commit.py:1224) verifies
    each added value with a bare substring test -- `if value.encode("utf-8") in
    source: continue`. The strings "es", "it", "fr" and "fi" ALL appear in
    Font.pm, so the TTF fabrication cleared that gate byte-for-byte. What was
    wrong was never the string; it was the NUMERIC KEY -> STRING PAIRING.

So this module is deliberately NOT a tightening of check_printconv. It is a new
tier that parses ExifTool's Perl lookup tables into (key, value) PAIRS and diffs
the pairs. It answers exactly one question and refuses to answer any other.

DESIGN RULE, earned the hard way: a false "clean" is far more expensive than an
honest "cannot-verify". Every ambiguity -- a table hint that matches three
tables, a PrintConv that is Perl code rather than a hash, a table containing
OTHER (whose sub can return anything for an unlisted key, so absence proves
nothing), a BITMASK sub-hash (whose keys are bit indices, not values), an empty
sub-table, a Rust match block we cannot bind to exactly one Perl table --
yields "cannot-verify" with a reason, never "clean" and never a guess.

WHAT COUNTS AS A FINDING
  mismatch        Rust maps key K to string S; ExifTool's table maps K to
                  something else, or does not define K at all. Both are
                  fabrications: ExifTool.pm:3614-3635 shows that when no key
                  matches and the table has no OTHER and no BITMASK, ExifTool
                  prints "Unknown ($val)" (or sprintf('Unknown (0x%x)',$val)
                  when the tag carries PrintHex), so an undefined key that
                  oxidex names is oxidex inventing data.
  catch-all arm   `_ => "Unknown"` and friends. Reported as its OWN finding
                  class, not as a mismatch, because the failure mode is
                  different: it does not contradict a table entry, it REPLACES
                  the "Unknown (2)" string ExifTool would have printed for every
                  value the table does not cover. Only STRING-valued catch-alls
                  are reported; `_ => None` is the correct fall-through and is
                  silent.
  unreachable     Pairs this run did not adjudicate, each with a reason
                  (sub-table filtered out, block unresolvable, non-numeric key,
                  or -- when the caller supplies `exercised_keys` -- a key the
                  sample cannot exercise, which is precisely the blind spot the
                  worker's recheck-pass gap count has).

PUBLIC API (the re-admission daemon calls these three; see the docstrings for
the exact record shapes, which are NamedTuples so `p.key` and `p[0]` both work):

  extract_rust_pairs(diff_text, *, extra_consts=None) -> list[RustPair]
  parse_perl_table(pm_path, table_hint) -> dict[str, str] | None
  verify(diff_text, pm_path, table_hint, ...) -> Verdict

Perl sources are the ExifTool checkout under --perl-lib, by default
/private/tmp/oxidex-exiftool-cache/exiftool/lib/Image/ExifTool.

Everything here is a pure function over text. There is no git, no subprocess and
no network in the verification path; `main()` shells out to git only to fetch a
diff, through an injected `run_git`, so the whole module is unit-testable
without a live repo.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import NamedTuple, Optional

# Bumped whenever the extraction or comparison rules change, so a daemon can
# tell which ruleset produced a stored verdict (same contract as
# validate_fix_commit.POLICY_VERSION, deliberately a separate counter because
# this tier's rules move independently of that one's).
VERIFIER_VERSION = 1

DEFAULT_PERL_LIB = Path("/private/tmp/oxidex-exiftool-cache/exiftool/lib/Image/ExifTool")

# Keys that appear inside a PrintConv hash but are NOT value pairs.
#   Notes    -- EXE.pm:55 puts a multi-line q{} POD block inside %languageCode.
#   OTHER    -- 51 sites; the sub can return a string for ANY unlisted key, so
#               "this key is absent from the table" stops being evidence.
#   BITMASK  -- 43 sites; keys are BIT INDICES, not values. Exif.pm:308-323 even
#               mixes plain pairs and a BITMASK sub-hash in one table.
# OTHER and BITMASK are handled as hard "cannot-verify" reasons below; the rest
# are simply not pairs.
_NON_PAIR_KEYS = frozenset(
    {
        "Notes",
        "OTHER",
        "BITMASK",
        "BITMASK_SEP",
        "SeparateTable",
        "PrintHex",
        "PrintString",
        "PrintConvColumns",
    }
)

# Keys whose presence proves a hash is a TAG DEFINITION (or an attribute
# fragment spliced by Perl hash flattening, e.g. Canon.pm:9198 %filterConv)
# rather than a bag of value pairs. Seeing any of these means "descend into
# PrintConv", not "treat the top level as pairs".
_TAG_ATTR_KEYS = frozenset(
    {
        "Name",
        "PrintConv",
        "ValueConv",
        "Writable",
        "Format",
        "Groups",
        "Flags",
        "Priority",
        "Condition",
        "RawConv",
        "SubDirectory",
        "Count",
        "Mask",
        "DataMember",
        "Description",
        "Unknown",
        "Hidden",
        "Protected",
        "PrintConvInv",
        "ValueConvInv",
    }
)
# Deliberately NOT in the set above: Notes. EXE.pm:55 %languageCode is a flat
# pair hash that opens with `Notes => q{...POD...}`, and counting Notes as a
# tag-definition marker made the parser descend looking for a PrintConv that
# was never there, reporting "no-printconv" for a perfectly good table.
# Measured 2026-07-26.

# Table-level metadata keys in a %Image::ExifTool::X::Y tag table.
_TABLE_META_KEYS = frozenset(
    {
        "GROUPS",
        "VARS",
        "NOTES",
        "PROCESS_PROC",
        "WRITE_PROC",
        "CHECK_PROC",
        "WRITABLE",
        "FORMAT",
        "FIRST_ENTRY",
        "DATAMEMBER",
        "IS_OFFSET",
        "SET_GROUP1",
        "PREFERRED",
        "AVOID",
        "TAG_PREFIX",
        "PERMANENT",
    }
)


# --------------------------------------------------------------------------
# record shapes
# --------------------------------------------------------------------------


class RustPair(NamedTuple):
    """One numeric-key -> display-string pair the diff ADDS.

    The first four fields are exactly the (key, value, file, line) contract the
    daemon was promised; the trailing fields are the extra evidence verify()
    needs and are safe to ignore. Unpack with slicing (`k, v, f, l = p[:4]`) or
    by attribute, not with a bare 4-name unpack.

    key         Canonical key string. Decimal for integers (hex is folded, so
                Rust 0x0c0a and Perl 0x0c0a both canonicalise to "3082"), the
                original text for fractional keys (Canon.pm has 215 of them --
                2.1 vs 2 must never collide), the literal text for quoted keys
                (EXE.pm's '0401' is a zero-padded STRING, not 0x401 and not
                401). None when the pattern carries no resolvable numeric key.
    value       The display string, unescaped. None for a non-string arm value.
    file        Path of the Rust file, from the diff header.
    line        1-based line number in the NEW file of the line the arm's `=>`
                sits on.
    kind        "pair" for a normal arm, "catch-all" for `_ => ...`.
    block       Opaque id of the enclosing `match { ... }` block. Pairs from
                DIFFERENT blocks are different tables: RW2 e0900a27 adds
                CustomRendered {0: Normal, 1: Custom} and ExposureMode
                {0: Auto, ...} in one diff, and flattening them would collide
                on key 0.
    block_key   Canonical key of the enclosing match arm, when the block is an
                arm body. This is how RW2's block gets bound to Exif.pm 0xa401:
                the Rust arm is `0xA401 if field_type == 3 => match ... {`.
    block_label Nearest preceding comment / fn name for the block, used as a
                weaker fallback binding.
    key_parts   For tuple patterns, [(const_name_or_None, canonical_key_or_None)]
                per tuple position, left to right. TTF's
                `(PLATFORM_MACINTOSH, LANGUAGE_SPANISH_MACINTOSH)` becomes
                [("PLATFORM_MACINTOSH", None), ("LANGUAGE_SPANISH_MACINTOSH",
                "12")] -- position 0 is the sub-table discriminant, position 1
                is the lookup key, and verify() works out which is which.
    pattern     Raw pattern text of the alternative, for human-readable output.
    """

    key: Optional[str]
    value: Optional[str]
    file: Optional[str]
    line: int
    kind: str
    block: int
    block_key: Optional[str]
    block_label: Optional[str]
    key_parts: tuple
    pattern: str


class Mismatch(NamedTuple):
    """One fabricated pair.

    exiftool_says is None when the key is absent from the table entirely; in
    that case ExifTool would have printed "Unknown (<key>)" (ExifTool.pm:3629)
    and oxidex is inventing a name. exiftool_key_for_value is where the string
    oxidex used ACTUALLY lives in the table, when it lives anywhere -- this is
    what turns "12 is wrong" into the far more useful "Spanish is 6, not 12".
    """

    key: Optional[str]
    rust_says: Optional[str]
    exiftool_says: Optional[str]
    exiftool_key_for_value: Optional[str] = None
    file: Optional[str] = None
    line: int = 0


class CatchAll(NamedTuple):
    """A `_ => "..."` arm. Distinct finding class -- see the module docstring."""

    value: str
    file: Optional[str]
    line: int
    data_replacing: bool = True


class Unreachable(NamedTuple):
    """A pair this run did not adjudicate, and why."""

    key: Optional[str]
    value: Optional[str]
    reason: str
    file: Optional[str] = None
    line: int = 0


class Verdict(NamedTuple):
    """Result of verify(). `status` is the only field a caller must branch on.

    status         "clean" | "fabricated" | "cannot-verify"
    mismatches     list[Mismatch]
    unreachable    list[Unreachable]
    catch_all_arms list[CatchAll]
    reason         short machine-readable reason, always set for
                   "cannot-verify" and for a "clean" that checked nothing
    table          fully-qualified name of the Perl table actually used, or a
                   comma-joined list in per-block mode
    candidates     when a hint matched more than one table, the names that
                   matched -- ZIP.pm holds THREE OperatingSystem PrintConvs
                   that disagree, so naming them is the useful output
    pairs_checked  how many pairs were actually compared. A daemon that treats
                   "clean" as a pass MUST also look at this: 0 means nothing
                   was verified, not that everything was right.
    """

    status: str
    mismatches: list
    unreachable: list
    catch_all_arms: list
    reason: Optional[str] = None
    table: Optional[str] = None
    candidates: tuple = ()
    pairs_checked: int = 0

    def to_dict(self):
        return {
            "status": self.status,
            "reason": self.reason,
            "table": self.table,
            "candidates": list(self.candidates),
            "pairs_checked": self.pairs_checked,
            "mismatches": [m._asdict() for m in self.mismatches],
            "unreachable": [u._asdict() for u in self.unreachable],
            "catch_all_arms": [c._asdict() for c in self.catch_all_arms],
            "verifier_version": VERIFIER_VERSION,
        }


# --------------------------------------------------------------------------
# key canonicalisation (shared by both sides -- this is what makes the diff a
# PAIR diff rather than a string-membership check)
# --------------------------------------------------------------------------

_HEX_RE = re.compile(r"^[-+]?0[xX][0-9a-fA-F_]+$")
_DEC_RE = re.compile(r"^[-+]?[0-9][0-9_]*$")
_FLOAT_RE = re.compile(r"^[-+]?[0-9]+\.[0-9]+$")


def canonical_key(token):
    """Canonicalise a key token from EITHER language to a comparable string.

    Integers fold to decimal so Rust `0x0c0a` and Perl `0x0c0a` meet at "3082".
    Fractional keys are NOT numeric-normalised: Canon.pm uses 2.1 / 8.1 / 169.7
    as disambiguation slots for colliding lens IDs, and coercing them to int
    silently merges 2.1 into 2. Anything else is returned stripped, so a quoted
    Perl key like '0401' stays the four-character string it is.
    """
    if token is None:
        return None
    raw = str(token)
    text = raw.strip()
    if not text:
        return None
    if _HEX_RE.match(text):
        return str(int(text.replace("_", ""), 16))
    if _DEC_RE.match(text):
        return str(int(text.replace("_", "")))
    if _FLOAT_RE.match(text):
        # keep the written form; 2.1 and 2.10 are not the same slot in practice
        return text
    # Non-numeric keys are returned VERBATIM, whitespace and all.
    # QuickTime.pm:146 keys %ftypLookup on 4-character FourCCs, and 'aax ' /
    # 'crx ' carry a significant trailing space -- trimming them corrupts the
    # table.
    return raw


def _is_numeric_key(text):
    if text is None:
        return False
    t = str(text).strip()
    return bool(_HEX_RE.match(t) or _DEC_RE.match(t) or _FLOAT_RE.match(t))


# --------------------------------------------------------------------------
# Perl side
# --------------------------------------------------------------------------

_TOK_STRING = "str"
_TOK_GROUP = "group"
_TOK_OP = "op"
_TOK_WORD = "word"

_OPEN = {"(": ")", "{": "}", "[": "]"}
_CLOSE = {")", "}", "]"}


def _skip_ws_and_comments(text, i):
    """Advance past whitespace and `#` comments.

    Safe because no PrintConv VALUE in the tree contains a '#' (measured
    2026-07-26 across all 172 .pm files: the only '#'-bearing strings are
    Notes/Groups metadata like ICC_Profile.pm:380 `Groups => {1 => 'ICC_Profile#'}`,
    which is not a value pair). This is also what drops ExifTool's many
    commented-out SPECULATIVE pairs -- Canon.pm:146 `# 27 => 'Carl Zeiss ...'`
    -- which must never be blessed as ground truth.
    """
    n = len(text)
    while i < n:
        c = text[i]
        if c in " \t\r\n":
            i += 1
        elif c == "#":
            j = text.find("\n", i)
            i = n if j < 0 else j + 1
        else:
            break
    return i


def _read_string(text, i):
    """Read a Perl '...' or "..." literal starting at i. Returns (value, next_i)."""
    quote = text[i]
    i += 1
    out = []
    n = len(text)
    while i < n:
        c = text[i]
        if c == "\\" and i + 1 < n:
            nxt = text[i + 1]
            if quote == "'":
                # in single quotes only \\ and \' are escapes
                if nxt in ("\\", "'"):
                    out.append(nxt)
                    i += 2
                    continue
                out.append(c)
                i += 1
                continue
            out.append({"n": "\n", "t": "\t", "\\": "\\", '"': '"', "$": "$", "@": "@"}.get(nxt, nxt))
            i += 2
            continue
        if c == quote:
            return "".join(out), i + 1
        out.append(c)
        i += 1
    return "".join(out), i


_QLIKE_RE = re.compile(r"\b(qq|qw|q)\s*([\{\(\[])")


def _match_qlike(text, i):
    """Return the index just past a q{...} / qq(...) / qw[...] body opened at i.

    Perl treats a q-quoted body as literal text: only the delimiter pair nests,
    and only a backslash escapes. Nothing else in there is syntax.

    This function exists because the generic scanner did NOT do that and it
    silently destroyed Exif.pm. `Notes => q{ ... various IFD's of DNG images
    ... }` (Exif.pm:11688 in %Main, tag 0x117) contains an apostrophe inside the
    q-block; the generic scanner treated it as an opening single quote and ran
    to the NEXT apostrophe several entries later, swallowing a closing brace.
    Depth never recovered, %Main tokenised to 24 top-level entries instead of
    ~1400, and every Exif.pm lookup came back "no-such-table" -- a
    "cannot-verify" that looked principled and was actually a parser bug.
    Measured 2026-07-26.
    """
    opener = text[i]
    closer = _OPEN[opener]
    depth = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == "\\":
            i += 2
            continue
        if c == opener:
            depth += 1
        elif c == closer:
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def _match_delimited(text, i):
    """Return the index just past the group opened by text[i] (one of ( { [).

    String- and comment-aware, and treats q{...} / qq{...} bodies as opaque so a
    '#' or an unbalanced quote inside Perl code (Canon.pm:1659 embeds
    `my %r = ( a => 'Alpha ', ... )` inside a `PrintConv => q{...}`) cannot
    corrupt depth tracking.
    """
    close = _OPEN[text[i]]
    depth = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c in "'\"":
            _, i = _read_string(text, i)
            continue
        if c == "#":
            j = text.find("\n", i)
            i = n if j < 0 else j + 1
            continue
        m = _QLIKE_RE.match(text, i)
        if m and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] in "_$@%&")):
            i = _match_qlike(text, m.end() - 1)
            continue
        if c in _OPEN:
            depth += 1
            i += 1
            continue
        if c in _CLOSE:
            depth -= 1
            i += 1
            if depth == 0:
                return i
            continue
        i += 1
    return n


_WORD_CHARS = set("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_:.\\%&$@+-*/?!<>|~^")


def _tokenize(body):
    """Tokenise a Perl hash body into ('str'|'group'|'op'|'word', payload) tokens.

    Groups are kept whole and opaque, so nesting never leaks into the top-level
    entry split. This is what makes multi-column layouts fall out for free:
    QuickTime.pm:482 `0 => 'undef',  22 => 'unsigned int',  71 => 'float[2] size',`
    is just six tokens and two commas, no line-splitting involved.
    """
    tokens = []
    i = 0
    n = len(body)
    while i < n:
        i = _skip_ws_and_comments(body, i)
        if i >= n:
            break
        c = body[i]
        if c in "'\"":
            value, i = _read_string(body, i)
            tokens.append((_TOK_STRING, value))
            continue
        m = _QLIKE_RE.match(body, i)
        if m and (i == 0 or not (body[i - 1].isalnum() or body[i - 1] in "_$@%&")):
            end = _match_qlike(body, m.end() - 1)
            tokens.append((_TOK_GROUP, ("q", body[m.end() : end - 1])))
            i = end
            continue
        if c in _OPEN:
            end = _match_delimited(body, i)
            tokens.append((_TOK_GROUP, (c, body[i + 1 : end - 1])))
            i = end
            continue
        if body.startswith("=>", i):
            tokens.append((_TOK_OP, "=>"))
            i += 2
            continue
        if c == ",":
            tokens.append((_TOK_OP, ","))
            i += 1
            continue
        if c in ";":
            tokens.append((_TOK_OP, ";"))
            i += 1
            continue
        start = i
        while i < n and body[i] in _WORD_CHARS:
            if body.startswith("=>", i):
                break
            i += 1
        if i == start:
            i += 1
            continue
        tokens.append((_TOK_WORD, body[start:i]))
    return tokens


def _entries(body):
    """Yield (key_token, value_token) for every `k => v` at the top level of body."""
    tokens = _tokenize(body)
    out = []
    idx = 0
    current = []
    for tok in tokens:
        if tok[0] == _TOK_OP and tok[1] in (",", ";"):
            if current:
                out.append(current)
            current = []
            continue
        current.append(tok)
    if current:
        out.append(current)
    result = []
    for group in out:
        arrows = [i for i, t in enumerate(group) if t[0] == _TOK_OP and t[1] == "=>"]
        if not arrows:
            continue
        a = arrows[0]
        key_toks = group[:a]
        val_toks = group[a + 1 :]
        if len(key_toks) != 1 or not val_toks:
            continue
        result.append((key_toks[0], val_toks))
        idx += 1
    return result


def _key_text(tok):
    """Key text from a token, plus whether it was a quoted (string) key."""
    if tok[0] == _TOK_STRING:
        return tok[1], True
    if tok[0] == _TOK_WORD:
        return tok[1], False
    return None, False


class PerlTable(NamedTuple):
    """Parsed Perl lookup table, or the reason it could not be parsed."""

    pairs: Optional[dict]
    name: Optional[str]
    reason: Optional[str] = None
    candidates: tuple = ()


def _find_hash_bodies(source):
    """Index every top-level Perl hash in a .pm: short_name -> (full_name, body).

    Covers both forms recon found: `%Image::ExifTool::ZIP::RAR5 = (` at column 0
    and `my %ttLang = (` / `%ttLang = (`. Both matter -- %ttLang is a
    package-level hash that no tag table references (it is consumed by
    procedural code at Font.pm:501), so a walker that only visits tag tables
    would never find the table the TTF fabrication violated.
    """
    bodies = {}
    pattern = re.compile(r"^(?:my\s+|our\s+)?%([A-Za-z_][A-Za-z0-9_:]*)\s*=\s*\(", re.MULTILINE)
    for m in pattern.finditer(source):
        open_paren = source.index("(", m.end() - 1)
        end = _match_delimited(source, open_paren)
        full = m.group(1)
        short = full.split("::")[-1]
        body = source[open_paren + 1 : end - 1]
        bodies.setdefault(short, (full, body))
        bodies.setdefault(full, (full, body))
    return bodies


def _pairs_from_body(body, name):
    """Turn a hash body into {canonical_key: value}, or explain why we cannot."""
    pairs = {}
    saw_pair = False
    for key_tok, val_toks in _entries(body):
        key_text, quoted = _key_text(key_tok)
        if key_text is None:
            continue
        if key_text in ("OTHER",):
            # ExifTool.pm:3620 calls this sub for any key the hash lacks, so
            # "absent from the table" stops being evidence of anything.
            return PerlTable(None, name, reason="table-has-OTHER")
        if key_text in ("BITMASK", "BITMASK_SEP"):
            # keys are bit indices, not values -- a different semantics entirely
            return PerlTable(None, name, reason="table-is-BITMASK")
        if key_text in _NON_PAIR_KEYS:
            continue
        if not quoted and key_text in _TABLE_META_KEYS:
            continue
        if len(val_toks) != 1 or val_toks[0][0] not in (_TOK_STRING, _TOK_WORD):
            continue
        vtok = val_toks[0]
        if vtok[0] == _TOK_STRING:
            value = vtok[1]
        else:
            # a bare unquoted value; Nikon.pm:793 has `0 => 0,`
            if not _is_numeric_key(vtok[1]):
                continue
            value = vtok[1]
        key = key_text if quoted else canonical_key(key_text)
        pairs[key] = value
        saw_pair = True
    if not saw_pair:
        return PerlTable(None, name, reason="table-empty")
    return PerlTable(pairs, name)


def _looks_like_tag_definition(body):
    for key_tok, _ in _entries(body):
        key_text, quoted = _key_text(key_tok)
        if not quoted and key_text in _TAG_ATTR_KEYS:
            return True
    return False


def _find_entry(body, wanted):
    """Find `wanted => <value>` at the top level of body. Returns val_toks or None."""
    for key_tok, val_toks in _entries(body):
        key_text, quoted = _key_text(key_tok)
        if key_text is None:
            continue
        if key_text == wanted:
            return val_toks
        if not quoted and _is_numeric_key(key_text) and canonical_key(key_text) == canonical_key(wanted):
            return val_toks
    return None


def _resolve_hashref(ref_name, pm_path):
    """Follow a `\\%name` PrintConv reference to the hash body it names.

    1536 PrintConvs in the tree are `\\%name` rather than an inline hash, so not
    following them would abstain on a third of the parseable surface. The
    reference may be file-local (Canon.pm's `\\%canonWhiteBalance`, 9+ call
    sites) or fully qualified into a DIFFERENT file -- Exif.pm:1448 says
    `\\%Image::ExifTool::JPEG::yCbCrSubSampling`, and that hash is actually
    defined in ExifTool.pm:2149, not JPEG.pm. So the search order is: this
    module, then <lib>/<LastPackage>.pm, then the parent ExifTool.pm. One hop
    only; an unresolvable ref stays "cannot-verify".
    """
    path = Path(pm_path)
    source = path.read_text(encoding="utf-8", errors="replace")
    bodies = _find_hash_bodies(source)
    if ref_name in bodies:
        full, body = bodies[ref_name]
        return body, full
    short = ref_name.split("::")[-1]
    if "::" in ref_name:
        pkg = ref_name.split("::")[-2]
        for candidate in (path.parent / f"{pkg}.pm", path.parent.parent / "ExifTool.pm"):
            if not candidate.is_file():
                continue
            other = _find_hash_bodies(candidate.read_text(encoding="utf-8", errors="replace"))
            if ref_name in other:
                full, body = other[ref_name]
                return body, full
            if short in other:
                full, body = other[short]
                return body, full
    return None, None


def _descend_printconv(body, name, pm_path):
    """From a tag-definition body, return the PrintConv hash body, or a reason."""
    val_toks = _find_entry(body, "PrintConv")
    if val_toks is None:
        return None, PerlTable(None, name, reason="no-printconv")
    tok = val_toks[0]
    if tok[0] == _TOK_GROUP and tok[1][0] == "{":
        return tok[1][1], None
    if tok[0] == _TOK_WORD and tok[1].startswith("\\%"):
        ref_name = tok[1][2:]
        ref_body, ref_full = _resolve_hashref(ref_name, pm_path)
        if ref_body is None:
            return None, PerlTable(None, name, reason="printconv-hashref-unresolved:" + ref_name)
        return ref_body, None
    # 'expression', q{...}, sub {...}, \&Sub -- all code, none of them a table
    return None, PerlTable(None, name, reason="printconv-is-code")


def _split_hint(table_hint):
    """Normalise a hint into path components.

    Accepted, all equivalent where they overlap:
        %Image::ExifTool::ZIP::RAR5     ZIP::RAR5      RAR5
        RAR5.OperatingSystem            RAR5::OperatingSystem
        %ttLang{Macintosh}              ttLang.Macintosh
        OperatingSystem                 (bare tag name -> tag search)
    """
    text = table_hint.strip().lstrip("%")
    text = re.sub(r"^Image::ExifTool::", "", text)
    text = re.sub(r"\{\s*'?([^'}]+)'?\s*\}", r".\1", text)
    parts = [p for p in re.split(r"::|\.", text) if p]
    return parts


def _resolve_from_bodies(bodies, parts, pm_stem, pm_path):
    """Resolve hint components against the module's hashes. Returns PerlTable-ish."""
    if not parts:
        return None, PerlTable(None, None, reason="empty-hint")
    if len(parts) > 1 and parts[0].lower() == pm_stem.lower() and parts[0] not in bodies:
        parts = parts[1:]
    head = parts[0]
    if head not in bodies:
        return None, None  # caller falls back to a tag search
    full_name, body = bodies[head]
    name = full_name
    for comp in parts[1:]:
        val_toks = _find_entry(body, comp)
        if val_toks is None:
            return None, PerlTable(None, name, reason="hint-component-missing:" + comp)
        tok = val_toks[0]
        if tok[0] != _TOK_GROUP or tok[1][0] != "{":
            return None, PerlTable(None, name, reason="hint-component-not-a-hash:" + comp)
        body = tok[1][1]
        name = f"{name}{{{comp}}}"
    if _looks_like_tag_definition(body):
        pc_body, failure = _descend_printconv(body, name, pm_path)
        if failure is not None:
            return None, failure
        body = pc_body
        name = name + ".PrintConv"
    return (body, name), None


def _tag_search(bodies, wanted):
    """Find every (table, tag) in the module whose Name or key is `wanted`.

    This is the ZIP.pm disambiguation demo: "OperatingSystem" matches GZIP tag
    9, RAR tag 8 and RAR5's name-keyed OperatingSystem, and those three tables
    DISAGREE (value 2 is 'VMS (or OpenVMS)', 'Win32' and undefined respectively).
    Returning all three is the useful answer; picking one would be a coin flip
    dressed up as a verdict.
    """
    hits = []
    seen_full = set()
    for short, (full, body) in bodies.items():
        if full in seen_full:
            continue
        seen_full.add(full)
        for key_tok, val_toks in _entries(body):
            key_text, quoted = _key_text(key_tok)
            if key_text is None:
                continue
            tok = val_toks[0]
            if tok[0] != _TOK_GROUP or tok[1][0] != "{":
                continue
            inner = tok[1][1]
            matched = key_text == wanted
            if not matched and _is_numeric_key(wanted) and _is_numeric_key(key_text):
                matched = canonical_key(key_text) == canonical_key(wanted)
            if not matched:
                name_toks = _find_entry(inner, "Name")
                if name_toks and name_toks[0][0] == _TOK_STRING and name_toks[0][1] == wanted:
                    matched = True
            if matched:
                hits.append((f"{full}[{key_text}]", inner))
    return hits


def parse_perl_table_detail(pm_path, table_hint):
    """parse_perl_table's honest sibling: returns a PerlTable carrying a reason.

    PerlTable.pairs is None whenever the table could not be reduced to a set of
    literal (key, value) pairs, and PerlTable.reason then says why -- one of
    no-such-module / no-such-table / ambiguous-table / printconv-is-code /
    printconv-is-hashref:<name> / table-has-OTHER / table-is-BITMASK /
    table-empty / no-printconv / hint-component-missing:<c>. Callers that just
    want the dict use parse_perl_table().
    """
    path = Path(pm_path)
    if not path.is_file():
        return PerlTable(None, None, reason="no-such-module")
    source = path.read_text(encoding="utf-8", errors="replace")
    bodies = _find_hash_bodies(source)
    parts = _split_hint(table_hint) if table_hint else []
    if not parts:
        return PerlTable(None, None, reason="empty-hint")

    resolved, failure = _resolve_from_bodies(bodies, parts, path.stem, path)
    if failure is not None:
        return failure
    if resolved is not None:
        body, name = resolved
        return _pairs_from_body(body, name)

    # No hash by that name -- treat the hint as a tag name / tag id.
    hits = _tag_search(bodies, parts[-1])
    if not hits:
        return PerlTable(None, None, reason="no-such-table")
    if len(hits) > 1:
        return PerlTable(
            None,
            None,
            reason="ambiguous-table",
            candidates=tuple(name for name, _ in hits),
        )
    name, inner = hits[0]
    if _looks_like_tag_definition(inner):
        pc_body, pc_failure = _descend_printconv(inner, name, path)
        if pc_failure is not None:
            return pc_failure
        return _pairs_from_body(pc_body, name + ".PrintConv")
    return _pairs_from_body(inner, name)


def parse_perl_table(pm_path, table_hint):
    """Parse one ExifTool lookup table into {canonical_key: display_string}.

    Returns None -- never a guess -- when the table is missing, ambiguous, is
    Perl code rather than a hash, contains OTHER or BITMASK, or is empty. Use
    parse_perl_table_detail() when you need to know which of those happened.

    Handles the shapes recon measured: inline `PrintConv => { ... }`, named
    package hashes (`%canonWhiteBalance`), one level of nesting
    (`ttLang.Macintosh`), multi-column layouts (several pairs per source line),
    values containing commas ("Off, Did not fire"), double-quoted values that
    contain apostrophes, empty values, bare numeric values, and trailing
    #comments. Commented-out speculative pairs are skipped.
    """
    return parse_perl_table_detail(pm_path, table_hint).pairs


# --------------------------------------------------------------------------
# Rust side
# --------------------------------------------------------------------------

_DIFF_FILE_RE = re.compile(r"^\+\+\+ b/(.+)$")
_HUNK_RE = re.compile(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@")


class _Line(NamedTuple):
    file: Optional[str]
    number: int
    text: str
    added: bool


def _new_file_lines(diff_text):
    """Reconstruct the NEW-file view of every hunk: (file, lineno, text, added).

    Context lines are kept, not thrown away, because consts and enclosing match
    arms often sit in context. Only `.rs` files are considered -- this tier has
    nothing to say about Perl, TOML or markdown hunks.
    """
    out = []
    current_file = None
    lineno = 0
    in_hunk = False
    for raw in diff_text.splitlines():
        m = _DIFF_FILE_RE.match(raw)
        if m:
            current_file = m.group(1)
            in_hunk = False
            continue
        if raw.startswith("--- ") or raw.startswith("diff --git "):
            in_hunk = False
            continue
        h = _HUNK_RE.match(raw)
        if h:
            lineno = int(h.group(1))
            in_hunk = True
            continue
        if not in_hunk or current_file is None:
            continue
        if not current_file.endswith(".rs"):
            continue
        if raw.startswith("+"):
            out.append(_Line(current_file, lineno, raw[1:], True))
            lineno += 1
        elif raw.startswith("-"):
            continue
        elif raw.startswith(" "):
            out.append(_Line(current_file, lineno, raw[1:], False))
            lineno += 1
        elif raw == "":
            out.append(_Line(current_file, lineno, "", False))
            lineno += 1
        else:
            # "\ No newline at end of file" and similar
            continue
    return out


_CONST_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const|static)\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*[A-Za-z0-9_]+\s*=\s*"
    r"([-+]?(?:0[xX][0-9a-fA-F_]+|[0-9][0-9_]*))\s*;"
)

_FN_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")

_RUST_STRING_VALUE_RE = re.compile(
    r'^\s*(?:Some\s*\(\s*)?(?:String\s*::\s*from\s*\(\s*|Cow\s*::\s*Borrowed\s*\(\s*)?'
    r'r?"((?:[^"\\]|\\.)*)"'
)


def _strip_rust_comments(text):
    """Blank out // comments and /* */ blocks, preserving length and newlines."""
    out = list(text)
    i = 0
    n = len(text)
    in_str = False
    quote = ""
    while i < n:
        c = text[i]
        if in_str:
            if c == "\\":
                i += 2
                continue
            if c == quote:
                in_str = False
            i += 1
            continue
        if c in "\"'":
            # a char literal like '}' must not open a string; only " does here
            if c == '"':
                in_str = True
                quote = c
            i += 1
            continue
        if text.startswith("//", i):
            j = text.find("\n", i)
            j = n if j < 0 else j
            for k in range(i, j):
                out[k] = " "
            i = j
            continue
        if text.startswith("/*", i):
            j = text.find("*/", i + 2)
            j = n if j < 0 else j + 2
            for k in range(i, j):
                if out[k] != "\n":
                    out[k] = " "
            i = j
            continue
        i += 1
    return "".join(out)


def _unescape_rust(text):
    return (
        text.replace("\\\\", "\x00")
        .replace('\\"', '"')
        .replace("\\n", "\n")
        .replace("\\t", "\t")
        .replace("\\'", "'")
        .replace("\x00", "\\")
    )


def _rust_arm_value(text):
    """Classify the expression after a `=>`. Returns (kind, value).

    kind is "string" for a display string in any of the wrappers this codebase
    uses -- "x", "x".to_string(), Some("x"), Some("x".to_string()),
    Some(String::from("x")) -- and "other" for everything else (None, a nested
    match, a block, a function call, a number).
    """
    m = _RUST_STRING_VALUE_RE.match(text)
    if not m:
        return "other", None
    return "string", _unescape_rust(m.group(1))


def _pattern_alternatives(pattern):
    """Split a Rust match pattern on top-level `|`."""
    parts = []
    depth = 0
    current = []
    i = 0
    n = len(pattern)
    while i < n:
        c = pattern[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        if c == "|" and depth == 0:
            parts.append("".join(current))
            current = []
            i += 1
            continue
        current.append(c)
        i += 1
    parts.append("".join(current))
    return [p.strip() for p in parts if p.strip()]


def _tuple_elements(alt):
    """Split `(A, B)` into ["A", "B"]; a scalar pattern becomes ["A"]."""
    text = alt.strip()
    if not (text.startswith("(") and text.endswith(")")):
        return [text]
    inner = text[1:-1]
    parts = []
    depth = 0
    current = []
    for c in inner:
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        if c == "," and depth == 0:
            parts.append("".join(current).strip())
            current = []
            continue
        current.append(c)
    parts.append("".join(current).strip())
    return [p for p in parts if p]


def _pattern_head_key(pattern):
    """Canonical key of the leading token of a pattern, if it is numeric.

    Used to bind a nested `match` block to the tag id of the arm that contains
    it: RW2 e0900a27's block sits under `0xA401 if field_type == 3 => match ...`,
    and Exif.pm keys CustomRendered as `0xa401 => {`.
    """
    m = re.match(r"\s*([-+]?(?:0[xX][0-9a-fA-F_]+|[0-9][0-9_]*(?:\.[0-9]+)?))\b", pattern)
    if not m:
        return None
    return canonical_key(m.group(1))


class _Arm(NamedTuple):
    pattern: str
    value_text: str
    arrow_line: int
    span_lines: tuple
    block: int
    file: Optional[str]


def _scan_arms(lines):
    """Find every match arm in the reconstructed new-file text.

    Works on the joined text with a position -> line map so an arm may span
    lines -- TTF 5249a506's arms do:
        (PLATFORM_MACINTOSH, LANGUAGE_SPANISH_MACINTOSH)
        | (PLATFORM_WINDOWS, LANGUAGE_SPANISH_WINDOWS) => Some("es"),
    Brackets, strings and comments are tracked so `=>` inside a string, and the
    `,` inside `Some("Off, Did not fire")`, never split an arm.
    """
    if not lines:
        return [], {}
    text_parts = []
    pos_line = []
    for ln in lines:
        stripped = _strip_rust_comments(ln.text)
        text_parts.append(stripped)
        pos_line.extend([ln] * (len(stripped) + 1))
        text_parts.append("\n")
    text = "".join(text_parts)
    # pos_line was built per-line including the newline slot
    while len(pos_line) < len(text):
        pos_line.append(lines[-1])

    arms = []
    block_stack = []  # (block_id, open_pos)
    next_block = [1]
    # last separator position per depth
    sep = {0: -1}
    # the arm currently "open" whose value contains the next `{`
    pending_arm = {}
    block_parent_arm = {0: None}
    i = 0
    n = len(text)
    depth = 0
    last_arrow = {}
    while i < n:
        c = text[i]
        if c == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    break
                j += 1
            i = j + 1
            continue
        if c == "'":
            # char literal or lifetime; consume conservatively
            if i + 2 < n and text[i + 2] == "'":
                i += 3
                continue
            if i + 3 < n and text[i + 1] == "\\" and text[i + 3] == "'":
                i += 4
                continue
            i += 1
            continue
        if text.startswith("=>", i):
            start = sep.get(depth, -1) + 1
            pattern = text[start:i]
            value_text = text[i + 2 :]
            last_arrow[depth] = i
            arms.append((depth, start, i, pattern, value_text, block_stack[-1][0] if block_stack else 0))
            sep[depth] = i + 1  # so a following `,` re-anchors normally
            i += 2
            continue
        if c in "([{":
            if c == "{":
                bid = next_block[0]
                next_block[0] += 1
                block_stack.append((bid, i))
                parent_arm = None
                if last_arrow.get(depth) is not None and last_arrow[depth] < i:
                    between = text[last_arrow[depth] + 2 : i]
                    if between.strip() in ("", "match", "&") or between.strip().startswith("match"):
                        parent_arm = len(arms) - 1
                block_parent_arm[bid] = parent_arm
            depth += 1
            sep[depth] = i
            i += 1
            continue
        if c in ")]}":
            if c == "}" and block_stack:
                block_stack.pop()
            depth -= 1
            if depth < 0:
                depth = 0
            sep.setdefault(depth, -1)
            if c == "}":
                # A closing brace ends the enclosing arm/statement, so the next
                # pattern starts here. A closing PAREN or BRACKET must NOT move
                # the separator -- doing so ate the whole pattern of every
                # tuple arm, which is exactly the TTF 5249a506 shape
                # `(PLATFORM_MACINTOSH, LANGUAGE_SPANISH_MACINTOSH) => ...`.
                sep[depth] = i
            last_arrow.pop(depth + 1, None)
            i += 1
            continue
        if c == "," or c == ";":
            sep[depth] = i
            i += 1
            continue
        i += 1

    def line_at(p):
        p = max(0, min(p, len(pos_line) - 1))
        return pos_line[p]

    out = []
    for depth_, start, arrow, pattern, value_text, block in arms:
        pstart = start
        while pstart < arrow and text[pstart] in " \t\r\n":
            pstart += 1
        span = set()
        end = arrow + 2
        # value ends at the next top-level comma/newline for span purposes
        vend = end
        d = 0
        while vend < n:
            ch = text[vend]
            if ch in "([{":
                d += 1
            elif ch in ")]}":
                if d == 0:
                    break
                d -= 1
            elif ch == "," and d == 0:
                break
            vend += 1
        for p in range(pstart, min(vend + 1, len(pos_line))):
            span.add(pos_line[p])
        arrow_ln = line_at(arrow)
        out.append(
            _Arm(
                pattern=pattern.strip(),
                value_text=value_text,
                arrow_line=arrow_ln.number,
                span_lines=tuple(span),
                block=block,
                file=arrow_ln.file,
            )
        )
    return out, block_parent_arm


def _block_context(lines):
    """Per-line nearest preceding `fn` name and comment text, for weak binding."""
    fn_at = {}
    comment_at = {}
    last_fn = None
    last_comment = None
    for ln in lines:
        stripped = ln.text.strip()
        m = _FN_RE.match(ln.text)
        if m:
            last_fn = m.group(1)
        if stripped.startswith("//"):
            last_comment = stripped.lstrip("/").strip()
        elif stripped:
            pass
        fn_at[(ln.file, ln.number)] = last_fn
        comment_at[(ln.file, ln.number)] = last_comment
    return fn_at, comment_at


def extract_rust_pairs(diff_text, *, extra_consts=None):
    """Pull numeric-key -> display-string pairs out of the ADDED lines of a diff.

    Returns list[RustPair]; see RustPair for the field contract. The first four
    fields are (key, value, file, line).

    Handles the three shapes this codebase actually produces:

      plain match arms      `0 => "Win32",` / `0 => Some("Auto".to_string()),`
                            (RAR a998b8fc, RW2 e0900a27)

      const + tuple arms    `const LANGUAGE_SPANISH_MACINTOSH: u16 = 12;` plus
                            `(PLATFORM_MACINTOSH, LANGUAGE_SPANISH_MACINTOSH)
                             | (PLATFORM_WINDOWS, LANGUAGE_SPANISH_WINDOWS)
                             => Some("es"),`   (TTF 5249a506)
                            Consts are resolved from the diff's context lines as
                            well as its added lines, and `extra_consts` (a plain
                            {name: literal} dict) covers consts the diff does not
                            show at all.

      catch-all arms        `_ => "Unknown",` -> kind="catch-all". Reported
                            separately because ExifTool prints "Unknown (2)"
                            (ExifTool.pm:3629) for an unmatched key, so a
                            catch-all string REPLACES data rather than
                            contradicting a table entry. `_ => None` is the
                            correct fall-through and yields kind="catch-all"
                            with value=None, which verify() ignores.

    An arm counts as ADDED when any line it spans is a `+` line. Arms made
    entirely of context are ignored -- this tier judges what the commit
    introduces, not what it inherited.
    """
    lines = _new_file_lines(diff_text)
    if not lines:
        return []

    consts = dict(extra_consts or {})
    for ln in lines:
        m = _CONST_RE.match(ln.text)
        if m:
            consts[m.group(1)] = m.group(2)

    arms, block_parent_arm = _scan_arms(lines)
    fn_at, comment_at = _block_context(lines)

    def resolve_element(elem):
        """(const_name_or_None, canonical_key_or_None) for one tuple element."""
        text = elem.strip()
        if _is_numeric_key(text):
            return None, canonical_key(text)
        if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", text):
            lit = consts.get(text)
            return text, canonical_key(lit) if lit is not None else None
        return None, None

    # block id -> (block_key, block_label)
    block_meta = {}
    arm_by_index = arms

    def meta_for(block, arm):
        if block in block_meta:
            return block_meta[block]
        parent_idx = block_parent_arm.get(block)
        block_key = None
        if parent_idx is not None and 0 <= parent_idx < len(arm_by_index):
            block_key = _pattern_head_key(arm_by_index[parent_idx].pattern)
        label = comment_at.get((arm.file, arm.arrow_line)) or fn_at.get((arm.file, arm.arrow_line))
        block_meta[block] = (block_key, label)
        return block_meta[block]

    pairs = []
    for arm in arms:
        if not any(ln.added for ln in arm.span_lines):
            continue
        kind_value, value = _rust_arm_value(arm.value_text)
        block_key, label = meta_for(arm.block, arm)
        if arm.pattern.strip() == "_" or arm.pattern.strip().startswith("_ if"):
            pairs.append(
                RustPair(
                    key=None,
                    value=value if kind_value == "string" else None,
                    file=arm.file,
                    line=arm.arrow_line,
                    kind="catch-all",
                    block=arm.block,
                    block_key=block_key,
                    block_label=label,
                    key_parts=(),
                    pattern=arm.pattern,
                )
            )
            continue
        if kind_value != "string":
            continue
        for alt in _pattern_alternatives(arm.pattern):
            elements = _tuple_elements(alt)
            parts = tuple(resolve_element(e) for e in elements)
            resolved = [p[1] for p in parts if p[1] is not None]
            key = resolved[-1] if resolved else None
            pairs.append(
                RustPair(
                    key=key,
                    value=value,
                    file=arm.file,
                    line=arm.arrow_line,
                    kind="pair",
                    block=arm.block,
                    block_key=block_key,
                    block_label=label,
                    key_parts=parts,
                    pattern=alt,
                )
            )
    return pairs


# --------------------------------------------------------------------------
# verification
# --------------------------------------------------------------------------


def _subtable_names(pm_path, table_hint):
    """Sibling sub-table names of a nested hint, e.g. {Macintosh, Windows}."""
    parts = _split_hint(table_hint) if table_hint else []
    if len(parts) < 2:
        return set(), None
    path = Path(pm_path)
    if not path.is_file():
        return set(), None
    source = path.read_text(encoding="utf-8", errors="replace")
    bodies = _find_hash_bodies(source)
    if parts[0] not in bodies:
        return set(), None
    _, body = bodies[parts[0]]
    names = set()
    for key_tok, val_toks in _entries(body):
        key_text, quoted = _key_text(key_tok)
        if key_text is None:
            continue
        if val_toks[0][0] == _TOK_GROUP and val_toks[0][1][0] == "{":
            names.add(key_text)
    return names, parts[-1]


def _select_discriminant(pairs, subtables):
    """Which tuple position selects the sub-table? Returns index or None.

    TTF's arms are `(PLATFORM_MACINTOSH, LANGUAGE_SPANISH_MACINTOSH)`. Both
    positions carry a const whose NAME contains "MACINTOSH", so name-matching
    alone is ambiguous -- the tiebreak is cardinality: the discriminant position
    holds 2 distinct names (PLATFORM_MACINTOSH / PLATFORM_WINDOWS) across 8
    alternatives, the key position holds 8. We require a strict minimum, and we
    require that minimum to be smaller than the number of alternatives; anything
    else returns None and the caller says "cannot-verify".
    """
    tuples = [p for p in pairs if p.kind == "pair" and len(p.key_parts) > 1]
    if not tuples:
        return None
    width = len(tuples[0].key_parts)
    if any(len(p.key_parts) != width for p in tuples):
        return None
    upper = {s.upper() for s in subtables}
    scores = []
    for pos in range(width):
        names = [p.key_parts[pos][0] for p in tuples]
        if any(nm is None for nm in names):
            continue
        if not all(any(u in nm.upper() for u in upper) for nm in names):
            continue
        scores.append((len(set(names)), pos))
    if not scores:
        return None
    scores.sort()
    if len(scores) > 1 and scores[0][0] == scores[1][0]:
        return None
    if scores[0][0] >= len(tuples):
        return None
    return scores[0][1]


def _compare(pairs, table, *, exercised_keys=None):
    mismatches = []
    unreachable = []
    checked = 0
    reverse = {}
    for k, v in table.items():
        reverse.setdefault(v, []).append(k)
    for p in pairs:
        if p.key is None:
            unreachable.append(
                Unreachable(p.key, p.value, "unresolved-key", p.file, p.line)
            )
            continue
        if exercised_keys is not None and p.key not in exercised_keys:
            unreachable.append(
                Unreachable(p.key, p.value, "sample-cannot-exercise", p.file, p.line)
            )
            continue
        checked += 1
        expected = table.get(p.key)
        if expected == p.value:
            continue
        where = reverse.get(p.value)
        mismatches.append(
            Mismatch(
                key=p.key,
                rust_says=p.value,
                exiftool_says=expected,
                exiftool_key_for_value=",".join(where) if where else None,
                file=p.file,
                line=p.line,
            )
        )
    return mismatches, unreachable, checked


def verify(
    diff_text,
    pm_path,
    table_hint=None,
    *,
    extra_consts=None,
    exercised_keys=None,
    discriminant_position=None,
):
    """Do the pairs this diff adds match the ExifTool table the hint names?

    Args:
      diff_text   output of `git show <sha>` or any unified diff.
      pm_path     path to the ExifTool .pm holding the table.
      table_hint  which table. Accepts %Image::ExifTool::ZIP::RAR5, ZIP::RAR5,
                  RAR5, RAR5.OperatingSystem, %ttLang{Macintosh},
                  ttLang.Macintosh, or a bare tag name/id. A bare name that
                  matches more than one table (ZIP.pm has THREE disagreeing
                  OperatingSystem PrintConvs) yields "cannot-verify" naming the
                  candidates. Pass None to bind each Rust match block on its own
                  via the tag id of its enclosing arm -- which is how one diff
                  that adds two unrelated tables (RW2 e0900a27 adds
                  CustomRendered AND ExposureMode) gets adjudicated correctly
                  instead of being flattened into one colliding key space.
      extra_consts  {name: literal} for consts the diff does not show.
      exercised_keys  optional set of canonical keys the sample actually
                  exercises. Supplying it moves every other pair into
                  `unreachable` -- an explicit statement of the blind spot the
                  worker's `recheck-pass gaps=N->M` trailer silently has.
      discriminant_position  override the tuple-position heuristic.

    Returns a Verdict. status is "fabricated" if any pair mismatches or any
    string-valued catch-all arm was added, "cannot-verify" if the table could
    not be pinned down, "clean" otherwise.
    """
    pairs = extract_rust_pairs(diff_text, extra_consts=extra_consts)
    catch_alls = [
        CatchAll(p.value, p.file, p.line, True)
        for p in pairs
        if p.kind == "catch-all" and p.value is not None
    ]
    value_pairs = [p for p in pairs if p.kind == "pair"]

    if not value_pairs and not catch_alls:
        return Verdict("clean", [], [], [], reason="no-enum-pairs-in-diff", pairs_checked=0)

    if table_hint:
        detail = parse_perl_table_detail(pm_path, table_hint)
        if detail.pairs is None:
            return Verdict(
                "cannot-verify",
                [],
                [Unreachable(p.key, p.value, detail.reason or "unparseable", p.file, p.line) for p in value_pairs],
                catch_alls,
                reason=detail.reason,
                table=detail.name,
                candidates=detail.candidates,
            )
        subtables, selected = _subtable_names(pm_path, table_hint)
        selected_pairs = value_pairs
        unreachable = []
        # Sub-table discrimination only applies when the Rust side keys on a
        # TUPLE. `RAR5.OperatingSystem` also has two hint components and
        # OperatingSystem is also a `{...}` sibling inside RAR5, but the Rust
        # match there is `match raw { 0 => "Win32", ... }` -- a scalar key with
        # no discriminant to find. Without this gate the RAR fixture came back
        # "cannot-verify: no-subtable-discriminant" instead of "fabricated".
        has_tuple_keys = any(len(p.key_parts) > 1 for p in value_pairs)
        if has_tuple_keys and subtables and selected in subtables:
            pos = discriminant_position
            if pos is None:
                pos = _select_discriminant(value_pairs, subtables)
            if pos is None:
                return Verdict(
                    "cannot-verify",
                    [],
                    [Unreachable(p.key, p.value, "no-subtable-discriminant", p.file, p.line) for p in value_pairs],
                    catch_alls,
                    reason="no-subtable-discriminant",
                    table=detail.name,
                )
            keep = []
            for p in value_pairs:
                if len(p.key_parts) <= pos:
                    unreachable.append(Unreachable(p.key, p.value, "pattern-arity", p.file, p.line))
                    continue
                name = p.key_parts[pos][0]
                if name and selected.upper() in name.upper():
                    # the lookup key is the first resolved element that is NOT
                    # the discriminant
                    others = [
                        v for i, (_, v) in enumerate(p.key_parts) if i != pos and v is not None
                    ]
                    keep.append(p._replace(key=others[-1] if others else None))
                else:
                    unreachable.append(
                        Unreachable(p.key, p.value, "subtable-filtered:" + (name or "?"), p.file, p.line)
                    )
            selected_pairs = keep
        mismatches, more_unreachable, checked = _compare(
            selected_pairs, detail.pairs, exercised_keys=exercised_keys
        )
        unreachable.extend(more_unreachable)
        if mismatches or catch_alls:
            status = "fabricated"
        elif checked == 0:
            status = "cannot-verify"
        else:
            status = "clean"
        return Verdict(
            status,
            mismatches,
            unreachable,
            catch_alls,
            reason=None if checked else "nothing-compared",
            table=detail.name,
            pairs_checked=checked,
        )

    # --- per-block mode -------------------------------------------------
    blocks = {}
    for p in value_pairs:
        blocks.setdefault(p.block, []).append(p)
    mismatches = []
    unreachable = []
    checked = 0
    tables_used = []
    any_resolved = False
    reasons = []
    for block, bpairs in sorted(blocks.items()):
        hint = bpairs[0].block_key or bpairs[0].block_label
        if not hint:
            reasons.append("block-unbound")
            unreachable.extend(
                Unreachable(p.key, p.value, "block-unbound", p.file, p.line) for p in bpairs
            )
            continue
        detail = parse_perl_table_detail(pm_path, hint)
        if detail.pairs is None:
            reasons.append(detail.reason or "unparseable")
            unreachable.extend(
                Unreachable(p.key, p.value, detail.reason or "unparseable", p.file, p.line)
                for p in bpairs
            )
            continue
        any_resolved = True
        tables_used.append(detail.name)
        m, u, c = _compare(bpairs, detail.pairs, exercised_keys=exercised_keys)
        mismatches.extend(m)
        unreachable.extend(u)
        checked += c
    # A catch-all arm may only convict once a table has actually RESOLVED.
    #
    # Measured 2026-07-27, adjudicating 8 real archived patches against a human
    # pass: this returned "fabricated" for pdf-a1a411f67e3f and
    # elf-4b5a26e97cb8 with pairs_checked=0 and reason=no-such-table -- the
    # verdict rested entirely on a `_ =>` arm while no ExifTool table had been
    # found at all. Both are verified-good work (PDF measures a real -4 gap
    # closure; ELF's 9/9 CPUType and 5/5 ObjectFileType pairs match EXE.pm), so
    # both were FALSE REJECTS.
    #
    # Why resolution is the right precondition: a resolved table is known to
    # have no ExifTool fallback, because OTHER and BITMASK both force
    # PerlTable(pairs=None) upstream (see "table-has-OTHER"). So against a
    # resolved table, a Rust catch-all really does substitute a string where
    # ExifTool prints the raw number. With NO table resolved we cannot know
    # whether ExifTool has an OTHER sub covering exactly those keys, and a
    # `_ =>` fallback is idiomatic Rust present in most parsers -- convicting
    # on it unconditionally mis-fires broadly.
    #
    # Direction matters: "fabricated" is terminal, so a wrong one permanently
    # discards good work, while "cannot-verify" merely defers. When the
    # evidence is absent, defer.
    if mismatches:
        status = "fabricated"
    elif catch_alls and any_resolved:
        status = "fabricated"
    elif not any_resolved or checked == 0:
        status = "cannot-verify"
    else:
        status = "clean"
    return Verdict(
        status,
        mismatches,
        unreachable,
        catch_alls,
        reason=None if status == "clean" else (";".join(sorted(set(reasons))) or None),
        table=",".join(tables_used) or None,
        pairs_checked=checked,
    )


# --------------------------------------------------------------------------
# CLI
# --------------------------------------------------------------------------


def _default_run_git(repo_root):
    def run_git(args):
        return subprocess.run(
            ["git", *args],
            cwd=repo_root,
            capture_output=True,
            text=True,
            check=True,
        ).stdout

    return run_git


def main(argv=None, *, run_git=None, stdout=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--sha", help="commit to verify (uses `git show`)")
    parser.add_argument("--diff-file", help="read a unified diff from this file instead")
    parser.add_argument("--repo", default=".", help="repo root for --sha")
    parser.add_argument("--perl-lib", default=str(DEFAULT_PERL_LIB))
    parser.add_argument("--pm", required=True, help="ExifTool module, e.g. Font.pm or ZIP.pm")
    parser.add_argument("--table", help="table hint; omit for per-block auto-binding")
    parser.add_argument("--json", action="store_true", help="emit the verdict as JSON")
    args = parser.parse_args(argv)

    out = stdout or sys.stdout
    if args.diff_file:
        diff_text = Path(args.diff_file).read_text(encoding="utf-8", errors="replace")
    elif args.sha:
        runner = run_git or _default_run_git(args.repo)
        diff_text = runner(["show", "--format=%B", args.sha])
    else:
        diff_text = sys.stdin.read()

    pm_path = Path(args.pm)
    if not pm_path.is_absolute() and not pm_path.exists():
        pm_path = Path(args.perl_lib) / args.pm

    verdict = verify(diff_text, pm_path, args.table)
    if args.json:
        out.write(json.dumps(verdict.to_dict(), indent=2, sort_keys=True) + "\n")
    else:
        out.write(f"status: {verdict.status}\n")
        if verdict.reason:
            out.write(f"reason: {verdict.reason}\n")
        out.write(f"table: {verdict.table}\n")
        out.write(f"pairs_checked: {verdict.pairs_checked}\n")
        for c in verdict.candidates:
            out.write(f"  candidate: {c}\n")
        for m in verdict.mismatches:
            extra = f" (exiftool has {m.rust_says!r} at key {m.exiftool_key_for_value})" if m.exiftool_key_for_value else ""
            out.write(
                f"  MISMATCH key={m.key} rust={m.rust_says!r} exiftool={m.exiftool_says!r}{extra}"
                f"  [{m.file}:{m.line}]\n"
            )
        for c in verdict.catch_all_arms:
            out.write(
                f"  CATCH-ALL _ => {c.value!r} replaces ExifTool's \"Unknown (<n>)\""
                f"  [{c.file}:{c.line}]\n"
            )
        for u in verdict.unreachable:
            out.write(f"  unreachable key={u.key} value={u.value!r} ({u.reason})\n")
    return 0 if verdict.status == "clean" else 1


if __name__ == "__main__":
    raise SystemExit(main())
