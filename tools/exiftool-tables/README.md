# ExifTool table generation and verification

For completed work, known gaps and priorities, start with
[Tag machinery status](../../docs/TAG_MACHINERY_STATUS.md). This file describes
the tooling present at the September 10, 2026 review (`c7f5dd81`, pin 13.59).

Follow-up `b36983c2` adds conservative IFD-aware upgrade classification on a
pending branch. See the [implementation and limits](../../docs/TAG_MACHINERY_STATUS.md#pending-upgrade-classifier-repair);
the complete upgrade transaction is still unfinished.

## Commands and scope

```sh
just regen-tables          # primary generation, including binary and IFD tables
just verify-tables         # independent checks of committed table declarations
just regen-tables-all      # primary generation plus downstream vendor/charset tables
just regen-tables-tier2    # downstream generation only
```

Run generation in an owned worktree with the intended pin and a capability-probed
Perl/ExifTool pair. Generation modifies tracked artifacts; ordinary measurement
scripts refuse a dirty tree unless the explicit override is provided and recorded.
The current wrapper/provenance gap is listed in the status page: these commands
are existing entry points, not a claim that the whole upgrade transaction is sound.

`regen.sh` generates binary and IFD tables, file identification, Composite
definitions and compiled Composite expressions, FITS names, and expression/value-
conversion ledgers. `regen-all.sh` adds vendor subdirectory tables, Nikon AF-point
grids, bespoke transcriptions, recovered Sony/Minolta generators and Macintosh
CJK charset tables. The scripts and their companion outputs are the source of
truth for the artifact inventory.

`just bump-exiftool <version>` also exists. Its artifact backup/restore lists and
triage classifier lag the current generators, and its temporary old-version
comparison rebuilds only the first generation tier. **Repair and rehearse this
workflow before treating it as an unattended upgrade or relying on complete
`--dry-run` restoration.** See the
[current gaps](../../docs/TAG_MACHINERY_STATUS.md#concrete-upgrade-gaps-at-this-snapshot)
and the historical [13.58 to 13.59 exercise](../../docs/reference/bump-reports/13.58-to-13.59.md).

Four generated-origin files still have no committed generator: Sony
`plain_tables.rs`/`enciphered_tables.rs` and Nikon
`settings_tables.rs`/`encrypted_tables.rs`. The two previously orphaned
Sony main-extra/Minolta outputs now have generators. `regen-all.sh` names the
remaining limits; generated once does not mean automatically refreshable.

## What is generated

ExifTool combines a generic engine with declarative tables and procedural
behavior. `dump_tables.pl` loads Perl modules and walks their in-memory tables,
including tables assembled dynamically; it does not try to parse the Perl source
with regular expressions.

`src/tag_sync` separately consumes the `-listx` documentation view. That catalog
provides names and descriptions, but not byte layouts, subdirectory dispatch,
conditions or conversion semantics. Catalog size is not extraction coverage.

```text
ExifTool loaded tables -> dump_tables.pl -> tables.json
  -> verify_exprs.py -> expression PASS ledger
  -> codegen.py -> binary_tables.rs + ifd_tables.rs + conversion accounting
  -> codegen_composite.py -> Composite definitions and expression computations
  -> downstream generators -> vendor-specific artifacts

ExifTool loaded tables -> oracle.pl -> independent declaration facts
  -> verify.py compares the generated Rust against those facts
```

`codegen.py` handles both `ProcessBinaryData` and `ProcessExif`-style IFD tables.
It does not implement arbitrary custom processing procedures. Unsupported
semantics require explicit refusal or further implementation; an ungenerated
table is not evidence that the upstream table does not exist.

`codegen_subdirs.py` targets the separate vendor subdirectory interpreter in
`src/parsers/tiff/makernotes/shared/binary_subdir.rs`. It takes an explicit table
list and raises on unsupported constructs; any intentional `--allow-skip` must
remain visible in the resulting evidence.

## Translation and verification

Conversions use reviewed exact translations or the closed expression compiler
in `exprs.py`. Oracle-gated ValueConv/PrintConv expressions must appear in the
matching PASS ledger. Unsupported semantics are refused and counted, not
approximated. Named helper implementations in `src/exiftool_tables/exprs.rs`
remain maintained Rust.

Keep these checks separate:

- `verify.py` compares generated binary/IFD declaration facts with the independent
  Perl oracle. It is not the expression execution checker.
- `verify_exprs.py` differentially evaluates translated scalar/list expressions
  on its defined probes. Generated Composite computations use a different domain
  excluded from this oracle; they need their own validation.
- `verify_cond.py` and `verify_subdirs.py` check conditions and Start/Base semantics
  within their declared scopes.
- Codegen reports identify omissions inside the scopes they enumerate. They do
  not yet constitute an exhaustive declaration-to-runtime producer inventory.
- `reachability.py` reports emitted, enabled, eligible and refused binary/IFD
  tables from static artifacts. Its lookup scan is not observed runtime execution.
- Engine/carrier tests and `conformance.py` compare actual behavior. A finite
  probe suite does not prove equivalence for every possible input.

## Where to spend effort

Use the [remaining-work backlog](../../docs/AUTOMATION-AND-TESTER-PLAN.md).
Check whether a missing output is caused by absent declarations, an unsupported
shared rule, disabled routing or a hand-specific procedure before writing a new
parser. Prefer a shared rule when it unblocks several relevant tables, then
validate the resulting runtime migration.

Run `expr_coverage.py`/`analyze.py` against the current pinned dump to rank refused
forms. Historical percentages in [Transcription](../../docs/TRANSCRIPTION.md)
describe their recorded snapshots, not today's extraction or upgrade automation.
