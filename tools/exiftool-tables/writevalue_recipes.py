"""Compile and evaluate the scalar ``WriteValue`` branch from captured Perl.

This is source-derived helper execution only.  It neither selects a tag nor
writes a file.  The scalar evaluator excludes numeric packing. The separate numeric compiler
below admits only count-one unsigned integer operands without ``$dataPt``.
"""
from dataclasses import dataclass
import operator
from typing import Any, Mapping

from checkexif_recipes import CodeProvenance, RecipeMalformed, RecipeRefused, _Body, _fact
from checkvalue_recipes import NativeScalar


_COMPARE = {">": operator.gt, ">=": operator.ge, "<": operator.lt,
            "<=": operator.le, "==": operator.eq, "!=": operator.ne}


@dataclass(frozen=True)
class ScalarWriteRecipe:
    provenance: CodeProvenance
    dispatch_lexical: str
    formats: tuple[str, str]
    terminated_format: str
    count_positive_operator: str
    count_positive_bound: int
    diff_negative_operator: str
    terminator: str


@dataclass(frozen=True)
class ScalarWriteResult:
    value: NativeScalar
    count: int


def _comparison(body: _Body) -> str:
    for symbol in (">=", "<=", "==", "!=", ">", "<"):
        if body.tokens[body.at:body.at + len(symbol)] == list(symbol):
            body.at += len(symbol)
            return symbol
    raise RecipeRefused("unsupported WriteValue comparison")


def _literal(body: _Body) -> str:
    token = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if token.startswith('"') and any(char in token[1:-1] for char in "$@"):
        raise RecipeRefused("interpolating WriteValue literal is unsupported")
    return body.quoted()


def _skip_block(body: _Body) -> None:
    """Skip only the proven-unreachable dispatch body, checking brace balance."""
    depth = 1
    while body.at < len(body.tokens) and depth:
        token = body.tokens[body.at]
        body.at += 1
        if token == "{":
            depth += 1
        elif token == "}":
            depth -= 1
    if depth:
        raise RecipeRefused("WriteValue dispatch block is incomplete")


def _dispatch_is_absent(fact: Mapping[str, Any], lexical: str, formats: tuple[str, str]) -> None:
    hashes = fact.get("lexical_hashes")
    if not isinstance(hashes, Mapping) or hashes.get("resolved") is not True:
        raise RecipeRefused("WriteValue lexical hash capture is unresolved")
    bindings = hashes.get("bindings")
    if not isinstance(bindings, Mapping):
        raise RecipeMalformed("WriteValue lexical hash bindings are malformed")
    captured = bindings.get(lexical)
    if not isinstance(captured, Mapping) or captured.get("resolved") is not True:
        raise RecipeRefused("WriteValue dispatch lexical hash is unresolved")
    entries = captured.get("entries")
    if not isinstance(entries, Mapping):
        raise RecipeMalformed("WriteValue dispatch lexical entries are malformed")
    if any(not isinstance(key, str) for key in entries):
        raise RecipeMalformed("WriteValue dispatch lexical key is malformed")
    for format_name in formats:
        if format_name in entries:
            raise RecipeRefused("WriteValue scalar format is intercepted by live dispatch")


def compile_scalar_write(fact: dict[str, Any]) -> ScalarWriteRecipe:
    provenance = _fact(fact, "native_write_helpers.write_value", require_binding=True)
    if provenance.requested_binding != "Image::ExifTool::WriteValue":
        raise RecipeRefused("WriteValue requested binding is stale")
    body = _Body(fact.get("__deparse"))
    body.take("(", "$", "$", ";", "$", "$", "$", "$", ")", "{", "package")
    package = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if not package or "::" not in package:
        raise RecipeRefused("WriteValue package unavailable")
    body.at += 1
    body.take(";", "use", "strict", ";", "(", "my", "(")
    value = body.word("$")
    body.take(",")
    format_name = body.word("$")
    body.take(",")
    count = body.word("$")
    body.take(",")
    data = body.word("$")
    body.take(",")
    offset = body.word("$")
    body.take(")", "=", "@", "_", ")", ";", "(", "my")
    proc = body.word("$")
    body.take("=", "$")
    # ``word`` owns sigils, but this lookup is a hash sigil followed by a
    # variable key and must retain both names for the live-pad join.
    if body.at >= len(body.tokens) or not body.tokens[body.at].isidentifier():
        raise RecipeRefused("WriteValue dispatch is not a lexical hash lookup")
    lexical = "%" + body.tokens[body.at]
    body.at += 1
    body.take("{", "$")
    if body.at >= len(body.tokens) or body.tokens[body.at] != format_name:
        raise RecipeRefused("WriteValue dispatch key is not the format argument")
    body.at += 1
    body.take("}", ")", ";", "my", "$")
    if body.at >= len(body.tokens) or not body.tokens[body.at].isidentifier():
        raise RecipeRefused("WriteValue packed local is unavailable")
    packed = body.tokens[body.at]
    body.at += 1
    body.take(";", "if", "(", "$", proc, ")", "{")
    if len({value, format_name, count, data, offset, proc, packed}) != 7:
        raise RecipeRefused("WriteValue local aliases an argument")
    _skip_block(body)
    body.take("elsif", "(", "(", "(", "$", format_name, "eq")
    first = _literal(body)
    body.take(")", "or", "(", "$", format_name, "eq")
    second = _literal(body)
    body.take(")", ")", ")", "{")
    body.take("(", "(", "$", format_name, "eq")
    terminated = _literal(body)
    body.take(")", "and", "(", "$", value, ".", "=")
    token = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if token != '"\\000"':
        raise RecipeRefused("unsupported WriteValue terminator")
    body.at += 1
    body.take(")", ")", ";", "if", "(", "(", "$", count, "and", "(", "$", count)
    positive = _comparison(body)
    if positive != ">":
        raise RecipeRefused("unsupported WriteValue count guard operator")
    token = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if not token.isdecimal():
        raise RecipeRefused("noninteger WriteValue count guard bound")
    bound = int(token)
    body.at += 1
    body.take(")", ")", ")", "{")
    body.take("(", "my", "$")
    if body.at >= len(body.tokens) or not body.tokens[body.at].isidentifier():
        raise RecipeRefused("WriteValue diff local is unavailable")
    diff = body.tokens[body.at]
    body.at += 1
    if diff in {value, format_name, count, data, offset, proc, packed}:
        raise RecipeRefused("WriteValue diff local aliases an existing local")
    body.take("=", "(", "$", count, "-", "length", "(", "$", value, ")", ")", ")", ";", "if", "(", "$", diff, ")", "{")
    body.take("if", "(", "(", "$", diff)
    negative = _comparison(body)
    if negative != "<":
        raise RecipeRefused("unsupported WriteValue truncation operator")
    body.take("0", ")", ")", "{")
    body.take("if", "(", "(", "$", format_name, "eq")
    truncate_terminated = _literal(body)
    body.take(")", ")", "{")
    body.take("(", "$", count, "or", "(", "return", "(", "undef", ")", ")", ")", ";")
    body.take("(", "$", value, "=", "substr", "(", "$", value, ",", "0", ",", "(", "$", count, "-", "1", ")", ")", ".")
    if body.at >= len(body.tokens) or body.tokens[body.at] != '"\\000"':
        raise RecipeRefused("unsupported WriteValue truncation terminator")
    body.at += 1
    body.take(")", ";", "}", "else", "{")
    body.take("(", "$", value, "=", "substr", "(", "$", value, ",", "0", ",", "$", count, ")", ")", ";", "}", "}", "else", "{")
    body.take("(", "$", value, ".", "=", "(")
    if body.at >= len(body.tokens) or body.tokens[body.at] != '"\\000"':
        raise RecipeRefused("unsupported WriteValue padding literal")
    body.at += 1
    body.take("x", "$", diff, ")", ")", ";", "}", "}", "}", "else", "{")
    body.take("(", "$", count, "=", "length", "(", "$", value, ")", ")", ";", "}")
    # ``$dataPt`` mutation is deliberately not executed, but its exact
    # placement before the scalar return proves this branch has no other call.
    body.take("(", "$", data, "and", "substr", "(", "$", "$", data, ",", "$", offset, ",", "$", count, ")", "=", "$", value, ")", ";", "(", "return", "$", value, ")", ";", "}")
    if body.at >= len(body.tokens):
        raise RecipeRefused("WriteValue fallback is incomplete")
    # The scalar return must be followed by a separate fallback and the numeric
    # tail; we do not execute either.  Balanced completion catches an inserted
    # scalar-side statement while avoiding a numerical grammar claim.
    body.take("else", "{")
    _skip_block(body)
    if body.at >= len(body.tokens) or body.tokens[-1] != "}":
        raise RecipeRefused("WriteValue function tail is incomplete")
    # The remaining numeric-tail return is unreachable after the scalar
    # branch's proven return. It is deliberately not translated here.
    if first == second or terminated not in {first, second} or truncate_terminated != terminated:
        raise RecipeRefused("WriteValue scalar format controls are inconsistent")
    _dispatch_is_absent(fact, lexical, (first, second))
    return ScalarWriteRecipe(provenance, lexical, (first, second), terminated,
                             positive, bound, negative, "\0")


def _append(value: NativeScalar, text: str) -> NativeScalar:
    if value.kind == "utf8":
        return NativeScalar("utf8", value.value + text)
    raw = b"" if value.kind == "undefined" else value.value
    return NativeScalar("bytes", raw + text.encode("latin1"))


def _truncate(value: NativeScalar, count: int) -> NativeScalar:
    if value.kind == "undefined":
        return value
    truncated = value.value[:count]
    if value.kind == "utf8" and not any(ord(char) > 0x7f for char in value.value):
        # Perl's ``substr`` may downgrade an upgraded ASCII scalar.  Keep that
        # state transition explicit instead of pretending all Python ``str``
        # results retain the UTF8 flag; a non-ASCII source retains it even
        # when the selected substring is empty.
        return NativeScalar("bytes", truncated.encode("latin1"))
    return NativeScalar(value.kind, truncated)


def evaluate_scalar_write(recipe: ScalarWriteRecipe, value: NativeScalar,
                          format_name: str, count: int | None) -> ScalarWriteResult:
    """Execute the admitted scalar return branch without ``$dataPt`` mutation."""
    if format_name not in recipe.formats:
        raise RecipeRefused("format is outside the proven WriteValue scalar branch")
    if count is not None and type(count) is not int:
        raise RecipeRefused("noninteger native count is unsupported")
    if format_name == recipe.terminated_format:
        value = _append(value, recipe.terminator)
    length = 0 if value.kind == "undefined" else len(value.value)
    if count and _COMPARE[recipe.count_positive_operator](count, recipe.count_positive_bound):
        diff = count - length
        if diff:
            if _COMPARE[recipe.diff_negative_operator](diff, 0):
                if format_name == recipe.terminated_format:
                    value = _append(_truncate(value, count - 1), recipe.terminator)
                else:
                    value = _truncate(value, count)
            else:
                value = _append(value, recipe.terminator * diff)
        return ScalarWriteResult(value, count)
    return ScalarWriteResult(value, length)


# A finite source grammar for the numeric specialization. Each production
# consumes the complete deparsed function, including validation, failure and
# return branches. These are executable terminals, not source hashes or
# required substrings. A new statement, operator, regex or dependency refuses.
# Deliberately normalize indentation only: regex/literal whitespace is semantic.
# Numeric execution is specialized to count=1, no data pointer, an integer
# operand in u16/u32 range. Other native input domains are not admitted.
_NUMERIC_BODIES = {
    'WriteValue': r'''($$;$$$$) {
    package Image::ExifTool;
    use strict;
    (my($val, $format, $count, $dataPt, $offset) = @_);
    (my($proc) = $writeValueProc{$format});
    my($packed);
    if ($proc) {
        (my @vals = split(' ', $val, 0));
        if ($count) {
            (($count < 0) and ($count = @vals));
        } else {
            ($count = 1);
        }
        ($packed = '');
        while (($count--)) {
            ($val = shift(@vals));
            (defined($val) or (return (undef)));
            if (($format =~ /^int/)) {
                unless ((IsInt($val) or IsHex($val))) {
                    (IsFloat($val) or (return (undef)));
                    ($val = int(($val + (($val < 0) ? (-0.5) : 0.5))));
                    ($_[0] = $val);
                }
            } elsif (not(IsFloat($val))) {
                ((($format =~ /^rational/) and ((($val eq 'inf') || ($val eq 'undef')) || IsRational($val))) or (return (undef)));
            }
            ($packed .= &$proc($val));
        }
    } elsif ((($format eq 'string') or ($format eq 'undef'))) {
        (($format eq 'string') and ($val .= "\000"));
        if (($count and ($count > 0))) {
            (my($diff) = ($count - length($val)));
            if ($diff) {
                if (($diff < 0)) {
                    if (($format eq 'string')) {
                        ($count or (return (undef)));
                        ($val = substr($val, 0, ($count - 1)) . "\000");
                    } else {
                        ($val = substr($val, 0, $count));
                    }
                } else {
                    ($val .= ("\000" x $diff));
                }
            }
        } else {
            ($count = length($val));
        }
        ($dataPt and substr($$dataPt, $offset, $count) = $val);
        (return $val);
    } else {
        warn(("Sorry, Can't write $format values on this platform\n"));
        (return (undef));
    }
    ($dataPt and substr($$dataPt, $offset, length($packed)) = $packed);
    (return $packed);
}''',
    'IsFloat': r'''($) {
    package Image::ExifTool;
    use strict;
    (($_[0] =~ /^[+-]?(?=\d|\.\d)\d*(\.\d*)?([Ee]([+-]?\d+))?$/) and (return 1));
    (($_[0] =~ /^[+-]?(?=\d|,\d)\d*(,\d*)?([Ee]([+-]?\d+))?$/) or (return 0));
    ($_[0] =~ tr/,/./);
    (return 1);
}''',
    'IsHex': r'''($) {
    package Image::ExifTool;
    use strict;
    (return scalar(($_[0] =~ /^(0x)?[0-9a-f]{1,8}$/i)));
}''',
    'IsInt': r'''($) {
    package Image::ExifTool;
    use strict;
    (return scalar(($_[0] =~ /^[+-]?\d+$/)));
}''',
    'IsRational': r'''($) {
    package Image::ExifTool;
    use strict;
    (return scalar(($_[0] =~ m[^[-+]?\d+/\d+$])));
}''',
    'Set16u': r'''(@) {
    package Image::ExifTool;
    use strict;
    (return DoPackStd('S', @_));
}''',
    'DoPackStd': r'''(@) {
    package Image::ExifTool;
    use strict;
    (my($val) = pack($Image::ExifTool::unpackStd{$_[0]}, $_[1]));
    ($_[2] and substr(${$_[2];}, $_[3], length($val)) = $val);
    (return $val);
}''',
    'SetRational64u': r'''(@) {
    package Image::ExifTool;
    use strict;
    (my($numer, $denom) = Rationalize($_[0], 4294967295));
    (my $val = Set32u($numer) . Set32u($denom));
    ($_[1] and substr(${$_[1];}, $_[2], length($val)) = $val);
    (return $val);
}''',
    'Rationalize': r'''($;$) {
    package Image::ExifTool;
    use strict;
    (my($val) = (shift()));
    (($val eq 'inf') and (return 1, 0));
    (($val eq 'undef') and (return 0, 0));
    (($val =~ m[^([-+]?\d+)/(\d+)$]) and (return $1, $2));
    (($val == 0) and (return 0, 1));
    (my($sign) = (($val < 0) ? (($val = (-$val)), (-1)) : 1));
    my($num, $denom, @fracs);
    (my($frac) = $val);
    (my($maxInt) = ((shift()) || 2147483647));
    while (1) {
        (my($n, $d) = AssembleRational(int(($frac + 0.5)), 1, @fracs));
        if ((($n > $maxInt) or ($d > $maxInt))) {
            (defined($num) and (last));
            (($val < 1) and (return $sign, $maxInt));
            (return ($sign * $maxInt), 1);
        }
        (($num, $denom) = ($n, $d));
        (my($err) = ((($n / $d) - $val) / $val));
        ((abs($err) < 1e-08) and (last));
        (my($int) = int($frac));
        unshift(@fracs, $int);
        (($frac -= $int) or (last));
        ($frac = (1 / $frac));
    }
    (return ($num * $sign), $denom);
}''',
    'AssembleRational': r'''($$@) {
    package Image::ExifTool;
    use strict;
    ((@_ < 3) and (return @_));
    (my($num, $denom, $frac) = splice(@_, 0, 3));
    (return AssembleRational((($frac * $num) + $denom), $num, @_));
}''',
    'Set32u': r'''(@) {
    package Image::ExifTool;
    use strict;
    (return DoPackStd('L', @_));
}''',
}


@dataclass(frozen=True)
class NumericWriteRecipe:
    """Proven count-one integer packing instructions, with no data mutation."""
    formats: tuple[str, ...]
    unsigned_bits: tuple[int, ...]


_NUMERIC_DEPENDENCIES = {
    'WriteValue': ('IsFloat', 'IsHex', 'IsInt', 'IsRational'),
    'Set16u': ('DoPackStd',),
    'Set32u': ('DoPackStd',),
    'SetRational64u': ('Rationalize', 'Set32u'),
    'Rationalize': ('AssembleRational',),
    'AssembleRational': ('AssembleRational',),
}
_WRITER_NUMERIC = {'WriteValue', 'SetRational64u', 'Rationalize', 'AssembleRational'}


def _numeric_body(source: Any) -> tuple[str, ...]:
    if not isinstance(source, str):
        raise RecipeRefused('numeric WriteValue source is unavailable')
    # No token stripping inside regexes, quoted strings or transliterations.
    # An inserted semicolon/statement or trailing source cannot disappear.
    return tuple(line.strip() for line in source.strip().splitlines())


def _numeric_mapping(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, Mapping):
        raise RecipeRefused(f'numeric WriteValue {label} is unavailable')
    return value


def _numeric_source(fact: Any, name: str, closure: Mapping[str, Any],
                    generic_closure: Mapping[str, Any]) -> Mapping[str, Any]:
    fact = _numeric_mapping(fact, name)
    source = 'Image/ExifTool/Writer.pl' if name in _WRITER_NUMERIC else 'Image/ExifTool.pm'
    digest = fact.get('source_sha256')
    import re
    if (fact.get('__perl') != 'CODE' or fact.get('resolved') is not True
            or fact.get('__name') != f'Image::ExifTool::{name}'
            or fact.get('source_file') != source
            or not isinstance(digest, str) or re.fullmatch(r'[0-9a-f]{64}', digest) is None
            or digest != closure.get(source) or digest != generic_closure.get(source)):
        raise RecipeRefused(f'numeric WriteValue {name} does not join the native source closures')
    return fact


def _numeric_function(fact: Any, name: str, closure: Mapping[str, Any],
                      generic_closure: Mapping[str, Any]) -> None:
    fact = _numeric_source(fact, name, closure, generic_closure)
    if _numeric_body(fact.get('__deparse')) != _numeric_body(_NUMERIC_BODIES[name]):
        raise RecipeRefused(f'numeric WriteValue dispatch/helper {name} is outside the fully consumed grammar')
    dependencies = _numeric_mapping(fact.get('dependencies', {}), f'{name} dependencies')
    expected = {f'Image::ExifTool::{dep}' for dep in _NUMERIC_DEPENDENCIES.get(name, ())}
    if set(dependencies) != expected:
        raise RecipeRefused(f'numeric WriteValue {name} dependencies do not match executable calls')
    for binding, dependency in dependencies.items():
        callee = binding.rsplit('::', 1)[1]
        if name == callee == 'AssembleRational':
            # The sole recursive edge targets this already-proven CV. For
            # admitted integers @fracs is empty and its first return wins.
            dependency = _numeric_mapping(dependency, 'AssembleRational recursive edge')
            if (dependency.get('__perl') != 'CODE' or dependency.get('__name') != binding
                    or dependency.get('resolved') is not False
                    or dependency.get('reason') != 'dependency_cycle'):
                raise RecipeRefused('numeric WriteValue recursive dependency is unsupported')
        else:
            _numeric_function(dependency, callee, closure, generic_closure)


def compile_numeric_write(helpers: Mapping[str, Any], closure: Mapping[str, Any],
                          generic_closure: Mapping[str, Any]) -> NumericWriteRecipe:
    """Compile the complete closed source graph for unsigned scalar packing.

    The validated WriteValue branches establish IsInt/IsFloat acceptance of
    decimal integer inputs and dispatch one value. Rationalize(0) returns 0/1;
    positive u32 integers pass AssembleRational's first return, satisfy the
    error bound on the first iteration, and return n/1. DoPackStd consumes the
    byte-order maps proven below. All other input types/ranges must refuse at
    runtime; this recipe never claims general Perl numeric compatibility.
    """
    write = _numeric_mapping(helpers.get('write_value'), 'helper')
    if write.get('requested_binding') != 'Image::ExifTool::WriteValue':
        raise RecipeRefused('numeric WriteValue requested binding is stale')
    _numeric_function(write, 'WriteValue', closure, generic_closure)
    hashes = _numeric_mapping(write.get('lexical_hashes'), 'lexical hashes')
    bindings = _numeric_mapping(hashes.get('bindings'), 'lexical bindings')
    dispatch = _numeric_mapping(bindings.get('%writeValueProc'), 'dispatch')
    if hashes.get('resolved') is not True or dispatch.get('resolved') is not True:
        raise RecipeRefused('numeric WriteValue lexical dispatch is unresolved')
    entries = _numeric_mapping(dispatch.get('entries'), 'dispatch entries')
    for format_name, name in (('int16u', 'Set16u'), ('int32u', 'Set32u'), ('rational64u', 'SetRational64u')):
        _numeric_function(entries.get(format_name), name, closure, generic_closure)

    # DoPackStd is not sufficient on its own: its pack template is mutable
    # state selected by SetByteOrder. The actual final lexical maps supply the
    # templates for the two runtime byte orders; never infer them from names.
    from native_reader_contract import _SET_BYTE_ORDER
    from native_reader_facts import body_tokens
    setter = _numeric_source(helpers.get('set_byte_order'), 'SetByteOrder', closure, generic_closure)
    if (setter.get('requested_binding') != 'Image::ExifTool::SetByteOrder'
            or body_tokens(setter.get('__deparse')) != body_tokens(_SET_BYTE_ORDER)
            or setter.get('dependencies', {})):
        raise RecipeRefused('numeric WriteValue byte-order setup is outside the fully consumed grammar')
    hashes = _numeric_mapping(setter.get('lexical_hashes'), 'byte-order lexical hashes')
    bindings = _numeric_mapping(hashes.get('bindings'), 'byte-order lexical bindings')
    if hashes.get('resolved') is not True:
        raise RecipeRefused('numeric WriteValue byte-order maps are unresolved')
    for name, expected in (('%unpackMotorola', {'S': 'n', 'L': 'N'}),
                           ('%unpackIntel', {'S': 'v', 'L': 'V'})):
        captured = _numeric_mapping(bindings.get(name), name)
        entries = _numeric_mapping(captured.get('entries'), name + ' entries')
        if captured.get('resolved') is not True or any(entries.get(k) != v for k, v in expected.items()):
            raise RecipeRefused('numeric WriteValue byte-order packing templates are unsupported')
    return NumericWriteRecipe(('int16u', 'int32u', 'rational64u'), (16, 32))
