#!/usr/bin/env python3
"""Generate the Rust Macintosh CJK charset tables from ExifTool's own Perl tables.

The four scripts used by TrueType `name` records on the Macintosh platform
(MacJapanese, MacChineseTW, MacKorean, MacChineseCN) are Apple's variants of
Shift_JIS / Big5 / EUC-KR / GBK, and they are *not* interchangeable with the
standard codecs. The only way to reproduce ExifTool's output byte for byte is
to carry ExifTool's tables verbatim, so this script transcribes them
mechanically rather than by hand.

Source of truth: `Image/ExifTool/Charset/Mac*.pm` from the ExifTool release
`.exiftool-version` pins -- never "an installed ExifTool", and never a bare
`exiftool` off PATH (AGENTS.md: PATH resolved to 13.55 while the tables were
transcribed from 13.59, and the two disagree). `tools/exiftool-tables/
regen-all.sh` tier 2e runs this script against the one resolved tree the rest
of the bump uses, and CI's verify-tables job re-runs it and diffs the result
against what is committed.
Each of those files is a Perl hash whose keys are single bytes and whose values
are one of:

  * a scalar codepoint            -- the byte maps to one Unicode character
  * an array ref of codepoints    -- the byte maps to several characters
  * a nested hash                 -- the byte is the lead byte of a 2-byte
                                     sequence, and the nested hash maps the
                                     trail byte the same way

`Charset.pm` never nests deeper than that (see its `Decompose` routine, the
"variable-width characters" branch), which this script asserts.

Usage (normally via `tools/exiftool-tables/regen-all.sh`, which supplies the
pinned tree and runs rustfmt afterwards):

    python3 generate_tables.py \
        target/exiftool-src/exiftool-$(cat .exiftool-version)/lib/Image/ExifTool/Charset

Writes `mac_japanese.rs`, `mac_chinese_tw.rs`, `mac_korean.rs` and
`mac_chinese_cn.rs` next to this script. Run `cargo fmt` afterwards --
without it the only difference from the committed files is the wrapping of
the `_LEADS` array literal, which is why CI can diff the output byte for
byte.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

# Perl hash key/value name -> (source file, Rust module, Rust statics prefix).
CHARSETS = [
    ("MacJapanese", "mac_japanese", "MAC_JAPANESE"),
    ("MacChineseTW", "mac_chinese_tw", "MAC_CHINESE_TW"),
    ("MacKorean", "mac_korean", "MAC_KOREAN"),
    ("MacChineseCN", "mac_chinese_cn", "MAC_CHINESE_CN"),
]

TOKEN = re.compile(r"0x[0-9a-fA-F]+|=>|[\[\]{},()]|#[^\n]*")


def tokenize(text: str) -> list[str]:
    return [m.group(0) for m in TOKEN.finditer(text) if not m.group(0).startswith("#")]


class Parser:
    def __init__(self, tokens: list[str]) -> None:
        self.tokens = tokens
        self.pos = 0

    def peek(self) -> str | None:
        return self.tokens[self.pos] if self.pos < len(self.tokens) else None

    def take(self) -> str:
        token = self.tokens[self.pos]
        self.pos += 1
        return token

    def expect(self, want: str) -> None:
        got = self.take()
        if got != want:
            raise ValueError(f"expected {want!r}, got {got!r} at token {self.pos}")

    def parse_hash_body(self, close: str) -> dict:
        out: dict[int, object] = {}
        while True:
            token = self.peek()
            if token is None:
                raise ValueError("unterminated hash")
            if token == close:
                self.take()
                return out
            if token == ",":
                self.take()
                continue
            key = int(self.take(), 16)
            self.expect("=>")
            out[key] = self.parse_value()

    def parse_value(self) -> object:
        token = self.take()
        if token == "[":
            values = []
            while True:
                item = self.take()
                if item == "]":
                    return values
                if item != ",":
                    values.append(int(item, 16))
        if token == "{":
            return self.parse_hash_body("}")
        return int(token, 16)


def load_table(path: Path, name: str) -> dict:
    text = path.read_text(encoding="utf-8")
    marker = f"%Image::ExifTool::Charset::{name} = ("
    start = text.index(marker) + len(marker) - 1
    body = text[start:]
    body = body[: body.rindex(");") + 2]
    parser = Parser(tokenize(body))
    parser.expect("(")
    return parser.parse_hash_body(")")


def rust_string(codepoints: list[int]) -> str:
    for cp in codepoints:
        if cp > 0x10FFFF or 0xD800 <= cp <= 0xDFFF:
            raise ValueError(f"codepoint U+{cp:04X} is not a Unicode scalar value")
    return '"' + "".join(f"\\u{{{cp:x}}}" for cp in codepoints) + '"'


def render(name: str, module: str, prefix: str, table: dict, source: Path) -> str:
    singles: list[tuple[int, list[int]]] = []
    leads: list[int] = []
    doubles: list[tuple[int, list[int]]] = []

    for byte, value in sorted(table.items()):
        if isinstance(value, dict):
            leads.append(byte)
            for trail, sub in sorted(value.items()):
                if isinstance(sub, dict):
                    raise ValueError(f"{name}: unexpected 3-byte nesting at {byte:#x}")
                doubles.append(
                    ((byte << 8) | trail, sub if isinstance(sub, list) else [sub])
                )
        else:
            singles.append((byte, value if isinstance(value, list) else [value]))

    # `Decompose` treats a table value of 0 as "no mapping" (Perl truthiness),
    # so a real 0 entry would be ambiguous. ExifTool has none; make sure.
    for _, cps in singles + doubles:
        if cps == [0]:
            raise ValueError(f"{name}: table maps a byte to codepoint 0")

    lines: list[str] = []
    out = lines.append
    out(f"//! `{name}` byte sequence -> Unicode, carried verbatim from ExifTool.")
    out("//!")
    out(f"//! Generated from `{source.name}` (ExifTool) by `generate_tables.py`.")
    out("//! DO NOT EDIT BY HAND -- re-run the generator instead.")
    out("")
    out("use super::MacCharset;")
    out("")
    out(f"pub(super) static {prefix}: MacCharset = MacCharset {{")
    out(f"    single: {prefix}_SINGLE,")
    out(f"    leads: {prefix}_LEADS,")
    out(f"    double: {prefix}_DOUBLE,")
    # MacJapanese is ExifTool csType 0x883; the 0x080 bit means "some bytes
    # below 0x80 are remapped" (0x5c is YEN SIGN, not REVERSE SOLIDUS). The
    # other three are 0x803 and leave ASCII alone.
    remaps = "true" if any(byte < 0x80 for byte, _ in singles) or any(
        byte < 0x80 for byte in leads
    ) else "false"
    out(f"    remaps_ascii: {remaps},")
    out("};")
    out("")
    out("/// Bytes that stand alone, sorted by byte.")
    out(f"static {prefix}_SINGLE: &[(u8, &str)] = &[")
    for byte, cps in singles:
        out(f"    ({byte:#04x}, {rust_string(cps)}),")
    out("];")
    out("")
    out("/// Bytes that introduce a two-byte sequence, sorted.")
    out(
        f"static {prefix}_LEADS: &[u8] = &["
        + ", ".join(f"{byte:#04x}" for byte in leads)
        + "];"
    )
    out("")
    out("/// `lead << 8 | trail` -> Unicode, sorted by key.")
    out(f"static {prefix}_DOUBLE: &[(u16, &str)] = &[")
    for key, cps in doubles:
        out(f"    ({key:#06x}, {rust_string(cps)}),")
    out("];")
    out("")
    return "\n".join(lines)


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__)
        return 2
    charset_dir = Path(sys.argv[1])
    out_dir = Path(__file__).resolve().parent
    for name, module, prefix in CHARSETS:
        source = charset_dir / f"{name}.pm"
        table = load_table(source, name)
        (out_dir / f"{module}.rs").write_text(
            render(name, module, prefix, table, source), encoding="utf-8"
        )
        print(f"wrote {module}.rs from {source}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
