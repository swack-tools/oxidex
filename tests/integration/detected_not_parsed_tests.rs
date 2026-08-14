//! Regression tests for four formats that `detect_format()` never produced a
//! variant for, so `read_metadata` fell through to `add_identity_tags` and
//! reported success while emitting nothing but File/System tags.
//!
//! Every expected value below was read off the pinned oracle
//! (`exiftool-pinned.sh -a -G1 -s <fixture>`, ExifTool 13.59), not off this
//! crate's output, and every fixture is a real file from the pinned
//! ExifTool tree's `t/images`.

use oxidex::core::operations::read_metadata;
use std::path::Path;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new("/tmp/oxidex-exiftool-cache/exiftool/t/images").join(name)
}

/// `t/images/PSP.psp`, a Paint Shop Pro X3 image.
///
/// Covers all three decode paths in `psp.rs`: `FileVersion` from the file
/// header, the `PSP::Image` binary table (including its two `PrintConv`
/// enums and the `double` that must print as `200`, not `200.00`), and the
/// `~FL\0` sub-block walk over `PSP::Creator` with its Unix-time and
/// reversed-byte-version conversions.
///
/// `PSP:Copyright` is asserted *absent*: the fixture stores it as a raw
/// Latin-1 `0xA9`, which no Rust `String` can carry faithfully (see
/// `psp.rs`'s `string_value` docs). Pinning the absence keeps a future
/// change from silently substituting U+FFFD or `?`.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn psp_fixture_matches_pinned_oracle() {
    let metadata = read_metadata(&fixture("PSP.psp")).expect("read pinned PSP fixture");

    assert_eq!(metadata.get_string("File:FileType"), Some("PSP"));
    assert_eq!(metadata.get_string("PSP:FileVersion"), Some("10.0"));

    // PSP::Image, via the generated table.
    assert_eq!(metadata.get_integer("PSP:ImageWidth"), Some(8));
    assert_eq!(metadata.get_integer("PSP:ImageHeight"), Some(8));
    assert_eq!(metadata.get_string("PSP:ImageResolution"), Some("200"));
    assert_eq!(metadata.get_string("PSP:ResolutionUnit"), Some("inches"));
    assert_eq!(metadata.get_string("PSP:Compression"), Some("LZ77"));
    assert_eq!(metadata.get_integer("PSP:BitsPerSample"), Some(24));
    assert_eq!(metadata.get_integer("PSP:Planes"), Some(1));
    assert_eq!(metadata.get_integer("PSP:NumColors"), Some(16777216));

    // PSP::Creator, via the hand-ported ProcessExtData walk.
    assert_eq!(metadata.get_string("PSP:Title"), Some("Test Image"));
    assert_eq!(
        metadata.get_string("PSP:CreateDate"),
        Some("2010:01:28 14:23:21+00:00")
    );
    assert_eq!(
        metadata.get_string("PSP:ModifyDate"),
        Some("2010:01:28 14:30:26+00:00")
    );
    assert_eq!(metadata.get_string("PSP:Artist"), Some("Phil Harvey"));
    assert_eq!(
        metadata.get_string("PSP:Description"),
        Some("A description")
    );
    assert_eq!(
        metadata.get_string("PSP:CreatorAppID"),
        Some("Paint Shop Pro")
    );
    // Stored low byte first, so the four bytes reverse to 13.0.0.0.
    assert_eq!(
        metadata.get_string("PSP:CreatorAppVersion"),
        Some("13.0.0.0")
    );

    // Deliberately absent -- see the doc comment above.
    assert_eq!(metadata.get_string("PSP:Copyright"), None);
}

/// `t/images/Sony.pmp`, a Sony DSC-F1 still.
///
/// Covers the `Sony::PMP` binary table plus all four of the `RawConv`/
/// `ValueConv` fields hand-implemented in `pmp.rs`. `ExposureTime` in
/// particular pins that the `RawConv` guard is acknowledged: an earlier
/// revision asked only for `VALUE_CONV` and the tag silently vanished.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn pmp_fixture_matches_pinned_oracle() {
    let metadata = read_metadata(&fixture("Sony.pmp")).expect("read pinned PMP fixture");

    assert_eq!(metadata.get_string("File:FileType"), Some("PMP"));
    // Sony.pm:11377-11378 stamps both unconditionally.
    assert_eq!(metadata.get_string("ExifTool:Make"), Some("Sony"));
    assert_eq!(metadata.get_string("ExifTool:Model"), Some("DSC-F1"));

    assert_eq!(metadata.get_integer("Sony:JpgFromRawStart"), Some(124));
    assert_eq!(metadata.get_integer("Sony:JpgFromRawLength"), Some(251));
    assert_eq!(metadata.get_integer("Sony:SonyImageWidth"), Some(640));
    assert_eq!(metadata.get_integer("Sony:SonyImageHeight"), Some(480));
    assert_eq!(
        metadata.get_string("Sony:Orientation"),
        Some("Horizontal (normal)")
    );
    assert_eq!(metadata.get_string("Sony:ImageQuality"), Some("Standard"));
    // int8u[6] with the year pivoted at 70.
    assert_eq!(
        metadata.get_string("Sony:DateTimeOriginal"),
        Some("1998:09:01 20:19:57")
    );
    assert_eq!(
        metadata.get_string("Sony:ModifyDate"),
        Some("1998:09:01 20:19:57")
    );
    // 2 ** (-val / 100) through PrintExposureTime.
    assert_eq!(metadata.get_string("Sony:ExposureTime"), Some("1/100"));
    assert_eq!(metadata.get_string("Sony:Flash"), Some("No Flash"));

    // Not written by the DSC-F1: each RawConv returns undef, so the tag
    // must not exist rather than appear as a zero.
    assert_eq!(metadata.get("Sony:FNumber"), None);
    assert_eq!(metadata.get("Sony:ExposureCompensation"), None);
    assert_eq!(metadata.get("Sony:FocalLength"), None);
}

/// `t/images/DV.dv`, a 625/50 PAL DV stream.
///
/// Covers profile selection, the derived bit rate and duration, the VAUX
/// date/time/aspect scan and the audio DIF block.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn dv_fixture_matches_pinned_oracle() {
    let metadata = read_metadata(&fixture("DV.dv")).expect("read pinned DV fixture");

    assert_eq!(metadata.get_string("File:FileType"), Some("DV"));

    // VAUX date + time records.
    assert_eq!(
        metadata.get_string("DV:DateTimeOriginal"),
        Some("2010:02:16 21:36:28")
    );
    // Profile constants (DV.pm's second @dvProfiles entry).
    assert_eq!(metadata.get_integer("DV:ImageWidth"), Some(720));
    assert_eq!(metadata.get_integer("DV:ImageHeight"), Some(576));
    assert_eq!(
        metadata.get_string("DV:VideoFormat"),
        Some("IEC 61834 - 625/50 (PAL)")
    );
    assert_eq!(metadata.get_string("DV:Colorimetry"), Some("4:2:0"));
    assert_eq!(metadata.get_string("DV:FrameRate"), Some("25"));
    // 8 * FrameSize * FrameRate = 28,800,000 through ConvertBitrate.
    assert_eq!(metadata.get_string("DV:TotalBitrate"), Some("28.8 Mbps"));
    // FileSize / (FrameSize * FrameRate) through ConvertDuration.
    assert_eq!(metadata.get_string("DV:Duration"), Some("0.00 s"));
    // Video-control record.
    assert_eq!(metadata.get_string("DV:VideoScanType"), Some("Interlaced"));
    assert_eq!(metadata.get_string("DV:AspectRatio"), Some("16:9"));
    // Audio DIF block.
    assert_eq!(metadata.get_integer("DV:AudioChannels"), Some(4));
    assert_eq!(metadata.get_integer("DV:AudioSampleRate"), Some(32000));
    assert_eq!(metadata.get_integer("DV:AudioBitsPerSample"), Some(12));
}

/// `t/images/ZISRAW.czi`, a Zeiss CZI microscopy image.
///
/// Only the three `ZISRAW::Main` header fields are routed; the XML-derived
/// tags are deliberately absent (see `czi.rs`'s module docs on
/// `ShortenTagNames`). `MicroscopeName` is asserted absent so that a future
/// partial implementation of that chain cannot land unnoticed.
#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn czi_header_matches_pinned_oracle() {
    let metadata = read_metadata(&fixture("ZISRAW.czi")).expect("read pinned CZI fixture");

    assert_eq!(metadata.get_string("File:FileType"), Some("CZI"));
    // int32u[2] through `tr/ /./`.
    assert_eq!(metadata.get_string("File:ZISRAWVersion"), Some("1.0"));
    // undef[16] through `unpack("H*",$val)`.
    assert_eq!(
        metadata.get_string("File:PrimaryFileGUID"),
        Some("8fae1a521bc8714e97e12b82ec8fa652")
    );
    assert_eq!(
        metadata.get_string("File:FileGUID"),
        Some("8fae1a521bc8714e97e12b82ec8fa652")
    );

    // Deliberately not implemented.
    assert_eq!(metadata.get("XML:MicroscopeName"), None);
}
