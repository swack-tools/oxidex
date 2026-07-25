# ExifTool Compatibility Report - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Automated comparison of OxiDex vs ExifTool output, published to GitHub Pages on parser changes.

**Architecture:** Existing `tag-comparison` binary extended with value comparison, regression tracking, and markdown generation. New GitHub workflow triggers on `src/parsers/**` changes, downloads ExifTool test suite, runs comparison, commits reports.

**Tech Stack:** Rust (existing binary), GitHub Actions, VitePress (existing docs)

---

## Parallel Execution Groups

```
Group A (independent, run in parallel):
├── Task 1: Fix module structure
├── Task 2: Create GitHub workflow
├── Task 3: Create VitePress config + placeholder pages
└── Task 4: Add baseline.json schema

Group B (depends on Task 1):
├── Task 5: Add value comparison to models
├── Task 6: Add regression detection
└── Task 7: Add markdown report generation

Group C (depends on all above):
└── Task 8: Integration test and verification
```

---

## Task 1: Fix Module Structure

**Problem:** The binary uses inline module declaration but files are in a separate directory, causing compilation errors.

**Files:**
- Modify: `src/bin/tag-comparison.rs`
- Modify: `Cargo.toml` (add binary configuration)

**Step 1: Update Cargo.toml to define binary with path**

In `Cargo.toml`, find the `[[bin]]` section (or add after `[lib]`):

```toml
[[bin]]
name = "tag-comparison"
path = "src/bin/tag-comparison/main.rs"
```

**Step 2: Rename and restructure main file**

```bash
mv src/bin/tag-comparison.rs src/bin/tag-comparison/main.rs
```

**Step 3: Update main.rs imports**

Replace the content of `src/bin/tag-comparison/main.rs`:

```rust
//! Tag Comparison Binary
//!
//! Command-line tool to compare tags extracted by OxiDex vs ExifTool

use clap::Parser;
use std::path::PathBuf;

mod models;
mod extraction;
mod comparison;

use models::ComparisonReport;
use extraction::{OxiDexExtractor, ExifToolExtractor};
use comparison::ComparisonEngine;

#[derive(Parser, Debug)]
#[command(name = "tag-comparison")]
#[command(about = "Compare tags extracted by OxiDex vs ExifTool", long_about = None)]
struct Args {
    /// Path to test fixtures/samples directory
    #[arg(long, default_value = "tests/fixtures")]
    samples: PathBuf,

    /// Specific format to process (if not specified, all formats)
    #[arg(long)]
    format: Option<String>,

    /// Output directory for markdown reports
    #[arg(short, long, default_value = "docs/reference/comparison")]
    output: PathBuf,

    /// Path to baseline.json for regression detection
    #[arg(long, default_value = "docs/reference/comparison/baseline.json")]
    baseline: PathBuf,

    /// Path to exiftool executable
    #[arg(long, default_value = "exiftool")]
    exiftool: String,

    /// ExifTool version string (for report metadata)
    #[arg(long, default_value = "unknown")]
    exiftool_version: String,

    /// OxiDex version string (for report metadata)
    #[arg(long, default_value = "unknown")]
    oxidex_version: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    println!("🏷️  Tag Comparison Tool");
    println!("=======================\n");
    println!("ExifTool: v{}", args.exiftool_version);
    println!("OxiDex: v{}", args.oxidex_version);
    println!("Samples: {}", args.samples.display());
    println!();

    // Load baseline for regression detection
    let baseline = if args.baseline.exists() {
        let content = std::fs::read_to_string(&args.baseline)?;
        serde_json::from_str(&content).ok()
    } else {
        None
    };

    // Create report
    let mut report = ComparisonReport::new();
    report.exiftool_version = args.exiftool_version.clone();
    report.oxidex_version = args.oxidex_version.clone();

    // Auto-detect formats from samples directory
    let formats = detect_formats(&args.samples)?;
    println!("Found {} formats to process\n", formats.len());

    // Process each format
    for format in &formats {
        println!("Processing format: {}", format);

        let mut oxidex_extractor = OxiDexExtractor::new(args.samples.clone());
        let mut exiftool_extractor = ExifToolExtractor::new(args.exiftool.clone());

        match (
            oxidex_extractor.extract_format_tags(format).await,
            exiftool_extractor.extract_format_tags(format, &args.samples).await,
        ) {
            (Ok(oxidex_tags), Ok(exiftool_tags)) => {
                println!("  OxiDex: {} tags, ExifTool: {} tags", oxidex_tags.len(), exiftool_tags.len());

                let previous = baseline.as_ref().and_then(|b: &ComparisonReport| b.by_format.get(format));
                let comparison = ComparisonEngine::compare(oxidex_tags, exiftool_tags, format, previous);
                println!("  Result: {}", comparison.summary());

                report.add_format(format.clone(), comparison);
            }
            (Err(e), _) => eprintln!("  Error extracting OxiDex tags: {}", e),
            (_, Err(e)) => eprintln!("  Error extracting ExifTool tags: {}", e),
        }
    }

    report.calculate_overall_coverage();

    println!("\n📊 Overall Results");
    println!("==================");
    println!("{}", report.summary);

    // Generate markdown reports
    comparison::generate_markdown_reports(&report, &args.output)?;

    // Save new baseline
    let baseline_json = serde_json::to_string_pretty(&report)?;
    std::fs::write(&args.baseline, baseline_json)?;
    println!("\n✅ Baseline updated: {}", args.baseline.display());

    Ok(())
}

fn detect_formats(samples_path: &PathBuf) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut formats = Vec::new();

    if samples_path.is_dir() {
        for entry in std::fs::read_dir(samples_path)? {
            let entry = entry?;
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    formats.push(name.to_uppercase());
                }
            }
        }
    }

    formats.sort();
    Ok(formats)
}
```

**Step 4: Remove old lib.rs**

```bash
rm src/bin/tag-comparison/lib.rs
```

**Step 5: Run to verify compilation**

```bash
cargo build --release --bin tag-comparison
```

Expected: Build succeeds (may have warnings about unused code)

**Step 6: Commit**

```bash
git add src/bin/tag-comparison/ Cargo.toml
git commit -m "fix(tag-comparison): restructure module layout for proper compilation"
```

---

## Task 2: Create GitHub Workflow

**Files:**
- Create: `.github/workflows/compare-exiftool.yml`

**Step 1: Create workflow file**

```yaml
name: ExifTool Compatibility Report

on:
  push:
    branches: [main]
    paths:
      - 'src/parsers/**'
  workflow_dispatch:

jobs:
  compare:
    runs-on: ubuntu-latest
    permissions:
      contents: write

    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Get ExifTool version
        id: exiftool-version
        run: |
          VERSION=$(curl -s https://exiftool.org/ver.txt)
          echo "version=$VERSION" >> $GITHUB_OUTPUT
          echo "ExifTool version: $VERSION"

      - name: Cache ExifTool + test images
        uses: actions/cache@v4
        id: cache-exiftool
        with:
          path: ~/exiftool
          key: exiftool-${{ steps.exiftool-version.outputs.version }}

      - name: Download ExifTool release
        if: steps.cache-exiftool.outputs.cache-hit != 'true'
        run: |
          VERSION=${{ steps.exiftool-version.outputs.version }}
          echo "Downloading ExifTool $VERSION..."
          curl -L "https://github.com/exiftool/exiftool/archive/refs/tags/$VERSION.tar.gz" -o exiftool.tar.gz
          tar -xzf exiftool.tar.gz
          mv "exiftool-$VERSION" ~/exiftool
          echo "ExifTool downloaded to ~/exiftool"
          ls -la ~/exiftool/t/images | head -20

      - name: Setup Rust
        uses: dtolnay/rust-toolchain@stable

      - name: Cache Rust dependencies
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Build OxiDex and comparison tool
        run: |
          cargo build --release
          cargo build --release --bin tag-comparison

      - name: Get OxiDex version
        id: oxidex-version
        run: |
          VERSION=$(cargo pkgid oxidex | cut -d# -f2 | cut -d@ -f2)
          echo "version=$VERSION" >> $GITHUB_OUTPUT

      - name: Run comparison
        run: |
          ./target/release/tag-comparison \
            --exiftool ~/exiftool/exiftool \
            --samples ~/exiftool/t/images \
            --baseline docs/reference/comparison/baseline.json \
            --output docs/reference/comparison \
            --exiftool-version "${{ steps.exiftool-version.outputs.version }}" \
            --oxidex-version "${{ steps.oxidex-version.outputs.version }}"

      - name: Commit reports
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git add docs/reference/comparison/
          if git diff --staged --quiet; then
            echo "No changes to commit"
          else
            git commit -m "docs: update ExifTool compatibility report

          ExifTool: v${{ steps.exiftool-version.outputs.version }}
          OxiDex: v${{ steps.oxidex-version.outputs.version }}"
            git push
          fi
```

**Step 2: Verify YAML syntax**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/compare-exiftool.yml'))"
```

Expected: No output (valid YAML)

**Step 3: Commit**

```bash
git add .github/workflows/compare-exiftool.yml
git commit -m "ci: add ExifTool compatibility report workflow"
```

---

## Task 3: Create VitePress Config + Placeholder Pages

**Files:**
- Modify: `docs/.vitepress/config.mts`
- Create: `docs/reference/comparison/index.md`

**Step 1: Update VitePress sidebar**

In `docs/.vitepress/config.mts`, find the `/reference/` sidebar section and add after the "Reference" items block (around line 56):

```typescript
        {
          text: 'Compatibility',
          items: [
            { text: 'ExifTool Comparison', link: '/reference/comparison/' }
          ]
        },
```

**Step 2: Create placeholder index page**

Create `docs/reference/comparison/index.md`:

```markdown
---
title: ExifTool Compatibility Report
---

# ExifTool Compatibility Report

This page is automatically generated by the CI pipeline when parser code changes.

## Status

Report not yet generated. The comparison workflow runs automatically when changes are pushed to `src/parsers/**`.

To generate manually:

```bash
cargo run --release --bin tag-comparison -- \
  --exiftool /path/to/exiftool \
  --samples /path/to/test/images \
  --output docs/reference/comparison
```

## What This Report Shows

- **Coverage**: Percentage of ExifTool tags that OxiDex also extracts
- **Missing Tags**: Tags ExifTool finds but OxiDex doesn't
- **Extra Tags**: Tags OxiDex finds but ExifTool doesn't
- **Value Differences**: Same tag, different extracted value
- **Regressions**: Tags OxiDex previously extracted but no longer does
```

**Step 3: Create directory structure**

```bash
mkdir -p docs/reference/comparison
```

**Step 4: Commit**

```bash
git add docs/.vitepress/config.mts docs/reference/comparison/
git commit -m "docs: add ExifTool comparison section to VitePress"
```

---

## Task 4: Add Baseline JSON Schema

**Files:**
- Create: `docs/reference/comparison/baseline.json`

**Step 1: Create initial empty baseline**

Create `docs/reference/comparison/baseline.json`:

```json
{
  "generated_at": "1970-01-01T00:00:00Z",
  "exiftool_version": "0.0",
  "oxidex_version": "0.0.0",
  "by_format": {},
  "overall_coverage": 0.0,
  "summary": "Initial baseline - no data yet"
}
```

**Step 2: Commit**

```bash
git add docs/reference/comparison/baseline.json
git commit -m "docs: add initial empty baseline for comparison tracking"
```

---

## Task 5: Add Value Comparison to Models

**Depends on:** Task 1

**Files:**
- Modify: `src/bin/tag-comparison/models/mod.rs`

**Step 1: Update TagInfo to include value**

Find the `TagInfo` struct and update:

```rust
/// Information about a single metadata tag
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TagInfo {
    /// Tag name (e.g., "Make", "Model", "DateTime")
    pub name: String,
    /// Tag family/group (e.g., "EXIF", "XMP", "IPTC")
    pub family: String,
    /// Tag value as string
    pub value: String,
    /// Optional tag ID in hex format (e.g., "0x010F")
    pub tag_id: Option<String>,
    /// Source file this tag was extracted from
    pub source_file: Option<String>,
}

impl TagInfo {
    /// Create a new TagInfo
    pub fn new(name: String, family: String, value: String) -> Self {
        Self {
            name,
            family,
            value,
            tag_id: None,
            source_file: None,
        }
    }

    /// Unique key for this tag (family:name)
    pub fn key(&self) -> String {
        format!("{}:{}", self.family, self.name)
    }
}
```

**Step 2: Add ValueDifference struct**

Add after `TagInfo`:

```rust
/// Represents a difference in extracted value for the same tag
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueDifference {
    /// Tag family:name
    pub tag_key: String,
    /// Value from ExifTool
    pub exiftool_value: String,
    /// Value from OxiDex
    pub oxidex_value: String,
    /// Source file where difference was found
    pub source_file: String,
}
```

**Step 3: Update FormatComparison to track values and regressions**

Update the struct:

```rust
/// Comparison results for a single file format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatComparison {
    /// Format name (e.g., "JPEG", "PNG")
    pub format: String,
    /// Number of test files processed
    pub files_tested: usize,
    /// Tags matched (same name, family, and value)
    pub matched_tags: Vec<String>,
    /// Tags in ExifTool but missing in OxiDex
    pub missing_in_oxidex: Vec<TagInfo>,
    /// Tags in OxiDex but not in ExifTool
    pub extra_in_oxidex: Vec<TagInfo>,
    /// Tags with different values
    pub value_differences: Vec<ValueDifference>,
    /// Tags that were present in baseline but now missing (regressions)
    pub regressions: Vec<String>,
    /// Coverage percentage
    pub coverage_percentage: f64,
    /// Total unique tags in ExifTool
    pub total_exiftool_tags: usize,
    /// Timestamp
    pub timestamp: String,
}
```

**Step 4: Update ComparisonReport to include version info**

Update the struct:

```rust
/// Complete comparison report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    pub generated_at: String,
    pub exiftool_version: String,
    pub oxidex_version: String,
    pub by_format: HashMap<String, FormatComparison>,
    pub overall_coverage: f64,
    pub total_regressions: usize,
    pub summary: String,
}

impl ComparisonReport {
    pub fn new() -> Self {
        Self {
            generated_at: chrono::Utc::now().to_rfc3339(),
            exiftool_version: String::new(),
            oxidex_version: String::new(),
            by_format: HashMap::new(),
            overall_coverage: 0.0,
            total_regressions: 0,
            summary: String::new(),
        }
    }
}
```

**Step 5: Run tests**

```bash
cargo test --bin tag-comparison
```

Expected: Some tests fail (will fix in next steps)

**Step 6: Update tests to match new signatures**

Update the tests in models/mod.rs to use the new `TagInfo::new(name, family, value)` signature.

**Step 7: Commit**

```bash
git add src/bin/tag-comparison/models/
git commit -m "feat(tag-comparison): add value comparison and regression tracking to models"
```

---

## Task 6: Add Regression Detection

**Depends on:** Task 5

**Files:**
- Modify: `src/bin/tag-comparison/comparison/engine.rs`

**Step 1: Update ComparisonEngine::compare signature**

```rust
impl ComparisonEngine {
    /// Compare OxiDex and ExifTool tags for a format
    pub fn compare(
        oxidex_tags: Vec<TagInfo>,
        exiftool_tags: Vec<TagInfo>,
        format: &str,
        previous: Option<&FormatComparison>,
    ) -> FormatComparison {
        let mut comparison = FormatComparison {
            format: format.to_string(),
            files_tested: 0, // Set by caller
            matched_tags: Vec::new(),
            missing_in_oxidex: Vec::new(),
            extra_in_oxidex: Vec::new(),
            value_differences: Vec::new(),
            regressions: Vec::new(),
            coverage_percentage: 0.0,
            total_exiftool_tags: exiftool_tags.len(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        // Build lookup maps
        let oxidex_by_key: HashMap<String, &TagInfo> = oxidex_tags
            .iter()
            .map(|t| (t.key(), t))
            .collect();

        let mut matched_exiftool_keys = HashSet::new();

        // Compare each ExifTool tag
        for et_tag in &exiftool_tags {
            let key = et_tag.key();

            if let Some(ox_tag) = oxidex_by_key.get(&key) {
                matched_exiftool_keys.insert(key.clone());

                if ox_tag.value == et_tag.value {
                    comparison.matched_tags.push(key);
                } else {
                    // Value difference
                    comparison.value_differences.push(ValueDifference {
                        tag_key: key,
                        exiftool_value: et_tag.value.clone(),
                        oxidex_value: ox_tag.value.clone(),
                        source_file: et_tag.source_file.clone().unwrap_or_default(),
                    });
                }
            } else {
                comparison.missing_in_oxidex.push(et_tag.clone());
            }
        }

        // Find extra tags in OxiDex
        for ox_tag in &oxidex_tags {
            let key = ox_tag.key();
            if !matched_exiftool_keys.contains(&key) {
                comparison.extra_in_oxidex.push(ox_tag.clone());
            }
        }

        // Detect regressions (tags we had before but don't now)
        if let Some(prev) = previous {
            let current_matched: HashSet<_> = comparison.matched_tags.iter().collect();
            for prev_tag in &prev.matched_tags {
                if !current_matched.contains(prev_tag) {
                    comparison.regressions.push(prev_tag.clone());
                }
            }
        }

        // Calculate coverage
        if comparison.total_exiftool_tags > 0 {
            comparison.coverage_percentage =
                (comparison.matched_tags.len() as f64 / comparison.total_exiftool_tags as f64) * 100.0;
        }

        comparison
    }
}
```

**Step 2: Update tests**

Update tests to pass `None` for the previous parameter in basic cases.

**Step 3: Add regression test**

```rust
#[test]
fn test_regression_detection() {
    let oxidex_tags = vec![
        TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
        // Model is now missing
    ];
    let exiftool_tags = vec![
        TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string()),
        TagInfo::new("Model".to_string(), "EXIF".to_string(), "5D".to_string()),
    ];

    // Previous baseline had both tags
    let mut previous = FormatComparison::new("JPEG".to_string(), 2);
    previous.matched_tags = vec!["EXIF:Make".to_string(), "EXIF:Model".to_string()];

    let result = ComparisonEngine::compare(oxidex_tags, exiftool_tags, "JPEG", Some(&previous));

    assert_eq!(result.regressions.len(), 1);
    assert!(result.regressions.contains(&"EXIF:Model".to_string()));
}
```

**Step 4: Run tests**

```bash
cargo test --bin tag-comparison
```

**Step 5: Commit**

```bash
git add src/bin/tag-comparison/comparison/
git commit -m "feat(tag-comparison): add regression detection against baseline"
```

---

## Task 7: Add Markdown Report Generation

**Depends on:** Task 5, Task 6

**Files:**
- Create: `src/bin/tag-comparison/comparison/markdown.rs`
- Modify: `src/bin/tag-comparison/comparison/mod.rs`

**Step 1: Create markdown.rs**

Create `src/bin/tag-comparison/comparison/markdown.rs`:

```rust
//! Markdown report generation

use crate::models::{ComparisonReport, FormatComparison};
use std::path::Path;
use std::io::Write;

/// Generate all markdown reports
pub fn generate_markdown_reports(
    report: &ComparisonReport,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(output_dir)?;

    // Generate index page
    generate_index(report, output_dir)?;

    // Generate per-format pages
    for (format, comparison) in &report.by_format {
        generate_format_page(format, comparison, report, output_dir)?;
    }

    Ok(())
}

fn generate_index(
    report: &ComparisonReport,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut content = String::new();

    content.push_str("---\n");
    content.push_str("title: ExifTool Compatibility Report\n");
    content.push_str("---\n\n");

    content.push_str("# ExifTool Compatibility Report\n\n");

    content.push_str(&format!(
        "**Generated:** {} | **ExifTool:** v{} | **OxiDex:** v{}\n\n",
        &report.generated_at[..10], // Just date
        report.exiftool_version,
        report.oxidex_version
    ));

    // Overall stats
    content.push_str(&format!(
        "**Overall Coverage:** {:.1}%",
        report.overall_coverage
    ));

    if report.total_regressions > 0 {
        content.push_str(&format!(" | **⚠️ Regressions:** {}", report.total_regressions));
    }
    content.push_str("\n\n");

    // Summary table
    content.push_str("## Coverage by Format\n\n");
    content.push_str("| Format | Files | Coverage | Missing | Extra | Value Diffs | Regressions |\n");
    content.push_str("|--------|-------|----------|---------|-------|-------------|-------------|\n");

    let mut formats: Vec<_> = report.by_format.iter().collect();
    formats.sort_by(|a, b| a.0.cmp(b.0));

    for (format, comp) in formats {
        let regression_cell = if comp.regressions.is_empty() {
            "0".to_string()
        } else {
            format!("⚠️ {}", comp.regressions.len())
        };

        content.push_str(&format!(
            "| [{}](./{}.md) | {} | {:.1}% | {} | {} | {} | {} |\n",
            format,
            format.to_lowercase(),
            comp.files_tested,
            comp.coverage_percentage,
            comp.missing_in_oxidex.len(),
            comp.extra_in_oxidex.len(),
            comp.value_differences.len(),
            regression_cell
        ));
    }

    content.push_str("\n---\n\n");
    content.push_str("*Auto-generated by [compare-exiftool.yml](https://github.com/swack-tools/oxidex/blob/main/.github/workflows/compare-exiftool.yml)*\n");

    let path = output_dir.join("index.md");
    let mut file = std::fs::File::create(&path)?;
    file.write_all(content.as_bytes())?;

    println!("Generated: {}", path.display());
    Ok(())
}

fn generate_format_page(
    format: &str,
    comparison: &FormatComparison,
    report: &ComparisonReport,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut content = String::new();

    content.push_str("---\n");
    content.push_str(&format!("title: {} Compatibility\n", format));
    content.push_str("---\n\n");

    content.push_str(&format!("# {} Compatibility Report\n\n", format));

    content.push_str(&format!(
        "**Generated:** {} | **ExifTool:** v{} | **OxiDex:** v{}\n\n",
        &report.generated_at[..10],
        report.exiftool_version,
        report.oxidex_version
    ));

    // Stats summary
    content.push_str("## Summary\n\n");
    content.push_str(&format!("- **Files Tested:** {}\n", comparison.files_tested));
    content.push_str(&format!("- **Coverage:** {:.1}%\n", comparison.coverage_percentage));
    content.push_str(&format!("- **Matched Tags:** {}\n", comparison.matched_tags.len()));
    content.push_str(&format!("- **Missing Tags:** {}\n", comparison.missing_in_oxidex.len()));
    content.push_str(&format!("- **Extra Tags:** {}\n", comparison.extra_in_oxidex.len()));
    content.push_str(&format!("- **Value Differences:** {}\n", comparison.value_differences.len()));

    if !comparison.regressions.is_empty() {
        content.push_str(&format!("- **⚠️ Regressions:** {}\n", comparison.regressions.len()));
    }
    content.push_str("\n");

    // Regressions (most important, show first)
    if !comparison.regressions.is_empty() {
        content.push_str("## ⚠️ Regressions\n\n");
        content.push_str("Tags that OxiDex previously extracted but no longer does:\n\n");
        content.push_str("| Tag |\n");
        content.push_str("|-----|\n");
        for tag in &comparison.regressions {
            content.push_str(&format!("| `{}` |\n", tag));
        }
        content.push_str("\n");
    }

    // Value differences
    if !comparison.value_differences.is_empty() {
        content.push_str("## Value Differences\n\n");
        content.push_str("Tags where ExifTool and OxiDex extract different values:\n\n");
        content.push_str("| Tag | ExifTool | OxiDex |\n");
        content.push_str("|-----|----------|--------|\n");
        for diff in comparison.value_differences.iter().take(50) {
            let et_val = truncate(&diff.exiftool_value, 40);
            let ox_val = truncate(&diff.oxidex_value, 40);
            content.push_str(&format!("| `{}` | {} | {} |\n", diff.tag_key, et_val, ox_val));
        }
        if comparison.value_differences.len() > 50 {
            content.push_str(&format!(
                "\n*...and {} more differences*\n",
                comparison.value_differences.len() - 50
            ));
        }
        content.push_str("\n");
    }

    // Missing tags
    if !comparison.missing_in_oxidex.is_empty() {
        content.push_str("## Missing Tags\n\n");
        content.push_str("Tags ExifTool extracts that OxiDex doesn't:\n\n");
        content.push_str("| Tag | Sample Value |\n");
        content.push_str("|-----|-------------|\n");
        for tag in comparison.missing_in_oxidex.iter().take(100) {
            let val = truncate(&tag.value, 50);
            content.push_str(&format!("| `{}:{}` | {} |\n", tag.family, tag.name, val));
        }
        if comparison.missing_in_oxidex.len() > 100 {
            content.push_str(&format!(
                "\n*...and {} more missing tags*\n",
                comparison.missing_in_oxidex.len() - 100
            ));
        }
        content.push_str("\n");
    }

    // Extra tags
    if !comparison.extra_in_oxidex.is_empty() {
        content.push_str("## Extra Tags\n\n");
        content.push_str("Tags OxiDex extracts that ExifTool doesn't:\n\n");
        content.push_str("| Tag | Value |\n");
        content.push_str("|-----|-------|\n");
        for tag in comparison.extra_in_oxidex.iter().take(50) {
            let val = truncate(&tag.value, 50);
            content.push_str(&format!("| `{}:{}` | {} |\n", tag.family, tag.name, val));
        }
        if comparison.extra_in_oxidex.len() > 50 {
            content.push_str(&format!(
                "\n*...and {} more extra tags*\n",
                comparison.extra_in_oxidex.len() - 50
            ));
        }
        content.push_str("\n");
    }

    content.push_str("---\n\n");
    content.push_str("[← Back to Overview](./)\n");

    let path = output_dir.join(format!("{}.md", format.to_lowercase()));
    let mut file = std::fs::File::create(&path)?;
    file.write_all(content.as_bytes())?;

    println!("Generated: {}", path.display());
    Ok(())
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.replace('|', "\\|").replace('\n', " ")
    } else {
        format!("{}...", &s[..max_len].replace('|', "\\|").replace('\n', " "))
    }
}
```

**Step 2: Update comparison/mod.rs**

```rust
//! Tag comparison engine

pub mod engine;
pub mod markdown;

pub use engine::ComparisonEngine;
pub use markdown::generate_markdown_reports;
```

**Step 3: Run build**

```bash
cargo build --release --bin tag-comparison
```

**Step 4: Commit**

```bash
git add src/bin/tag-comparison/comparison/
git commit -m "feat(tag-comparison): add markdown report generation"
```

---

## Task 8: Integration Test and Verification

**Depends on:** All previous tasks

**Files:**
- All modified files

**Step 1: Build everything**

```bash
cargo build --release
cargo build --release --bin tag-comparison
```

**Step 2: Run unit tests**

```bash
cargo test --bin tag-comparison
```

**Step 3: Run a local test (if exiftool installed)**

```bash
# Skip if exiftool not installed locally
which exiftool && ./target/release/tag-comparison \
  --samples tests/fixtures \
  --output /tmp/comparison-test \
  --exiftool-version "test" \
  --oxidex-version "test"
```

**Step 4: Verify workflow syntax**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/compare-exiftool.yml'))"
```

**Step 5: Check VitePress builds**

```bash
cd docs && npm run docs:build
```

**Step 6: Commit all remaining changes**

```bash
git add .
git status
# If there are uncommitted changes:
git commit -m "feat: complete ExifTool comparison report system"
```

**Step 7: Push and verify workflow triggers**

```bash
git push origin main
```

Monitor the GitHub Actions to ensure the workflow runs successfully.

---

## Verification Checklist

- [ ] `cargo build --release --bin tag-comparison` succeeds
- [ ] `cargo test --bin tag-comparison` passes
- [ ] Workflow YAML is valid
- [ ] VitePress docs build
- [ ] Workflow runs on push to main with parser changes
- [ ] Reports are generated and committed
- [ ] Docs site shows comparison page
