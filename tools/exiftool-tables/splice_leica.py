#!/usr/bin/env python3
"""Splice `scripts/gen_leica_lens_types.pl`'s output into `lens_data.rs`.

Every other tier-2 generator writes its own dedicated file; this one is the
exception. `LEICA_LENS_TYPES` was hand-placed inside `pub mod leica { ... }`
in `src/parsers/tiff/makernotes/lens_data.rs` -- a file that otherwise holds
several *other* manufacturers' hand-authored or differently-generated lens
databases -- so there is no whole file to overwrite. This script replaces
just the `pub static LEICA_LENS_TYPES: ... = [ ... ];` array between its own
start and end markers, leaving every surrounding line (including the doc
comments and the `lookup`/`value_conv` helpers below it) untouched, then lets
rustfmt reconcile formatting the same way it would for a hand edit.

Usage: splice_leica.py <generator-stdout> <lens_data.rs>
"""
import re
import sys
from pathlib import Path

START_RE = re.compile(r"^ {4}pub static LEICA_LENS_TYPES: .*= \[\s*$")


def main() -> None:
    if len(sys.argv) != 3:
        print("usage: splice_leica.py <generator-stdout> <lens_data.rs>", file=sys.stderr)
        raise SystemExit(2)
    gen_path, target_path = Path(sys.argv[1]), Path(sys.argv[2])

    gen_lines = gen_path.read_text().splitlines(keepends=True)
    # The generator's own output starts with two `//` header lines and a
    # blank line before the static; keep everything from the `pub static`
    # line onward -- that is the block we are replacing in place.
    gen_start = next(i for i, l in enumerate(gen_lines) if l.startswith("    pub static LEICA_LENS_TYPES"))
    new_block = gen_lines[gen_start:]

    target_lines = target_path.read_text().splitlines(keepends=True)
    start = next(i for i, l in enumerate(target_lines) if START_RE.match(l))
    # The matching close is the first top-level-in-block `    ];` at or after
    # start -- indentation distinguishes it from any `];` nested deeper.
    end = next(i for i in range(start, len(target_lines)) if target_lines[i] == "    ];\n")

    spliced = target_lines[:start] + new_block + target_lines[end + 1 :]
    target_path.write_text("".join(spliced))
    print(f"spliced {len(new_block) - 1} entries into {target_path}")


if __name__ == "__main__":
    main()
