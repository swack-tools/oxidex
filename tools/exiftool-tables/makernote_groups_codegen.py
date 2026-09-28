#!/usr/bin/env python3
"""Emit `src/writers/generated_makernote_groups.rs`: for every maker-note
root the pinned ExifTool can select, the family-1 groups of every table
reachable from it.

An ungrouped `-TAG=VALUE` makes ExifTool create the tag in its preferred
group and also *edit* the same-named tag wherever the file already carries
it (Writer.pl:613-825: every lower-priority candidate is written "if tag
exists"), MakerNotes included. oxidex cannot edit a maker-note entry, and its
maker-note decoders do not surface every row ExifTool reads, so the write
resolver (`src/writers/write_request.rs`) asks a table-level question
instead: *could* the file's maker note hold a same-named tag? A maker note is
read with one root table (`@Image::ExifTool::MakerNotes::Main`, whose first
matching `Condition` wins) plus every table its `SubDirectory` edges reach,
so the answer is the family-1 group closure captured here.

A value-typed entry (no `SubDirectory` table: `MakerNoteUnknownBinary`,
`MakerNoteUnknownText`, `MakerNoteSamsung1a`, `MakerNoteMinolta3`) is kept
with an empty table and closure: a note read that way holds no tags.

Each row is one `MakerNotes::Main` entry (and the CIFF root, which ExifTool
reads from a JPEG `APP0` segment and reports under family-1 `CIFF`): its
name, the root table, the root's family-1 group, and the sorted union of
`GROUPS{1}` and per-tag `Groups{1}` over every table reachable through
`SubDirectory => { TagTable => ... }`.

Two name-level captures ride along, both from the pinned `FindTagInfo`
over every `%tagLookup` (writable) name, so the resolver never has to infer
a candidate from a row the reader happened to surface:

- `MAKERNOTE_CANDIDATES`: every name with a writable `MakerNotes`
  candidate, with those candidates' family-1 groups. oxidex's maker-note
  decoders do not surface every row ExifTool edits (t/images/Nikon.jpg:
  no `[Nikon] FocusMode` row, which `-MakerNotes:FocusMode=` deletes).
- `GPS_NAME_CANDIDATES`: every writable candidate (family 0 and 1) of each
  `GPS::Main` name -- the bare GPS spellings the resolver keeps by hand
  (`GPSDateStamp`) have no rows in the SetNewValue address capture.

Usage:
    makernote_groups_codegen.py --exiftool-dir <pinned tree> --perl <pinned perl> \
        --output src/writers/generated_makernote_groups.rs
"""
from __future__ import annotations

import argparse
import hashlib
import os
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

WALK = r"""
use strict; use warnings;
use Image::ExifTool; use Image::ExifTool::MakerNotes;
Image::ExifTool->new;
sub walk {
    my ($name, $acc, $seen) = @_;
    return if $$seen{$name}++;
    my $t = eval { Image::ExifTool::GetTagTable($name) } or die "no table $name\n";
    my $g1 = $$t{GROUPS}{1};
    $$acc{$g1} = 1 if defined $g1 and length $g1;
    foreach my $key (Image::ExifTool::TagTableKeys($t)) {
        foreach my $info (Image::ExifTool::GetTagInfoList($t, $key)) {
            next unless ref $info eq 'HASH';
            my $tg1 = $$info{Groups} ? $$info{Groups}{1} : undef;
            $$acc{$tg1} = 1 if defined $tg1 and length $tg1;
            my $sub = $$info{SubDirectory} or next;
            my $tt = $$sub{TagTable} or next;
            walk($tt, $acc, $seen);
        }
    }
}
my @roots = map { [$$_{Name}, $$_{SubDirectory} ? $$_{SubDirectory}{TagTable} : undef] }
            grep { ref $_ eq 'HASH' } @Image::ExifTool::MakerNotes::Main;
push @roots, ['CIFF', 'Image::ExifTool::CanonRaw::Main'];
if (($ENV{OXIDEX_MN_MODE} // '') eq 'names') {
    require Image::ExifTool::TagLookup;
    require Image::ExifTool::GPS;
    my $et = Image::ExifTool->new;
    open my $fh, '<', $INC{'Image/ExifTool/TagLookup.pm'} or die;
    my ($in, %names);
    while (<$fh>) {
        $in = 1 if /^my %tagLookup = \(/;
        $in = 0 if $in and /^\);/;
        $names{$1} = 1 if $in and /^\t'([^']+)' => /;
    }
    my $gps = Image::ExifTool::GetTagTable('Image::ExifTool::GPS::Main');
    my %gpsnames;
    foreach my $key (Image::ExifTool::TagTableKeys($gps)) {
        foreach my $info (Image::ExifTool::GetTagInfoList($gps, $key)) {
            $gpsnames{lc $$info{Name}} = 1 if ref $info eq 'HASH';
        }
    }
    foreach my $name (sort keys %names) {
        my @infos = Image::ExifTool::TagLookup::FindTagInfo($name);
        my %mn;
        foreach my $info (@infos) {
            my @g = $et->GetGroup($info);
            $mn{$g[1]} = 1 if $g[0] eq 'MakerNotes';
            print join("\t", 'GPS', $name, $g[0], $g[1]), "\n" if $gpsnames{$name};
        }
        print join("\t", 'MN', $name, join(',', sort keys %mn)), "\n" if %mn;
    }
    exit 0;
}
foreach my $root (@roots) {
    my ($entry, $tt) = @$root;
    # A value-typed entry (no SubDirectory table): the note is one value,
    # holding no tags.
    unless ($tt) { print join("\t", $entry, '', '', ''), "\n"; next; }
    my (%acc, %seen);
    walk($tt, \%acc, \%seen);
    my $g1 = Image::ExifTool::GetTagTable($tt)->{GROUPS}{1};
    $g1 = $entry if $entry eq 'CIFF';
    print join("\t", $entry, $tt, $g1, join(',', sort keys %acc)), "\n";
}
"""


def perl(perl_bin: str, lib: Path, script: str, mode: str = "") -> str:
    env = {k: v for k, v in os.environ.items()
           if k not in {"PERL5LIB", "PERLLIB", "PERL5OPT", "EXIFTOOL", "OXIDEX_MN_MODE"}}
    if mode:
        env["OXIDEX_MN_MODE"] = mode
    run = subprocess.run([perl_bin, f"-I{lib}", "-e", script],
                         capture_output=True, text=True, env=env, check=True)
    return run.stdout


def rust_str(text: str) -> str:
    return '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--exiftool-dir", required=True, type=Path)
    parser.add_argument("--perl", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    pinned = (REPO / ".exiftool-version").read_text().strip()
    lib = args.exiftool_dir / "lib"
    version = perl(args.perl, lib, "use Image::ExifTool; print $Image::ExifTool::VERSION")
    if version != pinned:
        raise SystemExit(f"oracle is ExifTool {version}, pin is {pinned}")

    rows = []
    for line in perl(args.perl, lib, WALK).splitlines():
        entry, table, group, closure = line.split("\t")
        groups = [g for g in closure.split(",") if g]
        if not table:
            if group or groups:
                raise SystemExit(f"{entry}: a value-typed entry with groups")
            rows.append((entry, table, group, groups))
            continue
        if group not in groups:
            raise SystemExit(f"{entry}: root group {group} missing from its own closure")
        rows.append((entry, table, group, groups))
    if len(rows) < 50 or not any(entry == "MakerNoteCanon" for entry, *_ in rows):
        raise SystemExit(f"implausible capture: {len(rows)} roots")

    makernote_names: dict[str, list[str]] = {}
    gps_names: list[tuple[str, str, str]] = []
    for line in perl(args.perl, lib, WALK, mode="names").splitlines():
        kind, *rest = line.split("\t")
        if kind == "MN":
            name, groups = rest
            makernote_names[name] = [g for g in groups.split(",") if g]
        elif kind == "GPS":
            gps_names.append(tuple(rest))
        else:
            raise SystemExit(f"unrecognized capture line {line!r}")
    if len(makernote_names) < 1000 or "whitebalance" not in makernote_names:
        raise SystemExit(f"implausible maker-note name capture: {len(makernote_names)}")
    if not any(name == "gpsdatestamp" for name, *_ in gps_names):
        raise SystemExit("implausible GPS name capture")
    if any(name != name.lower() for name in makernote_names):
        raise SystemExit("a captured name is not lower-case")

    makernotes_pm = lib / "Image/ExifTool/MakerNotes.pm"
    sha = hashlib.sha256(makernotes_pm.read_bytes()).hexdigest()
    out = [
        "// @generated by tools/exiftool-tables/makernote_groups_codegen.py; do not edit.\n",
        f"// ExifTool {pinned}: {len(rows)} maker-note roots "
        "(MakerNotes::Main entries + CIFF).\n",
        "/// Provenance of [`MAKERNOTE_ROOTS`].\n",
        "pub(crate) struct MakerNoteGroupsCapture {\n",
        "    pub exiftool_version: &'static str,\n",
        "    pub makernotes_sha256: &'static str,\n",
        "}\n",
        "pub(crate) const MAKERNOTE_GROUPS_CAPTURE: MakerNoteGroupsCapture = MakerNoteGroupsCapture {\n",
        f"    exiftool_version: {rust_str(pinned)},\n",
        f"    makernotes_sha256: {rust_str(sha)},\n",
        "};\n",
        "/// One maker-note root ExifTool can read a note with.\n",
        "pub(crate) struct MakerNoteRoot {\n",
        "    /// The `MakerNotes::Main` entry (`MakerNoteCanon`), or `CIFF`.\n",
        "    pub entry: &'static str,\n",
        "    /// Its root tag table; empty for a value-typed entry (the note is\n",
        "    /// one value and holds no tags: `MakerNoteUnknownBinary`).\n",
        "    pub table: &'static str,\n",
        "    /// The root table's family-1 group (`CIFF` for the CIFF root); empty\n",
        "    /// for a value-typed entry.\n",
        "    pub group: &'static str,\n",
        "    /// Every family-1 group of every table reachable from the root\n",
        "    /// through `SubDirectory` edges, sorted.\n",
        "    pub closure: &'static [&'static str],\n",
        "}\n",
        "/// Every maker-note root, in `MakerNotes::Main` order, then CIFF.\n",
        "pub(crate) const MAKERNOTE_ROOTS: &[MakerNoteRoot] = &[\n",
    ]
    for entry, table, group, groups in rows:
        out.append(
            f"    MakerNoteRoot {{ entry: {rust_str(entry)}, table: {rust_str(table)}, "
            f"group: {rust_str(group)}, closure: &[{', '.join(rust_str(g) for g in groups)}] }},\n")
    out.append("];\n")
    out.append("/// Lower-case names with a writable `MakerNotes` candidate (pinned\n"
               "/// `FindTagInfo`), each with those candidates' family-1 groups; sorted\n"
               "/// for binary search.\n")
    out.append("pub(crate) const MAKERNOTE_CANDIDATES: &[(&str, &[&str])] = &[\n")
    for name in sorted(makernote_names):
        groups = ", ".join(rust_str(g) for g in makernote_names[name])
        out.append(f"    ({rust_str(name)}, &[{groups}]),\n")
    out.append("];\n")
    out.append("/// Every writable candidate (lower-case name, family 0, family 1) of each\n"
               "/// `GPS::Main` name (pinned `FindTagInfo`).\n")
    out.append("pub(crate) const GPS_NAME_CANDIDATES: &[(&str, &str, &str)] = &[\n")
    for name, g0, g1 in sorted(set(gps_names)):
        out.append(f"    ({rust_str(name)}, {rust_str(g0)}, {rust_str(g1)}),\n")
    out.append("];\n")
    args.output.write_text("".join(out))
    print(f"wrote {args.output}: {len(rows)} roots", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
