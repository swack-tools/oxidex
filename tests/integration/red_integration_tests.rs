use oxidex::core::operations::read_metadata;
use std::path::Path;

/// ExifTool 13.59 reads the pinned `t/images/Red.r3d` fixture -- a real
/// Redcode version 2 clip -- as 34 `[Red]` tags. The values asserted below
/// were taken from the pinned oracle directly
/// (`exiftool-pinned.sh -a -G1 -s Red.r3d`), not from this parser's output.
///
/// The set deliberately spans every decode path in `red.rs`, so a regression
/// in any one of them fails here:
///
/// - `RedcodeVersion`/`ImageWidth`/`ImageHeight` come from the generated
///   `Red::RED2` binary table (Red.pm:184-205).
/// - `FrameRate` is the hand-ported `int16u[3]` ValueConv the generator
///   declines to model (Red.pm:196-203).
/// - `StartEdgeCode` is a plain format-1 string entry (Red.pm:48).
/// - `DateCreated`/`TimeCreated` exercise the two `s///` date ValueConvs
///   (Red.pm:77-86).
/// - `ColorTemperature` and `RGBCurves` are format-2 floats, single and
///   repeating, rendered through Perl's `%.15g` (Red.pm:127, Red.pm:130).
/// - `FNumber` is `$val / 10` and `FocusDistance` is `$val/1000` then
///   `"$val m"` (Red.pm:141, Red.pm:145).
/// - `ISO` and `FocalLength` are bare int16u shorthands -- `FocalLength`
///   pins that no " mm" suffix is invented, since Red.pm:142 declares no
///   PrintConv (unlike Canon.pm:3138, which does).
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn red_r3d_fixture_matches_pinned_oracle() {
    let metadata = read_metadata(Path::new(
        "/tmp/oxidex-exiftool-cache/exiftool/t/images/Red.r3d",
    ))
    .expect("read pinned Red.r3d fixture");

    assert_eq!(metadata.get_string("File:FileType"), Some("R3D"));

    // Red::RED2 header (generated table).
    assert_eq!(metadata.get_string("Red:RedcodeVersion"), Some("2"));
    assert_eq!(metadata.get_integer("Red:ImageWidth"), Some(5120));
    assert_eq!(metadata.get_integer("Red:ImageHeight"), Some(2560));
    // Hand-ported ValueConv: (0 * 0x10000 + 24000) / 1001, then
    // int($val * 1000 + 0.5) / 1000.
    assert_eq!(metadata.get_string("Red:FrameRate"), Some("23.976"));

    // Red::Main directory entries.
    assert_eq!(
        metadata.get_string("Red:StartEdgeCode"),
        Some("01:49:54:11")
    );
    assert_eq!(
        metadata.get_string("Red:StartTimecode"),
        Some("21:36:16:18")
    );
    assert_eq!(metadata.get_string("Red:SerialNumber"), Some("130-246-CE5"));
    assert_eq!(metadata.get_string("Red:CameraType"), Some("A"));
    assert_eq!(metadata.get_string("Red:ReelNumber"), Some("106"));
    assert_eq!(metadata.get_string("Red:Take"), Some("037"));
    assert_eq!(metadata.get_string("Red:FirmwareVersion"), Some("6.2.34"));
    assert_eq!(metadata.get_string("Red:StorageType"), Some("RED 512GB V4"));
    assert_eq!(
        metadata.get_string("Red:StorageSerialNumber"),
        Some("15240FC80DE7")
    );
    assert_eq!(
        metadata.get_string("Red:OriginalFileName"),
        Some("A106_C037_0118G5_002.R3D")
    );
    assert_eq!(metadata.get_string("Red:Model"), Some("S-WEAPON"));
    assert_eq!(metadata.get_string("Red:Filter"), Some("STANDARD"));
    assert_eq!(metadata.get_string("Red:VideoFormat"), Some("5K 2:1"));

    // `s/(\d{4})(\d{2})/$1:$2:/` and `s/(\d{2})(\d{2})/$1:$2:/`.
    assert_eq!(metadata.get_string("Red:DateCreated"), Some("2016:01:18"));
    assert_eq!(metadata.get_string("Red:TimeCreated"), Some("21:35:55"));
    assert_eq!(
        metadata.get_string("Red:StorageFormatDate"),
        Some("2016:01:18")
    );
    assert_eq!(
        metadata.get_string("Red:StorageFormatTime"),
        Some("21:35:55")
    );

    // Format-2 floats through Perl's default %.15g stringification.
    assert_eq!(metadata.get_string("Red:ColorTemperature"), Some("4800"));
    assert_eq!(
        metadata.get_string("Red:RGBCurves"),
        Some(
            "0 0 0.25 0.25 0.5 0.5 0.75 0.75 1 1 0 0 0.25 0.25 0.5 0.5 0.75 0.75 1 1 \
             0 0 0.25 0.25 0.5 0.5 0.75 0.75 1 1"
        )
    );
    assert_eq!(metadata.get_string("Red:OriginalFrameRate"), Some("23.976"));

    // int16u entries, with and without a ValueConv.
    assert_eq!(metadata.get_string("Red:CropArea"), Some("0 0 5120 2560"));
    assert_eq!(metadata.get_string("Red:ISO"), Some("1280"));
    assert_eq!(metadata.get_string("Red:FNumber"), Some("4.9"));
    // Red.pm:142 declares no PrintConv, so no unit is appended.
    assert_eq!(metadata.get_string("Red:FocalLength"), Some("24"));
    // int32s, $val/1000 then "$val m".
    assert_eq!(metadata.get_string("Red:FocusDistance"), Some("-0.001 m"));
}
