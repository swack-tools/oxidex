#!/usr/bin/env python3
r"""SPIKE (measurement only -- not a production path): a recursive-descent
parser for the Perl subset ExifTool's embedded expressions are written in.

Purpose: answer "if oxidex parsed every `Condition` / `RawConv` / `ValueConv`
/ `PrintConv` / `*Inv` into an AST and evaluated it with an interpreter, how
much would that cover?" -- with numbers, before anyone commits to the
rewrite. This file PARSES (produces an AST) and REFUSES cleanly on anything
outside the grammar; it does not evaluate.

Doctrine (AGENTS.md, "Never approximate a conversion"): a construct that is
not recognised raises `Refuse` and the whole expression is counted refused.
Nothing is skipped, guessed at, or partially parsed. Parse success here is a
CEILING on interpreter coverage, never a claim of correct evaluation -- the
census script that consumes this file says so in every table it prints.

Feature flags: every production records the feature name it needed on the
parse (`Parser.features`). A parse under "all features" therefore yields the
exact set of productions an expression requires, so the growth ladder ("which
production unlocked how many new uses") is computed combinatorially from one
pass rather than by re-parsing under N flag subsets.

Deliberately NOT based on `native_reader_facts.body_tokens` (AGENTS.md
warning: it reads a Perl `/` as a regex literal and folds long spans into one
token) -- this is a fresh lexer whose `/` disambiguation is driven by the
parser's term/operator position, as Perl's own is.
"""

import re

# ---------------------------------------------------------------------------
# refusal
# ---------------------------------------------------------------------------


class Refuse(Exception):
    """This construct is outside the grammar. Never partially parsed."""

    def __init__(self, reason, pos=None):
        super().__init__(reason)
        self.reason = reason
        self.pos = pos


# ---------------------------------------------------------------------------
# lexer
# ---------------------------------------------------------------------------

_IDENT = re.compile(r"[A-Za-z_]\w*(?:::[A-Za-z_]\w*)*")
_NUM = re.compile(
    r"0[xX][0-9a-fA-F_]+"
    r"|0[bB][01_]+"
    r"|\d[\d_]*\.(?!\.)[\d_]*(?:[eE][+-]?\d+)?"
    r"|\.\d[\d_]*(?:[eE][+-]?\d+)?"
    r"|\d[\d_]*(?:[eE][+-]?\d+)?"
)
_SPECIAL_VAR_PUNCT = set("&`'+!@/\\,;.^-|?:\"<>[]()_0123456789$")
_CLOSERS = {"(": ")", "[": "]", "{": "}", "<": ">"}

QUOTE_OPS = {"q", "qq", "qw", "qr", "m", "s", "tr", "y"}

# Perl operators, longest first.
_OPS = [
    "<=>", "**=", "||=", "&&=", "//=", "...", "<<=", ">>=",
    "->", "++", "--", "**", "=~", "!~", "==", "!=", "<=", ">=", "&&", "||",
    "//", "..", "<<", ">>", "+=", "-=", "*=", "/=", ".=", "%=", "|=", "&=",
    "^=", "=>", "::",
    "+", "-", "*", "/", "%", ".", "<", ">", "=", "!", "~", "?", ":", ",",
    ";", "(", ")", "[", "]", "{", "}", "&", "|", "^", "\\",
]

WORD_OPS = {"lt", "gt", "le", "ge", "eq", "ne", "cmp", "and", "or", "xor",
            "x", "isa"}

NAMED_UNARY = {
    "defined", "ref", "lc", "uc", "lcfirst", "ucfirst", "length", "int",
    "abs", "hex", "oct", "ord", "chr", "sqrt", "log", "exp", "sin", "cos",
    "quotemeta", "chomp", "chop", "scalar", "exists", "delete", "undef",
    "pos", "rand", "srand", "fc", "each", "keys", "values", "shift", "pop",
    "localtime", "gmtime", "require", "caller", "sprintf", "lock", "study",
}
# sprintf/join/... are list operators, not named unaries; keep them out.
NAMED_UNARY -= {"sprintf"}

LIST_OPS = {
    "sprintf", "printf", "print", "join", "split", "push", "unshift",
    "splice", "reverse", "sort", "map", "grep", "pack", "unpack", "die",
    "warn", "return", "substr", "index", "rindex", "sprintf", "atan2",
    "wantarray", "bless", "open", "close", "read", "seek", "binmode",
    "exists", "sprintf",
}

# Perl builtins an interpreter gets "for free" (core semantics, no ExifTool
# knowledge). Anything called that is NOT here is an ExifTool helper sub.
CORE_BUILTINS = NAMED_UNARY | LIST_OPS | {
    "abs", "atan2", "chr", "cos", "exp", "hex", "index", "int", "join",
    "keys", "lc", "lcfirst", "length", "log", "oct", "ord", "pack", "pop",
    "push", "rand", "reverse", "rindex", "shift", "sin", "sort", "splice",
    "split", "sprintf", "sqrt", "substr", "uc", "ucfirst", "unpack",
    "unshift", "values", "wantarray", "defined", "ref", "scalar", "exists",
    "delete", "undef", "die", "warn", "map", "grep", "sprintf", "printf",
    "print", "localtime", "gmtime", "time", "each", "chomp", "chop", "pos",
    "quotemeta", "bless", "require", "eval", "do", "sprintf", "caller",
    "exit", "sleep", "uc",
}


class Token:
    __slots__ = ("kind", "val", "start", "end", "extra")

    def __init__(self, kind, val, start, end, extra=None):
        self.kind = kind
        self.val = val
        self.start = start
        self.end = end
        self.extra = extra

    def __repr__(self):  # pragma: no cover - debugging aid
        return f"Token({self.kind!r},{self.val!r})"


class Lexer:
    def __init__(self, src):
        self.src = src
        self.n = len(src)

    def skip_ws(self, i):
        src, n = self.src, self.n
        while i < n:
            c = src[i]
            if c in " \t\r\n\f":
                i += 1
            elif c == "#":
                j = src.find("\n", i)
                i = n if j < 0 else j + 1
            else:
                break
        return i

    def read_delimited(self, i, opener):
        """Read a quote-like body starting AFTER the opening delimiter.

        Returns (body, index_after_closing_delimiter)."""
        src, n = self.src, self.n
        closer = _CLOSERS.get(opener, opener)
        nest = 1
        out = []
        while i < n:
            c = src[i]
            if c == "\\":
                if i + 1 >= n:
                    raise Refuse("unterminated escape in quote-like", i)
                out.append(src[i:i + 2])
                i += 2
                continue
            if closer != opener and c == opener:
                nest += 1
            elif c == closer:
                nest -= 1
                if nest == 0:
                    return "".join(out), i + 1
            out.append(c)
            i += 1
        raise Refuse("unterminated quote-like construct", i)

    def token(self, i, term):
        """Lex one token at i. `term` is True when a TERM is expected (which
        is what decides `/` = regex vs divide, `%` = hash vs modulus, ...)."""
        src, n = self.src, self.n
        i = self.skip_ws(i)
        if i >= n:
            return Token("eof", None, i, i)
        c = src[i]

        # numbers
        if c.isdigit() or (term and c == "." and i + 1 < n and src[i + 1].isdigit()):
            m = _NUM.match(src, i)
            if m:
                return Token("num", m.group(0), i, m.end())

        # strings
        if c == "'":
            body, j = self.read_delimited(i + 1, "'")
            return Token("str", body, i, j, "'")
        if c == '"':
            body, j = self.read_delimited(i + 1, '"')
            return Token("dq", body, i, j, '"')
        if c == "`":
            raise Refuse("backtick (shell) string", i)

        # variables / casts
        if c in "$@%&" and term or c in "$@":
            t = self._var_token(i, term)
            if t is not None:
                return t

        if c == "/" and term:
            body, j = self.read_delimited(i + 1, "/")
            flags, j = self._flags(j)
            return Token("quote", "m", i, j, (body, None, flags, "/"))

        if c == "<" and term:
            raise Refuse("readline/glob <...>", i)

        # identifiers, word operators, quote-like operators
        m = _IDENT.match(src, i)
        if m:
            word = m.group(0)
            j = m.end()
            if not term and word == "x":
                if j < n and src[j] == "=" and src[j:j + 2] != "==":
                    return Token("op", "x=", i, j + 1)
                return Token("op", "x", i, j)
            if not term and re.fullmatch(r"x\d+", word):
                return Token("op", "x", i, i + 1)
            if not term and word in WORD_OPS:
                return Token("op", word, i, j)
            if term and word in QUOTE_OPS:
                k = self.skip_ws(j)
                if k < n and not src[k].isalnum() and src[k] not in " \t\r\n" \
                        and src[k] != "_" and src[k:k + 2] != "=>" \
                        and src[k] not in ",;)}]":
                    return self._quote_like(i, word, k)
            if word in ("__END__", "__DATA__"):
                raise Refuse("__END__/__DATA__", i)
            return Token("ident", word, i, j)

        for op in _OPS:
            if src.startswith(op, i):
                return Token("op", op, i, i + len(op))
        raise Refuse(f"unlexable character {c!r}", i)

    def _flags(self, i):
        m = re.compile(r"[a-zA-Z]*").match(self.src, i)
        return m.group(0), m.end()

    def _quote_like(self, start, word, k):
        src = self.src
        opener = src[k]
        body, j = self.read_delimited(k + 1, opener)
        if word in ("s", "tr", "y"):
            if opener in _CLOSERS:
                j = self.skip_ws(j)
                if j >= self.n:
                    raise Refuse("unterminated s///", j)
                opener2 = src[j]
                repl, j = self.read_delimited(j + 1, opener2)
            else:
                repl, j = self.read_delimited(j, opener)
            flags, j = self._flags(j)
            kind = "s" if word == "s" else "tr"
            return Token("quote", kind, start, j, (body, repl, flags, opener))
        flags, j = self._flags(j) if word in ("m", "qr") else ("", j)
        return Token("quote", word, start, j, (body, None, flags, opener))

    def _var_token(self, i, term):
        src, n = self.src, self.n
        c = src[i]
        j = i + 1
        if j >= n:
            raise Refuse("dangling sigil", i)
        nxt = src[j]
        if c == "%" and not term:
            return None
        if c == "&" and (not term or nxt in "&="):
            return None
        if c == "$" and nxt == "#":
            j += 1
            if j < n and (src[j] in "${"):
                return Token("cast", "$#", i, j)
            m = _IDENT.match(src, j)
            if m:
                return Token("var", ("$#", m.group(0)), i, m.end())
            return Token("var", ("$", "#"), i, j)
        if nxt in "${":
            if nxt == "{":
                m = re.compile(r"\{\s*(\^?\w+)\s*\}").match(src, j)
                if m and c != "&":
                    return Token("var", (c, m.group(1)), i, m.end())
            return Token("cast", c, i, j)
        if nxt == "^" and j + 1 < n and src[j + 1].isupper():
            return Token("var", (c, "^" + src[j + 1]), i, j + 2)
        m = _IDENT.match(src, j)
        if m:
            return Token("var", (c, m.group(0)), i, m.end())
        if c == "$" and nxt in _SPECIAL_VAR_PUNCT:
            return Token("var", ("$", nxt), i, j + 1)
        if c == "@" and nxt in "-+_":
            return Token("var", ("@", nxt), i, j + 1)
        if c == "%" and nxt in "-+":
            return Token("var", ("%", nxt), i, j + 1)
        if c in "%&":
            return None
        raise Refuse(f"unparsable variable at sigil {c!r}", i)


# ---------------------------------------------------------------------------
# interpolation scanner (double-quoted strings and regex patterns)
# ---------------------------------------------------------------------------

_INTERP_START = re.compile(r"[\$@]")


def _balanced(text, i, opener):
    closer = _CLOSERS[opener]
    nest = 0
    while i < len(text):
        c = text[i]
        if c == "\\":
            i += 2
            continue
        if c == opener:
            nest += 1
        elif c == closer:
            nest -= 1
            if nest == 0:
                return i + 1
        i += 1
    return None


def scan_interpolations(text, in_regex=False):
    """Return (literal_chunks, [expression_source, ...]) for a double-quoted
    body or regex pattern. Raises Refuse on a sigil it cannot resolve into
    either an interpolation or a literal."""
    exprs = []
    i = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == "\\":
            i += 2
            continue
        if c not in "$@":
            i += 1
            continue
        j = i + 1
        if j >= n:
            if in_regex or c == "@":
                i += 1
                continue
            raise Refuse("trailing sigil in string")
        nxt = text[j]
        if in_regex and c == "$" and (nxt in ")|" or nxt == "$" and j + 1 >= n):
            i += 1  # end-of-line anchor
            continue
        if nxt in " \t\n=,;%<>/*+-.?!~^&'\"" or (in_regex and nxt in "[]{}"):
            if c == "@" or in_regex:
                i += 1
                continue
            raise Refuse("unrecognised sigil use in string")
        start = i
        # sigil chain: $$x, @$x
        while j < n and text[j] == "$":
            j += 1
        if j < n and text[j] == "{":
            end = _balanced(text, j, "{")
            if end is None:
                raise Refuse("unbalanced ${...} in string")
            j = end
        else:
            m = _IDENT.match(text, j)
            if not m:
                m2 = re.compile(r"\d+").match(text, j)
                if m2:
                    j = m2.end()
                elif in_regex:
                    i += 1
                    continue
                else:
                    raise Refuse("unrecognised interpolation")
            else:
                j = m.end()
        # subscript chain
        while j < n:
            if text[j] == "[" and not (in_regex and text[start] == "@"):
                end = _balanced(text, j, "[")
                if end is None:
                    break
                inner = text[j + 1:end - 1]
                if in_regex and not re.fullmatch(r"[\s\$\w\+\-\*/\.\[\]{}'\"]*", inner):
                    break  # a character class, not a subscript
                j = end
            elif text[j] == "{":
                end = _balanced(text, j, "{")
                if end is None:
                    break
                j = end
            elif text.startswith("->", j) and j + 2 < n and text[j + 2] in "[{":
                end = _balanced(text, j + 2, text[j + 2])
                if end is None:
                    break
                j = end
            else:
                break
        exprs.append(text[start:j])
        i = j
    return exprs


# ---------------------------------------------------------------------------
# parser
# ---------------------------------------------------------------------------

ASSIGN_OPS = {"=", "+=", "-=", "*=", "/=", ".=", "%=", "x=", "**=", "&=",
              "|=", "^=", "<<=", ">>=", "&&=", "||=", "//="}

EQUALITY = {"==", "!=", "<=>", "eq", "ne", "cmp"}
RELATIONAL = {"<", ">", "<=", ">=", "lt", "gt", "le", "ge"}
SHIFT = {"<<", ">>"}
ADDITIVE = {"+", "-", "."}
MULTIPLICATIVE = {"*", "/", "%", "x"}

TERM_START_OPS = {"(", "[", "\\", "!", "~", "-", "+", "{", "++", "--"}


class Parser:
    def __init__(self, src, feature_sink=None):
        self.lx = Lexer(src)
        self.src = src
        self.pos = 0
        self.features = set() if feature_sink is None else feature_sink
        self._peek_cache = {}
        self.depth = 0

    # -- plumbing ---------------------------------------------------------
    def need(self, feature):
        self.features.add(feature)

    def peek(self, term=False):
        key = (self.pos, term)
        tok = self._peek_cache.get(key)
        if tok is None:
            tok = self.lx.token(self.pos, term)
            self._peek_cache[key] = tok
        return tok

    def next(self, term=False):
        tok = self.peek(term)
        self.pos = tok.end
        return tok

    def at_op(self, *vals, term=False):
        t = self.peek(term)
        return t.kind == "op" and t.val in vals

    def at_ident(self, *vals):
        t = self.peek()
        return t.kind == "ident" and t.val in vals

    def eat_op(self, val, term=False):
        t = self.peek(term)
        if t.kind == "op" and t.val == val:
            self.pos = t.end
            return True
        return False

    def expect_op(self, val, term=False):
        t = self.peek(term)
        if not (t.kind == "op" and t.val == val):
            raise Refuse(f"expected {val!r}, found {t.kind}:{t.val!r}", t.start)
        self.pos = t.end

    # -- program / statements --------------------------------------------
    def parse_program(self):
        stmts = []
        while self.peek(True).kind != "eof":
            s = self.parse_statement()
            if s is not None:
                stmts.append(s)
        if len(stmts) > 1:
            self.need("stmt_sequence")
        return ("program", stmts)

    def parse_block(self):
        self.expect_op("{", term=False)
        stmts = []
        while True:
            t = self.peek(True)
            if t.kind == "eof":
                raise Refuse("unterminated block", t.start)
            if t.kind == "op" and t.val == "}":
                self.pos = t.end
                break
            s = self.parse_statement()
            if s is not None:
                stmts.append(s)
        return ("block", stmts)

    def parse_statement(self):
        self.depth += 1
        if self.depth > 200:
            raise Refuse("recursion limit")
        try:
            return self._parse_statement()
        finally:
            self.depth -= 1

    def _parse_statement(self):
        t = self.peek(True)
        if t.kind == "op" and t.val == ";":
            self.pos = t.end
            return None
        if t.kind == "op" and t.val == "{":
            self.need("bare_block")
            return self.parse_block()
        if t.kind == "ident":
            word = t.val
            if word in ("if", "unless"):
                self.need("if_block")
                return self.parse_if()
            if word in ("while", "until"):
                self.need("loop")
                self.pos = t.end
                self.expect_op("(")
                cond = self.parse_expr()
                self.expect_op(")")
                body = self.parse_block()
                return ("while", word, cond, body)
            if word in ("for", "foreach"):
                self.need("loop")
                return self.parse_for()
            if word == "package":
                self.need("deparse_wrapper")
                self.pos = t.end
                name = self.next()
                if name.kind != "ident":
                    raise Refuse("package without a name", name.start)
                if self.at_op("{"):
                    return ("package_block", name.val, self.parse_block())
                self.expect_op(";")
                return ("package", name.val)
            if word in ("use", "no"):
                self.need("deparse_wrapper")
                self.pos = t.end
                while True:
                    tok = self.next(True)
                    if tok.kind == "eof":
                        raise Refuse("unterminated use statement", tok.start)
                    if tok.kind == "op" and tok.val == ";":
                        break
                return ("use",)
            if word == "sub":
                after = self.lx.token(t.end, False)
                if after.kind == "ident":
                    raise Refuse("named sub declaration", t.start)
            if word == "BEGIN":
                raise Refuse("BEGIN block", t.start)
        expr = self.parse_expr()
        # statement modifiers
        t = self.peek()
        if t.kind == "ident" and t.val in ("if", "unless", "while", "until",
                                           "for", "foreach"):
            self.need("stmt_modifier")
            self.pos = t.end
            cond = self.parse_expr()
            expr = ("modifier", t.val, expr, cond)
        t = self.peek()
        if t.kind == "op" and t.val == ";":
            self.pos = t.end
        elif t.kind == "eof" or (t.kind == "op" and t.val == "}"):
            pass
        else:
            raise Refuse(f"unterminated statement before {t.kind}:{t.val!r}", t.start)
        return expr

    def parse_if(self):
        t = self.next()
        clauses = []
        kind = t.val
        self.expect_op("(")
        cond = self.parse_expr()
        self.expect_op(")")
        body = self.parse_block()
        clauses.append((kind, cond, body))
        els = None
        while True:
            t = self.peek()
            if t.kind == "ident" and t.val == "elsif":
                self.pos = t.end
                self.expect_op("(")
                c = self.parse_expr()
                self.expect_op(")")
                clauses.append(("elsif", c, self.parse_block()))
                continue
            if t.kind == "ident" and t.val == "else":
                self.pos = t.end
                els = self.parse_block()
            break
        return ("if", clauses, els)

    def parse_for(self):
        self.next()
        t = self.peek(True)
        loopvar = None
        if t.kind == "ident" and t.val in ("my", "our"):
            self.pos = t.end
            self.need("my")
            v = self.next(True)
            if v.kind != "var":
                raise Refuse("foreach my without a variable", v.start)
            loopvar = ("my", [v.val])
        elif t.kind == "var":
            self.pos = t.end
            loopvar = ("var",) + tuple(t.val)
        self.expect_op("(")
        if self.at_op(";", term=True):
            init = None
        else:
            init = self.parse_expr()
        if self.at_op(";"):
            self.need("c_style_for")
            self.expect_op(";")
            cond = None if self.at_op(";", term=True) else self.parse_expr()
            self.expect_op(";")
            step = None if self.at_op(")", term=True) else self.parse_expr()
            self.expect_op(")")
            return ("cfor", init, cond, step, self.parse_block())
        self.expect_op(")")
        return ("foreach", loopvar, init, self.parse_block())

    # -- expressions ------------------------------------------------------
    def parse_expr(self):
        self.depth += 1
        if self.depth > 200:
            raise Refuse("recursion limit")
        try:
            return self.parse_low_or()
        finally:
            self.depth -= 1

    def parse_low_or(self):
        left = self.parse_low_and()
        while True:
            t = self.peek()
            if t.kind == "op" and t.val in ("or", "xor"):
                self.need("logic")
                self.pos = t.end
                left = ("binop", t.val, left, self.parse_low_and())
            else:
                return left

    def parse_low_and(self):
        left = self.parse_low_not()
        while True:
            t = self.peek()
            if t.kind == "op" and t.val == "and":
                self.need("logic")
                self.pos = t.end
                left = ("binop", "and", left, self.parse_low_not())
            else:
                return left

    def parse_low_not(self):
        t = self.peek(True)
        if t.kind == "ident" and t.val == "not":
            self.need("logic")
            self.pos = t.end
            return ("unop", "not", self.parse_low_not())
        return self.parse_comma()

    def parse_comma(self):
        first = self.parse_assign()
        items = None
        while True:
            t = self.peek()
            if t.kind == "op" and t.val in (",", "=>"):
                self.need("list_expr")
                self.pos = t.end
                if items is None:
                    items = [first]
                nt = self.peek(True)
                if nt.kind == "eof" or (nt.kind == "op" and nt.val in ")]};"):
                    break
                if nt.kind == "ident" and nt.val in ("if", "unless", "for",
                                                     "foreach", "while"):
                    break
                items.append(self.parse_assign())
            else:
                break
        return first if items is None else ("list", items)

    def parse_assign(self):
        left = self.parse_ternary()
        t = self.peek()
        if t.kind == "op" and t.val in ASSIGN_OPS:
            self.need("assign")
            self.pos = t.end
            return ("assign", t.val, left, self.parse_assign())
        return left

    def parse_ternary(self):
        cond = self.parse_range()
        if self.at_op("?"):
            self.need("ternary")
            self.expect_op("?")
            a = self.parse_assign()
            self.expect_op(":")
            b = self.parse_assign()
            return ("ternary", cond, a, b)
        return cond

    def parse_range(self):
        left = self.parse_oror()
        t = self.peek()
        if t.kind == "op" and t.val in ("..", "..."):
            self.need("range")
            self.pos = t.end
            return ("binop", t.val, left, self.parse_oror())
        return left

    def _binary_level(self, ops, sub, feature):
        left = sub()
        while True:
            t = self.peek()
            if t.kind == "op" and t.val in ops:
                self.need(feature(t.val) if callable(feature) else feature)
                self.pos = t.end
                left = ("binop", t.val, left, sub())
            else:
                return left

    def parse_oror(self):
        return self._binary_level({"||", "//"}, self.parse_andand, "logic")

    def parse_andand(self):
        return self._binary_level({"&&"}, self.parse_bitor, "logic")

    def parse_bitor(self):
        return self._binary_level({"|", "^"}, self.parse_bitand, "bitops")

    def parse_bitand(self):
        return self._binary_level({"&"}, self.parse_equality, "bitops")

    def parse_equality(self):
        return self._binary_level(
            EQUALITY, self.parse_relational,
            lambda op: "strcmp" if op in ("eq", "ne", "cmp") else "compare")

    def parse_relational(self):
        return self._binary_level(
            RELATIONAL, self.parse_uni,
            lambda op: "strcmp" if op in ("lt", "gt", "le", "ge") else "compare")

    def parse_uni(self):
        return self.parse_shift()

    def parse_shift(self):
        return self._binary_level(SHIFT, self.parse_additive, "bitops")

    def parse_additive(self):
        return self._binary_level(
            ADDITIVE, self.parse_multiplicative,
            lambda op: "concat" if op == "." else "arith")

    def parse_multiplicative(self):
        return self._binary_level(
            MULTIPLICATIVE, self.parse_bind,
            lambda op: "concat" if op == "x" else "arith")

    def parse_bind(self):
        left = self.parse_unary()
        while True:
            t = self.peek()
            if t.kind == "op" and t.val in ("=~", "!~"):
                self.pos = t.end
                rhs = self.parse_unary()
                if rhs[0] not in ("regex", "subst", "tr"):
                    self.need("regex_runtime")
                left = ("bind", t.val, left, rhs)
            else:
                return left

    def parse_unary(self):
        t = self.peek(True)
        if t.kind == "op" and t.val in ("!", "~", "\\", "-", "+"):
            self.pos = t.end
            if t.val == "\\":
                self.need("ref")
            elif t.val in ("!", "~"):
                self.need("logic" if t.val == "!" else "bitops")
            else:
                self.need("arith")
            if t.val == "-":
                nxt = self.peek(True)
                if nxt.kind == "ident" and nxt.val not in QUOTE_OPS \
                        and nxt.val not in CORE_BUILTINS:
                    after = self.lx.token(nxt.end, False)
                    if after.kind == "op" and after.val in ("=>", ",", "}"):
                        self.pos = nxt.end
                        return ("str", "-" + nxt.val)
            return ("unop", t.val, self.parse_unary())
        return self.parse_pow()

    def parse_pow(self):
        base = self.parse_incdec()
        if self.at_op("**"):
            self.need("arith")
            self.expect_op("**")
            return ("binop", "**", base, self.parse_unary())
        return base

    def parse_incdec(self):
        t = self.peek(True)
        if t.kind == "op" and t.val in ("++", "--"):
            self.need("incdec")
            self.pos = t.end
            return ("preinc", t.val, self.parse_incdec())
        node = self.parse_postfix()
        t = self.peek()
        if t.kind == "op" and t.val in ("++", "--"):
            self.need("incdec")
            self.pos = t.end
            return ("postinc", t.val, node)
        return node

    # -- postfix / terms --------------------------------------------------
    def parse_postfix(self, node=None):
        if node is None:
            node = self.parse_term()
        while True:
            t = self.peek()
            if t.kind == "op" and t.val == "->":
                self.pos = t.end
                # postfix dereference: ->@*, ->$#*, ->%*, ->$*
                j = self.lx.skip_ws(self.pos)
                postfix = next((pat for pat in ("$#*", "@*", "%*", "$*")
                                if self.src.startswith(pat, j)), None)
                if postfix is not None:
                    self.need("postfix_deref")
                    self.pos = j + len(postfix)
                    self._peek_cache.clear()
                    node = ("postderef", postfix[:-1], node)
                    continue
                nt = self.peek(True)
                if nt.kind == "op" and nt.val in ("[", "{"):
                    node = self._subscript(node, arrow=True)
                elif nt.kind == "op" and nt.val == "(":
                    self.need("code_deref_call")
                    self.pos = nt.end
                    args = self._call_args_rest()
                    node = ("callref", node, args)
                elif nt.kind == "ident":
                    self.need("method")
                    self.pos = nt.end
                    args = []
                    if self.at_op("("):
                        self.expect_op("(")
                        args = self._call_args_rest()
                    node = ("method", node, nt.val, args)
                elif nt.kind == "var":
                    self.need("method")
                    self.pos = nt.end
                    args = []
                    if self.at_op("("):
                        self.expect_op("(")
                        args = self._call_args_rest()
                    node = ("method_dyn", node, ("var",) + tuple(nt.val), args)
                else:
                    raise Refuse(f"unsupported -> target {nt.kind}", nt.start)
                continue
            if t.kind == "op" and t.val in ("[", "{") and self._subscriptable(node):
                node = self._subscript(node, arrow=False)
                continue
            return node

    def _subscriptable(self, node):
        head = node[0]
        if head in ("elem", "slice"):
            return True
        if head == "var" and node[1] in ("$", "@", "%"):
            return True
        if head == "deref" and node[1] in ("$", "@", "%"):
            return True
        return False

    def _subscript(self, node, arrow):
        t = self.next(True)
        opener = t.val
        sigil = "$"
        base = node
        if not arrow and node[0] == "var":
            sigil = node[1]
            base = ("var", "@" if opener == "[" else "%", node[2])
        elif not arrow and node[0] == "deref":
            sigil = node[1]
            base = ("deref", "@" if opener == "[" else "%", node[2])
        if opener == "[":
            self.need("subscript")
            idx = self.parse_expr()
            self.expect_op("]")
        else:
            self.need("hash_elem")
            idx = self._hash_key()
        kind = "slice" if sigil in ("@", "%") and not arrow else "elem"
        return (kind, base, opener, idx, arrow)

    def _hash_key(self):
        t = self.peek(True)
        if t.kind == "ident":
            after = self.lx.token(t.end, False)
            if after.kind == "op" and after.val == "}":
                self.pos = after.end
                return ("str", t.val)
        if t.kind == "op" and t.val == "-":
            nt = self.lx.token(t.end, False)
            if nt.kind == "ident":
                after = self.lx.token(nt.end, False)
                if after.kind == "op" and after.val == "}":
                    self.pos = after.end
                    return ("str", "-" + nt.val)
        idx = self.parse_expr()
        self.expect_op("}")
        return idx

    def _call_args_rest(self):
        """Parse arguments after '(' has been consumed, through ')'."""
        if self.at_op(")", term=True):
            self.expect_op(")")
            return []
        e = self.parse_expr()
        self.expect_op(")")
        return e[1] if e[0] == "list" else [e]

    def _term_follows(self, allow_sign):
        t = self.peek(True)
        if t.kind in ("num", "str", "dq", "var", "cast", "quote", "amp"):
            return True
        if t.kind == "ident":
            if t.val in ("if", "unless", "while", "until", "for", "foreach"):
                return False  # a statement modifier, never an argument
            return t.val not in WORD_OPS or t.val == "not"
        if t.kind == "op":
            if t.val in ("-", "+"):
                return allow_sign
            return t.val in TERM_START_OPS and t.val != "{"
        return False

    def parse_term(self):
        t = self.peek(True)
        k = t.kind
        if k == "num":
            self.pos = t.end
            return ("num", t.val)
        if k == "str":
            self.pos = t.end
            return ("str", _unescape_sq(t.val))
        if k == "dq":
            self.pos = t.end
            return self._interp_node(t.val)
        if k == "quote":
            self.pos = t.end
            return self._quote_node(t)
        if k == "var":
            self.pos = t.end
            sig, name = t.val
            if sig == "&":
                self.need("amp_call")
                args = []
                if self.at_op("("):
                    self.expect_op("(")
                    args = self._call_args_rest()
                return self._call_node(name, args)
            if sig == "$" and (name.isdigit() or not name[0].isalnum() and name != "_"
                               or name.startswith("^")):
                self.need("special_vars")
            if name == "_" and sig in ("$", "@"):
                self.need("special_vars")
            if sig == "$#":
                self.need("array_funcs")
            return ("var", sig, name)
        if k == "cast":
            self.pos = t.end
            self.need("deref")
            inner = self._cast_operand()
            if t.val == "&":
                self.need("amp_call")
                args = []
                if self.at_op("("):
                    self.expect_op("(")
                    args = self._call_args_rest()
                return ("callref", inner, args)
            return ("deref", t.val, inner)
        if k == "op":
            if t.val == "(":
                self.pos = t.end
                if self.at_op(")", term=True):
                    self.expect_op(")")
                    node = ("list", [])
                else:
                    inner = self.parse_expr()
                    self.expect_op(")")
                    node = ("paren", inner)
                if self.at_op("["):
                    self.need("list_slice")
                    self.expect_op("[")
                    idx = self.parse_expr()
                    self.expect_op("]")
                    node = ("listslice", node, idx)
                return node
            if t.val == "[":
                self.need("anon_ref")
                self.pos = t.end
                if self.at_op("]", term=True):
                    self.expect_op("]")
                    return ("anonarray", [])
                e = self.parse_expr()
                self.expect_op("]")
                return ("anonarray", e[1] if e[0] == "list" else [e])
            if t.val == "{":
                self.need("anon_ref")
                self.pos = t.end
                if self.at_op("}", term=True):
                    self.expect_op("}")
                    return ("anonhash", [])
                e = self.parse_expr()
                self.expect_op("}")
                return ("anonhash", e[1] if e[0] == "list" else [e])
        if k == "ident":
            return self.parse_ident_term()
        raise Refuse(f"unexpected {k}:{t.val!r}", t.start)

    def _cast_operand(self):
        t = self.peek(True)
        if t.kind == "op" and t.val == "{":
            self.pos = t.end
            e = self.parse_expr()
            self.expect_op("}")
            return e
        if t.kind == "var":
            self.pos = t.end
            return ("var",) + tuple(t.val)
        if t.kind == "cast":
            self.pos = t.end
            return ("deref", t.val, self._cast_operand())
        raise Refuse("unsupported dereference operand", t.start)

    def _interp_node(self, body):
        exprs = scan_interpolations(body, in_regex=False)
        if not exprs:
            return ("str", _unescape_dq(body))
        self.need("string_interp")
        parsed = [parse_expression_fragment(e, self.features) for e in exprs]
        return ("interp", body, parsed)

    def _quote_node(self, t):
        kind = t.val
        body, repl, flags, delim = t.extra
        if kind == "q":
            return ("str", _unescape_sq(body))
        if kind == "qq":
            self.need("quote_ops")
            return self._interp_node(body)
        if kind == "qw":
            self.need("qw")
            return ("list", [("str", w) for w in body.split()])
        if kind in ("m", "qr"):
            self.need("regex_match")
            parts = [] if delim == "'" else scan_interpolations(body, in_regex=True)
            parsed = [parse_expression_fragment(e, self.features) for e in parts]
            if parsed:
                self.need("regex_interp")
            return ("regex", kind, body, flags, parsed)
        if kind == "s":
            self.need("subst")
            parts = scan_interpolations(body, in_regex=True)
            parsed = [parse_expression_fragment(e, self.features) for e in parts]
            if "e" in flags:
                self.need("subst_e")
                rnode = parse_code_fragment(repl, self.features)
            else:
                rparts = scan_interpolations(repl, in_regex=False)
                rnode = ("interp", repl,
                         [parse_expression_fragment(e, self.features) for e in rparts])
            return ("subst", body, rnode, flags, parsed)
        if kind == "tr":
            self.need("tr")
            return ("tr", body, repl, flags)
        raise Refuse(f"unsupported quote-like operator {kind}")

    def parse_ident_term(self):
        t = self.next()
        word = t.val
        after = self.peek()
        if after.kind == "op" and after.val == "=>":
            return ("str", word)
        if after.kind == "op" and after.val == "->":
            return ("class", word)
        if word in ("my", "our", "state"):
            self.need("my")
            return self._parse_decl(word)
        if word == "local":
            self.need("local")
            target = self.parse_postfix()
            return ("localdecl", target)
        if word == "sub":
            self.need("anon_sub")
            return ("anonsub", self.parse_block())
        if word in ("do", "eval"):
            self.need("do_eval")
            if self.at_op("{"):
                return (word, self.parse_block())
            return (word, self.parse_unary())
        if word in ("last", "next", "redo"):
            self.need("loop_ctl")
            nt = self.peek()
            if nt.kind == "ident" and nt.val.isupper():
                self.pos = nt.end
                return (word, nt.val)
            return (word, None)
        if word == "return":
            self.need("return")
            if self._term_follows(allow_sign=True):
                return ("return", self.parse_comma())
            return ("return", None)
        if word in ("map", "grep", "sort"):
            self.need("map_grep_sort")
            return self._parse_map_grep_sort(word)
        if word == "require":
            nt = self.peek()
            if nt.kind == "ident":
                self.pos = nt.end
                self.need("require_module")
                return ("require_module", nt.val)
        if word == "__PACKAGE__":
            return ("str", "__PACKAGE__")
        if word in ("wantarray", "time"):
            if self.at_op("("):
                self.expect_op("(")
                self._call_args_rest()
            return ("call", word, [], "core")
        # generic call
        if self.at_op("("):
            self.need("call")
            self.expect_op("(")
            args = self._call_args_rest()
            return self._call_node(word, args)
        if word in NAMED_UNARY:
            self.need("named_unary")
            if self._term_follows(allow_sign=False):
                return self._call_node(word, [self.parse_shift()])
            return self._call_node(word, [])
        if word in LIST_OPS or True:
            if self._term_follows(allow_sign=True):
                self.need("listop_no_parens")
                e = self.parse_comma()
                args = e[1] if e[0] == "list" else [e]
                return self._call_node(word, args)
            self.need("bareword_call")
            return self._call_node(word, [])
        raise Refuse(f"unsupported bareword {word!r}", t.start)

    def _call_node(self, name, args):
        if name in CORE_BUILTINS:
            return ("call", name, args, "core")
        return ("call", name, args, "sub")

    def _parse_decl(self, word):
        t = self.peek(True)
        if t.kind == "var":
            self.pos = t.end
            return ("decl", word, [t.val])
        if t.kind == "op" and t.val == "(":
            self.pos = t.end
            names = []
            while True:
                v = self.next(True)
                if v.kind == "var":
                    names.append(v.val)
                elif v.kind == "ident" and v.val == "undef":
                    names.append(("$", "undef"))
                else:
                    raise Refuse("unsupported declaration list", v.start)
                if self.eat_op(","):
                    continue
                self.expect_op(")")
                break
            return ("decl", word, names)
        raise Refuse("unsupported declaration", t.start)

    def _parse_map_grep_sort(self, word):
        parens = self.eat_op("(")
        block = None
        if self.at_op("{", term=True):
            block = self.parse_block()
            self.eat_op(",")
        if self.peek(True).kind == "eof" or self.at_op(")", term=True):
            rest = ("list", [])
        else:
            e = self.parse_comma()
            rest = e if e[0] == "list" else ("list", [e])
        if parens:
            self.expect_op(")")
        return ("mapgrep", word, block, rest)


def _unescape_sq(body):
    return re.sub(r"\\([\\'])", r"\1", body)


_DQ_ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", "f": "\f",
               "e": "\x1b", "a": "\a", "\\": "\\", '"': '"', "$": "$",
               "@": "@"}


def _unescape_dq(body):
    out = []
    i = 0
    while i < len(body):
        c = body[i]
        if c == "\\" and i + 1 < len(body):
            nxt = body[i + 1]
            if nxt in _DQ_ESCAPES:
                out.append(_DQ_ESCAPES[nxt])
                i += 2
                continue
            if nxt == "x":
                m = re.compile(r"\\x\{([0-9a-fA-F]+)\}|\\x([0-9a-fA-F]{1,2})").match(body, i)
                if m:
                    out.append(chr(int(m.group(1) or m.group(2), 16)))
                    i = m.end()
                    continue
            out.append(nxt)
            i += 2
            continue
        out.append(c)
        i += 1
    return "".join(out)


def parse_expression_fragment(text, feature_sink):
    """Parse an interpolated fragment (`$val[0]`, `@{[ sprintf ... ]}`)."""
    p = Parser(text, feature_sink)
    node = p.parse_expr()
    if p.peek().kind != "eof":
        raise Refuse(f"trailing text in interpolation: {text!r}")
    return node


def parse_code_fragment(text, feature_sink):
    p = Parser(text, feature_sink)
    return p.parse_program()


# ---------------------------------------------------------------------------
# public entry points
# ---------------------------------------------------------------------------

_PROTO = re.compile(r"^\s*\(\s*[\$\@%;\\&\*\[\]+ ]*\s*\)\s*")


def parse_string_expr(text):
    """Parse an ExifTool expression STRING (Condition / *Conv / *Inv).

    Returns (ast, features). Raises Refuse."""
    features = set()
    p = Parser(text, features)
    ast = p.parse_program()
    return ast, features


def parse_code_ref(deparse):
    """Parse a B::Deparse sub body (`($) { package ...; ... }`)."""
    features = {"coderef"}
    text = deparse.strip()
    m = _PROTO.match(text)
    if m:
        text = text[m.end():]
    if not text.startswith("{"):
        raise Refuse("code ref body does not start with a block")
    p = Parser(text, features)
    block = p.parse_block()
    if p.peek().kind != "eof":
        raise Refuse("trailing text after code ref body")
    return ("program", [block]), features


def parse_any(text=None, deparse=None):
    if deparse is not None:
        return parse_code_ref(deparse)
    return parse_string_expr(text)


# ---------------------------------------------------------------------------
# AST walking
# ---------------------------------------------------------------------------

def children(node):
    for item in node:
        if isinstance(item, tuple) and item and isinstance(item[0], str):
            yield item
        elif isinstance(item, list):
            for sub in item:
                if isinstance(sub, tuple) and sub and isinstance(sub[0], str):
                    yield sub


def walk(node):
    yield node
    for child in children(node):
        yield from walk(child)
