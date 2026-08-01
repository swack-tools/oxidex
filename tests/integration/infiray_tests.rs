//! InfiRay IJPEG APPn records, end to end (`Image::ExifTool::InfiRay`).
//!
//! Cameras built on InfiRay's IJPEG SDK spread eight metadata records across
//! JPEG APP2 through APP9. None of the seven binary-data records carries an
//! identifier, so a wrong dispatch gate does not fail loudly -- it prints
//! confident values under real tag names. These tests replay the exact APPn
//! payloads of ExifTool's own `t/images/InfiRay.jpg` through the whole JPEG
//! reader and require every value to equal what ExifTool prints for that file.
//!
//! Ground truth, quoted byte for byte below, is
//! `exiftool -a -G0 -s InfiRay.jpg` (ExifTool 13.59). `-a` matters: without
//! it ExifTool suppresses a tag whenever a higher-priority group supplies the
//! same name, which would understate the expected set.

use oxidex::Metadata;
use oxidex::core::TagValue;
use std::io::Write;

/// `APP2_VERSION`: the 80-byte APP2 payload of ExifTool's
/// `t/images/InfiRay.jpg`, byte for byte -- the record ExifTool reads as
/// `%InfiRay::Version`.
const APP2_VERSION: &str = concat!(
    "00020001494a5045470001000401000001030000000000000000000000000000",
    "0080010000000000000000018001080000800100000000000000000180010800",
    "00800100000000000000000000000800"
);
/// `APP3_IMAGING_DATA`: the 20-byte APP3 payload of ExifTool's
/// `t/images/InfiRay.jpg`, byte for byte -- the record ExifTool reads as
/// `JPEG::Main APP3 ImagingData`.
const APP3_IMAGING_DATA: &str = "3c64756d6d7920696d6167696e6720646174613e";
/// `APP4_FACTORY`: the 231-byte APP4 payload of ExifTool's
/// `t/images/InfiRay.jpg`, byte for byte -- the record ExifTool reads as
/// `%InfiRay::Factory`.
const APP4_FACTORY: &str = concat!(
    "0001000080802c012c01200000000000000000000000000000000000da230000",
    "6d020000feff0000000000000000000000000000000000000000000000000000",
    "00000000851c8000800000000000000000000000000000000000000000000000",
    "0000000001010101000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000040000000c00000000018100040000009a",
    "02871999199d01"
);
/// `APP5_PICTURE`: the 42-byte APP5 payload of ExifTool's
/// `t/images/InfiRay.jpg`, byte for byte -- the record ExifTool reads as
/// `%InfiRay::Picture`.
const APP5_PICTURE: &str = concat!(
    "0000c8410000803ea4707d3f0000003f0000c841000000000000000000000000",
    "00010101000000000000"
);
/// `APP6_MIX_MODE`: the 192-byte APP6 payload of ExifTool's
/// `t/images/InfiRay.jpg`, byte for byte -- the record ExifTool reads as
/// `%InfiRay::MixMode`.
const APP6_MIX_MODE: &str = concat!(
    "000000803f000000400000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000"
);
/// `APP7_OP_MODE`: the 32-byte APP7 payload of ExifTool's
/// `t/images/InfiRay.jpg`, byte for byte -- the record ExifTool reads as
/// `%InfiRay::OpMode`.
const APP7_OP_MODE: &str = "0190010000f401000001010000c8410000000000000000000000000000000000";
/// `APP8_ISOTHERMAL`: the 32-byte APP8 payload of ExifTool's
/// `t/images/InfiRay.jpg`, byte for byte -- the record ExifTool reads as
/// `%InfiRay::Isothermal`.
const APP8_ISOTHERMAL: &str = "0000a0420000a0c10000a0420000a0c100000000000000000000000000000000";
/// `APP9_SENSOR`: the 768-byte APP9 payload of ExifTool's
/// `t/images/InfiRay.jpg`, byte for byte -- the record ExifTool reads as
/// `%InfiRay::Sensor`.
const APP9_SENSOR: &str = concat!(
    "696e666973656e73650000009d010000320036002e0037000321000000000000",
    "320037002e003300032100000000000080469487780000000000000000000000",
    "50325f5553425f49520000009d010000320036002e0037000321000000000000",
    "320037002e003300032100000000000080469487780000000000000000000000",
    "50325f425f56322e305f32303830313030303139423135363536313438000000",
    "4083feef770000008083feef770000000084307e770000006573006700000000",
    "5032303030313942313536353631343800303139423135363536313438000000",
    "4083feef770000008083feef770000000084307e770000006573006700000000",
    "322e30372e30312e303000009d010000320036002e0037000321000000000000",
    "320037002e003300032100000000000080469487780000000000000000000000",
    "cdcc8c3fcdcc4c40000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "696e666973656e73650000009d010000320036002e0037000321000000000000",
    "320037002e003300032100000000000080469487780000000000000000000000",
    "50325f5553425f49520000009d010000320036002e0037000321000000000000",
    "320037002e003300032100000000000080469487780000000000000000000000",
    "50325f425f56322e305f32303830313030303139423135363536313438000000",
    "4083feef770000008083feef770000000084307e770000006573006700000000",
    "5032303030313942313536353631343800303139423135363536313438000000",
    "4083feef770000008083feef770000000084307e770000006573006700000000",
    "322e30372e30312e303000009d010000320036002e0037000321000000000000",
    "320037002e003300032100000000000080469487780000000000000000000000",
    "cdcc8c3fcdcc4c40000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000"
);

/// Decodes one of the payload constants above.
fn hex(s: &str) -> Vec<u8> {
    assert!(
        s.len().is_multiple_of(2),
        "hex payload must have an even length"
    );
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex"))
        .collect()
}

/// Appends one APPn segment: marker, big-endian length-including-itself, data.
fn push_segment(jpeg: &mut Vec<u8>, marker: u8, data: &[u8]) {
    let length = u16::try_from(data.len() + 2).expect("segment fits in a JPEG length field");
    jpeg.extend_from_slice(&[0xFF, marker]);
    jpeg.extend_from_slice(&length.to_be_bytes());
    jpeg.extend_from_slice(data);
}

/// Builds a minimal JPEG carrying InfiRay.jpg's eight APPn records.
///
/// The image data is a synthetic 1x1 frame rather than the sample's, since
/// nothing here reads it; the APPn payloads are the sample's own bytes.
fn infiray_jpeg() -> Vec<u8> {
    let mut jpeg = vec![0xFF, 0xD8];
    push_segment(&mut jpeg, 0xE2, &hex(APP2_VERSION));
    push_segment(&mut jpeg, 0xE3, &hex(APP3_IMAGING_DATA));
    push_segment(&mut jpeg, 0xE4, &hex(APP4_FACTORY));
    push_segment(&mut jpeg, 0xE5, &hex(APP5_PICTURE));
    push_segment(&mut jpeg, 0xE6, &hex(APP6_MIX_MODE));
    push_segment(&mut jpeg, 0xE7, &hex(APP7_OP_MODE));
    push_segment(&mut jpeg, 0xE8, &hex(APP8_ISOTHERMAL));
    push_segment(&mut jpeg, 0xE9, &hex(APP9_SENSOR));
    // SOF0: 8-bit, 1x1, one component.
    push_segment(
        &mut jpeg,
        0xC0,
        &[0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00],
    );
    // SOS, then an empty scan and EOI.
    push_segment(&mut jpeg, 0xDA, &[0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
    jpeg.extend_from_slice(&[0xFF, 0xD9]);
    jpeg
}

/// Every APPn tag ExifTool 13.59 reports for `t/images/InfiRay.jpg`, with the
/// printed value exactly as `exiftool -a -G0 -s` renders it (PrintConv
/// included).
const EXPECTED: &[(&str, &str)] = &[
    ("APP2:IJPEGVersion", "0 2 0 1"),
    ("APP2:IJPEGOrgType", "4"),
    ("APP2:IJPEGDispType", "1"),
    ("APP2:IJPEGRotate", "0"),
    ("APP2:IJPEGMirrorFlip", "0"),
    ("APP2:ImageColorSwitchable", "1"),
    ("APP2:ThermalColorPalette", "3"),
    ("APP2:IRDataSize", "98304"),
    ("APP2:IRDataFormat", "0"),
    ("APP2:IRImageWidth", "256"),
    ("APP2:IRImageHeight", "384"),
    ("APP2:IRImageBpp", "8"),
    ("APP2:TempDataSize", "98304"),
    ("APP2:TempDataFormat", "0"),
    ("APP2:TempImageWidth", "256"),
    ("APP2:TempImageHeight", "384"),
    ("APP2:TempImageBpp", "8"),
    ("APP2:VisibleDataSize", "98304"),
    ("APP2:VisibleDataFormat", "0"),
    ("APP2:VisibleImageWidth", "0"),
    ("APP2:VisibleImageHeight", "0"),
    ("APP2:VisibleImageBpp", "8"),
    (
        "APP3:ImagingData",
        "(Binary data 20 bytes, use -b option to extract)",
    ),
    ("APP4:IJPEGTempVersion", "0 1 0 0"),
    ("APP4:FactDefEmissivity", "-128"),
    ("APP4:FactDefTau", "-128"),
    ("APP4:FactDefTa", "300"),
    ("APP4:FactDefTu", "300"),
    ("APP4:FactDefDist", "32"),
    ("APP4:FactDefA0", "0"),
    ("APP4:FactDefB0", "0"),
    ("APP4:FactDefA1", "0"),
    ("APP4:FactDefB1", "0"),
    ("APP4:FactDefP0", "9178"),
    ("APP4:FactDefP1", "621"),
    ("APP4:FactDefP2", "65534"),
    ("APP4:FactRelSensorTemp", "7301"),
    ("APP4:FactRelShutterTemp", "128"),
    ("APP4:FactRelLensTemp", "128"),
    ("APP4:FactStatusGain", "1"),
    ("APP4:FactStatusEnvOK", "1"),
    ("APP4:FactStatusDistOK", "1"),
    ("APP4:FactStatusTempMap", "1"),
    ("APP5:EnvironmentTemp", "25.00 C"),
    ("APP5:Distance", "0.25 m"),
    ("APP5:Emissivity", "0.99"),
    ("APP5:Humidity", "50.0 %"),
    ("APP5:ReferenceTemp", "25.00 C"),
    ("APP5:TempUnit", "0"),
    ("APP5:ShowCenterTemp", "1"),
    ("APP5:ShowMaxTemp", "1"),
    ("APP5:ShowMinTemp", "1"),
    ("APP5:TempMeasureCount", "0"),
    ("APP6:MixMode", "0"),
    ("APP6:FusionIntensity", "100.0 %"),
    ("APP6:OffsetAdjustment", "2"),
    (
        "APP6:CorrectionAsix",
        "0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
    ),
    ("APP7:WorkingMode", "1"),
    ("APP7:IntegralTime", "400"),
    ("APP7:IntegratTimeHdr", "500"),
    ("APP7:GainStable", "1"),
    ("APP7:TempControlEnable", "1"),
    ("APP7:DeviceTemp", "25.00 C"),
    ("APP8:IsothermalMax", "80"),
    ("APP8:IsothermalMin", "-20"),
    ("APP8:ChromaBarMax", "80"),
    ("APP8:ChromaBarMin", "-20"),
    ("APP9:IRSensorManufacturer", "infisense"),
    ("APP9:IRSensorName", "P2_USB_IR"),
    ("APP9:IRSensorPartNumber", "P2_B_V2.0_2080100019B15656148"),
    ("APP9:IRSensorSerialNumber", "P200019B15656148"),
    ("APP9:IRSensorFirmware", "2.07.01.00"),
    ("APP9:IRSensorAperture", "1.10"),
    ("APP9:IRFocalLength", "3.20"),
    ("APP9:VisibleSensorManufacturer", "infisense"),
    ("APP9:VisibleSensorName", "P2_USB_IR"),
    (
        "APP9:VisibleSensorPartNumber",
        "P2_B_V2.0_2080100019B15656148",
    ),
    ("APP9:VisibleSensorSerialNumber", "P200019B15656148"),
    ("APP9:VisibleSensorFirmware", "2.07.01.00"),
    ("APP9:VisibleSensorAperture", "1.10000002384186"),
    ("APP9:VisibleFocalLength", "3.20000004768372"),
];

fn read_infiray() -> Metadata {
    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    file.write_all(&infiray_jpeg()).expect("write jpeg");
    file.flush().expect("flush");
    Metadata::from_path(file.path()).expect("InfiRay JPEG parses")
}

/// The value as it reaches output, for the two variants these records emit.
///
/// Anything else is reported verbatim so a value stored in an unexpected
/// shape fails the comparison instead of quietly reading as absent.
fn printed(metadata: &Metadata, tag: &str) -> Option<String> {
    Some(match metadata.get(tag)? {
        TagValue::String(s) => s.clone(),
        TagValue::Integer(n) => n.to_string(),
        other => format!("<unexpected TagValue variant: {:?}>", other),
    })
}

#[test]
fn infiray_appn_records_match_exiftool() {
    let metadata = read_infiray();
    let mut wrong = Vec::new();
    for (tag, expected) in EXPECTED {
        match printed(&metadata, tag) {
            Some(ref actual) if actual == *expected => {}
            other => wrong.push(format!("{}: expected {:?}, got {:?}", tag, expected, other)),
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {} InfiRay tags differ from ExifTool:\n{}",
        wrong.len(),
        EXPECTED.len(),
        wrong.join("\n")
    );
}

#[test]
fn no_extra_appn_tags_are_invented() {
    // A record read at the wrong offset, or a table wired to the wrong marker,
    // shows up here as a name ExifTool never prints for this file.
    let metadata = read_infiray();
    let expected: std::collections::HashSet<&str> = EXPECTED.iter().map(|(t, _)| *t).collect();
    let extra: Vec<&String> = metadata
        .iter()
        .map(|(k, _)| k)
        .filter(|k| k.starts_with("APP") && !expected.contains(k.as_str()))
        .collect();
    assert!(extra.is_empty(), "unexpected APPn tags: {:?}", extra);
}

#[test]
fn the_records_are_gated_on_the_app2_ijpeg_header() {
    // The seven binary records have no identifier of their own. ExifTool reads
    // them only after an APP2 segment matching /^....IJPEG\0/s has set
    // $$self{HasIJPEG}; without it the same bytes must yield nothing, or an
    // unrelated APP4 or APP7 would be decoded as InfiRay data.
    let mut jpeg = vec![0xFF, 0xD8];
    push_segment(&mut jpeg, 0xE4, &hex(APP4_FACTORY));
    push_segment(&mut jpeg, 0xE5, &hex(APP5_PICTURE));
    push_segment(&mut jpeg, 0xE7, &hex(APP7_OP_MODE));
    push_segment(&mut jpeg, 0xE9, &hex(APP9_SENSOR));
    push_segment(
        &mut jpeg,
        0xC0,
        &[0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00],
    );
    push_segment(&mut jpeg, 0xDA, &[0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
    jpeg.extend_from_slice(&[0xFF, 0xD9]);

    let mut file = tempfile::NamedTempFile::new().expect("temp file");
    file.write_all(&jpeg).expect("write jpeg");
    file.flush().expect("flush");
    let metadata = Metadata::from_path(file.path()).expect("JPEG parses");

    let infiray: Vec<&String> = metadata
        .iter()
        .map(|(k, _)| k)
        .filter(|k| {
            k.starts_with("APP4:")
                || k.starts_with("APP5:")
                || k.starts_with("APP7:")
                || k.starts_with("APP9:")
        })
        .collect();
    assert!(infiray.is_empty(), "read without HasIJPEG: {:?}", infiray);
}
