#!/usr/bin/env python3
"""Generate Rust file-type identification tables from ExifTool's own hashes.

FileType, FileTypeExtension and MIMEType were the three most-missed tags in the
comparison corpus after the composites -- 129 instances across 43 files that
OxiDex could not identify at all. ExifTool answers all three from three plain
hashes, so this is transcription rather than reimplementation, and it cannot
drift from ExifTool because it *is* ExifTool's table.

Magic numbers are Perl regexes over raw bytes. They are translated here rather
than reused verbatim, because Rust's regex crate differs in two ways that
matter:

  * `\\0` is not a valid escape, so it becomes `\\x00`.
  * Unicode mode must be off (`(?-u)`) for byte classes above 0x7F to compile
    against a `&[u8]`, and `(?s)` is needed so `.` matches a newline byte --
    ExifTool's AIFF pattern `FORM....AIF[FC]` relies on that.

Anything that fails to translate is dropped and counted, never approximated:
a magic number that matches too eagerly would mislabel files, which is worse
than not identifying them.
"""

import argparse
import binascii
import json
import re

# Perl constructs with no direct Rust equivalent. A pattern using one is
# skipped rather than mangled into something that might still compile and then
# match the wrong files.
UNSUPPORTED = (
    r"(?{",     # embedded code
    r"(?<",     # lookbehind (Rust regex has none)
    r"(?=",     # lookahead
    r"(?!",     # negative lookahead
    r"\\G",     # anchor to previous match
)


def rust_bytes_literal(b):
    """Escape a byte pattern for a normal (non-raw) Rust string literal.

    ExifTool's magic numbers are regex *source*, so a backslash in them is a
    regex escape and must survive into the compiled pattern: `\\\\` in Rust
    source yields one backslash in the string, which the regex engine then
    reads as an escape.

    Bytes outside printable ASCII are emitted as the four characters `\\xNN`
    rather than as the byte itself. A Rust `&str` cannot hold `\\xNN` above
    0x7F at all, and even where it could, the regex engine reading an escape is
    equivalent and keeps the generated file pure ASCII.
    """
    out = []
    for ch in b:
        c = chr(ch)
        if c == "\\":
            out.append("\\\\")
        elif c == '"':
            out.append('\\"')
        elif 0x20 <= ch < 0x7F:
            out.append(c)
        else:
            out.append(f"\\\\x{ch:02x}")
    return "".join(out)


def translate(pattern_bytes):
    """Perl byte-regex -> Rust byte-regex, or None if unsupported."""
    p = pattern_bytes
    text = p.decode("latin-1")
    for bad in UNSUPPORTED:
        if bad in text:
            return None
    # `\0` -> `\x00`. Only when the backslash is not itself escaped.
    text = re.sub(r"(?<!\\)((?:\\\\)*)\\0(?![0-7])", r"\1\\x00", text)
    return text.encode("latin-1")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("filetypes_json")
    ap.add_argument("-o", "--out", required=True)
    args = ap.parse_args()

    with open(args.filetypes_json, encoding="utf-8") as fh:
        doc = json.load(fh)

    magic = doc["magic_number"]
    order = doc["test_order"]
    lookup = doc["file_type_lookup"]
    mime = doc["mime_type"]

    # ExifTool tests magic numbers in a fixed order and takes the first hit, so
    # the emitted order must be that order. Types with a magic number but no
    # place in the list are appended, deterministically, after it.
    seq = [t for t in order if t in magic]
    seq += sorted(t for t in magic if t not in seq)

    rows, skipped = [], []
    for t in seq:
        pat = translate(binascii.unhexlify(magic[t]))
        if pat is None:
            skipped.append(t)
            continue
        rows.append(f'    ("{t}", "(?s-u)^{rust_bytes_literal(pat)}"),')

    # Extension -> file type. Aliases are resolved here so the runtime does not
    # have to chase them; a cycle would hang, so depth is bounded.
    ext_rows = []
    for ext in sorted(lookup):
        entry, hops = lookup[ext], 0
        while "alias" in entry and hops < 8:
            nxt = lookup.get(entry["alias"])
            if not nxt:
                break
            entry, hops = nxt, hops + 1
        types = entry.get("types") or []
        if not types:
            continue
        # Lowercase the key: ExifTool stores extensions uppercase, but every
        # lookup here comes from a filename, so normalising once at generation
        # time keeps the runtime a plain comparison.
        ext_rows.append((ext.lower(), types[0]))

    # Sorted and deduplicated so the runtime can binary-search.
    ext_rows = [f'    ("{e}", "{t}"),' for e, t in sorted(dict(ext_rows).items())]

    mime_rows = [
        f'    ("{t}", "{mime[t]}"),' for t in sorted(mime) if mime[t]
    ]

    # ExifTool: `normExt = fileTypeExt{$fileType}` falling back to the type
    # name, printed lowercase.
    fte = doc.get("file_type_ext") or {}
    fte_rows = [f'    ("{t}", "{fte[t].lower()}"),' for t in sorted(fte)]

    with open(args.out, "w", encoding="utf-8") as fh:
        fh.write(f'''//! File-type identification tables, generated from ExifTool's own hashes.
//!
//! DO NOT EDIT. Regenerate with `just regen-tables`.
//!
//! `MAGIC` is in ExifTool's test order and must stay that way: ExifTool takes
//! the first pattern that matches, and several patterns overlap.

/// (file type, byte regex anchored at the start of the file).
pub static MAGIC: &[(&str, &str)] = &[
{chr(10).join(rows)}
];

/// Lowercase extension -> file type. Aliases already resolved.
pub static EXT_TO_TYPE: &[(&str, &str)] = &[
{chr(10).join(ext_rows)}
];

/// File type -> MIME type.
pub static MIME_TYPE: &[(&str, &str)] = &[
{chr(10).join(mime_rows)}
];

/// File type -> preferred extension, where it differs from the lowercased type.
///
/// ExifTool keeps this as a lexical hash, so it is sliced out of ExifTool.pm
/// and eval'd rather than read from the symbol table. Without it DICOM reports
/// its extension as `dicom` instead of `dcm`.
pub static FILE_TYPE_EXT: &[(&str, &str)] = &[
{chr(10).join(fte_rows)}
];
''')

    print(f"wrote {args.out}")
    print(f"  magic patterns   {len(rows)}")
    print(f"  extensions       {len(ext_rows)}")
    print(f"  mime types       {len(mime_rows)}")
    if skipped:
        print(f"  SKIPPED (unsupported regex): {', '.join(skipped)}")


if __name__ == "__main__":
    main()
