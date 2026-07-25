# Documentation Migration to Main Branch

**Date:** 2025-12-03
**Goal:** Consolidate all documentation into `docs/` on main branch, eliminate gh-pages as source branch

## Problem

Current setup has documentation split across two locations:
- `gh-pages` branch: Public VitePress site (oxidex.net)
- `main/docs/`: Internal documentation (plans, architecture, metrics)

This causes:
- Confusion about where to edit docs
- Two separate workflows for publishing
- Benchmark reports published separately from main docs

## Solution

### 1. New Documentation Structure

All content lives in `docs/` on main branch:

```
docs/
├── index.md                      # Landing page
├── guide/                        # User guides
│   ├── getting-started.md
│   ├── cli-usage.md
│   ├── library-api.md
│   ├── mcp-integration.md
│   └── troubleshooting.md
├── reference/                    # API reference
│   ├── architecture.md
│   ├── formats/
│   ├── ffi-api.md
│   ├── api-reference.md
│   ├── tag-database.md
│   ├── api/                      # Merged from docs/api/
│   └── packaging/                # Merged from docs/packaging/
├── performance/                  # Benchmarks
│   ├── index.md
│   ├── benchmarks.md
│   ├── profiling.md
│   └── optimization-strategy.md
├── contributing/
│   ├── testing/                  # Merged from docs/testing/
│   └── development/              # Merged from docs/development/
├── architecture/                 # Public - from docs/architecture/
├── diagrams/                     # Public - from docs/diagrams/
├── tag-domains/                  # Public - from docs/tag-domains/
├── changelog.md
├── .vitepress/                   # VitePress config
├── package.json
├── .plans/                       # Hidden - internal design docs
└── .internal/                    # Hidden - metrics, analysis, misc
    ├── analysis/
    ├── metrics/
    └── refactoring/
```

Hidden folders (prefixed with `.`) are excluded from VitePress by default.

### 2. CI Workflow - Direct GitHub Pages Deployment

No gh-pages branch needed. Single workflow builds and deploys everything:

```yaml
name: Deploy Documentation

on:
  push:
    branches: [main]
    paths:
      - 'docs/**'
      - 'benches/**'
      - '.github/workflows/deploy-docs.yml'
  workflow_dispatch:

jobs:
  build-and-deploy:
    runs-on: ubuntu-latest
    permissions:
      contents: read
      pages: write
      id-token: write
    environment:
      name: github-pages
      url: ${{ steps.deployment.outputs.page_url }}

    steps:
      - uses: actions/checkout@v4

      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: '20'
          cache: 'npm'
          cache-dependency-path: docs/package-lock.json

      - name: Install docs dependencies
        run: npm ci
        working-directory: docs

      - name: Build VitePress
        run: npm run docs:build
        working-directory: docs

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Run benchmarks
        run: cargo bench --bench integration_benchmarks --bench parse_benchmarks

      - name: Copy benchmark reports into site
        run: cp -r target/criterion docs/.vitepress/dist/benchmarks

      - name: Setup Pages
        uses: actions/configure-pages@v4

      - name: Upload Pages artifact
        uses: actions/upload-pages-artifact@v3
        with:
          path: docs/.vitepress/dist

      - name: Deploy to GitHub Pages
        id: deployment
        uses: actions/deploy-pages@v4
```

### 3. Content Migration Mapping

**From gh-pages branch:**
| Source | Destination |
|--------|-------------|
| `index.md` | `docs/index.md` |
| `guide/*` | `docs/guide/*` |
| `reference/*` | `docs/reference/*` |
| `performance/*` | `docs/performance/*` |
| `contributing/*` | `docs/contributing/*` |
| `changelog.md` | `docs/changelog.md` |
| `.vitepress/*` | `docs/.vitepress/*` |
| `package.json`, `package-lock.json` | `docs/` |

**From existing `docs/`:**
| Source | Destination | Visibility |
|--------|-------------|------------|
| `plans/` | `docs/.plans/` | Hidden |
| `architecture/` | `docs/architecture/` | Public |
| `analysis/` | `docs/.internal/analysis/` | Hidden |
| `metrics/` | `docs/.internal/metrics/` | Hidden |
| `diagrams/` | `docs/diagrams/` | Public |
| `tag-domains/` | `docs/tag-domains/` | Public |
| `api/` | `docs/reference/api/` | Public |
| `benchmarks/` | `docs/performance/` | Public |
| `testing/` | `docs/contributing/testing/` | Public |
| `development/` | `docs/contributing/development/` | Public |
| `packaging/` | `docs/reference/packaging/` | Public |
| `formats/` | `docs/reference/formats/` | Public |
| `refactoring/` | `docs/.internal/refactoring/` | Hidden |
| Standalone `.md` files | `docs/.internal/` | Hidden |
| `book/` | **Deleted** | (obsolete mdbook) |

## Implementation Steps

### Phase 1: Prepare main branch

1. [ ] Copy VitePress config from gh-pages to `docs/`
   - `.vitepress/config.ts`
   - `package.json`, `package-lock.json`
   - `tsconfig.json`
   - Other config files

2. [ ] Copy public content from gh-pages
   - `index.md`
   - `guide/`
   - `reference/`
   - `performance/`
   - `contributing/`
   - `changelog.md`
   - `images/`, `public/`

3. [ ] Reorganize existing `docs/` content
   - Rename `plans/` → `.plans/`
   - Create `.internal/` directory
   - Move `analysis/` → `.internal/analysis/`
   - Move `metrics/` → `.internal/metrics/`
   - Move `refactoring/` → `.internal/refactoring/`
   - Move standalone `.md` files → `.internal/`
   - Merge `api/` → `reference/api/`
   - Merge `benchmarks/` content → `performance/`
   - Move `testing/` → `contributing/testing/`
   - Move `development/` → `contributing/development/`
   - Move `packaging/` → `reference/packaging/`
   - Merge `formats/` → `reference/formats/`

4. [ ] Delete obsolete content
   - Remove `book/` folder

5. [ ] Update VitePress config
   - Update sidebar navigation for new structure
   - Ensure hidden folders are excluded
   - Update any internal links

6. [ ] Test build locally
   ```bash
   cd docs && npm install && npm run docs:build
   ```

### Phase 2: Update CI

1. [ ] Replace `deploy-docs.yml` with new unified workflow
2. [ ] Remove `publish-benchmarks` job from `ci.yml`
3. [ ] Update GitHub repo settings
   - Settings → Pages → Source → "GitHub Actions"

### Phase 3: Cleanup

1. [ ] Commit and push to main
2. [ ] Verify site deploys correctly at oxidex.net
3. [ ] Delete gh-pages branch
4. [ ] Remove local gh-pages worktree

## Rollback Plan

If deployment fails:
1. Re-create gh-pages branch from git reflog
2. Revert workflow changes
3. Reset Pages source to "Deploy from branch"

## Benefits

1. **Single source of truth** - All docs in one place on main
2. **Simpler workflow** - One branch, one deployment
3. **Atomic deployments** - Docs and benchmarks deploy together
4. **No branch maintenance** - No gh-pages branch to sync
5. **Better discoverability** - Internal docs visible in main repo
