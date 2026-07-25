# 2025-11-19 Tag Processing Documentation & Shared Library Plan

## Goal
Create a shared `oxidex-tags-common` library that centralizes tag schema validation, lookup helpers, and metadata conventions, then document and test each `oxidex-tags-*` crate around that shared core. This reduces code complexity (less duplicated logic, fewer bespoke helpers) while capturing institutional knowledge in docs/tests so future feature work stays aligned.

## Current Pain
- Each domain crate (`oxidex-tags-core`, `oxidex-tags-camera`, etc.) defines similar helpers for loading YAML tag tables, resolving descriptors, and normalizing strings, but the helpers are scattered with only sparse tests.
- Documentation for individual crates is limited to top-level README blurbs; maintainers must read code to infer invariants such as "Canon MakerNotes always normalize case" or "media tags use fractional rational encoding".
- Unit tests mostly check compile-time wiring and not real tag invariants, so regressions sneak in when contributors tweak YAML or helper functions.

## Guiding Principles
1. **Single Source of Validation Truth** – the shared crate exposes validation traits/macro helpers, so each domain crate becomes declarative (data + small wiring) instead of procedural.
2. **Executable Documentation** – docs describe the shared library’s contracts; golden tests ensure every domain crate exercises those contracts.
3. **Incremental Adoption** – migrate high-traffic crates (core, camera) first, then replicate patterns across the rest to keep risk manageable.

## Proposed Architecture
- New crate: `oxidex-tags-shared` (name bikesheddable) containing:
  - YAML schema structs + serde helpers for tag tables.
  - Validation pipeline (`TagValidator`) configurable via domain-specific rules (case-folding, unit conversions, numeric ranges).
  - Shared lookup utilities (`find_table`, `find_tag`) with tracing hooks for debugging.
  - Doc generator helpers (e.g., `DocSection` builder) that produce Markdown fragments capturing domain details straight from data definitions.
- Domain crates import the shared crate and configure it via `const DomainProfile` objects: specify normalization behavior, mandatory fields, version tags, etc.
- Integration glue moves into `oxidex-tags` facade: it now depends on shared crate for cross-domain `get_tag_table` and central test fixtures.

## Documentation Workstream
1. **Shared Library Reference** – add `docs/architecture/oxidex-tags-shared.md` explaining data flow, key traits, extension points.
2. **Per-Domain Playbooks** – auto-generate Markdown under `docs/tag-domains/<domain>.md` using doc generator helpers. Each playbook contains:
   - Domain purpose and canonical tag tables.
   - Validation rules derived from `DomainProfile` (e.g., "GPS tables enforce DMS normalization").
   - Sample lookups produced via real runtime queries.
3. **Contributor Guide Update** – extend `docs/development` with a "Adding a new tag table" section referencing the shared crate APIs and tests to touch.

## Testing Workstream
- Introduce `tests/tag-fixtures/<domain>/` YAML + expected results.
- Shared crate exports a `validate_domain(domain_profile, fixtures_path)` helper.
- For each domain crate, add integration tests invoking the helper on curated fixtures. Tests assert:
  - All YAML tables deserialize against the schema.
  - Domain-specific rules are enforced (e.g., manufacturer IDs unique, rational formats correct).
  - Generated documentation snippets are stable (snapshot tests).
- Add a fast `cargo test -p oxidex-tags-shared --features domain-core` matrix plus a nightly `cargo test --workspace --doc` runner to catch doc drift.

## Migration Plan
1. **Foundation (Week 1)**
   - Create `oxidex-tags-shared` crate with schema structs + validator traits.
   - Port a minimal subset of helpers from `oxidex-tags-core` to prove parity.
   - Write doc draft + inline rustdoc for exported APIs.
2. **Core Domain Adoption (Week 2)**
   - Refactor `oxidex-tags-core` to use shared crate; remove local helpers.
   - Add fixture-driven tests verifying EXIF/IPTC tables load + doc snapshots.
   - Generate `docs/tag-domains/core.md` via new doc helpers.
3. **Camera & Media Domains (Weeks 3-4)**
   - Repeat migration: define `DomainProfile`s, move helpers, add fixtures.
   - Document domain-specific quirks discovered while migrating; capture them in tests.
4. **Remaining Domains + Facade (Weeks 5-6)**
   - Migrate document/image/specialty crates, ensuring `oxidex-tags` facade compiles with shared lookup utilities.
   - Remove any now-dead code paths and ensure public API remains stable.
5. **Hardening (Week 7)**
   - Expand tests to cover edge fixtures, add lints or CI steps (cargo fmt, clippy, doc). Ensure docs reference the shared crate patterns.

## Risks & Mitigations
- **Schema Drift** – risk that shared validator diverges from domain-specific quirks. Mitigate by allowing `DomainProfile` hooks to inject custom validators and requiring fixtures covering edge cases.
- **Documentation Generation Fragility** – auto-generated Markdown may break structure. Add snapshot tests and a manual review checklist in CI (fail if diffs not checked in).
- **Contributor Onboarding** – new shared crate might feel complex. Provide example-based docs + templates for new tag tables to lower barrier.

## Success Criteria
- `oxidex-tags-core` and `oxidex-tags-camera` no longer define ad-hoc loader helpers; they import everything from the shared crate.
- Every domain crate owns a playbook document and at least two fixture tests covering common + edge tables.
- CI includes shared crate unit tests, domain fixtures, and doc snapshot verification.
- Engineers can add a new tag table by touching YAML + fixtures + doc template only—no bespoke helper code required.
