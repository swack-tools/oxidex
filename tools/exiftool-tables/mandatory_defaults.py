"""Closed compiler for WriteExif lexical mandatory-directory defaults.

The defaults themselves come from the final lexical ``%mandatory`` map.  The
only supported control flow is WriteExif's source fragment which selects that
map by directory, optionally clones IFD0 and substitutes three JFIF operands,
then adds missing entries for a new directory.  Unsupported source refuses.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass, replace
import hashlib
import re
from typing import Any, Mapping


class MandatoryRefused(ValueError): pass
class MandatoryMalformed(ValueError): pass

_SHA = re.compile(r"[0-9a-f]{64}\Z")
# Reviewed whole WriteExif implementation guard: a source refresh must receive an
# explicit grammar review before mandatory-cleanup eligibility can reappear.
_REVIEWED_WRITE_EXIF_SHA256 = "7ea2e8af8f17ebfef5979146bdc6b793392f39da9fc3ea15c6ba0326e1321533"
_ID = re.compile(r"(?:0|[1-9][0-9]*)\Z")
_CONTEXT = re.compile(
    r"\Amy\(\$mandatory, \$allMandatory, \$addMandatory\); "
    r"\(\$noMandatory or \(\$mandatory = \$mandatory\{\$dirName\}\)\); if \(\$mandatory\) \{ "
    r"if \(\(\(\$dirName eq 'IFD0'\) and defined\(\$et->\{'(?P<probe>JFIFYResolution)'\}\)\)\) \{ "
    r"\(my\(%ifd0Vals\) = %\$mandatory\); "
    r"\(\$ifd0Vals\{'(?P<x>[0-9]+)'\} = \$et->\{'(?P<xprop>JFIFXResolution)'\}\); "
    r"\(\$ifd0Vals\{'(?P<y>[0-9]+)'\} = \$et->\{'(?P<yprop>JFIFYResolution)'\}\); "
    r"\(\$ifd0Vals\{'(?P<unit>[0-9]+)'\} = \(\$et->\{'(?P<unitprop>JFIFResolutionUnit)'\} \+ 1\)\); "
    r"\(\$mandatory = \(\\%ifd0Vals\)\); \} \(\$allMandatory = \(\$addMandatory = 0\)\); "
    r"unless \(\$numEntries\) \{ foreach \$_ \(keys\(%\$mandatory\)\) \{ "
    r"\(defined\(\$set\{\$_\}\) or \(\$set\{\$_\} = \$tagTablePtr->\{\$_\}\)\); \} \} "
    r"\} else \{ \$deleteAll = undef; \} my\(\$addDirs, \@newTags\);\Z"
)
_LEGACY_CONTEXT = re.compile(
    r"\A\(my\(\$mandatory\) = \$mandatory\{\$dirName\}\); my\(\$allMandatory, \$addMandatory\); if \(\$mandatory\) \{ "
    r"if \(\(\(\$dirName eq 'IFD0'\) and defined\(\$et->\{'(?P<probe>JFIFYResolution)'\}\)\)\) \{ "
    r"\(my\(%ifd0Vals\) = %\$mandatory\); "
    r"\(\$ifd0Vals\{'(?P<x>[0-9]+)'\} = \$et->\{'(?P<xprop>JFIFXResolution)'\}\); "
    r"\(\$ifd0Vals\{'(?P<y>[0-9]+)'\} = \$et->\{'(?P<yprop>JFIFYResolution)'\}\); "
    r"\(\$ifd0Vals\{'(?P<unit>[0-9]+)'\} = \(\$et->\{'(?P<unitprop>JFIFResolutionUnit)'\} \+ 1\)\); "
    r"\(\$mandatory = \(\\%ifd0Vals\)\); \} \(\$allMandatory = \(\$addMandatory = 0\)\); "
    r"unless \(\$numEntries\) \{ foreach \$_ \(keys\(%\$mandatory\)\) \{ "
    r"\(defined\(\$set\{\$_\}\) or \(\$set\{\$_\} = \$tagTablePtr->\{\$_\}\)\); \} \} "
    r"\} else \{ \$deleteAll = undef; \} my\(\$addDirs, \@newTags\);\Z"
)

_CLEANUP_CONTEXT = re.compile(
    r"\Aif \(\$allMandatory and not \$isNextIFD and \(\$newEntries < \$numEntries or \$numEntries == 0\)\) \{ "
    r"\$newEntries = 0; \$dirBuff = ''; \$valBuff = ''; undef \$dirFixup; # no fixups in this directory "
    r"\+\+\$deleteAll if defined \$deleteAll; \$verbose > 1 and print \$out \" - \$allMandatory mandatory tag\(s\)\\n\"; "
    r"\$\$et\{CHANGED\} -= \$addMandatory; # didn't change these after all \} "
    r"if \(\$ifd and not \$newEntries\) \{ \$verbose and print \$out \" Deleting IFD1\\n\"; "
    r"last; # don't write IFD1 if empty \}\Z"
)

@dataclass(frozen=True)
class Default: tag_id: int; kind: str; value: str | int
@dataclass(frozen=True)
class DirectoryDefaults: directory: str; defaults: tuple[Default, ...]
@dataclass(frozen=True)
class JfifOverride: directory: str; probe: str; assignments: tuple[tuple[int, str, int], ...]
@dataclass(frozen=True)
class MandatorySelection:
    directory_operand: str
    disabled_when: str | None
    new_directory_when: str
@dataclass(frozen=True)
class MandatoryCleanup:
    all_mandatory: bool
    no_next_ifd: bool
    entry_count_shrinks_or_new: bool
    omit_empty_ifd1: bool
@dataclass(frozen=True)
class MandatoryRecipe:
    writer_source_file: str
    writer_source_sha256: str
    core_source_sha256: str
    exif_source_sha256: str
    directories: tuple[DirectoryDefaults, ...]
    jfif_override: JfifOverride
    selection: MandatorySelection
    cleanup: MandatoryCleanup
    encodings: tuple["DefaultEncoding", ...] = ()
    unencoded_numeric_defaults: tuple["DefaultEncodingOmission", ...] = ()
    write_value_source_sha256: str = ""
    survivor_encodings: tuple["SurvivorEncoding", ...] = ()

@dataclass(frozen=True)
class SurvivorEncoding:
    format_name: str
    tiff_type: int
    width: int
    operation: str

@dataclass(frozen=True)
class DefaultEncoding:
    """A direct `WriteValue` operand for a generated mandatory entry."""
    tag_id: int
    format_name: str
    tiff_type: int


@dataclass(frozen=True)
class DefaultEncodingOmission:
    """A numeric lexical default lacking a captured direct scalar entry path.

    The full mandatory map includes directories whose rows are not represented
    by the captured Exif/Main direct-entry controls.  They remain in the
    source-derived defaults recipe, but must not acquire a guessed encoder.
    """
    directory: str
    tag_id: int
    reason: str

def _mapping(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping): raise MandatoryMalformed(f"{label} is not an object")
    return value

def _source_context(value: Any) -> tuple[JfifOverride, MandatorySelection]:
    if not isinstance(value, str): raise MandatoryMalformed("new_directory_context_deparse is not text")
    normalized = re.sub(r"\s+", " ", value).strip()
    match = _CONTEXT.fullmatch(normalized)
    no_mandatory = "noMandatory"
    if not match:
        match = _LEGACY_CONTEXT.fullmatch(normalized)
        no_mandatory = None
    if not match: raise MandatoryRefused("mandatory executable new-directory flow is outside the closed grammar")
    groups = match.groupdict()
    return JfifOverride("IFD0", groups["probe"], (
        (int(groups["x"]), groups["xprop"], 0),
        (int(groups["y"]), groups["yprop"], 0),
        (int(groups["unit"]), groups["unitprop"], 1),
    )), MandatorySelection("dirName", no_mandatory, "numEntries == 0")

_CLASSIFIER = "while (defined $allMandatory) { if (defined $$mandatory{$newID}) { # values must correspond to mandatory values my $form = $$newInfo{Format} || $newFormName; my $mandVal = WriteValue($$mandatory{$newID}, $form, $newCount); if (defined $mandVal and $mandVal eq $$newValuePt) { ++$allMandatory; # count mandatory tags last; } } undef $deleteAll; undef $allMandatory; } } if (%validateInfo) {"

def _classifier_policy(fact: Mapping[str, Any]) -> None:
    source, digest = fact.get("mandatory_classifier_source"), fact.get("mandatory_classifier_source_sha256")
    if not isinstance(source, str) or not isinstance(digest, str) or not _SHA.fullmatch(digest):
        raise MandatoryMalformed("mandatory classifier provenance is unavailable")
    if hashlib.sha256(source.encode()).hexdigest() != digest:
        raise MandatoryRefused("mandatory classifier source digest differs")
    if re.sub(r"\s+", " ", source).strip() != _CLASSIFIER:
        raise MandatoryRefused("mandatory classifier is outside the closed grammar")

def _cleanup_policy(fact: Mapping[str, Any]) -> MandatoryCleanup:
    source = fact.get("mandatory_cleanup_source")
    digest = fact.get("mandatory_cleanup_source_sha256")
    if not isinstance(source, str) or not isinstance(digest, str) or not _SHA.fullmatch(digest):
        raise MandatoryMalformed("mandatory cleanup source provenance is unavailable")
    if hashlib.sha256(source.encode()).hexdigest() != digest:
        raise MandatoryRefused("mandatory cleanup source digest differs")
    normalized = re.sub(r"\s+", " ", source).strip()
    if not _CLEANUP_CONTEXT.fullmatch(normalized):
        raise MandatoryRefused("mandatory cleanup source is outside the closed grammar")
    return MandatoryCleanup(True, True, True, True)

def compile_mandatory(fact: Mapping[str, Any]) -> MandatoryRecipe:
    fact = _mapping(fact, "mandatory fact")
    if fact.get("schema") != 1 or fact.get("kind") != "oxidex_exif_mandatory_defaults_fact":
        raise MandatoryMalformed("mandatory fact schema/kind is unsupported")
    writer = _mapping(fact.get("writer"), "writer")
    native = _mapping(fact.get("native_identity"), "native_identity")
    if (not isinstance(native.get("exiftool_version"), str) or not native["exiftool_version"]
            or not isinstance(native.get("perl"), str) or not native["perl"]
            or not isinstance(native.get("perl_version"), str) or not native["perl_version"]):
        raise MandatoryMalformed("native interpreter/release identity is unavailable")
    if writer.get("requested_binding") != "Image::ExifTool::Exif::WriteExif" or writer.get("actual_name") != "Image::ExifTool::Exif::WriteExif":
        raise MandatoryRefused("final WriteExif binding is not the selected native writer")
    source = writer.get("source_file"); digest = writer.get("source_sha256")
    if source != "Image/ExifTool/WriteExif.pl" or writer.get("cv_file") != source or not isinstance(digest, str) or not _SHA.fullmatch(digest):
        raise MandatoryMalformed("WriteExif source identity is unavailable")
    if digest != _REVIEWED_WRITE_EXIF_SHA256:
        raise MandatoryRefused("WriteExif whole-body review hash is unrecognized")
    closure = _mapping(fact.get("loaded_exiftool_closure"), "loaded_exiftool_closure")
    if not closure or closure.get(source) != digest or any(not isinstance(k, str) or not k.startswith("Image/ExifTool") or not isinstance(v, str) or not _SHA.fullmatch(v) for k, v in closure.items()):
        raise MandatoryMalformed("selected native module closure is unavailable")
    # The CV, lexical binding, format table and writer helper must all come
    # from the selected final-loaded native closure.  This prevents a sidecar
    # from combining a valid WriteExif map with ambient core/helper modules.
    required = {"Image/ExifTool.pm", "Image/ExifTool/Writer.pl", "Image/ExifTool/Exif.pm", source}
    if not required.issubset(closure):
        raise MandatoryMalformed("selected native helper/source closure is incomplete")
    lexical = _mapping(fact.get("lexical"), "lexical")
    if lexical.get("name") != "%mandatory" or lexical.get("resolved") is not True:
        raise MandatoryRefused("mandatory lexical map is unresolved")
    raw = _mapping(lexical.get("entries"), "mandatory entries")
    directories = []
    for directory, values in sorted(raw.items()):
        if not isinstance(directory, str) or not directory: raise MandatoryMalformed("mandatory directory is invalid")
        values = _mapping(values, f"mandatory[{directory}]")
        defaults = []
        for tag_id, value in sorted(values.items(), key=lambda item: int(item[0]) if isinstance(item[0], str) and _ID.fullmatch(item[0]) else -1):
            if not isinstance(tag_id, str) or not _ID.fullmatch(tag_id) or int(tag_id) > 65535:
                raise MandatoryRefused("mandatory map contains an unrepresentable tag id")
            if type(value) is int: defaults.append(Default(int(tag_id), "Integer", value))
            elif isinstance(value, str): defaults.append(Default(int(tag_id), "Text", value))
            else: raise MandatoryRefused("mandatory map contains an unsupported default operand")
        directories.append(DirectoryDefaults(directory, tuple(defaults)))
    if not directories: raise MandatoryRefused("mandatory map is empty")
    override, selection = _source_context(fact.get("new_directory_context_deparse"))
    _classifier_policy(fact)
    cleanup = _cleanup_policy(fact)
    return MandatoryRecipe(str(source), digest, str(closure["Image/ExifTool.pm"]), str(closure["Image/ExifTool/Exif.pm"]), tuple(directories), override, selection, cleanup)

def compile_mandatory_joined(fact: Mapping[str, Any], document: Mapping[str, Any]) -> MandatoryRecipe:
    """Require defaults and the general captured WriteExif callback share a source identity."""
    recipe = compile_mandatory(fact)
    mandatory_closure = _mapping(fact.get("loaded_exiftool_closure"), "loaded_exiftool_closure")
    document = _mapping(document, "general writer document")
    if document.get("exiftool_version") != fact["native_identity"]["exiftool_version"]:
        raise MandatoryRefused("mandatory defaults do not join the selected native release")
    tables = _mapping(document.get("native_write_tables"), "native_write_tables")
    exif = _mapping(tables.get("Exif"), "native_write_tables.Exif")
    main = _mapping(exif.get("Main"), "native_write_tables.Exif.Main")
    procedure = _mapping(main.get("effective_write_proc"), "effective_write_proc")
    effective = _mapping(procedure.get("effective"), "effective_write_proc.effective")
    writer = _mapping(fact.get("writer"), "writer")
    if (effective.get("__name") != writer["actual_name"]
            or effective.get("source_file") != recipe.writer_source_file
            or effective.get("source_sha256") != recipe.writer_source_sha256):
        raise MandatoryRefused("mandatory defaults do not join the general captured WriteExif")
    helpers = _mapping(document.get("native_write_helpers"), "native_write_helpers")
    write_value = _mapping(helpers.get("write_value"), "native_write_helpers.write_value")
    if (write_value.get("requested_binding") != "Image::ExifTool::WriteValue"
            or write_value.get("resolved") is not True
            or write_value.get("__perl") != "CODE"
            or not isinstance(write_value.get("__deparse"), str)
            or not write_value["__deparse"].strip()
            or write_value.get("__name") != "Image::ExifTool::WriteValue"
            or write_value.get("source_file") != "Image/ExifTool/Writer.pl"
            or write_value.get("source_sha256") != mandatory_closure.get("Image/ExifTool/Writer.pl")):
        raise MandatoryRefused("mandatory defaults do not join the captured WriteValue helper")
    generic_context = _mapping(document.get("native_write_capture_context"), "native_write_capture_context")
    loaded = _mapping(generic_context.get("loaded_modules"), "native_write_capture_context.loaded_modules")
    if (generic_context.get("resolved") is not True
            or loaded.get("Image/ExifTool/Writer.pl") != write_value["source_sha256"]):
        raise MandatoryRefused("mandatory WriteValue helper does not join the generic loaded closure")
    from writevalue_recipes import RecipeRefused, compile_numeric_write
    try:
        numeric = compile_numeric_write(helpers, mandatory_closure, loaded)
    except RecipeRefused as error:
        raise MandatoryRefused(str(error)) from error

    registry = _mapping(document.get("native_write_format_registry"), "native_write_format_registry")
    source = _mapping(registry.get("source"), "mandatory TIFF format registry provenance")
    registry_file = source.get("library_relative_path")
    registry_sha = source.get("sha256")
    if (registry.get("state") != "resolved" or registry_file != "Image/ExifTool/Exif.pm"
            or registry_sha != mandatory_closure.get(registry_file)
            or registry_sha != loaded.get(registry_file)):
        raise MandatoryRefused("mandatory TIFF format registry does not join the native source closures")
    numbers = _mapping(registry.get("format_number"), "mandatory TIFF format numbers")

    rows = _mapping(fact.get("raw_exif_main_row_properties"), "raw_exif_main_row_properties")
    # The direct new-directory path chooses Format when present, otherwise
    # Writable.  This bounded encoder only implements those two native scalar
    # packing procedures, and rejects every row with another write hook.
    # Every integer lexical default enters the same `$tagTablePtr->{id}` /
    # `WriteValue(value, format, 1)` path.  Directory selection chooses the
    # destination; it does not change the selected table row or its format.
    # Text operands remain outside this bounded numeric packer.
    candidates = [(directory.directory, item.tag_id)
                  for directory in recipe.directories for item in directory.defaults
                  if item.kind == "Integer"]
    # JFIF replaces values already assigned through the same direct IFD0 map.
    # Include those IDs even if a release stops listing their base integer
    # default, but keep their admission subject to the identical row checks.
    candidates.extend((recipe.jfif_override.directory, tag_id)
                      for tag_id, _property, _adjustment in recipe.jfif_override.assignments)
    encodings = []
    omissions = []
    emitted_ids: set[int] = set()
    for directory, tag_id in sorted(set(candidates), key=lambda item: (item[1], item[0])):
        controls = _mapping(rows.get(str(tag_id)), f"raw_exif_main_row_properties[{tag_id}]")
        props = controls
        def present(name: str) -> Any:
            item = _mapping(controls.get(name), f"mandatory row control {name}")
            if item.get("unsupported") is True:
                raise MandatoryRefused(f"mandatory row {name} is a reference")
            return item.get("value") if item.get("present") is True else None
        write_group, writable = present("WriteGroup"), present("Writable")
        # The entry assignment is captured from `$tagTablePtr->{id}`.  A row
        # explicitly directed to IFD0 is the source representation we can
        # join to that direct path.  This admits the IFD1 defaults whose Exif
        # row retains IFD0 as its write destination, but leaves e.g. ExifIFD
        # defaults with no direct entry identity explicit and unencoded.
        if write_group != "IFD0":
            omissions.append(DefaultEncodingOmission(directory, tag_id,
                                                      "direct_write_group_unrepresented"))
            continue
        if writable not in {"int16u", "rational64u"}:
            omissions.append(DefaultEncodingOmission(directory, tag_id,
                                                      "direct_writable_unrepresented"))
            continue
        if _mapping(props.get("Format", {"present": False}), "mandatory row Format").get("present") is True:
            omissions.append(DefaultEncodingOmission(directory, tag_id,
                                                      "direct_format_selection_unrepresented"))
            continue
        for name in ("CanCreate", "DelValue", "Deletable", "PrintConvInv", "RawConvInv", "Validate", "ValueConvInv", "WriteAlso", "WriteCheck", "WriteCondition", "WriteHook", "WriteLast", "WritePseudo"):
            if present(name) is not None:
                omissions.append(DefaultEncodingOmission(directory, tag_id,
                                                          f"direct_{name.lower()}_unrepresented"))
                break
        else:
            format_name = str(writable)
            if format_name not in numeric.formats:
                omissions.append(DefaultEncodingOmission(directory, tag_id,
                                                          "direct_numeric_format_unrepresented"))
                continue
            type_code = numbers.get(format_name)
            sizes = registry.get("format_size")
            if (type(type_code) is not int or not 0 < type_code <= 65535 or not isinstance(sizes, list)
                    or type_code >= len(sizes) or type(sizes[type_code]) is not int
                    or sizes[type_code] != (2 if format_name == "int16u" else 8)
                    or not isinstance(registry.get("format_name"), list)
                    or type_code >= len(registry["format_name"])
                    or registry["format_name"][type_code] != format_name):
                raise MandatoryRefused("mandatory TIFF format registry entry is unsupported")
            if tag_id not in emitted_ids:
                encodings.append(DefaultEncoding(tag_id, format_name, type_code))
                emitted_ids.add(tag_id)
            continue
        # The `for` loop found an unmodeled WriteValue-affecting property.
        # It is deliberately not a fatal error for unrelated directories, but
        # remains in the serialized recipe/report as an explicit omission.
        continue
    sizes = registry.get("format_size")
    names = registry.get("format_name")
    survivor = []
    # These closed physical forms are tied to the exact helpers compiled in
    # compile_numeric_write. A registry remap/width change is a new native
    # layout, never an implicit activation of a Rust conversion.
    physical = {"int16u": (3, 2), "int32u": (4, 4), "rational64u": (5, 8)}
    for format_name in numeric.formats:
        expected_type, expected_width = physical[format_name]
        type_code = numbers.get(format_name)
        if (type_code != expected_type or not isinstance(sizes, list) or not isinstance(names, list)
                or type_code >= len(sizes) or type_code >= len(names)
                or sizes[type_code] != expected_width or names[type_code] != format_name):
            raise MandatoryRefused("mandatory survivor format registry layout is unsupported")
        survivor.append(SurvivorEncoding(format_name, type_code, expected_width, "write_value_scalar"))
    return replace(recipe, encodings=tuple(encodings),
                   unencoded_numeric_defaults=tuple(omissions),
                   write_value_source_sha256=str(write_value["source_sha256"]),
                   survivor_encodings=tuple(survivor))


def recipe_json(recipe: MandatoryRecipe) -> dict[str, Any]:
    value = asdict(recipe)
    value["recipe_sha256"] = hashlib.sha256(repr(value).encode()).hexdigest()
    return value
