#!/usr/bin/env python3
"""Compare compiled GeoTIFF names, map dispatch and lookups with loaded Perl facts.

This verifier does not import the producer or parse its expected Rust text.
It executes the candidate module over every u16 key and conversion value.
It verifies the generated module's declared scope, not container parsing.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
MODULES = ("Image/ExifTool.pm", "Image/ExifTool/GeoTiff.pm")
ORACLE = r'''
use strict;
use warnings;
use Image::ExifTool::GeoTiff;
use JSON::PP;
my (%names, %conversions);
while (my ($id, $info) = each %Image::ExifTool::GeoTiff::Main) {
    next if $id eq 'GROUPS';
    die "unsupported oracle key $id" unless $id =~ /\A(?:0|[1-9][0-9]*)\z/ && $id <= 65535;
    my $name = ref($info) eq 'HASH' ? $info->{Name} : $info;
    die "invalid oracle name $id" if !defined($name) || ref($name) || !length($name);
    $names{$id} = "$name";
    if (ref($info) eq 'HASH' && exists($info->{PrintConv})) {
        my $map = $info->{PrintConv};
        die "unsupported oracle conversion $id" unless ref($map) eq 'HASH' && keys(%$map);
        my %values;
        while (my ($value, $label) = each %$map) {
            die "invalid oracle conversion key $id:$value"
                unless $value =~ /\A(?:0|[1-9][0-9]*)\z/ && $value <= 65535;
            die "unsupported oracle conversion label $id:$value"
                if !defined($label) || ref($label) || !length($label);
            $values{$value} = "$label";
        }
        $conversions{$id} = \%values;
    }
}
die "empty oracle population" unless keys(%names) && keys(%conversions);
print JSON::PP->new->canonical->encode({version => "$Image::ExifTool::VERSION",
    loaded => [map { $INC{$_} } qw(Image/ExifTool.pm Image/ExifTool/GeoTiff.pm)],
    names => \%names, conversions => \%conversions});
'''


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def run(argv, **kwargs):
    return subprocess.run(argv, text=True, capture_output=True, check=True, **kwargs).stdout


def oracle(tree, perl):
    modules = [tree / "lib" / p for p in MODULES]
    for module in modules:
        if not module.is_file():
            raise ValueError(f"selected source module missing: {module}")
    env = {k: v for k, v in os.environ.items() if not k.startswith("PERL")}
    doc = json.loads(run([perl, "-I", str(tree / "lib"), "-e", ORACLE], env=env))
    if doc["version"] != (ROOT / ".exiftool-version").read_text().strip():
        raise ValueError("selected source does not match the repository pin")
    if [Path(p).resolve() for p in doc["loaded"]] != [p.resolve() for p in modules]:
        raise ValueError("oracle loaded modules outside the selected tree")
    return doc


def compiled_facts(candidate, rustc, directory):
    source = directory / "probe.rs"
    executable = directory / "probe"
    source.write_text(f'#[path = {json.dumps(str(candidate))}] mod candidate;\n' + r'''
fn main() {
    for key in 0..=u16::MAX {
        if let Some(name) = candidate::geokey_name(key) {
            println!("N\t{}\t{:?}", key, name);
        }
        if let Some(map) = candidate::geokey_print_conv(key) {
            println!("M\t{}", key);
            for value in 0..=u16::MAX {
                if let Some(label) = candidate::print_conv_lookup(map, value) {
                    println!("C\t{}\t{}\t{:?}", key, value, label);
                }
            }
        }
    }
}
''')
    run([rustc, "--edition=2024", "-C", "opt-level=1", str(source), "-o", str(executable)])
    names, maps = {}, {}
    for line in run([str(executable)]).splitlines():
        row = line.split("\t")
        if row[0] == "N" and len(row) == 3 and row[1] not in names:
            names[row[1]] = json.loads(row[2])
        elif row[0] == "M" and len(row) == 2 and row[1] not in maps:
            maps[row[1]] = {}
        elif row[0] == "C" and len(row) == 4 and row[1] in maps and row[2] not in maps[row[1]]:
            maps[row[1]][row[2]] = json.loads(row[3])
        else:
            raise ValueError(f"unexpected/duplicate compiled record: {line}")
    return {"names": names, "conversions": maps}, sha(executable)


def verify(tree, candidate, perl, rustc):
    modules = [tree / "lib" / p for p in MODULES]
    identities = {str(p): sha(p) for p in [candidate, *modules]}
    tool_hashes = {p: sha(p) for p in (perl, rustc)}
    expected = oracle(tree, perl)
    with tempfile.TemporaryDirectory(prefix="verify-geotiff-") as scratch:
        actual, executable_sha = compiled_facts(candidate, rustc, Path(scratch))
    if identities != {p: sha(p) for p in identities}:
        raise ValueError("source or candidate changed during verification")
    if tool_hashes != {p: sha(p) for p in tool_hashes}:
        raise ValueError("verification executable changed during verification")
    mismatches = []
    for kind in ("names", "conversions"):
        for key in sorted(expected[kind].keys() | actual[kind].keys(), key=int):
            if expected[kind].get(key) != actual[kind].get(key):
                mismatches.append(f"{kind}:{key}")
    return {"instrument": "verify_geotiff.py: compiled u16 domain vs loaded Perl",
            "pin": expected["version"], "perl": perl, "rustc": rustc,
            "git_head": run(["git", "-C", str(ROOT), "rev-parse", "HEAD"]).strip(),
            "git_dirty": bool(run(["git", "-C", str(ROOT), "status", "--porcelain"])),
            "tool_hashes": tool_hashes,
            "perl_version": run([perl, "-e", "print $^V"], env={
                k: v for k, v in os.environ.items() if not k.startswith("PERL")}),
            "rustc_version": run([rustc, "--version"]).strip(),
            "source_hashes": identities, "compiled_probe_sha256": executable_sha,
            "names": len(expected["names"]), "conversion_keys": len(expected["conversions"]),
            "conversion_facts": sum(map(len, expected["conversions"].values())),
            "candidate_names": len(actual["names"]), "candidate_conversion_keys": len(actual["conversions"]),
            "mismatches": mismatches, "passed": not mismatches}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--exiftool-dir", type=Path, required=True)
    ap.add_argument("--perl", default=os.environ.get("EXIFTOOL_PERL", "perl"))
    ap.add_argument("--rustc", default="rustc")
    ap.add_argument("--rust-file", type=Path, default=ROOT / "src/parsers/tiff/geotiff_printconv.rs")
    ap.add_argument("--report-out", type=Path)
    args = ap.parse_args()
    perl, rustc = shutil.which(args.perl), shutil.which(args.rustc)
    if not perl or not rustc:
        raise ValueError("selected Perl/rustc executable is missing")
    print("=== instrument: verify_geotiff.py ===", flush=True)
    report = verify(args.exiftool_dir.resolve(), args.rust_file.resolve(), perl, rustc)
    if args.report_out:
        args.report_out.parent.mkdir(parents=True, exist_ok=True)
        args.report_out.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report, sort_keys=True))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, subprocess.CalledProcessError) as exc:
        detail = exc.stderr.strip() if isinstance(exc, subprocess.CalledProcessError) and exc.stderr else str(exc)
        sys.exit(f"GeoTIFF verification refused: {detail}")
