"""Lexical facts for a captured length-prefixed word processor.

This module intentionally does not recognise Perl control flow.  It gives the
compiler and an independent verifier one stable, raw source fact to compare:
the UTF-8 bytes of B::Deparse output.  Semantic acceptance remains separately
implemented by ``word_directory`` and must not be inferred from this digest.
"""

import hashlib


class WordDirectoryFactRefused(ValueError):
    """The capture has no stable deparsed source text."""


def deparse_sha256(source):
    """Return the canonical digest of unmodified captured B::Deparse text."""
    if not isinstance(source, str):
        raise WordDirectoryFactRefused("missing processor deparse source")
    try:
        encoded = source.encode("utf-8", "strict")
    except UnicodeError as error:
        raise WordDirectoryFactRefused("processor deparse source is not UTF-8") from error
    return hashlib.sha256(encoded).hexdigest()
