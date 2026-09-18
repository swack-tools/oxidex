import { defineConfig } from 'vitepress'
import fs from 'node:fs'
import path from 'node:path'
import { execSync } from 'node:child_process'

// ---------------------------------------------------------------------------
// Release channels.
//
// The same sources build two sites:
//
//   stable   the live site, oxidex.net, built from `main` (the default -- a
//            plain `npm run docs:build` is the stable build).
//   preview  a development preview of `refactor/tag-machinery`, built by
//            .github/workflows/deploy-docs-preview.yml and served from a
//            separate host so it can never overwrite the live site.
//
// Everything is driven by environment variables so neither channel needs a
// config fork:
//
//   DOCS_CHANNEL      'stable' (default) | 'preview'
//   DOCS_BASE         site base path, default '/'. Must start and end with '/'.
//   DOCS_STABLE_URL   absolute root of the stable site  (version dropdown)
//   DOCS_PREVIEW_URL  absolute root of the preview site (version dropdown)
//   DOCS_PREVIEW_SHA  commit the preview was built from; defaults to
//                     `git rev-parse HEAD`. Preview only.
//
// The dropdown links are absolute on purpose: the two sites live on different
// bases (and hosts), so a base-relative link could never cross between them.
// ---------------------------------------------------------------------------
const CHANNEL = process.env.DOCS_CHANNEL || 'stable'
if (CHANNEL !== 'stable' && CHANNEL !== 'preview') {
  throw new Error(`DOCS_CHANNEL must be 'stable' or 'preview', got '${CHANNEL}'`)
}
const IS_PREVIEW = CHANNEL === 'preview'

const BASE = process.env.DOCS_BASE || '/'
if (!BASE.startsWith('/') || !BASE.endsWith('/')) {
  throw new Error(`DOCS_BASE must start and end with '/', got '${BASE}'`)
}

const STABLE_VERSION = 'v1.2.1'
const PREVIEW_BRANCH = 'refactor/tag-machinery'
const REPO_URL = 'https://github.com/swack-tools/oxidex'
const STABLE_URL = process.env.DOCS_STABLE_URL || 'https://oxidex.net/'
const PREVIEW_URL = process.env.DOCS_PREVIEW_URL || 'https://swack-tools.github.io/oxidex-next/'

function previewSha(): string {
  const fromEnv = process.env.DOCS_PREVIEW_SHA
  if (fromEnv) return fromEnv
  try {
    return execSync('git rev-parse HEAD', { cwd: __dirname, encoding: 'utf8' }).trim()
  } catch {
    throw new Error('DOCS_CHANNEL=preview needs DOCS_PREVIEW_SHA (git rev-parse HEAD failed)')
  }
}

// Read by docs/.vitepress/theme/index.ts through a Vite `define`, so the
// stable build carries no banner code path at all (it is dead-code eliminated).
const PREVIEW = IS_PREVIEW
  ? (() => {
      const sha = previewSha()
      if (!/^[0-9a-f]{7,40}$/.test(sha)) {
        throw new Error(`DOCS_PREVIEW_SHA is not a commit SHA: '${sha}'`)
      }
      return {
        branch: PREVIEW_BRANCH,
        sha,
        shortSha: sha.slice(0, 8),
        commitUrl: `${REPO_URL}/commit/${sha}`
      }
    })()
  : null

// Height of the preview banner. VitePress offsets its fixed nav, sidebar and
// content by --vp-layout-top-height, so the banner must declare it.
const BANNER_CSS = `
:root { --vp-layout-top-height: 36px; }
.oxidex-preview-banner {
  position: fixed; top: 0; left: 0; right: 0; z-index: 100;
  height: var(--vp-layout-top-height);
  display: flex; align-items: center; justify-content: center;
  padding: 0 16px; overflow: hidden; text-align: center;
  font-size: 13px; font-weight: 500; line-height: 1.3;
  color: #1f1300; background: #f5b841; border-bottom: 1px solid #c98a12;
}
/* Phones: two lines rather than an ellipsis that would hide the commit. */
@media (max-width: 639px) {
  :root { --vp-layout-top-height: 48px; }
  .oxidex-preview-banner { font-size: 12px; }
}
.oxidex-preview-banner a { color: inherit; text-decoration: underline; font-family: var(--vp-font-family-mono); }
.oxidex-preview-banner a:hover { text-decoration: none; }
`

const versionItems = {
  text: 'Versions',
  items: [
    { text: `${STABLE_VERSION} (stable)`, link: STABLE_URL, target: '_self', noIcon: true },
    { text: `branch: ${PREVIEW_BRANCH} (preview)`, link: PREVIEW_URL, target: '_self', noIcon: true }
  ]
}

// Dynamically generate sidebar items for comparison formats.
//
// docs/reference/comparison/ is gitignored and produced by
// `just compare-exiftool-full-update` (see scripts/ensure-comparison-stub.mjs),
// so on a fresh clone it may hold nothing but the generated stub. Returning an
// empty list keeps a cold `npm run docs:build` working instead of throwing
// ENOENT while the config is still loading.
function getComparisonFormats() {
  const comparisonDir = path.resolve(__dirname, '../reference/comparison')
  if (!fs.existsSync(comparisonDir)) {
    return []
  }
  const files = fs.readdirSync(comparisonDir)

  return files
    .filter(file => file.endsWith('.md') && file !== 'index.md')
    .map(file => {
      const name = file.replace('.md', '')
      const displayName = name.toUpperCase()
      return { text: displayName, link: `/reference/comparison/${name}` }
    })
    .sort((a, b) => a.text.localeCompare(b.text))
}

export default defineConfig({
  title: 'OxiDex',
  description: 'Modern, high-performance Rust implementation of ExifTool',
  lang: 'en-US',
  // Stable: custom domain (oxidex.net) serves from root. Preview: see DOCS_BASE.
  base: BASE,
  outDir: '.vitepress/dist',
  cleanUrls: true,
  lastUpdated: true,

  themeConfig: {
    logo: '/logo.svg',
    siteTitle: 'OxiDex',

    nav: [
      { text: 'Home', link: '/' },
      { text: 'Guide', link: '/guide/' },
      { text: 'Reference', link: '/reference/' },
      { text: 'Architecture', link: '/architecture/' },
      { text: 'Performance', link: '/performance/' },
      {
        text: IS_PREVIEW ? `branch: ${PREVIEW_BRANCH}` : STABLE_VERSION,
        items: [
          versionItems,
          {
            items: [
              { text: 'Changelog', link: '/changelog' },
              { text: 'Contributing', link: '/contributing/' },
              { text: 'Tag Machinery Status', link: '/TAG_MACHINERY_STATUS' },
              { text: 'Automation Backlog', link: '/AUTOMATION-AND-TESTER-PLAN' },
              { text: 'Transcription', link: '/TRANSCRIPTION' },
              { text: 'AI Harness', link: '/AI_HARNESS' }
            ]
          }
        ]
      }
    ],

    sidebar: {
      '/guide/': [
        {
          text: 'Getting Started',
          items: [
            { text: 'Introduction', link: '/guide/' },
            { text: 'Installation', link: '/guide/getting-started' },
            { text: 'CLI Usage', link: '/guide/cli-usage' },
            { text: 'Library API', link: '/guide/library-api' },
            { text: 'MCP Integration', link: '/guide/mcp-integration' },
            { text: 'Troubleshooting', link: '/guide/troubleshooting' }
          ]
        }
      ],
      '/reference/': [
        {
          text: 'Reference',
          items: [
            { text: 'Overview', link: '/reference/' },
            { text: 'Architecture', link: '/reference/architecture' },
            { text: 'API Reference', link: '/reference/api-reference' },
            { text: 'FFI API', link: '/reference/ffi-api' },
            { text: 'Tag Database', link: '/reference/tag-database' },
            { text: 'MakerNotes', link: '/reference/makernotes' },
            { text: 'ExifTool Coverage', link: '/reference/tag-coverage-analysis' },
            { text: 'Source Catalog Baseline', link: '/reference/catalog-baseline' },
            { text: 'Hydrated Reader Source', link: '/reference/hydrated-reader-layout-baseline' },
            { text: 'Catalog to Source Ledger', link: '/reference/catalog-hydrated-join' },
            { text: 'Verified Read and Write Observations', link: '/reference/catalog-hydrated-observed' },
            { text: 'Corpus Read Observations', link: '/reference/catalog-corpus-observed' },
            { text: 'Metadata Parity Resume Guide', link: '/reference/metadata-parity-resume' },
            { text: 'BinaryData Engine', link: '/reference/binary-data-engine' }
          ]
        },
        {
          text: 'Compatibility',
          collapsed: true,
          items: [
            { text: 'Overview', link: '/reference/comparison/' },
            { text: 'JPEG Tag Support', link: '/reference/jpeg-tag-support' },
            { text: 'JPEG Tag Matrix', link: '/reference/jpeg-tag-matrix' },
            ...getComparisonFormats()
          ]
        },
        {
          text: 'Formats',
          items: [
            { text: 'Overview', link: '/reference/formats/' },
            { text: 'Camera RAW', link: '/reference/formats/camera-raw' },
            { text: 'PE Executable', link: '/reference/formats/pe-executable' }
          ]
        },
        {
          text: 'API Documentation',
          items: [
            { text: 'Rust API', link: '/reference/api/' }
          ]
        },
        {
          text: 'Packaging',
          items: [
            { text: 'Distribution', link: '/reference/packaging/' }
          ]
        }
      ],
      '/architecture/': [
        {
          text: 'Architecture',
          items: [
            { text: 'Overview', link: '/architecture/' },
            { text: 'Tag Database', link: '/architecture/tag-database' },
            { text: 'Multi-Crate Tags', link: '/architecture/multi-crate-tags' },
            { text: 'Parser Shared Infrastructure', link: '/architecture/parser-shared-infrastructure' },
            { text: 'Parser Migration Guide', link: '/architecture/parser-migration-guide' },
            { text: 'OxiDex Tags Shared', link: '/architecture/oxidex-tags-shared' },
            { text: 'Docs Site Deployment', link: '/architecture/github-pages-setup' }
          ]
        }
      ],
      '/performance/': [
        {
          text: 'Performance',
          items: [
            { text: 'Overview', link: '/performance/' },
            { text: 'Benchmarks', link: '/performance/benchmarks' },
            { text: 'Profiling', link: '/performance/profiling' },
            { text: 'Optimization Strategy', link: '/performance/optimization-strategy' }
          ]
        },
        {
          text: 'Historical Data',
          collapsed: true,
          items: [
            { text: 'Baseline (2025-11-15)', link: '/performance/baseline-2025-11-15' },
            { text: 'Post-Optimization (2025-11-15)', link: '/performance/post-optimization-2025-11-15' },
            { text: 'Compilation Speedup', link: '/performance/compilation-speedup' }
          ]
        }
      ],
      '/contributing/': [
        {
          text: 'Contributing',
          items: [
            { text: 'Getting Started', link: '/contributing/' },
            { text: 'Release Checklist', link: '/contributing/release-checklist' }
          ]
        },
        {
          text: 'Development',
          items: [
            { text: 'Tag Machinery Status', link: '/TAG_MACHINERY_STATUS' },
            { text: 'Automation Backlog', link: '/AUTOMATION-AND-TESTER-PLAN' },
            { text: 'Development Guide', link: '/contributing/development/' },
            { text: 'Code Quality Patterns', link: '/contributing/development/code-quality-patterns' },
            { text: 'TagRegistry Refactoring', link: '/contributing/development/tagregistry-refactoring' }
          ]
        },
        {
          text: 'Testing',
          items: [
            { text: 'Testing Guide', link: '/contributing/testing/' },
            { text: 'Test Failure Triage', link: '/contributing/testing/TEST_FAILURE_TRIAGE' }
          ]
        },
        {
          text: 'ExifTool Comparison',
          collapsed: true,
          items: [
            { text: 'Measuring Coverage', link: '/contributing/measuring-coverage' },
            { text: 'ExifTool Coverage Report', link: '/reference/tag-coverage-analysis' }
          ]
        }
      ],
      '/tag-domains/': [
        {
          text: 'Tag Domains',
          items: [
            { text: 'Overview', link: '/tag-domains/' },
            { text: 'Core', link: '/tag-domains/core' },
            { text: 'Camera', link: '/tag-domains/camera' },
            { text: 'Image', link: '/tag-domains/image' },
            { text: 'Media', link: '/tag-domains/media' },
            { text: 'Document', link: '/tag-domains/document' },
            { text: 'Specialty', link: '/tag-domains/specialty' }
          ]
        }
      ]
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/swack-tools/oxidex' }
    ],

    footer: {
      message: 'Released under the GPL-3.0 License.',
      copyright: 'Copyright © 2024-2026 OxiDex Contributors'
    },

    editLink: {
      pattern: `${REPO_URL}/edit/${IS_PREVIEW ? PREVIEW_BRANCH : 'main'}/docs/:path`,
      text: 'Edit this page on GitHub'
    },

    search: {
      provider: 'local'
    },

    outline: [2, 3]
  },

  // VitePress does not rewrite head URLs for `base`. The stable href is kept
  // byte-identical to what oxidex.net has always served (note: /oxidex/logo.svg
  // 404s there -- the file is at /logo.svg; fixing it changes the live site, so
  // it is left to a separate change). The preview uses the base-correct path.
  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: IS_PREVIEW ? `${BASE}logo.svg` : '/oxidex/logo.svg' }],
    ['meta', { name: 'theme-color', content: '#dd7732' }],
    ['meta', { name: 'og:type', content: 'website' }],
    ['meta', { name: 'og:locale', content: 'en' }],
    ['meta', { name: 'og:site_name', content: 'OxiDex' }],
    // A preview must never outrank the release it previews in search results.
    ...(IS_PREVIEW
      ? ([
          ['meta', { name: 'robots', content: 'noindex, nofollow' }],
          ['style', {}, BANNER_CSS]
        ] as [string, Record<string, string>, string?][])
      : [])
  ],

  vite: {
    define: {
      __OXIDEX_PREVIEW__: JSON.stringify(PREVIEW)
    }
  },

  markdown: {
    // Criterion reports under /benchmarks/ are copied into the STABLE site by
    // deploy-docs.yml; the preview never carries them. The pages link to them
    // with raw <a href="/benchmarks/..."> HTML (which VitePress does not
    // base-prefix, so it escapes the base) and with Markdown links (which it
    // does prefix, to a path the preview does not have). Both would 404.
    // Point them at the stable site's reports instead. Preview only.
    ...(IS_PREVIEW
      ? {
          config: (md: any) => {
            const rewrite = (s: string) => s.replace(/href="\/benchmarks\//g, `href="${STABLE_URL}benchmarks/`)
            md.core.ruler.push('oxidex-preview-benchmarks', (state: any) => {
              for (const tok of state.tokens) {
                if (tok.type === 'html_block') tok.content = rewrite(tok.content)
                for (const child of tok.children || []) {
                  if (child.type === 'html_inline') child.content = rewrite(child.content)
                  // Markdown links, e.g. [x](/benchmarks/...): same destination.
                  const href = child.type === 'link_open' ? child.attrGet('href') : null
                  if (href && href.startsWith('/benchmarks/')) {
                    child.attrSet('href', `${STABLE_URL}${href.slice(1)}`)
                  }
                }
              }
            })
          }
        }
      : {}),
    theme: {
      light: 'github-light',
      dark: 'github-dark'
    },
    lineNumbers: true,
    languageAlias: {
      'rust,ignore': 'rust',
      'rust,no_run': 'rust'
    }
  },

  ignoreDeadLinks: [
    // Benchmark reports - deployed separately by CI
    /^\/benchmarks\//
  ]
})
