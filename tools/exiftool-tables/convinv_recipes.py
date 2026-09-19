"""Inactive closed ConvInv scalar fallthrough recipe."""
from dataclasses import dataclass
from pathlib import Path
import json
import re
from typing import Any
from checkexif_recipes import RecipeRefused, _fact
import native_reader_facts

@dataclass(frozen=True)
class ConvInvRecipe:
    provenance: Any
    default_type: str = "PrintConv"
    error_separator: str = " for "


@dataclass(frozen=True)
class ConvInvProfile:
    """One complete ConvInv body shape, captured from an immutable release.

    ``template`` is admitted only by exact token consumption; ``captured_from``
    names the release the capture came from and never selects anything.  Each
    ``operands`` entry is B::Deparse with every whitespace character removed and
    must occur exactly once in the raw body: the template proves no executable
    token was ignored, the operands prove the regions the scalar executor
    models still carry their native meaning.  ``equivalences`` records each way
    the body differs from the pin and why the generated executor is still exact
    for it; the pin's own profile has none.
    """
    template: str
    captured_from: str
    operands: tuple[tuple[str, str], ...]
    equivalences: tuple[str, ...] = ()


def _stripped(source: str) -> str:
    return re.sub(r"\s+", "", source)


# Regions every admitted profile shares.  `<ERROR_LITERAL>` and
# `<DEFAULT_TYPE>` are replaced with the body's own (whitespace-stripped)
# placeholder tokens before counting.  Operands are checked on raw source, not
# on `body_tokens`: that tokenizer normalizes `my($x)` and reads `/` as a regex
# opener, so its output is not a faithful substring index.
_SHARED_OPERANDS = (
    ("PrintConv->ValueConv advance", r"}elsif(($typeeq'PrintConv')){($type='ValueConv');}else{"),
    ("conversion presence", r"((defined($conv)ordefined($convInv))or(next));"),
    ("CHECK_PROC guard", r"if((($tableand$table->{'CHECK_PROC'})andnot($tagInfo->{'RawConvInv'}))){"),
    ("error result", r"""if(defined($err2)){if($err2){($err=<ERROR_LITERAL>);$self->VPrint(2,("$err\n"));$val=undef;}else{($err=$err2);}}"""),
)
_DEFAULTED_CONVTYPE = (
    ("default conversion type", r"($convTypeor($convType=($self->{'ConvType'}||<DEFAULT_TYPE>)));"),
    ("first conversion type", r"($type=$convType);"),
)
_CHECK_PROC_WITH_CONVTYPE = (
    ("CHECK_PROC list call", r"($err2=&$checkProc($self,$tagInfo,(\$v),$convType));"),
    ("CHECK_PROC scalar call", r"($err2=&$checkProc($self,$tagInfo,(\$val),$convType));"),
)
_DEFINED_WRITECHECK_GATE = (("WriteCheck result gate", r"unless(defined($err2)){"),)
_TRUTHY_WRITECHECK_GATE = (("WriteCheck result gate", r"unless($err2){"),)

# The historical WriteCheck gate reads `$err2` for truth, 13.59 for
# definedness.  `$err2` is only ever assigned there by WriteCheck, and both
# `scalar_fallthrough` and the Rust executor refuse a WriteCheck row, so the
# gate's operand is always undef and both forms run CHECK_PROC.
_TRUTHY_WRITECHECK = (
    "WriteCheck result gate is `unless ($err2)`, not `unless (defined($err2))`: "
    "only WriteCheck assigns $err2 before it and every WriteCheck row is refused, "
    "so $err2 is undef and CHECK_PROC runs under both forms")

_PROFILES = (
    ConvInvProfile(
        "convinv_full_template.json", "13.59",
        _DEFAULTED_CONVTYPE + _DEFINED_WRITECHECK_GATE + _CHECK_PROC_WITH_CONVTYPE + _SHARED_OPERANDS),
    ConvInvProfile(
        "convinv_full_template_12_64.json", "12.64",
        _DEFAULTED_CONVTYPE + _TRUTHY_WRITECHECK_GATE + _CHECK_PROC_WITH_CONVTYPE + _SHARED_OPERANDS,
        (_TRUTHY_WRITECHECK,)),
    ConvInvProfile(
        "convinv_full_template_11_78.json", "11.78",
        (("default conversion type",
          r"($type=(($convType||$self->{'ConvType'})||<DEFAULT_TYPE>));"),)
        + _TRUTHY_WRITECHECK_GATE
        + (("CHECK_PROC list call", r"($err2=&$checkProc($self,$tagInfo,(\$v)));"),
           ("CHECK_PROC scalar call", r"($err2=&$checkProc($self,$tagInfo,(\$val)));"))
        + _SHARED_OPERANDS,
        ("the default conversion type is resolved into $type inside the loop "
         "(`$type = $convType || $$self{ConvType} || <DEFAULT_TYPE>`) and $convType "
         "itself is never defaulted: the first $type is the same falsy-chain value "
         "the executor computes, and nothing else in the body reads $convType",
         "CHECK_PROC is called with three arguments, not four ($convType omitted): an "
         "admitted CHECK_PROC binds exactly `my ($et, $tagInfo, $valPtr) = @_` and has "
         "no other statement that could read a fourth (checkexif_recipes grammar), and "
         "the Rust check_exif takes no conversion type",
         _TRUTHY_WRITECHECK)),
)


def _template(profile: ConvInvProfile) -> list[str]:
    try:
        value = json.loads(Path(__file__).with_name(profile.template).read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise RecipeRefused(f"ConvInv template {profile.template} is unavailable") from error
    if not isinstance(value, list) or any(not isinstance(token, str) for token in value):
        raise RecipeRefused(f"ConvInv template {profile.template} is malformed")
    if value.count("<ERROR_LITERAL>") != 1 or value.count("<DEFAULT_TYPE>") != 1:
        raise RecipeRefused("ConvInv template is malformed")
    return value


def _matches(tokens: list[str], template: list[str]) -> bool:
    return len(tokens) == len(template) and all(
        actual == wanted or wanted in {"<ERROR_LITERAL>", "<DEFAULT_TYPE>"}
        for actual, wanted in zip(tokens, template))


def compile_convinv_profile(fact: Any) -> tuple[ConvInvRecipe, ConvInvProfile]:
    # Active conversion calls have a large native dependency closure. They are
    # deliberately outside this route, so validate the ConvInv CV itself while
    # retaining (but never trusting) the opaque closure.  `_fact` recursively
    # validates executable dependencies for a composed call; doing that here
    # would make an unexecuted ConvertDateTime dependency an implicit input to
    # this scalar fallthrough.
    if not isinstance(fact, dict):
        raise RecipeRefused("ConvInv fact is not an object")
    shallow = dict(fact)
    shallow["dependencies"] = {}
    p = _fact(shallow, "native_write_helpers.conv_inv", require_binding=True)
    if p.requested_binding != "Image::ExifTool::ConvInv":
        raise RecipeRefused("ConvInv binding is stale")
    body = fact.get("__deparse")
    if not isinstance(body, str):
        raise RecipeRefused("ConvInv body is outside scalar closed grammar")
    try:
        tokens = native_reader_facts.body_tokens(body)
    except native_reader_facts.ReaderRefused as error:
        raise RecipeRefused("ConvInv body is outside scalar closed grammar") from error
    # Admit a profile only by exact consumption of its whole body.  The
    # release label is never consulted.  A body no profile has the length of
    # has statements none models; one of a matching length that still differs
    # has a changed operand or order.
    templates = [(profile, _template(profile)) for profile in _PROFILES]
    matched = [(profile, template) for profile, template in templates if _matches(tokens, template)]
    if not matched:
        if not any(len(tokens) == len(template) for _, template in templates):
            raise RecipeRefused("ConvInv body has unsupported statements")
        raise RecipeRefused("ConvInv source operand/order is unsupported")
    if len(matched) != 1:
        raise RecipeRefused("ConvInv body matches more than one profile")
    profile, template = matched[0]
    default = tokens[template.index("<DEFAULT_TYPE>")]
    if default not in {"'PrintConv'"}: raise RecipeRefused("ConvInv default conversion type is unsupported")
    literal = tokens[template.index("<ERROR_LITERAL>")]
    match = re.fullmatch(r'"\$err2([^$@\\]*)\$\{wgrp1\}:\$tag"', literal)
    if match is None: raise RecipeRefused("ConvInv error literal is unsupported")
    compact = _stripped(body)
    for name, operand in profile.operands:
        operand = operand.replace("<ERROR_LITERAL>", _stripped(literal)).replace(
            "<DEFAULT_TYPE>", _stripped(default))
        # Exactly once: a second copy would be a second, unmodeled path.
        if compact.count(operand) != 1:
            raise RecipeRefused(f"ConvInv {name} is not the modeled operand")
    return ConvInvRecipe(p, default_type=default[1:-1], error_separator=match.group(1)), profile


def compile_convinv(fact: Any) -> ConvInvRecipe:
    return compile_convinv_profile(fact)[0]

def scalar_fallthrough(recipe: ConvInvRecipe, value: Any, row: dict[str, Any], conv_type: str | None = None):
    if not isinstance(row, dict) or (conv_type is not None and conv_type != recipe.default_type):
        raise RecipeRefused("ConvInv input/type is unsupported")
    # Defined false conversions still alter native control flow: presence refuses.
    for key in ("PrintConv", "PrintConvInv", "ValueConv", "ValueConvInv"):
        if key in row: raise RecipeRefused("ConvInv active conversion: " + key)
    for key in ("List", "RawJoin", "WriteCheck", "RawConvInv"):
        if row.get(key): raise RecipeRefused("ConvInv scalar gate: " + key)
    table=row.get("Table")
    if table is not None and not isinstance(table, dict): raise RecipeRefused("ConvInv table malformed")
    if isinstance(table,dict) and table.get("CHECK_PROC"): raise RecipeRefused("ConvInv CHECK_PROC")
    return value, None
