# VitePress Documentation Site Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a VitePress documentation site on gh-pages branch with migrated content, preserving CI/CD benchmark deployment.

**Architecture:** VitePress site at root of gh-pages, benchmarks preserved at /benchmarks/, separate CI workflow for docs deployment, progressive disclosure structure (guide → reference → performance → contributing).

**Tech Stack:** VitePress 1.5.0, Vue 3.5.13, Node.js 18+, GitHub Actions

---

## Phase 1: Setup VitePress Infrastructure

### Task 1: Initialize Node.js Project

**Files:**
- Create: `.worktrees/gh-pages/package.json`
- Create: `.worktrees/gh-pages/tsconfig.json`
- Create: `.worktrees/gh-pages/.gitignore`

**Step 1: Create package.json**

```bash
cd .worktrees/gh-pages
```

Create `.worktrees/gh-pages/package.json`:
```json
{
  "name": "oxidex-docs",
  "version": "1.1.0",
  "description": "Documentation site for OxiDex - Modern Rust ExifTool",
  "private": true,
  "type": "module",
  "scripts": {
    "docs:dev": "vitepress dev",
    "docs:build": "vitepress build",
    "docs:preview": "vitepress preview"
  },
  "devDependencies": {
    "vitepress": "^1.5.0",
    "vue": "^3.5.13"
  },
  "engines": {
    "node": ">=18.0.0"
  }
}
```

**Step 2: Create tsconfig.json**

Create `.worktrees/gh-pages/tsconfig.json`:
```json
{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "lib": ["ESNext", "DOM"],
    "moduleResolution": "Bundler",
    "resolveJsonModule": true,
    "strict": true,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "types": ["vitepress/client"]
  },
  "include": [".vitepress/**/*"],
  "exclude": ["node_modules", ".vitepress/dist"]
}
```

**Step 3: Create .gitignore**

Create `.worktrees/gh-pages/.gitignore`:
```
node_modules/
.vitepress/dist/
.vitepress/cache/
package-lock.json
```

**Step 4: Install dependencies**

Run:
```bash
cd .worktrees/gh-pages
npm install
```

Expected: VitePress and Vue installed successfully

**Step 5: Commit**

```bash
git add package.json tsconfig.json .gitignore
git commit -m "feat: initialize VitePress project

Add Node.js configuration and VitePress dependencies

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: Create VitePress Configuration

**Files:**
- Create: `.worktrees/gh-pages/.vitepress/config.mts`
- Create: `.worktrees/gh-pages/.vitepress/theme/index.ts`
- Create: `.worktrees/gh-pages/.vitepress/theme/custom.css`

**Step 1: Create directory structure**

Run:
```bash
cd .worktrees/gh-pages
mkdir -p .vitepress/theme
```

**Step 2: Create VitePress config**

Create `.worktrees/gh-pages/.vitepress/config.mts`:
```typescript
import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'OxiDex',
  description: 'Modern, high-performance Rust implementation of ExifTool',
  lang: 'en-US',
  base: '/oxidex/',
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
      { text: 'Performance', link: '/performance/' },
      {
        text: 'v1.1.0',
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
            { text: 'Troubleshooting', link: '/guide/troubleshooting' }
          ]
        }
      ],
      '/reference/': [
        {
          text: 'Reference',
          items: [
            { text: 'Architecture', link: '/reference/architecture' },
            { text: 'API Reference', link: '/reference/api-reference' },
            { text: 'FFI API', link: '/reference/ffi-api' },
            { text: 'Tag Database', link: '/reference/tag-database' },
            { text: 'Formats', link: '/reference/formats/' }
          ]
        }
      ],
      '/performance/': [
        {
          text: 'Performance',
          items: [
            { text: 'Overview', link: '/performance/' },
            { text: 'Benchmarks', link: '/performance/benchmarks' },
            { text: 'Profiling', link: '/performance/profiling' }
          ]
        }
      ],
      '/contributing/': [
        {
          text: 'Contributing',
          items: [
            { text: 'Getting Started', link: '/contributing/' },
            { text: 'Development', link: '/contributing/development' }
          ]
        }
      ]
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/swack-tools/oxidex' }
    ],

    footer: {
      message: 'Released under the GPL-3.0 License.',
      copyright: 'Copyright © 2024 OxiDex Contributors'
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
    lineNumbers: true
  }
})
```

**Step 3: Create theme files**

Create `.worktrees/gh-pages/.vitepress/theme/index.ts`:
```typescript
import DefaultTheme from 'vitepress/theme'
import './custom.css'

export default {
  extends: DefaultTheme
}
```

Create `.worktrees/gh-pages/.vitepress/theme/custom.css`:
```css
:root {
  --vp-c-brand-1: #dd7732;
  --vp-c-brand-2: #c96628;
  --vp-c-brand-3: #b5551e;
  --vp-code-bg: #f6f8fa;
}

.dark {
  --vp-c-brand-1: #f59e6f;
  --vp-c-brand-2: #dd7732;
  --vp-c-brand-3: #c96628;
  --vp-code-bg: #161b22;
}

.benchmark-link {
  display: inline-flex;
  align-items: center;
  padding: 0.5rem 1rem;
  margin: 0.5rem 0;
  border: 1px solid var(--vp-c-divider);
  border-radius: 8px;
  transition: all 0.2s;
}

.benchmark-link:hover {
  border-color: var(--vp-c-brand-1);
  background-color: var(--vp-c-bg-soft);
}
```

**Step 4: Test VitePress dev server**

Run:
```bash
cd .worktrees/gh-pages
npm run docs:dev
```

Expected: Dev server starts at http://localhost:5173/oxidex/ (will show 404 until we add index.md)

Press Ctrl+C to stop server.

**Step 5: Commit**

```bash
git add .vitepress/
git commit -m "feat: add VitePress configuration

Configure navigation, sidebar, theme, and styling

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Create Directory Structure

**Files:**
- Create: `.worktrees/gh-pages/guide/`
- Create: `.worktrees/gh-pages/reference/`
- Create: `.worktrees/gh-pages/reference/formats/`
- Create: `.worktrees/gh-pages/performance/`
- Create: `.worktrees/gh-pages/contributing/`
- Create: `.worktrees/gh-pages/public/`

**Step 1: Create directories**

Run:
```bash
cd .worktrees/gh-pages
mkdir -p guide reference/formats performance contributing public/images
```

**Step 2: Verify structure**

Run:
```bash
tree -L 2 -d
```

Expected: Shows directory tree with guide/, reference/, performance/, contributing/, public/

**Step 3: Commit**

```bash
git add guide/ reference/ performance/ contributing/ public/
git commit -m "feat: create documentation directory structure

Add folders for guide, reference, performance, contributing

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>" --allow-empty
```

---

## Phase 2: Migrate Content - Priority 1 (Core User Docs)

### Task 4: Create Home Page

**Files:**
- Create: `.worktrees/gh-pages/index.md`
- Reference: `README.md` (main branch)

**Step 1: Read source content**

Run from main repo:
```bash
cat README.md | head -150
```

**Step 2: Create index.md with hero layout**

Create `.worktrees/gh-pages/index.md`:
```markdown
---
layout: home

hero:
  name: OxiDex
  text: Modern ExifTool in Rust
  tagline: High-performance metadata management for 300+ file formats
  actions:
    - theme: brand
      text: Get Started
      link: /guide/getting-started
    - theme: alt
      text: View Benchmarks
      link: /performance/benchmarks
    - theme: alt
      text: GitHub
      link: https://github.com/swack-tools/oxidex

features:
  - icon: ⚡
    title: Up to 10x Faster
    details: 3.7-9.7x performance improvement over Perl ExifTool with zero-cost abstractions and parallel processing
  - icon: 🔒
    title: Memory Safe
    details: Rust eliminates buffer overflows, use-after-free bugs, and entire classes of vulnerabilities
  - icon: 🎯
    title: 32,677 Metadata Tags
    details: Complete parity with ExifTool across 140+ format families, automatically synchronized
  - icon: 🛠️
    title: Drop-in Replacement
    details: CLI compatible with original ExifTool syntax for seamless migration
  - icon: 📦
    title: Static Binaries
    details: Self-contained executables with no runtime dependencies for easy deployment
  - icon: 🌐
    title: Cross-Platform
    details: Native binaries for Windows, Linux (x86_64/ARM64), and macOS (Intel/Apple Silicon)
---

## Quick Example

```bash
# Extract all metadata from a file
oxidex photo.jpg

# Extract specific tags
oxidex -Make -Model -DateTimeOriginal photo.jpg

# Write metadata
oxidex -Artist="Your Name" photo.jpg

# Batch processing (recursive)
oxidex -r /path/to/photos/

# JSON output
oxidex -json photo.jpg
```

## Performance Comparison

OxiDex delivers exceptional performance improvements over the Perl-based ExifTool:

- **3.7x faster** - Single file metadata extraction (31.8ms vs 116.5ms)
- **9.7x faster** - Batch processing 1000 files (197ms vs 1911ms)
- **8.7x faster** - Write operations (23ms vs 200ms)
- **6.5x faster** - Format detection (10ms vs 67ms)

[View detailed benchmarks →](/performance/benchmarks)

## Why OxiDex?

**For Photographers & Archivists:**
- Process large image libraries in seconds, not minutes
- Reliable metadata preservation with memory-safe operations
- Support for 40+ camera RAW formats

**For Developers:**
- Native Rust library API for integration
- C FFI bindings for cross-language support
- Comprehensive documentation and examples

**For DevOps:**
- Static binaries with no dependencies
- Cross-compilation for all major platforms
- Continuous fuzzing for security

## Supported Formats

140+ format families including:
- **Images:** JPEG, PNG, TIFF, GIF, BMP, WebP, HEIF
- **RAW:** Canon (CR2/CR3), Nikon (NEF), Sony (ARW), and 35+ more
- **Video:** MP4, MOV, MKV, AVI, FLV
- **Audio:** MP3, FLAC, AAC, WAV, OGG
- **Documents:** PDF, Office formats
- **Metadata:** EXIF, XMP, IPTC, ICC Profiles, MakerNotes

[See complete format list →](/reference/formats/)
```

**Step 3: Test home page**

Run:
```bash
cd .worktrees/gh-pages
npm run docs:dev
```

Visit http://localhost:5173/oxidex/

Expected: Home page displays with hero, features, and quick example

Press Ctrl+C to stop server.

**Step 4: Commit**

```bash
git add index.md
git commit -m "feat: create home page with hero layout

Add landing page with features, quick example, and performance highlights

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: Migrate Guide - Introduction

**Files:**
- Create: `.worktrees/gh-pages/guide/index.md`
- Reference: `docs/book/src/intro.md` (main branch)

**Step 1: Copy and adapt intro content**

Create `.worktrees/gh-pages/guide/index.md`:
```markdown
# Introduction

OxiDex is a modern, high-performance Rust implementation of the industry-standard [ExifTool](https://exiftool.org/) metadata management library and command-line application.

## What is OxiDex?

OxiDex provides a memory-safe, high-performance alternative to the Perl-based ExifTool while maintaining full compatibility with its extensive metadata tag support. It delivers superior performance through zero-cost abstractions, native cross-compilation capabilities, and seamless integration into modern software ecosystems.

## Key Capabilities

### Metadata Extraction
Read metadata from 300+ file formats including images (JPEG, PNG, TIFF, RAW), videos (MP4, MKV, AVI), audio (MP3, FLAC), and documents (PDF).

### Metadata Writing
Modify EXIF, XMP, and IPTC metadata with atomic file operations ensuring data integrity.

### Batch Processing
Process thousands of files in parallel, leveraging all CPU cores for maximum performance.

### Format Detection
Automatically identify file formats using magic byte detection, even when file extensions are incorrect.

## Who Should Use OxiDex?

**Photographers:** Manage metadata in large photo libraries efficiently. Process 1000 RAW files in under 200ms.

**Archivists:** Preserve and extract metadata from diverse file formats with memory-safe operations.

**Developers:** Integrate metadata management into applications via Rust library API or C FFI bindings.

**System Administrators:** Deploy static binaries with no runtime dependencies across multiple platforms.

## Current Status

**Version:** 1.1.0 (Stable Release)

- ✅ 32,677 metadata tags (113% of ExifTool's 28,853 tags)
- ✅ 140+ format families with complete ExifTool parity
- ✅ 3.7-9.7x performance improvement
- ✅ Full CLI with backward compatibility
- ✅ Rust library API and C FFI bindings
- ✅ Cross-platform binaries (Linux, macOS, Windows)

## Next Steps

- [Installation Guide](/guide/getting-started) - Install OxiDex via cargo, homebrew, or binaries
- [CLI Usage](/guide/cli-usage) - Learn command-line interface
- [Library API](/guide/library-api) - Integrate OxiDex into Rust projects
- [Performance](/performance/) - View benchmark comparisons

## Project Goals

1. **100% ExifTool Tag Parity:** Support all 32,677+ metadata tags
2. **High Performance:** 10-100x faster than Perl implementation
3. **Memory Safety:** Eliminate vulnerabilities through Rust's ownership system
4. **Drop-in Replacement:** CLI compatibility for seamless migration
5. **Developer-Friendly:** Clean API for library and FFI integration

## License

OxiDex is released under the GNU General Public License v3.0 (GPL-3.0).

## Acknowledgments

This project is inspired by [ExifTool](https://exiftool.org/) by Phil Harvey. OxiDex is an independent reimplementation and is not affiliated with or endorsed by the original ExifTool project.
```

**Step 2: Test guide page**

Run:
```bash
cd .worktrees/gh-pages
npm run docs:dev
```

Visit http://localhost:5173/oxidex/guide/

Expected: Introduction page displays correctly

**Step 3: Commit**

```bash
git add guide/index.md
git commit -m "docs: add guide introduction page

Migrate intro content from mdBook

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 6: Migrate Guide - Getting Started

**Files:**
- Create: `.worktrees/gh-pages/guide/getting-started.md`
- Reference: `docs/book/src/installation.md`, `README.md` (main branch)

**Step 1: Create getting started guide**

Create `.worktrees/gh-pages/guide/getting-started.md`:
```markdown
# Getting Started

This guide will help you install OxiDex and run your first commands.

## Installation

OxiDex provides multiple installation methods. Choose the one that works best for your workflow.

### Option 1: Cargo (Recommended for Rust Users)

Install directly from crates.io:

```bash
cargo install oxidex
```

Verify installation:

```bash
oxidex --version
```

### Option 2: Homebrew (macOS)

For macOS users with [Homebrew](https://brew.sh):

```bash
# Install from Homebrew formula (source build)
brew install --build-from-source https://raw.githubusercontent.com/swack-tools/oxidex/main/packaging/homebrew/oxidex.rb

# Verify installation
oxidex --version
```

**Note:** The Homebrew formula builds from source, which may take 5-10 minutes.

### Option 3: Pre-Built Binaries

Download static binaries from the [GitHub Releases](https://github.com/swack-tools/oxidex/releases) page:

**Linux (x86_64):**
```bash
wget https://github.com/swack-tools/oxidex/releases/download/v1.1.0/oxidex-x86_64-linux-musl.tar.gz
tar xzf oxidex-x86_64-linux-musl.tar.gz
sudo mv oxidex /usr/local/bin/
oxidex --version
```

**Linux (ARM64):**
```bash
wget https://github.com/swack-tools/oxidex/releases/download/v1.1.0/oxidex-aarch64-linux-musl.tar.gz
tar xzf oxidex-aarch64-linux-musl.tar.gz
sudo mv oxidex /usr/local/bin/
oxidex --version
```

**macOS (Intel):**
```bash
wget https://github.com/swack-tools/oxidex/releases/download/v1.1.0/oxidex-x86_64-macos.tar.gz
tar xzf oxidex-x86_64-macos.tar.gz
sudo mv oxidex /usr/local/bin/
oxidex --version
```

**macOS (Apple Silicon):**
```bash
wget https://github.com/swack-tools/oxidex/releases/download/v1.1.0/oxidex-aarch64-macos.tar.gz
tar xzf oxidex-aarch64-macos.tar.gz
sudo mv oxidex /usr/local/bin/
oxidex --version
```

**Windows (x86_64):**
Download `oxidex-x86_64-windows.zip` from releases, extract, and add to PATH.

### Option 4: Build from Source

For development or custom builds:

```bash
# Clone the repository
git clone https://github.com/swack-tools/oxidex.git
cd oxidex

# Build release binary
cargo build --release

# Run
./target/release/oxidex --version

# Optional: Install to system
cargo install --path .
```

## First Steps

### Extract Metadata from a File

```bash
oxidex photo.jpg
```

Output:
```
FileName: photo.jpg
FileSize: 2.3 MB
Make: Canon
Model: Canon EOS 5D Mark IV
DateTimeOriginal: 2024:11:15 14:23:05
ISO: 400
FNumber: 5.6
ExposureTime: 1/250
...
```

### Extract Specific Tags

```bash
oxidex -Make -Model -DateTimeOriginal photo.jpg
```

Output:
```
Make: Canon
Model: Canon EOS 5D Mark IV
DateTimeOriginal: 2024:11:15 14:23:05
```

### Write Metadata

```bash
oxidex -Artist="Jane Doe" -Copyright="Copyright 2024" photo.jpg
```

### Process Multiple Files

```bash
# Recursive directory scan
oxidex -r /path/to/photos/

# Specific file pattern
oxidex *.jpg
```

### Output Formats

**JSON:**
```bash
oxidex -json photo.jpg
```

**CSV (for batch analysis):**
```bash
oxidex -csv -r /path/to/photos/ > metadata.csv
```

## Verification

Test your installation with a sample command:

```bash
# Create a test file (if you don't have one)
echo "test" > test.txt

# Extract metadata
oxidex test.txt
```

Expected output should include file information like FileName, FileSize, etc.

## Next Steps

- [CLI Usage Guide](/guide/cli-usage) - Learn all command-line options
- [Library API Guide](/guide/library-api) - Use OxiDex in Rust projects
- [Troubleshooting](/guide/troubleshooting) - Common issues and solutions

## System Requirements

- **OS:** Linux (Ubuntu 18.04+), macOS (10.15+), Windows (10+)
- **Architecture:** x86_64 or ARM64
- **For source builds:** Rust 1.75+

## Getting Help

- [GitHub Issues](https://github.com/swack-tools/oxidex/issues) - Report bugs or request features
- [Troubleshooting Guide](/guide/troubleshooting) - Common problems
- [GitHub Discussions](https://github.com/swack-tools/oxidex/discussions) - Ask questions
```

**Step 2: Test page**

Run:
```bash
npm run docs:dev
```

Visit http://localhost:5173/oxidex/guide/getting-started

Expected: Getting started page displays with installation options

**Step 3: Commit**

```bash
git add guide/getting-started.md
git commit -m "docs: add getting started guide

Installation instructions and first steps

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 7: Migrate Guide - CLI Usage

**Files:**
- Create: `.worktrees/gh-pages/guide/cli-usage.md`
- Reference: `docs/book/src/cli_usage.md`, `README.md` (main branch)

**Step 1: Create CLI usage guide**

Due to length, the full content would include:
- All CLI flags and options
- Common usage patterns
- Output format examples
- Batch processing examples
- Advanced features (date shifting, tag copying)

Create `.worktrees/gh-pages/guide/cli-usage.md` with comprehensive CLI documentation.

**Step 2: Test page**

**Step 3: Commit**

```bash
git add guide/cli-usage.md
git commit -m "docs: add CLI usage guide

Comprehensive command-line reference

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 8: Migrate Guide - Library API

**Files:**
- Create: `.worktrees/gh-pages/guide/library-api.md`
- Reference: `docs/book/src/library_api.md` (main branch)

**Step 1: Create library API guide**

**Step 2: Test page**

**Step 3: Commit**

```bash
git add guide/library-api.md
git commit -m "docs: add library API guide

Rust library integration guide

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 9: Migrate Guide - Troubleshooting

**Files:**
- Create: `.worktrees/gh-pages/guide/troubleshooting.md`
- Reference: `docs/book/src/troubleshooting.md` (main branch)

**Step 1: Create troubleshooting guide**

**Step 2: Test page**

**Step 3: Commit**

```bash
git add guide/troubleshooting.md
git commit -m "docs: add troubleshooting guide

Common issues and solutions

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com)"
```

---

### Task 10: Create Changelog Page

**Files:**
- Create: `.worktrees/gh-pages/changelog.md`
- Reference: `CHANGELOG.md` (main branch)

**Step 1: Copy changelog**

```bash
cd .worktrees/gh-pages
cp ../../CHANGELOG.md ./changelog.md
```

**Step 2: Add frontmatter**

Edit `.worktrees/gh-pages/changelog.md` and add at top:
```markdown
---
outline: deep
---

# Changelog

```

**Step 3: Test page**

**Step 4: Commit**

```bash
git add changelog.md
git commit -m "docs: add changelog page

Version history from CHANGELOG.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Phase 3: Migrate Content - Priority 2 (Reference & Performance)

### Task 11: Create Performance Overview

**Files:**
- Create: `.worktrees/gh-pages/performance/index.md`
- Reference: `README.md`, `benches/benchmark_results.md` (main branch)

**Step 1: Create performance overview**

Create `.worktrees/gh-pages/performance/index.md`:
```markdown
# Performance

OxiDex delivers exceptional performance improvements over the Perl-based ExifTool through zero-cost abstractions, parallel processing, and native compiled code.

## Benchmark Results

The following benchmarks compare OxiDex against the original Perl ExifTool running on identical hardware.

### Summary

| Scenario | Perl ExifTool | OxiDex | Speedup |
|----------|---------------|--------|---------|
| Single JPEG Read | 116.5ms ± 15.6ms | 31.8ms ± 14.1ms | **3.7x faster** |
| Batch Processing (1000 files) | 1911.4ms ± 171.9ms | 197.6ms ± 3.1ms | **9.7x faster** |
| Write Operation | 200.7ms ± 50.4ms | 23.0ms ± 1.6ms | **8.7x faster** |
| Format Detection | 67.3ms ± 13.9ms | 10.4ms ± 3.6ms | **6.5x faster** |

### System Specifications

- **OS:** Linux (Ubuntu 22.04)
- **CPU:** x86_64 (4 cores)
- **Memory:** 8GB RAM
- **Perl ExifTool:** latest version
- **OxiDex:** version 1.1.0

## Live Benchmark Reports

View detailed interactive benchmark results from our CI/CD pipeline:

<div class="benchmark-links">

- 📊 [Main Benchmark Report](/oxidex/benchmarks/report/index.html)
- 📁 [Single File Extraction](/oxidex/benchmarks/single_extraction/report/index.html)
- 📦 [Batch Processing (100 JPEGs)](/oxidex/benchmarks/batch_100_jpegs/report/index.html)
- 🎯 [Format Comparison](/oxidex/benchmarks/format_comparison/report/index.html)
- 🔍 [Format Detection](/oxidex/benchmarks/format_detection/report/index.html)
- 📝 [Full Metadata Read](/oxidex/benchmarks/full_read_metadata/report/index.html)

</div>

These reports are automatically generated by [Criterion.rs](https://github.com/bheisler/criterion.rs) on every commit to main and include:
- Performance graphs with statistical analysis
- Historical trend comparisons
- Outlier detection
- Detailed timing breakdowns

## Key Performance Improvements

### 1. Single File Operations (3.7x faster)

Zero-cost abstractions and compiled code eliminate Perl interpreter overhead:
- No runtime interpretation
- Direct memory access
- Optimized binary parsing with `nom`

### 2. Batch Processing (9.7x faster)

Parallel processing with [Rayon](https://github.com/rayon-rs/rayon) leverages all CPU cores:
- Process 1000 files in 197.6ms vs 1911.4ms
- Linear scaling with CPU core count
- Efficient work stealing scheduler

### 3. Write Operations (8.7x faster)

Efficient binary manipulation and atomic file operations:
- In-place metadata updates
- Memory-mapped I/O with `memmap2`
- Atomic file writes prevent corruption

### 4. Format Detection (6.5x faster)

Native compiled code dramatically outperforms interpreted Perl:
- Efficient magic byte matching
- Compile-time optimizations
- CPU cache-friendly algorithms

## Reproducing Benchmarks

### Comparative Benchmarks (vs Perl ExifTool)

Run the comparative benchmark suite:

```bash
# Install prerequisites
brew install hyperfine exiftool  # macOS
# or
sudo apt install hyperfine libimage-exiftool-perl  # Ubuntu

# Build OxiDex in release mode
cargo build --release

# Run comparison benchmarks
./benches/exiftool_comparison.sh

# View results
cat benches/benchmark_results.md
```

### Library Micro-Benchmarks

Run detailed benchmarks for internal operations:

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench format_detection
cargo bench jpeg_segment_parsing
cargo bench tiff_ifd_parsing
cargo bench full_read_metadata
```

View HTML reports:

```bash
# macOS
open target/criterion/report/index.html

# Linux
xdg-open target/criterion/report/index.html

# Windows
start target/criterion/report/index.html
```

## Profiling

For detailed profiling information, see [Profiling Guide](/performance/profiling).

## Performance Tips

### For Maximum Throughput

1. **Use batch processing:** Process multiple files in one command for parallelism
2. **Extract specific tags only:** Use `-TagName` to avoid parsing entire file
3. **Binary output:** Use `-b` for binary tag values (faster than formatting)
4. **Disable verbose output:** Use `-q` for quiet mode

### Example: Fast Batch Extraction

```bash
# Extract just Make, Model, DateTimeOriginal from 10,000 JPEGs
oxidex -Make -Model -DateTimeOriginal -csv -r photos/ > metadata.csv
```

## Ongoing Optimization

We continuously monitor and improve performance:
- Automated benchmarks on every commit
- Profiling with [samply](https://github.com/mstange/samply)
- Flamegraph analysis
- Memory allocation tracking

See our [optimization strategy](/performance/optimization-strategy) for current focus areas.
```

**Step 2: Test page**

**Step 3: Commit**

```bash
git add performance/index.md
git commit -m "docs: add performance overview page

Benchmark results and links to live reports

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 12-20: Continue Content Migration

**Tasks 12-20 would cover:**
- Task 12: Performance - Benchmarks page
- Task 13: Performance - Profiling guide
- Task 14: Reference - Architecture
- Task 15: Reference - API Reference
- Task 16: Reference - FFI API
- Task 17: Reference - Tag Database
- Task 18: Reference - Formats overview
- Task 19: Contributing guide
- Task 20: Add static assets (logo, images)

Each following the same pattern:
1. Create file
2. Migrate/adapt content
3. Test in dev server
4. Commit

---

## Phase 4: CI/CD Integration

### Task 21: Create Deployment Workflow

**Files:**
- Create: `.github/workflows/deploy-docs.yml` (in main branch, not worktree)

**Step 1: Switch to main branch**

```bash
cd /Users/allen/Documents/git/exiftool-rs
```

**Step 2: Create workflow file**

Create `.github/workflows/deploy-docs.yml`:
```yaml
name: Deploy Documentation

on:
  push:
    branches: [main]
    paths:
      - '.worktrees/gh-pages/**'
      - '.github/workflows/deploy-docs.yml'
  workflow_dispatch:

jobs:
  build-deploy:
    runs-on: ubuntu-latest
    permissions:
      contents: write

    steps:
      - name: Checkout repository
        uses: actions/checkout@v4
        with:
          fetch-depth: 0

      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: '20'
          cache: 'npm'
          cache-dependency-path: .worktrees/gh-pages/package.json

      - name: Install dependencies
        run: |
          cd .worktrees/gh-pages
          npm ci

      - name: Build VitePress
        run: |
          cd .worktrees/gh-pages
          npm run docs:build

      - name: Deploy to GitHub Pages
        uses: peaceiris/actions-gh-pages@v4
        with:
          github_token: ${{ secrets.GITHUB_TOKEN }}
          publish_dir: .worktrees/gh-pages/.vitepress/dist
          publish_branch: gh-pages
          keep_files: false
          cname: oxidex.net
```

**Step 3: Test workflow syntax**

```bash
# Validate YAML syntax
cat .github/workflows/deploy-docs.yml | python -c "import yaml, sys; yaml.safe_load(sys.stdin)"
```

Expected: No output (valid YAML)

**Step 4: Commit**

```bash
git add .github/workflows/deploy-docs.yml
git commit -m "ci: add VitePress documentation deployment workflow

Deploy docs to gh-pages on push to main

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com)"
```

---

## Phase 5: Testing & Deployment

### Task 22: Test Local Build

**Step 1: Build VitePress site**

```bash
cd .worktrees/gh-pages
npm run docs:build
```

Expected: Build succeeds, output in `.vitepress/dist/`

**Step 2: Preview built site**

```bash
npm run docs:preview
```

Visit http://localhost:4173/oxidex/

Expected: Production build serves correctly

**Step 3: Verify all pages**

Test each page:
- Home: http://localhost:4173/oxidex/
- Guide: http://localhost:4173/oxidex/guide/
- Performance: http://localhost:4173/oxidex/performance/
- Reference: http://localhost:4173/oxidex/reference/
- Contributing: http://localhost:4173/oxidex/contributing/

Expected: All pages load without errors

**Step 4: Check benchmarks link**

Visit http://localhost:4173/oxidex/benchmarks/report/

Expected: 404 (benchmarks not in build, will be on gh-pages)

---

### Task 23: Deploy to gh-pages

**Step 1: Push worktree commits**

```bash
cd .worktrees/gh-pages
git push origin gh-pages
```

**Step 2: Verify GitHub Pages**

Visit https://oxidex.net

Expected: Site loads (may take a few minutes for DNS/deployment)

**Step 3: Test live site**

Check:
- Home page
- Navigation works
- Search works
- Benchmarks accessible at /benchmarks/report/
- Dark mode toggle works

**Step 4: Verify mobile responsive**

Test on:
- Mobile phone
- Tablet
- Desktop

---

### Task 24: Final Cleanup

**Step 1: Delete obsolete files in main branch**

```bash
cd /Users/allen/Documents/git/exiftool-rs

# Delete release docs
git rm RELEASE_ANNOUNCEMENT.md RELEASE_CHECKLIST.md

# Delete test fixture reports
git rm tests/data_lfs_error_summary.md tests/data_lfs_final_report.md
git rm tests/fixtures/I5T9_*.md tests/fixtures/COMPLETION_REPORT.md
```

**Step 2: Archive historical plans**

```bash
mkdir -p docs/plans/archived/2025-11
git mv docs/plans/2025-11-0[2-9]-*.md docs/plans/archived/2025-11/
git mv docs/plans/2025-11-1[0-7]-*.md docs/plans/archived/2025-11/
```

**Step 3: Commit cleanup**

```bash
git commit -m "chore: clean up obsolete documentation

- Remove outdated release and test docs
- Archive historical planning documents

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com)"
```

**Step 4: Push main branch**

```bash
git push origin main
```

---

## Success Criteria Checklist

After completing all tasks, verify:

- [ ] Site live at https://oxidex.net
- [ ] Home page displays correctly with hero layout
- [ ] All navigation links work
- [ ] Sidebar navigation works in each section
- [ ] Search functionality works
- [ ] Benchmarks accessible at /benchmarks/report/
- [ ] Dark mode toggle works
- [ ] Mobile responsive design works
- [ ] All internal links work (no 404s)
- [ ] CI/CD deploys docs automatically
- [ ] Repository cleaned (31 obsolete files removed)
- [ ] Page load time < 1 second
- [ ] Cross-browser compatibility (Chrome, Firefox, Safari, Edge)

---

## Estimated Time

- **Phase 1 (Setup):** 8-12 hours
- **Phase 2 (Content Migration P1):** 8-12 hours
- **Phase 3 (Content Migration P2):** 12-16 hours
- **Phase 4 (CI/CD):** 4-6 hours
- **Phase 5 (Testing):** 4-8 hours

**Total:** 36-54 hours (9-13 days at 4 hours/day)

---

## Notes

**Critical path:**
1. Setup (Tasks 1-3) must complete first
2. Home page (Task 4) unblocks testing
3. CI/CD (Task 21) can be done in parallel with content migration
4. Cleanup (Task 24) must be last

**Parallelization opportunities:**
- Tasks 5-10 (Guide pages) can be done in any order
- Tasks 12-20 (Reference/Performance) can be done in any order
- Task 21 (CI/CD) can be done while working on Tasks 12-20

**Dependencies:**
- All tasks require Node.js 18+ installed
- All tasks require being in `.worktrees/gh-pages` directory (except Task 21, 24)
- Task 23 (deployment) requires all content tasks complete

**Testing reminders:**
- Run `npm run docs:dev` frequently during content migration
- Test all links after completing each section
- Verify mobile responsive after each major page
- Check dark mode toggle on all pages

**For details on Tasks 7-20, refer to:**
- Main branch files: `README.md`, `docs/book/src/*.md`, `docs/*.md`
- Design document: `docs/plans/2025-11-18-vitepress-documentation-site-design.md`
- Audit report from subagent analysis
