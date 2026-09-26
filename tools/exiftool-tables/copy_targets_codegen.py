#!/usr/bin/env python3
"""Emit `src/writers/generated_copy_targets.rs`: where the pinned ExifTool's
`-TagsFromFile` writes each tag it copies.

`SetNewValuesFromFile` (Writer.pl:1254-1590) copies a source tag *by name*:
for every source tag it calls `SetNewValue(<name>, <value>, Replace => 1,
...)`, and `SetNewValue` (Writer.pl:607-1100) resolves the name exactly as it
resolves `-<name>=<value>`: every `FindTagInfo` candidate that is writable
and not protected becomes a new value, the highest-priority ones (the
group's `WRITE_PRIORITY` plus any `Preferred` bump, `Avoid` yielding to the
next best) are *created* (`IsCreating`), and every other one is written only
where the file already carries it ("if tag exists"). A selection (`-all`, a
wildcard) copies with the `Protected` option removed and `NoFlat` set
(Writer.pl:1568-1576); a tag named without wildcards copies with
`Protected => 1` (Writer.pl:1577-1580).

That resolution depends only on the tag name and those options -- not on the
value, and not on the destination file -- so it is captured here once per
writable name (`TagLookup.pm` `%tagLookup`), in both modes, by running the
pinned interpreter's own `SetNewValue` and reading back the new-value hashes
it built (`GetNewTagInfoList` / `GetNewValueHash`): each candidate's tag
name, family-0 group, family-1 write group (`GetWriteGroup1`), directory
(`WriteGroup`), `IsCreating` flag, whether it is a structure (which a
plain value never reaches: `Improperly formed structure`) and whether its
table is `PanasonicRaw::Main` (realised only in a Panasonic RAW). Value conversion
is not what is being captured, so `ConvInv` is replaced by the identity for
the capture: a candidate a real value would fail to convert for is still
listed, and the copy converts (or refuses) the real value itself.

The destination decides which created candidates are realised (Writer.pl
`InitWriteDirs` and the per-format map); that is `writers::copy_targets`'s
job, not this table's.

Also emitted: the tags `SetNewValuesFromFile` never copies from a source
under a selection -- `Protected` and `Binary` (Writer.pl:1570-1572) -- as
(family-0 group, lower-case name) pairs.

Usage:
    copy_targets_codegen.py --exiftool-dir <pinned tree> --perl <pinned perl> \\
        --output src/writers/generated_copy_targets.rs
"""
from __future__ import annotations

import argparse
import hashlib
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
KEY = re.compile(r"^\t'([^']+)' => ")

# Mode bits, matching the Rust constants below.
SELECTED = 1
SELECTED_CREATE = 2
NAMED = 4
NAMED_CREATE = 8
STRUCT = 16
PANASONIC_RAW = 32
# Flag bits: what `SetNewValue` weighs when a group is named (`-GROUP:all`),
# where it ranks the group's candidates itself (Writer.pl:622-825).
AVOID = 1
PERMANENT = 2
PREFERRED_SHIFT = 2

PROBE = r"""
use strict; use warnings;
use Image::ExifTool;
require 'Image/ExifTool/Writer.pl';
{
    no strict 'refs'; no warnings;
    # The capture is of *where* a value goes, not of converting one: an
    # identity ConvInv keeps every candidate a real value could reach.
    *{'Image::ExifTool::ConvInv'} = sub { return ($_[1]); };
}
my @names = map { chomp; $_ } <STDIN>;
for my $mode ('selected', 'named') {
    my $et = Image::ExifTool->new;
    for my $name (@names) {
        my %opts = (Replace => 1);
        if ($mode eq 'named') { $opts{Protected} = 1 } else { $opts{NoFlat} = 1 }
        local $SIG{__WARN__} = sub {};
        $et->SetNewValue($name, 'x', %opts);
        for my $ti ($et->GetNewTagInfoList()) {
            for (my $nvh = $et->GetNewValueHash($ti); $nvh; $nvh = $$nvh{Next}) {
                my $wg = $$nvh{WriteGroup};
                my $prf = defined $$ti{Preferred} ? $$ti{Preferred} : $$ti{Table}{PREFERRED};
                my $perm = $$ti{Permanent};
                $perm = $$ti{Table}{PERMANENT} unless defined $perm;
                $perm = 1 if not defined $perm and $wg eq 'MakerNotes';
                printf "%s\t%s\t%s\t%s\t%s\t%s\t%d\t%d\t%s\t%d\t%d\t%d\n", $mode, $name,
                    $$ti{Name}, $et->GetGroup($ti, 0), $et->GetWriteGroup1($ti, $wg), $wg,
                    $$nvh{IsCreating} ? 1 : 0, $$ti{Struct} ? 1 : 0,
                    $$ti{Table}{TABLE_NAME}, $$ti{Avoid} ? 1 : 0, $perm ? 1 : 0, $prf || 0;
            }
        }
        $et->SetNewValue();
    }
}
"""

PROTECTED_BINARY = r"""
use strict; use warnings;
use Image::ExifTool;
Image::ExifTool::LoadAllTables();
my %seen;
no strict 'refs';
for my $tn (sort(keys %Image::ExifTool::allTables), 'Image::ExifTool::Composite') {
    my $t = Image::ExifTool::GetTagTable($tn) or next;
    for my $id (Image::ExifTool::TagTableKeys($t)) {
        for my $ti (Image::ExifTool::GetTagInfoList($t, $id)) {
            next unless ref $ti eq 'HASH' and $$ti{Protected} and $$ti{Binary};
            my $g0 = $$ti{Groups}{0} || $$t{GROUPS}{0};
            $seen{"$g0\t" . lc $$ti{Name}} = 1;
        }
    }
}
print "$_\n" for sort keys %seen;
"""


def block_keys(text: str, opener: str) -> list[str]:
    start = text.index(opener)
    end = text.index("\n);\n", start)
    keys = []
    for line in text[start:end].splitlines()[1:]:
        match = KEY.match(line)
        if match is None:
            raise SystemExit(f"unrecognized {opener!r} line: {line!r}")
        keys.append(match.group(1))
    return keys


def perl(perl_bin: str, lib: Path, script: str, stdin: str = "") -> str:
    env = {k: v for k, v in os.environ.items()
           if k not in {"PERL5LIB", "PERLLIB", "PERL5OPT", "EXIFTOOL"}}
    run = subprocess.run([perl_bin, f"-I{lib}", "-e", script], input=stdin,
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

    lookup_pm = lib / "Image/ExifTool/TagLookup.pm"
    writer_pl = lib / "Image/ExifTool/Writer.pl"
    names = sorted(set(block_keys(lookup_pm.read_text(encoding="latin-1"),
                                  "my %tagLookup = (")))
    rows = perl(args.perl, lib, PROBE, "\n".join(names) + "\n").splitlines()

    # requested name -> {(tag, group0, group1, dir, PanasonicRaw, flags): mode bits}
    targets: dict[str, dict[tuple[str, str, str, str, bool, int], int]] = {}
    for row in rows:
        (mode, name, tag, group0, group1, directory, creating, struct, table, avoid,
         permanent, preferred) = row.split("\t")
        if int(preferred) > 3:
            raise SystemExit(f"a Preferred level does not fit two bits: {row}")
        flags = ((AVOID if avoid == "1" else 0) | (PERMANENT if permanent == "1" else 0)
                 | int(preferred) << PREFERRED_SHIFT)
        bits = (SELECTED | (SELECTED_CREATE if creating == "1" else 0)
                if mode == "selected"
                else NAMED | (NAMED_CREATE if creating == "1" else 0))
        if struct == "1":
            bits |= STRUCT
        panasonic = table == "Image::ExifTool::PanasonicRaw::Main"
        if panasonic:
            bits |= PANASONIC_RAW
        entry = targets.setdefault(name, {})
        key = (tag, group0, group1, directory, panasonic, flags)
        entry[key] = entry.get(key, 0) | bits

    # Controls the capture must reproduce (pinned 13.59, `-v2`): Make is
    # created in IFD0 and PNG text and written to XMP-tiff where it exists;
    # IFD0 ImageWidth is Protected, so a selection creates XMP-tiff instead,
    # and a named copy IFD0.
    def bits(name, tag, group1):
        return sum(v for (t, _, g1, _, panasonic, _), v in targets.get(name, {}).items()
                   if t == tag and g1 == group1 and not panasonic) & ~STRUCT
    controls = [
        (bits("make", "Make", "IFD0"), SELECTED | SELECTED_CREATE | NAMED | NAMED_CREATE),
        (bits("make", "Make", "PNG"), SELECTED | SELECTED_CREATE | NAMED | NAMED_CREATE),
        (bits("make", "Make", "XMP-tiff"), SELECTED | NAMED),
        (bits("imagewidth", "ImageWidth", "XMP-tiff"), SELECTED | SELECTED_CREATE | NAMED),
        (bits("imagewidth", "ImageWidth", "IFD0"), NAMED | NAMED_CREATE),
    ]
    for got, want in controls:
        if got != want:
            raise SystemExit(f"capture disagrees with a pinned control: {controls}")

    protected_binary = [line.split("\t")
                        for line in perl(args.perl, lib, PROTECTED_BINARY).splitlines()]

    lookup_sha = hashlib.sha256(lookup_pm.read_bytes()).hexdigest()
    writer_sha = hashlib.sha256(writer_pl.read_bytes()).hexdigest()
    count = sum(len(entry) for entry in targets.values())
    out = [
        "// @generated by tools/exiftool-tables/copy_targets_codegen.py; do not edit.\n",
        f"// ExifTool {pinned}: SetNewValue's new-value hashes for {len(targets)} writable "
        f"names ({count} candidates).\n",
        "/// Provenance of [`COPY_TARGETS`].\n",
        "pub(crate) struct CopyTargetsCapture {\n",
        "    pub exiftool_version: &'static str,\n",
        "    pub tag_lookup_sha256: &'static str,\n",
        "    pub writer_sha256: &'static str,\n",
        "}\n",
        "pub(crate) const COPY_TARGETS_CAPTURE: CopyTargetsCapture = CopyTargetsCapture {\n",
        f"    exiftool_version: \"{pinned}\",\n",
        f"    tag_lookup_sha256: \"{lookup_sha}\",\n",
        f"    writer_sha256: \"{writer_sha}\",\n",
        "};\n",
        "/// A candidate of a selection (`-all`, a wildcard: `Protected` removed).\n",
        f"pub(crate) const SELECTED: u8 = {SELECTED};\n",
        "/// ... that the selection creates (`IsCreating`).\n",
        f"pub(crate) const SELECTED_CREATE: u8 = {SELECTED_CREATE};\n",
        "/// A candidate of a tag named without wildcards (`Protected => 1`).\n",
        f"pub(crate) const NAMED: u8 = {NAMED};\n",
        "/// ... that the named copy creates (`IsCreating`).\n",
        f"pub(crate) const NAMED_CREATE: u8 = {NAMED_CREATE};\n",
        "/// A structure (`Struct`): `SetNewValue` refuses a plain value for it\n",
        "/// (`Improperly formed structure`); only a structure copies to it.\n",
        f"pub(crate) const STRUCT: u8 = {STRUCT};\n",
        "/// A `PanasonicRaw::Main` tag: only a Panasonic RAW's IFD0 is read and\n",
        "/// written with that table (ExifTool.pm:8646-8659), so it is realised in\n",
        "/// no other file.\n",
        f"pub(crate) const PANASONIC_RAW: u8 = {PANASONIC_RAW};\n",
        "/// `Avoid`: yields creation to an alternative (Writer.pl:792-825).\n",
        f"pub(crate) const AVOID: u8 = {AVOID};\n",
        "/// `Permanent` (a maker-note tag): edited where it exists, never created.\n",
        f"pub(crate) const PERMANENT: u8 = {PERMANENT};\n",
        "/// Where the `Preferred` / table `PREFERRED` level (0-3) sits in `flags`.\n",
        f"pub(crate) const PREFERRED_SHIFT: u8 = {PREFERRED_SHIFT};\n",
        "/// One new-value hash `SetNewValue` builds for a copied name.\n",
        "pub(crate) struct CopyTarget {\n",
        "    /// The tag it writes (another name than the copied one for a\n",
        "    /// `WriteAlso` or structure field).\n",
        "    pub name: &'static str,\n",
        "    pub group0: &'static str,\n",
        "    /// `GetWriteGroup1`: the family-1 group it is written to.\n",
        "    pub group1: &'static str,\n",
        "    /// `WriteGroup`: the directory `InitWriteDirs` maps.\n",
        "    pub dir: &'static str,\n",
        "    /// [`SELECTED`] | [`SELECTED_CREATE`] | [`NAMED`] | [`NAMED_CREATE`] |\n",
        "    /// [`STRUCT`] | [`PANASONIC_RAW`].\n",
        "    pub modes: u8,\n",
        "    /// [`AVOID`] | [`PERMANENT`] | the `Preferred` level <<\n",
        "    /// [`PREFERRED_SHIFT`].\n",
        "    pub flags: u8,\n",
        "}\n",
        "const fn t(name: &'static str, group0: &'static str, group1: &'static str, "
        "dir: &'static str, modes: u8, flags: u8) -> CopyTarget {\n",
        "    CopyTarget { name, group0, group1, dir, modes, flags }\n",
        "}\n",
        "/// Lower-case copied name -> its candidates, sorted by name for binary search.\n",
        "#[rustfmt::skip]\n",
        "pub(crate) static COPY_TARGETS: &[(&str, &[CopyTarget])] = &[\n",
    ]
    for name in sorted(targets):
        rows = [f"t({rust_str(tag)}, {rust_str(group0)}, {rust_str(group1)}, "
                f"{rust_str(directory)}, {modes}, {flags})"
                for (tag, group0, group1, directory, _, flags), modes
                in sorted(targets[name].items())]
        if len(rows) == 1:
            out.append(f"    ({rust_str(name)}, &[{rows[0]}]),\n")
        else:
            out.append(f"    ({rust_str(name)}, &[\n")
            out.extend(f"        {row},\n" for row in rows)
            out.append("    ]),\n")
    out.append("];\n")
    out.append("/// (family-0 group, lower-case name) of every `Protected` `Binary` tag: a\n"
               "/// selection never copies one *from* a source (Writer.pl:1570-1572).\n")
    out.append("#[rustfmt::skip]\n")
    out.append("pub(crate) static PROTECTED_BINARY_SOURCES: &[(&str, &str)] = &[\n")
    for group0, name in protected_binary:
        out.append(f"    ({rust_str(group0)}, {rust_str(name)}),\n")
    out.append("];\n")
    args.output.write_text("".join(out))
    print(f"wrote {args.output}: {len(targets)} names, {count} candidates, "
          f"{len(protected_binary)} protected binary tags", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
