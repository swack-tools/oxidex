"""Closed compiler for a native length-prefixed u16 key/value directory.

This is deliberately a source recognizer, not a route selector.  A capture
caller supplies one resolved Perl CODE fact and the independently authenticated
unsigned-reader state.  The recognizer accepts precisely one complete control
flow: a header-size check with one member-regex exception, followed by a
fixed-stride u16 word loop whose high bits select a key and masked low bits
are the numeric value passed to ``HandleTag``.  Any extra statement, alias, or
unmodeled handler operand refuses.

The returned descriptor is a staged generated operand.  No Rust reader or
table activation is introduced by this module.
"""

from dataclasses import dataclass
import hashlib
import json
from pathlib import PurePosixPath
import re

import conds
import native_reader_contract
import native_reader_facts
import word_directory_facts


class WordDirectoryRefused(ValueError):
    """The captured native processor is outside the exact shared shape."""


_FQ = r"(?:[A-Za-z_]\w*::)*[A-Za-z_]\w*"
_TOKEN = re.compile(
    r"\s+|'(?:\\.|[^'\\])*'|\"(?:\\.|[^\"\\])*\"|/(?!\s)(?:\\.|[^/\\])*/[a-z]*"
    r"|\$\$?[A-Za-z_]\w*|@[A-Za-z_]\w*|(?:[A-Za-z_]\w*::)*[A-Za-z_]\w*"
    r"|\d+|==|=~|>>|\+=|->|[(){};,=+*/&<>$@-]"
)


def _tokens(source):
    if not isinstance(source, str):
        raise WordDirectoryRefused("missing native processor body")
    result, at = [], 0
    while at < len(source):
        match = _TOKEN.match(source, at)
        if match is None:
            raise WordDirectoryRefused("processor syntax outside the closed grammar")
        if not match.group().isspace():
            result.append(match.group())
        at = match.end()
    return result


class _Body:
    def __init__(self, source):
        self.tokens = _tokens(source)
        self.at = 0

    def take(self, *wanted):
        if self.tokens[self.at:self.at + len(wanted)] != list(wanted):
            raise WordDirectoryRefused("processor body is not a length-prefixed u16 directory")
        self.at += len(wanted)

    def scalar(self):
        if self.at >= len(self.tokens) or re.fullmatch(r"\$[A-Za-z_]\w*", self.tokens[self.at]) is None:
            raise WordDirectoryRefused("processor binder is not a scalar local")
        value = self.tokens[self.at]
        self.at += 1
        return value

    def string(self):
        if self.at >= len(self.tokens) or not self.tokens[self.at].startswith(("'", '"')):
            raise WordDirectoryRefused("processor diagnostic is not a literal string")
        value = self.tokens[self.at]
        self.at += 1
        quote, text = value[0], value[1:-1]
        out, at = [], 0
        while at < len(text):
            char = text[at]
            if char in "$@" and quote == '"':
                raise WordDirectoryRefused("processor diagnostic has unmodeled Perl interpolation")
            if char != "\\":
                out.append(char)
                at += 1
                continue
            if at + 1 == len(text):
                raise WordDirectoryRefused("processor diagnostic has a trailing escape")
            escaped = text[at + 1]
            if quote == "'":
                # Single-quoted Perl only unescapes a quote and backslash.
                out.append(escaped if escaped in "\\'" else "\\" + escaped)
            else:
                decoded = {"n": "\n", "r": "\r", "t": "\t", "f": "\f", "b": "\b",
                           "a": "\a", "e": "\x1b", "\\": "\\", '"': '"', "$": "$", "@": "@"}
                if escaped not in decoded:
                    raise WordDirectoryRefused("processor diagnostic escape is outside the shared literal grammar")
                out.append(decoded[escaped])
            at += 2
        return "".join(out)

    def number(self, *, maximum=0xffffffff):
        if self.at >= len(self.tokens) or re.fullmatch(r"(?:0|[1-9]\d*)", self.tokens[self.at]) is None:
            raise WordDirectoryRefused("processor operand is not an unsigned literal")
        value = int(self.tokens[self.at])
        self.at += 1
        if value > maximum:
            raise WordDirectoryRefused("processor operand exceeds the shared integer domain")
        return value

    def declaration(self):
        self.take("my")
        wrapped = self.tokens[self.at:self.at + 1] == ["("]
        if wrapped:
            self.take("(")
        name = self.scalar()
        if wrapped:
            self.take(")")
        return name


def _provenance(fact):
    if not isinstance(fact, dict) or fact.get("__perl") != "CODE" or fact.get("resolved") is not True:
        raise WordDirectoryRefused("native processor source is unresolved")
    if not isinstance(fact.get("__name"), str) or re.fullmatch(_FQ, fact["__name"]) is None:
        raise WordDirectoryRefused("native processor identity is unavailable")
    file, sha = fact.get("source_file"), fact.get("source_sha256")
    if (not isinstance(file, str) or not file or PurePosixPath(file).is_absolute()
            or any(part in {".", ".."} for part in file.split("/"))
            or PurePosixPath(file).as_posix() != file or "\\" in file
            or not isinstance(sha, str) or re.fullmatch(r"[0-9a-f]{64}", sha) is None):
        raise WordDirectoryRefused("native processor source provenance is unavailable")
    return file, sha


def _reader_fingerprint(processor, reader_contracts, package):
    try:
        snapshot = reader_contracts.get("unsigned16") if isinstance(reader_contracts, dict) else None
        reader_sha = native_reader_contract.fingerprint(snapshot)
        dependencies = processor.get("dependencies")
        # B::Deparse leaves this processor's calls bare.  The capture must
        # therefore follow the actual package symbol selected by that body;
        # accepting a fabricated core-package fact would miss a local glob
        # rebind while preserving the identical deparsed processor text.
        binding = f"{package}::Get16u"
        get16u = dependencies.get(binding) if isinstance(dependencies, dict) else None
        expected = native_reader_facts.source_fact(snapshot["loaded_functions"]["get16u"], "Image::ExifTool::Get16u")
        if native_reader_facts.source_fact(get16u, "Image::ExifTool::Get16u") != expected:
            raise native_reader_facts.ReaderRefused("processor reader differs from loaded reader")
    except (native_reader_facts.ReaderRefused, KeyError, TypeError):
        raise WordDirectoryRefused("unsigned reader contract is unavailable or does not bind this processor") from None
    return reader_sha


def _condition(regex):
    # The source predicate is reconstructed only from the captured operand;
    # ``conds`` supplies the shared closed member-regex grammar and byte-regex
    # policy.  Do not broaden it here.
    if not isinstance(regex, str) or not regex.startswith("/"):
        raise WordDirectoryRefused("processor exception is not a source regex")
    closing = regex.rfind("/")
    if closing <= 0:
        raise WordDirectoryRefused("processor exception is not a source regex")
    # Perl interpolates unescaped scalar and array sigils in a regex literal.
    # The shared Rust regex operand is static, so accepting `/\$size/` as the
    # characters dollar/size is fine but accepting `/$size/` would silently
    # freeze a runtime-native value into the wrong literal pattern.
    pattern = regex[1:closing]
    escaped = False
    for char in pattern:
        if escaped:
            escaped = False
        elif char == "\\":
            escaped = True
        elif char in "$@":
            raise WordDirectoryRefused("processor regex has unmodeled Perl interpolation")
    value = conds.compile_cond("$$self{Model} =~ " + regex)
    if value is None:
        raise WordDirectoryRefused("processor model predicate is outside the shared condition grammar")
    return value


def is_candidate(processor):
    """Whether an unauthenticated CODE body has the shared processor cues.

    This never permits generation.  It lets the generator name a missing or
    malformed source/provenance capture as a processor blocker instead of
    silently treating a structurally relevant table as outside the inventory.
    """
    body = processor.get("__deparse") if isinstance(processor, dict) else None
    if not isinstance(body, str):
        return False
    try:
        tokens = set(_tokens(body))
    except WordDirectoryRefused:
        return False
    return {"Get16u", "HandleTag", "'DataPt'", "'DirStart'", "'DirLen'"}.issubset(tokens)


@dataclass(frozen=True)
class LengthPrefixedU16Pairs:
    """Generated operands plus the native evaluation rules a reader must keep.

    ``Model`` is an empty regex subject when absent, and a short ``Get16u``
    result is numerically zero in this processor because Perl coerces undef in
    the subsequent shift/mask expressions.  These are intentional descriptor
    rules, not generic assumptions about conditions or binary reads.
    """
    pair_start: int
    pair_stride: int
    key_shift: int
    value_mask: int
    header_adjustment: int
    model_condition: str
    exact_length_first: bool
    missing_model_as_empty: bool
    short_u16_as_zero: bool
    index_divisor: int
    index_bias: int
    value_format: str
    value_count: int
    value_size: int
    invalid_warning: str
    verbose_directory: str
    source_file: str
    source_sha256: str
    source_body_sha256: str
    reader_contract_sha256: str

    def fingerprint(self):
        return hashlib.sha256(json.dumps(self.__dict__, sort_keys=True, separators=(",", ":")).encode()).hexdigest()

    def rust(self, escape):
        """Return a staged literal for the future shared schema, not activation."""
        formats = {"int8u": "Fmt::Int8u"}
        value_format = formats.get(self.value_format)
        if value_format is None:  # guarded by the closed body recognizer
            raise WordDirectoryRefused("word-directory format has no shared Rust spelling")
        return (
            "WordDirectory { "
            f"pair_start: {self.pair_start}, pair_stride: {self.pair_stride}, "
            f"key_shift: {self.key_shift}, value_mask: {self.value_mask}, "
            f"header_adjustment: {self.header_adjustment}, "
            f"model_condition: {self.model_condition}, exact_length_first: {str(self.exact_length_first).lower()}, "
            f"missing_model_as_empty: {str(self.missing_model_as_empty).lower()}, "
            f"short_u16_as_zero: {str(self.short_u16_as_zero).lower()}, "
            f"index_divisor: {self.index_divisor}, index_bias: {self.index_bias}, "
            f"value_format: {value_format}, value_count: {self.value_count}, value_size: {self.value_size}, "
            f'invalid_warning: "{escape(self.invalid_warning)}", verbose_directory: "{escape(self.verbose_directory)}", '
            f'source_file: "{escape(self.source_file)}", source_sha256: "{self.source_sha256}", '
            f'source_body_sha256: "{self.source_body_sha256}", reader_contract_sha256: "{self.reader_contract_sha256}" '
            "}"
        )


def compile_word_directory(processor, reader_contracts):
    """Compile one complete native processor body into source-derived operands."""
    file, sha = _provenance(processor)
    body_source = processor.get("__deparse")
    body = _Body(body_source)

    body.take("(", "$", "$", "$", ")", "{", "package")
    if body.at >= len(body.tokens) or re.fullmatch(_FQ, body.tokens[body.at]) is None:
        raise WordDirectoryRefused("processor package is unavailable")
    package = body.tokens[body.at]
    body.at += 1
    reader_sha = _reader_fingerprint(processor, reader_contracts, package)
    body.take(";", "use", "strict", ";", "(", "my", "(")
    et, directory, table = body.scalar(), None, None
    body.take(",")
    directory = body.scalar()
    body.take(",")
    table = body.scalar()
    body.take(")", "=", "@_", ")", ";", "(")

    data = body.declaration()
    body.take("=", directory, "->", "{", "'DataPt'", "}", ")", ";", "(")
    offset = body.declaration()
    body.take("=", directory, "->", "{", "'DirStart'", "}", ")", ";", "(")
    size = body.declaration()
    body.take("=", directory, "->", "{", "'DirLen'", "}", ")", ";", "(")
    verbose = body.declaration()
    body.take("=", et, "->", "Options", "(")
    options = body.string()
    body.take(")", ")", ";", "(")
    if options != "Verbose":
        raise WordDirectoryRefused("processor verbose setup is outside the shared directory contract")

    length = body.declaration()
    body.take("=", "Get16u", "(", data, ",", offset, ")", ")", ";", "unless", "(", "(", "(", length, "==", size, ")", "or", "(", "(", et, "->", "{", "'Model'", "}", "=~")
    regex = body.tokens[body.at] if body.at < len(body.tokens) else None
    body.at += 1
    model_condition = _condition(regex)
    body.take(")", "and", "(", "(", length, "+")
    header_adjustment = body.number()
    body.take(")", "==", size, ")", ")", ")", ")", "{", et, "->", "Warn", "(")
    warning = body.string()
    body.take(")", ";", "(", "return", "0", ")", ";", "}", "(", verbose, "and", et, "->", "VerboseDir", "(")
    verbose_directory = body.string()
    body.take(",", "(", "(", size, "/")
    index_divisor = body.number()
    body.take(")", "-")
    index_bias = body.number()
    body.take(")", ")", ")", ";", "my", "(")
    position = body.scalar()
    body.take(")", ";", "for", "(", "(", position, "=")
    pair_start = body.number()
    body.take(")", ";", "(", position, "<", size, ")", ";", "(", position, "+=")
    pair_stride = body.number()
    body.take(")", ")", "{", "(")

    value = body.declaration()
    body.take("=", "Get16u", "(", data, ",", "(", offset, "+", position, ")", ")", ")", ";", "(")
    key = body.declaration()
    body.take("=", "(", value, ">>")
    key_shift = body.number(maximum=15)
    body.take(")", ")", ";", "(", value, "=", "(", value, "&")
    value_mask = body.number(maximum=0xffff)
    body.take(")", ")", ";", et, "->", "HandleTag", "(", table, ",", key, ",", value, ",")
    index_name = body.string()
    body.take(",", "(", "(", position, "/")
    handler_divisor = body.number()
    body.take(")", "-")
    handler_bias = body.number()
    body.take(")", ",")
    format_name = body.string()
    body.take(",")
    value_format = body.string()
    body.take(",")
    count_name = body.string()
    body.take(",")
    value_count = body.number()
    body.take(",")
    size_name = body.string()
    body.take(",")
    value_size = body.number()
    body.take(")", ";", "}", "(", "return", "1", ")", ";", "}")
    if body.at != len(body.tokens):
        raise WordDirectoryRefused("processor has extra statements or control flow")
    if len({et, directory, table, data, offset, size, verbose, length, position, value, key}) != 11:
        raise WordDirectoryRefused("processor aliases local bindings")
    if (pair_start != header_adjustment or pair_stride != index_divisor
            or handler_divisor != index_divisor or handler_bias != index_bias
            or index_name != "Index" or format_name != "Format" or count_name != "Count" or size_name != "Size"):
        raise WordDirectoryRefused("processor loop and handler operands disagree")
    if (pair_start == 0 or pair_stride == 0 or header_adjustment == 0
            or value_format != "int8u" or value_count != 1 or value_size != 1):
        # HandleTag receives the already-defined numeric result of this mask.
        # Its Format/Count/Size describe the native handler call; they must not
        # cause a future reader to truncate or re-decode that scalar as bytes.
        raise WordDirectoryRefused("processor handler value metadata is outside the shared scalar contract")
    # `HandleTag` accepts Perl numeric Index values. The staged reader records
    # the authenticated handler index as a non-negative integer, so reject a
    # body change that would make the first index fractional or negative
    # instead of silently flooring/underflowing it in Rust.
    if (pair_start % index_divisor != 0 or pair_stride % index_divisor != 0
            or pair_start // index_divisor < index_bias):
        raise WordDirectoryRefused("processor index is outside the shared non-negative integer contract")
    return LengthPrefixedU16Pairs(
        pair_start, pair_stride, key_shift, value_mask, header_adjustment,
        model_condition, True, True, True, index_divisor, index_bias, value_format, value_count,
        value_size, warning, verbose_directory, file, sha,
        word_directory_facts.deparse_sha256(body_source),
        reader_sha,
    )


def stale_reason(existing, processor, reader_contracts):
    """Return a concrete stale reason without trusting a prior descriptor."""
    try:
        fresh = compile_word_directory(processor, reader_contracts)
    except WordDirectoryRefused as error:
        return f"native processor is no longer compilable: {error}"
    if not isinstance(existing, LengthPrefixedU16Pairs) or existing.fingerprint() != fresh.fingerprint():
        return "native processor operands or provenance differ"
    return None
