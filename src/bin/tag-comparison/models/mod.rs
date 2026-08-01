//! Data models for tag comparison and reporting

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Information about a single metadata tag
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct TagInfo {
    /// Tag name (e.g., "Make", "Model", "DateTime")
    pub name: String,
    /// Tag family (e.g., "EXIF", "XMP", "IPTC", "MakerNote")
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

    /// Set the tag ID
    #[allow(dead_code)]
    pub fn with_tag_id(mut self, tag_id: String) -> Self {
        self.tag_id = Some(tag_id);
        self
    }

    /// Set the source file
    #[allow(dead_code)]
    pub fn with_source_file(mut self, source_file: String) -> Self {
        self.source_file = Some(source_file);
        self
    }
}

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

/// One ranked, actionable item in the cross-format parity queue.
///
/// The comparison engine deliberately keeps per-format evidence intact. The
/// queue groups the same missing/value-different key across formats so a
/// shared parser or conversion fix rises above one-off residue.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GapQueueItem {
    /// `missing`, `value`, or `extra`.
    pub kind: String,
    /// Normalized family:name key.
    pub tag_key: String,
    /// Formats in which this gap occurs, sorted and deduplicated.
    pub formats: Vec<String>,
    /// Number of format reports contributing evidence.
    pub occurrences: usize,
    /// Representative values from the oracle/OxiDex side.
    #[serde(default)]
    pub examples: Vec<String>,
    /// Higher values should be investigated first.
    pub priority: usize,
}

/// Comparison results for a single file format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatComparison {
    /// Format name (e.g., "JPEG", "PNG", "TIFF")
    pub format: String,
    /// Number of test files processed
    pub files_tested: usize,
    /// List of matched tag names
    pub matched_tags: Vec<String>,
    /// Tags found in ExifTool but missing in OxiDex
    pub missing_in_oxidex: Vec<TagInfo>,
    /// Tags found in OxiDex but not in ExifTool
    pub extra_in_oxidex: Vec<TagInfo>,
    /// Tags with different values
    pub value_differences: Vec<ValueDifference>,
    /// Tags that were present in baseline but now missing (regressions)
    pub regressions: Vec<String>,
    /// Tag keys ("family:name") the oxidex side emits more than once for
    /// the SAME sample file (spec M3 double-emission gate). Defaults to
    /// empty for any report serialized before this field existed.
    #[serde(default)]
    pub duplicate_emissions: Vec<String>,
    /// Coverage percentage (matched / total_exiftool)
    pub coverage_percentage: f64,
    /// Total number of tags in ExifTool for this format
    pub total_exiftool_tags: usize,
    /// Timestamp when this comparison was generated
    pub timestamp: String,
}

impl FormatComparison {
    /// Create a new FormatComparison result
    pub fn new(format: String, files_tested: usize) -> Self {
        Self {
            format,
            files_tested,
            matched_tags: Vec::new(),
            missing_in_oxidex: Vec::new(),
            extra_in_oxidex: Vec::new(),
            value_differences: Vec::new(),
            regressions: Vec::new(),
            duplicate_emissions: Vec::new(),
            coverage_percentage: 0.0,
            total_exiftool_tags: 0,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Calculate coverage percentage
    pub fn calculate_coverage(&mut self) {
        if self.total_exiftool_tags == 0 {
            self.coverage_percentage = 0.0;
        } else {
            self.coverage_percentage =
                (self.matched_tags.len() as f64 / self.total_exiftool_tags as f64) * 100.0;
        }
    }

    /// Get summary statistics
    pub fn summary(&self) -> String {
        format!(
            "{}: {}/{} tags ({:.1}% coverage)",
            self.format,
            self.matched_tags.len(),
            self.total_exiftool_tags,
            self.coverage_percentage
        )
    }
}

/// Complete comparison report for all formats
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComparisonReport {
    /// When this report was generated
    pub generated_at: String,
    /// ExifTool version used for comparison
    pub exiftool_version: String,
    /// OxiDex version used for comparison
    pub oxidex_version: String,
    /// Comparisons indexed by format name
    pub by_format: BTreeMap<String, FormatComparison>,
    /// Overall coverage across all formats
    pub overall_coverage: f64,
    /// Total regressions across all formats
    pub total_regressions: usize,
    /// Summary text
    pub summary: String,
    /// Cross-format ranked queue used to choose the next shared fix.
    #[serde(default)]
    pub gap_queue: Vec<GapQueueItem>,
}

impl ComparisonReport {
    /// Create a new empty report
    pub fn new() -> Self {
        Self {
            generated_at: chrono::Utc::now().to_rfc3339(),
            exiftool_version: String::new(),
            oxidex_version: String::new(),
            by_format: BTreeMap::new(),
            overall_coverage: 0.0,
            total_regressions: 0,
            summary: String::new(),
            gap_queue: Vec::new(),
        }
    }

    /// Add a format comparison to the report
    pub fn add_format(&mut self, format: String, comparison: FormatComparison) {
        self.by_format.insert(format, comparison);
    }

    /// Calculate overall coverage across all formats
    pub fn calculate_overall_coverage(&mut self) {
        if self.by_format.is_empty() {
            self.overall_coverage = 0.0;
            self.total_regressions = 0;
            self.summary = "No formats analyzed".to_string();
            self.gap_queue.clear();
            return;
        }

        let total_matched: usize = self.by_format.values().map(|c| c.matched_tags.len()).sum();

        let total_tags: usize = self.by_format.values().map(|c| c.total_exiftool_tags).sum();

        self.total_regressions = self.by_format.values().map(|c| c.regressions.len()).sum();

        if total_tags == 0 {
            self.overall_coverage = 0.0;
        } else {
            self.overall_coverage = (total_matched as f64 / total_tags as f64) * 100.0;
        }

        let format_count = self.by_format.len();
        self.summary = format!(
            "Analyzed {} formats: {}/{} tags ({:.1}% overall coverage)",
            format_count, total_matched, total_tags, self.overall_coverage
        );
        self.gap_queue = build_gap_queue(&self.by_format);
    }

    /// Get format names in sorted order
    #[allow(dead_code)]
    pub fn format_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self.by_format.keys().cloned().collect();
        names.sort();
        names
    }
}

fn build_gap_queue(comparisons: &BTreeMap<String, FormatComparison>) -> Vec<GapQueueItem> {
    #[derive(Default)]
    struct Accumulator {
        formats: Vec<String>,
        examples: Vec<String>,
    }

    let mut grouped: HashMap<(String, String), Accumulator> = HashMap::new();
    for (format, comparison) in comparisons {
        for tag in &comparison.missing_in_oxidex {
            let key = tag.key();
            let entry = grouped.entry(("missing".to_string(), key)).or_default();
            entry.formats.push(format.clone());
            entry.examples.push(format!("{}: {}", format, tag.value));
        }
        for difference in &comparison.value_differences {
            let entry = grouped
                .entry(("value".to_string(), difference.tag_key.clone()))
                .or_default();
            entry.formats.push(format.clone());
            entry.examples.push(format!(
                "{}: ExifTool={:?}; OxiDex={:?}",
                format, difference.exiftool_value, difference.oxidex_value
            ));
        }
        for tag in &comparison.extra_in_oxidex {
            let key = tag.key();
            let entry = grouped.entry(("extra".to_string(), key)).or_default();
            entry.formats.push(format.clone());
            entry.examples.push(format!("{}: {}", format, tag.value));
        }
    }

    let mut queue: Vec<_> = grouped
        .into_iter()
        .map(|((kind, tag_key), mut accumulator)| {
            accumulator.formats.sort();
            accumulator.formats.dedup();
            accumulator.examples.sort();
            accumulator.examples.dedup();
            accumulator.examples.truncate(3);
            let occurrences = accumulator.formats.len();
            let base = match kind.as_str() {
                "missing" => 100,
                "value" => 80,
                _ => 20,
            };
            GapQueueItem {
                kind,
                tag_key,
                formats: accumulator.formats,
                occurrences,
                examples: accumulator.examples,
                priority: base + occurrences.saturating_mul(10),
            }
        })
        .collect();
    queue.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.kind.cmp(&b.kind))
            .then_with(|| a.tag_key.cmp(&b.tag_key))
    });
    queue
}

impl Default for ComparisonReport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_info_creation() {
        let tag = TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string());
        assert_eq!(tag.name, "Make");
        assert_eq!(tag.family, "EXIF");
        assert_eq!(tag.value, "Canon");
        assert_eq!(tag.tag_id, None);
        assert_eq!(tag.source_file, None);
    }

    #[test]
    fn test_tag_info_key() {
        let tag = TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string());
        assert_eq!(tag.key(), "EXIF:Make");
    }

    #[test]
    fn test_tag_info_with_builder() {
        let tag = TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string())
            .with_tag_id("0x010F".to_string())
            .with_source_file("test.jpg".to_string());

        assert_eq!(tag.name, "Make");
        assert_eq!(tag.value, "Canon");
        assert_eq!(tag.tag_id, Some("0x010F".to_string()));
        assert_eq!(tag.source_file, Some("test.jpg".to_string()));
    }

    #[test]
    fn test_format_comparison_creation() {
        let comp = FormatComparison::new("JPEG".to_string(), 5);
        assert_eq!(comp.format, "JPEG");
        assert_eq!(comp.files_tested, 5);
        assert_eq!(comp.total_exiftool_tags, 0);
        assert_eq!(comp.matched_tags.len(), 0);
        assert_eq!(comp.value_differences.len(), 0);
        assert_eq!(comp.regressions.len(), 0);
        assert_eq!(comp.coverage_percentage, 0.0);
    }

    #[test]
    fn test_format_comparison_coverage_calculation() {
        let mut comp = FormatComparison::new("JPEG".to_string(), 5);
        comp.total_exiftool_tags = 100;
        comp.matched_tags = vec![
            "Make".to_string(),
            "Model".to_string(),
            "DateTime".to_string(),
        ];
        comp.calculate_coverage();

        assert_eq!(comp.coverage_percentage, 3.0); // 3/100 = 3%
    }

    #[test]
    fn test_format_comparison_coverage_zero_tags() {
        let mut comp = FormatComparison::new("JPEG".to_string(), 5);
        comp.total_exiftool_tags = 0;
        comp.calculate_coverage();
        assert_eq!(comp.coverage_percentage, 0.0);
    }

    #[test]
    fn test_format_comparison_summary() {
        let mut comp = FormatComparison::new("JPEG".to_string(), 5);
        comp.total_exiftool_tags = 100;
        comp.matched_tags = vec!["Make".to_string(), "Model".to_string()];
        comp.calculate_coverage();

        let summary = comp.summary();
        assert!(summary.contains("JPEG"));
        assert!(summary.contains("2/100"));
        assert!(summary.contains("2.0% coverage"));
    }

    #[test]
    fn test_comparison_report_creation() {
        let report = ComparisonReport::new();
        assert_eq!(report.by_format.len(), 0);
        assert_eq!(report.overall_coverage, 0.0);
        assert_eq!(report.total_regressions, 0);
        assert_eq!(report.exiftool_version, "");
        assert_eq!(report.oxidex_version, "");
    }

    #[test]
    fn test_comparison_report_add_format() {
        let mut report = ComparisonReport::new();
        let comp = FormatComparison::new("JPEG".to_string(), 5);
        report.add_format("JPEG".to_string(), comp);

        assert_eq!(report.by_format.len(), 1);
        assert!(report.by_format.contains_key("JPEG"));
    }

    #[test]
    fn test_comparison_report_overall_coverage_single_format() {
        let mut report = ComparisonReport::new();
        let mut comp = FormatComparison::new("JPEG".to_string(), 5);
        comp.total_exiftool_tags = 100;
        comp.matched_tags = (0..50).map(|i| format!("Tag{}", i)).collect();
        comp.calculate_coverage();
        report.add_format("JPEG".to_string(), comp);
        report.calculate_overall_coverage();

        assert_eq!(report.overall_coverage, 50.0); // 50/100 = 50%
        assert!(report.summary.contains("50.0%"));
    }

    #[test]
    fn test_comparison_report_overall_coverage_multiple_formats() {
        let mut report = ComparisonReport::new();

        // Format 1: 50/100 = 50%
        let mut comp1 = FormatComparison::new("JPEG".to_string(), 5);
        comp1.total_exiftool_tags = 100;
        comp1.matched_tags = (0..50).map(|i| format!("Tag{}", i)).collect();
        comp1.calculate_coverage();

        // Format 2: 30/50 = 60%
        let mut comp2 = FormatComparison::new("PNG".to_string(), 3);
        comp2.total_exiftool_tags = 50;
        comp2.matched_tags = (0..30).map(|i| format!("Tag{}", i)).collect();
        comp2.calculate_coverage();

        report.add_format("JPEG".to_string(), comp1);
        report.add_format("PNG".to_string(), comp2);
        report.calculate_overall_coverage();

        // Total: 80/150 = 53.33%
        assert!((report.overall_coverage - 53.33).abs() < 0.1);
        assert!(report.summary.contains("2 formats"));
    }

    #[test]
    fn test_comparison_report_format_names() {
        let mut report = ComparisonReport::new();
        report.add_format(
            "PNG".to_string(),
            FormatComparison::new("PNG".to_string(), 3),
        );
        report.add_format(
            "JPEG".to_string(),
            FormatComparison::new("JPEG".to_string(), 5),
        );
        report.add_format(
            "TIFF".to_string(),
            FormatComparison::new("TIFF".to_string(), 2),
        );

        let names = report.format_names();
        assert_eq!(names, vec!["JPEG", "PNG", "TIFF"]); // Sorted order
    }

    #[test]
    fn test_comparison_report_empty_summary() {
        let mut report = ComparisonReport::new();
        report.calculate_overall_coverage();
        assert!(report.summary.contains("No formats"));
    }

    #[test]
    fn ranked_gap_queue_groups_shared_gaps() {
        let mut report = ComparisonReport::new();

        let mut jpeg = FormatComparison::new("JPEG".to_string(), 1);
        jpeg.missing_in_oxidex.push(TagInfo::new(
            "LensModel".to_string(),
            "EXIF".to_string(),
            "EF 50mm".to_string(),
        ));
        jpeg.value_differences.push(ValueDifference {
            tag_key: "EXIF:ISO".to_string(),
            exiftool_value: "400".to_string(),
            oxidex_value: "0400".to_string(),
            source_file: "a.jpg".to_string(),
        });

        let mut tiff = FormatComparison::new("TIFF".to_string(), 1);
        tiff.missing_in_oxidex.push(TagInfo::new(
            "LensModel".to_string(),
            "EXIF".to_string(),
            "EF 50mm".to_string(),
        ));
        report.add_format("TIFF".to_string(), tiff);
        report.add_format("JPEG".to_string(), jpeg);
        report.calculate_overall_coverage();

        assert_eq!(report.gap_queue[0].kind, "missing");
        assert_eq!(report.gap_queue[0].tag_key, "EXIF:LensModel");
        assert_eq!(report.gap_queue[0].formats, vec!["JPEG", "TIFF"]);
        assert_eq!(report.gap_queue[0].occurrences, 2);
        assert!(report.gap_queue[0].priority > report.gap_queue[1].priority);
    }

    #[test]
    fn ranked_gap_queue_is_backward_compatible() {
        let json = r#"{
            "generated_at": "2026-08-01T00:00:00Z",
            "exiftool_version": "13.30",
            "oxidex_version": "0.1.0",
            "by_format": {},
            "overall_coverage": 0.0,
            "total_regressions": 0,
            "summary": "No formats analyzed"
        }"#;
        let report: ComparisonReport = serde_json::from_str(json).unwrap();
        assert!(report.gap_queue.is_empty());
    }

    #[test]
    fn test_tag_info_serialization() {
        let tag = TagInfo::new("Make".to_string(), "EXIF".to_string(), "Canon".to_string());
        let json = serde_json::to_string(&tag).unwrap();
        let deserialized: TagInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(tag, deserialized);
    }

    #[test]
    fn test_format_comparison_serialization() {
        let mut comp = FormatComparison::new("JPEG".to_string(), 5);
        comp.total_exiftool_tags = 100;
        comp.matched_tags = vec!["Make".to_string(), "Model".to_string()];
        comp.calculate_coverage();

        let json = serde_json::to_string(&comp).unwrap();
        let deserialized: FormatComparison = serde_json::from_str(&json).unwrap();
        assert_eq!(comp.format, deserialized.format);
        assert_eq!(comp.matched_tags, deserialized.matched_tags);
    }

    #[test]
    fn test_duplicate_emissions_defaults_empty_and_serializes() {
        let mut comp = FormatComparison::new("JPEG".to_string(), 5);
        assert!(comp.duplicate_emissions.is_empty());

        comp.duplicate_emissions = vec!["MakerNotes:AELButton".to_string()];
        let json = serde_json::to_string(&comp).unwrap();
        let deserialized: FormatComparison = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.duplicate_emissions, comp.duplicate_emissions);
    }

    #[test]
    fn test_duplicate_emissions_defaults_when_absent_from_json() {
        // Spec M3: #[serde(default)] -- a report serialized before this
        // field existed must still deserialize.
        let json = r#"{
            "format": "JPEG",
            "files_tested": 1,
            "matched_tags": [],
            "missing_in_oxidex": [],
            "extra_in_oxidex": [],
            "value_differences": [],
            "regressions": [],
            "coverage_percentage": 0.0,
            "total_exiftool_tags": 0,
            "timestamp": "2026-07-24T00:00:00Z"
        }"#;
        let deserialized: FormatComparison = serde_json::from_str(json).unwrap();
        assert!(deserialized.duplicate_emissions.is_empty());
    }

    #[test]
    fn test_value_difference() {
        let diff = ValueDifference {
            tag_key: "EXIF:Make".to_string(),
            exiftool_value: "Canon".to_string(),
            oxidex_value: "CANON".to_string(),
            source_file: "test.jpg".to_string(),
        };
        assert_eq!(diff.tag_key, "EXIF:Make");
        assert_eq!(diff.exiftool_value, "Canon");
        assert_eq!(diff.oxidex_value, "CANON");
    }

    #[test]
    fn test_comparison_report_with_regressions() {
        let mut report = ComparisonReport::new();

        let mut comp1 = FormatComparison::new("JPEG".to_string(), 5);
        comp1.total_exiftool_tags = 100;
        comp1.matched_tags = vec!["EXIF:Make".to_string()];
        comp1.regressions = vec!["EXIF:Model".to_string(), "EXIF:DateTime".to_string()];

        let mut comp2 = FormatComparison::new("PNG".to_string(), 3);
        comp2.total_exiftool_tags = 50;
        comp2.matched_tags = vec!["PNG:Width".to_string()];
        comp2.regressions = vec!["PNG:Height".to_string()];

        report.add_format("JPEG".to_string(), comp1);
        report.add_format("PNG".to_string(), comp2);
        report.calculate_overall_coverage();

        assert_eq!(report.total_regressions, 3); // 2 + 1
    }
}
