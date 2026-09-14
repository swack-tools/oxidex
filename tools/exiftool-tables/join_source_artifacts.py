#!/usr/bin/env python3
"""Join a recorded ExifTool source census to immutable generated artifacts.

This tool keeps the source table universe as its denominator.  It imports the
selection predicates from a Git archive of one source-selector commit, and
reads generated Rust files with ``git show`` from a separate immutable artifact
commit.  A table literal is only artifact presence: it says nothing about a
runtime route, manual implementation, output parity, or an automation share.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import inventory_source_processors as source_inventory


ARTIFACT_PATHS = {
    "binary": "src/exiftool_tables/binary_tables.rs",
    "ifd": "src/exiftool_tables/ifd_tables.rs",
    # The schema may be committed without an emitted keyed artifact.  That is
    # an observable result, never a reason to shrink the source denominator.
    "keyed": "src/exiftool_tables/keyed_tables.rs",
}
ARTIFACT_TYPES = {
    "binary": ("BinaryTable", "ALL_BINARY_TABLES", "OmittedNativeField"),
    "ifd": ("IfdTable", "ALL_IFD_TABLES", None),
    "keyed": ("KeyedDirectoryTable", "ALL_KEYED_TABLES", "OmittedKeyedNativeRow"),
}
ENABLEMENT_PATHS = {"binary": ("src/exiftool_tables/enabled.rs", "ENABLED"), "ifd": ("src/exiftool_tables/enabled_ifd.rs", "ENABLED_IFD")}
# Candidate executor locations, not proof of a registry call or file-format
# dispatch. Immutable snapshots prove only source presence; observations and
# verified caller evidence remain separate.
RUNTIME_CONSUMERS = {
    "binary": {"kind": "generic_binary_engine", "refs": ["src/exiftool_tables/engine.rs"]},
    "ifd": {"kind": "generic_ifd_engine", "refs": ["src/core/exif_dir_engine.rs"]},
    "keyed": {"kind": "generic_keyed_directory", "refs": ["src/exiftool_tables/keyed_engine.rs"]},
}

def consumer_snapshot(repo: Path, commit: str) -> dict[str, Any]:
    result = {}
    for kind, route in RUNTIME_CONSUMERS.items():
        blobs = []
        for path in route["refs"]:
            blob = git_blob_or_none(repo, commit, path)
            if blob is None:
                result[kind] = {"state": "missing", "path": path}; break
            blobs.append({"path": path, "sha256": sha(blob)})
        else:
            result[kind] = {"state": "present_static_ref", "refs": blobs}
    return result

def policy_source(text: str) -> tuple[str, str]:
    """Blank comments and separately mask literals when locating declarations."""
    import re

    code, declarations = list(text), list(text)
    at = 0
    while at < len(text):
        end = at
        comment = False
        if text.startswith("//", at):
            end = text.find("\n", at)
            end = len(text) if end < 0 else end
            comment = True
        elif text.startswith("/*", at):
            depth, end = 1, at + 2
            while end < len(text) and depth:
                if text.startswith("/*", end):
                    depth += 1
                    end += 2
                elif text.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            if depth:
                raise ValueError("unterminated block comment")
            comment = True
        elif re.match(r'r#*"', text[at:]):
            raise ValueError("raw Rust literals are unsupported in enablement policy")
        elif text[at] == '"':
            end = at + 1
            while end < len(text) and text[end] != '"':
                end += 2 if text[end] == "\\" else 1
            if end >= len(text):
                raise ValueError("unterminated string")
            end += 1
        elif (char := re.match(r"'(?:\\.|[^'\\])'", text[at:])):
            end = at + len(char.group())
        if end > at:
            for index in range(at, end):
                if text[index] != "\n":
                    declarations[index] = " "
                    if comment:
                        code[index] = " "
            at = end
        else:
            at += 1
    return "".join(code), "".join(declarations)


def parse_enabled(text: str, symbol: str) -> set[tuple[str, str]]:
    import re

    code, declarations = policy_source(text)
    pattern = (rf"\bpub\s+static\s+{re.escape(symbol)}\s*:\s*"
               r"&\s*\[\s*\(\s*&str\s*,\s*&str\s*\)\s*\]\s*=\s*&\s*\[")
    markers = list(re.finditer(pattern, declarations))
    if len(markers) != 1:
        raise ValueError(f"expected exactly one {symbol} declaration")
    start, end = brace_span(code, markers[0].end() - 1, "[", "]")
    if not code[end:].lstrip().startswith(";"):
        raise ValueError(f"unterminated {symbol} declaration")
    body = code[start + 1:end - 1]
    entry = re.compile(r'\s*\(\s*"([^"\\\r\n]+)"\s*,\s*"([^"\\\r\n]+)"\s*\)\s*')
    result, at = set(), 0
    while body[at:].strip():
        match = entry.match(body, at)
        if match is None:
            raise ValueError(f"unrecognised {symbol} entry syntax")
        identity = match.group(1), match.group(2)
        if identity in result:
            raise ValueError(f"duplicate {symbol} entry")
        result.add(identity)
        at = match.end()
        if not body[at:].strip():
            break
        if body[at] != ",":
            raise ValueError(f"unrecognised {symbol} entry separator")
        at += 1
    return result


def runtime_evidence(selection: str, artifact: dict[str, Any], enabled: set[tuple[str, str]] | None = None, identity: tuple[str, str] | None = None, consumer_verified: bool = False) -> dict[str, Any]:
    """Classify static route evidence without inventing dynamic reachability."""
    if selection not in RUNTIME_CONSUMERS:
        return {"state": "unknown", "reason": "no_protocol_consumer_manifest", "refs": [], "observed": {"read": None, "write": None}}
    if artifact.get("definition") != "present":
        return {"state": "unknown", "reason": "absent_from_this_generated_registry", "refs": [], "observed": {"read": None, "write": None}}
    if not artifact.get("registry_listed"):
        return {"state": "not_enabled", "reason": "definition_not_listed_in_generated_registry", "refs": [], "observed": {"read": None, "write": None}}
    if artifact.get("gate_a_blocked_by"):
        return {"state": "not_enabled", "reason": "gate_a_blocked", "refs": [], "observed": {"read": None, "write": None}}
    if enabled is None:
        return {"state": "unknown", "reason": "enablement_policy_missing", "refs": [], "observed": {"read": None, "write": None}}
    if identity not in enabled:
        return {"state": "not_enabled", "reason": "not_in_commit_pinned_enablement_allowlist", "refs": [], "observed": {"read": None, "write": None}}
    if not consumer_verified:
        return {"state": "unverified", "reason": "executor_dispatch_not_proven", "refs": [], "observed": {"read": None, "write": None}}
    route = RUNTIME_CONSUMERS[selection]
    return {"state": "known_generic_route", "reason": "static_protocol_registry_route", "kind": route["kind"], "refs": route["refs"], "observed": {"read": None, "write": None}}


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def git(repo: Path, *args: str) -> str:
    return source_inventory.git(repo, *args)


def git_blob_or_none(repo: Path, commit: str, path: str) -> bytes | None:
    """Read one commit-pinned blob; only an absent tree path is optional.

    ``cat-file -e`` returns the same non-zero status for an absent path and a
    missing/corrupt object.  Inspect the already-validated commit's tree first
    so an optional keyed artifact cannot hide a repository failure.
    """
    listing = subprocess.run(
        ["git", "-C", str(repo), "ls-tree", "-z", commit, "--", path],
        capture_output=True,
        check=False,
    )
    if listing.returncode:
        detail = listing.stderr.decode("utf-8", "replace").strip()
        raise RuntimeError(f"cannot inspect artifact path {path!r} at {commit}: {detail}")
    entries = [entry for entry in listing.stdout.split(b"\0") if entry]
    if not entries:
        return None
    if len(entries) != 1:
        raise RuntimeError(f"artifact path {path!r} at {commit} resolves to {len(entries)} tree entries")
    try:
        metadata, listed_path = entries[0].split(b"\t", 1)
        mode, object_type, object_id = metadata.decode("ascii").split(" ", 2)
    except (UnicodeDecodeError, ValueError) as exc:
        raise RuntimeError(f"malformed Git tree entry for artifact path {path!r}") from exc
    if listed_path.decode("utf-8", "replace") != path or mode != "100644" or object_type != "blob":
        raise RuntimeError(f"artifact path {path!r} is not one regular blob at {commit}")
    blob = subprocess.run(
        ["git", "-C", str(repo), "cat-file", "blob", object_id],
        capture_output=True,
        check=False,
    )
    if blob.returncode:
        detail = blob.stderr.decode("utf-8", "replace").strip()
        raise RuntimeError(f"cannot read artifact blob {object_id} for {path!r}: {detail}")
    return blob.stdout


def brace_span(text: str, start: int, opening: str, closing: str) -> tuple[int, int]:
    """Return an inclusive-open, exclusive-close balanced span, respecting strings."""
    if start >= len(text) or text[start] != opening:
        raise ValueError(f"expected {opening!r} at {start}")
    depth = 0
    quote = False
    escaped = False
    for pos in range(start, len(text)):
        char = text[pos]
        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                quote = False
            continue
        if char == '"':
            quote = True
        elif char == opening:
            depth += 1
        elif char == closing:
            depth -= 1
            if depth == 0:
                return start, pos + 1
            if depth < 0:
                break
    raise ValueError(f"unbalanced {opening}{closing} span at {start}")


def rust_string(value: str) -> str:
    # Generated module/table identities have no escape-sensitive spelling in
    # the recorded dump today.  Keeping this deliberately narrow means a new
    # generated representation fails closed rather than silently rekeys rows.
    if "\\" in value:
        raise ValueError("escaped Rust table identity is unsupported by this inventory")
    return value


def member_string(body: str, name: str) -> str:
    import re

    match = re.search(rf"\b{name}:\s*\"([^\"]*)\"\s*,", body)
    if match is None:
        raise ValueError(f"table lacks literal {name}")
    return rust_string(match.group(1))


def gate_a(body: str) -> list[list[Any]]:
    import re

    marker = re.search(r"\bgate_a:\s*GateA\s*\{\s*blocked_by:\s*&\[", body)
    if marker is None:
        raise ValueError("table lacks GateA.blocked_by")
    start, end = brace_span(body, marker.end() - 1, "[", "]")
    values = body[start + 1:end - 1]
    result: list[list[Any]] = []
    token = re.compile(r'\s*\(\s*"([^"\\]+)"\s*,\s*(\d+)\s*\)\s*,?')
    pos = 0
    while pos < len(values):
        match = token.match(values, pos)
        if match is None:
            if values[pos:].strip() == "":
                break
            raise ValueError(f"unrecognised GateA blocker: {values[pos:].strip()!r}")
        result.append([match.group(1), int(match.group(2))])
        pos = match.end()
    return result


def array_field_count(body: str) -> int:
    # This count is diagnostic only.  It identifies definitions with no
    # emitted rows while never deciding whether the native source is complete.
    return body.count("Field {") + body.count("IfdTag {") + body.count("KeyedTag {")


def parse_tables(text: str, kind: str) -> tuple[dict[tuple[str, str], dict[str, Any]], dict[str, Any]]:
    """Parse generated table headers and their closed registry without codegen."""
    import re

    table_type, registry, _sidecar = ARTIFACT_TYPES[kind]
    header = re.compile(rf"pub\s+static\s+(\w+)\s*:\s*{table_type}\s*=\s*{table_type}\s*\{{")
    declared = list(header.finditer(text))
    if not declared:
        raise ValueError(f"{kind} artifact declares no {table_type} statics")
    tables: dict[tuple[str, str], dict[str, Any]] = {}
    identifiers: dict[str, tuple[str, str]] = {}
    for match in declared:
        start, end = brace_span(text, match.end() - 1, "{", "}")
        body = text[start + 1:end - 1]
        key = (member_string(body, "module"), member_string(body, "table"))
        ident = match.group(1)
        if key in tables or ident in identifiers:
            raise ValueError(f"duplicate {kind} table {key!r}/{ident}")
        tables[key] = {
            "ident": ident,
            "gate_a_blocked_by": gate_a(body),
            "emitted_row_literals": array_field_count(body),
        }
        identifiers[ident] = key

    registry_marker = re.search(
        rf"pub\s+static\s+{registry}\s*:\s*&\[&{table_type}\]\s*=\s*&\[", text
    )
    if registry_marker is None:
        raise ValueError(f"{kind} artifact lacks {registry}")
    start, end = brace_span(text, registry_marker.end() - 1, "[", "]")
    registry_body = text[start + 1:end - 1]
    listed = []
    pos = 0
    entry = re.compile(r"\s*&(?P<ident>\w+)(?:\s*,|(?=\s*$))")
    while pos < len(registry_body):
        match = entry.match(registry_body, pos)
        if match is None:
            if registry_body[pos:].strip() == "":
                break
            raise ValueError(f"{kind} registry has unrecognised syntax: {registry_body[pos:].strip()!r}")
        listed.append(match.group("ident"))
        pos = match.end()
    if len(listed) != len(set(listed)):
        raise ValueError(f"{kind} registry lists a table more than once")
    unknown = sorted(set(listed) - set(identifiers))
    unlisted = sorted(set(identifiers) - set(listed))
    if unknown or unlisted:
        raise ValueError(f"{kind} registry mismatch: unknown={unknown}, unlisted={unlisted}")
    for ident in listed:
        tables[identifiers[ident]]["registry_listed"] = True
    return tables, {"table_definitions": len(tables), "registry_entries": len(listed)}


def parse_sidecar_tables(text: str, kind: str) -> Counter[tuple[str, str]]:
    """Count source-row omissions by source table, rejecting partial reads."""
    import re

    _type, _registry, struct = ARTIFACT_TYPES[kind]
    if struct is None:
        return Counter()
    marker = re.search(rf"pub\s+static\s+OMITTED(?:_KEYED)?_NATIVE_(?:FIELDS|ROWS)\s*:\s*&\[\s*{struct}\s*\]\s*=\s*&\[", text)
    if marker is None:
        # Keyed artifacts use a sidecar only once one exists.  Binary must
        # always carry it; an absent one would hide fully refused source rows.
        if kind == "binary":
            raise ValueError("binary artifact lacks OMITTED_NATIVE_FIELDS")
        return Counter()
    start, end = brace_span(text, marker.end() - 1, "[", "]")
    body = text[start + 1:end - 1]
    expected = len(re.findall(rf"{struct}\s*\{{", body))
    pattern = re.compile(
        rf'{struct}\s*\{{\s*module:\s*"([^"\\]+)"\s*,\s*table:\s*"([^"\\]+)"\s*,',
        re.S,
    )
    found = pattern.findall(body)
    if len(found) != expected:
        raise ValueError(f"parsed {len(found)} {struct} entries but sidecar declares {expected}")
    return Counter((rust_string(module), rust_string(table)) for module, table in found)


def artifact_snapshot(repo: Path, commit: str) -> tuple[dict[str, Any], dict[str, dict[tuple[str, str], dict[str, Any]]], dict[str, Counter[tuple[str, str]]]]:
    snapshot: dict[str, Any] = {"commit": commit, "artifacts": {}}
    parsed: dict[str, dict[tuple[str, str], dict[str, Any]]] = {}
    sidecars: dict[str, Counter[tuple[str, str]]] = {}
    for kind, path in ARTIFACT_PATHS.items():
        blob = git_blob_or_none(repo, commit, path)
        if blob is None:
            if kind in {"binary", "ifd"}:
                raise ValueError(f"required {kind} artifact is absent at {commit}: {path}")
            snapshot["artifacts"][kind] = {"path": path, "state": "absent_at_commit"}
            parsed[kind] = {}
            sidecars[kind] = Counter()
            continue
        text = blob.decode("utf-8")
        tables, accounting = parse_tables(text, kind)
        parsed[kind] = tables
        sidecars[kind] = parse_sidecar_tables(text, kind)
        snapshot["artifacts"][kind] = {
            "path": path,
            "state": "present",
            "sha256": sha(blob),
            **accounting,
            "sidecar_omitted_rows": sum(sidecars[kind].values()),
        }
    snapshot["enablement"] = {}
    for kind, (path, symbol) in ENABLEMENT_PATHS.items():
        blob = git_blob_or_none(repo, commit, path)
        snapshot["enablement"][kind] = ({"state": "missing", "path": path} if blob is None else {
            "state": "present", "path": path, "sha256": sha(blob),
            "tables": sorted(parse_enabled(blob.decode("utf-8"), symbol)),
        })
    snapshot["consumers"] = consumer_snapshot(repo, commit)
    return snapshot, parsed, sidecars


def source_rows(out: Path, repo: Path, selector_commit: str, entries: list[tuple[str, str, dict[str, Any], dict[str, Any], dict[str, Any]]]) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    snapshot = source_inventory.archive_tools(repo, selector_commit, out)
    selected = source_inventory.run_selectors(out / snapshot["tools_directory"], [meta for _, _, _, meta, _ in entries])
    source_inventory.require(len(entries) == len(selected), "selector result length does not match table records")
    rows: list[dict[str, Any]] = []
    for (module, table, data, meta, tags), result in zip(entries, selected):
        selection = (
            "binary" if result.get("binary") else
            "keyed" if result.get("keyed_profile") or result.get("keyed_word_candidate") else
            "ifd" if result.get("ifd") else
            "other_unclassified"
        )
        processor_identity, processor_shape = source_inventory.processor(meta)
        rows.append({
            "module": module,
            "table": table,
            "source_identity": data["full_name"],
            "processor_identity": processor_identity,
            "processor_shape": processor_shape,
            "selection": selection,
            "selection_evidence": result,
            "declared_tag_count": data["tag_count"],
            "counts": dict(sorted(source_inventory.count_rows(tags).items())),
        })
    source_inventory.require(len({(row["module"], row["table"]) for row in rows}) == len(rows), "source table identities are not unique")
    return rows, snapshot


def join_rows(rows: list[dict[str, Any]], parsed: dict[str, dict[tuple[str, str], dict[str, Any]]], sidecars: dict[str, Counter[tuple[str, str]]], artifact_states: dict[str, str] | None = None, enablement: dict[str, Any] | None = None, consumers: dict[str, Any] | None = None) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    source_keys = {(row["module"], row["table"]) for row in rows}
    artifact_states = artifact_states or {kind: "present" for kind in ARTIFACT_PATHS}
    for kind, tables in parsed.items():
        orphan = sorted(set(tables) - source_keys)
        if orphan:
            raise ValueError(f"{kind} artifact has generated table identities absent from source: {orphan[:5]}")
        selection_by_identity = {(row["module"], row["table"]): row["selection"] for row in rows}
        wrong_family = sorted(key for key in tables if selection_by_identity[key] != kind)
        if wrong_family:
            raise ValueError(f"{kind} artifact table identities disagree with source selection: {wrong_family[:5]}")
        sidecar_orphan = sorted(set(sidecars[kind]) - source_keys)
        if sidecar_orphan:
            raise ValueError(f"{kind} sidecar refers to source-absent identities: {sidecar_orphan[:5]}")

    joined: list[dict[str, Any]] = []
    totals: dict[str, Counter[str]] = defaultdict(Counter)
    for row in rows:
        key = (row["module"], row["table"])
        present: dict[str, Any] = {}
        for kind in ARTIFACT_PATHS:
            table = parsed[kind].get(key)
            omitted_rows = sidecars[kind][key]
            if table is None:
                present[kind] = {
                    "artifact_state": artifact_states[kind],
                    "definition": "absent",
                    "sidecar_omitted_rows": omitted_rows,
                    "gate_a": "unavailable",
                }
                # The full joined rows retain absence for every source family.
                # Totals below remain family-scoped, so they cannot make an
                # unrelated IFD table look like a missing binary candidate.
                if row["selection"] == kind:
                    totals[kind]["source_selected_tables"] += 1
                    totals[kind]["definitions_absent"] += 1
                    if omitted_rows:
                        totals[kind]["sidecar_only_tables"] += 1
                        totals[kind]["sidecar_omitted_rows"] += omitted_rows
                    else:
                        totals[kind]["definitions_absent_without_sidecar"] += 1
            else:
                present[kind] = {
                    "artifact_state": artifact_states[kind],
                    "definition": "present",
                    "registry_listed": table["registry_listed"],
                    "emitted_row_literals": table["emitted_row_literals"],
                    "sidecar_omitted_rows": omitted_rows,
                    "gate_a_blocked_by": table["gate_a_blocked_by"],
                }
                if row["selection"] == kind:
                    totals[kind]["source_selected_tables"] += 1
                    totals[kind]["definitions_present"] += 1
                    totals[kind]["emitted_row_literals"] += table["emitted_row_literals"]
                    totals[kind]["sidecar_omitted_rows"] += omitted_rows
                    if table["emitted_row_literals"] == 0:
                        totals[kind]["zero_emitted_definitions"] += 1
                    if table["gate_a_blocked_by"]:
                        totals[kind]["gate_a_refused_tables"] += 1
        selected_artifact = present.get(row["selection"], {})
        enabled = None if not enablement or enablement.get(row["selection"], {}).get("state") != "present" else set(map(tuple, enablement[row["selection"]]["tables"]))
        joined.append({**row, "artifacts": present,
                       "candidate_executor_source": (consumers or {}).get(row["selection"], {"state": "not_in_manifest"}),
                       "runtime_consumer": runtime_evidence(row["selection"], selected_artifact, enabled, (row["module"], row["table"]), False)})
    source_inventory.require(len(joined) == len(rows), "join does not conserve source rows")
    return joined, {kind: dict(sorted(counts.items())) for kind, counts in sorted(totals.items())}


def gate_a_blocker_counts(joined: list[dict[str, Any]]) -> dict[str, Counter[str]]:
    """Aggregate only categories that have a generated artifact family."""
    blockers: dict[str, Counter[str]] = defaultdict(Counter)
    for row in joined:
        kind = row["selection"]
        if kind not in ARTIFACT_PATHS:
            continue
        for reason, count in row["artifacts"][kind].get("gate_a_blocked_by", []):
            blockers[kind][reason] += count
    return blockers


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--dump", type=Path, required=True)
    parser.add_argument("--repo", type=Path, required=True, help="repository containing both commits")
    parser.add_argument("--selector-ref", required=True)
    parser.add_argument("--expected-dump-sha", required=True)
    parser.add_argument("--expected-selector-commit", required=True)
    parser.add_argument("--artifact-ref", required=True)
    parser.add_argument("--expected-artifact-commit", required=True)
    parser.add_argument("--out", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if not args.dump.is_file():
        raise SystemExit(f"dump not found: {args.dump}")
    dump_sha = source_inventory.sha_file(args.dump)
    if dump_sha != args.expected_dump_sha:
        raise SystemExit(f"dump SHA mismatch: {dump_sha} != {args.expected_dump_sha}")
    selector_commit = git(args.repo, "rev-parse", f"{args.selector_ref}^{{commit}}")
    if selector_commit != args.expected_selector_commit:
        raise SystemExit(f"selector commit mismatch: {selector_commit} != {args.expected_selector_commit}")
    artifact_commit = git(args.repo, "rev-parse", f"{args.artifact_ref}^{{commit}}")
    if artifact_commit != args.expected_artifact_commit:
        raise SystemExit(f"artifact commit mismatch: {artifact_commit} != {args.expected_artifact_commit}")
    if args.out.exists():
        raise SystemExit(f"output path already exists: {args.out}")
    try:
        document = json.loads(args.dump.read_text())
        entries = source_inventory.validate_document(document)
        artifacts, parsed, sidecars = artifact_snapshot(args.repo, artifact_commit)
    except (ValueError, UnicodeDecodeError) as exc:
        raise SystemExit(f"invalid source/artifact input: {exc}") from exc

    runner_sha = source_inventory.sha_file(Path(__file__).resolve())
    print("=== instrument: join_source_artifacts ===", file=sys.stderr)
    print(f"runner_sha256: {runner_sha}", file=sys.stderr)
    print(f"immutable_selector_commit: {selector_commit}", file=sys.stderr)
    print(f"immutable_artifact_commit: {artifact_commit}", file=sys.stderr)
    print(f"recorded_dump: ExifTool {document.get('exiftool_version')} sha256={dump_sha}", file=sys.stderr)
    print("scope: recorded-input/artifact-shape only; no oxidex, runtime, or live Perl oracle run", file=sys.stderr)

    args.out.mkdir(parents=True)
    rows, selector_snapshot = source_rows(args.out, args.repo, selector_commit, entries)
    artifact_states = {kind: value["state"] for kind, value in artifacts["artifacts"].items()}
    try:
        joined, artifact_totals = join_rows(rows, parsed, sidecars, artifact_states, artifacts.get("enablement"), artifacts.get("consumers"))
    except ValueError as exc:
        raise SystemExit(f"invalid source/artifact accounting: {exc}") from exc
    selection_counts = Counter(row["selection"] for row in rows)
    runtime_counts = Counter(row["runtime_consumer"]["state"] for row in joined)
    families = defaultdict(Counter)
    for row in joined:
        key = f"{row.get('processor_identity') or 'default'}|{row.get('processor_shape')}"
        families[key]["tables"] += 1
        families[key]["declared_rows"] += row.get("declared_tag_count", 0)
        families[key][row["runtime_consumer"]["state"]] += 1
    gate_a_blockers = gate_a_blocker_counts(joined)
    summary = {
        "runner_sha256": runner_sha,
        "source_inventory_helper_sha256": source_inventory.sha_file(Path(source_inventory.__file__).resolve()),
        "scope": "one recorded dump plus immutable generated artifact shapes; not generated acceptance, runtime reachability, manual maintenance, output parity, or an automation percentage",
        "dump": {"sha256": dump_sha, "exiftool_version": document.get("exiftool_version"), "table_records": len(rows)},
        "selector_snapshot": selector_snapshot,
        "artifact_snapshot": artifacts,
        "conservation": {
            "source_table_records": len(rows),
            "joined_table_records": len(joined),
            "selection_table_counts": dict(sorted(selection_counts.items())),
            "runtime_consumer_table_counts": dict(sorted(runtime_counts.items())),
            "processor_families": {key: dict(sorted(value.items())) for key, value in sorted(families.items())},
            "artifact_accounting": artifact_totals,
            "gate_a_blocker_counts": {kind: dict(sorted(counts.items())) for kind, counts in sorted(gate_a_blockers.items())},
        },
        "gaps": [
            "Artifact presence is not runtime reachability, manual maintenance, output parity, or an automation percentage.",
            "A keyed schema without a committed keyed generated artifact is recorded as absent_at_commit.",
            "Gate A is recorded only where a generated table literal exposes GateA; unavailable is not a refusal reason.",
            "The recorded dump is not a whole-source module-discovery or dispatch inventory.",
        ],
    }
    (args.out / "joined-tables.json").write_text(json.dumps(joined, indent=2, sort_keys=True) + "\n")
    (args.out / "summary.json").write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"source_tables": len(rows), "joined_tables": len(joined), "selection": dict(sorted(selection_counts.items()))}, sort_keys=True))


if __name__ == "__main__":
    main()
