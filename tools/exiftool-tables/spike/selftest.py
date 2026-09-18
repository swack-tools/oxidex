#!/usr/bin/env python3
"""SPIKE self-test: pins the parser's behaviour on shapes taken verbatim from
the pinned 13.59 dump, plus the refusals it must keep refusing.

    python3 tools/exiftool-tables/spike/selftest.py
"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import classify  # noqa: E402
import perl_subset as P  # noqa: E402

MUST_PARSE = [
    "$val / 100",
    'sprintf("%.1f",$val)',
    "$self->ConvertDateTime($val)",
    "$val =~ s/ .*//; $val",
    '$$self{Make} eq "Canon" and $$self{Model} =~ /EOS/',
    "ConvertUnixTime($val - ((66 * 365 + 17) * 24 * 3600))",
    "Image::ExifTool::Exif::PrintExposureTime($val)",
    "$val[0] ? $val[1]/$val[0] : undef",
    'unpack("H*",$val)',
    "tr/ /:/; $val",
    r"$$valPt =~ /^\x01/",
    "ToFloat(@val); return undef unless $val[2] and $val[2] == 3; return $val[1];",
    "require Image::ExifTool::XMP; Image::ExifTool::XMP::ConvertXMPDate($val)",
    "my $i = 0; $i++ while $i < 3; $i",
    'my @a = split " ", $val; join(".", map { sprintf "%d", $_ } @a)',
    "$s->$#*",
]

MUST_REFUSE = [
    "$val m",                                   # MXF.pm: not valid Perl
    '$$self{HasIJPEG}"',                        # JPEG.pm: stray quote
    "int(log($val)*2400) + 0.5)",               # CanonCustom.pm: extra paren
    '$self->Decode($val, ($$self{URIFlags} & 0x80) ? "UTF16" : "Latin"',
]

DEPS = [
    ("$val / 100", set(), set()),
    ("$$self{Model} =~ /EOS/", {"self:Model"}, set()),
    ("Image::ExifTool::Exif::PrintExposureTime($val)", set(),
     {"Exif::PrintExposureTime"}),
    # the invocant is the helper's business, so a method call records the
    # helper and no session key of its own
    ("$self->ConvertDateTime($val)", set(), {"ET->ConvertDateTime"}),
    # ... but `$self` handed to a helper as an ARGUMENT is a session read
    ('Image::ExifTool::GPS::ToDMS($self, $val, 1, "N")', {"self:<object>"},
     {"GPS::ToDMS"}),
    ('$$self{VALUE}{Make} =~ /Nikon/', {"self:VALUE{*} (other tags)"}, set()),
]


def main():
    failures = []
    for text in MUST_PARSE:
        try:
            P.parse_string_expr(text)
        except P.Refuse as e:
            failures.append(f"REFUSED but must parse: {text!r} ({e.reason})")
    for text in MUST_REFUSE:
        try:
            P.parse_string_expr(text)
            failures.append(f"PARSED but must refuse: {text!r}")
        except P.Refuse:
            pass
    for text, session, helpers in DEPS:
        ast, _ = P.parse_string_expr(text)
        deps = classify.classify(ast)
        if set(deps.session) != session or set(deps.helpers) != helpers:
            failures.append(
                f"deps mismatch for {text!r}: session={sorted(deps.session)} "
                f"helpers={sorted(deps.helpers)}")
    for f in failures:
        print("FAIL:", f)
    print(f"{len(MUST_PARSE) + len(MUST_REFUSE) + len(DEPS) - len(failures)}"
          f"/{len(MUST_PARSE) + len(MUST_REFUSE) + len(DEPS)} checks passed")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
