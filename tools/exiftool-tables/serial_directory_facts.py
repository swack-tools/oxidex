"""Pure source facts for staged serial-directory descriptors.

This module deliberately does not recognize Perl control flow.  It serializes
captured source values deterministically so a compiler and an independent
verifier can both detect stale native facts without sharing a recognizer.
"""

import hashlib
import json
from pathlib import PurePosixPath
import re


class SerialFactRefused(ValueError):
    """A required raw source fact is malformed."""


_SHA256 = re.compile(r"[0-9a-f]{64}")


def deparse_sha256(source):
    """Digest the exact UTF-8 B::Deparse source supplied by the native dump."""
    if not isinstance(source, str):
        raise SerialFactRefused("missing serial processor deparse source")
    try:
        raw = source.encode("utf-8", "strict")
    except UnicodeError as error:
        raise SerialFactRefused("serial processor deparse is not UTF-8") from error
    return hashlib.sha256(raw).hexdigest()


def canonical_json_sha256(value):
    """Digest a source-shaped JSON value without interpreting its semantics."""
    try:
        raw = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8", "strict")
    except (TypeError, UnicodeError) as error:
        raise SerialFactRefused("serial native facts are not canonical JSON") from error
    return hashlib.sha256(raw).hexdigest()


def source_identity(processor):
    """Return independently checkable, library-relative processor provenance."""
    if not isinstance(processor, dict) or processor.get("__perl") != "CODE":
        raise SerialFactRefused("serial processor is not a captured CODE fact")
    if processor.get("resolved") is not True:
        raise SerialFactRefused("serial processor source is unresolved")
    name = processor.get("__name")
    source_file = processor.get("source_file")
    source_sha = processor.get("source_sha256")
    if not isinstance(name, str) or not name:
        raise SerialFactRefused("serial processor identity is unavailable")
    if (
        not isinstance(source_file, str)
        or not source_file
        or "\\" in source_file
        or PurePosixPath(source_file).is_absolute()
        or PurePosixPath(source_file).as_posix() != source_file
        or any(part in {".", ".."} for part in source_file.split("/"))
    ):
        raise SerialFactRefused("serial processor source path is not library-relative")
    if not isinstance(source_sha, str) or _SHA256.fullmatch(source_sha) is None:
        raise SerialFactRefused("serial processor source hash is unavailable")
    return {"name": name, "source_file": source_file, "source_sha256": source_sha}
