"""Replay a literal Task18 field against Task8 v3 raw child occurrences.

The signed Task18 attribution document supplies a finite field map.  A map is
an assertion to check, never evidence by itself: the v3 validator must first
replay the entire receipt and these rows are then read from its raw children.
"""
from __future__ import annotations

import hashlib
import json
import re
import struct
from dataclasses import dataclass
from pathlib import Path

import runtime_ownership as ownership
from genshare import attribute


class BlockedAttribution(ValueError):
    pass


@dataclass(frozen=True)
class _Number:
    spelling: str


def _reject_constant(value: str) -> None:
    raise ValueError(f"non-JSON constant {value}")


def parse_field_json(raw: bytes, side: str) -> dict:
    """Parse a raw child for field proof without the comparator's float coercion."""
    try:
        document = json.loads(raw.decode("utf-8"), object_pairs_hook=attribute._reject_duplicate_pairs,
                              parse_int=_Number, parse_float=_Number,
                              parse_constant=_reject_constant)
    except (UnicodeError, ValueError) as exc:
        raise BlockedAttribution(f"BLOCKED_ATTRIBUTION: invalid {side} raw JSON") from exc
    if not isinstance(document, list) or len(document) != 1 or not isinstance(document[0], dict):
        raise BlockedAttribution(f"BLOCKED_ATTRIBUTION: {side} needs one JSON object")
    return document[0]


def _serialized(value: object) -> str:
    if isinstance(value, _Number):
        return value.spelling
    if isinstance(value, list):
        return "[" + ",".join(_serialized(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{" + ",".join(json.dumps(key, ensure_ascii=False) + ":" + _serialized(item)
                              for key, item in value.items()) + "}"
    return json.dumps(value, ensure_ascii=False, separators=(",", ":"))


def _safe_value(value: object) -> object:
    # JSON-safe lossless representation: Python float would round or overflow.
    if isinstance(value, _Number):
        return {"number_spelling": value.spelling}
    if isinstance(value, list):
        return [_safe_value(item) for item in value]
    if isinstance(value, dict):
        return {key: _safe_value(item) for key, item in value.items()}
    return value


def field_occurrences(document: dict) -> list[dict]:
    counts = {}
    result = []
    for position, (key, value) in enumerate(document.items()):
        group, _, name = key.partition(":")
        identity = (group, name)
        counts[identity] = counts.get(identity, 0) + 1
        serialized = _serialized(value)
        result.append({"raw_key": key, "group": group if name else "", "name": name if name else key,
                       "duplicate_instance": counts[identity], "position": position,
                       "type": "number" if isinstance(value, _Number) else type(value).__name__,
                       "value": _safe_value(value), "raw_serialized": serialized})
    return result


def _child(receipt: dict, mode: str, carrier: str, side: str) -> list[dict]:
    run = receipt.get("runs", {}).get(mode, {})
    children = [item for item in run.get("children", []) if item.get("relative_path") == carrier]
    if len(children) != 1:
        raise BlockedAttribution(f"BLOCKED_ATTRIBUTION: {mode} has no unique carrier {carrier}")
    process = json.loads(Path(children[0]["process"]["path"]).read_bytes(),
                         object_pairs_hook=attribute._reject_duplicate_pairs)
    record = process[side]
    raw = Path(record["stdout"]["path"]).read_bytes()
    return field_occurrences(parse_field_json(raw, side))


def _ifd0_tags(raw: bytes) -> list[int]:
    """Read only bounded TIFF IFD0 entries; unknown containers cannot prove a field."""
    if raw.startswith(b"\xff\xd8"):
        offset = 2
        exif_segments = []
        while offset + 4 <= len(raw):
            if raw[offset:offset + 2] in (b"\xff\xd9", b"\xff\xda"):
                break
            if raw[offset] != 0xff or raw[offset + 1] == 0x00:
                raise BlockedAttribution("BLOCKED_ATTRIBUTION: malformed JPEG carrier")
            size = int.from_bytes(raw[offset + 2:offset + 4], "big")
            if size < 2 or offset + 2 + size > len(raw):
                raise BlockedAttribution("BLOCKED_ATTRIBUTION: truncated JPEG carrier")
            payload = raw[offset + 4:offset + 2 + size]
            if raw[offset + 1] == 0xe1 and payload.startswith(b"Exif\x00\x00"):
                exif_segments.append(payload[6:])
            offset += 2 + size
        if len(exif_segments) != 1:
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: no unique bounded EXIF carrier")
        raw = exif_segments[0]
    if len(raw) < 8 or raw[:2] not in (b"II", b"MM"):
        raise BlockedAttribution("BLOCKED_ATTRIBUTION: unsupported field carrier")
    endian = "<" if raw[:2] == b"II" else ">"
    if struct.unpack_from(endian + "H", raw, 2)[0] != 42:
        raise BlockedAttribution("BLOCKED_ATTRIBUTION: unsupported TIFF header")
    offset = struct.unpack_from(endian + "I", raw, 4)[0]
    if offset < 8 or offset + 2 > len(raw):
        raise BlockedAttribution("BLOCKED_ATTRIBUTION: IFD0 offset absent")
    count = struct.unpack_from(endian + "H", raw, offset)[0]
    if offset + 2 + 12 * count + 4 > len(raw):
        raise BlockedAttribution("BLOCKED_ATTRIBUTION: truncated IFD0")
    return [struct.unpack_from(endian + "H", raw, offset + 2 + 12 * index)[0]
            for index in range(count)]


def _verify_field_carrier(receipt: dict, carrier: str, identity: ownership.StableFieldId,
                          row: dict, token: str, oracle_key: str, candidate_key: str) -> None:
    # The only reviewed route here is Exif::Main in TIFF IFD0 through the
    # generated conversion arm guarded by Token::Engine in ifd_engine.rs.
    if (token != "engine" or row.get("owner") != "generated"
            or row.get("symbol") != "src/exiftool_tables/conv/exif_main.rs::decode"
            or not re.fullmatch(r"0x[0-9a-f]{4}", identity.value)
            or oracle_key != f"EXIF:IFD0:{candidate_key.removeprefix('IFD0:')}"
            or not candidate_key.startswith("IFD0:")
            or candidate_key.count(":") != 1):
        raise BlockedAttribution("BLOCKED_ATTRIBUTION: field/group/token/source owner bridge absent")
    matches = [item for item in receipt.get("selection", {}).get("ordered_manifest", [])
               if item.get("relative_path") == carrier]
    if len(matches) != 1:
        raise BlockedAttribution("BLOCKED_ATTRIBUTION: carrier manifest binding absent")
    entry = matches[0]
    path = Path(entry.get("staged_path", ""))
    if (not path.is_file() or path.stat().st_size > 64_000_000
            or hashlib.sha256(path.read_bytes()).hexdigest() != entry.get("sha256")):
        raise BlockedAttribution("BLOCKED_ATTRIBUTION: carrier bytes differ from manifest")
    for mode in ("control-empty", token):
        children = [item for item in receipt.get("runs", {}).get(mode, {}).get("children", [])
                    if item.get("relative_path") == carrier]
        if len(children) != 1:
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: carrier child absent")
        process = json.loads(Path(children[0]["process"]["path"]).read_bytes(),
                             object_pairs_hook=attribute._reject_duplicate_pairs)
        if process.get("source_sha256") != entry["sha256"] or process.get("staged_sha256") != entry["sha256"]:
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: child source bytes differ")
    if _ifd0_tags(path.read_bytes()).count(int(identity.value, 16)) != 1:
        raise BlockedAttribution("BLOCKED_ATTRIBUTION: actual IFD0 source field absent or repeated")


def _relative(rows: list[dict]) -> list[tuple]:
    return [(row["raw_key"], row["type"], row["raw_serialized"], row["duplicate_instance"])
            for row in rows]


def project(receipt: dict, field_map: object, fields: list[str], rows: list[dict]) -> list[dict]:
    """Require exact raw, ordered, typed observations for every Task1 field.

    The map uses Task1 stable IDs, not the older ambiguous ``Exif::Main:id``
    shorthand. A field also needs matching raw carrier bytes, IFD0 group,
    engine token and generated owner; the v3 JSON cannot identify a source
    field from a tag name.
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
        _verify_field_carrier(receipt, carrier, identity, row, spec["token"], oracle_key, candidate_key)
        def selected(mode: str, side: str, key: str) -> list[dict]:
            return [item for item in _child(receipt, mode, carrier, side) if item["raw_key"] == key]
        # -G0:1:4 -a retains CopyN identities. A single signed key cannot
        # account for another copy, so refuse the entire ambiguous carrier.
        for side, mode, prefix in (("oracle", "control-empty", "EXIF:IFD0:"),
                                   ("candidate", "control-empty", "IFD0:"),
                                   ("candidate", spec["token"], "IFD0:")):
            copies = [item for item in _child(receipt, mode, carrier, side)
                      if re.fullmatch(re.escape(prefix) + r"Copy\d+:" + re.escape(name), item["raw_key"])]
            if copies:
                raise BlockedAttribution("BLOCKED_ATTRIBUTION: multi-copy field carrier unsupported")
        oracle = selected("control-empty", "oracle", oracle_key)
        on = selected("control-empty", "candidate", candidate_key)
        off = selected(spec["token"], "candidate", candidate_key)
        fallback_carrier = spec["fallback_carrier"]
        if fallback_carrier == carrier:
            raise BlockedAttribution("UNEXERCISED: fallback needs a distinct decline carrier")
        if fallback_carrier is not None and fallback_carrier not in receipt["selection"]["ordered_paths"]:
            raise BlockedAttribution("UNEXERCISED: fallback carrier absent from Task8 selection")
        if fallback_carrier is not None:
            _verify_field_carrier(receipt, fallback_carrier, identity, row, spec["token"],
                                  oracle_key, candidate_key)
            for side, mode, prefix in (("oracle", "control-empty", "EXIF:IFD0:"),
                                       ("candidate", "control-empty", "IFD0:"),
                                       ("candidate", spec["token"], "IFD0:")):
                if any(re.fullmatch(re.escape(prefix) + r"Copy\d+:" + re.escape(name), item["raw_key"])
                       for item in _child(receipt, mode, fallback_carrier, side)):
                    raise BlockedAttribution("BLOCKED_ATTRIBUTION: multi-copy fallback carrier unsupported")
        fallback_on = ([item for item in _child(receipt, "control-empty", fallback_carrier, "candidate")
                        if item["raw_key"] == candidate_key] if fallback_carrier else [])
        fallback_off = ([item for item in _child(receipt, spec["token"], fallback_carrier, "candidate")
                         if item["raw_key"] == candidate_key] if fallback_carrier else [])
        if not oracle or not on or len(oracle) != len(on):
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
        if residual and (fallback_carrier is None or not fallback_on or _relative(fallback_on) != _relative(fallback_off)):
            raise BlockedAttribution("UNEXERCISED: required decline fallback is not stable")
        if residual:
            fallback_oracle = [item for item in _child(receipt, "control-empty", fallback_carrier, "oracle")
                               if item["raw_key"] == oracle_key]
            if not fallback_oracle or [(item["type"], item["raw_serialized"]) for item in fallback_oracle] != [(item["type"], item["raw_serialized"]) for item in fallback_on]:
                raise BlockedAttribution("BLOCKED_ATTRIBUTION: fallback oracle value differs")
        if not residual and (fallback_carrier is not None or fallback_on or fallback_off):
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: fallback has no Task1 residual owner")
        # The oracle must agree in typed value, while exact group spelling may
        # differ between ExifTool's -G1 and OxiDex's public projection.
        if [(item["type"], item["raw_serialized"]) for item in oracle] != [(item["type"], item["raw_serialized"]) for item in on]:
            raise BlockedAttribution("BLOCKED_ATTRIBUTION: oracle value differs")
        result.append({"source_field": field, "stable_field_id": stable,
                       "carrier": carrier, "token": spec["token"],
                       "on_count": len(on), "off_count": len(off),
                       "fallback_count": len(fallback_on)})
    return result
