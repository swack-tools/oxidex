#!/usr/bin/env python3
"""One generated file per ExifTool module: the on-disk layout of the binary
and IFD table artifacts, and the only place that knows it.

`codegen.py` used to write `src/exiftool_tables/binary_tables.rs` (6.8 MB)
and `ifd_tables.rs` (4.4 MB) as single files. Each is now a directory::

    src/exiftool_tables/binary/mod.rs     the hub: prelude, schema types, the
                                          shared `ExprId` enum, one `mod
                                          <stem>;` + `pub use <stem>::*;` per
                                          ExifTool module, then the index and
                                          the omission sidecar
    src/exiftool_tables/binary/canon.rs   every `pub static` of one module
    ...

`src/exiftool_tables/mod.rs` mounts each hub under its old module name
(`#[path = "binary/mod.rs"] pub mod binary_tables;`) and the glob
re-exports put every table static back at the path it always had, so no
Rust consumer changed; `find_table` still walks `ALL_BINARY_TABLES`.

Text consumers (`verify.py`, `reachability.py`, `retire_binary_tables.py`,
...) read the artifact through `read_logical`, which splices each module
file into the hub at its `mod` line. A regex that used to run over the
monolith runs over the same declarations in the same order; only the
per-file headers (`//!` doc, `#![allow]`, `use super::*;`) are new text.
A path that is not named `mod.rs` is read as the single file it is, so the
hand-written fixtures (`fixtures/ifd_tables_sample.rs`) and any temp copy a
test writes keep working unchanged.

Layout rules the generator and the readers share:

* the hub is always named `mod.rs`; `write_files` refuses any other name,
  because rustc resolves a hub's `mod x;` to `x.rs` beside it only for a
  file with that name (or with a `#[path]` per child, which nothing emits);
* the file stem of module `Canon` is `module_stem("Canon") == "canon"`:
  lower-cased, every non-alphanumeric as `_`; a stem that is a Rust keyword,
  starts with a digit, or collides with another module's is refused;
* the hub declares `mod` lines in `sorted(stems)` order, which is the order
  rustfmt's `reorder_modules`/`reorder_imports` would put them in anyway
  (checked against rustfmt 1.98 for both artifacts), so `format_artifacts`
  never reorders what the generator wrote and the logical text a reader
  splices is the text the generator rendered;
* `write_files` deletes a sibling `*.rs` the new file set no longer names
  -- a module the pinned release dropped would otherwise stay behind as an
  orphan nothing declares, invisible to rustc and visible to every text
  consumer -- but only when the file carries the generated-header marker,
  so it can never remove a hand-written file.
"""
from __future__ import annotations

import re
from pathlib import Path
from typing import Callable, Iterable, Mapping

MOD_RS = "mod.rs"
# Every generated table file (hub or module) says this in its `//!` header.
GENERATED_MARKER = "DO NOT EDIT"
MOD_LINE_RE = re.compile(r"^mod ([a-z0-9_]+);$", re.M)

_RUST_KEYWORDS = frozenset(
    "as break const continue crate else enum extern false fn for if impl in let "
    "loop match mod move mut pub ref return self Self static struct super trait "
    "true type unsafe use where while async await dyn abstract become box do "
    "final macro override priv typeof unsized virtual yield try gen".split()
)


def module_stem(mod_name: str) -> str:
    """File stem (and Rust module name) for one ExifTool module name."""
    stem = re.sub(r"[^a-z0-9]", "_", str(mod_name).lower())
    if not stem or stem[0].isdigit() or stem in _RUST_KEYWORDS or stem == "mod":
        raise SystemExit(
            f"ExifTool module {mod_name!r} has no usable Rust module name "
            f"({stem!r}); extend table_modules.module_stem before regenerating"
        )
    return stem


def is_split(path) -> bool:
    return Path(path).name == MOD_RS


def tables_dir(path) -> Path:
    """`src/exiftool_tables` for either layout of an artifact inside it."""
    path = Path(path)
    return path.parent.parent if is_split(path) else path.parent


def sibling_artifact(path, split_dir: str, legacy_name: str) -> Path:
    """The sister artifact of `path` in whichever layout `path` uses: the
    IFD hub beside a binary hub (`binary/mod.rs` -> `ifd/mod.rs`), or the
    legacy single file beside a legacy single file."""
    path = Path(path)
    if is_split(path):
        return path.parent.parent / split_dir / MOD_RS
    return path.with_name(legacy_name)


def render_hub(head: str, stems: Iterable[str], tail: str) -> str:
    stems = sorted(set(stems))
    decls = "".join(f"mod {stem};\n" for stem in stems)
    uses = "".join(f"pub use {stem}::*;\n" for stem in stems)
    return f"{head}{decls}\n{uses}{tail}"


def render_files(head: str, tail: str, module_header: Callable[[str], str],
                 chunks: Iterable[str]) -> dict[str, str]:
    """Lay out `chunks` (each tagged with `.module`, see `codegen.TableChunk`)
    as `{"mod.rs": hub, "<stem>.rs": header + that module's chunks, ...}`.

    Chunk order within a module is preserved; the hub's `mod` order is
    `sorted(stems)`. A chunk without a module tag is refused rather than
    filed somewhere plausible.
    """
    by_stem: dict[str, list[str]] = {}
    names: dict[str, str] = {}
    for chunk in chunks:
        module = getattr(chunk, "module", None)
        if not isinstance(module, str) or not module:
            raise SystemExit("table chunk carries no ExifTool module name; cannot file it")
        stem = module_stem(module)
        owner = names.setdefault(stem, module)
        if owner != module:
            raise SystemExit(
                f"module file collision: ExifTool modules {owner!r} and {module!r} "
                f"both map to {stem}.rs"
            )
        by_stem.setdefault(stem, []).append(chunk)
    files = {f"{stem}.rs": module_header(names[stem]) + "".join(parts)
             for stem, parts in by_stem.items()}
    files[MOD_RS] = render_hub(head, by_stem, tail)
    return files


def is_generated(path: Path) -> bool:
    try:
        head = path.read_text(encoding="utf-8", errors="replace")[:4096]
    except OSError:
        return False
    return head.startswith("//!") and GENERATED_MARKER in head


def write_files(mod_rs, files: Mapping[str, str]) -> list[Path]:
    """Write a rendered file set beside `mod_rs`; returns the stale generated
    siblings it removed."""
    mod_rs = Path(mod_rs)
    if mod_rs.name != MOD_RS:
        raise SystemExit(
            f"{mod_rs}: a split table artifact is written as <dir>/{MOD_RS} "
            "(one file per ExifTool module beside it), e.g. "
            "src/exiftool_tables/binary/mod.rs"
        )
    if MOD_RS not in files:
        raise SystemExit("rendered file set has no hub")
    directory = mod_rs.parent
    directory.mkdir(parents=True, exist_ok=True)
    for name, text in files.items():
        (directory / name).write_text(text, encoding="utf-8")
    removed = []
    for path in sorted(directory.glob("*.rs")):
        if path.name in files:
            continue
        if not is_generated(path):
            raise SystemExit(
                f"{path} is not a generated table file (no `//! ... {GENERATED_MARKER}` "
                "header) but sits in a generated directory; refusing to touch it"
            )
        path.unlink()
        removed.append(path)
    return removed


def read_files_with(reader: Callable[[str], str | None], hub_name: str = MOD_RS) -> dict[str, str]:
    """`{name: text}` for a hub and every module it declares, through
    `reader(name)` (a filesystem or `git show` lookup); a declared module the
    reader cannot supply is an error, never a silently shorter artifact."""
    hub = reader(hub_name)
    if hub is None:
        raise SystemExit(f"table artifact hub {hub_name} is missing")
    files = {MOD_RS: hub}
    for stem in MOD_LINE_RE.findall(hub):
        name = f"{stem}.rs"
        text = reader(name)
        if text is None:
            raise SystemExit(f"hub declares `mod {stem};` but {name} is missing beside it")
        files[name] = text
    return files


def read_files(mod_rs) -> dict[str, str]:
    mod_rs = Path(mod_rs)

    def reader(name: str) -> str | None:
        path = mod_rs.with_name(name)
        return path.read_text(encoding="utf-8") if path.is_file() else None

    return read_files_with(reader)


def logical_text(files: Mapping[str, str]) -> str:
    """The artifact as one text: the hub with every module file spliced in
    at its `mod` line. Not valid Rust (each module keeps its `//!` header);
    it exists so a text consumer sees the monolith's declaration order."""
    hub = files[MOD_RS]

    def splice(match: re.Match) -> str:
        name = f"{match.group(1)}.rs"
        if name not in files:
            raise SystemExit(f"hub declares `mod {match.group(1)};` but the file set has no {name}")
        return files[name]

    return MOD_LINE_RE.sub(splice, hub)


def read_logical(path) -> str:
    """`logical_text` of a split artifact, or the text of a single file."""
    path = Path(path)
    if is_split(path):
        return logical_text(read_files(path))
    return path.read_text(encoding="utf-8")


def declared_stems(mod_rs) -> list[str]:
    return MOD_LINE_RE.findall(Path(mod_rs).read_text(encoding="utf-8"))
