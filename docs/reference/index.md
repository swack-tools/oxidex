# Reference Documentation

Technical reference for the library, the CLI's FFI, the tag database and the
measured state of ExifTool parity. Where a page carries numbers, it names the
instrument and commit they came from; a page without both is a record, not a
claim about the current tip.

## Where the work stands

- [Autogeneration plan](/AUTOGENERATION-PLAN) - the goal, the measured state and the ordered next steps
- [Autogeneration v2 design](/AUTOGENERATION-V2-DESIGN) - the mechanism: generated conversions over a session, a real grammar, a helper library, per-field mixed mode
- [Autogeneration progress](/AUTOGENERATION-PROGRESS) - the working scoreboard, newest checkpoint first
- [Tag machinery status](/TAG_MACHINERY_STATUS) - dated integration status and evidence limits
- [Transcription](/TRANSCRIPTION) - the method, with its historical experiments

## Contents

### [Architecture](/reference/architecture)
OxiDex's internal design: the hexagonal layering, parser dispatch and core abstractions.

### [API Reference](/reference/api-reference)
The Rust library API with examples. The longer [Rust API document](/reference/api/) covers the same surface in more detail.

### [FFI API](/reference/ffi-api)
The C-compatible interface for other languages (Python, Node.js, Go).

### [Tag Database](/reference/tag-database)
How tag definitions are synced from ExifTool and organised into the `oxidex-tags-*` crates.

### [MakerNotes](/reference/makernotes)
Manufacturer-specific metadata support.

### [ExifTool Coverage](/reference/tag-coverage-analysis)
The generated, CI-refreshed conformance report: definitions counted separately from measured extraction.

### [ExifTool Compatibility](/reference/comparison/)
Per-format comparison against the pinned ExifTool, regenerated at deploy time, with the generated [JPEG Tag Support](/reference/jpeg-tag-support) and [JPEG Tag Matrix](/reference/jpeg-tag-matrix) reports.

### Source catalog and observations
Generated reports (never hand-edited; the named tool refreshes each):
- [Source Catalog Baseline](/reference/catalog-baseline) - `catalog_snapshot.py --report`
- [Catalog to Source Ledger](/reference/catalog-hydrated-join) - `join_catalog_hydrated.py`; the parity ratchet reads it
- [Verified Read and Write Observations](/reference/catalog-hydrated-observed) and [Corpus Read Observations](/reference/catalog-corpus-observed) - `catalog_observed_snapshot.py --report`

Hand-written companion:
- [Hydrated Reader Source](/reference/hydrated-reader-layout-baseline) - what the hydrated-layout capture makes available to source selectors

### [Supported Formats](/reference/formats/)
Format families with implementation notes.

### [Packaging](/reference/packaging/)
Building and distributing OxiDex.

## Records and checkpoints

Dated records from the autogeneration work. Each is accurate for the commit it
names and is kept as evidence; none is a statement about the current tip.

**Source capture and inventories**
- [Catalog-to-hydrated join](/CATALOG-HYDRATED-JOIN), [Hydrated catalog universe](/HYDRATED-CATALOG-UNIVERSE), [Hydrated layout projection](/HYDRATED-LAYOUT-PROJECTION)
- [Source artifact and enablement baseline (2026-09-14)](/reference/source-artifact-baseline-20260914)
- [Recorded source-to-artifact join, 13.59](/reference/source-artifact-join-13.59)
- [Recorded source-processor inventory, 13.59](/reference/source-processor-inventory-13.59)
- [Staged native serial-layout inventory](/reference/serial-layout-inventory)
- [Native write-definition catalog](/reference/write-coverage-catalog)

**Parity checkpoints**
- [Generated metadata parity checkpoint (2026-09-14)](/reference/goal-checkpoint-20260914) and its [review record](/reference/parity-rollup-review-20260914)
- [Resume metadata parity work](/reference/metadata-parity-resume)
- [Source-family baseline and generic readers](/reference/source-family-migration-plan)
- [Generated reading, writing and ExifTool upgrades](/reference/read-write-version-plan) and [the writer's input pipeline](/reference/writer-input-pipeline)
- [Native creation of a JPEG EXIF block](/reference/native-jpeg-exif-defaults)
- [Keyed reporting-policy validation](/reference/keyed-reporting-policy-validation)
- [Generated ItemList reader progress](/reference/quicktime-generated-reader)
- [Garmin FIT source review](/reference/garmin-fit-source-review)

**Reader migrations and recoveries**
- [Canon AFInfo2 production plan](/reference/afinfo2-production-plan) and [serial AFInfo plan](/reference/serial-afinfo-plan)
- [Serial processor checkpoint](/reference/serial-processor-checkpoint), [serial runtime checkpoint](/reference/serial-runtime-checkpoint), [word-directory checkpoint](/reference/word-directory-checkpoint), [directory validation checkpoint](/reference/directory-validation-checkpoint)
- [Real AudioV4 manual-sequence retirement](/reference/real-audio-v4-retirement)
- [Nikon settings generator recovery](/reference/nikon-settings-generator-recovery)
- [Sony plain-table producer recovery](/reference/sony-plain-generator-recovery) and [Sony raw-ID repair](/reference/sony-raw-id-runtime)
- [Shared EXIF conversion reconciliation (2026-09-11)](/reference/rawconv-reconciliation-2026-09-11)
- [Historical capture path notation](/reference/path-normalization)

**Engine studies and bump exercises (August 2026)**
- [BinaryData engine and its gates (Step 28)](/reference/binary-data-engine) and [corpus synthesis](/reference/corpus-synthesis)
- [Step 33 format backlog](/reference/step-33-format-backlog)
- Bump exercises: [13.55 to 13.59](/reference/bump-reports/13.55-to-13.59), [13.58 to 13.59](/reference/bump-reports/13.58-to-13.59)

## Quick Links

- **Getting Started**: the [Guide section](/guide/) for installation and usage
- **Performance**: the [performance page](/performance/), including the status of the published benchmarks
- **Contributing**: the [Contributing guide](/contributing/)
