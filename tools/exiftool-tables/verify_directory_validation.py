"""Independent artifact/call-argument audit for directory size checks.

No generator import. Native expressions and source-file digests arrive from
oracle.pl; expression operands are read through Python's AST, independently
of the compiler's closed call recognizer.
"""
import ast
import re


def parse_rust(text, unescape):
    if text == "None":
        return None
    string = r'"((?:[^"\\]|\\.)*)"'
    pattern = (
        r'Some\(U16SizeCheck\s*\{\s*offset:\s*(\d+),\s*expected:\s*&\[([^\]]*)\],\s*'
        + r'expression:\s*' + string + r',\s*callee:\s*' + string
        + r',\s*source_file:\s*' + string + r',\s*source_sha256:\s*' + string
        + r',\s*reader_contract_sha256:\s*(None|Some\(\s*"[0-9a-f]{64}"\s*,?\s*\))'
        + r'\s*,?\s*\}\s*,?\)'
    )
    match = re.fullmatch(pattern, text, re.S)
    if match is None:
        raise SystemExit("unrecognised directory validation schema")
    offset, values, expression, callee, file, sha, reader = match.groups()
    expected, at = [], 0
    token = re.compile(r"\s*SizeExpectation::(Relative|Constant|Quotient)\((-?\d+)\)\s*(?:,|$)")
    while at < len(values):
        part = token.match(values, at)
        if part is None:
            if not values[at:].strip():
                break
            raise SystemExit("unparsed directory size expectation")
        kind, raw = part.groups()
        value = int(raw)
        if ((kind == "Relative" and not -(1 << 31) <= value < (1 << 31))
                or (kind != "Relative" and not 0 <= value <= 0xffffffff)
                or (kind == "Quotient" and value == 0)):
            raise SystemExit("directory size expectation outside the native integer domain")
        expected.append((kind, value))
        at = part.end()
    if not expected or int(offset) > 0xffffffff:
        raise SystemExit("empty or unrepresentable directory size check")
    return (int(offset), tuple(expected), unescape(expression), unescape(callee),
            unescape(file), unescape(sha),
            None if reader == "None" else re.search(r'"([0-9a-f]{64})"', reader).group(1))


def _operand(source):
    # Only the two exact lexical variables exist in this eval scope. Unknown
    # Perl syntax, including an unconverted $, fails ast.parse below.
    source = re.sub(r"\$(size|subdirStart)\b", r"\1", source)
    return ast.parse(source.strip(), mode="eval").body


def _constant(node):
    if isinstance(node, ast.Constant) and type(node.value) is int:
        return node.value
    raise ValueError("not an integer constant")


def call_arguments(expression):
    match = re.fullmatch(r"\s*((?:[A-Za-z_]\w*::)+[A-Za-z_]\w*)\s*\(([^()]*)\)\s*", expression)
    if match is None:
        raise ValueError("not a direct native validation call")
    callee, operands = match.groups()
    args = operands.split(",")
    if len(args) < 3 or args[0].strip() != "$dirData":
        raise ValueError("validation data is outside the local scope")
    start = _operand(args[1])
    if isinstance(start, ast.Name) and start.id == "subdirStart":
        offset = 0
    elif (isinstance(start, ast.BinOp) and isinstance(start.op, ast.Add)
          and isinstance(start.left, ast.Name) and start.left.id == "subdirStart"):
        offset = _constant(start.right)
    else:
        raise ValueError("unmodeled native offset")
    expected = []
    for source in args[2:]:
        value = _operand(source)
        if isinstance(value, ast.Name) and value.id == "size":
            expected.append(("Relative", 0))
        elif isinstance(value, ast.Constant):
            expected.append(("Constant", _constant(value)))
        elif (isinstance(value, ast.BinOp) and isinstance(value.left, ast.Name)
              and value.left.id == "size"):
            other = _constant(value.right)
            if isinstance(value.op, ast.Add):
                expected.append(("Relative", other))
            elif isinstance(value.op, ast.Sub):
                expected.append(("Relative", -other))
            elif isinstance(value.op, ast.Div) and other > 0:
                expected.append(("Quotient", other))
            else:
                raise ValueError("unmodeled native size operation")
        else:
            raise ValueError("unmodeled native expected size")
    return offset, tuple(expected), callee


def mismatch(compiled, native):
    if not native:
        return "compiled validation lacks independent native helper facts"
    expression, callee, file, sha = native
    if not file or re.fullmatch(r"[0-9a-f]{64}", sha) is None:
        return "native helper source is unavailable"
    try:
        offset, expected, called = call_arguments(expression)
    except (ValueError, SyntaxError):
        return "compiled validation claims an unmodeled native call"
    if compiled[:2] != (offset, expected) or compiled[3:6] != (called, file, sha) or callee != called:
        return "compiled validation operands or helper source differ from native"
    if re.sub(r"[\t\r\n]+", " ", compiled[2]) != expression:
        return "compiled validation expression differs from native"
    return None
