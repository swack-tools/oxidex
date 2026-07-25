# VitePress Documentation Site Design

**Date:** 2025-11-18
**Status:** Approved
**Implementation:** Full (Option A)

## Overview

This document outlines the design for migrating OxiDex documentation to a VitePress-based GitHub Pages site. The goal is to create a modern, searchable documentation site that serves all audiences (new users, developers, contributors) while preserving existing CI/CD benchmark deployment.

## Background

**Current State:**
- Documentation scattered across 79 markdown files
- Existing mdBook setup at `docs/book/`
- GitHub Pages at `oxidex.net` shows 404 (no root index)
- Benchmark reports deployed to `/benchmarks/` via CI/CD
- 31 outdated files cluttering the repository

**Problems:**
1. No public-facing landing page at oxidex.net
2. Documentation is difficult to discover and navigate
3. Historical planning documents create clutter
4. No unified documentation experience

## Goals

1. **User Experience:** Create intuitive, searchable documentation site
2. **Performance Showcase:** Prominently feature benchmark results
3. **Preserve CI/CD:** Maintain automated benchmark deployment
4. **Clean Repository:** Remove outdated files, organize documentation
5. **Maintainability:** Easy to update and extend

## Design Decisions

### Framework: VitePress

**Chosen:** VitePress (over Docusaurus, mdBook, static HTML)

**Rationale:**
- Faster build times (important for CI)
- Lighter bundle size
- Built-in search, dark mode, mobile responsive
- Simpler configuration than Docusaurus
- Better developer experience than mdBook
- Modern, actively maintained

### Structure: Progressive Disclosure (Approach C)

**Layout:**
- **Home:** Compelling hero → Features → Quick start → Performance → CTA
- **Guide:** Getting started, installation, usage (for users)
- **Reference:** Architecture, API docs, formats (for developers)
- **Performance:** Benchmarks, profiling, optimization (showcase)
- **Contributing:** Development guide, testing, troubleshooting

**Rationale:** Guides users from "why OxiDex" to "how to use" naturally, balancing all audiences.

### CI/CD: Separate Workflow

**Approach:** Create new `deploy-docs.yml` workflow separate from benchmark deployment.

**Rationale:**
- Clean separation of concerns
- Easier to debug issues
- Preserves existing benchmark workflow
- Reduces risk of breaking benchmarks

## Architecture

### Directory Structure

```
gh-pages/
├── benchmarks/            # ⚠️ CI-managed, do not touch
│   └── report/            # Criterion reports
├── .vitepress/
│   ├── config.mts         # Site configuration
│   └── theme/
│       ├── index.ts       # Custom theme
│       └── custom.css     # OxiDex styling
├── index.md               # Home page (hero layout)
├── guide/
│   ├── index.md           # Guide overview
│   ├── getting-started.md # Installation + quick start
│   ├── cli-usage.md       # CLI reference
│   ├── library-api.md     # Rust API guide
│   ├── troubleshooting.md # Common issues
│   └── migration.md       # Perl ExifTool → OxiDex
├── reference/
│   ├── architecture.md    # System design
│   ├── api-reference.md   # Detailed API docs
│   ├── ffi-api.md         # C FFI integration
│   ├── tag-database.md    # Tag system
│   └── formats/
│       ├── index.md       # Formats overview
│       ├── camera-raw.md  # RAW format details
│       └── pe-format.md   # Windows PE format
├── performance/
│   ├── index.md           # Performance overview
│   ├── benchmarks.md      # Links to /benchmarks/
│   ├── profiling.md       # Profiling guide
│   └── optimization-strategy.md
├── contributing/
│   ├── index.md           # Contributing guide
│   ├── development.md     # Dev setup
│   └── troubleshooting.md # Dev issues
├── changelog.md           # Version history
├── package.json           # Node.js dependencies
└── public/                # Static assets
    ├── logo.svg
    └── images/
```

### VitePress Configuration

**Key Settings:**
- Base path: `/oxidex/` (GitHub Pages project site)
- Clean URLs: enabled (no `.html` extensions)
- Local search: enabled
- Dark mode: enabled
- Brand colors: Rust orange (#dd7732)
- Markdown line numbers: enabled
- Last updated timestamps: enabled

**Navigation:**
```
Home | Guide | Reference | Performance | Contributing
```

**Sidebar:** Auto-generated from directory structure per section

## Content Migration Plan

### Phase 1: Delete Obsolete Files (Priority 0)

**Action:** Remove 10 obsolete files
```bash
rm RELEASE_ANNOUNCEMENT.md RELEASE_CHECKLIST.md
rm tests/data_lfs_error_summary.md tests/data_lfs_final_report.md
rm tests/fixtures/I5T9_*.md tests/fixtures/COMPLETION_REPORT.md
```

**Rationale:** These are outdated post-v1.0 release, LFS issues resolved, historical test reports.

### Phase 2: Archive Historical Plans (Priority 0)

**Action:** Archive 27 planning documents
```bash
mkdir -p docs/plans/archived/2025-11
mv docs/plans/2025-11-[0-1][0-7]-*.md docs/plans/archived/2025-11/
```

**Exception:** Keep `2025-11-18-parsing-performance-optimization-design.md` (current)

**Rationale:** These document completed features, keep for history but remove from active workspace.

### Phase 3: Migrate Core Docs (Priority 1)

**Files to migrate (7 files):**

| Source | Destination | Notes |
|--------|-------------|-------|
| README.md | index.md | Hero layout, features, quick start |
| CHANGELOG.md | changelog.md | Version history |
| docs/book/src/intro.md | guide/index.md | Project intro |
| docs/book/src/installation.md | guide/getting-started.md | Install + quick start |
| docs/book/src/cli_usage.md | guide/cli-usage.md | CLI reference |
| docs/book/src/library_api.md | guide/library-api.md | Rust API guide |
| docs/book/src/troubleshooting.md | guide/troubleshooting.md | User support |

**Estimated effort:** 8-12 hours

### Phase 4: Migrate Reference Docs (Priority 2)

**Files to migrate (11 files):**

| Source | Destination | Action |
|--------|-------------|--------|
| docs/TAG_DATABASE.md | reference/tag-database.md | Direct copy |
| docs/api/library_api.md | reference/api-reference.md | Merge with guide version |
| docs/api/ffi_api.md + docs/ffi_usage.md | reference/ffi-api.md | Consolidate FFI docs |
| docs/book/src/ffi.md | reference/ffi-api.md | Merge into consolidated FFI |
| docs/book/src/formats.md | reference/formats/index.md | Overview |
| docs/formats/camera-raw.md | reference/formats/camera-raw.md | Details |
| docs/pe-format-support.md | reference/formats/pe-format.md | Details |
| docs/architecture/multi-crate-tags.md | reference/architecture.md | Part of architecture synthesis |

**Estimated effort:** 12-16 hours

### Phase 5: Migrate Performance Docs (Priority 2)

**Files to migrate (6 files):**

| Source | Destination | Notes |
|--------|-------------|-------|
| benches/benchmark_results.md | performance/benchmarks.md | Link to /benchmarks/ |
| docs/profiling.md | performance/profiling.md | Recent samply guide |
| docs/benchmarks/*.md | performance/*.md | Historical baselines |
| docs/plans/2025-11-18-parsing-performance-optimization-design.md | performance/optimization-strategy.md | Current strategy |

**Estimated effort:** 6-8 hours

### Phase 6: Create Synthesis Docs (Priority 3)

**New documents to create:**

1. **reference/architecture.md** (synthesize from):
   - README.md architecture section
   - docs/IMPLEMENTATION_ROADMAP.md
   - docs/architecture/multi-crate-tags.md

2. **contributing/index.md** (synthesize from):
   - docs/testing/integration_test_plan.md
   - docs/testing/comparison/README.md
   - General contributing guidelines

3. **guide/migration.md** (create new):
   - Perl ExifTool → OxiDex migration
   - CLI compatibility matrix
   - Known differences (from tests/integration/KNOWN_DISCREPANCIES.md)

**Estimated effort:** 8-12 hours

## CI/CD Integration

### New Workflow: `.github/workflows/deploy-docs.yml`

**Trigger:**
- Push to main branch with changes to `docs/**`
- Manual workflow dispatch

**Steps:**
1. Checkout main branch
2. Setup Node.js 20
3. Install VitePress dependencies (`npm ci`)
4. Build VitePress (`npm run docs:build`)
5. Checkout gh-pages branch
6. Remove old docs (preserve `/benchmarks/`)
7. Copy new docs from build output
8. Commit and push to gh-pages

**Key Safety:** Preserve `/benchmarks/` directory during deployment

**Estimated effort:** 4-6 hours (includes testing)

## Implementation Phases

### Phase 1: Setup (Week 1, Days 1-2)

**Tasks:**
1. Create git worktree for gh-pages branch
2. Initialize VitePress (package.json, config)
3. Create directory structure
4. Configure VitePress (nav, sidebar, theme)
5. Add static assets (logo, favicon)
6. Test local dev server

**Deliverable:** VitePress site running locally

**Estimated effort:** 8-12 hours

### Phase 2: Content Migration (Week 1-2, Days 3-7)

**Tasks:**
1. Delete obsolete files (Priority 0)
2. Archive historical plans (Priority 0)
3. Migrate core docs (Priority 1)
4. Migrate reference docs (Priority 2)
5. Migrate performance docs (Priority 2)
6. Create synthesis docs (Priority 3)

**Deliverable:** All content migrated and formatted

**Estimated effort:** 20-28 hours

### Phase 3: CI/CD Integration (Week 2, Day 8)

**Tasks:**
1. Create `deploy-docs.yml` workflow
2. Test deployment to gh-pages
3. Verify benchmarks preserved
4. Update main branch documentation references

**Deliverable:** Automated docs deployment working

**Estimated effort:** 4-6 hours

### Phase 4: Polish & Launch (Week 2, Days 9-10)

**Tasks:**
1. Cross-browser testing
2. Mobile responsive testing
3. Link validation
4. Search functionality verification
5. Performance optimization
6. Final review and launch

**Deliverable:** Production-ready documentation site

**Estimated effort:** 4-8 hours

**Total estimated effort:** 36-54 hours (1.5-2.5 weeks)

## Success Criteria

1. ✅ Site live at https://oxidex.net with proper home page
2. ✅ All user-facing docs migrated and accessible
3. ✅ Benchmarks preserved and accessible via /benchmarks/
4. ✅ Search functionality works
5. ✅ Navigation is intuitive (Guide → Reference → Performance)
6. ✅ CI/CD deploys docs automatically on push to main
7. ✅ Mobile responsive design works
8. ✅ Page load time < 1 second
9. ✅ All links functional (no 404s)
10. ✅ Repository cleaned (31 obsolete files removed)

## Risks & Mitigation

### Risk 1: Benchmark Overwrite
**Risk:** CI accidentally deletes VitePress files
**Mitigation:** Test workflow in fork first, add verification checks

### Risk 2: Build Failures
**Risk:** VitePress build fails in CI
**Mitigation:** Lock dependency versions, test locally, pin Node.js version

### Risk 3: Broken Links
**Risk:** Links to benchmarks break after migration
**Mitigation:** Use absolute paths (`/oxidex/benchmarks/`), test all links

### Risk 4: Content Migration Errors
**Risk:** Lose content or introduce errors during migration
**Mitigation:** Use git worktree, review all changes before merge

## Files to Keep in Repository

**27 developer/internal files to keep (not migrate to VitePress):**

- `docs/IMPLEMENTATION_ROADMAP.md` (active planning)
- `docs/DEBUG_MODE_OOM_FIXES.md` (debugging notes)
- `docs/testing/*.md` (test infrastructure)
- `docs/testing/comparison/*.md` (test methodology)
- `tests/integration/README.md` (test guide)
- `tests/integration/KNOWN_DISCREPANCIES.md` (test reference)
- `docs/packaging/packaging-guide.md`, `docs/analysis/fixture-removal-analysis.md`, `docs/plans/archived/2025-11/repo-cleanup-plan.md`
- `docs/analysis/exiftool-module-audit.md` (historical analysis)
- `docs/plans/archived/` (historical plans)

**Action:** Link to these from `contributing/index.md` for contributor access

## Alternatives Considered

1. **Keep mdBook** - Rejected: less modern, limited theming
2. **Use Docusaurus** - Rejected: heavier, more complex than needed
3. **Static HTML** - Rejected: no search, hard to maintain
4. **GitHub Wiki** - Rejected: no version control, limited customization

## Dependencies

**Node.js:**
- Node.js ≥18.0.0
- npm ≥8.0.0

**Packages:**
- vitepress: ^1.5.0
- vue: ^3.5.13

## Next Steps

1. **Approval:** Confirm design approach ✅
2. **Setup git worktree:** Isolate gh-pages work
3. **Write implementation plan:** Detailed task breakdown
4. **Execute phases 1-4:** Follow implementation timeline
5. **Launch:** Deploy to production

## Appendix: Content Migration Matrix

See full audit report for detailed file-by-file action matrix (79 files categorized).

**Summary:**
- **Migrate:** 21 files
- **Keep:** 27 files
- **Delete:** 10 files
- **Archive:** 21 files

---

**Design approved for full implementation (Option A).**
