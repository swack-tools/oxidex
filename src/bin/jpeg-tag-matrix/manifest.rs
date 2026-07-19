#![allow(dead_code)]

use clap::Args;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use crate::types::{self, ManifestFile, ManifestTag, ReadonlyFile, ReadonlyTag};

#[derive(Args)]
pub struct ManifestArgs {
    #[arg(long)]
    pub flag_noops: bool,
}

/// XML schema for ExifTool's `-listx` output
#[derive(Debug, Deserialize)]
pub struct ListxRoot {
    #[serde(rename = "table", default)]
    pub tables: Vec<ListxTable>,
}

#[derive(Debug, Deserialize)]
pub struct ListxTable {
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "@g0", default)]
    pub g0: String,
    #[serde(rename = "@g1", default)]
    pub g1: String,
    #[serde(rename = "tag", default)]
    pub tags: Vec<ListxTag>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ListxTag {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(rename = "@g1", default)]
    pub g1: String,
    #[serde(rename = "@type", default)]
    pub vtype: String,
    #[serde(rename = "@writable", default)]
    pub writable: String,
    #[serde(rename = "@flags", default)]
    pub flags: String,
    #[serde(rename = "@count", default)]
    pub count: String,
    #[serde(rename = "values", default)]
    pub values: Option<ListxValues>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ListxValues {
    #[serde(rename = "key", default)]
    pub keys: Vec<ListxKey>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ListxKey {
    #[serde(rename = "val", default)]
    pub vals: Vec<ListxVal>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ListxVal {
    #[serde(rename = "@lang", default)]
    pub lang: String,
    #[serde(rename = "$text", default)]
    pub text: String,
}

/// First English enum label, preferring a distinctive one over a bare
/// "None"/"Unknown" sentinel: those are frequently a tag's own unset
/// default, so writing that exact value as the sample makes a genuine
/// write indistinguishable from a no-op that left the default untouched.
fn first_en_value(tag: &ListxTag) -> Option<String> {
    let values = tag.values.as_ref()?;
    let labels: Vec<&str> = values
        .keys
        .iter()
        .flat_map(|k| k.vals.iter())
        .filter(|v| v.lang == "en")
        .map(|v| v.text.as_str())
        .collect();
    labels
        .iter()
        .find(|l| **l != "None" && **l != "Unknown")
        .or_else(|| labels.first())
        .map(|s| s.to_string())
}

const DT: &str = "2024:01:15 10:30:00";
const D: &str = "2024:01:15";
const T: &str = "10:30:00";

static INT_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "int8u", "int8s", "int16u", "int16s", "int32u", "int32s", "int64u", "int64s", "integer",
        "digits",
    ]
    .into_iter()
    .collect()
});

static RAT_TYPES: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    [
        "rational32u",
        "rational32s",
        "rational64u",
        "rational64s",
        "rational",
        "real",
        "float",
        "double",
        "fixed16u",
        "fixed16s",
        "fixed32u",
        "fixed32s",
    ]
    .into_iter()
    .collect()
});

static STRINGISH: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    ["string", "undef", "?", "var_ustr32", "var_string", "lang-alt", "binary"]
        .into_iter()
        .collect()
});

fn override_sample(group1: &str, name: &str) -> Option<&'static str> {
    match (group1, name) {
        ("Photoshop", "IPTCDigest") => Some("new"),
        ("GPS", "GPSVersionID") => Some("2.3.0.0"),
        // PhotoshopThumbnail/PhotoshopBGRThumbnail resolved to BASE_FIXTURE
        // path by the caller (main()), not here -- see gps/file sample handling.
        _ => None,
    }
}

fn gps_sample(name: &str) -> Option<&'static str> {
    match name {
        "GPSLatitude" | "GPSDestLatitude" => Some("37.7749"),
        "GPSLatitudeRef" | "GPSDestLatitudeRef" => Some("N"),
        "GPSLongitude" | "GPSDestLongitude" => Some("122.4194"),
        "GPSLongitudeRef" | "GPSDestLongitudeRef" => Some("W"),
        "GPSAltitude" => Some("10.5"),
        "GPSDestDistance" => Some("1.5"),
        "GPSTimeStamp" => Some("10:30:00"),
        "GPSDateStamp" => Some("2024:01:15"),
        "GPSDateTime" => Some("2024:01:15 10:30:00"),
        _ => None,
    }
}

pub fn make_sample(family0: &str, name: &str, vtype: &str, tag: &ListxTag, group1: &str) -> String {
    if let Some(s) = override_sample(group1, name) {
        return s.to_string();
    }
    if let Some(s) = gps_sample(name) {
        return s.to_string();
    }
    if family0 == "EXIF" && vtype == "undef" && name.contains("Version") {
        return "0100".to_string();
    }
    if name.starts_with("OffsetTime") {
        return "+05:30".to_string();
    }
    if vtype == "boolean" {
        return "True".to_string();
    }
    if let Some(ev) = first_en_value(tag) {
        return ev;
    }
    if vtype == "date" {
        return DT.to_string();
    }
    if vtype == "struct" {
        return "{}".to_string();
    }
    if STRINGISH.contains(vtype) || vtype == "digits" {
        if name.starts_with("SubSec") {
            return "3".to_string();
        }
        if name.contains("Date") {
            if family0 == "IPTC" || vtype == "digits" {
                return D.to_string();
            }
            return DT.to_string();
        }
        if name.contains("Time") && family0 == "IPTC" {
            return T.to_string();
        }
    }
    if INT_TYPES.contains(vtype) || RAT_TYPES.contains(vtype) {
        let scalar = if INT_TYPES.contains(vtype) { "3" } else { "1.5" };
        let n: usize = tag.count.parse().unwrap_or(1);
        if n > 1 {
            return vec![scalar; n].join(" ");
        }
        return scalar.to_string();
    }
    "OxTest".to_string()
}

/// Run `exiftool -f -listx -{group}:all`, persist the raw XML dump alongside
/// the other working files (useful for post-mortem debugging), and parse it
/// into the `ListxRoot` schema.
fn dump_listx(exiftool: &str, group: &str, work: &Path) -> anyhow::Result<ListxRoot> {
    let out = Command::new(exiftool)
        .args(["-f", "-listx", &format!("-{group}:all")])
        .output()?;
    let xml = String::from_utf8_lossy(&out.stdout).to_string();
    std::fs::write(work.join(format!("listx_{group}.xml")), &xml)?;
    let root: ListxRoot = quick_xml::de::from_str(&xml)
        .map_err(|e| anyhow::anyhow!("empty or malformed -listx dump for {group}: {e}"))?;
    Ok(root)
}

/// (listx group arg, family0 bucket, table filter predicate)
struct Source {
    group_arg: &'static str,
    family0: &'static str,
    table_pred: fn(&ListxTable) -> bool,
}

const SOURCES: &[Source] = &[
    Source {
        group_arg: "EXIF",
        family0: "EXIF",
        table_pred: |t| t.name == "Exif::Main" || t.name == "GPS::Main",
    },
    Source {
        group_arg: "XMP",
        family0: "XMP",
        table_pred: |t| t.g0 == "XMP",
    },
    Source {
        group_arg: "IPTC",
        family0: "IPTC",
        table_pred: |t| t.name.starts_with("IPTC::"),
    },
    Source {
        group_arg: "JFIF",
        family0: "JFIF",
        table_pred: |t| t.name.starts_with("JFIF::"),
    },
    Source {
        group_arg: "Photoshop",
        family0: "Photoshop",
        table_pred: |t| t.name.starts_with("Photoshop::"),
    },
    Source {
        group_arg: "ICC_Profile",
        family0: "ICC_Profile",
        table_pred: |t| t.name.starts_with("ICC_Profile::"),
    },
    // JPEG COM segment: only the Comment tag from the Extra table.
    Source {
        group_arg: "File",
        family0: "File",
        table_pred: |t| t.name == "Extra",
    },
];

/// (group1, name) pairs whose sample must be a file path (written as
/// `-TAG<=file`) rather than a literal value.
const FILE_SAMPLES: &[(&str, &str)] = &[
    ("Photoshop", "PhotoshopThumbnail"),
    ("Photoshop", "PhotoshopBGRThumbnail"),
];

/// Write-test suspect tags (MakerNote*/Photoshop/JFIF) against the base
/// fixture and mark silent no-ops with `noop: true`. This is a wiring-only
/// stub: the behavioral write-test itself is implemented in a later task.
fn flag_noops(
    manifest: &mut ManifestFile,
    _ver: &str,
    _exiftool: &str,
    _base_fixture: &Path,
    _work: &Path,
) -> anyhow::Result<()> {
    let _ = manifest;
    Ok(())
}

pub fn run(args: ManifestArgs) -> anyhow::Result<()> {
    let exiftool = std::env::var("EXIFTOOL").unwrap_or_else(|_| "exiftool".into());
    let work = std::env::var("TAGMATRIX_WORK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("oxidex-tagmap"));
    let repo = std::env::current_dir()?;
    let base_fixture = repo.join("tests/fixtures/jpeg/tag_matrix_base.jpg");
    std::fs::create_dir_all(&work)?;

    let ver_out = Command::new(&exiftool).arg("-ver").output()?;
    let ver = String::from_utf8_lossy(&ver_out.stdout).trim().to_string();
    println!("exiftool {ver}; work dir {}", work.display());

    let mut all_entries: std::collections::HashMap<(String, String), ManifestTag> =
        std::collections::HashMap::new();

    for source in SOURCES {
        let root = dump_listx(&exiftool, source.group_arg, &work)?;
        for table in root.tables.iter().filter(|t| (source.table_pred)(t)) {
            for tag_el in &table.tags {
                if source.family0 == "File" && tag_el.name != "Comment" {
                    continue;
                }
                let g1 = if source.family0 == "File" {
                    "File".to_string()
                } else if !tag_el.g1.is_empty() {
                    tag_el.g1.clone()
                } else {
                    table.g1.clone()
                };
                let writable = tag_el.writable == "true";
                let flagset: HashSet<&str> =
                    tag_el.flags.split(',').filter(|s| !s.is_empty()).collect();
                let protected = flagset.contains("Protected")
                    || flagset.contains("Unsafe")
                    || flagset.contains("Avoid");

                let mut entry = ManifestTag {
                    group: g1.clone(),
                    name: tag_el.name.clone(),
                    family0: source.family0.to_string(),
                    writable,
                    vtype: tag_el.vtype.clone(),
                    protected,
                    flags: if tag_el.flags.is_empty() {
                        None
                    } else {
                        Some(tag_el.flags.clone())
                    },
                    count: tag_el.count.parse().ok(),
                    sample: None,
                    sample_is_file: None,
                    noop: None,
                };

                if writable {
                    let mut sample =
                        make_sample(source.family0, &tag_el.name, &tag_el.vtype, tag_el, &g1);
                    let is_file_sample =
                        FILE_SAMPLES.contains(&(g1.as_str(), tag_el.name.as_str()));
                    if is_file_sample {
                        sample = base_fixture.display().to_string();
                        entry.sample_is_file = Some(true);
                    }
                    entry.sample = Some(sample);
                }

                let key = (g1.clone(), tag_el.name.clone());
                match all_entries.get(&key) {
                    None => {
                        all_entries.insert(key, entry);
                    }
                    Some(prev) => {
                        // Prefer writable over not, then non-protected.
                        let prev_rank = (prev.writable, !prev.protected);
                        let new_rank = (entry.writable, !entry.protected);
                        if new_rank > prev_rank {
                            all_entries.insert(key, entry);
                        }
                    }
                }
            }
        }
    }

    let mut entries: Vec<ManifestTag> = all_entries.into_values().collect();
    entries.sort_by(|a, b| (&a.family0, &a.group, &a.name).cmp(&(&b.family0, &b.group, &b.name)));

    let writable_tags: Vec<ManifestTag> = entries.iter().filter(|e| e.writable).cloned().collect();
    let readonly_tags: Vec<ReadonlyTag> = entries
        .iter()
        .filter(|e| !e.writable)
        .map(|e| ReadonlyTag {
            group: e.group.clone(),
            name: e.name.clone(),
            family0: e.family0.clone(),
            vtype: e.vtype.clone(),
        })
        .collect();

    let mut groups: std::collections::BTreeMap<String, types::GroupCounts> = Default::default();
    for e in &entries {
        let g = groups.entry(e.family0.clone()).or_default();
        if e.writable {
            g.writable += 1;
            if e.protected {
                g.protected_writable += 1;
            }
        } else {
            g.readonly += 1;
        }
    }

    let mut manifest = ManifestFile {
        generated_by: format!("exiftool {ver}"),
        description: "ExifTool tags writable in JPEG files (testable universe for a \
                       read/write support matrix). group = ExifTool family-1 group."
            .into(),
        groups: groups.clone(),
        tag_count: writable_tags.len(),
        tags: writable_tags,
        noop_note: None,
    };

    if args.flag_noops {
        flag_noops(&mut manifest, &ver, &exiftool, &base_fixture, &work)?;
    }

    std::fs::write(
        work.join("exiftool_jpeg_tags.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    let readonly = ReadonlyFile {
        generated_by: format!("exiftool {ver}"),
        description: "JPEG-relevant ExifTool tags that are read-only (writable=false); \
                       not testable via synthesis."
            .into(),
        tag_count: readonly_tags.len(),
        tags: readonly_tags,
    };
    std::fs::write(
        work.join("exiftool_jpeg_readonly_tags.json"),
        serde_json::to_string_pretty(&readonly)?,
    )?;

    println!(
        "{:<12} {:>8} {:>11} {:>8}",
        "family0", "writable", "(protected)", "readonly"
    );
    let mut total = types::GroupCounts::default();
    for (g, c) in &groups {
        println!(
            "{g:<12} {:>8} {:>11} {:>8}",
            c.writable, c.protected_writable, c.readonly
        );
        total.writable += c.writable;
        total.protected_writable += c.protected_writable;
        total.readonly += c.readonly;
    }
    println!(
        "{:<12} {:>8} {:>11} {:>8}",
        "TOTAL", total.writable, total.protected_writable, total.readonly
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(name: &str, vtype: &str, count: &str) -> ListxTag {
        ListxTag {
            name: name.to_string(),
            g1: String::new(),
            vtype: vtype.to_string(),
            writable: "true".to_string(),
            flags: String::new(),
            count: count.to_string(),
            values: None,
        }
    }

    #[test]
    fn override_wins_over_type() {
        let t = tag("IPTCDigest", "string", "1");
        assert_eq!(
            make_sample("Photoshop", "IPTCDigest", "string", &t, "Photoshop"),
            "new"
        );
    }

    #[test]
    fn gps_sample_table_wins() {
        let t = tag("GPSLatitude", "rational64u", "1");
        assert_eq!(
            make_sample("EXIF", "GPSLatitude", "rational64u", &t, "GPS"),
            "37.7749"
        );
    }

    #[test]
    fn exif_undef_version_tag() {
        let t = tag("ExifVersion", "undef", "4");
        assert_eq!(
            make_sample("EXIF", "ExifVersion", "undef", &t, "ExifIFD"),
            "0100"
        );
    }

    #[test]
    fn offset_time_tag() {
        let t = tag("OffsetTimeOriginal", "string", "1");
        assert_eq!(
            make_sample("EXIF", "OffsetTimeOriginal", "string", &t, "ExifIFD"),
            "+05:30"
        );
    }

    #[test]
    fn boolean_type() {
        let t = tag("SomeFlag", "boolean", "1");
        assert_eq!(
            make_sample("XMP", "SomeFlag", "boolean", &t, "XMP-x"),
            "True"
        );
    }

    #[test]
    fn int_type_repeats_scalar_for_count() {
        let t = tag("SomeInts", "int16u", "3");
        assert_eq!(
            make_sample("EXIF", "SomeInts", "int16u", &t, "ExifIFD"),
            "3 3 3"
        );
    }

    #[test]
    fn rational_type_single_count() {
        let t = tag("SomeRational", "rational64u", "1");
        assert_eq!(
            make_sample("EXIF", "SomeRational", "rational64u", &t, "ExifIFD"),
            "1.5"
        );
    }

    #[test]
    fn fallback_generic_string() {
        let t = tag("SomeWeirdTag", "unknowntype", "1");
        assert_eq!(
            make_sample("EXIF", "SomeWeirdTag", "unknowntype", &t, "ExifIFD"),
            "OxTest"
        );
    }

    #[test]
    fn parses_minimal_listx_fixture() {
        let xml =
            std::fs::read_to_string("tests/fixtures/jpeg-tag-matrix/listx_sample.xml").unwrap();
        let root: ListxRoot = quick_xml::de::from_str(&xml).unwrap();
        assert_eq!(root.tables.len(), 1);
        assert_eq!(root.tables[0].tags[0].name, "ISO");
    }
}
