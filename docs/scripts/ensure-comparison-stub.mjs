#!/usr/bin/env node
/**
 * Guarantee that docs/reference/comparison/index.md exists before VitePress runs.
 *
 * The ExifTool comparison report is generated at deploy time by
 * `just compare-exiftool-full-update` and the whole directory is gitignored, so a
 * fresh clone has nothing there. Several committed pages link to
 * /reference/comparison/ (docs/index.md, docs/reference/index.md,
 * docs/reference/jpeg-tag-support.md, docs/guides/MANUAL-WORKFLOW-TRIGGER.md), and
 * VitePress treats unresolved internal links as build errors. Without this stub,
 * `npm run docs:build` cannot succeed on a clean checkout.
 *
 * This script is deliberately a no-op when the real report is present, so the
 * deployed site is byte-for-byte what it was before: deploy-docs.yml runs the
 * generator first, index.md already exists, and we leave it alone.
 */

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const comparisonDir = path.resolve(__dirname, '../reference/comparison')
const indexPath = path.join(comparisonDir, 'index.md')

if (fs.existsSync(indexPath)) {
  console.log('[ensure-comparison-stub] real report present, leaving it untouched')
  process.exit(0)
}

const stub = `---
title: ExifTool Compatibility Report
---

# ExifTool Compatibility Report

::: warning This report has not been generated in this checkout
The per-format ExifTool comparison tables are **generated artifacts**, not
checked-in files. \`docs/reference/comparison/\` is gitignored, so a fresh clone
has nothing to show here and this placeholder stands in so the docs site still
builds and every link resolves.

The published site at [oxidex.net](https://oxidex.net) always shows the real
report: the deploy workflow regenerates it before building.
:::

## Generating the report locally

The generator downloads a sample corpus, builds the \`tag-comparison\` binary in
release mode and runs it against a real ExifTool install, so it needs a Rust
toolchain and Perl and it is not quick:

\`\`\`bash
just compare-exiftool-full-update
\`\`\`

Re-run the docs build afterwards and this page is replaced by the generated
overview, with one sidebar entry per format.

## What the real report contains

- Overall tag coverage against ExifTool, per format
- Counts of missing, extra and differing tags for each format
- A regression column comparing against the committed baseline

Until it is generated, see [JPEG Tag Support](/reference/jpeg-tag-support) and
[ExifTool Coverage](/reference/tag-coverage-analysis), which are checked in and
always available.
`

fs.mkdirSync(comparisonDir, { recursive: true })
fs.writeFileSync(indexPath, stub)
console.log(
  '[ensure-comparison-stub] wrote placeholder docs/reference/comparison/index.md\n' +
    '[ensure-comparison-stub] run `just compare-exiftool-full-update` for the real report'
)
