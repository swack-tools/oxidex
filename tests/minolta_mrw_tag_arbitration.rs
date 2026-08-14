//! Which occurrence of a duplicated tag name an MRW file projects by default,
//! and which family-1 group that occurrence reports.
//!
//! Every expectation below is quoted from the pinned 13.59 oracle run against
//! the real carrier -- no hand-authored bytes:
//!
//! ```text
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -ver
//! 13.59
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -G1 -s \
//!     -ExposureMode -WhiteBalance -MeteringMode -Sharpness -ISOSetting -BWFilter \
//!     /tmp/oxidex-exiftool-cache/combined-samples/Minolta.mrw
//! [ExifIFD]       ExposureMode                    : Manual
//! [Minolta]       WhiteBalance                    : Auto
//! [ExifIFD]       MeteringMode                    : Unknown
//! [MinoltaRaw]    Sharpness                       : 0
//! [MinoltaRaw]    ISOSetting                      : 65
//! [MinoltaRaw]    BWFilter                        : 0
//! $ ... -G0 -s ...
//! [EXIF]          ExposureMode                    : Manual
//! [MakerNotes]    WhiteBalance                    : Auto
//! [EXIF]          MeteringMode                    : Unknown
//! [MakerNotes]    Sharpness                       : 0
//! [MakerNotes]    ISOSetting                      : 65
//! [MakerNotes]    BWFilter                        : 0
//! ```
//!
//! `-a -G1` shows that every one of these names occurs more than once in the
//! file, so each line above is an arbitration outcome, not a decode:
//! `[Minolta] MeteringMode: Multi-segment` and `[ExifIFD] MeteringMode:
//! Unknown` both exist, and ExifTool projects the ExifIFD one because
//! `%Minolta::CameraSettings` is `PRIORITY => 0` (Minolta.pm:974) and
//! `FoundTag` will not let a 0 displace a value already present
//! (ExifTool.pm:9564, :9585).

use oxidex::cli::tag_resolution::{
    family0_label, family1_label, resolve_requested_tags, resolved_display_value,
};
use oxidex::core::operations::read_metadata;
use std::path::Path;

const MINOLTA_MRW: &str = "/tmp/oxidex-exiftool-cache/combined-samples/Minolta.mrw";

/// The one-line form ExifTool's `-s` prints. `BWFilter`/`Sharpness` are stored
/// as integers, everything else here as an already-formatted string.
fn shown(value: &oxidex::core::TagValue) -> String {
    value
        .as_string()
        .map(str::to_string)
        .or_else(|| value.as_integer().map(|i| i.to_string()))
        .unwrap_or_default()
}

/// `(tag, family-0 group, family-1 group, value)`, transcribed from the two
/// oracle runs in the module comment.
const ORACLE_DEFAULT_PROJECTION: &[(&str, &str, &str, &str)] = &[
    ("ExposureMode", "EXIF", "ExifIFD", "Manual"),
    ("WhiteBalance", "MakerNotes", "Minolta", "Auto"),
    ("MeteringMode", "EXIF", "ExifIFD", "Unknown"),
    ("Sharpness", "MakerNotes", "MinoltaRaw", "0"),
    ("ISOSetting", "MakerNotes", "MinoltaRaw", "65"),
    ("BWFilter", "MakerNotes", "MinoltaRaw", "0"),
];

#[test]
fn minolta_mrw_default_projection_matches_the_pinned_oracle() {
    if !Path::new(MINOLTA_MRW).is_file() {
        eprintln!("skipping: corpus fixture not present at {MINOLTA_MRW}");
        return;
    }
    let metadata = read_metadata(Path::new(MINOLTA_MRW)).expect("Minolta.mrw parses");

    for (tag, group0, group1, value) in ORACLE_DEFAULT_PROJECTION {
        let resolved = resolve_requested_tags(&metadata, &[(*tag).to_string()], false);
        assert_eq!(
            resolved.len(),
            1,
            "{tag}: an unqualified request projects exactly one occurrence"
        );
        let occurrence = resolved[0].occurrence;
        assert_eq!(
            shown(&resolved_display_value(occurrence, false)),
            *value,
            "{tag}: value"
        );
        assert_eq!(family0_label(occurrence), *group0, "{tag}: family 0");
        assert_eq!(family1_label(occurrence), *group1, "{tag}: family 1");
    }
}

/// The losing occurrences are still retained -- the fix arbitrates, it does
/// not drop a tag. Quoted from
/// `exiftool-pinned.sh -a -G1 -s -MeteringMode -Sharpness ... Minolta.mrw`:
///
/// ```text
/// [ExifIFD]       MeteringMode                    : Unknown
/// [Minolta]       MeteringMode                    : Multi-segment
/// [Minolta]       Sharpness                       : Normal
/// [ExifIFD]       Sharpness                       : Normal
/// [MinoltaRaw]    Sharpness                       : 0
/// ```
#[test]
fn minolta_mrw_retains_the_losing_occurrences() {
    if !Path::new(MINOLTA_MRW).is_file() {
        eprintln!("skipping: corpus fixture not present at {MINOLTA_MRW}");
        return;
    }
    let metadata = read_metadata(Path::new(MINOLTA_MRW)).expect("Minolta.mrw parses");

    // `all_occurrences = true` is the `-a` in the oracle command above.
    let all = |tag: &str| -> Vec<(String, String)> {
        resolve_requested_tags(&metadata, &[tag.to_string()], true)
            .iter()
            .map(|r| {
                (
                    family1_label(r.occurrence).to_string(),
                    shown(&resolved_display_value(r.occurrence, false)),
                )
            })
            .collect()
    };

    let metering = all("MeteringMode");
    assert!(
        metering.contains(&("Minolta".to_string(), "Multi-segment".to_string())),
        "the MakerNote MeteringMode must survive as a non-winning occurrence, got {metering:?}"
    );

    let sharpness: Vec<String> = all("Sharpness").into_iter().map(|(_, v)| v).collect();
    assert!(
        sharpness.iter().any(|v| v == "Normal") && sharpness.iter().any(|v| v == "0"),
        "both MakerNote Sharpness readings must survive, got {sharpness:?}"
    );
}
