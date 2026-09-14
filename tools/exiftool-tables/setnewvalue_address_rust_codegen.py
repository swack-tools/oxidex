"""Emit inactive, source-selected SetNewValue EXIF addressing operands."""
from __future__ import annotations

from typing import Any, Mapping
import re

from checkexif_recipes import RecipeMalformed, RecipeRefused
from scalar_helper_codegen import rust_string
from setnewvalue_addressing import (RUNTIME_STATUS, Addressing, _owned_names,
                                    _candidate_identity, compile_addressing,
                                    _observed_candidates)
from setnewvalue_ownership_ledger import (build_ledger, owned_names,
                                           qualified_ownership, validate_ledger)


def _opt_text(value: Any) -> str:
    return "Some(" + rust_string(value) + ")" if isinstance(value, str) else "None"


def _row(index: int, row: Any) -> str:
    values = (row.module, row.table, row.full_name, row.raw_id, row.name,
              row.group0, row.group1, row.write_group)
    return "StaticSetNewValueAddress { index: %d, module: %s, table: %s, full_name: %s, raw_id: %s, name: %s, group0: %s, group1: %s, write_group: %s }" % ((index,) + tuple(rust_string(item) for item in values))


def _owned_names_const(names: set[str] | frozenset[str]) -> str:
    return ("pub(crate) const SET_NEW_VALUE_OWNED_NAMES: &[&str] = &[" +
            ", ".join(rust_string(name) for name in sorted(names)) + "];\n")


def _qualified_owned_const(entries) -> str:
    chunks = ["pub(crate) const SET_NEW_VALUE_OWNED_QUALIFIED: &[StaticSetNewValueOwnedName] = &[\n"]
    chunks.extend("    StaticSetNewValueOwnedName { group0: %s, group1: %s, name: %s, removed: %s },\n" % (
        rust_string(entry.group0), rust_string(entry.group1), rust_string(entry.name),
        "true" if entry.removed else "false") for entry in entries)
    chunks.append("];\n")
    return "".join(chunks)


_SHA256 = re.compile(r"[0-9a-f]{64}\Z")


def _source_sha(value: Any, context: str) -> str:
    if not isinstance(value, str) or _SHA256.fullmatch(value) is None:
        raise RecipeRefused(f"{context} is not an authenticated SHA-256")
    return value


def _source_capture_identity(document: Mapping[str, Any], addressing: Addressing) -> dict[str, str]:
    """Return the final-loaded source intersection used by public routing.

    These are executable operands, rather than release comments: runtime joins
    them to the final scalar stage before it can route a public address.  Each
    digest must agree with both its exact captured helper/table fact and the
    final loaded writer closure.
    """
    write_context = document.get("native_write_capture_context")
    if not isinstance(write_context, Mapping) or write_context.get("kind") != "write_exif_postload_context_v1" or write_context.get("resolved") is not True:
        raise RecipeRefused("WriteExif post-load capture context is unavailable")
    loaded = write_context.get("loaded_modules")
    if not isinstance(loaded, Mapping):
        raise RecipeRefused("WriteExif post-load source closure is unavailable")
    required = ("Image/ExifTool.pm", "Image/ExifTool/WriteExif.pl", "Image/ExifTool/Writer.pl", "Image/ExifTool/Exif.pm")
    hashes = {path: _source_sha(loaded.get(path), f"WriteExif post-load closure {path}") for path in required}

    # The generic capture closure is independently sealed and must describe the
    # same selected files.  This catches a partially reloaded source tree.
    generic_modules = addressing.capture_context["loaded_closure"]["modules"]
    generic = {item.get("inc"): item.get("source_sha256") for item in generic_modules
               if isinstance(item, Mapping)}
    for path in required:
        if generic.get(path) != hashes[path]:
            raise RecipeRefused(f"address and WriteExif closures disagree for {path}")

    tables = document.get("native_write_tables")
    try:
        table = tables["Exif"]["Main"]
        write = table["effective_write_proc"]
        write_fact = write["effective"]
    except (KeyError, TypeError) as error:
        raise RecipeRefused("Exif::Main effective WriteExif source is unavailable") from error
    if (write.get("present") is not True or write_fact.get("resolved") is not True
            or write_fact.get("__name") != "Image::ExifTool::Exif::WriteExif"
            or write_fact.get("source_file") != "Image/ExifTool/WriteExif.pl"
            or _source_sha(write_fact.get("source_sha256"), "Exif::Main WriteExif source") != hashes["Image/ExifTool/WriteExif.pl"]):
        raise RecipeRefused("Exif::Main WriteExif source does not join the loaded closure")
    if addressing.set_new_value_source_file != "Image/ExifTool/Writer.pl" or addressing.set_new_value_source_sha256 != hashes["Image/ExifTool/Writer.pl"]:
        raise RecipeRefused("SetNewValue source does not join the loaded Writer.pl closure")

    registry = document.get("native_write_format_registry")
    if not isinstance(registry, Mapping) or registry.get("state") != "resolved":
        raise RecipeRefused("Exif TIFF registry source is unavailable")
    registry_source = registry.get("source")
    if (not isinstance(registry_source, Mapping)
            or registry_source.get("library_relative_path") != "Image/ExifTool/Exif.pm"
            or _source_sha(registry_source.get("sha256"), "Exif TIFF registry source") != hashes["Image/ExifTool/Exif.pm"]):
        raise RecipeRefused("Exif TIFF registry source does not join the loaded closure")
    version = document.get("exiftool_version")
    if not isinstance(version, str) or version != addressing.capture_context.get("exiftool_version"):
        raise RecipeRefused("address source release differs from native capture context")
    return {
        "exiftool_version": version,
        "main_source_sha256": hashes["Image/ExifTool.pm"],
        "write_exif_source_sha256": hashes["Image/ExifTool/WriteExif.pl"],
        "writer_source_sha256": hashes["Image/ExifTool/Writer.pl"],
        "exif_source_sha256": hashes["Image/ExifTool/Exif.pm"],
    }


def _capture_const(identity: Mapping[str, str] | None) -> str:
    if identity is None:
        return "pub(crate) const SET_NEW_VALUE_ADDRESS_CAPTURE: Option<StaticSetNewValueAddressCapture> = None;\n"
    return ("pub(crate) const SET_NEW_VALUE_ADDRESS_CAPTURE: Option<StaticSetNewValueAddressCapture> = Some("
            "StaticSetNewValueAddressCapture { exiftool_version: %s, main_source_sha256: %s, write_exif_source_sha256: %s, writer_source_sha256: %s, exif_source_sha256: %s });\n" %
            tuple(rust_string(identity[key]) for key in ("exiftool_version", "main_source_sha256", "write_exif_source_sha256", "writer_source_sha256", "exif_source_sha256")))


def _families(groups: Mapping[str, Any]) -> tuple[tuple[int, str], ...]:
    values = []
    for family, value in groups.items():
        if not isinstance(family, str) or not family.isdecimal() or not isinstance(value, str):
            raise RecipeMalformed("native lookup group family is malformed")
        number = int(family)
        if number > 255:
            raise RecipeMalformed("native lookup group family exceeds u8")
        values.append((number, value))
    return tuple(sorted(values))


def _omitted_source(prelude: str, names: set[str] | frozenset[str], qualified, *,
                    reason: str, ledger: Mapping[str, Any] | None = None) -> tuple[str, dict[str, Any]]:
    """Render the complete inactive API without retiring protected ownership."""
    chunks = [prelude, "// SetNewValue addressing omitted: unsupported source.\n",
              "pub(crate) const SET_NEW_VALUE_ADDRESS_ROWS: &[StaticSetNewValueAddress] = &[];\n",
              "pub(crate) const SET_NEW_VALUE_LOOKUP: &[StaticNativeLookupCandidate] = &[];\n",
              "pub(crate) const SET_NEW_VALUE_ADMITTED_QUALIFIER_SCOPE: &[StaticSetNewValueQualifierScope] = &[];\n",
              _owned_names_const(names), _qualified_owned_const(qualified), _capture_const(None),
              "pub(crate) const SET_NEW_VALUE_ADDRESSING: Option<&[StaticSetNewValueAddress]> = None;\n"]
    return "".join(chunks), {
        "emitted": False, "reason": reason, "runtime_status": RUNTIME_STATUS,
        "owned_names": len(names), "removed_owned_names": sum(entry.removed for entry in qualified),
        "ownership_ledger_sha256": None if ledger is None else ledger["ledger_sha256"],
    }


def generate(document: Mapping[str, Any], observations: Mapping[str, Any], *,
             prior_ledger: Mapping[str, Any] | None = None,
             bootstrap_ownership_ledger: bool = False) -> tuple[str, dict[str, Any]]:
    prelude = (
        "// @generated by setnewvalue_address_rust_codegen.py; inactive.\n"
        "pub(crate) struct StaticSetNewValueAddress { pub index: usize, pub module: &'static str, pub table: &'static str, pub full_name: &'static str, pub raw_id: &'static str, pub name: &'static str, pub group0: &'static str, pub group1: &'static str, pub write_group: &'static str }\n"
        "pub(crate) struct StaticNativeLookupFamily { pub family: u8, pub value: &'static str }\n"
        "pub(crate) struct StaticNativeLookupCandidate { pub name: &'static str, pub row_index: Option<usize>, pub source_identity_present: bool, pub groups: &'static [StaticNativeLookupFamily] }\n"
        "pub(crate) struct StaticSetNewValueQualifierScope { pub family: u8, pub group: &'static str }\n"
        "pub(crate) struct StaticSetNewValueOwnedName { pub group0: &'static str, pub group1: &'static str, pub name: &'static str, pub removed: bool }\n"
        "pub(crate) struct StaticSetNewValueAddressCapture { pub exiftool_version: &'static str, pub main_source_sha256: &'static str, pub write_exif_source_sha256: &'static str, pub writer_source_sha256: &'static str, pub exif_source_sha256: &'static str }\n"
    )
    # Validate a published ledger before inspecting current source.  A bad
    # historical artifact must fail the command before it can overwrite its
    # terminal ownership union with a convenient current-source projection.
    validated_prior = validate_ledger(prior_ledger) if prior_ledger is not None else None
    try:
        ledger = build_ledger(document, validated_prior, bootstrap=bootstrap_ownership_ledger)
        ledger_names = owned_names(ledger)
        ledger_qualified = qualified_ownership(ledger)
        addressing, report = compile_addressing(document)
        capture_identity = _source_capture_identity(document, addressing)
        by_identity = {row.identity: index for index, row in enumerate(addressing.rows)}
        lookup = []
        for name in sorted(addressing.owned_names):
            # Requiring the observation now means a missing native lookup is an
            # owned omission, never an invitation to call an older writer.
            candidates = _observed_candidates(addressing, observations, name)
            if candidates is None:
                raise RecipeRefused(f"native lookup observation is unavailable for {name}")
            for candidate in candidates:
                identity = _candidate_identity(candidate)
                index = by_identity.get(identity)
                groups = candidate.get("groups")
                if not isinstance(groups, Mapping):
                    raise RecipeMalformed("native lookup candidate lacks groups")
                lookup.append((name, index, identity is not None, _families(groups)))
    except (KeyError, RecipeMalformed, RecipeRefused) as error:
        # Once a ledger was authenticated, its full historical qualified union
        # remains terminal even if this release's source grammar is unsupported.
        # Without a prior ledger there is no published history to preserve.
        if validated_prior is not None:
            return _omitted_source(prelude, owned_names(validated_prior),
                                   qualified_ownership(validated_prior),
                                   reason=str(error), ledger=validated_prior)
        try:
            current_owned_names = _owned_names(document)
        except (RecipeMalformed, RecipeRefused):
            current_owned_names = set()
        return _omitted_source(prelude, current_owned_names, (), reason=str(error))
    chunks = [
        prelude,
        "pub(crate) const SET_NEW_VALUE_ADDRESS_ROWS: &[StaticSetNewValueAddress] = &[\n",
    ]
    chunks.extend("    " + _row(index, row) + ",\n" for index, row in enumerate(addressing.rows))
    chunks.append("];\n")
    chunks.append("pub(crate) const SET_NEW_VALUE_LOOKUP: &[StaticNativeLookupCandidate] = &[\n")
    chunks.extend(
        "    StaticNativeLookupCandidate { name: %s, row_index: %s, source_identity_present: %s, groups: &[%s] },\n" % (
            rust_string(name), "Some(%d)" % index if index is not None else "None",
            "true" if source_identity_present else "false",
            ", ".join("StaticNativeLookupFamily { family: %d, value: %s }" %
                      (family, rust_string(value)) for family, value in families))
        for name, index, source_identity_present, families in lookup)
    chunks.append("];\n")
    chunks.append("pub(crate) const SET_NEW_VALUE_ADMITTED_QUALIFIER_SCOPE: &[StaticSetNewValueQualifierScope] = &[\n")
    chunks.extend("    StaticSetNewValueQualifierScope { family: %d, group: %s },\n" %
                  (family, rust_string(group)) for family, group in addressing.qualifier_scope)
    chunks.append("];\n")
    chunks.append(_owned_names_const(ledger_names))
    chunks.append(_qualified_owned_const(ledger_qualified))
    chunks.append(_capture_const(capture_identity))
    chunks.append("pub(crate) const SET_NEW_VALUE_ADDRESSING: Option<&[StaticSetNewValueAddress]> = Some(SET_NEW_VALUE_ADDRESS_ROWS);\n")
    return "".join(chunks), {
        "emitted": True, "runtime_status": RUNTIME_STATUS,
        "rows": len(addressing.rows), "owned_names": len(ledger_names),
        "removed_owned_names": sum(entry.removed for entry in ledger_qualified),
        "ownership_ledger_sha256": ledger["ledger_sha256"],
        "source_capture_identity": capture_identity,
        "lookup_candidates": len(lookup), "source_report": report,
    }


def main() -> None:
    import argparse
    import json
    from pathlib import Path
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tables", type=Path)
    parser.add_argument("observations", type=Path)
    parser.add_argument("-o", "--output", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--ownership-ledger", type=Path,
                        help="authenticated previous ownership ledger")
    parser.add_argument("--write-ownership-ledger", type=Path,
                        help="write the deterministic updated ownership ledger")
    parser.add_argument("--bootstrap-ownership-ledger", action="store_true",
                        help="explicitly create the initial ownership ledger")
    args = parser.parse_args()
    document = json.loads(args.tables.read_text(encoding="utf-8"))
    prior = json.loads(args.ownership_ledger.read_text(encoding="utf-8")) if args.ownership_ledger else None
    source, report = generate(document, json.loads(args.observations.read_text(encoding="utf-8")),
                              prior_ledger=prior,
                              bootstrap_ownership_ledger=args.bootstrap_ownership_ledger)
    args.output.write_text(source, encoding="utf-8")
    args.report.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    if args.write_ownership_ledger:
        ledger = build_ledger(document, prior, bootstrap=args.bootstrap_ownership_ledger)
        args.write_ownership_ledger.write_text(json.dumps(ledger, sort_keys=True, indent=2) + "\n",
                                                encoding="utf-8")


if __name__ == "__main__":
    main()
