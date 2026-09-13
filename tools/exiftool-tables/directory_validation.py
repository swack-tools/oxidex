"""Compile source-authenticated directory size checks, without tag-name rules.

The call supplies the offset and expected sizes. The callee must have the
closed, side-effect-free body below: read a u16, compare it with each supplied
argument, return true on equality. Missing source is a refusal, never a reason
to trust a function name. B::Deparse's -p form preserves operator grouping.
"""

from dataclasses import dataclass
import re
from pathlib import PurePosixPath


class ValidationRefused(ValueError):
    pass


_FQ = r"(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*"
_TOKEN = re.compile(r"\s+|(?:[A-Za-z_]\w*::)+[A-Za-z_]\w*|[$@][A-Za-z_]\w*|[A-Za-z_]\w*|\d+|==|[(){};,=&$@]")


def _tokens(source):
    if not isinstance(source, str):
        raise ValidationRefused("missing helper body")
    out, at = [], 0
    while at < len(source):
        match = _TOKEN.match(source, at)
        if match is None:
            raise ValidationRefused("helper syntax outside the closed grammar")
        if not match.group().isspace():
            out.append(match.group())
        at = match.end()
    return out


class _Body:
    def __init__(self, source):
        self.tokens = _tokens(source)
        self.at = 0

    def take(self, *wanted):
        if self.tokens[self.at:self.at + len(wanted)] != list(wanted):
            raise ValidationRefused("helper body is not a u16 membership check")
        self.at += len(wanted)

    def variable(self, sigil):
        if self.at >= len(self.tokens) or re.fullmatch(re.escape(sigil) + r"[A-Za-z_]\w*", self.tokens[self.at]) is None:
            raise ValidationRefused("helper binder is not a local variable")
        value = self.tokens[self.at]
        self.at += 1
        return value

    def scalar_declaration(self):
        # Perl 5.34 and 5.38 differ only in the parentheses around `my $x`.
        self.take("my")
        wrapped = self.tokens[self.at:self.at + 1] == ["("]
        if wrapped:
            self.take("(")
        name = self.variable("$")
        if wrapped:
            self.take(")")
        return name


def authenticate_body(source):
    """Recognize the body structurally, preserving bindings and grouping."""
    body = _Body(source)
    body.take("(", "$", "$", "@", ")", "{", "package")
    package = body.tokens[body.at] if body.at < len(body.tokens) else ""
    if re.fullmatch(_FQ, package) is None:
        raise ValidationRefused("missing helper package")
    body.at += 1
    body.take(";", "use", "strict", ";", "(", "my", "(")
    data = body.variable("$")
    body.take(",")
    offset = body.variable("$")
    body.take(",")
    choices = body.variable("@")
    body.take(")", "=", "@_", ")", ";", "(")
    read = body.scalar_declaration()
    body.take("=", "&", "Image::ExifTool::Get16u", "(", data, ",", offset, ")", ")", ";")
    item = body.scalar_declaration()
    body.take(";", "foreach", item, "(", choices, ")", "{", "(", "(", item,
              "==", read, ")", "and", "(", "return", "1", ")", ")", ";", "}",
              "(", "return", "(", "undef", ")", ")", ";", "}")
    if body.at != len(body.tokens) or len({data, offset, read, item}) != 4:
        raise ValidationRefused("helper has extra statements or aliased local bindings")


@dataclass(frozen=True)
class CompiledValidation:
    offset: int
    expected: tuple[tuple[str, int], ...]
    expression: str
    callee: str
    source_file: str
    source_sha256: str

    def rust(self, escape):
        values = ", ".join(f"SizeExpectation::{kind}({value})" for kind, value in self.expected)
        return (
            "Some(U16SizeCheck { "
            f"offset: {self.offset}, expected: &[{values}], "
            f'expression: "{escape(self.expression)}", '
            f'callee: "{escape(self.callee)}", source_file: "{escape(self.source_file)}", '
            f'source_sha256: "{self.source_sha256}" }})'
        )


def _integer(text, *, maximum):
    if re.fullmatch(r"(?:0[xX][0-9a-fA-F]+|0|[1-9][0-9]*)", text) is None:
        raise ValidationRefused("nonliteral size operand")
    value = int(text, 16 if text.lower().startswith("0x") else 10)
    if value > maximum:
        raise ValidationRefused("size operand outside the exact integer domain")
    return value


def compile_validation(expression, helpers):
    if not isinstance(expression, str):
        raise ValidationRefused("validation is not a source expression")
    match = re.fullmatch(r"\s*(" + _FQ + r")\s*\(([^()]*)\)\s*", expression)
    if match is None:
        raise ValidationRefused("validation is not one direct helper call")
    name, arguments = match.groups()
    args = [arg.strip() for arg in arguments.split(",")]
    if len(args) < 3 or args[0] != "$dirData":
        raise ValidationRefused("validation requires local data and expected sizes")
    start = re.fullmatch(r"\$subdirStart(?:\s*\+\s*(.+))?", args[1])
    if start is None:
        raise ValidationRefused("validation offset is outside the local start domain")
    offset = 0 if start.group(1) is None else _integer(start.group(1), maximum=0xffffffff)
    expected = []
    for arg in args[2:]:
        if arg == "$size":
            expected.append(("Relative", 0))
        elif (relative := re.fullmatch(r"\$size\s*([+-])\s*(.+)", arg)) is not None:
            delta = _integer(relative.group(2), maximum=0x7fffffff)
            expected.append(("Relative", delta if relative.group(1) == "+" else -delta))
        elif (quotient := re.fullmatch(r"\$size\s*/\s*(.+)", arg)) is not None:
            divisor = _integer(quotient.group(1), maximum=0xffffffff)
            if divisor == 0:
                raise ValidationRefused("zero divisor")
            expected.append(("Quotient", divisor))
        else:
            expected.append(("Constant", _integer(arg, maximum=0xffffffff)))
    helper = helpers.get(name) if isinstance(helpers, dict) else None
    if (not isinstance(helper, dict) or helper.get("__perl") != "CODE"
            or helper.get("resolved") is not True):
        raise ValidationRefused("validation helper source is unavailable")
    # An alias is fine: the captured body, not either function name, decides
    # whether the primitive is sound. The oracle authenticates file provenance.
    authenticate_body(helper.get("__deparse"))
    file = helper.get("source_file")
    sha = helper.get("source_sha256")
    if (not isinstance(file, str) or not file or PurePosixPath(file).is_absolute()
            or any(part in {".", ".."} for part in file.split("/"))
            or PurePosixPath(file).as_posix() != file or "\\" in file
            or not isinstance(sha, str) or re.fullmatch(r"[0-9a-f]{64}", sha) is None):
        raise ValidationRefused("helper source provenance is unavailable")
    return CompiledValidation(offset, tuple(expected), expression, name, file, sha)
