"""Authenticated bounded Writer group-selector operands (families 0 and 1).

Writer.pl splits on the last colon, then evaluates every requested group in
order.  This component represents the safe subset with explicit ``0``/``1``
families and conjunction semantics.  It is source data for a later Rust route;
it does not activate SetNewValue.
"""
from __future__ import annotations

from dataclasses import dataclass
import re
from typing import Any, Mapping

from checkexif_recipes import RecipeRefused
from setnewvalue_addressing import AddressRow, _observed_candidates, compile_addressing


_SELECTOR = re.compile(r"^([01])([A-Za-z0-9_-]+)$")


@dataclass(frozen=True)
class FamilySelector:
    family: int
    group: str


@dataclass(frozen=True)
class FamilyResolution:
    state: str
    row: AddressRow | None = None
    write_group: str | None = None
    reason: str | None = None


def parse(text: str) -> tuple[tuple[FamilySelector, ...], str] | FamilyResolution:
    if not isinstance(text, str) or ":" not in text:
        return FamilyResolution("owned_unsupported", reason="explicit family selector is required")
    group, tag = text.rsplit(":", 1)
    if not tag or not group:
        return FamilyResolution("owned_unsupported", reason="Writer last-colon group syntax is malformed")
    selectors = []
    for token in group.split(":"):
        match = _SELECTOR.fullmatch(token)
        if not match:
            return FamilyResolution("owned_unsupported", reason="selector family is outside bounded 0/1 subset")
        selectors.append(FamilySelector(int(match.group(1)), match.group(2).lower()))
    return tuple(selectors), tag


def resolve(document: Mapping[str, Any], observations: Mapping[str, Any], text: str) -> FamilyResolution:
    """Apply all explicit family selectors as Writer-style conjunctions."""
    parsed = parse(text)
    if isinstance(parsed, FamilyResolution):
        return parsed
    selectors, name = parsed
    addressing, _ = compile_addressing(document)
    rows = {row.identity: row for row in addressing.rows if row.name.lower() == name.lower()}
    if not rows:
        return FamilyResolution("owned_unsupported", reason="source name has no final static row")
    observed = _observed_candidates(addressing, observations, name)
    if observed is None:
        return FamilyResolution("owned_unsupported", reason="native lookup observation is unavailable")
    matched = []
    external = False
    for candidate in observed:
        groups = candidate.get("groups")
        if not isinstance(groups, Mapping):
            raise RecipeRefused("native group observation is malformed")
        if not all(isinstance(groups.get(str(selector.family)), str) and
                   groups[str(selector.family)].lower() == selector.group for selector in selectors):
            continue
        identity = tuple(candidate.get(key) for key in ("module", "table", "full_name", "raw_id", "name"))
        row = rows.get(identity)
        if row is None:
            external = True
        else:
            matched.append(row)
    if external:
        return FamilyResolution("owned_unsupported", reason="selected family group has an external native candidate")
    unique = {row.identity: row for row in matched}
    if len(unique) != 1:
        return FamilyResolution("owned_unsupported", reason="family selectors do not select one static row")
    row = next(iter(unique.values()))
    return FamilyResolution("resolved", row=row, write_group=row.write_group)
