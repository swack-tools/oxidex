#!/usr/bin/env python3
"""Build and verify generated/runtime ownership fragments.

This is deliberately an ownership inventory, not a coverage instrument: rows
say who is allowed to produce a field, never that a corpus has observed it.
The legacy Exif::Main residual fragment retains its exact live-source fence.
"""
from __future__ import annotations

import argparse
import dataclasses
import functools
import hashlib
import json
import re
from pathlib import Path, PurePosixPath
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
        if (
            not isinstance(field, dict)
            or set(field) != {"kind", "value"}
            or field.get("kind") not in {"numeric", "name", "index"}
        ):
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


@dataclasses.dataclass(frozen=True)
class FragmentSet:
    digest: str
    ownership_rows: dict[str, list[dict]]
    candidate_documents: dict[str, dict]


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


def _portable_relative(value: object, label: str) -> str:
    if not isinstance(value, str) or not value or value in {".", ".."}:
        raise Refused(f"invalid {label}: relative path")
    if (
        "\\" in value
        or "$" in value
        or "~" in value
        or any(ord(character) < 0x20 or ord(character) == 0x7F for character in value)
    ):
        raise Refused(f"invalid {label}: relative path")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {".", "..", ""} for part in path.parts):
        raise Refused(f"invalid {label}: relative path")
    if str(path) != value:
        raise Refused(f"invalid {label}: relative path")
    return value


def _regular_contained_file(base: Path, relative: str, label: str) -> Path:
    candidate = base / relative
    current = base
    for part in PurePosixPath(relative).parts:
        current = current / part
        if current.is_symlink():
            raise Refused(f"invalid {label}: symlink")
    try:
        candidate.resolve().relative_to(base.resolve())
    except ValueError as error:
        raise Refused(f"invalid {label}: containment") from error
    if not candidate.exists():
        raise Refused(f"missing {label}: {relative}")
    if not candidate.is_file():
        raise Refused(f"invalid {label}: regular file required")
    return candidate


def _validate_reference(
    root: Path,
    reference: object,
    *,
    label: str,
    hashed: bool,
    allowed_roots: set[str],
    ops_root: Path | None,
    native_carrier: bool = False,
) -> None:
    required = {"root", "relative_path", "sha256"} if hashed else {"root", "relative_path"}
    if not isinstance(reference, dict) or set(reference) != required:
        raise Refused(f"invalid {label} keys")
    root_token = reference.get("root")
    if root_token not in allowed_roots:
        raise Refused(f"invalid {label}: root token")
    relative = _portable_relative(reference.get("relative_path"), label)
    if native_carrier and PurePosixPath(relative).suffix == ".rs":
        raise Refused(f"invalid {label}: Rust implementation is not a native carrier")
    if hashed and (
        not isinstance(reference.get("sha256"), str)
        or not re.fullmatch(r"[0-9a-f]{64}", reference["sha256"])
    ):
        raise Refused(f"invalid {label}: sha256")
    base = root if root_token == "REPOSITORY" else ops_root
    if base is None:
        return
    path = _regular_contained_file(base, relative, label)
    if hashed and _source_hash(path) != reference["sha256"]:
        raise Refused(f"invalid {label}: carrier sha256")


def _validate_symbol(root: Path, symbol: object, label: str) -> None:
    if not isinstance(symbol, str) or symbol.count("::") != 1:
        raise Refused(f"invalid {label}: {symbol}")
    symbol_path, name = symbol.rsplit("::", 1)
    _portable_relative(symbol_path, label)
    if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*", name):
        raise Refused(f"invalid {label}: {symbol}")
    actual = root / symbol_path
    declaration = rf"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:fn|const|static)\s+{re.escape(name)}\b"
    if not actual.is_file() or not re.search(declaration, _rust_code(actual.read_text())):
        raise Refused(f"missing {label}: {symbol}")


def _validate_source_binding(root: Path, row: dict, identity: StableFieldId) -> None:
    expected_row_keys = {
        "module", "table", "field", "owner", "symbol", "source_release",
        "source_sha256", "provenance_schema", "source_binding", "refusal", "fixture",
    }
    if set(row) != expected_row_keys:
        raise Refused(f"invalid provenance for {identity.text()}: source-carrier row keys")
    if row.get("provenance_schema") != "source-carrier/v1":
        raise Refused(f"invalid provenance for {identity.text()}: source-carrier/v1 required")
    binding = row.get("source_binding")
    binding_keys = {"ledger", "full_name", "raw_key", "name", "variant_path"}
    if not isinstance(binding, dict) or set(binding) != binding_keys:
        raise Refused(f"invalid provenance for {identity.text()}: source binding keys")
    if binding.get("ledger") != "tools/exiftool-tables/ifd_identity_ledger.json":
        raise Refused(f"invalid provenance for {identity.text()}: source ledger")
    if not all(isinstance(binding.get(key), str) and binding[key] for key in ("full_name", "raw_key", "name")):
        raise Refused(f"invalid provenance for {identity.text()}: source binding types")
    if (
        not isinstance(binding.get("variant_path"), list)
        or not all(isinstance(index, int) and not isinstance(index, bool) and index >= 0 for index in binding["variant_path"])
    ):
        raise Refused(f"invalid provenance for {identity.text()}: source binding types")
    ledger_path = root / binding["ledger"]
    if not ledger_path.is_file():
        raise Refused(f"invalid provenance for {identity.text()}: source ledger")
    ledger = json.loads(ledger_path.read_text())
    if ledger.get("schema") != "oxidex_ifd_identity_ledger_v1" or not isinstance(ledger.get("rows"), list):
        raise Refused(f"invalid provenance for {identity.text()}: source ledger schema")
    matches = [
        entry for entry in ledger.get("rows", [])
        if all(entry.get(key) == binding[key] for key in ("full_name", "raw_key", "name", "variant_path"))
    ]
    if len(matches) != 1:
        raise Refused(f"invalid provenance for {identity.text()}: unique source binding")
    match = matches[0]
    if match.get("module") != identity.module or match.get("table") != identity.table:
        raise Refused(f"invalid provenance for {identity.text()}: stable identity")
    if identity.kind == "numeric":
        try:
            raw_identity = int(match["raw_key"], 0)
            same_identity = raw_identity >= 0 and identity.value == f"0x{raw_identity:04x}"
        except (TypeError, ValueError):
            same_identity = False
    elif identity.kind == "name":
        same_identity = identity.value == match.get("name")
    else:
        same_identity = identity.value == match.get("raw_key")
    if not same_identity:
        raise Refused(f"invalid provenance for {identity.text()}: stable identity")
    if row.get("source_release") != ledger.get("exiftool_version"):
        raise Refused(f"invalid provenance for {identity.text()}: source release")
    if row.get("source_sha256") != match.get("source_sha256"):
        raise Refused(f"invalid provenance for {identity.text()}: source sha256")


def _validate_paths_and_symbols(
    root: Path,
    rows: Sequence[dict],
    ops_root: Path | None = None,
) -> None:
    for row in rows:
        identity = StableFieldId.from_row(row)
        fixture = row["fixture"]
        if row.get("provenance_schema") is not None or row["module"] != "Exif":
            _validate_source_binding(root, row, identity)
            _validate_reference(
                root,
                fixture,
                label=f"fixture for {identity.text()}",
                hashed=True,
                allowed_roots={"REPOSITORY", "OXIDEX_OPS_DIR"},
                ops_root=ops_root,
                native_carrier=True,
            )
            _validate_symbol(root, row["symbol"], f"symbol for {identity.text()}")
            continue
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
        _validate_symbol(root, symbol, f"symbol for {identity.text()}")
        if row["owner"] == "residual" and row["source_sha256"] != _source_hash(path):
            raise Refused(f"invalid provenance for {identity.text()}: carrier sha256")


def _fragment_digest(directory: Path) -> FragmentSet:
    paths = sorted(directory.glob("*.json"))
    digest = hashlib.sha256()
    ownership_rows: dict[str, list[dict]] = {}
    candidate_documents: dict[str, dict] = {}
    for path in paths:
        raw = path.read_bytes()
        digest.update(path.name.encode() + b"\0" + raw)
        try:
            value = json.loads(raw)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise Refused(f"malformed fragment {path}") from error
        if isinstance(value, list):
            ownership_rows[path.name] = value
            continue
        if not isinstance(value, dict):
            raise Refused(f"fragment {path} is neither a row list nor a typed object")
        if value.get("schema") != "runtime-deletion-candidates/v1":
            raise Refused(f"unsupported fragment schema in {path}")
        if set(value) != {"schema", "candidates"}:
            raise Refused(f"invalid candidate document keys in {path}")
        if not isinstance(value.get("candidates"), list):
            raise Refused(f"invalid candidates in {path}")
        candidate_documents[path.name] = value
    return FragmentSet(digest.hexdigest(), ownership_rows, candidate_documents)


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


def _stable_field_from_text(value: object) -> StableFieldId:
    if not isinstance(value, str) or "::" not in value:
        raise Refused(f"invalid candidate source field: {value!r}")
    module, remainder = value.split("::", 1)
    parts = remainder.split(":", 2)
    if len(parts) != 3:
        raise Refused(f"invalid candidate source field: {value!r}")
    table, kind, field_value = parts
    try:
        identity = StableFieldId.from_row({
            "module": module,
            "table": table,
            "field": {"kind": kind, "value": field_value},
        })
    except Refused as error:
        raise Refused(f"invalid candidate source field: {value!r}") from error
    if identity.text() != value:
        raise Refused(f"invalid candidate source field: {value!r}")
    return identity


def _validate_candidates(
    root: Path,
    documents: dict[str, dict],
    rows: Sequence[dict],
    ops_root: Path | None,
) -> None:
    candidate_keys = {
        "old_symbol", "source_fields", "new_owner", "fixture", "oracle_receipt",
        "attribution_receipt", "generated_on", "generated_off", "deletion_commit",
    }
    rows_by_identity = {StableFieldId.from_row(row): row for row in rows}
    old_symbols: set[str] = set()
    for filename in sorted(documents):
        for candidate in documents[filename]["candidates"]:
            if not isinstance(candidate, dict) or set(candidate) != candidate_keys:
                raise Refused(f"invalid candidate keys in {filename}")
            old_symbol = candidate.get("old_symbol")
            if not isinstance(old_symbol, str) or old_symbol in old_symbols:
                raise Refused(f"duplicate candidate old symbol in {filename}: {old_symbol!r}")
            _validate_symbol(root, old_symbol, "candidate old symbol")
            old_symbols.add(old_symbol)
            source_fields = candidate.get("source_fields")
            if (
                not isinstance(source_fields, list)
                or not source_fields
                or not all(isinstance(value, str) for value in source_fields)
                or len(set(source_fields)) != len(source_fields)
            ):
                raise Refused(f"candidate requires nonempty unique source_fields in {filename}")
            source_rows = []
            for value in source_fields:
                identity = _stable_field_from_text(value)
                if identity not in rows_by_identity:
                    raise Refused(f"unresolved candidate source field: {value}")
                source_rows.append(rows_by_identity[identity])
            new_owner = candidate.get("new_owner")
            if new_owner == "generated":
                owner_matches = all(row["owner"] == "generated" for row in source_rows)
            else:
                owner_matches = (
                    isinstance(new_owner, str)
                    and all(row["owner"] == "residual" and row["symbol"] == new_owner for row in source_rows)
                )
            if not owner_matches:
                raise Refused(f"candidate new_owner does not match source fields in {filename}")
            _validate_reference(
                root,
                candidate["fixture"],
                label="candidate fixture",
                hashed=True,
                allowed_roots={"REPOSITORY", "OXIDEX_OPS_DIR"},
                ops_root=ops_root,
                native_carrier=True,
            )
            for receipt_key in ("oracle_receipt", "attribution_receipt"):
                _validate_reference(
                    root,
                    candidate[receipt_key],
                    label=receipt_key.replace("_", " "),
                    hashed=False,
                    allowed_roots={"OXIDEX_OPS_DIR"},
                    ops_root=ops_root,
                )
            if candidate.get("generated_on") != "matched":
                raise Refused(f"invalid candidate generated_on in {filename}")
            if candidate.get("generated_off") != "missing-or-residual":
                raise Refused(f"invalid candidate generated_off in {filename}")
            if candidate.get("deletion_commit") is not None:
                raise Refused(f"candidate deletion_commit must be null in {filename}")


def _validated_ops_root(ops_root: Path | None) -> Path | None:
    if ops_root is None:
        return None
    if not ops_root.is_absolute():
        raise Refused("--ops-root must be absolute")
    if ops_root.is_symlink():
        raise Refused("--ops-root must not be a symlink")
    if not ops_root.exists():
        raise Refused("--ops-root is missing")
    if not ops_root.is_dir():
        raise Refused("--ops-root must be a directory")
    return ops_root.resolve()


def build_inventory(root: Path, ops_root: Path | None = None) -> dict:
    root = root.resolve()
    ops_root = _validated_ops_root(ops_root)
    fragments = _fragment_digest(root / "tools/exiftool-tables/runtime_ownership.d")
    expected_residuals = _expected_residual_rows(root)
    exif_residuals = fragments.ownership_rows.get("exif_main_residuals.json")
    if exif_residuals is None or _canonical(exif_residuals) != _canonical(expected_residuals):
        raise Refused("residual fragments differ from live Rust residual arrays")
    for filename, fragment_rows in fragments.ownership_rows.items():
        if filename == "exif_main_residuals.json":
            continue
        if any(not isinstance(row, dict) or row.get("provenance_schema") != "source-carrier/v1" for row in fragment_rows):
            raise Refused(f"ownership fragment {filename} rows require source-carrier/v1")
    fragment_rows = [
        row
        for filename in sorted(fragments.ownership_rows)
        for row in fragments.ownership_rows[filename]
    ]
    rows = _base_rows(root) + fragment_rows
    checked = verify_rows(rows)
    _validate_paths_and_symbols(root, rows, ops_root)
    _validate_candidates(root, fragments.candidate_documents, rows, ops_root)
    ledger = json.loads((root / "tools/exiftool-tables/conv_exif_main_ledger.json").read_text())
    return {"schema": SCHEMA, "source_release": ledger["exiftool_version"],
            "source_tree_sha256": ledger["table_sha256"], "fragment_inputs_sha256": fragments.digest,
            "category_totals": checked.totals, "rows": _canonical(rows)}


def load_rows(root: Path, ops_root: Path | None = None) -> list[dict]:
    inventory = json.loads((root / "tools/exiftool-tables/runtime_ownership.json").read_text())
    if inventory.get("schema") != SCHEMA or not isinstance(inventory.get("rows"), list):
        raise Refused("unsupported runtime ownership inventory")
    expected = build_inventory(root, ops_root)
    for key in ("source_release", "source_tree_sha256", "fragment_inputs_sha256", "category_totals", "rows"):
        if inventory.get(key) != expected[key]:
            raise Refused(f"runtime ownership inventory is stale or tampered: {key}")
    verify_rows(inventory["rows"])
    _validate_paths_and_symbols(root, inventory["rows"], _validated_ops_root(ops_root))
    return inventory["rows"]


def write_inventory(root: Path, ops_root: Path | None = None) -> None:
    root = root.resolve()
    fragment = root / "tools/exiftool-tables/runtime_ownership.d/exif_main_residuals.json"
    fragment.write_text(json.dumps(_expected_residual_rows(root), indent=2, sort_keys=True) + "\n")
    output = root / "tools/exiftool-tables/runtime_ownership.json"
    output.write_text(json.dumps(build_inventory(root, ops_root), indent=2, sort_keys=True) + "\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("command", choices=("verify", "write"))
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--ops-root", type=Path)
    args = parser.parse_args()
    root = args.root.resolve()
    if args.command == "write":
        write_inventory(root, args.ops_root)
    else:
        verification = verify_rows(load_rows(root, args.ops_root))
        print(f"runtime ownership verified: {verification.rows} rows; {verification.totals}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
