"""Closed compiler for WriteExif lexical mandatory-directory defaults.

The defaults themselves come from the final lexical ``%mandatory`` map.  The
only supported control flow is WriteExif's source fragment which selects that
map by directory, optionally clones IFD0 and substitutes three JFIF operands,
then adds missing entries for a new directory.  Unsupported source refuses.
"""
from __future__ import annotations

from dataclasses import asdict, dataclass
import hashlib
import re
from typing import Any, Mapping


class MandatoryRefused(ValueError): pass
class MandatoryMalformed(ValueError): pass

_SHA = re.compile(r"[0-9a-f]{64}\Z")
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
class MandatoryRecipe:
    writer_source_file: str
    writer_source_sha256: str
    directories: tuple[DirectoryDefaults, ...]
    jfif_override: JfifOverride
    selection: MandatorySelection

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
    return MandatoryRecipe(str(source), digest, tuple(directories), override, selection)

def compile_mandatory_joined(fact: Mapping[str, Any], document: Mapping[str, Any]) -> MandatoryRecipe:
    """Require defaults and the general captured WriteExif callback share a source identity."""
    recipe = compile_mandatory(fact)
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
    return recipe

def recipe_json(recipe: MandatoryRecipe) -> dict[str, Any]:
    value = asdict(recipe)
    value["recipe_sha256"] = hashlib.sha256(repr(value).encode()).hexdigest()
    return value
