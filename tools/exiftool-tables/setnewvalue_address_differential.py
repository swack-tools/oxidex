"""Export source-validated static addressing operands and resolver cases.

Every expected result is evaluated by ``setnewvalue_addressing.resolve`` from
the supplied manifest-authenticated dump and native observation, never copied
from a hand-written tag list.
"""
from __future__ import annotations

from dataclasses import asdict
import hashlib
import json
from pathlib import Path
from typing import Any, Mapping

from setnewvalue_addressing import compile_addressing, resolve


def _sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _case(addressing, observations: Mapping[str, Any], spelling: str, kind: str) -> dict[str, Any]:
    outcome = resolve(addressing, observations, spelling)
    value: dict[str, Any] = {"kind": kind, "spelling": spelling, "state": outcome.state}
    if outcome.row is not None:
        value["identity"] = list(outcome.row.identity)
    if outcome.reason is not None:
        value["reason"] = outcome.reason
    return value


def export(document: Mapping[str, Any], observations: Mapping[str, Any], *,
           dump_sha256: str, observations_sha256: str, operands_sha256: str) -> dict[str, Any]:
    addressing, report = compile_addressing(document)
    cases: dict[str, tuple[str, str]] = {}
    for row in sorted(addressing.rows, key=lambda row: row.identity):
        for kind, spelling in (
                ("ordinary", row.name), ("case_alias", row.name.lower()),
                ("family0", f"EXIF:{row.name}"), ("family1", f"IFD0:{row.name}")):
            cases.setdefault(spelling, (kind, spelling))
    # Native candidates outside EXIF/IFD0 prove the terminal outside-scope
    # branch for explicit qualified requests.  Group spellings come entirely
    # from observed GetGroup data.
    for query, observed in sorted(observations["queries"].items()):
        for candidate in observed["candidates"]:
            for group in candidate.get("groups", {}).values():
                if not isinstance(group, str) or not group or group.lower() in {"exif", "ifd0"}:
                    continue
                spelling = f"{group}:{query}"
                cases.setdefault(spelling, ("explicit_external_group", spelling))
    # These are native Writer grammar forms that the ordinary-name compiler
    # currently classifies explicitly rather than claiming to execute.
    grammar_row = next((row for row in addressing.rows if row.name.lower() == "artist"), addressing.rows[0])
    for kind, spelling in (
            ("multi_group_last_colon", f"IFD0:EXIF:{grammar_row.name}"),
            ("valueconv_hash", f"{grammar_row.name}#"),
            ("trailing_text", f"{grammar_row.name} trailing"),
            ("language_suffix", f"{grammar_row.name}-en"),
            ("shortcut_or_invalid", f"${grammar_row.name}")):
        cases.setdefault(spelling, (kind, spelling))
    distinct_write_groups = [row for row in addressing.rows
                             if row.write_group not in {row.group0, row.group1}]
    if distinct_write_groups:
        row = sorted(distinct_write_groups, key=lambda row: row.identity)[0]
        cases.setdefault(row.name, ("distinct_write_group", row.name))
    aliases = [
        {"query": query, "candidate_name": candidate.get("name")}
        for query, observed in sorted(observations["queries"].items())
        for candidate in observed["candidates"]
        if isinstance(candidate.get("name"), str) and candidate["name"].lower() != query
    ]
    return {
        "schema": "setnewvalue_address_differential_v1",
        "inputs": {"dump_sha256": dump_sha256, "observations_sha256": observations_sha256,
                   "operands_sha256": operands_sha256},
        "source_report": report,
        "rows": [asdict(row) for row in addressing.rows],
        "cases": [_case(addressing, observations, spelling, kind)
                  for kind, spelling in sorted(cases.values(), key=lambda item: (item[0], item[1].lower(), item[1]))],
        "observed_alias_name_differences": aliases,
        "limitations": {
            "multi_group": "classified from current ordinary resolver; Writer family selection is not yet compiled",
            "valueconv_hash": "classified; conversion type route is not yet compiled",
            "trailing_text": "classified; native tag-key trailing-text normalization is not yet compiled",
            "language_suffix": "classified; LANG_INFO route is not yet compiled",
            "shortcuts": "classified; Shortcuts recursion is not yet compiled",
        },
    }


def main() -> None:
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dump", type=Path)
    parser.add_argument("observations", type=Path)
    parser.add_argument("operands", type=Path)
    parser.add_argument("-o", "--output", type=Path, required=True)
    args = parser.parse_args()
    document = json.loads(args.dump.read_text(encoding="utf-8"))
    observations = json.loads(args.observations.read_text(encoding="utf-8"))
    fixture = export(document, observations, dump_sha256=_sha(args.dump),
                     observations_sha256=_sha(args.observations), operands_sha256=_sha(args.operands))
    args.output.write_text(json.dumps(fixture, sort_keys=True, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
