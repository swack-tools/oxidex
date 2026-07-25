//! Integration tests for tag family normalization
//!
//! These tests verify that tag families are normalized to match ExifTool conventions.
//!
//! NOTE: The core library preserves ExifIFD, GPS, IFD0, Canon, etc. as-is to match
//! Perl ExifTool's default output format. Comparison-specific normalization
//! (ExifIFD→EXIF, GPS→EXIF, Canon→MakerNotes) is handled separately in the
//! tag-comparison binary's oxidex_extractor module.

use oxidex::core::tag_normalization::normalize_tag_family;
use oxidex::core::{MetadataMap, TagValue};

#[test]
fn test_exififd_unchanged() {
    // ExifIFD remains unchanged to match Perl ExifTool output
    assert_eq!(normalize_tag_family("ExifIFD:Make"), "ExifIFD:Make");
    assert_eq!(normalize_tag_family("ExifIFD:Model"), "ExifIFD:Model");
    assert_eq!(
        normalize_tag_family("ExifIFD:DateTimeOriginal"),
        "ExifIFD:DateTimeOriginal"
    );
    assert_eq!(normalize_tag_family("ExifIFD:ISO"), "ExifIFD:ISO");
    assert_eq!(normalize_tag_family("ExifIFD:FNumber"), "ExifIFD:FNumber");
}

#[test]
fn test_ifd0_unchanged() {
    assert_eq!(normalize_tag_family("IFD0:Make"), "IFD0:Make");
    assert_eq!(normalize_tag_family("IFD0:Model"), "IFD0:Model");
    assert_eq!(normalize_tag_family("IFD0:Orientation"), "IFD0:Orientation");
}

#[test]
fn test_ifd1_unchanged() {
    assert_eq!(normalize_tag_family("IFD1:Compression"), "IFD1:Compression");
    assert_eq!(normalize_tag_family("IFD1:ImageWidth"), "IFD1:ImageWidth");
}

#[test]
fn test_gps_unchanged() {
    // GPS tags remain unchanged to match Perl ExifTool output
    assert_eq!(normalize_tag_family("GPS:GPSLatitude"), "GPS:GPSLatitude");
    assert_eq!(normalize_tag_family("GPS:GPSLongitude"), "GPS:GPSLongitude");
    assert_eq!(normalize_tag_family("GPS:GPSAltitude"), "GPS:GPSAltitude");
    assert_eq!(
        normalize_tag_family("GPS:GPSAltitudeRef"),
        "GPS:GPSAltitudeRef"
    );
    assert_eq!(normalize_tag_family("GPS:GPSDateStamp"), "GPS:GPSDateStamp");
    assert_eq!(normalize_tag_family("GPS:GPSDOP"), "GPS:GPSDOP");
    assert_eq!(normalize_tag_family("GPS:GPSTimeStamp"), "GPS:GPSTimeStamp");
}

#[test]
fn test_makernotes_unchanged() {
    // Canon
    assert_eq!(normalize_tag_family("Canon:LensModel"), "Canon:LensModel");
    assert_eq!(normalize_tag_family("Canon:MacroMode"), "Canon:MacroMode");

    // Nikon
    assert_eq!(
        normalize_tag_family("Nikon:ShutterCount"),
        "Nikon:ShutterCount"
    );
    assert_eq!(normalize_tag_family("Nikon:LensType"), "Nikon:LensType");

    // Sony
    assert_eq!(normalize_tag_family("Sony:SonyModelID"), "Sony:SonyModelID");

    // Fujifilm
    assert_eq!(
        normalize_tag_family("Fujifilm:FilmMode"),
        "Fujifilm:FilmMode"
    );

    // Panasonic
    assert_eq!(
        normalize_tag_family("Panasonic:Quality"),
        "Panasonic:Quality"
    );
}

#[test]
fn test_unknown_family_unchanged() {
    assert_eq!(normalize_tag_family("Custom:Tag"), "Custom:Tag");
    assert_eq!(normalize_tag_family("Unknown:Field"), "Unknown:Field");
    assert_eq!(normalize_tag_family("MyApp:Data"), "MyApp:Data");
}

#[test]
fn test_no_colon_unchanged() {
    assert_eq!(normalize_tag_family("NoColonHere"), "NoColonHere");
    assert_eq!(normalize_tag_family("SimpleTag"), "SimpleTag");
    assert_eq!(normalize_tag_family("JustAName"), "JustAName");
}

#[test]
fn test_empty_string() {
    assert_eq!(normalize_tag_family(""), "");
}

#[test]
fn test_interop_ifd_unchanged() {
    assert_eq!(
        normalize_tag_family("InteropIFD:InteropIndex"),
        "InteropIFD:InteropIndex"
    );
}

#[test]
fn test_profile_to_icc_profile() {
    // Profile -> ICC_Profile is the only family mapping that changes the family name
    assert_eq!(
        normalize_tag_family("Profile:ColorSpaceData"),
        "ICC_Profile:ColorSpaceData"
    );
    assert_eq!(
        normalize_tag_family("Profile:CMMFlags"),
        "ICC_Profile:CMMFlags"
    );
    assert_eq!(
        normalize_tag_family("Profile:ProfileVersion"),
        "ICC_Profile:ProfileVersion"
    );
}

#[test]
fn test_trc_name_normalization() {
    // ToneReproductionCurve tags should be shortened to TRC (within ICC_Profile family)
    assert_eq!(
        normalize_tag_family("Profile:BlueToneReproductionCurve"),
        "ICC_Profile:BlueTRC"
    );
    assert_eq!(
        normalize_tag_family("Profile:GreenToneReproductionCurve"),
        "ICC_Profile:GreenTRC"
    );
    assert_eq!(
        normalize_tag_family("Profile:RedToneReproductionCurve"),
        "ICC_Profile:RedTRC"
    );
    assert_eq!(
        normalize_tag_family("Profile:GrayToneReproductionCurve"),
        "ICC_Profile:GrayTRC"
    );
}

#[test]
fn test_normalize_metadata_map_preserves_families() {
    let mut map = MetadataMap::new();
    map.insert("ExifIFD:Make", TagValue::new_string("Canon"));
    map.insert("ExifIFD:Model", TagValue::new_string("EOS R5"));
    map.insert("ExifIFD:ISO", TagValue::new_integer(400));
    map.insert("ExifIFD:FNumber", TagValue::new_float(2.8));

    let normalized = oxidex::core::tag_normalization::normalize_metadata_map(&map);

    // ExifIFD should remain unchanged
    assert_eq!(normalized.get_string("ExifIFD:Make"), Some("Canon"));
    assert_eq!(normalized.get_string("ExifIFD:Model"), Some("EOS R5"));
    assert_eq!(normalized.get_integer("ExifIFD:ISO"), Some(400));
    assert_eq!(normalized.get_float("ExifIFD:FNumber"), Some(2.8));

    // Verify we have the same number of tags
    assert_eq!(normalized.len(), map.len());
}

#[test]
fn test_normalize_metadata_map_mixed_families() {
    let mut map = MetadataMap::new();
    map.insert("ExifIFD:Make", TagValue::new_string("Canon"));
    map.insert("ExifIFD:Model", TagValue::new_string("EOS R5"));
    map.insert("IFD0:Software", TagValue::new_string("OxiDex"));
    map.insert("GPS:GPSLatitude", TagValue::new_string("37.7749"));
    map.insert("Canon:LensModel", TagValue::new_string("EF 24-70mm"));
    map.insert("File:FileSize", TagValue::new_integer(1024000));
    map.insert("Profile:ColorSpaceData", TagValue::new_string("sRGB"));

    let normalized = oxidex::core::tag_normalization::normalize_metadata_map(&map);

    // ExifIFD, GPS, IFD0, Canon should all remain unchanged
    assert_eq!(normalized.get_string("ExifIFD:Make"), Some("Canon"));
    assert_eq!(normalized.get_string("ExifIFD:Model"), Some("EOS R5"));
    assert_eq!(normalized.get_string("GPS:GPSLatitude"), Some("37.7749"));
    assert_eq!(normalized.get_string("IFD0:Software"), Some("OxiDex"));
    assert_eq!(normalized.get_string("Canon:LensModel"), Some("EF 24-70mm"));
    assert_eq!(normalized.get_integer("File:FileSize"), Some(1024000));

    // Profile -> ICC_Profile should be normalized
    assert_eq!(
        normalized.get_string("ICC_Profile:ColorSpaceData"),
        Some("sRGB")
    );
    assert!(normalized.get("Profile:ColorSpaceData").is_none());

    // Verify we have the same number of tags
    assert_eq!(normalized.len(), map.len());
}

#[test]
fn test_normalize_empty_map() {
    let map = MetadataMap::new();
    let normalized = oxidex::core::tag_normalization::normalize_metadata_map(&map);
    assert_eq!(normalized.len(), 0);
    assert!(normalized.is_empty());
}

#[test]
fn test_normalize_preserves_all_value_types() {
    let mut map = MetadataMap::new();
    map.insert("ExifIFD:Make", TagValue::new_string("Canon"));
    map.insert("ExifIFD:ISO", TagValue::new_integer(400));
    map.insert("ExifIFD:FNumber", TagValue::new_float(2.8));
    map.insert("ExifIFD:ExposureTime", TagValue::new_rational(1, 125));
    map.insert(
        "ExifIFD:ThumbnailImage",
        TagValue::new_binary(vec![0xFF, 0xD8, 0xFF, 0xE0]),
    );

    let normalized = oxidex::core::tag_normalization::normalize_metadata_map(&map);

    // Verify all value types are preserved (ExifIFD remains unchanged)
    assert_eq!(normalized.get_string("ExifIFD:Make"), Some("Canon"));
    assert_eq!(normalized.get_integer("ExifIFD:ISO"), Some(400));
    assert_eq!(normalized.get_float("ExifIFD:FNumber"), Some(2.8));

    // Check rational
    if let Some(TagValue::Rational {
        numerator,
        denominator,
    }) = normalized.get("ExifIFD:ExposureTime")
    {
        assert_eq!(*numerator, 1);
        assert_eq!(*denominator, 125);
    } else {
        panic!("Expected rational value for ExposureTime");
    }

    // Check binary
    if let Some(TagValue::Binary(data)) = normalized.get("ExifIFD:ThumbnailImage") {
        assert_eq!(data.len(), 4);
        assert_eq!(data[0], 0xFF);
        assert_eq!(data[1], 0xD8);
    } else {
        panic!("Expected binary value for ThumbnailImage");
    }

    assert_eq!(normalized.len(), map.len());
}

#[test]
fn test_case_sensitivity() {
    // ExifTool tag families are case-sensitive
    assert_eq!(normalize_tag_family("ExifIFD:Make"), "ExifIFD:Make");
    assert_eq!(normalize_tag_family("exififd:Make"), "exififd:Make"); // lowercase unchanged
    assert_eq!(normalize_tag_family("EXIFIFD:Make"), "EXIFIFD:Make"); // uppercase unchanged
}

#[test]
fn test_multiple_colons() {
    // Edge case: multiple colons - only split on first
    assert_eq!(
        normalize_tag_family("ExifIFD:Some:Complex:Tag"),
        "ExifIFD:Some:Complex:Tag"
    );
    assert_eq!(
        normalize_tag_family("Profile:Some:Complex:Tag"),
        "ICC_Profile:Some:Complex:Tag"
    );
}

#[test]
fn test_tag_with_special_characters() {
    // Tags can have special characters in the name
    assert_eq!(
        normalize_tag_family("ExifIFD:Tag-With-Dashes"),
        "ExifIFD:Tag-With-Dashes"
    );
    assert_eq!(
        normalize_tag_family("ExifIFD:Tag_With_Underscores"),
        "ExifIFD:Tag_With_Underscores"
    );
    assert_eq!(
        normalize_tag_family("ExifIFD:Tag.With.Dots"),
        "ExifIFD:Tag.With.Dots"
    );
}
