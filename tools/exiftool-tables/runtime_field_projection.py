"""Replay a literal Task18 field against Task8 v3 raw child occurrences.

The signed Task18 attribution document supplies a finite field map.  A map is
an assertion to check, never evidence by itself: the v3 validator must first
replay the entire receipt and these rows are then read from its raw children.
"""
from __future__ import annotations

import json
from pathlib import Path

import runtime_ownership as ownership
from genshare import attribute


class BlockedAttribution(ValueError):
    pass


def _child(receipt: dict, mode: str, carrier: str, side: str) -> list[dict]:
    run = receipt.get("runs", {}).get(mode, {})
    children = [item for item in run.get("children", []) if item.get("relative_path") == carrier]
    if len(children) != 1:
        raise BlockedAttribution(f"BLOCKED_ATTRIBUTION: {mode} has no unique carrier {carrier}")
    process = json.loads(Path(children[0]["process"]["path"]).read_bytes(),
                         object_pairs_hook=attribute._reject_duplicate_pairs)
    record = process[side]
    raw = Path(record["stdout"]["path"]).read_bytes()
    return attribute.occurrence_sequence(attribute.parse_json_output(raw, side),
                                         normalize_access_date=False)


def project(receipt: dict, field_map: object, fields: list[str], rows: list[dict]) -> list[dict]:
    """Require exact raw, ordered, typed observations for every Task1 field.

    The map uses Task1 stable IDs, not the older ambiguous ``Exif::Main:id``
    shorthand. A field's tag name also has to agree with the source conversion
    ledger; the v3 JSON alone cannot identify a source field from a tag name.
    An unchanged output under silence is unexercised even when a signed map
    calls it a fallback, since v3 does not expose the producer of that copy.
    """
    if not isinstance(field_map, list) or len(field_map) != len(fields):
        raise BlockedAttribution("BLOCKED_ATTRIBUTION: field map is incomplete")
    by_id = {ownership.StableFieldId.from_row(row).text(): row for row in rows}
    source_ledger = json.loads((Path(__file__).parent / "conv_exif_main_ledger.json").read_text())
    generated = {item["id"]: item for item in source_ledger["generated"]}
    result = []
    seen = set()
    for spec, field in zip(field_map, fields):
        if not isinstance(spec, dict) or set(spec) != {
            "source_field", "stable_field_id", "owner", "carrier", "token",
            "oracle_key", "candidate_key", "on", "off", "fallback_carrier",
            "fallback_on", "fallback_off"
        } or spec["source_field"] != field:
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: literal field map differs")
        stable = spec["stable_field_id"]
        row = by_id.get(stable)
        if row is None or stable in seen or spec["owner"] != row["symbol"]:
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: Task1 field or owner differs")
        seen.add(stable)
        identity = ownership.StableFieldId.from_row(row)
        if (identity.module != "Exif" or identity.table != "Main"
                or identity.kind != "numeric" or field != identity.text()):
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: no reviewed literal source mapping")
        source = generated.get(identity.value)
        if source is None or source["source_sha256"] != row["source_sha256"]:
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: source ledger provenance differs")
        name = source["name"]
        if sum(item["name"] == name for item in source_ledger["generated"]) != 1:
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: output name is not unique in source table")
        candidate_key = spec["candidate_key"]
        oracle_key = spec["oracle_key"]
        if (not isinstance(candidate_key, str) or candidate_key.split(":")[-1] != name
                or not isinstance(oracle_key, str) or oracle_key.split(":")[-1] != name
                or spec["token"] not in attribute.TOKENS):
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: output key or token is unmapped")
        carrier = spec["carrier"]
        if carrier not in receipt.get("selection", {}).get("ordered_paths", []):
            raise BlockedAttribution("UNEXERCISED: carrier absent from Task8 selection")
        def selected(mode: str, side: str, key: str) -> list[dict]:
            return [item for item in _child(receipt, mode, carrier, side) if item["raw_key"] == key]
        oracle = selected("control-empty", "oracle", oracle_key)
        on = selected("control-empty", "candidate", candidate_key)
        off = selected(spec["token"], "candidate", candidate_key)
        fallback_carrier = spec["fallback_carrier"]
        if fallback_carrier == carrier:
            raise BlockedAttribution("UNEXERCISED: fallback needs a distinct decline carrier")
        if fallback_carrier is not None and fallback_carrier not in receipt["selection"]["ordered_paths"]:
            raise BlockedAttribution("UNEXERCISED: fallback carrier absent from Task8 selection")
        fallback_on = ([item for item in _child(receipt, "control-empty", fallback_carrier, "candidate")
                        if item["raw_key"] == candidate_key] if fallback_carrier else [])
        fallback_off = ([item for item in _child(receipt, spec["token"], fallback_carrier, "candidate")
                         if item["raw_key"] == candidate_key] if fallback_carrier else [])
        if not oracle or not on or oracle_key == candidate_key and len(oracle) != len(on):
            raise BlockedAttribution("UNEXERCISED: no nonzero oracle/on occurrence")
        if (spec["on"] != on or spec["off"] != off or spec["fallback_on"] != fallback_on
                or spec["fallback_off"] != fallback_off):
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: typed ordered raw occurrences differ")
        if off:
            raise BlockedAttribution("UNEXERCISED: generated-off retained the generated key")
        if on == off:
            raise BlockedAttribution("UNEXERCISED: silence did not distinguish this field")
        residual = [item for item in rows if item["module"] == identity.module
                    and item["table"] == identity.table
                    and item["field"] == {"kind": "index", "value": f"IFD0/{identity.value}"}
                    and item.get("residual_disposition") == "fallback-on-decline"]
        if residual and (fallback_carrier is None or not fallback_on or fallback_on != fallback_off):
            raise BlockedAttribution("UNEXERCISED: required decline fallback is not stable")
        if residual:
            fallback_oracle = [item for item in _child(receipt, "control-empty", fallback_carrier, "oracle")
                               if item["raw_key"] == oracle_key]
            if not fallback_oracle or [item["raw_serialized"] for item in fallback_oracle] != [item["raw_serialized"] for item in fallback_on]:
                raise BlockedAttribution("BLOCKED_ATTRIBUTION: fallback oracle value differs")
        if not residual and (fallback_carrier is not None or fallback_on or fallback_off):
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: fallback has no Task1 residual owner")
        # The oracle must agree in typed value, while exact group spelling may
        # differ between ExifTool's -G1 and OxiDex's public projection.
        if [item["raw_serialized"] for item in oracle] != [item["raw_serialized"] for item in on]:
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: oracle value differs")
        result.append({"source_field": field, "stable_field_id": stable,
                       "carrier": carrier, "token": spec["token"],
                       "on_count": len(on), "off_count": len(off),
                       "fallback_count": len(fallback_on)})
    return result
