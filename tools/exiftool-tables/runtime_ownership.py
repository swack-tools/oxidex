#!/usr/bin/env python3
"""Build and verify the Exif::Main generated/runtime ownership boundary.

This is deliberately an ownership inventory, not a coverage instrument: rows
say who is allowed to produce a field, never that a corpus has observed it.
"""
from __future__ import annotations

import argparse
import dataclasses
import functools
import hashlib
import json
import re
from pathlib import Path
from typing import Literal, Sequence

SCHEMA = 1
OWNERS = {"generated", "walker-owned", "residual", "refused", "not-applicable"}
HERE = Path(__file__).resolve().parent
RAW_STRING_START = re.compile(r"r(#+)?\"")


class Refused(ValueError):
    """Inventory is malformed or cannot establish exclusive ownership."""


@dataclasses.dataclass(frozen=True, order=True)
class StableFieldId:
    module: str
    table: str
    kind: Literal["numeric", "name", "index"]
    value: str

    @classmethod
    def from_row(cls, row: dict) -> "StableFieldId":
        field = row.get("field")
        if not isinstance(field, dict) or field.get("kind") not in {"numeric", "name", "index"}:
            raise Refused(f"invalid stable identity: {row!r}")
        if not all(isinstance(row.get(key), str) and row[key] for key in ("module", "table")):
            raise Refused(f"invalid stable identity: {row!r}")
        if not isinstance(field.get("value"), str) or not field["value"]:
            raise Refused(f"invalid stable identity: {row!r}")
        return cls(row["module"], row["table"], field["kind"], field["value"])

    def text(self) -> str:
        return f"{self.module}::{self.table}:{self.kind}:{self.value}"


@dataclasses.dataclass(frozen=True)
class Verification:
    totals: dict[str, int]
    rows: int


def _canonical(rows: list[dict]) -> list[dict]:
    return sorted(rows, key=lambda row: (row["module"], row["table"], row["field"]["kind"], row["field"]["value"], row["owner"]))


def verify_rows(rows: Sequence[dict]) -> Verification:
    seen: dict[StableFieldId, dict] = {}
    totals = {owner: 0 for owner in sorted(OWNERS)}
    for row in rows:
        identity = StableFieldId.from_row(row)
        owner = row.get("owner")
        if owner not in OWNERS:
            raise Refused(f"unowned enabled field {identity.text()}: owner={owner!r}")
        if not isinstance(row.get("symbol"), str) or not row["symbol"]:
            raise Refused(f"unowned enabled field {identity.text()}: missing symbol")
        if identity in seen:
            previous = seen[identity]
            raise Refused(
                f"duplicate owner for {identity.text()}: "
                f"{previous['owner']} ({previous['symbol']}) vs {owner} ({row['symbol']})"
            )
        if owner == "refused" and (not isinstance(row.get("refusal"), str) or not row["refusal"].strip()):
            raise Refused(f"unowned enabled field {identity.text()}: refused row has no reason")
        if owner != "refused" and row.get("refusal") is not None:
            raise Refused(f"invalid provenance for {identity.text()}: non-refused row has refusal")
        if row.get("source_release") != "13.59":
            raise Refused(f"invalid provenance for {identity.text()}: source release")
        if not isinstance(row.get("source_sha256"), str) or not re.fullmatch(r"[0-9a-f]{64}", row["source_sha256"]):
            raise Refused(f"invalid provenance for {identity.text()}: source sha256")
        seen[identity] = row
        totals[owner] += 1
    return Verification(totals=totals, rows=len(rows))


def _source_hash(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


@functools.lru_cache(maxsize=32)
def _rust_code(text: str) -> str:
    """Blank comments and literals while retaining line boundaries for anchors."""
    out, i, block = [], 0, 0
    state = "code"
    raw_end = ""
    while i < len(text):
        if state == "code":
            if text.startswith("//", i): state = "line"; out.extend("  "); i += 2; continue
            if text.startswith("/*", i): state = "block"; block = 1; out.extend("  "); i += 2; continue
            raw = RAW_STRING_START.match(text, i)
            if raw:
                hashes = raw.group(1) or ""; raw_end = '"' + hashes; state = "raw"; out.extend(" " * len(raw.group(0))); i += len(raw.group(0)); continue
            if text[i] == '"': state = "string"; out.append(" "); i += 1; continue
            if text[i] == "'": state = "char"; out.append(" "); i += 1; continue
            out.append(text[i]); i += 1; continue
        if state == "line":
            if text[i] == "\n": state = "code"; out.append("\n")
            else: out.append(" ")
            i += 1; continue
        if state == "block":
            if text.startswith("/*", i): block += 1; out.extend("  "); i += 2; continue
            if text.startswith("*/", i): block -= 1; out.extend("  "); i += 2; state = "code" if block == 0 else "block"; continue
            out.append("\n" if text[i] == "\n" else " "); i += 1; continue
        if state == "raw" and text.startswith(raw_end, i): out.extend(" " * len(raw_end)); i += len(raw_end); state = "code"; continue
        if state in {"string", "char"} and text[i] == "\\" and i + 1 < len(text): out.extend("  "); i += 2; continue
        quote = '"' if state == "string" else "'"
        if state in {"string", "char"} and text[i] == quote: out.append(" "); i += 1; state = "code"; continue
        out.append("\n" if text[i] == "\n" else " "); i += 1
    return "".join(out)


def _array_values(text: str, name: str, constants: dict[str, str]) -> list[str]:
    match = re.search(rf"(?:pub\(crate\) )?const {name}: &\[u16\] = &\[(.*?)\];", _rust_code(text), re.S)
    if not match:
        raise Refused(f"missing residual array {name}")
    values = []
    for token in match.group(1).replace(",", " ").split():
        if token.startswith("0x"):
            values.append(f"0x{int(token, 16):04x}")
        elif token in constants:
            values.append(constants[token])
        else:
            raise Refused(f"unresolved residual constant {token} in {name}")
    return values


def _has_enabled_exif_main(text: str) -> bool:
    masked = _rust_code(text)
    for match in re.finditer(r'\("Exif", "Main"\)', text):
        # Parentheses survive only when the whole tuple is code; literals and
        # comments are blanked at every position by _rust_code.
        if masked[match.start()] == "(" and masked[match.end() - 1] == ")":
            return True
    return False


def _expected_residual_rows(root: Path) -> list[dict]:
    exif_engine = root / "src/core/exif_dir_engine.rs"
    tiff = root / "src/core/tiff_helpers.rs"
    e_text, t_text = exif_engine.read_text(), tiff.read_text()
    constants = {name: f"0x{int(value, 16):04x}" for name, value in re.findall(r"const (TAG_[A-Z_]+): u16 = (0x[0-9A-Fa-f]+);", _rust_code(t_text))}
    groups = (("IFD0", exif_engine, "IFD0_HAND_KEPT"), ("ExifIFD", tiff, "EXIF_IFD_HAND_KEPT"), ("IFD1", tiff, "IFD1_RESIDUAL_IDS"))
    rows = []
    for directory, path, symbol in groups:
        values = _array_values(path.read_text(), symbol, constants)
        for value in values:
            rows.append({"module": "Exif", "table": "Main", "field": {"kind": "index", "value": f"{directory}/{value}"},
                         "owner": "residual", "symbol": f"{path.relative_to(root)}::{symbol}", "source_release": "13.59",
                         "source_sha256": _source_hash(path), "refusal": None, "fixture": str(path.relative_to(root))})
    return rows


def _validate_paths_and_symbols(root: Path, rows: Sequence[dict]) -> None:
    for row in rows:
        identity = StableFieldId.from_row(row)
        fixture = row["fixture"]
        if not isinstance(fixture, str) or fixture.startswith("/") or ".." in Path(fixture).parts:
            raise Refused(f"invalid fixture for {identity.text()}")
        path = root / fixture
        if not path.is_file():
            raise Refused(f"missing fixture for {identity.text()}: {fixture}")
        symbol = row["symbol"]
        if row["owner"] == "not-applicable":
            expected = f"ledger:conv_exif_main_ledger.json:not_conversion_fields:{identity.value}"
            if symbol != expected or fixture != "tools/exiftool-tables/conv_exif_main_ledger.json":
                raise Refused(f"invalid structural ledger reference for {identity.text()}")
            ledger = json.loads(path.read_text())
            if not any(entry.get("id") == identity.value and entry.get("reason") for entry in ledger.get("not_conversion_fields", [])):
                raise Refused(f"missing structural ledger row for {identity.text()}")
            continue
        if "::" not in symbol:
            raise Refused(f"missing symbol for {identity.text()}: {symbol}")
        symbol_path, name = symbol.rsplit("::", 1)
        actual = root / symbol_path
        declaration = rf"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:fn|const|static)\s+{re.escape(name)}\b"
        if not actual.is_file() or not re.search(declaration, _rust_code(actual.read_text())):
            raise Refused(f"missing symbol for {identity.text()}: {symbol}")
        if row["owner"] == "residual" and row["source_sha256"] != _source_hash(path):
            raise Refused(f"invalid provenance for {identity.text()}: carrier sha256")


def _fragment_digest(directory: Path) -> tuple[str, list[dict]]:
    paths = sorted(directory.glob("*.json"))
    digest = hashlib.sha256()
    rows: list[dict] = []
    for path in paths:
        raw = path.read_bytes()
        digest.update(path.name.encode() + b"\0" + raw)
        value = json.loads(raw)
        if not isinstance(value, list):
            raise Refused(f"fragment {path} is not a row list")
        rows.extend(value)
    return digest.hexdigest(), rows


def _base_rows(root: Path) -> list[dict]:
    registry = (root / "src/exiftool_tables/enabled_ifd.rs").read_text()
    if not _has_enabled_exif_main(registry):
        raise Refused("enabled-table registry does not enable Exif::Main")
    ledger_path = root / "tools/exiftool-tables/conv_exif_main_ledger.json"
    ledger = json.loads(ledger_path.read_text())
    release = ledger["exiftool_version"]
    fixture = str(ledger_path.relative_to(root))
    def row(entry: dict, owner: str, symbol: str, refusal: str | None = None) -> dict:
        return {"module": "Exif", "table": "Main", "field": {"kind": "numeric", "value": entry["id"]},
                "owner": owner, "symbol": symbol, "source_release": release,
                "source_sha256": entry.get("source_sha256", ledger["table_sha256"]),
                "refusal": refusal, "fixture": fixture}
    rows = [row(entry, "generated", "src/exiftool_tables/conv/exif_main.rs::decode") for entry in ledger["generated"]]
    rows.extend(row(entry, "refused", "src/exiftool_tables/conv/exif_main.rs::REFUSED", entry["reason"]) for entry in ledger["refused"])
    rows.extend(row(entry, "not-applicable", f"ledger:conv_exif_main_ledger.json:not_conversion_fields:{entry['id']}") for entry in ledger["not_conversion_fields"])
    return rows


def build_inventory(root: Path) -> dict:
    digest, fragments = _fragment_digest(root / "tools/exiftool-tables/runtime_ownership.d")
    expected_residuals = _expected_residual_rows(root)
    if _canonical(fragments) != _canonical(expected_residuals):
        raise Refused("residual fragments differ from live Rust residual arrays")
    rows = _base_rows(root) + expected_residuals
    checked = verify_rows(rows)
    _validate_paths_and_symbols(root, rows)
    ledger = json.loads((root / "tools/exiftool-tables/conv_exif_main_ledger.json").read_text())
    return {"schema": SCHEMA, "source_release": ledger["exiftool_version"],
            "source_tree_sha256": ledger["table_sha256"], "fragment_inputs_sha256": digest,
            "category_totals": checked.totals, "rows": _canonical(rows)}


def load_rows(root: Path) -> list[dict]:
    inventory = json.loads((root / "tools/exiftool-tables/runtime_ownership.json").read_text())
    if inventory.get("schema") != SCHEMA or not isinstance(inventory.get("rows"), list):
        raise Refused("unsupported runtime ownership inventory")
    expected = build_inventory(root)
    for key in ("source_release", "source_tree_sha256", "fragment_inputs_sha256", "category_totals", "rows"):
        if inventory.get(key) != expected[key]:
            raise Refused(f"runtime ownership inventory is stale or tampered: {key}")
    verify_rows(inventory["rows"])
    _validate_paths_and_symbols(root, inventory["rows"])
    return inventory["rows"]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("verify", "write"))
    parser.add_argument("--root", type=Path, default=Path("."))
    args = parser.parse_args()
    root = args.root.resolve()
    if args.command == "write":
        fragment = root / "tools/exiftool-tables/runtime_ownership.d/exif_main_residuals.json"
        fragment.write_text(json.dumps(_expected_residual_rows(root), indent=2, sort_keys=True) + "\n")
        output = root / "tools/exiftool-tables/runtime_ownership.json"
        output.write_text(json.dumps(build_inventory(root), indent=2, sort_keys=True) + "\n")
    else:
        verification = verify_rows(load_rows(root))
        print(f"runtime ownership verified: {verification.rows} rows; {verification.totals}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
