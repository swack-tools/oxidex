#!/usr/bin/env python3
"""Record which Sony models each release's `Sony::Main` model Conditions select.

Writes `fixtures/sony_main_model_conditions.json`: for every release lib given,
every `$$self{Model} =~ /RE/` (or `!~`) Condition on a MANIFEST tag of
`gen_sony_main_extra_tables.py`, the models that release's own Perl selects
(evaluated by `capture_sony_main_conditions.pl`) next to the `MCond` text the
generator emits for that exact Condition. The Rust test
`main_extra::tests::model_conditions_select_what_each_releases_perl_selects`
evaluates each emitted `MCond` through the real interpreter over the same
models and requires the same selection; the Python tests require the emitted
text to still be what the generator produces.

The model list is the pinned release's 0xb001 SonyModelID PrintConv (real
Model strings), plus a few labelled synthetic probes aimed at the regexes'
edges (anchoring, case, a prefix of an alternative). Each release record's
`exiftool_version` is the loaded module's own `$Image::ExifTool::VERSION`,
recorded for the reader; nothing here admits or refuses on it.

usage:
  capture_sony_main_conditions.py --perl PERL --models-lib LIB13.59 \\
      --lib LIB [--lib LIB ...] -o fixtures/sony_main_model_conditions.json
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path

import gen_sony_main_extra_tables as generator

HERE = Path(__file__).resolve().parent
PL = HERE / "capture_sony_main_conditions.pl"

# Not real bodies: edges the regexes must agree on anyway.
SYNTHETIC = [
    "",                 # empty Model
    "ilce-7m3",         # case: every pattern here is case-sensitive
    "XILCE-7M3",        # anchoring: `^` must hold
    "DSC-RX1RM3X",      # word-boundary edge for `\b` patterns
    "DSC-RX100M5AX",    # an alternative followed by more text (no `$`)
    "DSC-HX9",          # a strict prefix of an alternative
    "HX95",             # an inner alternative without its `DSC-` group
    "ZV-",              # a bare prefix
]


def perl(perl_bin, *args):
    proc = subprocess.run([perl_bin, str(PL), *args], capture_output=True, text=True)
    if proc.returncode != 0 or proc.stderr.strip():
        raise SystemExit(f"{PL.name} {args[0]} failed (rc={proc.returncode}): {proc.stderr.strip()}")
    return json.loads(proc.stdout)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--perl", required=True)
    ap.add_argument("--models-lib", required=True)
    ap.add_argument("--lib", action="append", required=True)
    ap.add_argument("-o", "--output", required=True)
    args = ap.parse_args()

    model_src = perl(args.perl, "models", args.models_lib)
    models = model_src["models"] + [m for m in SYNTHETIC if m not in model_src["models"]]
    ids = [f"0x{i:x}" for i in generator.MANIFEST]

    releases = []
    with tempfile.NamedTemporaryFile("w", suffix=".json") as tmp:
        json.dump(models, tmp)
        tmp.flush()
        for lib in args.lib:
            rel = perl(args.perl, "eval", lib, tmp.name, *ids)
            for row in rel["conditions"]:
                row["generated"] = generator.translate_cond(int(row["tag"], 16), row["name"], row["condition"])
            releases.append(rel)

    out = {
        "about": "Models each release's own Perl selects with each Sony::Main model Condition; "
                 "regenerate with capture_sony_main_conditions.py.",
        "models": {"source": f"{model_src['source']} ({model_src['exiftool_version']}, "
                             f"Sony.pm sha256 {model_src['sony_pm_sha256']})",
                   "real": model_src["models"], "synthetic": SYNTHETIC},
        "releases": releases,
    }
    Path(args.output).write_text(json.dumps(out, indent=1, sort_keys=True) + "\n", encoding="utf-8")


if __name__ == "__main__":
    try:
        main()
    except generator.Unsupported as e:
        print(f"capture_sony_main_conditions.py: {e}", file=sys.stderr)
        sys.exit(1)
