# Generated reading, writing and ExifTool upgrades

Updated September 13, 2026. This records the expanded read/write and version
objective alongside the [main plan](../AUTOGENERATION-PLAN.md).

## Finish line

OxiDex should derive all tag-specific reading and native-writable tag behavior
from the selected ExifTool Perl source. Shared Rust mechanisms still perform
file access, arithmetic, encoding and safe file changes. A tag-specific rule
retyped into Python or Rust is unfinished automation.

The version used to build generated definitions is the version used to check
those definitions and their behavior. ExifTool 13.59 is the current working pin,
not a limit on the requested version scope. Keep an explicit upstream release
catalog and per-version results. Missing source, unsupported semantics,
unexercised behaviors and failed versions remain visible. Native read-only
fields are explicitly ineligible for writes, not failed writer implementations.

## Work in order, with independent tasks in parallel

1. Finish Canon AFInfo2/AFInfo3 reader retirement through PR #760. Preserve the
   failed legacy synthetic test, correct its invalid size field with native
   evidence, run full tests and merge only after the required checks pass.
2. Capture one complete source model for both directions. Retain permissions,
   actual writer/checker functions, placement, forward and inverse conversions,
   validation, insertion/deletion and ordering rules. Keep unknown property
   values and distinguish absent, undefined, zero and empty. Preserve existing
   read admission while expanding write facts.
3. Prove the first complete generated writer using HostComputer `0x013c` and the
   generic Exif::Main scalar rule class. Generate its actual physical IFD0
   placement and write type; use the existing JPEG/TIFF surgical mechanisms.
   Verify insert, update, growth, shrinkage and deletion against native ExifTool
   in both byte orders, preserving unrelated metadata and image/file payload.
   A copied native name/type/placement change must propagate without another
   hand-written rule. Retire the replaced manual lookup after proof.
4. Expand shared capabilities and migrate eligible read/write families through
   them. Publish manual rules removed and remaining unsupported behavior for
   each direction; table/line counts alone are not completion.
5. Add a non-promoting version-rehearsal runner around existing generation and
   isolated build mechanisms. The existing promotion bump compares both builds
   against the new oracle, so it is not proof of each version's native behavior.
   Regenerate/build BOTH selected versions and compare each with ITS native
   reader/writer, using the same declared fixtures and corpus where applicable.
6. Exercise upgrades repeatedly, then close the remaining historical-version
   and semantic inventory. Random tests are a discovery tool; they cannot
   certify untested versions or behaviors.

## Reproducible random version rehearsals

Snapshot the actual official release tags with immutable commit/archive
identities. Pick two distinct releases from that catalog with a persisted seed,
record selection before work starts, then test older-to-newer regeneration.
Do not silently restrict selection to versions already known to pass. Missing
native prerequisites or unsupported old syntax must be recorded as failures or
explicit unsupported scope, not skipped out of the success denominator.

Run after every three relevant merged generator/reader/writer batches or one
week since the last rehearsal, whichever comes first, once the runner exists
and passes its own tests.
Keep a persisted counter and last run identity so interruption cannot reset the
cadence or accidentally launch duplicate runs. The existing hourly continuation now records this cadence; it must finish
and validate the runner before launching expensive rehearsals. Do not create
a duplicate schedule or run a rehearsal on every heartbeat.
All heavy work uses its host queue/lock and survives disconnects.

For each version record native source/interpreter/capability identity, generated
artifact hashes, clean code revision, binary hash, fixture hashes, all commands
and exits, and raw native/OxiDex outputs. Compare create/update/delete plus
unmodified-data preservation separately from reads. Record every manual edit
needed to make the upgrade work. Successful automation requires zero
tag-specific edits; unsupported semantics require shared implementation work.

Use prior failures as fixed regression cases alongside new random selections.
Report the complete catalog population, tested versions/pairs, failures,
untested releases and unexercised read/write behaviors. Keep corpus attribution,
read conformance, write conformance and source-rule automation as separate
measurements. No exact completion date follows from the current partial data.

## Native readiness checkpoint, September 13

The rehearsal CLI can verify a selected materialized release, its explicit
Perl interpreter and required native capability, then read and set/delete
metadata on private fixture copies. Reports bind commands and results to the
verified archive/source tree and publish without replacing an existing report,
including a concurrent publisher. File-move/link pseudo-tags require a separate
containment contract; they are not metadata cases in this readiness instrument.

The portable command-line tests create their own archived source fixtures and
controlled executable stand-in. They exercise read/set/delete success, wrong
version, changed plan/source, missing fixtures, filesystem-action refusal and
report publication races without private paths or conditional skips. These
tests establish the instrument's behavior, not ExifTool compatibility.

The persisted random pair, 11.78 and 12.64, has also passed actual native CLI
readiness for JPEG FileType and Comment set/delete, including UTF-8 input.
This does not establish OxiDex/native conformance. Generating and building
OxiDex for both versions, comparing both readers and writers with their own
native release, and accounting for unsupported behavior remain the next
rehearsal steps. A ready native oracle must never mark those steps passed.
