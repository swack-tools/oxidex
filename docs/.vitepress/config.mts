import { defineConfig } from 'vitepress'
import fs from 'node:fs'
import path from 'node:path'

// Dynamically generate sidebar items for comparison formats
function getComparisonFormats() {
  const comparisonDir = path.resolve(__dirname, '../reference/comparison')
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
  base: '/', // Custom domain (oxidex.net) serves from root
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
        text: 'v1.2.1',
        items: [
          { text: 'Changelog', link: '/changelog' },
          { text: 'Contributing', link: '/contributing/' }
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
            { text: 'ExifTool Coverage', link: '/reference/tag-coverage-analysis' }
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
            { text: 'OxiDex Tags Shared', link: '/architecture/oxidex-tags-shared' }
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
            { text: 'Development Guide', link: '/contributing/development/' },
            { text: 'Code Quality Patterns', link: '/contributing/development/code-quality-patterns' },
            { text: 'TagRegistry Refactoring', link: '/contributing/development/tagregistry-refactoring' },
            { text: 'Archived Context', link: '/contributing/development/archived-context' }
          ]
        },
        {
          text: 'Testing',
          items: [
            { text: 'Testing Guide', link: '/contributing/testing/' },
            { text: 'Integration Test Plan', link: '/contributing/testing/integration_test_plan' },
            { text: 'Test Failure Triage', link: '/contributing/testing/TEST_FAILURE_TRIAGE' }
          ]
        },
        {
          text: 'ExifTool Comparison',
          collapsed: true,
          items: [
            { text: 'Overview', link: '/contributing/testing/comparison/README' },
            { text: 'Parity Report', link: '/contributing/testing/comparison/PARITY_REPORT' },
            { text: 'Field Naming Guide', link: '/contributing/testing/comparison/FIELD_NAMING_GUIDE' },
            { text: 'Test Coverage', link: '/contributing/testing/comparison/TEST_COVERAGE' }
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
      copyright: 'Copyright © 2024-2025 OxiDex Contributors'
    },

    editLink: {
      pattern: 'https://github.com/swack-tools/oxidex/edit/main/docs/:path',
      text: 'Edit this page on GitHub'
    },

    search: {
      provider: 'local'
    },

    outline: [2, 3]
  },

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/oxidex/logo.svg' }],
    ['meta', { name: 'theme-color', content: '#dd7732' }],
    ['meta', { name: 'og:type', content: 'website' }],
    ['meta', { name: 'og:locale', content: 'en' }],
    ['meta', { name: 'og:site_name', content: 'OxiDex' }]
  ],

  markdown: {
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
