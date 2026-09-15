"""Compile the selected native new-JPEG EXIF byte-order branch.

This is deliberately smaller than the public writer.  It authenticates the
actual Writer.pl method and the executable DoProcessTIFF caller block which
creates a new TIFF header, then admits only the IFD0/no-override branch.
Options and remembered MakerNote state remain explicit runtime refusals.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
from typing import Any, Mapping

from native_reader_facts import body_tokens


def _normalized_body_sha256(body: str) -> str:
    """Digest the complete B::Deparse token stream, ignoring whitespace only."""
    return hashlib.sha256("\x1f".join(body_tokens(body)).encode()).hexdigest()

_SHA = re.compile(r"[0-9a-f]{64}\Z")


class FreshByteOrderRefused(ValueError):
    pass


def _mapping(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise FreshByteOrderRefused(f"{context} is not an object")
    return value


def _sha(value: Any, context: str) -> str:
    if not isinstance(value, str) or _SHA.fullmatch(value) is None:
        raise FreshByteOrderRefused(f"{context} is not a SHA-256 digest")
    return value


def _path(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value or "\\" in value:
        raise FreshByteOrderRefused(f"{context} is not a canonical relative path")
    parsed = PurePosixPath(value)
    if parsed.is_absolute() or any(part in {"", ".", ".."} for part in parsed.parts):
        raise FreshByteOrderRefused(f"{context} is not a canonical relative path")
    return value


def _fact(document: Mapping[str, Any], key: str, expected_name: str, expected_file: str) -> Mapping[str, Any]:
    fact = _mapping(document.get(key), key)
    if (fact.get("resolved") is not True or fact.get("__perl") != "CODE"
            or fact.get("__name") != expected_name):
        raise FreshByteOrderRefused(f"{key} is unresolved or rebound")
    if _path(fact.get("source_file"), f"{key}.source_file") != expected_file:
        raise FreshByteOrderRefused(f"{key} is outside the selected native source")
    _sha(fact.get("source_sha256"), f"{key}.source_sha256")
    if not isinstance(fact.get("__deparse"), str):
        raise FreshByteOrderRefused(f"{key} body is unavailable")
    return fact


def _capture_context(document: Mapping[str, Any], writer_sha: str, core_sha: str) -> None:
    context = _mapping(document.get("capture_context"), "capture_context")
    if context.get("kind") != "fresh_jpeg_byte_order_probe_v1" or context.get("resolved") is not True:
        raise FreshByteOrderRefused("capture context is unresolved")
    loaded = _mapping(context.get("loaded_modules"), "capture_context.loaded_modules")
    if loaded.get("Image/ExifTool/Writer.pl") != writer_sha or loaded.get("Image/ExifTool.pm") != core_sha:
        raise FreshByteOrderRefused("loaded closure does not join selected callable sources")
    for file, sha in loaded.items():
        file = _path(file, "loaded closure path")
        if file != "Image/ExifTool.pm" and not file.startswith("Image/ExifTool/"):
            raise FreshByteOrderRefused("loaded closure escaped selected native namespace")
        _sha(sha, "loaded closure source")
    closure = _sha(context.get("loaded_closure_sha256"), "loaded closure digest")
    payload = [{"file": file, "sha256": sha} for file, sha in sorted(loaded.items())]
    actual = hashlib.sha256(json.dumps(payload, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    if closure != actual:
        raise FreshByteOrderRefused("loaded closure digest is inconsistent")


# B::Deparse body of Image::ExifTool::SetPreferredByteOrder, with the two
# fallback literal positions marked by None.  Comparing the full token stream
# makes inserted/reordered statements refuse; accepting II/MM at those two
# positions lets a copied executable default change propagate as data.
_METHOD_TEMPLATE: tuple[str | None, ...] = (
    '(', '$', ';', '$', ')', '{', 'package', 'Image::ExifTool', ';', 'use', 'strict', ';',
    '(', 'my', '(', '$', 'self', ',', '$', 'default', ')', '=', '@', '_', ')', ';',
    '(', 'my', '$', 'byteOrder', '=', '(', '(', '(', '(', '$', 'self', '-', '>', 'Options',
    '(', "'ByteOrder'", ')', '|', '|', '$', 'self', '-', '>', 'GetNewValue', '(',
    "'ExifByteOrder'", ')', ')', '|', '|', '$', 'default', ')', '|', '|', '$', 'self', '-',
    '>', '{', "'MAKER_NOTE_BYTE_ORDER'", '}', ')', '|', '|', None, ')', ')', ';',
    'unless', '(', 'SetByteOrder', '(', '$', 'byteOrder', ')', ')', '{', '(', '$', 'self',
    '-', '>', 'Options', '(', "'Verbose'", ')', 'and', 'warn', '(', '(',
    '"Invalid byte order \'${byteOrder}\'\\n"', ')', ')', ')', ';', '(', '$', 'byteOrder',
    '=', '(', '$', 'self', '-', '>', '{', "'MAKER_NOTE_BYTE_ORDER'", '}', '|', '|', None,
    ')', ')', ';', 'SetByteOrder', '(', '$', 'byteOrder', ')', ';', '}', '(', 'return',
    'GetByteOrder', ')', ';', '}'
)

# The only dynamic operation admitted from SetPreferredByteOrder is its
# selected II/MM fallback.  Its two direct package helpers are closed too: they
# mutate and read process-global byte-order state that controls the emitted
# TIFF header.
_SET_BYTE_ORDER_FULL_BODY_SHA256 = '59a7c469a92f6dfa30781a7fcac4c4cf4783bba998448501bcf3220b87af7fe3'
_GET_BYTE_ORDER_FULL_BODY_SHA256 = 'ec444b8559811d658353c39da300d2e105ef2f30bfb0298aee35245111f394d9'


# This is the full normalized executable body of DoProcessTIFF, not merely the
# newly-created-header fragment.  A source change anywhere in the invoking
# routine (including immediately after the header block) is an omission until
# a new closed grammar is supplied.
_DOPROCESS_TIFF_FULL_BODY_SHA256 = '89fadbe942d8a4669b7f2c0329e061333026176e2854a79ab4bae4569008a1e0'

_CALLER_BLOCK: tuple[str, ...] = (
    'unless', '(', 'defined', '(', '$', 'self', '-', '>', '{', "'EXIF_DATA'", '}', ')', ')', '{',
    'my', '$', 'defaultByteOrder', ';', 'if', '(', '(', '$', 'dirInfo', '-', '>', '{',
    "'DirName'", '}', 'and', '(', '$', 'dirInfo', '-', '>', '{', "'DirName'", '}', 'eq',
    "'GPS'", ')', ')', ')', '{', '(', '$', 'defaultByteOrder', '=', '$', 'self', '-', '>', '{',
    "'SaveExifByteOrder'", '}', ')', ';', '}', 'if', '(', '(', '$', 'self', '-', '>',
    'SetPreferredByteOrder', '(', '$', 'defaultByteOrder', ')', 'eq', "'MM'", ')', ')', '{',
    '(', '$', 'self', '-', '>', '{', "'EXIF_DATA'", '}', '=', '"MM\\000*\\000\\000\\000\\cH"', ')', ';',
    '}', 'else', '{', '(', '$', 'self', '-', '>', '{', "'EXIF_DATA'", '}', '=',
    '"II*\\000\\cH\\000\\000\\000"', ')', ';', '}', '}'
)


@dataclass(frozen=True)
class _FreshJpegSourceProfile:
    """Complete callable profiles plus the selected IFD0 caller predicate."""

    name: str
    method_template: tuple[str | None, ...]
    set_byte_order_body_sha256: str
    get_byte_order_body_sha256: str
    caller_body_sha256: str
    caller_block: tuple[str, ...]
    # The fixed fresh-JPEG route is IFD0.  Retain whether source reached
    # SetPreferredByteOrder with no second argument or an IFD0-proven undef
    # default; this is a source predicate, not an inferred common behavior.
    caller_mode: str


_METHOD_TEMPLATE_11_78: tuple[str | None, ...] = (
    '(', '$', ')', '{', 'package', 'Image::ExifTool', ';', 'use', 'strict', ';',
    '(', 'my', '$', 'self', '=', '(', 'shift', '(', ')', ')', ')', ';',
    '(', 'my', '$', 'byteOrder', '=', '(', '(', '(', '$', 'self', '-', '>', 'Options',
    '(', "'ByteOrder'", ')', '|', '|', '$', 'self', '-', '>', 'GetNewValue',
    '(', "'ExifByteOrder'", ')', ')', '|', '|', '$', 'self', '-', '>', '{',
    "'MAKER_NOTE_BYTE_ORDER'", '}', ')', '|', '|', None, ')', ')', ';', 'unless',
    '(', 'SetByteOrder', '(', '$', 'byteOrder', ')', ')', '{', '(', '$', 'self',
    '-', '>', 'Options', '(', "'Verbose'", ')', 'and', 'warn', '(', '(',
    '"Invalid byte order \'${byteOrder}\'\\n"', ')', ')', ')', ';', '(', '$',
    'byteOrder', '=', '(', '$', 'self', '-', '>', '{', "'MAKER_NOTE_BYTE_ORDER'",
    '}', '|', '|', None, ')', ')', ';', 'SetByteOrder', '(', '$', 'byteOrder',
    ')', ';', '}', '(', 'return', 'GetByteOrder', ')', ';', '}',
)

_CALLER_BLOCK_11_78: tuple[str, ...] = (
    'unless', '(', 'defined', '(', '$', 'self', '-', '>', '{', "'EXIF_DATA'", '}', ')', ')',
    '{', 'if', '(', '(', '$', 'self', '-', '>', 'SetPreferredByteOrder', 'eq', "'MM'", ')', ')',
    '{', '(', '$', 'self', '-', '>', '{', "'EXIF_DATA'", '}', '=', '"MM\\000*\\000\\000\\000\\cH"', ')', ';', '}',
    'else', '{', '(', '$', 'self', '-', '>', '{', "'EXIF_DATA'", '}', '=', '"II*\\000\\cH\\000\\000\\000"', ')', ';', '}', '}',
)

_PROFILE_11_78 = _FreshJpegSourceProfile(
    name="no-default-argument-ifd0",
    method_template=_METHOD_TEMPLATE_11_78,
    set_byte_order_body_sha256='59a7c469a92f6dfa30781a7fcac4c4cf4783bba998448501bcf3220b87af7fe3',
    get_byte_order_body_sha256='ec444b8559811d658353c39da300d2e105ef2f30bfb0298aee35245111f394d9',
    caller_body_sha256='17eb23190c1454a26d6b94419ba52ae769300fa6ea93bc19789b3c0729fd509f',
    caller_block=_CALLER_BLOCK_11_78,
    caller_mode="NoDefaultArgument",
)
_PROFILE_12_64 = _FreshJpegSourceProfile(
    name="default-argument-ifd0-12-64",
    method_template=_METHOD_TEMPLATE,
    set_byte_order_body_sha256='59a7c469a92f6dfa30781a7fcac4c4cf4783bba998448501bcf3220b87af7fe3',
    get_byte_order_body_sha256='ec444b8559811d658353c39da300d2e105ef2f30bfb0298aee35245111f394d9',
    caller_body_sha256='0d82e3b6c7fe0d7e1f08db3055735321687666131d20f076516777979081ff5c',
    caller_block=_CALLER_BLOCK,
    caller_mode="Ifd0DefaultArgumentUndef",
)
_CURRENT_SOURCE_PROFILE = _FreshJpegSourceProfile(
    name="default-argument-ifd0",
    method_template=_METHOD_TEMPLATE,
    set_byte_order_body_sha256=_SET_BYTE_ORDER_FULL_BODY_SHA256,
    get_byte_order_body_sha256=_GET_BYTE_ORDER_FULL_BODY_SHA256,
    caller_body_sha256=_DOPROCESS_TIFF_FULL_BODY_SHA256,
    caller_block=_CALLER_BLOCK,
    caller_mode="Ifd0DefaultArgumentUndef",
)
_SOURCE_PROFILES: tuple[_FreshJpegSourceProfile, ...] = (
    _PROFILE_11_78, _PROFILE_12_64, _CURRENT_SOURCE_PROFILE,
)


def _method_default(body: str, template: tuple[str | None, ...]) -> str:
    tokens = body_tokens(body)
    if len(tokens) != len(template):
        raise FreshByteOrderRefused("SetPreferredByteOrder body is outside the admitted grammar")
    values: list[str] = []
    for actual, expected in zip(tokens, template):
        if expected is None:
            if actual not in {"'II'", "'MM'"}:
                raise FreshByteOrderRefused("SetPreferredByteOrder fallback is unsupported")
            values.append(actual[1:-1])
        elif actual != expected:
            raise FreshByteOrderRefused("SetPreferredByteOrder body is outside the admitted grammar")
    if len(values) != 2 or values[0] != values[1]:
        raise FreshByteOrderRefused("SetPreferredByteOrder fallback paths disagree")
    return values[0]


def _matches_caller_profile(body: str, profile: _FreshJpegSourceProfile) -> bool:
    tokens = body_tokens(body)
    locations = [index for index in range(len(tokens))
                 if tuple(tokens[index:index + len(profile.caller_block)]) == profile.caller_block]
    return len(locations) == 1 and _normalized_body_sha256(body) == profile.caller_body_sha256


def _source_profile(method_body: str, set_body: str, get_body: str, caller_body: str) -> tuple[_FreshJpegSourceProfile, str]:
    """Select one complete profile and extract its dynamic fallback literal."""
    method_matches: list[tuple[_FreshJpegSourceProfile, str]] = []
    for profile in _SOURCE_PROFILES:
        try:
            method_matches.append((profile, _method_default(method_body, profile.method_template)))
        except FreshByteOrderRefused:
            continue
    if not method_matches:
        raise FreshByteOrderRefused("SetPreferredByteOrder body is outside the admitted grammar")
    set_matches = [item for item in method_matches
                   if _normalized_body_sha256(set_body) == item[0].set_byte_order_body_sha256]
    if not set_matches:
        raise FreshByteOrderRefused("SetByteOrder body is outside the admitted grammar")
    get_matches = [item for item in set_matches
                   if _normalized_body_sha256(get_body) == item[0].get_byte_order_body_sha256]
    if not get_matches:
        raise FreshByteOrderRefused("GetByteOrder body is outside the admitted grammar")
    matches = [item for item in get_matches if _matches_caller_profile(caller_body, item[0])]
    if len(matches) != 1:
        raise FreshByteOrderRefused("DoProcessTIFF complete caller body is outside the admitted grammar")
    return matches[0]


@dataclass(frozen=True)
class FreshJpegByteOrderRecipe:
    order: str
    caller_mode: str
    set_preferred_source_sha256: str
    set_preferred_body_sha256: str
    set_byte_order_source_sha256: str
    set_byte_order_body_sha256: str
    get_byte_order_source_sha256: str
    get_byte_order_body_sha256: str
    caller_source_sha256: str
    caller_body_sha256: str
    closure_sha256: str
    writer_capture_closure_sha256: str
    writer_read_capture_closure_sha256: str
    exiftool_version: str
    perl_version: str


def _writer_capture_join(probe: Mapping[str, Any], writer_document: Mapping[str, Any]) -> tuple[str, str]:
    """Join the small executable probe to the full selected writer/read capture.

    The byte-order probe has the actual helper and caller bodies.  The full
    dump proves those source files belong to the same post-load writer context
    and to the reader-side selected-library closure used by all writer rules.
    """
    writer_document = _mapping(writer_document, "writer tables document")
    write_context = _mapping(writer_document.get("native_write_capture_context"), "writer capture context")
    if (write_context.get("kind") != "write_exif_postload_context_v1"
            or write_context.get("resolved") is not True
            or write_context.get("deparse_options") != ["-p", "-sC"]
            or write_context.get("load_errors") != []):
        raise FreshByteOrderRefused("full writer capture context is unresolved")
    write_loaded = _mapping(write_context.get("loaded_modules"), "full writer capture loaded modules")
    probe_loaded = _mapping(_mapping(probe.get("capture_context"), "capture_context").get("loaded_modules"), "probe loaded modules")
    for file in ("Image/ExifTool.pm", "Image/ExifTool/Writer.pl"):
        if write_loaded.get(file) != probe_loaded.get(file):
            raise FreshByteOrderRefused("full writer capture does not join preferred-byte-order closure")
        _sha(write_loaded.get(file), "full writer capture source")
    read_context = _mapping(writer_document.get("native_capture_context"), "reader capture context")
    closure = _mapping(read_context.get("loaded_closure"), "reader capture closure")
    if read_context.get("schema") != "native_exiftool_capture_context_v1":
        raise FreshByteOrderRefused("reader capture context is unresolved")
    read_digest = _sha(closure.get("sha256"), "reader capture closure digest")
    modules = closure.get("modules")
    if not isinstance(modules, list):
        raise FreshByteOrderRefused("reader capture closure modules are unavailable")
    read_loaded: dict[str, str] = {}
    for item in modules:
        item = _mapping(item, "reader capture closure module")
        inc = item.get("inc")
        source_file = item.get("source_file")
        source_sha = item.get("source_sha256")
        if inc != source_file:
            continue
        read_loaded[_path(inc, "reader capture closure module path")] = _sha(source_sha, "reader capture closure source")
    for file in ("Image/ExifTool.pm", "Image/ExifTool/Writer.pl"):
        if read_loaded.get(file) != probe_loaded.get(file):
            raise FreshByteOrderRefused("reader capture does not join preferred-byte-order closure")
    writer_payload = [{"file": file, "sha256": sha} for file, sha in sorted(write_loaded.items())]
    writer_digest = hashlib.sha256(json.dumps(writer_payload, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return writer_digest, read_digest


def compile_recipe(document: Mapping[str, Any], writer_document: Mapping[str, Any]) -> FreshJpegByteOrderRecipe:
    method = _fact(document, "set_preferred_byte_order", "Image::ExifTool::SetPreferredByteOrder", "Image/ExifTool/Writer.pl")
    set_byte_order = _fact(document, "set_byte_order", "Image::ExifTool::SetByteOrder", "Image/ExifTool.pm")
    get_byte_order = _fact(document, "get_byte_order", "Image::ExifTool::GetByteOrder", "Image/ExifTool.pm")
    caller = _fact(document, "new_jpeg_caller", "Image::ExifTool::DoProcessTIFF", "Image/ExifTool.pm")
    method_source = _sha(method["source_sha256"], "SetPreferredByteOrder source")
    set_byte_order_source = _sha(set_byte_order["source_sha256"], "SetByteOrder source")
    get_byte_order_source = _sha(get_byte_order["source_sha256"], "GetByteOrder source")
    caller_source = _sha(caller["source_sha256"], "DoProcessTIFF source")
    # Both unqualified helpers reached by SetPreferredByteOrder bind in the
    # core package.  Their source file identity must be the exact selected
    # core source that supplied DoProcessTIFF and the loaded-closure record.
    if set_byte_order_source != caller_source or get_byte_order_source != caller_source:
        raise FreshByteOrderRefused("byte-order helper source does not join DoProcessTIFF core source")
    _capture_context(document, method_source, caller_source)
    writer_capture_closure_sha256, writer_read_capture_closure_sha256 = _writer_capture_join(document, writer_document)
    profile, fallback = _source_profile(method["__deparse"], set_byte_order["__deparse"],
                                        get_byte_order["__deparse"], caller["__deparse"])
    observations = _mapping(document.get("observations"), "observations")
    default = _mapping(observations.get("fresh_ifd0_no_overrides"), "fresh IFD0 observation")
    if default.get("selected") != fallback or default.get("reported") != fallback:
        raise FreshByteOrderRefused("native fresh-IFD0 default does not match executable fallback")
    if fallback not in {"II", "MM"}:
        raise FreshByteOrderRefused("native fresh-IFD0 default is unsupported")
    for name in ("byte_order_option", "exif_byte_order", "maker_note_byte_order"):
        observed = _mapping(observations.get(name), f"{name} observation")
        if observed.get("selected") not in {"II", "MM"} or observed.get("reported") != observed.get("selected"):
            raise FreshByteOrderRefused(f"{name} observation is unresolved")
    return FreshJpegByteOrderRecipe(
        order=fallback,
        caller_mode=profile.caller_mode,
        set_preferred_source_sha256=method_source,
        set_preferred_body_sha256=hashlib.sha256(method["__deparse"].encode()).hexdigest(),
        set_byte_order_source_sha256=set_byte_order_source,
        set_byte_order_body_sha256=hashlib.sha256(set_byte_order["__deparse"].encode()).hexdigest(),
        get_byte_order_source_sha256=get_byte_order_source,
        get_byte_order_body_sha256=hashlib.sha256(get_byte_order["__deparse"].encode()).hexdigest(),
        caller_source_sha256=caller_source,
        caller_body_sha256=hashlib.sha256(caller["__deparse"].encode()).hexdigest(),
        closure_sha256=_sha(_mapping(document["capture_context"], "capture_context")["loaded_closure_sha256"], "closure digest"),
        writer_capture_closure_sha256=writer_capture_closure_sha256,
        writer_read_capture_closure_sha256=writer_read_capture_closure_sha256,
        exiftool_version=str(_mapping(document["capture_context"], "capture_context").get("exiftool_version")),
        perl_version=str(_mapping(document["capture_context"], "capture_context").get("perl_version")),
    )


def render_rust(recipe: FreshJpegByteOrderRecipe) -> str:
    variant = "LittleEndian" if recipe.order == "II" else "BigEndian"
    return f'''// @generated by fresh_jpeg_byte_order_codegen.py; selected native executable operands only.\n\n#[derive(Clone, Copy, Debug, Eq, PartialEq)]\npub(crate) enum FreshJpegExifByteOrder {{\n    LittleEndian,\n    BigEndian,\n}}\n\npub(crate) struct FreshJpegByteOrderRecipe {{\n    pub selected: FreshJpegExifByteOrder,\n    pub set_preferred_source_sha256: &'static str,\n    pub set_preferred_body_sha256: &'static str,\n    pub set_byte_order_source_sha256: &'static str,\n    pub set_byte_order_body_sha256: &'static str,\n    pub get_byte_order_source_sha256: &'static str,\n    pub get_byte_order_body_sha256: &'static str,\n    pub caller_source_sha256: &'static str,\n    pub caller_body_sha256: &'static str,\n    pub closure_sha256: &'static str,\n    pub writer_capture_closure_sha256: &'static str,\n    pub writer_read_capture_closure_sha256: &'static str,\n    pub exiftool_version: &'static str,\n    pub perl_version: &'static str,\n}}\n\npub(crate) struct FreshJpegByteOrderInputs<'a> {{\n    pub byte_order_option: Option<&'a str>,\n    pub exif_byte_order: Option<&'a str>,\n    pub maker_note_byte_order: Option<&'a str>,\n}}\n\npub(crate) const FRESH_JPEG_BYTE_ORDER: FreshJpegByteOrderRecipe = FreshJpegByteOrderRecipe {{\n    selected: FreshJpegExifByteOrder::{variant},\n    set_preferred_source_sha256: "{recipe.set_preferred_source_sha256}",\n    set_preferred_body_sha256: "{recipe.set_preferred_body_sha256}",\n    set_byte_order_source_sha256: "{recipe.set_byte_order_source_sha256}",\n    set_byte_order_body_sha256: "{recipe.set_byte_order_body_sha256}",\n    get_byte_order_source_sha256: "{recipe.get_byte_order_source_sha256}",\n    get_byte_order_body_sha256: "{recipe.get_byte_order_body_sha256}",\n    caller_source_sha256: "{recipe.caller_source_sha256}",\n    caller_body_sha256: "{recipe.caller_body_sha256}",\n    closure_sha256: "{recipe.closure_sha256}",\n    writer_capture_closure_sha256: "{recipe.writer_capture_closure_sha256}",\n    writer_read_capture_closure_sha256: "{recipe.writer_read_capture_closure_sha256}",\n    exiftool_version: "{recipe.exiftool_version}",\n    perl_version: "{recipe.perl_version}",\n}};\n\npub(crate) fn fresh_jpeg_byte_order(\n    recipe: &FreshJpegByteOrderRecipe,\n    inputs: FreshJpegByteOrderInputs<'_>,\n) -> Result<FreshJpegExifByteOrder, &'static str> {{\n    if inputs.byte_order_option.is_some()\n        || inputs.exif_byte_order.is_some()\n        || inputs.maker_note_byte_order.is_some()\n    {{\n        return Err("fresh JPEG byte-order override is outside the generated native branch");\n    }}\n    Ok(recipe.selected)\n}}\n'''


def generate(document: Mapping[str, Any], writer_document: Mapping[str, Any]) -> tuple[str, dict[str, Any]]:
    recipe = compile_recipe(document, writer_document)
    report = {"state": "resolved", "order": recipe.order, "recipe": recipe.__dict__,
              "writer_capture_joined": True}
    return render_rust(recipe), report


def main() -> int:
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path, help="native preferred-byte-order probe JSON")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--report", type=Path)
    parser.add_argument("--writer-tables", type=Path, required=True)
    args = parser.parse_args()
    document = json.loads(args.input.read_text(encoding="utf-8"))
    writer_document = json.loads(args.writer_tables.read_text(encoding="utf-8"))
    source, report = generate(document, writer_document)
    args.output.write_text(source, encoding="utf-8")
    if args.report is not None:
        args.report.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
