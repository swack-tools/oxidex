# Reference

Technical reference for OxiDex v2.0.0-beta.1 (the `refactor/tag-machinery`
line). Where a page carries numbers, it names the instrument and commit that
produced them. A page without both is a record, not a claim about the
current tip.

## Using OxiDex

- [Supported formats](/reference/formats/): what is parsed, what is parsed generically, what is only identified, and what can be written
- [Rust API](/reference/api-reference): the public read and write API, with signatures
- [C API](/reference/ffi-api): the 15 `exiftool_*` functions, the error codes and the headers
- [MakerNotes](/reference/makernotes): how vendor blocks are dispatched
- [Camera RAW](/reference/formats/camera-raw) and [executables](/reference/formats/pe-executable)
- [Packaging](/reference/packaging/): building .deb, .rpm and other packages

## Parity reports

These are generated. Never hand-edit them; the named tool refreshes each one.

- [ExifTool comparison](/reference/comparison/): per-format tables against the pinned ExifTool, generated when the site is deployed
- [ExifTool coverage](/reference/tag-coverage-analysis): `scripts/generate_tag_coverage.py`; tag definitions counted separately from measured extraction
- [JPEG tag support](/reference/jpeg-tag-support) and [JPEG tag matrix](/reference/jpeg-tag-matrix): the `jpeg-tag-matrix` binary
- [Corpus read observations](/reference/catalog-corpus-observed) and [verified read and write observations](/reference/catalog-hydrated-observed): `catalog_observed_snapshot.py --report`, the receipts behind the proven-read and proven-write counts
- [Source catalog baseline](/reference/catalog-baseline): `catalog_snapshot.py --report`
- [Catalog to source ledger](/reference/catalog-hydrated-join): `join_catalog_hydrated.py`, which the parity ratchet reads
- [Hydrated reader source](/reference/hydrated-reader-layout-baseline) (hand-written companion): what the hydrated-layout capture makes available to source selectors

## Direction and status

- [Status](/status/): the current measured state in one place
- [Autogeneration plan](/AUTOGENERATION-PLAN): the goal, the measured state and the ordered next steps
- [Autogeneration v2 design](/AUTOGENERATION-V2-DESIGN): the mechanism, which generates conversions over a session with per-field mixed mode
- [Autogeneration progress](/AUTOGENERATION-PROGRESS): the working scoreboard
- [Upgrade rehearsal 11.78 / 12.64](/reference/upgrade-rehearsal-11.78-12.64): the first end-to-end regeneration against older ExifTool releases
- [Tag machinery status](/TAG_MACHINERY_STATUS) and [Transcription](/TRANSCRIPTION): dated status and the method

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

