"""Small Rust lexical mask shared by static source checks.

Preserves offsets and newlines, erases comments and character literals, and
replaces each string with an @ marker plus spaces. Original string slices are
returned separately. This is not a Rust parser or macro expander.
"""
import re

RAW_STRING = re.compile(r'(?:b|c)?r(#{0,255})"')
CHARACTER = re.compile(r"'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'")

def lexical_source(source):
    """Keep offsets/newlines, replace strings by @ markers and erase comments."""
    code = list(source)
    literals = {}
    i = 0
    while i < len(source):
        start = i
        literal = False
        if source.startswith("//", i):
            end = source.find("\n", i)
            i = len(source) if end < 0 else end
        elif source.startswith("/*", i):
            depth = 1
            i += 2
            while i < len(source) and depth:
                if source.startswith("/*", i):
                    depth += 1
                    i += 2
                elif source.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
        elif raw := RAW_STRING.match(source, i):
            end = source.find('"' + raw[1], raw.end())
            i = len(source) if end < 0 else end + 1 + len(raw[1])
            literal = True
        elif source[i] == '"':
            i += 1
            while i < len(source):
                if source[i] == "\\":
                    i += 2
                elif source[i] == '"':
                    i += 1
                    break
                else:
                    i += 1
            i = min(i, len(source))
            literal = True
        elif char := CHARACTER.match(source, i):
            i = char.end()  # Do not mistake Rust lifetimes for character literals.
        else:
            i += 1
            continue
        code[start:i] = ["\n" if c == "\n" else " " for c in source[start:i]]
        if literal:
            code[start] = "@"
            literals[start] = source[start:i]
    return "".join(code), literals

