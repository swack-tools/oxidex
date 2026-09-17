//! Family-1 groups for three readers whose values were already right but
//! reported under a family-1 group ExifTool does not use: ID3v2 frames,
//! non-standard (trailer) IPTC, and the InfiRay / GoPro / Adobe JPEG APPn
//! records. Family 0 and the storage keys are unchanged.
//!
//! Every expectation is the pinned ExifTool 13.59 oracle's own output
//! (`-ver` 13.59, `OOXML.docx` probe `DOCX`) on the named `t/images` file:
//!
//! ```text
//! MP3.mp3       -G1 -s -Title          [ID3v2_2] Title : ExifTool Test
//! MP3.mp3       -G0 -s -Title          [ID3]     Title : ExifTool Test
//! AIFF.aif      -G1 -s -Album          [ID3v2_2] Album : the album
//! AFCP.jpg      -G1 -s -Headline       [IPTC2]   Headline : headline
//! FotoStation.jpg -G1 -s -Category     [IPTC2]   Category : Cat
//! ExifTool.jpg  -a -G1 -s -ApplicationRecordVersion
//!               [IPTC]  2 / [IPTC2] 2 / [IPTC3] 2
//! ExifTool.jpg  -G0 -s -ColorTransform [APP14]   ColorTransform : YCbCr
//! ExifTool.jpg  -G1 -s -ColorTransform [Adobe]   ColorTransform : YCbCr
//! InfiRay.jpg   -G1 -s -IJPEGVersion   [InfiRay] IJPEGVersion : 0 2 0 1
//! InfiRay.jpg   -G0 -s -IJPEGVersion   [APP2]    IJPEGVersion : 0 2 0 1
//! InfiRay.jpg   -G1 -s -ImagingData    [InfiRay] ImagingData : (Binary data 20 bytes, ...)
//! InfiRay.jpg   -G1 -s -IsothermalMax  [InfiRay] IsothermalMax : 80
//! GoPro.jpg     -G1 -s -Model          [GoPro]   Model : HERO6 Black
//! GoPro.jpg     -G0 -s -GoPro:Model    [APP6]    Model : HERO6 Black
//! ```

#[path = "common/fixtures.rs"]
mod fixtures;

use fixtures::pinned_fixture_path;
use std::process::Command;

fn run(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_oxidex"))
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("failed to run oxidex {args:?}: {e}"));
    let stdout = String::from_utf8(output.stdout)
        .unwrap_or_else(|e| panic!("oxidex {args:?} produced non-UTF8 stdout: {e}"));
    assert!(!stdout.trim().is_empty(), "oxidex {args:?} printed nothing");
    stdout
}

/// `[Group] Tag: value` lines, whitespace-normalized.
fn lines(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

fn case(file: &str, args: &[&str]) -> Option<Vec<String>> {
    let path = pinned_fixture_path(file)?;
    let mut all = args.to_vec();
    let path = path.to_str().unwrap().to_string();
    all.push(&path);
    Some(lines(&run(&all)))
}

#[test]
fn id3v2_frames_report_the_version_table_group() {
    let Some(g1) = case("MP3.mp3", &["-G1", "-s", "-Title"]) else {
        return;
    };
    // The ID3v2.2 frame wins the bare request; the v1 copy is priority 0.
    assert_eq!(g1, vec!["[ID3v2_2] Title: ExifTool Test"]);
    let g0 = case("MP3.mp3", &["-G0", "-s", "-Title"]).unwrap();
    assert_eq!(g0, vec!["[ID3] Title: ExifTool Test"]);
    // Both the new family-1 name and the family-0 name select it.
    let by_g1 = case("MP3.mp3", &["-G1", "-s", "-ID3v2_2:Title"]).unwrap();
    assert_eq!(by_g1, vec!["[ID3v2_2] Title: ExifTool Test"]);
    let by_g0 = case("MP3.mp3", &["-a", "-G1", "-s", "-ID3:Title"]).unwrap();
    assert_eq!(
        by_g0,
        vec!["[ID3v2_2] Title: ExifTool Test", "[ID3v1] Title: Title"]
    );

    let aiff = case("AIFF.aif", &["-G1", "-s", "-Album"]).unwrap();
    assert_eq!(aiff, vec!["[ID3v2_2] Album: the album"]);
}

#[test]
fn trailer_iptc_reports_numbered_groups() {
    let Some(afcp) = case("AFCP.jpg", &["-G1", "-s", "-Headline"]) else {
        return;
    };
    assert_eq!(afcp, vec!["[IPTC2] Headline: headline"]);
    let afcp_g0 = case("AFCP.jpg", &["-G0", "-s", "-Headline"]).unwrap();
    assert_eq!(afcp_g0, vec!["[IPTC] Headline: headline"]);

    let foto = case("FotoStation.jpg", &["-G1", "-s", "-Category"]).unwrap();
    assert_eq!(foto, vec!["[IPTC2] Category: Cat"]);

    // Standard APP13 IPTC, then FotoStation (outermost trailer) IPTC2, then
    // AFCP IPTC3. oxidex lists the trailers first (its own insertion order),
    // so compare the set, not the order.
    let mut all = case(
        "ExifTool.jpg",
        &["-a", "-G1", "-s", "-ApplicationRecordVersion"],
    )
    .unwrap();
    all.sort();
    assert_eq!(
        all,
        vec![
            "[IPTC2] ApplicationRecordVersion: 2",
            "[IPTC3] ApplicationRecordVersion: 2",
            "[IPTC] ApplicationRecordVersion: 2",
        ]
    );
    // The standard directory keeps the default winner.
    let winner = case("ExifTool.jpg", &["-G1", "-s", "-Headline"]).unwrap();
    assert_eq!(winner, vec!["[IPTC] Headline: No headline"]);
    let iptc3 = case("ExifTool.jpg", &["-G1", "-s", "-IPTC3:Headline"]).unwrap();
    assert_eq!(iptc3, vec!["[IPTC3] Headline: headline"]);
}

#[test]
fn adobe_app14_reports_the_adobe_group() {
    let Some(g1) = case("ExifTool.jpg", &["-G1", "-s", "-ColorTransform"]) else {
        return;
    };
    assert_eq!(g1, vec!["[Adobe] ColorTransform: YCbCr"]);
    let g0 = case("ExifTool.jpg", &["-G0", "-s", "-APP14:ColorTransform"]).unwrap();
    assert_eq!(g0, vec!["[APP14] ColorTransform: YCbCr"]);
}

#[test]
fn infiray_records_report_the_infiray_group() {
    let Some(version) = case("InfiRay.jpg", &["-G1", "-s", "-IJPEGVersion"]) else {
        return;
    };
    assert_eq!(version, vec!["[InfiRay] IJPEGVersion: 0 2 0 1"]);
    let version_g0 = case("InfiRay.jpg", &["-G0", "-s", "-IJPEGVersion"]).unwrap();
    assert_eq!(version_g0, vec!["[APP2] IJPEGVersion: 0 2 0 1"]);
    // APP3 (JPEG.pm's ImagingData), APP6 MixMode, APP8 Isothermal and APP9
    // Sensor each reach the output through a different merge.
    let rest = case(
        "InfiRay.jpg",
        &[
            "-G1",
            "-s",
            "-ImagingData",
            "-MixMode",
            "-IsothermalMax",
            "-IRSensorName",
        ],
    )
    .unwrap();
    assert_eq!(
        rest,
        vec![
            "[InfiRay] ImagingData: (Binary data 20 bytes, use -b option to extract)",
            "[InfiRay] MixMode: 0",
            "[InfiRay] IsothermalMax: 80",
            "[InfiRay] IRSensorName: P2_USB_IR",
        ]
    );
}

#[test]
fn gopro_app6_reports_the_gopro_group() {
    let Some(g1) = case("GoPro.jpg", &["-G1", "-s", "-Model"]) else {
        return;
    };
    assert_eq!(g1, vec!["[GoPro] Model: HERO6 Black"]);
    let g0 = case("GoPro.jpg", &["-G0", "-s", "-GoPro:Model"]).unwrap();
    assert_eq!(g0, vec!["[APP6] Model: HERO6 Black"]);
}

/// A JPEG with an IFD0 IPTC-NAA block and an AFCP trailer. Pinned ExifTool
/// numbers the IFD0 block `IPTC2` and the trailer `IPTC3`; OxiDex does not
/// yet number the IFD0 block, so the trailer must not take `IPTC2` (a request
/// for `IPTC2:ObjectName` would silently return the trailer's value). Both stay
/// under the unnumbered group until the IFD0 block is modelled.
///
/// Fixture: `t/images/AFCP.jpg` with `-IFD0:IPTC-NAA<=iptc.bin` written by the
/// pinned ExifTool 13.59, whose `-a -G1 -s -ObjectName` output is
/// `[IPTC2] ObjectName : ifd0obj` and `[IPTC3] ObjectName : object name`.
#[test]
fn trailer_iptc_is_not_numbered_ahead_of_an_ifd0_iptc_block() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/jpeg/afcp_trailer_with_ifd0_iptc.jpg"
    );
    let out = run(&["-a", "-G1", "-s", "-ObjectName", path]);
    assert!(!out.contains("[IPTC2]"), "trailer took IPTC2:\n{out}");
    assert!(
        out.contains("ObjectName: object name"),
        "trailer value missing:\n{out}"
    );
    assert!(
        out.contains("ObjectName: ifd0obj"),
        "IFD0 value missing:\n{out}"
    );
}
