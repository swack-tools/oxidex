# Nikon AFPointsUsed / PrimaryAFPoint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Decode Nikon `AFPointsUsed` and `PrimaryAFPoint` (currently deliberately unimplemented in `af_info2.rs`) across all `AFInfo2` versions (0100–0402), by transcribing ExifTool's ten per-body point-name grids mechanically instead of guessing them.

**Architecture:** A one-time Perl extractor slices ExifTool's ten `afPoints*` lexicals out of `Nikon.pm` and evals them to JSON; a Python codegen step turns that JSON into a committed Rust const table (`af_points.rs`). Two small pure print-conversion functions (bitmap→lookup, bitmap→computed-grid-name) port ExifTool's `PrintAFPoints`/`PrintAFPointsGrid`. The existing per-version `match` in `af_info2.rs` gains the tag reads at their documented byte offsets, gated on the same `Condition`s ExifTool uses.

**Tech Stack:** Rust (workspace crate `oxidex`), Perl (extraction, matches existing `tools/exiftool-tables/*.pl` scripts), Python (codegen, matches existing `tools/exiftool-tables/codegen*.py`).

## Global Constraints

- Scope is exactly `AFPointsUsed` and `PrimaryAFPoint`. Do not touch `AFPointsInFocus`, `AFPointsSelected`, `FocusPositionHorizontal`/`Vertical` — out of scope per the spec.
- Never approximate: a body/version this plan doesn't cover must produce no tag, not a guessed one. All four `AFInfo2` families ARE covered by this plan, so after it lands there should be no remaining "unclaimed but plausible" case for these two tags.
- Every generated Rust value must trace to a specific line/table in `/tmp/oxidex-exiftool-cache/exiftool/lib/Image/ExifTool/Nikon.pm`.
- Gates before merge: `cargo fmt --all`, `cargo clippy --workspace`, `cargo test --workspace`.
- Commit author email must be `swackhamer@users.noreply.github.com`; commit with `git -c commit.gpgsign=false commit --no-gpg-sign` (SSH signing hangs non-interactively).
- Verification bar: `exiftool -G1 -s` vs `oxidex -e -s`, byte-for-byte, per file — not corpus totals.

---

## File Structure

- **Create** `tools/exiftool-tables/dump_af_points.pl` — Perl extractor, slices+evals the ten `afPoints*` lexicals, emits JSON.
- **Create** `tools/exiftool-tables/codegen_af_points.py` — JSON → committed Rust const file.
- **Create** `src/parsers/tiff/makernotes/nikon/af_points.rs` — generated Rust data (the ten tables) plus the two hand-written print-conversion functions and their unit tests (generated block clearly delimited from hand-written block; see Task 2/3).
- **Modify** `src/parsers/tiff/makernotes/nikon/af_info2.rs` — add the `AFPointsUsed`/`PrimaryAFPoint` reads to each version arm of `parse_af_info2`, and add a `model: Option<&str>` parameter for the V0400 family's model-gated dispatch.
- **Modify** `src/parsers/tiff/makernotes/nikon.rs:1449` — pass `model` through to `parse_af_info2`.

---

## Task 1: Perl extractor for the ten point-name tables

**Files:**
- Create: `tools/exiftool-tables/dump_af_points.pl`
- Test: manual run against the real `Nikon.pm`, verified by shape assertions in the script itself (a standalone Perl script has no external test harness in this repo — `dump_tables.pl` and `dump_filetypes.pl` are validated the same way, by their own internal completeness checks plus the downstream `verify.py`/`conformance.py` pass in Task 8).

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces: `tools/exiftool-tables/af_points.json`, shape:
  ```json
  {
    "afPoints11":  {"kind": "array", "points": ["Center", "Top", ...]},
    "afPoints51":  {"kind": "hash",  "points": {"1": "C6", "11": "C5", ...}},
    "afPoints39":  {"kind": "hash",  "points": {...}},
    "afPoints105": {"kind": "hash",  "points": {...}},
    "afPoints135": {"kind": "hash",  "points": {...}},
    "afPoints153": {"kind": "hash",  "points": {...}},
    "afPoints81":  {"kind": "hash",  "points": {...}},
    "afPoints231": {"kind": "array", "points": ["A1", "A2", ...]},
    "afPoints299": {"kind": "array", "points": [...]},
    "afPoints405": {"kind": "array", "points": [...]}
  }
  ```
  Later tasks consume this JSON via `codegen_af_points.py` (Task 2). No other task reads it directly.

- [ ] **Step 1: Write the extractor script**

```perl
#!/usr/bin/env perl
# Slices the ten `afPoints*` point-name tables out of Nikon.pm and evals
# them with real Perl, mirroring the %fileTypeExt precedent documented in
# docs/TRANSCRIPTION.md ("One table is not reachable this way" -- these are
# `my` lexicals, invisible to dump_tables.pl's symbol-table walk).
use strict;
use warnings;
use JSON::PP;

my $nikon_pm = $ARGV[0] or die "usage: dump_af_points.pl <path/to/Nikon.pm> <out.json>\n";
my $out_path = $ARGV[1] or die "usage: dump_af_points.pl <path/to/Nikon.pm> <out.json>\n";

open my $fh, '<', $nikon_pm or die "open $nikon_pm: $!\n";
local $/;
my $src = <$fh>;
close $fh;

# Each hash-shaped table: `my %afPointsNNN = ( ... );`
my @hash_tables = qw(afPoints51 afPoints39 afPoints105 afPoints135 afPoints153 afPoints81);
# Each array-shaped table: `my @afPointsNNN = ( ... );` (231/299/405 are
# `qw()` lists; afPoints11 is hash-shaped in the source but semantically an
# 11-slot ordered list once the BITMASK/0/0x7ff special keys are stripped --
# handled separately below).
my @array_tables = qw(afPoints231 afPoints299 afPoints405);

my %result;

for my $name (@hash_tables) {
    $src =~ /my \s+ \%\Q$name\E \s* = \s* \( (.*?) \) \s* ; /sx
        or die "shape changed: could not find 'my \%$name = ( ... );' in $nikon_pm\n";
    my $literal = "my \%tmp = ($1);";
    my %tmp;
    { no strict 'vars'; eval $literal; die "eval \%$name failed: $@" if $@; }
    $result{$name} = { kind => 'hash', points => { %tmp } };
}

for my $name (@array_tables) {
    $src =~ /my \s+ \@\Q$name\E \s* = \s* \( \s* qw\( (.*?) \) \s* \) \s* ; /sx
        or die "shape changed: could not find 'my \@$name = (qw(...));' in $nikon_pm\n";
    my @tmp = split ' ', $1;
    $result{$name} = { kind => 'array', points => [ @tmp ] };
}

# afPoints11: `my %afPoints11 = ( 0 => '(none)', 0x7ff => 'All 11 Points',
# BITMASK => { 0 => 'Center', ..., 10 => 'Far Right' } );` -- extract the
# BITMASK sub-hash, ordered 0..10, as a plain 11-slot array (see af_info2.rs
# Task 4 for how the '(none)'/'All 11 Points' literals are handled in Rust).
{
    $src =~ /my \s+ \%afPoints11 \s* = \s* \( (.*?) \) \s* ; /sx
        or die "shape changed: could not find 'my \%afPoints11 = ( ... );'\n";
    my $literal = "my \%tmp = ($1);";
    my %tmp;
    { no strict 'vars'; eval $literal; die "eval \%afPoints11 failed: $@" if $@; }
    my $bitmask = $tmp{BITMASK} or die "afPoints11 has no BITMASK key\n";
    my @ordered = map { $bitmask->{$_} } 0 .. 10;
    die "afPoints11 BITMASK is not 0..10\n" if grep { !defined } @ordered;
    $result{afPoints11} = { kind => 'array', points => [ @ordered ] };
}

for my $name (@hash_tables, @array_tables, 'afPoints11') {
    die "missing table: $name\n" unless exists $result{$name};
}

open my $out, '>', $out_path or die "open $out_path: $!\n";
print $out JSON::PP->new->canonical->pretty->encode(\%result);
close $out;
print "wrote $out_path: " . join(', ', map { "$_=" . (ref $result{$_}{points} eq 'ARRAY' ? scalar(@{$result{$_}{points}}) : scalar(keys %{$result{$_}{points}})) } sort keys %result) . "\n";
```

- [ ] **Step 2: Run it against the real ExifTool source and inspect the output**

Run:
```bash
perl tools/exiftool-tables/dump_af_points.pl \
  /tmp/oxidex-exiftool-cache/exiftool/lib/Image/ExifTool/Nikon.pm \
  tools/exiftool-tables/af_points.json
```
Expected: prints a summary line with all ten table names and counts —
`afPoints11=11, afPoints51=51, afPoints39=39, afPoints105=105, afPoints135=135, afPoints153=153, afPoints81=81, afPoints231=231, afPoints299=299, afPoints405=405`
(the array tables list every grid cell including unused ones the DSLR AF sensor doesn't populate — e.g. `afPoints405` for Z8/Z9 is `undef[51]` = 408 bits but only 405 named cells exist in the qw() list; a byte-count mismatch here is expected and handled in Rust, not here).

Spot-check the JSON by hand: `afPoints51` must have `"1": "C6"` (center point, per Nikon.pm:1466), `afPoints81` must have `"1": "E5"` (Nikon.pm:1616), `afPoints11` (the derived array) must be exactly `["Center","Top","Bottom","Mid-left","Mid-right","Upper-left","Upper-right","Lower-left","Lower-right","Far Left","Far Right"]` (Nikon.pm:1444-1456, BITMASK order 0..10).

- [ ] **Step 3: Commit**

```bash
git add tools/exiftool-tables/dump_af_points.pl tools/exiftool-tables/af_points.json
git -c commit.gpgsign=false commit --no-gpg-sign --author="swackhamer <swackhamer@users.noreply.github.com>" -m "$(cat <<'EOF'
tools(nikon): extract AF point-name grids from Nikon.pm lexicals

afPoints11/51/39/105/135/153/81/231/299/405 are Perl `my` lexicals in
Nikon.pm, invisible to dump_tables.pl's symbol-table walk -- the same
situation TRANSCRIPTION.md documents for %fileTypeExt. Slices each literal
out of the source and evals it with real Perl rather than retyping ~1,200
(index, name) pairs by hand.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: Codegen — JSON to committed Rust consts

**Files:**
- Create: `tools/exiftool-tables/codegen_af_points.py`
- Create (generated, committed): `src/parsers/tiff/makernotes/nikon/af_points.rs` (data section only — Task 3 appends the hand-written print-conversion functions to the same file, below a clear `// --- hand-written below ---` marker the codegen script must not overwrite).

**Interfaces:**
- Consumes: `tools/exiftool-tables/af_points.json` (Task 1).
- Produces: Rust consts every later task reads by name:
  - `pub const AF_POINTS_11: &[&str]` (11 entries, positional: index 0 = bit 0 = "Center", ... index 10 = "Far Right")
  - `pub const AF_POINTS_51: &[(u8, &str)]` (bit-number → name, bit-number 1-based)
  - `pub const AF_POINTS_39: &[(u8, &str)]`
  - `pub const AF_POINTS_105: &[(u8, &str)]`
  - `pub const AF_POINTS_135: &[(u8, &str)]`
  - `pub const AF_POINTS_153: &[(u8, &str)]`
  - `pub const AF_POINTS_81: &[(u8, &str)]`
  - `pub const AF_POINTS_231: &[&str]` (positional, index 0 = grid cell 0)
  - `pub const AF_POINTS_299: &[&str]`
  - `pub const AF_POINTS_405: &[&str]`

- [ ] **Step 1: Write the codegen script**

```python
#!/usr/bin/env python3
"""JSON (from dump_af_points.pl) -> src/parsers/tiff/makernotes/nikon/af_points.rs

Only the data section (above the "hand-written below" marker) is
regenerated; everything below the marker is preserved verbatim so this
script can be re-run without clobbering the print-conversion functions.
"""
import json
import subprocess
import sys
from pathlib import Path

MARKER = "// --- hand-written below: do not edit above this line by hand ---"

HASH_TABLES = ["afPoints51", "afPoints39", "afPoints105", "afPoints135", "afPoints153", "afPoints81"]
ARRAY_TABLES = ["afPoints11", "afPoints231", "afPoints299", "afPoints405"]


def rust_name(name: str) -> str:
    # afPoints51 -> AF_POINTS_51
    digits = "".join(c for c in name if c.isdigit())
    return f"AF_POINTS_{digits}"


def emit_hash(name: str, points: dict) -> str:
    pairs = sorted(((int(k), v) for k, v in points.items()), key=lambda p: p[0])
    body = ", ".join(f'({k}, "{v}")' for k, v in pairs)
    return f"pub const {rust_name(name)}: &[(u8, &str)] = &[{body}];\n"


def emit_array(name: str, points: list) -> str:
    body = ", ".join(f'"{p}"' for p in points)
    return f"pub const {rust_name(name)}: &[&str] = &[{body}];\n"


def main() -> None:
    json_path = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("tools/exiftool-tables/af_points.json")
    rs_path = Path(sys.argv[2]) if len(sys.argv) > 2 else Path(
        "src/parsers/tiff/makernotes/nikon/af_points.rs"
    )
    data = json.loads(json_path.read_text())

    lines = [
        "//! Nikon AF point-name grids, transcribed from ExifTool's Nikon.pm\n",
        "//! `afPoints*` lexicals by `tools/exiftool-tables/dump_af_points.pl` +\n",
        "//! `codegen_af_points.py`. Regenerate with both scripts; do not hand-edit\n",
        "//! the data section below.\n\n",
    ]
    for name in HASH_TABLES:
        lines.append(emit_hash(name, data[name]["points"]))
    for name in ARRAY_TABLES:
        lines.append(emit_array(name, data[name]["points"]))
    lines.append(f"\n{MARKER}\n")

    existing = rs_path.read_text() if rs_path.exists() else ""
    hand_written = ""
    if MARKER in existing:
        hand_written = existing.split(MARKER, 1)[1]

    rs_path.write_text("".join(lines) + hand_written)
    subprocess.run(["rustfmt", str(rs_path)], check=True)
    print(f"wrote {rs_path}")


if __name__ == "__main__":
    main()
```

- [ ] **Step 2: Run it, creating the initial file (hand-written section empty for now — Task 3 fills it)**

Run:
```bash
python3 tools/exiftool-tables/codegen_af_points.py \
  tools/exiftool-tables/af_points.json \
  src/parsers/tiff/makernotes/nikon/af_points.rs
```
Expected: `wrote src/parsers/tiff/makernotes/nikon/af_points.rs`. The file exists with ten `pub const` declarations and a trailing marker line, nothing below it yet.

- [ ] **Step 3: Verify it compiles as a standalone module stub**

Add a temporary `mod af_points;` to `src/parsers/tiff/makernotes/nikon/mod.rs` (or wherever sibling modules like `af_info2` are declared — check with `grep -n "mod af_info2" src/parsers/tiff/makernotes/nikon.rs`) so the new file is part of the build.

Run: `cargo build 2>&1 | tail -30`
Expected: compiles clean (unused-const warnings are fine at this point — Task 3/4 consume them). If `mod af_info2;` lives in `nikon.rs` itself rather than a `mod.rs`, add `mod af_points;` there instead, at the same nesting level.

- [ ] **Step 4: Spot-check three generated constants against the source by hand**

Run: `grep -A2 'AF_POINTS_51:' src/parsers/tiff/makernotes/nikon/af_points.rs | head -3`
Expected: contains `(1, "C6")` (Nikon.pm:1466) and `(30, "C10")` (Nikon.pm:1471 col 3).
Run: `grep -A2 'AF_POINTS_11:' src/parsers/tiff/makernotes/nikon/af_points.rs`
Expected: `&["Center", "Top", "Bottom", "Mid-left", "Mid-right", "Upper-left", "Upper-right", "Lower-left", "Lower-right", "Far Left", "Far Right"]` (Nikon.pm:1444-1456).

- [ ] **Step 5: Commit**

```bash
git add tools/exiftool-tables/codegen_af_points.py src/parsers/tiff/makernotes/nikon/af_points.rs src/parsers/tiff/makernotes/nikon.rs
git -c commit.gpgsign=false commit --no-gpg-sign --author="swackhamer <swackhamer@users.noreply.github.com>" -m "$(cat <<'EOF'
tools(nikon): codegen AF point-name grids into af_points.rs

Turns tools/exiftool-tables/af_points.json into committed Rust consts,
one per Nikon.pm table (AF_POINTS_51 etc.). Regenerable; the data section
is separated from the hand-written print-conversion functions that land
in the next commit by a marker comment codegen_af_points.py preserves.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Print-conversion functions (bitmap-to-name)

**Files:**
- Modify: `src/parsers/tiff/makernotes/nikon/af_points.rs` (append below the `// --- hand-written below ---` marker from Task 2)

**Interfaces:**
- Consumes: the ten `AF_POINTS_*` consts (Task 2).
- Produces (consumed by Tasks 4-7):
  ```rust
  pub fn print_af_points_lookup(bits: &[u8], table: &[(u8, &str)]) -> String
  pub fn print_af_points_array(bits: &[u8], table: &[&str]) -> String
  pub fn print_af_points_grid(bits: &[u8], ncols: u16) -> String
  ```

- [ ] **Step 1: Write the failing tests**

Append to `af_points.rs` (below the marker):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // NikonD850.jpg (153-point, V0101): AFPointsUsed = E9. Bit 1 (bit-number
    // 1, 1-based) is byte 0 bit 0.
    #[test]
    fn print_af_points_lookup_single_bit() {
        let bits = [0x01u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(print_af_points_lookup(&bits, AF_POINTS_153), "E9");
    }

    // NikonD7000.jpg (39-point, V0100): AFPointsUsed =
    // C1,C2,C3,C5,C6,C7,D2,D3,D5,E1,E2 -- several bits set across bytes,
    // exercises the ExifTool sort order (numeric-aware: "C1" before "C10").
    #[test]
    fn print_af_points_lookup_multiple_bits_sorted() {
        // bit-numbers (1-based) for C1,C2,C3,C5,C6,C7,D2,D3,D5,E1,E2 in
        // afPoints39 (Nikon.pm:1484-1495): C1=49,C2=44,C3=39,C5=11,C6=1,
        // C7=6,D2=47,D3=33,D5=14,E1=15,E2=5.
        let bit_numbers = [49u32, 44, 39, 11, 1, 6, 47, 33, 14, 15, 5];
        let mut bits = [0u8; 5];
        for n in bit_numbers {
            let i = (n - 1) / 8;
            let j = (n - 1) % 8;
            bits[i as usize] |= 1 << j;
        }
        assert_eq!(
            print_af_points_lookup(&bits, AF_POINTS_39),
            "C1,C2,C3,C5,C6,C7,D2,D3,D5,E1,E2"
        );
    }

    #[test]
    fn print_af_points_lookup_none_set() {
        let bits = [0u8; 7];
        assert_eq!(print_af_points_lookup(&bits, AF_POINTS_51), "(none)");
    }

    // GetAFPointGrid(val=82, ncol=15) = chr(65 + 82/15) + (82 - 15*5 + 1)
    // = chr(65+5) + 8 = "F8" -- the Nikon 1 S2 165-point center point
    // (Nikon.pm:4658).
    #[test]
    fn print_af_points_grid_center_point() {
        // bit 82 is byte 10 (82/8=10), bit offset 2 (82%8=2).
        let mut bits = [0u8; 21];
        bits[10] = 1 << 2;
        assert_eq!(print_af_points_grid(&bits, 15), "F8");
    }

    #[test]
    fn print_af_points_grid_none_set() {
        let bits = [0u8; 21];
        assert_eq!(print_af_points_grid(&bits, 15), "(none)");
    }

    #[test]
    fn print_af_points_array_positional() {
        let table: &[&str] = &["A1", "A2", "A3"];
        let mut bits = [0u8; 1];
        bits[0] = 1 << 1; // bit index 1 -> "A2"
        assert_eq!(print_af_points_array(&bits, table), "A2");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p oxidex nikon::af_points 2>&1 | tail -20`
Expected: FAIL — `print_af_points_lookup`/`print_af_points_grid`/`print_af_points_array` not found (functions don't exist yet).

- [ ] **Step 3: Write the implementation**

Append above the test module (still below the codegen marker):

```rust
/// Port of ExifTool's `PrintAFPoints` (Nikon.pm ~line 13307). Walks `bits`
/// bit by bit; for each set bit, looks up bit-number+1 (1-based, matching
/// ExifTool's `$i*8+$j+1`) in `table`. Unmatched bit numbers are silently
/// skipped (as in ExifTool: `push @points, $point if defined $point`).
/// Sort order matches ExifTool's numeric-aware comparator: same-length
/// names sort lexically; a 2-char name (letter + single digit) sorts as if
/// zero-padded, so "C1" precedes "C10".
pub fn print_af_points_lookup(bits: &[u8], table: &[(u8, &str)]) -> String {
    let mut points = Vec::new();
    for (i, byte) in bits.iter().enumerate() {
        if *byte == 0 {
            continue;
        }
        for j in 0..8u32 {
            if byte & (1 << j) == 0 {
                continue;
            }
            let bit_number = (i as u32) * 8 + j + 1;
            if let Ok(bit_number) = u8::try_from(bit_number)
                && let Some((_, name)) = table.iter().find(|(n, _)| *n == bit_number)
            {
                points.push(*name);
            }
        }
    }
    if points.is_empty() {
        return "(none)".to_string();
    }
    points.sort_by(|a, b| af_point_sort_key(a).cmp(&af_point_sort_key(b)));
    points.join(",")
}

/// Same bitmap walk as `print_af_points_lookup`, but for the three tables
/// ExifTool stores as plain positional arrays (`afPoints231/299/405`):
/// `table[bit_index]`, 0-based, no +1 offset (ExifTool: `$$afPoints[$i*8+$j]`).
pub fn print_af_points_array(bits: &[u8], table: &[&str]) -> String {
    let mut points = Vec::new();
    for (i, byte) in bits.iter().enumerate() {
        if *byte == 0 {
            continue;
        }
        for j in 0..8usize {
            if byte & (1 << j) == 0 {
                continue;
            }
            if let Some(name) = table.get(i * 8 + j) {
                points.push(*name);
            }
        }
    }
    if points.is_empty() {
        return "(none)".to_string();
    }
    points.sort_by(|a, b| af_point_sort_key(a).cmp(&af_point_sort_key(b)));
    points.join(",")
}

/// Port of `PrintAFPointsGrid` + `GetAFPointGrid` (Nikon.pm ~13361, 13378):
/// the point name is *computed* from (row, col) rather than looked up.
/// `row = bit / ncols`, `col = bit - ncols*row + 1`, name = letter(65+row)
/// followed by `col`. ExifTool's grid variant does not sort the output
/// (`return join ',', @points`, no `sort`) -- points come out in bit order.
pub fn print_af_points_grid(bits: &[u8], ncols: u16) -> String {
    let mut points = Vec::new();
    for (i, byte) in bits.iter().enumerate() {
        if *byte == 0 {
            continue;
        }
        for j in 0..8u32 {
            if byte & (1 << j) == 0 {
                continue;
            }
            let bit = (i as u32) * 8 + j;
            let row = bit / (ncols as u32);
            let col = bit - (ncols as u32) * row + 1;
            let Some(letter) = char::from_u32(65 + row) else {
                continue;
            };
            points.push(format!("{letter}{col}"));
        }
    }
    if points.is_empty() {
        return "(none)".to_string();
    }
    points.join(",")
}

/// ExifTool's point-name comparator (Nikon.pm PrintAFPoints):
/// same-length names compare directly; a 2-char name (row letter + single
/// digit column) is treated as zero-padded so "C1" < "C10".
fn af_point_sort_key(name: &str) -> String {
    if name.len() == 2 {
        let mut chars = name.chars();
        let letter = chars.next().unwrap_or(' ');
        let digit = chars.next().unwrap_or(' ');
        format!("{letter}0{digit}")
    } else {
        name.to_string()
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p oxidex nikon::af_points 2>&1 | tail -20`
Expected: PASS, all 6 tests green.

- [ ] **Step 5: Commit**

```bash
git add src/parsers/tiff/makernotes/nikon/af_points.rs
git -c commit.gpgsign=false commit --no-gpg-sign --author="swackhamer <swackhamer@users.noreply.github.com>" -m "$(cat <<'EOF'
feat(nikon): port PrintAFPoints/PrintAFPointsGrid bitmap decoders

Direct ports of Nikon.pm's two AF-point print-conversion algorithms:
print_af_points_lookup/print_af_points_array walk a bitmap and look up
each set bit in a point-name table (hash- or array-shaped); print_af_points_grid
computes the point name from (row, col) instead of a lookup, for the three
tables ExifTool authored as plain sequential grids. No write-side/inverse,
since oxidex doesn't write MakerNotes.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Wire V0100 / V0101 dispatch

**Files:**
- Modify: `src/parsers/tiff/makernotes/nikon/af_info2.rs`

**Interfaces:**
- Consumes: `af_points::{print_af_points_lookup, AF_POINTS_11, AF_POINTS_51, AF_POINTS_39, AF_POINTS_153}` (Tasks 2/3).
- Produces: `Nikon:AFPointsUsed` and `Nikon:PrimaryAFPoint` tags inserted for `AFInfo2Version` `"0100"` and `"0101"`.

**Background — exact offsets/conditions from Nikon.pm:**
- V0100 (Nikon.pm:4137-4184 header, 4181-4297 body): offset 7 = `PrimaryAFPoint` (single raw byte, direct lookup — not a bitmap), offset 8 = `AFPointsUsed` (bitmap, byte length depends on schema). `FocusPointSchema` (already read at offset 6 into `FOCUS_POINT_SCHEMA_V0100`) selects: `1` → 51-point (`undef[7]`, `AF_POINTS_51`), `2` → 11-point (`undef[2]`, little-endian `int16u` **BITMASK**, not the byte-array bitmap walk — see below), `3` → 39-point (`undef[5]`, `AF_POINTS_39`), `0`/unclaimed → `"(none)"` for both tags.
- V0101 (Nikon.pm:4360-4483): offset 8 = `AFPointsUsed` (same three schema branches as V0100, but schema `7` → 153-point `undef[20]`/`AF_POINTS_153` instead of V0100's 39-point), offset `0x44` (68) = `PrimaryAFPoint` (single raw byte, same direct-lookup shape, `FocusPointSchema` `1`/`2`/`7`/`0`).
- The 11-point schema is *not* the bitmap walk from Task 3: `AFPointsUsed`'s PrintConv is a raw `int16u` **BITMASK** hash (Nikon.pm:4258-4270 / 4396-4408 duplicate of `%afPoints11`'s own `PrintConv` block, not a call to `PrintAFPoints`). `0` → `"(none)"`, `0x7ff` → `"All 11 Points"`, otherwise each set bit `n` (0-based) maps to `AF_POINTS_11[n]`, comma-joined (ExifTool's `BITMASK` PrintConv sorts by bit position, ascending — same order as `AF_POINTS_11` itself, so no separate sort is needed).
- `PrimaryAFPoint`'s direct-lookup tables reuse the *same* per-schema table as `AFPointsUsed` (`AF_POINTS_51`/`AF_POINTS_39`/`AF_POINTS_153`), with `0 → "(none)"` and a `1 → "{center} (Center)"` override — bit-number `1` is documented as the center point in every one of these tables (Nikon.pm comments: `1 => 'C6'` for 51/39-point, `1 => 'E9'` for 153-point). For schema `2` (11-point), `PrimaryAFPoint` uses its own direct 1..11 → name table (Nikon.pm:4517-4528), which is `AF_POINTS_11` shifted by one (raw value `v` → `AF_POINTS_11[v-1]`), with no "(Center)" suffix (11-point mode has no single distinguished center per ExifTool's source).

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` block in `af_info2.rs`:

```rust
#[test]
fn v0100_51point_reports_center_from_real_sample_shape() {
    // NikonD3.jpg: AFPointsUsed=C6, PrimaryAFPoint=C6 (Center).
    // FocusPointSchema=1 (51-point), AFPointsUsed bit-number 1 = byte0 bit0.
    let mut data = vec![0u8; 16];
    data[..4].copy_from_slice(b"0100");
    data[6] = 1; // FocusPointSchema = 51-point
    data[7] = 1; // PrimaryAFPoint raw = 1 (center)
    data[8] = 0x01; // AFPointsUsed bitmap byte 0, bit 0 -> bit-number 1
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
    assert_eq!(tags["Nikon:AFPointsUsed"], "C6");
    assert_eq!(tags["Nikon:PrimaryAFPoint"], "C6 (Center)");
}

#[test]
fn v0100_11point_uses_bitmask_not_lookup_table() {
    // NikonD90.jpg: AFPointsUsed=Top, PrimaryAFPoint=Top.
    // FocusPointSchema=2 (11-point). AFPointsUsed is little-endian int16u
    // BITMASK: bit 1 = "Top" (Nikon.pm:1446). PrimaryAFPoint raw=2 -> "Top"
    // (Nikon.pm:4204: 2 => 'Top', one-based).
    let mut data = vec![0u8; 16];
    data[..4].copy_from_slice(b"0100");
    data[6] = 2; // FocusPointSchema = 11-point
    data[7] = 2; // PrimaryAFPoint raw = 2 -> Top
    data[8] = 0x02; // little-endian u16 = 2 -> bit 1 set -> "Top"
    data[9] = 0x00;
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
    assert_eq!(tags["Nikon:AFPointsUsed"], "Top");
    assert_eq!(tags["Nikon:PrimaryAFPoint"], "Top");
}

#[test]
fn v0100_11point_all_points_literal() {
    let mut data = vec![0u8; 16];
    data[..4].copy_from_slice(b"0100");
    data[6] = 2;
    data[7] = 0;
    data[8] = 0xff; // 0x7ff little-endian
    data[9] = 0x07;
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
    assert_eq!(tags["Nikon:AFPointsUsed"], "All 11 Points");
}

#[test]
fn v0100_schema_zero_reports_none_for_both_tags() {
    let mut data = vec![0u8; 16];
    data[..4].copy_from_slice(b"0100");
    data[6] = 0; // FocusPointSchema = Off
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
    assert_eq!(tags["Nikon:AFPointsUsed"], "(none)");
    assert_eq!(tags["Nikon:PrimaryAFPoint"], "(none)");
}

#[test]
fn v0101_153point_center_at_offset_0x44() {
    // NikonD850.jpg: AFPointsUsed=E9, PrimaryAFPoint=E9 (Center).
    let mut data = vec![0u8; 105];
    data[..4].copy_from_slice(b"0101");
    data[6] = 7; // FocusPointSchema = 153-point
    data[8] = 0x01; // AFPointsUsed bit-number 1 -> "E9"
    data[0x44] = 1; // PrimaryAFPoint raw = 1 -> center
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
    assert_eq!(tags["Nikon:AFPointsUsed"], "E9");
    assert_eq!(tags["Nikon:PrimaryAFPoint"], "E9 (Center)");
}
```

Note the test calls now pass a fourth argument (`None` for `model`) — `parse_af_info2`'s signature changes in this task; see Step 3. Update the three *existing* tests in this file (`area_mode_table_follows_the_detection_method`, `coordinates_are_gated_on_the_availability_flag`, `an_unclaimed_version_reports_only_itself`) to add the same `None,` argument, or they won't compile.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p oxidex nikon::af_info2 2>&1 | tail -30`
Expected: FAIL to compile (`parse_af_info2` takes 3 args, tests pass 4) or, once the signature is stubbed, FAIL on missing `Nikon:AFPointsUsed`/`Nikon:PrimaryAFPoint` keys.

- [ ] **Step 3: Implement**

Add near the top of `af_info2.rs`, alongside the other imports:

```rust
use super::af_points::{
    self, AF_POINTS_11, AF_POINTS_39, AF_POINTS_51, AF_POINTS_153,
};
```

Add a helper (used by every version arm in this and later tasks):

```rust
/// `PrimaryAFPoint`'s direct-lookup shape: raw byte 0 -> "(none)"; raw byte
/// 1 (always the documented center bit-number in every one of these
/// tables) -> "{center name} (Center)"; otherwise a plain table lookup.
/// Ports the repeated `PrintConv => { 0 => '(none)', %afPointsNN, 1 =>
/// 'XX (Center)' }` pattern (e.g. Nikon.pm:4181-4193).
fn primary_af_point(raw: u8, table: &[(u8, &str)]) -> String {
    if raw == 0 {
        return "(none)".to_string();
    }
    match table.iter().find(|(n, _)| *n == raw) {
        Some((1, name)) => format!("{name} (Center)"),
        Some((_, name)) => (*name).to_string(),
        None => format!("Unknown ({raw})"),
    }
}

/// `AFPointsUsed`'s 11-point shape is a raw little-endian `int16u` BITMASK,
/// not the byte-array bitmap `PrintAFPoints` walks (Nikon.pm:4258-4270).
fn af_points_used_bitmask11(raw: u16) -> String {
    if raw == 0 {
        return "(none)".to_string();
    }
    if raw == 0x7ff {
        return "All 11 Points".to_string();
    }
    let mut points = Vec::new();
    for (bit, name) in AF_POINTS_11.iter().enumerate() {
        if raw & (1 << bit) != 0 {
            points.push(*name);
        }
    }
    points.join(",")
}

/// `PrimaryAFPoint`'s 11-point shape: direct 1-based lookup into
/// `AF_POINTS_11`, no "(Center)" suffix (Nikon.pm:4517-4528).
fn primary_af_point_11(raw: u8) -> String {
    if raw == 0 {
        return "(none)".to_string();
    }
    match AF_POINTS_11.get((raw - 1) as usize) {
        Some(name) => (*name).to_string(),
        None => format!("Unknown ({raw})"),
    }
}
```

Change `pub fn parse_af_info2` to take `model: Option<&str>` (unused by V0100/V0101, consumed starting Task 7 — kept in the signature from the start so every earlier version arm's call site doesn't need touching twice):

```rust
pub fn parse_af_info2(
    data: &[u8],
    order: ByteOrder,
    model: Option<&str>,
    tags: &mut HashMap<String, String>,
) {
```
Add `let _ = model;` right after the version-string read if `model` isn't used yet by the time this compiles (removed again once Task 7 consumes it — do not leave an unused-parameter warning in the meantime).

In the `"0100" | "0101"` match arm, after the existing `FocusPointSchema` block, add:

```rust
// PrimaryAFPoint's Condition checks only FocusPointSchema (Nikon.pm:4181-
// 4230, 4505-4551), never AFDetectionMethod -- `detection` (already bound
// above for AFAreaMode) is unrelated here.
let primary_at = if version == "0100" { 7 } else { 0x44 };
if let Some(raw) = byte(primary_at) {
    let primary = match (version.as_str(), byte(6).unwrap_or(0)) {
        (_, 1) => primary_af_point(raw, AF_POINTS_51),
        (_, 2) => primary_af_point_11(raw),
        ("0100", 3) => primary_af_point(raw, AF_POINTS_39),
        ("0101", 7) => primary_af_point(raw, AF_POINTS_153),
        _ => "(none)".to_string(),
    };
    tags.insert("Nikon:PrimaryAFPoint".to_string(), primary);
}

let schema = byte(6).unwrap_or(0);
let used = match (version.as_str(), schema) {
    (_, 1) => data
        .get(8..15)
        .map(|bits| af_points::print_af_points_lookup(bits, AF_POINTS_51)),
    (_, 2) => read_u16(data, 8, order).map(af_points_used_bitmask11),
    ("0100", 3) => data
        .get(8..13)
        .map(|bits| af_points::print_af_points_lookup(bits, AF_POINTS_39)),
    ("0101", 7) => data
        .get(8..28)
        .map(|bits| af_points::print_af_points_lookup(bits, AF_POINTS_153)),
    (_, 0) => Some("(none)".to_string()),
    _ => None,
};
if let Some(used) = used {
    tags.insert("Nikon:AFPointsUsed".to_string(), used);
}
```

Note this replaces the unused `detection` variable name collision risk: the existing code already binds `let detection = byte(4).unwrap_or(0);` earlier in this arm — reuse it, don't rebind.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p oxidex nikon::af_info2 2>&1 | tail -40`
Expected: PASS, all new and existing tests green.

- [ ] **Step 5: Update the call site in `nikon.rs`**

`src/parsers/tiff/makernotes/nikon.rs:1449` currently calls
`af_info2::parse_af_info2(&bytes, order, tags);`. Change to
`af_info2::parse_af_info2(&bytes, order, model, tags);` — `model` is
already in scope at that point (same variable `parse_file_info` uses two
match arms later, `NIKON_FILE_INFO` at line ~1455).

Run: `cargo build 2>&1 | tail -20`
Expected: compiles clean.

- [ ] **Step 6: Commit**

```bash
git add src/parsers/tiff/makernotes/nikon/af_info2.rs src/parsers/tiff/makernotes/nikon.rs
git -c commit.gpgsign=false commit --no-gpg-sign --author="swackhamer <swackhamer@users.noreply.github.com>" -m "$(cat <<'EOF'
feat(nikon): decode AFPointsUsed/PrimaryAFPoint for AFInfo2 V0100/V0101

Wires the 51/11/39-point (V0100) and 51/11/153-point (V0101) grids at
their documented offsets (Nikon.pm:4137-4483), gated on the already-tracked
FocusPointSchema data member. The 11-point AFPointsUsed shape is a raw
BITMASK PrintConv, not the byte-array bitmap walk the other schemas use --
kept as its own small function rather than forced through print_af_points_lookup.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Wire V0200 / V0201 dispatch (Nikon 1 series)

**Files:**
- Modify: `src/parsers/tiff/makernotes/nikon/af_info2.rs`

**Interfaces:**
- Consumes: `af_points::{print_af_points_lookup, print_af_points_grid, AF_POINTS_135}`.
- Produces: `Nikon:AFPointsUsed`/`Nikon:PrimaryAFPoint` for `AFInfo2Version` `"0200"`/`"0201"`.

**Background (Nikon.pm:4600-4729):** `PhaseDetectAF` (offset 6, already read into `Nikon:PhaseDetectAF` but not currently stored as a data member) selects the shape: `4` → 135-point lookup (`undef[17]` at offset 8, `AF_POINTS_135`, `PrimaryAFPoint` at offset 7 direct-lookup with center override at bit-number 1 = "E8"), `5` → grid-computed 15-column (`undef[21]` at offset 8, `print_af_points_grid(bits, 15)`; `PrimaryAFPoint` also grid-computed via `GetAFPointGrid(raw, 15)` with `82 → "F8 (Center)"` literal override), `6` → grid-computed 21-column (`undef[29]` at offset 8; `PrimaryAFPoint` grid-computed via `GetAFPointGrid(raw, 21)` with `115 → "F11 (Center)"` override).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn v0200_135point_phase4_uses_lookup_table() {
    // Nikon1J1.jpg: AFPointsUsed=B11, PhaseDetectAF=4.
    // afPoints135 bit-number 13 = 'B9'... use a value traceable to the
    // table directly: bit-number 1 = 'E8' (Nikon.pm:1534).
    let mut data = vec![0u8; 30];
    data[..4].copy_from_slice(b"0200");
    data[6] = 4; // PhaseDetectAF = On (73-point)
    data[7] = 1; // PrimaryAFPoint raw = 1 -> center
    data[8] = 0x01; // AFPointsUsed bit-number 1 -> "E8"
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
    assert_eq!(tags["Nikon:AFPointsUsed"], "E8");
    assert_eq!(tags["Nikon:PrimaryAFPoint"], "E8 (Center)");
}

#[test]
fn v0200_135point_phase5_uses_computed_grid() {
    // PhaseDetectAF=5: grid-computed, ncols=15. Center is bit 82 -> "F8".
    let mut data = vec![0u8; 35];
    data[..4].copy_from_slice(b"0200");
    data[6] = 5;
    data[7] = 82; // PrimaryAFPoint raw = 82 -> literal "F8 (Center)" override
    data[8 + 10] = 1 << 2; // AFPointsUsed bit 82 (byte 10, offset 2) -> "F8"
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
    assert_eq!(tags["Nikon:AFPointsUsed"], "F8");
    assert_eq!(tags["Nikon:PrimaryAFPoint"], "F8 (Center)");
}

#[test]
fn v0200_171point_phase6_uses_computed_grid_21_cols() {
    // Nikon1J4.jpg: AFPointsUsed=F11, PhaseDetectAF=6, ncols=21, center
    // bit=115 -> "F11" (115/21=5->'F', 115-21*5+1=11).
    let mut data = vec![0u8; 40];
    data[..4].copy_from_slice(b"0200");
    data[6] = 6;
    data[7] = 115; // literal "F11 (Center)" override
    data[8 + 14] = 1 << 3; // bit 115 = byte 14 (115/8=14), offset 3 (115%8=3)
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
    assert_eq!(tags["Nikon:AFPointsUsed"], "F11");
    assert_eq!(tags["Nikon:PrimaryAFPoint"], "F11 (Center)");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p oxidex nikon::af_info2::tests::v0200 2>&1 | tail -30`
Expected: FAIL — keys absent (V0200 arm doesn't read `PhaseDetectAF` as a stored value yet, only inserts the printed tag).

- [ ] **Step 3: Implement**

In the `"0200" | "0201"` arm, capture `PhaseDetectAF`'s raw byte before it's consumed by `lookup(PHASE_DETECT_AF, raw)` (the existing code already reads `byte(6)` for this — store it):

```rust
// Nikon 1 series.
"0200" | "0201" => {
    if let Some(raw) = byte(5) {
        tags.insert(
            "Nikon:AFAreaMode".to_string(),
            lookup(AF_AREA_MODE_V0200, raw),
        );
    }
    let phase_detect = byte(6);
    if let Some(raw) = phase_detect {
        tags.insert(
            "Nikon:PhaseDetectAF".to_string(),
            lookup(PHASE_DETECT_AF, raw),
        );
    }
    match phase_detect {
        Some(4) => {
            if let Some(raw) = byte(7) {
                tags.insert(
                    "Nikon:PrimaryAFPoint".to_string(),
                    primary_af_point(raw, AF_POINTS_135),
                );
            }
            if let Some(bits) = data.get(8..25) {
                tags.insert(
                    "Nikon:AFPointsUsed".to_string(),
                    af_points::print_af_points_lookup(bits, AF_POINTS_135),
                );
            }
        }
        Some(5) => {
            if let Some(raw) = byte(7) {
                tags.insert(
                    "Nikon:PrimaryAFPoint".to_string(),
                    primary_af_point_grid(raw, 15, 82, "F8"),
                );
            }
            if let Some(bits) = data.get(8..29) {
                tags.insert(
                    "Nikon:AFPointsUsed".to_string(),
                    af_points::print_af_points_grid(bits, 15),
                );
            }
        }
        Some(6) => {
            if let Some(raw) = byte(7) {
                tags.insert(
                    "Nikon:PrimaryAFPoint".to_string(),
                    primary_af_point_grid(raw, 21, 115, "F11"),
                );
            }
            if let Some(bits) = data.get(8..37) {
                tags.insert(
                    "Nikon:AFPointsUsed".to_string(),
                    af_points::print_af_points_grid(bits, 21),
                );
            }
        }
        _ => {}
    }
}
```

Add the grid-computed `PrimaryAFPoint` helper next to `primary_af_point`:

```rust
/// `PrimaryAFPoint`'s grid-computed shape (Nikon.pm ~4656, 4673): raw 0 ->
/// "(none)"; the documented center bit -> "{center_name} (Center)"; else
/// the name is computed the same way `print_af_points_grid` computes it
/// per-bit (ExifTool's `GetAFPointGrid`, non-inverse direction).
fn primary_af_point_grid(raw: u8, ncols: u16, center_bit: u32, center_name: &str) -> String {
    if raw == 0 {
        return "(none)".to_string();
    }
    let bit = raw as u32;
    if bit == center_bit {
        return format!("{center_name} (Center)");
    }
    let row = bit / (ncols as u32);
    let col = bit - (ncols as u32) * row + 1;
    match char::from_u32(65 + row) {
        Some(letter) => format!("{letter}{col}"),
        None => format!("Unknown ({raw})"),
    }
}
```

Add `AF_POINTS_135` to the `use super::af_points::{...}` import list from Task 4.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p oxidex nikon::af_info2 2>&1 | tail -50`
Expected: PASS, all tests green (Task 4's tests still pass — unaffected).

- [ ] **Step 5: Commit**

```bash
git add src/parsers/tiff/makernotes/nikon/af_info2.rs
git -c commit.gpgsign=false commit --no-gpg-sign --author="swackhamer <swackhamer@users.noreply.github.com>" -m "$(cat <<'EOF'
feat(nikon): decode AFPointsUsed/PrimaryAFPoint for AFInfo2 V0200/V0201

Nikon 1 series: PhaseDetectAF (offset 6) selects between the 135-point
lookup table (value 4) and two grid-computed layouts (values 5/6, 15 and
21 columns respectively) per Nikon.pm:4636-4728. PhaseDetectAF is now
captured as a local before its PrintConv consumes it, the same pattern
FocusPointSchema already uses elsewhere in this file.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Wire V0300 / V0301 dispatch

**Files:**
- Modify: `src/parsers/tiff/makernotes/nikon/af_info2.rs`

**Interfaces:**
- Consumes: `af_points::{print_af_points_lookup, AF_POINTS_51, AF_POINTS_81, AF_POINTS_105}`.
- Produces: `Nikon:AFPointsUsed`/`Nikon:PrimaryAFPoint` for `AFInfo2Version` `"0300"`/`"0301"`.

**Background (Nikon.pm:4731-4920):** `AFPointsUsed` at offset `0x0a` (10), `PrimaryAFPoint` at offset `0x38` (56). Both gated on `FocusPointSchema` (`1`→51-point/`AF_POINTS_51`/`undef[7]`, `8`→81-point/`AF_POINTS_81`/`undef[11]`, `9`→105-point/`AF_POINTS_105`/`undef[14]`) **and** `AFCoordinatesAvailable == 0` — when coordinates are populated instead (`==1`), ExifTool has no `Condition` arm at all for these offsets, so neither tag exists. The existing code already reads and stores `AFCoordinatesAvailable` (`coords` local) for the geometry fields further down; reuse it.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn v0300_81point_gated_on_coordinates_unavailable() {
    // NikonZ50.jpg: AFPointsUsed includes C5,C6,D5,E5,E6, PrimaryAFPoint=D5.
    // FocusPointSchema=8 (81-point). afPoints81 bit-number for 'E5' is 1
    // (Nikon.pm:1616) -- use that as the minimal traceable case.
    let mut data = vec![0u8; 60];
    data[..4].copy_from_slice(b"0300");
    data[6] = 8; // FocusPointSchema = 81-point
    data[7] = 0; // AFCoordinatesAvailable = No
    data[0x38] = 1; // PrimaryAFPoint raw = 1 -> center
    data[0x0a] = 0x01; // AFPointsUsed bit-number 1 -> "E5"
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
    assert_eq!(tags["Nikon:AFPointsUsed"], "E5");
    assert_eq!(tags["Nikon:PrimaryAFPoint"], "E5 (Center)");
}

#[test]
fn v0300_absent_when_coordinates_available() {
    let mut data = vec![0u8; 60];
    data[..4].copy_from_slice(b"0300");
    data[6] = 8;
    data[7] = 1; // AFCoordinatesAvailable = Yes -> neither tag exists
    data[0x38] = 1;
    data[0x0a] = 0x01;
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
    assert!(!tags.contains_key("Nikon:AFPointsUsed"));
    assert!(!tags.contains_key("Nikon:PrimaryAFPoint"));
}

#[test]
fn v0300_105point_d6() {
    // NikonD6.jpg-shaped: FocusPointSchema=9 (105-point), center bit-number
    // 1 -> "D8" (Nikon.pm:1500).
    let mut data = vec![0u8; 60];
    data[..4].copy_from_slice(b"0300");
    data[6] = 9;
    data[7] = 0;
    data[0x38] = 1;
    data[0x0a] = 0x01;
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, None, &mut tags);
    assert_eq!(tags["Nikon:AFPointsUsed"], "D8");
    assert_eq!(tags["Nikon:PrimaryAFPoint"], "D8 (Center)");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p oxidex nikon::af_info2::tests::v0300 2>&1 | tail -30`
Expected: FAIL — keys absent.

- [ ] **Step 3: Implement**

In the `"0300" | "0301"` arm, after the existing `coords` local is bound (`let coords = byte(7).unwrap_or(0);`) and before the geometry `put_u16` calls, insert:

```rust
if coords == 0 {
    let schema = byte(6).unwrap_or(0);
    let table = match schema {
        1 => Some(AF_POINTS_51),
        8 => Some(AF_POINTS_81),
        9 => Some(AF_POINTS_105),
        _ => None,
    };
    let bitmap_len = match schema {
        1 => 7,
        8 => 11,
        9 => 14,
        _ => 0,
    };
    if let Some(table) = table {
        if let Some(raw) = byte(0x38) {
            tags.insert(
                "Nikon:PrimaryAFPoint".to_string(),
                primary_af_point(raw, table),
            );
        }
        if let Some(bits) = data.get(0x0a..0x0a + bitmap_len) {
            tags.insert(
                "Nikon:AFPointsUsed".to_string(),
                af_points::print_af_points_lookup(bits, table),
            );
        }
    }
}
```

Add `AF_POINTS_81`, `AF_POINTS_105` to the shared import list.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p oxidex nikon::af_info2 2>&1 | tail -50`
Expected: PASS, all tests green.

- [ ] **Step 5: Commit**

```bash
git add src/parsers/tiff/makernotes/nikon/af_info2.rs
git -c commit.gpgsign=false commit --no-gpg-sign --author="swackhamer <swackhamer@users.noreply.github.com>" -m "$(cat <<'EOF'
feat(nikon): decode AFPointsUsed/PrimaryAFPoint for AFInfo2 V0300/V0301

D6/D780/Z5/Z6/Z7/Z30/Z50/Zfc: wires the 51/81/105-point grids at offsets
0x0a/0x38 (Nikon.pm:4731-4920), gated on FocusPointSchema and, per
ExifTool's own Condition, on AFCoordinatesAvailable==0 -- when coordinates
are populated instead, neither tag exists at all, matching ExifTool's
missing Condition arm rather than emitting a guessed value.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Wire V0400 / V0401 / V0402 dispatch (model-gated)

**Files:**
- Modify: `src/parsers/tiff/makernotes/nikon/af_info2.rs`

**Interfaces:**
- Consumes: `af_points::{print_af_points_array, AF_POINTS_231, AF_POINTS_299, AF_POINTS_405}`, the `model: Option<&str>` parameter threaded through since Task 4.
- Produces: `Nikon:AFPointsUsed` for `AFInfo2Version` `"0400"`/`"0401"`/`"0402"`. (ExifTool defines no `PrimaryAFPoint` for this family — confirmed absent from Nikon.pm's V0400 table; only `AFPointsUsed` applies here.)

**Background (Nikon.pm:4922-4989):** offset 10 = `AFPointsUsed`, gated on `AFAreaModeUsed` (the raw byte already read at offset 5) being `197` (Auto) or `207` (3D-tracking) **and** the camera model: `Z 8`/`Z 9` → `undef[51]`/`AF_POINTS_405`, `Z6_3`/`Z f`/`Z5_2` → `undef[38]`/`AF_POINTS_299`, `Z50_2` → `undef[29]`/`AF_POINTS_231`. These three use `print_af_points_array` (positional, 0-based), not `print_af_points_lookup` — ExifTool authored them as plain `qw()` arrays.

Model matching: ExifTool's `Condition` uses `$$self{Model} =~ /^NIKON (Z 8|Z 9)\b/i` etc. — the model string as reported by the camera, prefixed `"NIKON "`. oxidex's `model: Option<&str>` should be matched the same way; use a case-insensitive prefix/contains check rather than a full regex port (no regex crate dependency currently used in this module — confirm with `grep -n "^use" src/parsers/tiff/makernotes/nikon/af_info2.rs` before adding one; a plain `str` check is sufficient and avoids adding a dependency for three fixed literal patterns).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn v0400_z8_z9_uses_405point_array() {
    let mut data = vec![0u8; 70];
    data[..4].copy_from_slice(b"0400");
    data[5] = 197; // AFAreaModeUsed = Auto
    data[7] = 0; // AFCoordinatesAvailable = No
    data[10] = 0x01; // AFPointsUsed bit index 0 -> AF_POINTS_405[0]
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, Some("NIKON Z 9"), &mut tags);
    assert_eq!(
        tags["Nikon:AFPointsUsed"],
        af_points::AF_POINTS_405[0]
    );
}

#[test]
fn v0400_z8_z9_absent_for_other_area_modes() {
    let mut data = vec![0u8; 70];
    data[..4].copy_from_slice(b"0400");
    data[5] = 193; // AFAreaModeUsed = Single, not Auto/3D-tracking
    data[10] = 0x01;
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, Some("NIKON Z 9"), &mut tags);
    assert!(!tags.contains_key("Nikon:AFPointsUsed"));
}

#[test]
fn v0401_zf_uses_299point_array() {
    let mut data = vec![0u8; 60];
    data[..4].copy_from_slice(b"0401");
    data[5] = 207; // 3D-tracking
    data[10] = 0x01;
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, Some("NIKON Z f"), &mut tags);
    assert_eq!(
        tags["Nikon:AFPointsUsed"],
        af_points::AF_POINTS_299[0]
    );
}

#[test]
fn v0402_z50ii_uses_231point_array() {
    let mut data = vec![0u8; 50];
    data[..4].copy_from_slice(b"0402");
    data[5] = 197;
    data[10] = 0x01;
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, Some("NIKON Z50_2"), &mut tags);
    assert_eq!(
        tags["Nikon:AFPointsUsed"],
        af_points::AF_POINTS_231[0]
    );
}

#[test]
fn v0400_unrecognized_model_reports_nothing() {
    let mut data = vec![0u8; 60];
    data[..4].copy_from_slice(b"0400");
    data[5] = 197;
    data[10] = 0x01;
    let mut tags = HashMap::new();
    parse_af_info2(&data, ByteOrder::BigEndian, Some("NIKON D850"), &mut tags);
    assert!(!tags.contains_key("Nikon:AFPointsUsed"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p oxidex nikon::af_info2::tests::v040 2>&1 | tail -40`
Expected: FAIL — keys absent (or wrong content, since `data[10]` bit collides with the `AFAreaModeUsed`/`AFCoordinatesAvailable` fields the existing V0400 arm already reads — check for offset collisions before writing the byte-length; the existing arm reads `put_u16(62, ...)` etc., offset 10 is unused territory, safe).

- [ ] **Step 3: Implement**

In the `"0400" | "0401" | "0402"` arm, after the existing `AFCoordinatesAvailable` block and before the `put_u16(62, ...)` geometry calls, insert:

```rust
let area_mode_used = byte(5);
if matches!(area_mode_used, Some(197) | Some(207)) {
    let m = model.unwrap_or("");
    let table_and_len: Option<(&[&str], usize)> = if model_matches(m, &["NIKON Z 8", "NIKON Z 9"]) {
        Some((af_points::AF_POINTS_405, 51))
    } else if model_matches(m, &["NIKON Z6_3", "NIKON Z f", "NIKON Z5_2"]) {
        Some((af_points::AF_POINTS_299, 38))
    } else if model_matches(m, &["NIKON Z50_2"]) {
        Some((af_points::AF_POINTS_231, 29))
    } else {
        None
    };
    if let Some((table, len)) = table_and_len
        && let Some(bits) = data.get(10..10 + len)
    {
        tags.insert(
            "Nikon:AFPointsUsed".to_string(),
            af_points::print_af_points_array(bits, table),
        );
    }
}
```

Add the model-matching helper near the other free functions in this file:

```rust
/// Ports ExifTool's `$$self{Model} =~ /^NIKON (...)\b/i` model-prefix
/// checks (Nikon.pm:4966,4975,4983) as a plain case-insensitive prefix
/// match against each literal in `prefixes` -- no regex dependency needed
/// for three fixed alternatives.
fn model_matches(model: &str, prefixes: &[&str]) -> bool {
    let model = model.to_ascii_uppercase();
    prefixes
        .iter()
        .any(|p| model.starts_with(&p.to_ascii_uppercase()))
}
```

Remove the now-unnecessary `let _ = model;` placeholder added in Task 4.

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p oxidex nikon::af_info2 2>&1 | tail -60`
Expected: PASS, all tests green, no unused-parameter warnings.

- [ ] **Step 5: Commit**

```bash
git add src/parsers/tiff/makernotes/nikon/af_info2.rs
git -c commit.gpgsign=false commit --no-gpg-sign --author="swackhamer <swackhamer@users.noreply.github.com>" -m "$(cat <<'EOF'
feat(nikon): decode AFPointsUsed for AFInfo2 V0400/V0401/V0402

Z8/Z9 (405-point), Z6III/Zf/Z5II (299-point) and Z50II (231-point), gated
on AFAreaModeUsed (Auto/3D-tracking only, per Nikon.pm:4962-4989) and the
camera model string -- the only place this file needs Model, threaded in
as a plain Option<&str> parameter rather than a context struct, since it
doesn't vary across the block the way Sony's cross-tag data members do
(PR #319). No PrimaryAFPoint exists for this family in ExifTool's tables.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Corpus verification, full gates, and update the module header

**Files:**
- Modify: `src/parsers/tiff/makernotes/nikon/af_info2.rs` (module doc comment — remove the now-inaccurate "deliberately not decoded" claim)
- No other files.

**Interfaces:**
- Consumes: everything from Tasks 1-7.
- Produces: nothing new — this task verifies and documents.

- [ ] **Step 1: Update the module header**

Read the current header (`src/parsers/tiff/makernotes/nikon/af_info2.rs:1-8`) and replace the paragraph claiming `AFPointsUsed`/`PrimaryAFPoint`/`AFPointsInFocus`/`AFPointsSelected` are undecoded — only the last two remain true:

```rust
//! `Nikon::AFInfo2V0100` .. `AFInfo2V0400` (`Nikon::Main` 0x00b7).
//!
//! Five layouts selected by the block's own version. Not encrypted.
//!
//! `AFPointsUsed` and `PrimaryAFPoint` are decoded via the point-name grids
//! transcribed into `af_points.rs` from ExifTool's own `afPoints*` tables.
//! `AFPointsInFocus` and `AFPointsSelected` remain deliberately undecoded:
//! they share the same per-body grid dependency but weren't in scope for
//! this pass (see docs/plans/specs/2026-08-01-nikon-af-points-design.md).
```

- [ ] **Step 2: Build the release binary and run corpus verification**

Run:
```bash
cargo build --release 2>&1 | tail -20
```
Expected: clean build.

Run, from the repo root, the exiftool-parity harness against every Nikon sample carrying either tag (54 files per the task's original count):
```bash
cd /tmp/oxidex-exiftool-cache/combined-samples/Nikon
for f in *.jpg; do
  et=$(exiftool -G1 -s -Nikon:AFPointsUsed -Nikon:PrimaryAFPoint "$f" 2>/dev/null)
  ox=$(/Users/allen/git/oxidex/.claude/worktrees/infallible-kilby-15388e/target/release/oxidex -e -s "$f" 2>/dev/null | grep -i "AFPointsUsed\|PrimaryAFPoint")
  if [ -n "$et" ] || [ -n "$ox" ]; then
    echo "=== $f ==="
    echo "exiftool: $et"
    echo "oxidex:   $ox"
  fi
done
```
Expected: for every file where `exiftool` reports a value, `oxidex` reports the identical string under the identical group/tag name. Any mismatch means either an offset, a `Condition`, or a table entry is wrong — go back to the relevant task and fix it against the Nikon.pm line cited in that task's Background section, not by adjusting the test expectation (per `TRANSCRIPTION.md` rule 6: fix the expectation against ExifTool, never the other way round — and here ExifTool *is* the expectation).

- [ ] **Step 3: Run the full gate suite**

Run: `cargo fmt --all -- --check`
Expected: no diff. If there is one, run `cargo fmt --all` (not `-- --check`) and re-verify.

Run: `cargo clippy --workspace 2>&1 | tail -60`
Expected: no warnings/errors introduced by this work (pre-existing warnings elsewhere are out of scope).

Run: `cargo test --workspace 2>&1 | tail -40`
Expected: all tests pass, including every test added in Tasks 3-7.

- [ ] **Step 4: Commit the header update**

```bash
git add src/parsers/tiff/makernotes/nikon/af_info2.rs
git -c commit.gpgsign=false commit --no-gpg-sign --author="swackhamer <swackhamer@users.noreply.github.com>" -m "$(cat <<'EOF'
docs(nikon): update af_info2 module header now that AF points decode

AFPointsUsed/PrimaryAFPoint are covered end to end across all four
AFInfo2 version families; AFPointsInFocus/AFPointsSelected remain
undecoded (out of scope for this pass, tracked in the design doc).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 5: Sync with origin/main, resolve any conflicts, open the PR**

Run:
```bash
git fetch origin main
git log --oneline HEAD..origin/main | head -20
```
If origin/main has moved, rebase or merge (prefer `git merge origin/main` over rebase for a feature branch already pushed, to avoid rewriting shared history — check whether this branch has been pushed yet with `git status -sb`; if not yet pushed, either is fine, prefer rebase for a clean history):
```bash
git merge origin/main
```
If conflicts appear, resolve them file by file — re-run `cargo test --workspace` after resolving to confirm nothing broke, paying particular attention to `af_info2.rs`'s existing test module (line numbers may have shifted) and `nikon.rs:1449`'s call site.

Push and open the PR:
```bash
git push -u origin claude/infallible-kilby-15388e
gh pr create --title "fix(nikon): decode AFPointsUsed/PrimaryAFPoint via transcribed point-name grids" --body "$(cat <<'EOF'
## Summary
- Nikon `AFPointsUsed`/`PrimaryAFPoint` were deliberately unimplemented (af_info2.rs's module header) because the wrong per-body point-name grid would produce a confident, wrong answer.
- Transcribes all ten grids ExifTool needs (afPoints11/51/39/105/135/153/81/231/299/405) mechanically via a Perl slice-and-eval extractor + codegen step, following the %fileTypeExt precedent in docs/TRANSCRIPTION.md, rather than hand-retyping ~1,200 (index, name) pairs.
- Wires dispatch across every AFInfo2 version (0100-0402) on ExifTool's own Condition chains (FocusPointSchema / PhaseDetectAF / AFAreaModeUsed / AFCoordinatesAvailable / Model), same shape as PR #319 (Sony AFAreaModeSetting/AFPointSelected).
- Design doc: docs/plans/specs/2026-08-01-nikon-af-points-design.md. Plan: docs/superpowers/plans/2026-08-01-nikon-af-points.md.

## Test plan
- [x] cargo test --workspace
- [x] cargo clippy --workspace
- [x] cargo fmt --all -- --check
- [x] Per-file exiftool -G1 -s vs oxidex -e -s over the 54 Nikon corpus samples carrying either tag, byte-for-byte
EOF
)"
```

- [ ] **Step 6: Watch CI, then squash-merge on green**

Run:
```bash
gh pr checks --watch
```
Once all checks pass:
```bash
gh pr merge --squash --auto
```
If `--auto` isn't accepted (branch protection doesn't require it, or a check is still pending), poll with `gh pr checks` until green, then `gh pr merge --squash`.
