"""Compile the closed ordinary-EXIF scalar final writer stage.

The input is the raw ``dump_tables.pl`` sidecar, not a caller-supplied list of
writer actions.  This compiler authenticates the *actual final-loaded*
``Exif::Main`` WriteExif B::Deparse body, then binds each accepted row to its
raw source controls and to the separately captured TIFF format registry.  The
result is deliberately an internal resolved-edit recipe; it creates no public
writer route.

Pinned scope: ordinary ``Exif::Main`` rows whose literal ``Writable`` is in
the independently compiled scalar WriteValue formats, with no ``Format``,
fixed count, callbacks, MakerNotes, EntryBased, or CharsetEXIF recoding. All
other source shapes are omissions, not approximate recipes.
"""

from __future__ import annotations

from dataclasses import asdict, dataclass
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
from typing import Any, Mapping
from scalar_helper_codegen import rust_string
from writevalue_recipes import RecipeRefused, compile_scalar_write
import native_reader_facts


class FinalStageRefused(ValueError):
    """The supplied native source facts cannot safely enter this stage."""


_SHA = re.compile(r"[0-9a-f]{64}\Z")
_RAW_ID = re.compile(r"(?:0x[0-9a-fA-F]+|[0-9]+)\Z")
_FULL_TEMPLATE_PATHS = (
    "final_scalar_writeexif_full_template.json",
    # These complete token profiles were captured from the immutable selected
    # 11.78/12.64 source pair.  They deliberately describe the whole
    # final-loaded CV, including historical branches now absent in 13.59.
    # A version label never admits a source: only exact token consumption does.
    "final_scalar_writeexif_full_template_11_78.json",
    "final_scalar_writeexif_full_template_12_64.json",
)
_FULL_TEMPLATES = tuple(
    json.loads(Path(__file__).with_name(name).read_text(encoding="utf-8"))
    for name in _FULL_TEMPLATE_PATHS
)


# This stage deliberately does not turn a complete historical body match into
# blanket source compatibility.  It uses only the ordinary scalar terminal
# path below: undefined input/delete, WriteValue, defined-nonempty output,
# UTF-8 or disabled-CharsetEXIF encoding, the later count-ceil assignment, and
# the TIFF value-buffer append/padding path.  The full template proves no
# other executable body tokens were ignored; these operands prove that the
# executor's individual final-stage operations still have their native source
# meanings.  The patterns operate on B::Deparse with whitespace removed.
_OPERATIVE_FINAL_SCALAR_OPERANDS = (
    ("undefined/delete", "if((not(defined($newVal))or($xDelete{$newID}andnot(defined($nvHash->{'Shift'})))))"),
    ("WriteValue operands", "($newValue=WriteValue($newVal,$newFormName,$newCount));"),
    ("invalid serialized value", "unless(defined($newValue)){"),
    ("defined-empty guard", "if(length($newValue)){"),
    ("zero-length NoOverwrite", "Can'twritezerolength"),
    ("later count ceil", "(($oldInfoand$oldInfo->{'FixedSize'})or($newCount=int(((($newSize+$fsize)-1)/$fsize))));"),
    ("out-of-line guard", "if(($newSize>4)){"),
    ("even/count-width padding", "while((($newSize&1)or($newSize<($newCount*$fsize)))){"),
    ("out-of-line append", "($valBuff.=$$newValuePt);"),
)


def _authenticate_operative_final_scalar_semantics(body: str, wire_formats: set[str]) -> None:
    """Require native operations represented by the selected scalar recipes.

    ``utf8`` is a native *wire-format name*.  It is unrelated to Rust's
    ``Scalar::Utf8``, which this stage refuses because the final boundary is
    after Sanitize and therefore contains bytes.  Require the native UTF-8
    encoding branch only when a selected recipe actually has the ``utf8``
    format.  A selected ``string`` recipe instead requires its guarded
    CharsetEXIF branch; default disabled CharsetEXIF leaves those bytes
    untouched, exactly as the final executor requires.
    """
    compact = re.sub(r"\s+", "", body)
    positions = []
    for operand, source in _OPERATIVE_FINAL_SCALAR_OPERANDS:
        position = compact.find(source)
        if position < 0:
            raise FinalStageRefused(
                f"WriteExif operative final-scalar {operand} is outside the generated executor"
            )
        positions.append(position)

    if "string" in wire_formats:
        charset_guard = "($strEncand($newFormNameeq'string'))"
        charset_encode = "($newValue=$et->Encode($newValue,$strEnc));"
        charset_position = compact.find(charset_guard)
        charset_encode_position = compact.find(charset_encode, charset_position)
        if charset_position < 0 or charset_encode_position < charset_position:
            raise FinalStageRefused(
                "WriteExif operative final-scalar CharsetEXIF string guard is outside the generated executor"
            )
        encoding_positions = [charset_position, charset_encode_position]
    else:
        encoding_positions = []

    if "utf8" in wire_formats:
        utf8 = "if(($newFormNameeq'utf8')){($newValue=$et->Encode($newValue,'UTF8'));}"
        utf8_position = compact.find(utf8)
        if utf8_position < 0:
            raise FinalStageRefused(
                "WriteExif operative final-scalar UTF-8 wire-format encoding is outside the generated executor"
            )
        encoding_positions.append(utf8_position)

    # The generic terminal path must remain in source order.  Encoding branches
    # sit after the nonempty guard (index 3) and before the zero-length branch
    # (index 4), not at the tail after count/padding/append.
    if positions != sorted(positions) or any(
        position <= positions[3] or position >= positions[4]
        for position in encoding_positions
    ) or encoding_positions != sorted(encoding_positions):
        raise FinalStageRefused("WriteExif operative final-scalar branch order is outside the generated executor")


def _authenticate_capture_context(document: Mapping[str, Any]) -> None:
    context = _mapping(document.get("native_write_capture_context"), "native_write_capture_context")
    if (context.get("kind") != "write_exif_postload_context_v1"
            or context.get("resolved") is not True or context.get("deparse_options") != ["-p", "-sC"]
            or context.get("load_errors") != []):
        raise FinalStageRefused("WriteExif capture context is unresolved or unsupported")
    loaded = _mapping(context.get("loaded_modules"), "WriteExif capture context loaded modules")
    if "Image/ExifTool.pm" not in loaded or "Image/ExifTool/WriteExif.pl" not in loaded:
        raise FinalStageRefused("WriteExif capture context lacks loaded native core/writer")
    for file, sha in loaded.items():
        file = _relative(file, "WriteExif loaded context module")
        if file != "Image/ExifTool.pm" and not file.startswith("Image/ExifTool/"):
            raise FinalStageRefused("WriteExif loaded context module is outside native namespace")
        _sha(sha, "WriteExif loaded context source")
    modules = context.get("modules")
    expected = ["Image/ExifTool/Canon.pm", "Image/ExifTool/PanasonicRaw.pm", "Image/ExifTool/Sony.pm"]
    if not isinstance(modules, list) or len(modules) != len(expected):
        raise FinalStageRefused("WriteExif capture context modules are incomplete")
    for module, name in zip(modules, expected):
        module = _mapping(module, "WriteExif capture context module")
        if module.get("file") != name or module.get("resolved") is not True:
            raise FinalStageRefused("WriteExif capture context module is unresolved or reordered")
        _sha(module.get("source_sha256"), "WriteExif capture context source")
        if loaded.get(name) != module["source_sha256"]:
            raise FinalStageRefused("WriteExif capture context module differs from loaded closure")


@dataclass(frozen=True)
class FinalScalarRecipe:
    module: str
    table: str
    full_name: str
    raw_tag_id: int
    name: str
    table_group0: str
    physical_write_group: str
    conversion_format: str
    wire_format: str
    count_rule: str
    undefined_rule: str
    source_control_sha256: str
    write_proc_body_sha256: str
    write_proc_source_sha256: str
    registry_source_sha256: str
    writer_source_sha256: str
    main_source_sha256: str


@dataclass(frozen=True)
class NativeFormatRegistryArtifact:
    source_file: str
    source_sha256: str
    canonical_facts: tuple[tuple[str, int, int], ...]
    aliases: tuple[tuple[str, int], ...]


def _mapping(value: Any, context: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise FinalStageRefused(f"{context} is not an object")
    return value


def _fact(value: Any, context: str) -> tuple[bool, Any]:
    value = _mapping(value, context)
    present = value.get("present")
    if not isinstance(present, bool):
        raise FinalStageRefused(f"{context}.present is not a boolean")
    if present != ("value" in value):
        raise FinalStageRefused(f"{context} does not preserve absent versus present value")
    return present, value.get("value")


def _relative(value: Any, context: str) -> str:
    if not isinstance(value, str) or not value or "\\" in value:
        raise FinalStageRefused(f"{context} is not a canonical relative path")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in ("", ".", "..") for part in path.parts):
        raise FinalStageRefused(f"{context} is not a canonical relative path")
    return value


def _sha(value: Any, context: str) -> str:
    if not isinstance(value, str) or _SHA.fullmatch(value) is None:
        raise FinalStageRefused(f"{context} is not a SHA-256 digest")
    return value


def _property(properties: Mapping[str, Any], name: str, context: str) -> tuple[bool, Any]:
    return _fact(properties.get(name, {"present": False}), f"{context}.{name}")


def _same_projection(row: Mapping[str, Any], name: str) -> tuple[bool, Any]:
    properties = _mapping(row.get("properties"), "row.properties")
    controls = _mapping(row.get("write_controls"), "row.write_controls")
    source = _property(properties, name, "row.properties")
    projected = _property(controls, name, "row.write_controls")
    if source != projected:
        raise FinalStageRefused(f"row {name} source/control projections differ")
    return source


def _authenticate_write_exif(table: Mapping[str, Any]) -> tuple[str, str, str]:
    wrapper = _mapping(table.get("effective_write_proc"), "effective_write_proc")
    present = wrapper.get("present")
    if present is not True:
        raise FinalStageRefused("effective WriteExif procedure is absent")
    fact = _mapping(wrapper.get("effective"), "effective_write_proc.effective")
    if fact.get("resolved") is not True or fact.get("__perl") != "CODE":
        raise FinalStageRefused("effective WriteExif procedure is unresolved")
    if fact.get("__name") != "Image::ExifTool::Exif::WriteExif":
        raise FinalStageRefused("effective procedure is not Exif::WriteExif")
    if _relative(fact.get("source_file"), "WriteExif.source_file") != "Image/ExifTool/WriteExif.pl":
        raise FinalStageRefused("WriteExif source file is outside selected native path")
    source_sha = _sha(fact.get("source_sha256"), "WriteExif.source_sha256")
    body = fact.get("__deparse")
    if not isinstance(body, str):
        raise FinalStageRefused("WriteExif body is unavailable")
    try:
        tokens = native_reader_facts.body_tokens(body)
    except native_reader_facts.ReaderRefused as error:
        raise FinalStageRefused("WriteExif body cannot be tokenized") from error
    # The committed template covers every deparsed token, including branches
    # not selected by the ordinary scalar row. No names, body hashes, or
    # substring anchors grant execution: every inserted/reordered action leaves
    # an unconsumed token and refuses. Whitespace/comments are intentionally
    # absent from body_tokens.
    if not any(tokens == template for template in _FULL_TEMPLATES):
        raise FinalStageRefused("WriteExif body is outside the complete final-stage token grammar")
    control_sha = hashlib.sha256(json.dumps(tokens, separators=(",", ":")).encode("utf-8")).hexdigest()
    return hashlib.sha256(body.encode("utf-8")).hexdigest(), source_sha, control_sha


def _authenticate_check_exif(table: Mapping[str, Any], write_source_sha: str) -> None:
    wrapper = _mapping(table.get("effective_check_proc"), "effective_check_proc")
    fact = _mapping(wrapper.get("effective"), "effective_check_proc.effective")
    if wrapper.get("present") is not True or fact.get("resolved") is not True:
        raise FinalStageRefused("effective CheckExif procedure is unresolved")
    if fact.get("__name") != "Image::ExifTool::Exif::CheckExif":
        raise FinalStageRefused("effective procedure is not Exif::CheckExif")
    if _relative(fact.get("source_file"), "CheckExif.source_file") != "Image/ExifTool/WriteExif.pl":
        raise FinalStageRefused("CheckExif source file is unsupported")
    if _sha(fact.get("source_sha256"), "CheckExif.source_sha256") != write_source_sha:
        raise FinalStageRefused("CheckExif and WriteExif source identities differ")


def _authenticate_registry(document: Mapping[str, Any]) -> NativeFormatRegistryArtifact:
    registry = _mapping(document.get("native_write_format_registry"), "native_write_format_registry")
    if registry.get("state") != "resolved":
        raise FinalStageRefused("native TIFF format registry is unresolved")
    source = _mapping(registry.get("source"), "native_write_format_registry.source")
    if _relative(source.get("library_relative_path"), "registry.source") != "Image/ExifTool/Exif.pm":
        raise FinalStageRefused("native TIFF registry source is unsupported")
    source_sha = _sha(source.get("sha256"), "registry.source.sha256")
    names = registry.get("format_name")
    sizes = registry.get("format_size")
    numbers = _mapping(registry.get("format_number"), "native_write_format_registry.format_number")
    if not isinstance(names, list) or not isinstance(sizes, list) or len(names) != len(sizes):
        raise FinalStageRefused("native TIFF registry sparse arrays are malformed")
    if any(value is not None and not isinstance(value, str) for value in names):
        raise FinalStageRefused("native TIFF registry has non-string format name")
    if any(value is not None and (type(value) is not int or not 0 < value <= 0xffffffff) for value in sizes):
        raise FinalStageRefused("native TIFF registry has invalid format size")
    facts = []
    for number, (name, size) in enumerate(zip(names, sizes)):
        if name is None and size is None:
            continue
        if not isinstance(name, str) or type(size) is not int or not 0 < number <= 0xffff:
            raise FinalStageRefused("native TIFF registry canonical slot is malformed")
        if numbers.get(name) != number:
            raise FinalStageRefused("native TIFF registry canonical name does not round-trip")
        facts.append((name, number, size))
    aliases = []
    for name, number in numbers.items():
        if not isinstance(name, str) or type(number) is not int or not 0 < number <= 0xffff:
            raise FinalStageRefused("native TIFF registry alias is malformed")
        if number >= len(names) or not isinstance(names[number], str) or type(sizes[number]) is not int:
            raise FinalStageRefused("native TIFF registry alias lacks canonical target")
        aliases.append((name, number))
    return NativeFormatRegistryArtifact("Image/ExifTool/Exif.pm", source_sha, tuple(facts), tuple(sorted(aliases)))


def _registry_format(name: str, registry: NativeFormatRegistryArtifact) -> None:
    matching = [fact for fact in registry.canonical_facts if fact[0] == name]
    if len(matching) != 1:
        raise FinalStageRefused("native TIFF registry does not map selected format to a valid type")


def _admitted_row(raw_id: str, row: Mapping[str, Any], supported_formats: set[str]) -> tuple[int, str, str]:
    if row.get("entry_kind") != "HASH" or _RAW_ID.fullmatch(raw_id) is None:
        raise FinalStageRefused("row is not a physical HASH u16 entry")
    numeric_id = int(raw_id, 0)
    if not 0 <= numeric_id <= 0xffff:
        raise FinalStageRefused("row id is outside u16")
    properties = _mapping(row.get("properties"), "row.properties")
    unknown = _mapping(row.get("unknown_properties"), "row.unknown_properties")
    data_member_present, _ = _property(properties, "DataMember", "row.properties")
    if data_member_present:
        raise FinalStageRefused("row DataMember activates a RawConv branch outside this specialization")
    basic = {"Name", "Writable", "WriteGroup"}
    metadata_only = {"Groups", "Notes"}
    inert_without_data_member = {"RawConv"}
    numeric = properties.get("Writable", {}).get("value") in {"int16u", "rational64u"}
    allowed = basic | metadata_only | inert_without_data_member
    if numeric:
        allowed |= {"Mandatory", "Priority"}
    if not basic.issubset(properties) or set(properties) - allowed or unknown:
        raise FinalStageRefused("row has unmodeled final-stage properties")
    # Groups and Notes describe the tag to ExifTool's metadata layer. They
    # are not consulted by the authenticated WriteExif specialization below.
    groups_present, groups = _property(properties, "Groups", "row.properties")
    if groups_present and (not isinstance(groups, Mapping)
                           or any(not isinstance(key, str) or not isinstance(value, str)
                                  for key, value in groups.items())):
        raise FinalStageRefused("row Groups metadata is malformed")
    notes_present, notes = _property(properties, "Notes", "row.properties")
    if notes_present and not isinstance(notes, str):
        raise FinalStageRefused("row Notes metadata is malformed")
    raw_conv_present, _ = _property(properties, "RawConv", "row.properties")
    # The complete, immutable WriteExif body authenticated by
    # _authenticate_write_exif reaches RawConv only in its DataMember branch
    # (WriteExif.pl:1933-1959). DataMember's proven absence makes this table
    # metadata inert for this resolved edit path.
    if raw_conv_present and data_member_present:
        raise FinalStageRefused("row RawConv is active through DataMember")
    # These declarations do not convert explicit values. Priority affects
    # source lookup, which is separately authenticated; mandatory cleanup and
    # numeric conversion remain independently runtime-guarded.
    for key, expected in (("Mandatory", "1"), ("Priority", "0")):
        if key in properties and ( _same_projection(row, key) if key == "Mandatory" else _property(properties, key, "row.properties")) != (True, expected):
            raise FinalStageRefused("numeric row control is outside the supported specialization: " + key)
    name_present, name = _property(properties, "Name", "row.properties")
    writable_present, writable = _same_projection(row, "Writable")
    group_present, group = _same_projection(row, "WriteGroup")
    if not name_present or not isinstance(name, str) or not writable_present or writable not in supported_formats:
        raise FinalStageRefused("row Writable format is outside generated scalar WriteValue formats")
    if not group_present or not isinstance(group, str):
        raise FinalStageRefused("row has no literal physical WriteGroup")
    if group not in {"IFD0", "ExifIFD"}:
        raise FinalStageRefused("row physical WriteGroup is outside ordinary TIFF directories")
    controls = _mapping(row.get("write_controls"), "row.write_controls")
    if any(key not in {"Writable", "WriteGroup", "Mandatory"} and _property(controls, key, "row.write_controls")[0] for key in controls):
        raise FinalStageRefused("row has callback, count, condition, or overwrite control")
    # `Format` and FixedSize are properties rather than the dumper's compact
    # controls list, so the exact property set above proves they are absent.
    return numeric_id, name, group


def compile_final_scalar_stage(document: Mapping[str, Any]) -> tuple[list[FinalScalarRecipe], list[dict[str, str]], NativeFormatRegistryArtifact]:
    document = _mapping(document, "document")
    _authenticate_capture_context(document)
    registry = _authenticate_registry(document)
    try:
        scalar_write = compile_scalar_write(_mapping(_mapping(document.get("native_write_helpers"), "native_write_helpers").get("write_value"), "native_write_helpers.write_value"))
    except RecipeRefused as error:
        raise FinalStageRefused(f"generated scalar WriteValue dependency refused: {error}") from error
    supported_formats = set(scalar_write.formats)
    from numeric_scalar import compile_numeric_scalar
    try:
        numeric = compile_numeric_scalar(document)
        supported_formats.update(numeric.formats)
    except RecipeRefused:
        # Preserve the existing scalar route; omitted numeric rows stay
        # explicit in the per-row report and public ownership ledger.
        pass
    tables = _mapping(document.get("native_write_tables"), "native_write_tables")
    exif = _mapping(tables.get("Exif"), "native_write_tables.Exif")
    table = _mapping(exif.get("Main"), "native_write_tables.Exif.Main")
    if (table.get("module"), table.get("table"), table.get("full_name")) != ("Exif", "Main", "Image::ExifTool::Exif::Main"):
        raise FinalStageRefused("source table identity is not Exif::Main")
    write_body_sha, write_source_sha, control_sha = _authenticate_write_exif(table)
    _authenticate_check_exif(table, write_source_sha)
    loaded = document["native_write_capture_context"]["loaded_modules"]
    for file, sha in (("Image/ExifTool/WriteExif.pl", write_source_sha),
                      (scalar_write.provenance.source_file, scalar_write.provenance.source_sha256),
                      (registry.source_file, registry.source_sha256)):
        if loaded.get(file) != sha:
            raise FinalStageRefused(f"compiled source differs from loaded capture context: {file}")
    props = _mapping(table.get("table_properties"), "table.table_properties")
    groups_present, groups = _property(props, "GROUPS", "table.table_properties")
    if not groups_present or groups != {"0": "EXIF", "1": "IFD0", "2": "Image"}:
        raise FinalStageRefused("Exif::Main effective groups are not the admitted ordinary EXIF group")
    rows = _mapping(table.get("rows"), "table.rows")
    recipes, omissions = [], []
    for raw_id, candidate in sorted(rows.items(), key=lambda item: int(item[0], 0) if _RAW_ID.fullmatch(item[0]) else 1 << 32):
        try:
            tag_id, name, group = _admitted_row(raw_id, _mapping(candidate, f"row[{raw_id}]"), supported_formats)
            _registry_format(candidate["properties"]["Writable"]["value"], registry)
        except FinalStageRefused as error:
            omissions.append({"raw_id": raw_id, "reason": str(error)})
            continue
        recipes.append(FinalScalarRecipe("Exif", "Main", "Image::ExifTool::Exif::Main", tag_id, name, "EXIF", group,
                                         candidate["properties"]["Writable"]["value"], candidate["properties"]["Writable"]["value"], "CeilDivision", "DeleteEntry", control_sha, write_body_sha,
                                         write_source_sha, registry.source_sha256, scalar_write.provenance.source_sha256, loaded["Image/ExifTool.pm"]))
    # The encoding predicate is selected from the rows that survived every
    # source-row/WriteValue/registry guard above.  Do not require a native
    # wire-format branch that no emitted recipe can invoke.
    write_body = _mapping(_mapping(table.get("effective_write_proc"), "effective_write_proc").get("effective"), "effective_write_proc.effective").get("__deparse")
    if not isinstance(write_body, str):
        raise FinalStageRefused("WriteExif body is unavailable")
    _authenticate_operative_final_scalar_semantics(write_body, {recipe.wire_format for recipe in recipes})
    return recipes, omissions, registry


def compile_final_scalar_rows(document: Mapping[str, Any]) -> list[FinalScalarRecipe]:
    """Compatibility wrapper for callers that only need emitted candidates.

    Consumers that publish a report must use :func:`compile_final_scalar_stage`
    so every withheld row remains visible.
    """
    return compile_final_scalar_stage(document)[0]


def render_rust(recipes: list[FinalScalarRecipe], registry: NativeFormatRegistryArtifact | None) -> str:
    """Render static operands only; the public write route remains unconnected."""
    lines = ["// @generated by final_scalar_stage.py; no public writer route.\n",
             "use crate::writers::tiff_scalar_final_stage::{NativeTiffFormatAliasFact, NativeTiffFormatFact, NativeTiffFormatRegistry, TiffScalarFinalStageRecipe};\n",
             "pub(crate) const TIFF_SCALAR_FINAL_FORMAT_FACTS: &[NativeTiffFormatFact] = &[\n"]
    esc = rust_string
    facts = () if registry is None else registry.canonical_facts
    aliases = () if registry is None else registry.aliases
    lines.extend("NativeTiffFormatFact { name: %s, number: %d, size: %d },\n" % (esc(name), number, size)
                 for name, number, size in facts)
    lines.extend(["];\n", "pub(crate) const TIFF_SCALAR_FINAL_FORMAT_ALIASES: &[NativeTiffFormatAliasFact] = &[\n"])
    lines.extend("NativeTiffFormatAliasFact { name: %s, number: %d },\n" % (esc(name), number)
                 for name, number in aliases)
    registry_literal = "None" if registry is None else "Some(NativeTiffFormatRegistry { source_file: %s, source_sha256: %s, facts: TIFF_SCALAR_FINAL_FORMAT_FACTS, aliases: TIFF_SCALAR_FINAL_FORMAT_ALIASES })" % (esc(registry.source_file), esc(registry.source_sha256))
    lines.extend(["];\n", "pub(crate) const TIFF_SCALAR_FINAL_FORMAT_REGISTRY: Option<NativeTiffFormatRegistry> = %s;\n" % registry_literal,
             "pub(crate) const TIFF_SCALAR_FINAL_RECIPES: &[TiffScalarFinalStageRecipe] = &[\n"])
    for recipe in recipes:
        lines.append("TiffScalarFinalStageRecipe { module: %s, table: %s, full_name: %s, raw_tag_id: 0x%04x, tag_name: %s, table_group0: %s, physical_write_group: %s, conversion_format: %s, wire_format: %s, write_value: crate::writers::generated_scalar_rules::WRITE_VALUE.as_ref(), count_rule: crate::writers::tiff_scalar_final_stage::NativeCountRule::%s, undefined_rule: crate::writers::tiff_scalar_final_stage::NativeUndefinedRule::%s, source_control_sha256: %s, write_proc_source_sha256: %s, registry_source_sha256: %s, writer_source_sha256: %s, main_source_sha256: %s },\n" % (esc(recipe.module), esc(recipe.table), esc(recipe.full_name), recipe.raw_tag_id, esc(recipe.name), esc(recipe.table_group0), esc(recipe.physical_write_group), esc(recipe.conversion_format), esc(recipe.wire_format), recipe.count_rule, recipe.undefined_rule, esc(recipe.source_control_sha256), esc(recipe.write_proc_source_sha256), esc(recipe.registry_source_sha256), esc(recipe.writer_source_sha256), esc(recipe.main_source_sha256)))
    lines.append("];\n")
    return "".join(lines)


def generate(document: dict[str, Any]) -> tuple[str, dict[str, Any]]:
    """Generate final-stage operands or a visible, non-executable omission."""
    if not isinstance(document, dict):
        raise TypeError("final scalar stage document is not an object")
    report = {"exiftool_version": document.get("exiftool_version"),
              "capture_context": document.get("native_write_capture_context"),
              "runtime_status": "internal final scalar stage; public writer not connected"}
    try:
        recipes, omissions, registry = compile_final_scalar_stage(document)
    except FinalStageRefused as error:
        return render_rust([], None), {**report, "emitted": False, "reason": str(error), "recipes": [], "omitted_rows": [], "registry": None}
    return render_rust(recipes, registry), {**report, "emitted": bool(recipes), "reason": None, "recipes": [asdict(recipe) for recipe in recipes], "omitted_rows": omissions, "registry": asdict(registry)}


def main() -> None:
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tables", type=Path)
    parser.add_argument("-o", "--output", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    source, report = generate(json.loads(args.tables.read_text(encoding="utf-8")))
    args.output.write_text(source, encoding="utf-8")
    args.report.write_text(json.dumps(report, sort_keys=True, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
