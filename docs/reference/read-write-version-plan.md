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

## Newer native behavior takes precedence

An upgrade adopts the newer native release's parsing corrections, new tags,
type and formatting changes, and writable behavior. Do not preserve an old
result merely because it once matched an older oracle. Test three relationships:

- Old generated OxiDex against the old native ExifTool.
- New generated OxiDex against the new native ExifTool.
- Native old versus native new, to identify intentional upstream changes.

Cross-version equality is not a passing requirement. The new build must match
the new oracle. If the generator cannot represent a changed rule, record that
unsupported behavior explicitly and extend the shared machinery. Do not hide it
by retaining the old tag-specific implementation. A sampled pair passing does
not certify other releases or behaviors.

## Work in order, with independent tasks in parallel

1. **Done:** Canon AFInfo2/AFInfo3 reader retirement merged in PR #760 at
   `4a3eb26c`, with all five required hosted checks passing on `2630ded8`.
   Preserve the failed legacy fixture, native correction and acceptance record.
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
   Keep defined-empty values distinct from deletion. Both EXIF family names
   and physical IFD names must address the same generated identity; the
   pre-migration EXIF-qualified deletion silently succeeds without deleting,
   while TIFF deletion is explicitly unsupported. Preserve these as baseline
   failures until the complete generated operation is implemented.
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


## Implementation checkpoint after the reader merge

The native write-fact sidecar and offline seeded planner are integrated on
`codex/read-write-upgrade-integration-20260913`. Independent review accepted the
complete loader-token grammar after rejecting two earlier bypasses. The full
153-module read projection is unchanged. Planner repair checks exact catalog
selection, matching-version oracle bindings, selected journal membership and
untested scope; 17 focused tests and five independent altered-plan checks pass.
These are source and planning foundations: writer activation, official live
catalog/source capture, both-version regeneration/builds and real read/write
comparisons are still required. Combined official regeneration is the next
integration check. No successful release upgrade is claimed by a saved plan.
