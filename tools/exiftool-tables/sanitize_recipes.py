"""Compile the complete captured Writer.pl ``Sanitize`` helper body.

The resulting recipe is deliberately only an input-normalization mechanism.
It carries the two Perl-version predicates and captured dependency evidence.
This result is a source mechanism, NOT executable admission: the consumer must
validate any dependency used by its chosen path before emitting a runtime rule.
Unresolved optional branches stay visible instead of making the whole source
body disappear. No caller-provided runtime capability is accepted here.
"""
from copy import deepcopy
from dataclasses import dataclass
import re
from typing import Any

from checkexif_recipes import CodeProvenance, RecipeRefused, _fact
import native_reader_facts


_CANONICAL = r"""($$) {
package Image::ExifTool;
use strict;
(my($self, $valPt) = @_);
((ref($$valPt) eq 'SCALAR') and ($$valPt = $$$valPt));
if ((($] >= 5.006) and (($self->{'OPTIONS'}{'EncodeHangs'} || eval {
do {
(require Encode);
Encode::is_utf8($$valPt)
}
}) || $@))) {
(local $SIG{'__WARN__'} = (\&Image::ExifTool::SetWarning));
($$valPt = (($self->{'OPTIONS'}{'EncodeHangs'} || $@) ? pack('C*', unpack((($] < 5.01) ? 'U0C*' : 'C0C*'), $$valPt)) : Encode::encode('utf8', $$valPt)));
}
if ($self->{'OPTIONS'}{'Escape'}) {
if (($self->{'OPTIONS'}{'Escape'} eq 'XML')) {
($$valPt = &Image::ExifTool::XMP::UnescapeXML($$valPt));
} elsif (($self->{'OPTIONS'}{'Escape'} eq 'HTML')) {
($$valPt = &Image::ExifTool::HTML::UnescapeHTML($$valPt, $self->{'OPTIONS'}{'Charset'}));
}
}
}"""

_DEPENDENCIES = {
    "Encode::encode",
    "Encode::is_utf8",
    "Image::ExifTool::XMP::UnescapeXML",
    "Image::ExifTool::HTML::UnescapeHTML",
}

def _normalize_callback_ampersands(tokens: list[str]) -> list[str]:
    """Normalize explicit-argument calls only; implicit argument forwarding refuses."""
    out = []
    for index, token in enumerate(tokens):
        if token == "&" and index + 2 < len(tokens) and tokens[index + 2] == "(" and tokens[index + 1] in {
            "Image::ExifTool::XMP::UnescapeXML", "Image::ExifTool::HTML::UnescapeHTML",
            "Encode::encode", "Encode::is_utf8",
        }:
            continue
        out.append(token)
    return out


@dataclass(frozen=True)
class SanitizeRecipe:
    provenance: CodeProvenance
    raw_dependencies: dict[str, Any]
    callback_references: dict[str, Any]
    downgrade_at_or_after: int
    manual_pack_before: int
    encode_name: str
    manual_pack_formats: tuple[str, str, str]
    xml_unescape: str
    html_unescape: str


def _validate_retained_fact(fact: Any, context: str, depth: int = 0) -> None:
    """Validate partial evidence without requiring every branch to resolve."""
    if depth > 10 or not isinstance(fact, dict) or fact.get("__perl") != "CODE":
        raise RecipeRefused(f"Sanitize malformed dependency fact: {context}")
    if type(fact.get("resolved")) is not bool:
        raise RecipeRefused(f"Sanitize dependency resolution is malformed: {context}")
    children = fact.get("dependencies", {})
    if not isinstance(children, dict):
        raise RecipeRefused(f"Sanitize dependency children are malformed: {context}")
    if fact["resolved"]:
        # Authenticate this node, then validate each child independently so an
        # unresolved transitive branch stays explicit rather than disappearing.
        shallow = dict(fact)
        shallow.pop("dependencies", None)
        _fact(shallow, context, require_binding=False)
    else:
        name = fact.get("__name")
        reason = fact.get("reason")
        body = fact.get("__deparse")
        if (not isinstance(name, str)
                or re.fullmatch(r"(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*", name) is None
                or not isinstance(reason, str) or not reason
                or fact.get("source_file") is not None
                or fact.get("source_sha256") is not None
                or (body is not None and not isinstance(body, str))):
            raise RecipeRefused(f"Sanitize unresolved dependency is malformed: {context}")
    for binding, child in children.items():
        if (not isinstance(binding, str)
                or re.fullmatch(r"(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*", binding) is None):
            raise RecipeRefused(f"Sanitize dependency binding is malformed: {context}")
        _validate_retained_fact(child, f"{context}.{binding}", depth + 1)


def _version(tokens: list[str], marker: tuple[str, ...]) -> tuple[int, list[str]]:
    hits = [index for index in range(len(tokens) - len(marker)) if tuple(tokens[index:index + len(marker)]) == marker]
    if len(hits) != 1:
        raise RecipeRefused("Sanitize Perl-version predicate is outside the closed grammar")
    index = hits[0] + len(marker)
    if index + 2 >= len(tokens) or tokens[index + 1] != ".":
        raise RecipeRefused("Sanitize Perl-version predicate is malformed")
    try:
        major = int(tokens[index])
        fraction = tokens[index + 2]
        if not fraction.isdigit() or len(fraction) > 6:
            raise ValueError
    except ValueError as error:
        raise RecipeRefused("Sanitize Perl-version predicate is malformed") from error
    value = major * 1_000_000 + int(fraction.ljust(6, "0"))
    return value, tokens[:index] + ["$VERSION"] + tokens[index + 3:]


def compile_sanitize(fact: Any) -> SanitizeRecipe:
    # Dependencies include dynamic Encode XS facts which may be deliberately
    # unresolved.  Authenticate the helper CV itself without asking the common
    # recursive CODE validator to turn an unavailable optional branch into a
    # loss of the source mechanism artifact.
    if not isinstance(fact, dict):
        raise RecipeRefused("Sanitize fact is not an object")
    root_fact = dict(fact)
    raw_dependencies = root_fact.pop("dependencies", {})
    provenance = _fact(root_fact, "native_write_helpers.sanitize", require_binding=True)
    if provenance.requested_binding != "Image::ExifTool::Sanitize":
        raise RecipeRefused("Sanitize requested binding is stale or unavailable")
    if not isinstance(raw_dependencies, dict) or not all(
        isinstance(key, str) and isinstance(value, dict) for key, value in raw_dependencies.items()
    ):
        raise RecipeRefused("Sanitize dependencies are malformed")
    callbacks = fact.get("callback_references")
    if not isinstance(callbacks, dict) or set(callbacks) != {"Image::ExifTool::SetWarning"}:
        raise RecipeRefused("Sanitize callback reference set is unsupported")
    for evidence in (raw_dependencies, callbacks):
        for binding, observation in evidence.items():
            _validate_retained_fact(observation, binding)
    bindings = set(raw_dependencies)
    if bindings != _DEPENDENCIES:
        raise RecipeRefused("Sanitize direct dependency set is unsupported")
    try:
        tokens = native_reader_facts.body_tokens(fact["__deparse"])
        canonical = native_reader_facts.body_tokens(_CANONICAL)
    except (KeyError, TypeError, native_reader_facts.ReaderRefused) as error:
        raise RecipeRefused("Sanitize has no deparsed body") from error
    downgrade, normalized = _version(tokens, ("$", "]", ">", "="))
    manual, normalized = _version(normalized, ("$", "]", "<"))
    _canonical_downgrade, canonical = _version(canonical, ("$", "]", ">", "="))
    _canonical_manual, canonical = _version(canonical, ("$", "]", "<"))
    if _normalize_callback_ampersands(normalized) != _normalize_callback_ampersands(canonical):
        raise RecipeRefused("Sanitize executable body is outside the closed grammar")
    return SanitizeRecipe(
        provenance=provenance,
        raw_dependencies=deepcopy(raw_dependencies),
        callback_references=deepcopy(callbacks),
        downgrade_at_or_after=downgrade,
        manual_pack_before=manual,
        encode_name="utf8",
        manual_pack_formats=("C*", "U0C*", "C0C*"),
        xml_unescape="Image::ExifTool::XMP::UnescapeXML",
        html_unescape="Image::ExifTool::HTML::UnescapeHTML",
    )
