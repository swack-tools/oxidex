//! Integration tests for ExifTool's CR3-only local-time rendering of
//! QuickTime-container timestamps.
//!
//! QuickTime.pm's shared `%timeInfo` block (QuickTime.pm:242-291, reused by
//! CreateDate/ModifyDate/MediaCreateDate/MediaModifyDate/TrackCreateDate/
//! TrackModifyDate) renders those fields through:
//! ```text
//! ValueConv => 'ConvertUnixTime($val, $self->Options("QuickTimeUTC") || $$self{FileType} eq "CR3")',
//! PrintConv => '$self->ConvertDateTime($val)',
//! ```
//! (QuickTime.pm:280,287). `ConvertUnixTime`'s second argument selects
//! `localtime` plus a `TimeZoneString` offset suffix over zone-less `gmtime`
//! (ExifTool.pm:6784-6810). With no `QuickTimeUTC` option, that argument is
//! `$$self{FileType} eq "CR3"` -- true only for a Canon CR3 still, resolved
//! from the file's `CNCV` box (Canon.pm's `%Canon::uuid` `CNCV` entry:
//! `OverrideFileType($1) if $val =~ /^Canon(\w{3})/i`), never for a Canon RAW
//! movie (`CRM`) and never for a generic QuickTime/MP4 container.
//!
//! These tests run the built CLI binary in a subprocess with an explicit
//! `TZ` env var, rather than mutating the test process's own environment,
//! so the assertion is deterministic under `cargo test`'s default
//! multi-threaded runner instead of racing every other test that might read
//! or rely on the ambient time zone.

use std::path::Path;
use std::process::Command;

const OXIDEX_BIN: &str = env!(
    "CARGO_BIN_EXE_oxidex",
    "oxidex binary not found. Run `cargo build` first."
);

/// Pinned ExifTool sample corpus populated by `just compare-exiftool-full`.
/// This is a local developer/CI cache, not a committed fixture -- see
/// `oxidex::test_support::PINNED_CORPUS_ROOT` -- so every test reading from
/// it gates on the file's presence and skips (rather than fails) when it is
/// absent.
const CR3_FIXTURE: &str = "/tmp/oxidex-exiftool-cache/combined-samples/CanonRaw.cr3";
const MOV_FIXTURE: &str = "/tmp/oxidex-exiftool-cache/combined-samples/QuickTime.mov";

fn run_oxidex_json(path: &str, tz: &str) -> serde_json::Value {
    let output = Command::new(OXIDEX_BIN)
        .args(["-j", path])
        .env("TZ", tz)
        .output()
        .expect("failed to execute oxidex");
    assert!(
        output.status.success(),
        "oxidex exited non-zero for {path}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).expect("oxidex -j must emit valid JSON");
    // oxidex -j returns an array of per-file objects; take the first.
    json.as_array()
        .and_then(|array| array.first())
        .cloned()
        .unwrap_or(json)
}

#[test]
fn cr3_create_date_renders_local_time_with_offset_under_america_chicago() {
    if !Path::new(CR3_FIXTURE).is_file() {
        eprintln!("skipping: corpus fixture not present at {CR3_FIXTURE}");
        return;
    }

    // Ground truth, instrument named: pinned oracle
    // (`/usr/bin/perl5.34 -I/tmp/oxidex-exiftool-cache/exiftool/lib
    // /tmp/oxidex-exiftool-cache/exiftool/exiftool`, ExifTool 13.59) run as
    // `TZ=America/Chicago exiftool -a -G1 -s t/images/CanonRaw.cr3` reports
    // `[QuickTime] CreateDate = 2018:02:21 06:08:56-06:00`, while
    // `[ExifIFD] CreateDate = 2018:02:21 12:08:56` (zone-less) and
    // `[Canon] TimeZone = +00:00` stay untouched -- confirming this file's
    // camera-local and UTC instants coincide, which is why the local-time
    // conversion and the EXIF zone-less rendering both surface a
    // recognizable "12:08:56"/"06:08:56" pair instead of an unrelated time.
    let json = run_oxidex_json(CR3_FIXTURE, "America/Chicago");

    assert_eq!(
        json.get("QuickTime:CreateDate").and_then(|v| v.as_str()),
        Some("2018:02:21 06:08:56-06:00"),
        "CR3 QuickTime:CreateDate must convert to local time with a UTC offset suffix"
    );
    assert_eq!(
        json.get("QuickTime:ModifyDate").and_then(|v| v.as_str()),
        Some("2018:02:21 06:08:56-06:00")
    );
    assert_eq!(
        json.get("QuickTime:MediaCreateDate")
            .and_then(|v| v.as_str()),
        Some("2018:02:21 06:08:56-06:00")
    );
    assert_eq!(
        json.get("QuickTime:TrackCreateDate")
            .and_then(|v| v.as_str()),
        Some("2018:02:21 06:08:56-06:00")
    );

    // The EXIF-side timestamp of the very same instant must stay zone-less:
    // QuickTime.pm's rule only touches the QuickTime-group tags rendered
    // through %timeInfo, never EXIF's DateTime PrintConv.
    assert_eq!(
        json.get("EXIF:CreateDate").and_then(|v| v.as_str()),
        Some("2018:02:21 12:08:56"),
        "EXIF:CreateDate must remain the zone-less rendering"
    );
}

#[test]
fn generic_quicktime_create_date_stays_zone_less_under_america_chicago() {
    if !Path::new(MOV_FIXTURE).is_file() {
        eprintln!("skipping: corpus fixture not present at {MOV_FIXTURE}");
        return;
    }

    // QuickTime.pm's local-time conversion is gated on
    // `$$self{FileType} eq 'CR3'` (QuickTime.pm:271,280); a plain .mov never
    // resolves to that file type, so it keeps the zone-less `gmtime`
    // rendering regardless of the process TZ. Ground truth (same pinned
    // oracle instrument as above): `QuickTime:CreateDate` on QuickTime.mov is
    // `2005:08:11 14:03:54` under any TZ.
    let json = run_oxidex_json(MOV_FIXTURE, "America/Chicago");

    assert_eq!(
        json.get("QuickTime:CreateDate").and_then(|v| v.as_str()),
        Some("2005:08:11 14:03:54"),
        "generic QuickTime CreateDate rendering must not change with TZ or this fix"
    );
}
