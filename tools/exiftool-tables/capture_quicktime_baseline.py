#!/usr/bin/env python3
"""Extract three verbatim hydrated tables from a recorded full pinned dump.

This reads the full JSON document; run it under the host's shared heavy-job lock.
The supplied commit identifies the dump tool used by the preceding capture.
It is checked against the caller's separately recorded capture evidence.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

import quicktime_atom_tables as selector


def extract(document, *, full_hash, source_commit, perl_version, tool_hash):
    pin = (selector.ROOT / ".exiftool-version").read_text().strip()
    if document.get("exiftool_version") != pin or document.get("modules_failed") != 0:
        raise ValueError("full dump must match the pin and have no failed modules")
    module = document["modules"]["QuickTime"]
    tables = {name: module["tables"][name] for name in selector.TABLES}
    if module["table_count"] != len(module["tables"]):
        raise ValueError("QuickTime table count does not conserve captured identities")
    result = {"exiftool_version": pin,
            "modules": {"QuickTime": {"module": module["module"],
                                        "package": module["package"],
                                        "table_count": len(tables), "tables": tables}},
            "capture_scope": {"kind": "verbatim selected tables from hydrated dump",
                              "tables": ["QuickTime::" + name for name in selector.TABLES],
                              "source_module_table_count": module["table_count"],
                              "source_commit": source_commit, "full_dump_sha256": full_hash,
                              "dump_tool_sha256": tool_hash, "perl_version": perl_version}}

    if "quicktime_itemlist_reader_protocol" in document:
        result["quicktime_itemlist_reader_protocol"] = document["quicktime_itemlist_reader_protocol"]
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dump", required=True, type=Path)
    parser.add_argument("--source-commit", required=True)
    parser.add_argument("--perl-version", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    selector.validate_outputs(args.dump, [args.output])
    state = selector.instrument.git_state(selector.ROOT)
    override = selector.instrument.refuse_if_dirty(state, "capture_quicktime_baseline.py")
    selector.instrument.print_header(tool="capture_quicktime_baseline.py", git=state,
                                     dirty_overridden=override,
                                     extra=["scope: extract selected hydrated tables; no support claim"])
    source_commit = subprocess.check_output(["git", "-C", str(selector.ROOT), "rev-parse",
                                             args.source_commit + "^{commit}"], text=True).strip()
    tool = subprocess.check_output(["git", "-C", str(selector.ROOT), "show",
                                    source_commit + ":tools/exiftool-tables/dump_tables.pl"])
    with args.dump.open("rb") as stream:
        full_hash = hashlib.file_digest(stream, "sha256").hexdigest()
    with args.dump.open() as stream:
        document = json.load(stream)
    result = extract(document, full_hash=full_hash, source_commit=source_commit,
                     perl_version=args.perl_version, tool_hash=hashlib.sha256(tool).hexdigest())
    encoded = selector.serialized(result)
    selector.report(encoded.encode())  # validate provenance and all selected rows before writing
    with args.output.open("x") as target:
        target.write(encoded)


if __name__ == "__main__":
    main()
