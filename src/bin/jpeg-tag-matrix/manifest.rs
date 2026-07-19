#![allow(dead_code)]

use clap::Args;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Args)]
pub struct ManifestArgs {
    #[arg(long)]
    pub flag_noops: bool,
}

pub fn run(_args: ManifestArgs) -> anyhow::Result<()> {
    Ok(())
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
}
