#!/usr/bin/env python3
"""Reconcile a small hydrated-table catalog against one large tables dump.

Only the existing dump's ``modules`` projection is read.  The reader is a
streaming structural parser: it validates the root/module/table schema and
never loads the multi-gigabyte source dump into memory.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any


CATALOG_SCHEMA = "oxidex_hydrated_catalog_universe_v1"
REPORT_SCHEMA = "oxidex_hydrated_catalog_reconciliation_v1"


class Refused(ValueError):
    pass


def pinned_version(repo_root: Path) -> str:
    value = (repo_root / ".exiftool-version").read_text(encoding="utf-8").strip()
    if not value or any(char not in "0123456789." for char in value) or value.count(".") != 1:
        raise Refused("repository ExifTool pin is malformed")
    return value


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


class _Chars:
    def __init__(self, path: Path):
        self._file = path.open("r", encoding="utf-8")
        self._buffer = ""
        self._at = 0

    def close(self) -> None:
        self._file.close()

    def peek(self) -> str:
        if self._at == len(self._buffer):
            self._buffer = self._file.read(65536)
            self._at = 0
        return self._buffer[self._at:self._at + 1]

    def take(self) -> str:
        char = self.peek()
        if char:
            self._at += 1
        return char

    def ws(self) -> None:
        while self.peek() and self.peek() in " \t\r\n":
            self.take()

    def expect(self, expected: str) -> None:
        self.ws()
        actual = self.take()
        if actual != expected:
            raise Refused(f"expected {expected!r}, got {actual!r}")

    def string(self) -> str:
        self.ws()
        if self.take() != '"':
            raise Refused("expected JSON string")
        raw = ['"']
        escaped = False
        while True:
            char = self.take()
            if not char:
                raise Refused("unterminated JSON string")
            raw.append(char)
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                try:
                    return json.loads("".join(raw))
                except json.JSONDecodeError as exc:
                    raise Refused("invalid JSON string") from exc

    def scalar(self) -> Any:
        self.ws()
        if self.peek() == '"':
            return self.string()
        token: list[str] = []
        while self.peek() and self.peek() not in ",]} \t\r\n":
            token.append(self.take())
        try:
            return json.loads("".join(token))
        except json.JSONDecodeError as exc:
            raise Refused("invalid JSON scalar") from exc

    def skip_string(self) -> None:
        self.ws()
        if self.take() != '"':
            raise Refused("expected JSON string")
        while True:
            char = self.take()
            if not char:
                raise Refused("unterminated JSON string")
            if ord(char) < 0x20:
                raise Refused("unescaped control character in skipped JSON string")
            if char == '"':
                return
            if char != "\\":
                continue
            escaped = self.take()
            if escaped not in {'"', "\\", "/", "b", "f", "n", "r", "t", "u"}:
                raise Refused("invalid escape in skipped JSON string")
            if escaped == "u":
                hex_digits = "".join(self.take() for _ in range(4))
                if len(hex_digits) != 4 or any(digit not in "0123456789abcdefABCDEF" for digit in hex_digits):
                    raise Refused("invalid unicode escape in skipped JSON string")

    def skip_value(self) -> None:
        self.ws()
        char = self.peek()
        if char == '"':
            self.skip_string(); return
        if char == '{':
            self.take(); self.ws()
            if self.peek() == '}': self.take(); return
            while True:
                self.string(); self.expect(':'); self.skip_value(); self.ws()
                separator = self.take()
                if separator == '}': return
                if separator != ',': raise Refused("malformed JSON object")
        elif char == '[':
            self.take(); self.ws()
            if self.peek() == ']': self.take(); return
            while True:
                self.skip_value(); self.ws()
                separator = self.take()
                if separator == ']': return
                if separator != ',': raise Refused("malformed JSON array")
        else:
            self.scalar()


def _object_start(stream: _Chars) -> bool:
    stream.ws()
    if stream.take() != '{':
        raise Refused("expected JSON object")
    stream.ws()
    if stream.peek() == '}':
        stream.take()
        return False
    return True


def _next_pair(stream: _Chars) -> tuple[str, bool]:
    key = stream.string()
    stream.expect(':')
    return key, True


def _finish_pair(stream: _Chars) -> bool:
    stream.ws()
    separator = stream.take()
    if separator == '}': return False
    if separator != ',': raise Refused("malformed JSON object")
    return True


def _object_count(stream: _Chars) -> int:
    count = 0
    more = _object_start(stream)
    while more:
        _next_pair(stream); stream.skip_value(); count += 1
        more = _finish_pair(stream)
    return count


def _table(stream: _Chars, module: str, table: str) -> str:
    full_name = None; meta = tags = None; tag_count = None; seen: set[str] = set()
    more = _object_start(stream)
    while more:
        key, _ = _next_pair(stream)
        if key in seen: raise Refused(f"{module}::{table}: duplicate table field {key}")
        seen.add(key)
        stream.ws()
        if key == "full_name": full_name = stream.scalar()
        elif key == "meta": meta = _object_count(stream)
        elif key == "tags": tags = _object_count(stream)
        elif key == "tag_count": tag_count = stream.scalar()
        else: stream.skip_value()
        more = _finish_pair(stream)
    if not isinstance(full_name, str) or not full_name:
        raise Refused(f"{module}::{table}: full_name is required")
    if full_name != f"Image::ExifTool::{module}::{table}":
        raise Refused(f"{module}::{table}: full_name does not match old dump identity schema")
    if meta is None or tags is None or not isinstance(tag_count, int) or tag_count < 0:
        raise Refused(f"{full_name}: missing table shape fields")
    if tags != tag_count:
        raise Refused(f"{full_name}: tag_count does not conserve tag keys")
    return full_name


def _module(stream: _Chars, module: str) -> set[str]:
    table_count = None; module_field = None; tables: set[str] | None = None; seen: set[str] = set()
    more = _object_start(stream)
    while more:
        key, _ = _next_pair(stream)
        if key in seen: raise Refused(f"{module}: duplicate module field {key}")
        seen.add(key)
        if key == "module": module_field = stream.scalar()
        elif key == "table_count": table_count = stream.scalar()
        elif key == "tables":
            tables = set(); table_keys: set[str] = set(); inner = _object_start(stream)
            while inner:
                table, _ = _next_pair(stream)
                if not table: raise Refused(f"{module}: empty table key")
                if table in table_keys: raise Refused(f"{module}: duplicate table key {table}")
                table_keys.add(table)
                identity = _table(stream, module, table)
                if identity in tables: raise Refused(f"{module}: duplicate table identity")
                tables.add(identity); inner = _finish_pair(stream)
        else: stream.skip_value()
        more = _finish_pair(stream)
    if module_field != module or tables is None or not isinstance(table_count, int) or table_count != len(tables):
        raise Refused(f"{module}: invalid module table schema")
    return tables


def stream_dump_identities(path: Path, expected_version: str) -> tuple[set[str], str]:
    stream = _Chars(path)
    try:
        identities: set[str] | None = None; version = ok = failed = None
        more = _object_start(stream); root_seen: set[str] = set(); module_keys: set[str] = set()
        while more:
            key, _ = _next_pair(stream)
            if key in root_seen: raise Refused(f"duplicate dump root field {key}")
            root_seen.add(key)
            if key == "exiftool_version": version = stream.scalar()
            elif key == "modules_ok": ok = stream.scalar()
            elif key == "modules_failed": failed = stream.scalar()
            elif key == "modules":
                identities = set(); inner = _object_start(stream)
                while inner:
                    module, _ = _next_pair(stream)
                    if module in module_keys: raise Refused(f"duplicate dump module key {module}")
                    module_keys.add(module)
                    identities.update(_module(stream, module)); inner = _finish_pair(stream)
            else: stream.skip_value()
            more = _finish_pair(stream)
            # The dump's modules projection is complete at this point.  Do not
            # scan native_write_tables or code-fact bodies merely to learn its
            # identity set; those are deliberately outside this sidecar's scope.
            if identities is not None and isinstance(version, str) and isinstance(ok, int) and failed == 0:
                if ok <= 0:
                    raise Refused("dump modules_ok must be positive")
                if ok != len(module_keys):
                    raise Refused("dump modules_ok does not match actual module count")
                if version != expected_version:
                    raise Refused("dump ExifTool version does not match repository pin")
                return identities, version
        if not isinstance(version, str) or not isinstance(ok, int) or failed != 0 or identities is None:
            raise Refused("dump root lacks a complete successful modules projection")
        if ok <= 0:
            raise Refused("dump modules_ok must be positive")
        if ok != len(module_keys) or version != expected_version:
            raise Refused("dump root does not match pinned module universe")
        return identities, version
    finally:
        stream.close()


def _catalog(document: Any, expected_version: str) -> tuple[set[str], set[str], dict[str, int]]:
    if not isinstance(document, dict) or document.get("schema") != CATALOG_SCHEMA:
        raise Refused("catalog schema is unsupported")
    if document.get("exiftool_version") != expected_version:
        raise Refused("catalog ExifTool version does not match repository pin")
    families = document.get("families")
    if not isinstance(families, dict): raise Refused("catalog families are required")
    tables = families.get("hydrated_tables"); shortcuts = families.get("shortcuts")
    if not isinstance(tables, list) or not isinstance(shortcuts, list): raise Refused("catalog family lists are required")
    def names(rows: list[Any], kind: str) -> set[str]:
        result: set[str] = set()
        for row in rows:
            if not isinstance(row, dict) or not isinstance(row.get("full_name"), str) or not row["full_name"]:
                raise Refused(f"catalog {kind} identity is malformed")
            if row["full_name"] in result: raise Refused(f"catalog {kind} identity is duplicated")
            result.add(row["full_name"])
        return result
    table_names, helper_names = names(tables, "table"), names(shortcuts, "helper")
    if table_names & helper_names: raise Refused("catalog table/helper identities overlap")
    counts = document.get("counts")
    if not isinstance(counts, dict) or counts.get("hydrated_tables") != len(table_names):
        raise Refused("catalog hydrated table count does not conserve")
    return table_names, helper_names, {"tables": len(table_names), "helpers": len(helper_names)}


def reconcile(catalog: Any, dump_path: Path, expected_version: str, *, catalog_sha256: str | None = None) -> dict[str, Any]:
    tables, helpers, catalog_counts = _catalog(catalog, expected_version)
    dumped, dump_version = stream_dump_identities(dump_path, expected_version)
    matched_tables = tables & dumped; matched_helpers = helpers & dumped
    missing_tables = tables - dumped; missing_helpers = helpers - dumped
    extra = dumped - tables - helpers
    return {
        "schema": REPORT_SCHEMA,
        "scope": "hydrated catalog identities versus structurally validated dump modules projection; no layout migration or runtime support claim",
        "inputs": {"expected_exiftool_version": expected_version,
                   "catalog": {"sha256": catalog_sha256, "exiftool_version": catalog["exiftool_version"]},
                   "dump": {"sha256": sha256_file(dump_path), "exiftool_version": dump_version}},
        "catalog": catalog_counts,
        "dump": {"table_identities": len(dumped)},
        "reconciliation": {
            "matched_tables": len(matched_tables), "missing_tables": sorted(missing_tables),
            "matched_helpers": len(matched_helpers), "missing_helpers": sorted(missing_helpers),
            "extra_dump_identities": sorted(extra),
            "conservation": {"catalog_tables": len(matched_tables) + len(missing_tables),
                             "catalog_helpers": len(matched_helpers) + len(missing_helpers),
                             "dump_identities": len(matched_tables) + len(matched_helpers) + len(extra)},
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--dump", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[2])
    args = parser.parse_args()
    if args.output.exists() or args.output.is_symlink(): raise Refused("output already exists")
    repo_root = args.repo_root.resolve()
    status = subprocess.run(["git", "-C", str(repo_root), "status", "--porcelain"], check=True, text=True, capture_output=True).stdout
    if status and os.environ.get("OXIDEX_ALLOW_DIRTY_TREE") != "1":
        raise Refused("refusing to measure against a dirty working tree; set OXIDEX_ALLOW_DIRTY_TREE=1 to override")
    expected = pinned_version(repo_root)
    raw_catalog = args.catalog.read_bytes()
    print("=== instrument: hydrated_catalog_reconcile.py ===", file=sys.stderr)
    print(f"repo: {repo_root}", file=sys.stderr)
    print(f"commit: {subprocess.run(['git', '-C', str(repo_root), 'rev-parse', 'HEAD'], check=True, text=True, capture_output=True).stdout.strip()}", file=sys.stderr)
    print(f"tree: {'dirty [OXIDEX_ALLOW_DIRTY_TREE=1: measuring anyway]' if status else 'clean'}", file=sys.stderr)
    print(f"expected_exiftool_version: {expected}", file=sys.stderr)
    report = reconcile(json.loads(raw_catalog), args.dump, expected, catalog_sha256=hashlib.sha256(raw_catalog).hexdigest())
    args.output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    try: raise SystemExit(main())
    except (OSError, json.JSONDecodeError, Refused) as exc:
        raise SystemExit(f"hydrated catalog reconciliation refused: {exc}")
