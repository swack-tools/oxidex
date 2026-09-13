"""Canonical native-reader evidence encoding, shared without compiler semantics.

No expected function bodies or execution model live here. The compiler supplies
its closed recognition; the verifier obtains independent native observations.
"""
import hashlib
import json
from pathlib import PurePosixPath
import re

class ReaderRefused(ValueError):
    pass

FUNCTIONS = (("get16u", "Image::ExifTool::Get16u"),
             ("do_unpack_std", "Image::ExifTool::DoUnpackStd"),
             ("set_byte_order", "Image::ExifTool::SetByteOrder"),
             ("get_byte_order", "Image::ExifTool::GetByteOrder"))

# Preserve quoted literals and regexes verbatim. Stripping all whitespace from
# Perl would incorrectly equate e.g. /^B ig/ with /^Big/.
_TOKEN = re.compile(
    r"\s+|'(?:\\.|[^'\\])*'|\"(?:\\.|[^\"\\])*\"|/(?:\\.|[^/\\])*/[a-z]*"
    r"|(?:[A-Za-z_]\w*::)*[A-Za-z_]\w*|\d+|[^\s]"
)


def body_tokens(source):
    if not isinstance(source, str):
        raise ReaderRefused("missing native reader body")
    result = [m.group() for m in _TOKEN.finditer(source) if not m.group().isspace()]
    # B::Deparse on supported Perls varies in the parentheses around a scalar
    # declaration. Removing only that structural pair does not change binding.
    normalized, at = [], 0
    while at < len(result):
        if (result[at:at + 3] == ["my", "(", "$"] and at + 4 < len(result)
                and re.fullmatch(r"[A-Za-z_]\w*", result[at + 3])
                and result[at + 4] == ")"):
            normalized.extend(["my", "$", result[at + 3]])
            at += 5
        else:
            normalized.append(result[at])
            at += 1
    return normalized


def source_fact(fact, name):
    if (not isinstance(fact, dict) or fact.get("__perl") != "CODE"
            or fact.get("resolved") is not True or fact.get("__name") != name):
        raise ReaderRefused("native reader function is unresolved or rebound")
    file, sha = fact.get("source_file"), fact.get("source_sha256")
    if (not isinstance(file, str) or not file or PurePosixPath(file).is_absolute()
            or any(p in {".", ".."} for p in file.split("/"))
            or PurePosixPath(file).as_posix() != file or "\\" in file
            or not isinstance(sha, str) or re.fullmatch(r"[0-9a-f]{64}", sha) is None):
        raise ReaderRefused("native reader source provenance is unavailable")
    return (name, file, sha, body_tokens(fact.get("__deparse")))


def fingerprint(snapshot):
    if not isinstance(snapshot, dict):
        raise ReaderRefused("missing native reader facts")
    loaded = snapshot.get("loaded_functions")
    if not isinstance(loaded, dict):
        raise ReaderRefused("missing loaded reader functions")
    facts = tuple(source_fact(loaded.get(key), name) for key, name in FUNCTIONS)
    observations = snapshot.get("observations")
    orders = observations.get("orders") if isinstance(observations, dict) else None
    if not isinstance(orders, dict):
        raise ReaderRefused("missing native byte-order facts")
    if any(not isinstance(orders.get(order), dict) for order in ("II", "MM")):
        raise ReaderRefused("malformed native byte-order facts")
    # Select only stable observations; extra diagnostic timestamps or probe
    # timings do not alter the identity of the native source and input state.
    selected_orders = {order: {key: orders.get(order, {}).get(key)
                       for key in ("set_return", "reported_byte_order", "unpack_std_s", "error")}
                       for order in ("II", "MM")}
    payload = {"functions": facts, "orders": selected_orders}
    return hashlib.sha256(json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def observation_failure(snapshot):
    """Check independent native observations; no compiler body rules are used."""
    if not isinstance(snapshot, dict) or snapshot.get("resolved") is not True:
        return "native reader observations are unresolved"
    absent = {"unpack": False, "pack": False}
    if snapshot.get("builtin_overrides") != absent or snapshot.get("isolated_builtin_overrides") != absent:
        return "native binary builtins are overridden or unobserved"
    observations = snapshot.get("observations")
    if not isinstance(observations, dict):
        return "missing native byte-order observations"
    orders = observations.get("orders")
    if not isinstance(orders, dict) or set(orders) != {"II", "MM"}:
        return "missing native byte-order transitions"
    for order, template in (("II", "v"), ("MM", "n")):
        got = orders[order]
        if (not isinstance(got, dict) or got.get("set_return") != 1
                or got.get("reported_byte_order") != order or got.get("unpack_std_s") != template
                or got.get("error") is not None):
            return "native unsigned reader does not match inherited byte order"
    loaded_state = snapshot.get("loaded_state")
    if not isinstance(loaded_state, dict):
        return "missing loaded native reader state"
    loaded_order = loaded_state.get("reported_byte_order")
    if loaded_order not in orders or loaded_state.get("unpack_std_s") != orders[loaded_order].get("unpack_std_s"):
        return "loaded native reader state disagrees with its reported byte order"
    initial = observations.get("initial_byte_order")
    if (initial not in {"II", "MM"} or observations.get("restore_return") != 1
            or observations.get("restored_byte_order") != initial
            or observations.get("restore_error") is not None):
        return "native reader state was not restored"
    probe = snapshot.get("get16u_probe")
    if (not isinstance(probe, dict) or probe.get("kind") != "get16u_native_probe_v1"
            or probe.get("valid_case_count") != 262144 or probe.get("boundary_case_count") != 10
            or probe.get("failure_count") != 0 or probe.get("failure_details") != []
            or probe.get("restore_return") != 1 or probe.get("restored_byte_order") != initial
            or probe.get("restore_error") is not None):
        return "native unsigned-reader probe is incomplete or disagrees"
    rows = probe.get("boundary_cases")
    expected_boundaries = [
        (order, name, offset, bytes_hex, outcome)
        for order in ("II", "MM")
        for name, offset, bytes_hex, outcome in (
            ("empty_at_zero", 0, "", "undef"),
            ("one_byte_at_zero", 0, "34", "undef"),
            ("one_byte_remaining", 2, "aabb34", "undef"),
            ("offset_at_end", 2, "aabb", "undef"),
            ("offset_beyond_end", 3, "aabb", "error"),
        )
    ]
    if not isinstance(rows, list) or len(rows) != len(expected_boundaries):
        return "native unsigned-reader boundary observations are incomplete"
    for row, expected in zip(rows, expected_boundaries):
        if not isinstance(row, dict):
            return "native unsigned-reader boundary observations are incomplete"
        order, name, offset, bytes_hex, outcome = expected
        required = {"byte_order", "name", "offset", "bytes_hex", "expected_outcome",
                    "expected", "observed", "error", "matches_expected_native_outcome"}
        if not required.issubset(row):
            return "native unsigned-reader boundary observations are incomplete"
        if (row["byte_order"], row["name"], row["offset"], row["bytes_hex"], row["expected_outcome"]) != expected:
            return "native unsigned-reader boundary observations are incomplete"
        if (row["expected"] is not None or row["observed"] is not None
                or row["matches_expected_native_outcome"] is not True):
            return "native unsigned-reader boundary observations are incomplete"
        if outcome == "error":
            if not isinstance(row["error"], str) or not row["error"]:
                return "native unsigned-reader boundary observations are incomplete"
        elif row["error"] is not None:
            return "native unsigned-reader boundary observations are incomplete"
    return None
