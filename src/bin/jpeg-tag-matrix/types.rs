//! Shared JSON schema for the JPEG tag matrix pipeline (manifest -> run -> report).
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestTag {
    pub group: String,
    pub name: String,
    pub family0: String,
    pub writable: bool,
    #[serde(rename = "type")]
    pub vtype: String,
    pub protected: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample: Option<String>,
    #[serde(rename = "sample_is_file", skip_serializing_if = "Option::is_none")]
    pub sample_is_file: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noop: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GroupCounts {
    pub writable: u32,
    pub readonly: u32,
    pub protected_writable: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFile {
    pub generated_by: String,
    pub description: String,
    pub groups: std::collections::BTreeMap<String, GroupCounts>,
    pub tag_count: usize,
    pub tags: Vec<ManifestTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub noop_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadonlyTag {
    pub group: String,
    pub name: String,
    pub family0: String,
    #[serde(rename = "type")]
    pub vtype: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadonlyFile {
    pub generated_by: String,
    pub description: String,
    pub tag_count: usize,
    pub tags: Vec<ReadonlyTag>,
}

/// One tag's accumulated read+write result. Mirrors the Python `results[key]`
/// dict, which is built incrementally across the read and write phases, so
/// every field is optional except the manifest-derived ones attached last.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResultEntry {
    pub group: String,
    pub name: String,
    pub sample: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub vtype: Option<String>,
    #[serde(default)]
    pub protected: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_batch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_bug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ox_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ox_val: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub et_val: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub write: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_ox_val: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_et_val: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_ox_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bug_cluster: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_warnings: Option<String>,
}

/// docs/reference/jpeg-tag-baseline.json — key order matters for readable diffs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaselineCounts {
    pub total_tested: u32,
    pub readable: u32,
    pub writable_cli: u32,
    pub full: u32,
    pub full_nonstandard: u32,
    pub read_only: u32,
    pub read_broken: u32,
    pub write_broken: u32,
    pub unsupported: u32,
    pub untestable: u32,
}
