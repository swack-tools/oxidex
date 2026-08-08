//! Markdown report generation

use crate::models::{ComparisonReport, FormatComparison};
use std::io::Write;
use std::path::Path;

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

    // Overall stats. The headline is the per-(file, tag) figure: of every tag
    // ExifTool read out of these files, the share oxidex read the same way.
    content.push_str(&format!(
        "**Overall Coverage:** {:.1}%",
        report.overall_instance_coverage
    ));

    if report.total_regressions > 0 {
        content.push_str(&format!(
            " | **⚠️ Regressions:** {}",
            report.total_regressions
        ));
    }
    content.push_str("\n\n");

    content.push_str(&format!(
        "::: warning What this number is, and what it is not\n\
         **Overall Coverage** is measured per (file, tag): every tag ExifTool \
         extracted from every sample file counts once, and it counts as \
         covered only when OxiDex extracted the same tag from *that same file* \
         with the same value. Across this corpus that is \
         **{matched} of {total} tag instances**.\n\n\
         It is deliberately **not** the share of distinct tag *names* the \
         corpus contains, which for the same run is {name_pct:.1}% \
         ({name_matched} of {name_total} keys). That figure deduplicates both \
         sides across the whole corpus, so a tag ExifTool reads from 4,000 \
         files counts as covered when OxiDex reads it from one — possibly not \
         even the same one. On ExifTool's own `t/images` the two differ by 22 \
         points. The per-name column is kept below as a breadth signal; it is \
         an inventory of names, not a measure of extraction.\n\
         :::\n\n",
        matched = report
            .by_format
            .values()
            .map(|c| c.matched_instances)
            .sum::<usize>(),
        total = report
            .by_format
            .values()
            .map(|c| c.total_exiftool_instances)
            .sum::<usize>(),
        name_pct = report.overall_coverage,
        name_matched = report
            .by_format
            .values()
            .map(|c| c.matched_tags.len())
            .sum::<usize>(),
        name_total = report
            .by_format
            .values()
            .map(|c| c.total_exiftool_tags)
            .sum::<usize>(),
    ));

    if !report.unmeasurable_formats.is_empty() {
        content.push_str(&format!(
            "::: info {} format(s) not measurable\n\
             Every tag ExifTool emitted for {} fell in a pseudo-family this \
             harness skips (`File`, `System`, `Composite`, `ExifTool`), so \
             there was nothing to compare. These are excluded from **Overall \
             Coverage** and listed as `n/a` below rather than scored 0% — an \
             absent measurement is not a measured zero.\n\
             :::\n\n",
            report.unmeasurable_formats.len(),
            report.unmeasurable_formats.join(", "),
        ));
    }

    content.push_str(
        "::: tip Empirical JPEG tag matrix\n\
         These reports sample real fixture files per format. For JPEG, a \
         complete per-tag read/write matrix against ExifTool (every writable \
         tag, with a CI regression gate) is also available: \
         [JPEG Tag Support](/reference/jpeg-tag-support) · \
         [JPEG Tag Matrix](/reference/jpeg-tag-matrix)\n\
         :::\n\n",
    );

    // Summary table for tested formats
    content.push_str("## Coverage by Format\n\n");
    content.push_str(
        "`Coverage` is per (file, tag) — matched instances over ExifTool \
         instances. `Names Seen` is the distinct-key inventory described \
         above, shown for breadth only. `Missing`, `Extra` and `Value Diffs` \
         count distinct keys.\n\n",
    );
    content
        .push_str("| Format | Files | Tag Instances | Coverage | Names Seen | Missing | Extra | Value Diffs | Regressions |\n");
    content
        .push_str("|--------|-------|---------------|----------|------------|---------|-------|-------------|-------------|\n");

    let mut formats: Vec<_> = report.by_format.iter().collect();
    formats.sort_by(|a, b| a.0.cmp(b.0));

    // Track which formats we've listed
    let mut listed_formats: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (format, comp) in formats {
        let regression_cell = if comp.regressions.is_empty() {
            "0".to_string()
        } else {
            format!("⚠️ {}", comp.regressions.len())
        };

        // `n/a`, never `0.0%`: a format with no comparable ExifTool tags was
        // not measured, and printing a zero states a result no run produced.
        let coverage_cell = if comp.is_measurable() {
            format!("{:.1}%", comp.instance_coverage_percentage)
        } else {
            "n/a".to_string()
        };
        let names_cell = if comp.total_exiftool_tags == 0 {
            "n/a".to_string()
        } else {
            format!(
                "{}/{} ({:.0}%)",
                comp.matched_tags.len(),
                comp.total_exiftool_tags,
                comp.coverage_percentage
            )
        };

        content.push_str(&format!(
            "| [{}](./{}.md) | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            format,
            format.to_lowercase(),
            comp.files_tested,
            comp.total_exiftool_instances,
            coverage_cell,
            names_cell,
            comp.missing_in_oxidex.len(),
            comp.extra_in_oxidex.len(),
            comp.value_differences.len(),
            regression_cell
        ));
        listed_formats.insert(format.to_lowercase());
    }

    // Scan for other existing .md files in the output directory
    let mut other_formats: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(output_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "md")
                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
            {
                // Skip index, baseline, and already-listed formats
                if stem != "index"
                    && !stem.contains("baseline")
                    && !listed_formats.contains(&stem.to_lowercase())
                {
                    other_formats.push(stem.to_string());
                }
            }
        }
    }

    // Add section for other available format pages
    if !other_formats.is_empty() {
        other_formats.sort();
        content.push_str("\n## Other Format Reports\n\n");
        content.push_str("Additional format-specific reports:\n\n");
        for format in &other_formats {
            // Capitalize first letter for display
            let display_name = format
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string() + &format[1..])
                .unwrap_or_else(|| format.clone());
            content.push_str(&format!("- [{}](./{}.md)\n", display_name, format));
        }
        content.push('\n');
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
    content.push_str(&format!(
        "- **Files Tested:** {}\n",
        comparison.files_tested
    ));
    if comparison.is_measurable() {
        content.push_str(&format!(
            "- **Coverage (per file+tag):** {:.1}% — {} of {} tag instances\n",
            comparison.instance_coverage_percentage,
            comparison.matched_instances,
            comparison.total_exiftool_instances,
        ));
    } else {
        content.push_str(
            "- **Coverage (per file+tag):** n/a — ExifTool emitted no \
             comparable tags for this format (all fell in skipped \
             pseudo-families), so there was nothing to measure\n",
        );
    }
    content.push_str(&format!(
        "- **Distinct tag names seen:** {} of {} ({:.1}%) — an inventory of \
         names across the corpus, not extraction coverage\n",
        comparison.matched_tags.len(),
        comparison.total_exiftool_tags,
        comparison.coverage_percentage,
    ));
    content.push_str(&format!(
        "- **Matched Tags:** {}\n",
        comparison.matched_tags.len()
    ));
    content.push_str(&format!(
        "- **Missing Tags:** {}\n",
        comparison.missing_in_oxidex.len()
    ));
    content.push_str(&format!(
        "- **Extra Tags:** {}\n",
        comparison.extra_in_oxidex.len()
    ));
    content.push_str(&format!(
        "- **Value Differences:** {}\n",
        comparison.value_differences.len()
    ));

    if !comparison.regressions.is_empty() {
        content.push_str(&format!(
            "- **⚠️ Regressions:** {}\n",
            comparison.regressions.len()
        ));
    }
    content.push('\n');

    // Regressions (most important, show first)
    if !comparison.regressions.is_empty() {
        content.push_str("## ⚠️ Regressions\n\n");
        content.push_str("Tags that OxiDex previously extracted but no longer does:\n\n");
        content.push_str("| Tag |\n");
        content.push_str("|-----|\n");
        for tag in &comparison.regressions {
            content.push_str(&format!("| `{}` |\n", tag));
        }
        content.push('\n');
    }

    // Value differences
    if !comparison.value_differences.is_empty() {
        content.push_str("## Value Differences\n\n");
        content.push_str("Tags where ExifTool and OxiDex extract different values:\n\n");
        content.push_str("| Tag | ExifTool | OxiDex |\n");
        content.push_str("|-----|----------|--------|\n");
        for diff in &comparison.value_differences {
            let et_val = truncate(&diff.exiftool_value, 40);
            let ox_val = truncate(&diff.oxidex_value, 40);
            content.push_str(&format!(
                "| `{}` | {} | {} |\n",
                diff.tag_key, et_val, ox_val
            ));
        }
        content.push('\n');
    }

    // Matched Tags
    if !comparison.matched_tags.is_empty() {
        content.push_str("## Matched Tags\n\n");
        content.push_str(
            "Tags where OxiDex matches ExifTool exactly (or matches expected format):\n\n",
        );
        content.push_str("| Tag |\n");
        content.push_str("|-----|\n");
        // Sort tags for better readability
        let mut matched = comparison.matched_tags.clone();
        matched.sort();
        for tag in matched {
            content.push_str(&format!("| `{}` |\n", tag));
        }
        content.push('\n');
    }

    // Missing tags
    if !comparison.missing_in_oxidex.is_empty() {
        content.push_str("## Missing Tags\n\n");
        content.push_str("Tags ExifTool extracts that OxiDex doesn't:\n\n");
        content.push_str("| Tag | Sample Value |\n");
        content.push_str("|-----|-------------|\n");
        for tag in &comparison.missing_in_oxidex {
            let val = truncate(&tag.value, 50);
            content.push_str(&format!("| `{}:{}` | {} |\n", tag.family, tag.name, val));
        }
        content.push('\n');
    }

    // Extra tags
    if !comparison.extra_in_oxidex.is_empty() {
        content.push_str("## Extra Tags\n\n");
        content.push_str("Tags OxiDex extracts that ExifTool doesn't:\n\n");
        content.push_str("| Tag | Value |\n");
        content.push_str("|-----|-------|\n");
        for tag in &comparison.extra_in_oxidex {
            let val = truncate(&tag.value, 50);
            content.push_str(&format!("| `{}:{}` | {} |\n", tag.family, tag.name, val));
        }
        content.push('\n');
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
    // Sanitize the string first - replace control chars and non-printable bytes
    let sanitized: String = s
        .chars()
        .filter(|c| !c.is_control() || *c == ' ')
        .map(|c| match c {
            '|' => '¦', // Pipe breaks markdown tables
            '<' => '‹', // Less-than interpreted as HTML tag by VitePress
            '>' => '›', // Greater-than interpreted as HTML tag by VitePress
            _ => c,
        })
        .collect();

    // Truncate by character count, not byte count
    if sanitized.chars().count() <= max_len {
        sanitized.replace('\n', " ")
    } else {
        let truncated: String = sanitized.chars().take(max_len).collect();
        format!("{}...", truncated.replace('\n', " "))
    }
}
