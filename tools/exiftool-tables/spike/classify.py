#!/usr/bin/env python3
"""SPIKE (measurement only): classify a parsed ExifTool expression by what an
interpreter would need to EVALUATE it.

Three buckets, per the spike's question:

  PURE     only `$val` (and the composite `@val`/`@prt`/`@raw` inputs),
           literals, operators, core Perl builtins, regex/`s///`/`tr///`.
  SESSION  reads ExifTool session state: `$$self{...}` / `$self->{...}` /
           `$$et{...}` / `$$dirInfo{...}` / `$self->Options(...)` / another
           tag's value, or one of the lexicals ExifTool has in scope at the
           eval site (`$tag`, `$format`, `$count`, ...).
  HELPER   calls a named ExifTool sub (or method on the ExifTool object), or
           reads a module-level data table (`%canonLensTypes`) -- both are
           "port once, shared by every expression that uses it".

A use can be SESSION+HELPER. Nothing here evaluates anything; it reports the
dependency set a real interpreter would have to satisfy.
"""

import re

import perl_subset as P

# `$val` and its composite siblings: the value under conversion. Not session.
INPUT_SCALARS = {"val", "prt", "raw", "valPt", "self_UNUSED"}
INPUT_ARRAYS = {"val", "prt", "raw"}

# Perl's own punctuation/implicit variables: core semantics, not ExifTool.
PURE_SPECIAL = set("_&`'+1234567890/\\,;.\"@") | {"a", "b", "^W"}

OBJECT_VARS = {"self", "et", "exifTool"}

# Lexicals ExifTool has in scope where it evals a conversion or Condition.
# They are inputs to the eval, but they are NOT `$val` -- an interpreter has
# to be handed each one, so they count as session context.
KNOWN_CTX = {
    "tag", "tagInfo", "format", "count", "size", "dataPt", "dataPos",
    "dirInfo", "dirStart", "dirLen", "base", "index", "type", "pos",
    "tagTablePtr", "priority", "wantGroup", "byteOrder", "mode", "cnt",
    "oldVal", "conv", "tbl", "name", "exifTool", "key", "start", "len",
}

ENV_BUILTINS = {"localtime", "gmtime", "time", "sleep", "rand", "srand",
                "caller", "exit"}

# Regex constructs Rust's `regex` crate cannot compile (an interpreter in
# Rust needs `fancy-regex`/PCRE or a hand-rolled engine for these).
_FANCY = [
    (r"\(\?<[=!]", "lookbehind"),
    (r"\(\?[=!]", "lookahead"),
    (r"\\[1-9]", "backreference"),
    (r"\\G", "\\G anchor"),
    (r"\(\?\{", "embedded code"),
    (r"\(\?R\)|\(\?[0-9]\)", "recursion"),
    (r"\\K", "\\K"),
    (r"\(\?\|", "branch reset"),
]


class Deps:
    def __init__(self):
        self.session = {}        # key -> count of references
        self.helpers = {}        # helper name -> count
        self.data = {}           # module data table -> count
        self.builtins = {}
        self.regex_fancy = set()
        self.session_writes = set()
        self.unknown_free = {}

    def add(self, d, key):
        d[key] = d.get(key, 0) + 1

    @property
    def is_pure(self):
        return not self.session and not self.helpers and not self.data

    def summary(self):
        return {
            "session": dict(sorted(self.session.items())),
            "helpers": dict(sorted(self.helpers.items())),
            "data": dict(sorted(self.data.items())),
            "builtins": dict(sorted(self.builtins.items())),
            "regex_fancy": sorted(self.regex_fancy),
            "session_writes": sorted(self.session_writes),
            "unknown_free": dict(sorted(self.unknown_free.items())),
        }


def _is_node(x):
    return isinstance(x, tuple) and x and isinstance(x[0], str)


class Classifier:
    def __init__(self, package=None):
        self.package = package
        self.deps = Deps()
        self.declared = set()

    # -- declaration pre-pass ---------------------------------------------
    def collect_declared(self, node):
        if not _is_node(node):
            return
        head = node[0]
        if head == "decl":
            for sigil, name in node[2]:
                self.declared.add(name)
            return
        if head == "foreach" and node[1] is not None:
            lv = node[1]
            if lv[0] == "my":
                for sigil, name in lv[1]:
                    self.declared.add(name)
            elif lv[0] == "var":
                self.declared.add(lv[2])
        for child in self._children(node):
            self.collect_declared(child)

    def _children(self, node):
        for item in node:
            if _is_node(item):
                yield item
            elif isinstance(item, list):
                for sub in item:
                    if _is_node(sub):
                        yield sub

    # -- main walk ---------------------------------------------------------
    def visit(self, node):
        if not _is_node(node):
            return
        head = node[0]
        fn = getattr(self, "v_" + head, None)
        if fn is not None:
            fn(node)
            return
        for child in self._children(node):
            self.visit(child)

    # -- leaves ------------------------------------------------------------
    def v_decl(self, node):
        return

    def v_str(self, node):
        return

    def v_num(self, node):
        return

    def v_use(self, node):
        return

    def v_require_module(self, node):
        return

    def v_package(self, node):
        return

    def v_class(self, node):
        return

    def v_var(self, node):
        sigil, name = node[1], node[2]
        if name in self.declared:
            return
        if sigil in ("$", "@", "%", "$#"):
            if name in PURE_SPECIAL or name.startswith("^"):
                return
            if sigil in ("$", "@") and name in INPUT_ARRAYS:
                return
            if sigil == "$" and name in INPUT_SCALARS:
                return
            if name in OBJECT_VARS:
                # bare `$self` (passed to a helper, or used as an object):
                # the helper it is handed to accounts for the session read.
                self.deps.add(self.deps.session, "self:<object>")
                return
            if name in KNOWN_CTX:
                self.deps.add(self.deps.session, f"ctx:${name}")
                return
            if "::" in name:
                self.deps.add(self.deps.data, f"{sigil}{name}")
                return
            if sigil in ("@", "%"):
                self.deps.add(self.deps.data, f"{sigil}{name}")
                return
            self.deps.add(self.deps.unknown_free, f"{sigil}{name}")
            self.deps.add(self.deps.session, f"ctx:${name}")

    # -- structure ---------------------------------------------------------
    def _chain_root(self, node):
        """(root_var_node, [key-or-None, ...]) for an elem/slice chain."""
        keys = []
        cur = node
        while _is_node(cur) and cur[0] in ("elem", "slice"):
            idx = cur[3]
            keys.append(idx[1] if _is_node(idx) and idx[0] == "str" else None)
            cur = cur[1]
        keys.reverse()
        while _is_node(cur) and cur[0] == "deref":
            cur = cur[2]
        return cur, keys

    def _visit_indices(self, node):
        cur = node
        while _is_node(cur) and cur[0] in ("elem", "slice"):
            idx = cur[3]
            if _is_node(idx) and idx[0] != "str":
                self.visit(idx)
            cur = cur[1]

    def v_elem(self, node):
        self._elem_like(node)

    def v_slice(self, node):
        self._elem_like(node)

    def _elem_like(self, node):
        root, keys = self._chain_root(node)
        self._visit_indices(node)
        if not _is_node(root) or root[0] != "var":
            self.visit(root)
            return
        name = root[2]
        if name in self.declared:
            return
        first = keys[0] if keys else None
        if name in OBJECT_VARS:
            if first is None:
                self.deps.add(self.deps.session, "self:<dynamic key>")
            elif first == "VALUE":
                self.deps.add(self.deps.session, "self:VALUE{*} (other tags)")
            else:
                self.deps.add(self.deps.session, f"self:{first}")
            return
        if name == "dirInfo":
            self.deps.add(self.deps.session,
                          f"dirInfo:{first if first else '<dynamic key>'}")
            return
        if name == "tagInfo" or name == "tagTablePtr":
            self.deps.add(self.deps.session, f"ctx:${name}")
            return
        if name in INPUT_ARRAYS and root[1] in ("@", "$", "%"):
            return
        if name in INPUT_SCALARS:
            return
        if name in KNOWN_CTX:
            self.deps.add(self.deps.session, f"ctx:${name}")
            return
        self.deps.add(self.deps.data, f"{root[1]}{name}")

    def v_postderef(self, node):
        self.visit(node[2])

    def v_deref(self, node):
        inner = node[2]
        if _is_node(inner) and inner[0] == "var" and inner[2] in INPUT_SCALARS \
                and inner[2] not in self.declared:
            return
        self.visit(inner)

    def v_assign(self, node):
        target = node[2]
        if _is_node(target) and target[0] in ("elem", "slice"):
            root, keys = self._chain_root(target)
            if _is_node(root) and root[0] == "var" and root[2] in OBJECT_VARS:
                self.deps.session_writes.add(
                    f"self:{keys[0] if keys else '<dynamic key>'}")
        self.visit(node[2])
        self.visit(node[3])

    # -- calls -------------------------------------------------------------
    def v_call(self, node):
        name, args, kind = node[1], node[2], node[3]
        if kind == "core":
            self.deps.add(self.deps.builtins, name)
            if name in ENV_BUILTINS:
                self.deps.add(self.deps.helpers, f"<perl builtin {name}>")
        else:
            self.deps.add(self.deps.helpers, canonical_helper(name, self.package))
        for a in args:
            self.visit(a)

    def v_callref(self, node):
        self.deps.add(self.deps.helpers, "<code ref call>")
        self.visit(node[1])
        for a in node[2]:
            self.visit(a)

    def v_method(self, node):
        inv, name, args = node[1], node[2], node[3]
        invname = inv[2] if _is_node(inv) and inv[0] == "var" else None
        if _is_node(inv) and inv[0] == "deref":
            root = inv
            while _is_node(root) and root[0] == "deref":
                root = root[2]
            if _is_node(root) and root[0] == "var":
                invname = root[2]
        if invname in OBJECT_VARS:
            if name == "Options":
                opt = args[0][1] if args and _is_node(args[0]) and args[0][0] == "str" \
                    else "<dynamic>"
                self.deps.add(self.deps.session, f"Options({opt})")
            elif name in ("GetValue", "GetTagInfo", "GetInfo", "FindValue"):
                self.deps.add(self.deps.session, "self:VALUE{*} (other tags)")
                self.deps.add(self.deps.helpers, f"ET->{name}")
            else:
                self.deps.add(self.deps.helpers, f"ET->{name}")
        elif _is_node(inv) and inv[0] == "class":
            self.deps.add(self.deps.helpers,
                          canonical_helper(inv[1] + "::" + name, self.package))
        else:
            self.deps.add(self.deps.helpers, f"<dynamic>->{name}")
            self.visit(inv)
        for a in args:
            self.visit(a)

    def v_method_dyn(self, node):
        self.deps.add(self.deps.helpers, "<dynamic method>")
        self.visit(node[1])
        for a in node[3]:
            self.visit(a)

    # -- regex -------------------------------------------------------------
    def _regex_features(self, pattern):
        for rx, label in _FANCY:
            if re.search(rx, pattern):
                self.deps.regex_fancy.add(label)

    def v_regex(self, node):
        self._regex_features(node[2])
        for e in node[4]:
            self.visit(e)

    def v_subst(self, node):
        self._regex_features(node[1])
        self.visit(node[2])
        for e in node[4]:
            self.visit(e)


def canonical_helper(name, package=None):
    """`Image::ExifTool::Exif::PrintExposureTime` -> `Exif::PrintExposureTime`;
    a bare name -> the package it resolves in (code refs carry a `package`
    line; ExifTool evals conversion STRINGS inside package Image::ExifTool,
    so a bare name there is a core sub)."""
    if name.startswith("Image::ExifTool::"):
        return name[len("Image::ExifTool::"):]
    if "::" in name:
        return name
    if package and package.startswith("Image::ExifTool::"):
        return package[len("Image::ExifTool::"):] + "::" + name
    return name


def classify(ast, package=None):
    c = Classifier(package=package)
    c.collect_declared(ast)
    c.visit(ast)
    return c.deps
