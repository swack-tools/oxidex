use oxidex::core::operations::read_metadata;
use oxidex::parsers::jpeg::app_segments::dji_dbg::{
    parse_dji_dbg_app7, parse_dji_info_records_in_group,
};

const DJI_INFO_NAMES: &[(&[u8], &str)] = &[
    (b"ae_dbg_info", "AEDebugInfo"),
    (b"ae_histogram_info", "AEHistogramInfo"),
    (b"ae_local_histogram", "AELocalHistogram"),
    (b"ae_liveview_histogram_info", "AELiveViewHistogramInfo"),
    (b"ae_liveview_local_histogram", "AELiveViewLocalHistogram"),
    (b"awb_dbg_info", "AWBDebugInfo"),
    (b"af_dbg_info", "AFDebugInfo"),
    (b"hiso", "Histogram"),
    (b"xidiri", "Xidiri"),
    (b"GimbalDegree(Y,P,R)", "GimbalDegree"),
    (b"FlightDegree(Y,P,R)", "FlightDegree"),
    (b"adj_dbg_info", "ADJDebugInfo"),
    (b"sensor_id", "SensorID"),
    (b"FlightSpeed(X,Y,Z)", "FlightSpeed"),
    (b"hyperlapse_dbg_info", "HyperlapsDebugInfo"),
];

fn jpeg_with_app7(payload: &[u8]) -> Vec<u8> {
    let mut jpeg = vec![0xff, 0xd8];
    jpeg.extend_from_slice(&[0xff, 0xe7]);
    jpeg.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
    jpeg.extend_from_slice(payload);
    jpeg.extend_from_slice(&[0xff, 0xd9]);
    jpeg
}

fn tiff_with_makernote(make: &str, payload: &[u8]) -> Vec<u8> {
    const EXIF_IFD: usize = 38;
    const DATA_START: usize = 56;
    let make = [make.as_bytes(), b"\0"].concat();
    let make_is_inline = make.len() <= 4;
    let make_offset = DATA_START;
    let makernote_offset = DATA_START + usize::from(!make_is_inline) * make.len();
    let mut tiff = vec![0_u8; DATA_START];
    tiff[..8].copy_from_slice(b"II\x2a\0\x08\0\0\0");

    tiff[8..10].copy_from_slice(&2_u16.to_le_bytes());
    tiff[10..12].copy_from_slice(&0x010f_u16.to_le_bytes());
    tiff[12..14].copy_from_slice(&2_u16.to_le_bytes());
    tiff[14..18].copy_from_slice(&u32::try_from(make.len()).unwrap().to_le_bytes());
    if make_is_inline {
        tiff[18..18 + make.len()].copy_from_slice(&make);
    } else {
        tiff[18..22].copy_from_slice(&u32::try_from(make_offset).unwrap().to_le_bytes());
    }
    tiff[22..24].copy_from_slice(&0x8769_u16.to_le_bytes());
    tiff[24..26].copy_from_slice(&4_u16.to_le_bytes());
    tiff[26..30].copy_from_slice(&1_u32.to_le_bytes());
    tiff[30..34].copy_from_slice(&(EXIF_IFD as u32).to_le_bytes());

    tiff[EXIF_IFD..EXIF_IFD + 2].copy_from_slice(&1_u16.to_le_bytes());
    tiff[EXIF_IFD + 2..EXIF_IFD + 4].copy_from_slice(&0x927c_u16.to_le_bytes());
    tiff[EXIF_IFD + 4..EXIF_IFD + 6].copy_from_slice(&7_u16.to_le_bytes());
    tiff[EXIF_IFD + 6..EXIF_IFD + 10]
        .copy_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    tiff[EXIF_IFD + 10..EXIF_IFD + 14]
        .copy_from_slice(&u32::try_from(makernote_offset).unwrap().to_le_bytes());
    if !make_is_inline {
        tiff.extend_from_slice(&make);
    }
    tiff.extend_from_slice(payload);
    tiff
}

fn tiff_with_makernote_without_make(payload: &[u8]) -> Vec<u8> {
    const EXIF_IFD: usize = 26;
    const MAKERNOTE: usize = 44;
    let mut tiff = vec![0_u8; MAKERNOTE];
    tiff[..8].copy_from_slice(b"II\x2a\0\x08\0\0\0");

    tiff[8..10].copy_from_slice(&1_u16.to_le_bytes());
    tiff[10..12].copy_from_slice(&0x8769_u16.to_le_bytes());
    tiff[12..14].copy_from_slice(&4_u16.to_le_bytes());
    tiff[14..18].copy_from_slice(&1_u32.to_le_bytes());
    tiff[18..22].copy_from_slice(&(EXIF_IFD as u32).to_le_bytes());

    tiff[EXIF_IFD..EXIF_IFD + 2].copy_from_slice(&1_u16.to_le_bytes());
    tiff[EXIF_IFD + 2..EXIF_IFD + 4].copy_from_slice(&0x927c_u16.to_le_bytes());
    tiff[EXIF_IFD + 4..EXIF_IFD + 6].copy_from_slice(&7_u16.to_le_bytes());
    tiff[EXIF_IFD + 6..EXIF_IFD + 10]
        .copy_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    tiff[EXIF_IFD + 10..EXIF_IFD + 14]
        .copy_from_slice(&u32::try_from(MAKERNOTE).unwrap().to_le_bytes());
    tiff.extend_from_slice(payload);
    tiff
}

fn read_fixture(bytes: &[u8], suffix: &str) -> oxidex::core::MetadataMap {
    let file = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("create synthetic DJI carrier");
    std::fs::write(file.path(), bytes).expect("write synthetic DJI carrier");
    read_metadata(file.path()).expect("synthetic DJI carrier parses")
}

/// ExifTool 13.59 selects DJI::Info for APP7 `DJI-DBG\0` and exposes the
/// bracketed `sensor_id` record unchanged.
#[test]
fn dji_app7_sensor_id_carrier_matches_exiftool() {
    let metadata = read_fixture(
        &jpeg_with_app7(b"DJI-DBG\0[sensor_id:4XAGJCP02AA007]"),
        ".jpg",
    );

    assert_eq!(metadata.get_string("DJI:SensorID"), Some("4XAGJCP02AA007"));
    assert!(metadata.get("APP7:SensorID").is_none());
}

#[test]
fn dji_info_table_covers_all_fifteen_pinned_names() {
    for &(source_name, output_name) in DJI_INFO_NAMES {
        let mut record = Vec::with_capacity(source_name.len() + 8);
        record.push(b'[');
        record.extend_from_slice(source_name);
        record.extend_from_slice(b":value]");
        let metadata = parse_dji_info_records_in_group(&record, "DJI");
        assert_eq!(
            metadata.get_string(&format!("DJI:{output_name}")),
            Some("value"),
            "missing DJI::Info mapping for {}",
            String::from_utf8_lossy(source_name)
        );
    }
}

#[test]
fn dji_app7_carrier_exposes_an_extended_diagnostic_name() {
    let metadata = read_fixture(
        &jpeg_with_app7(b"DJI-DBG\0[awb_dbg_info:white-balance]"),
        ".jpg",
    );

    assert_eq!(
        metadata.get_string("DJI:AWBDebugInfo"),
        Some("white-balance")
    );
    assert!(metadata.get("APP7:AWBDebugInfo").is_none());
}

#[test]
fn dji_app7_public_reader_emits_all_fifteen_names_in_the_dji_group() {
    let mut payload = b"DJI-DBG\0".to_vec();
    for &(source_name, _) in DJI_INFO_NAMES {
        payload.push(b'[');
        payload.extend_from_slice(source_name);
        payload.extend_from_slice(b":value]");
    }
    let metadata = read_fixture(&jpeg_with_app7(&payload), ".jpg");

    for &(_, output_name) in DJI_INFO_NAMES {
        assert_eq!(
            metadata.get_string(&format!("DJI:{output_name}")),
            Some("value"),
            "missing public DJI:{output_name}"
        );
        assert!(
            metadata.get(&format!("APP7:{output_name}")).is_none(),
            "APP7:{output_name} is not a source-compatible family-1 identity"
        );
    }
}

#[test]
fn dji_info_makernote_uses_the_dji_group() {
    let payload = b"[ae_dbg_info:exposure][ae_histogram_info:histogram]";
    let metadata = read_fixture(&tiff_with_makernote("DJI", payload), ".tif");

    assert_eq!(metadata.get_string("DJI:AEDebugInfo"), Some("exposure"));
    assert_eq!(
        metadata.get_string("DJI:AEHistogramInfo"),
        Some("histogram")
    );
    assert!(metadata.get("APP7:AEHistogramInfo").is_none());
}

#[test]
fn dji_info_makernote_selector_is_independent_of_make() {
    let metadata = read_fixture(
        &tiff_with_makernote("ACME", b"[ae_dbg_info:exposure]"),
        ".tif",
    );

    assert_eq!(metadata.get_string("DJI:AEDebugInfo"), Some("exposure"));
}

#[test]
fn dji_info_signature_precedes_empty_and_make_only_routing() {
    for make in ["", "SIGMA", "FOVEON"] {
        let metadata = read_fixture(
            &tiff_with_makernote(make, b"[ae_dbg_info:exposure]"),
            ".tif",
        );

        assert_eq!(
            metadata.get_string("DJI:AEDebugInfo"),
            Some("exposure"),
            "exact DJI Info signature must win for Make={make:?}"
        );
    }

    let missing_make = read_fixture(
        &tiff_with_makernote_without_make(b"[ae_dbg_info:exposure]"),
        ".tif",
    );
    assert_eq!(
        missing_make.get_string("DJI:AEDebugInfo"),
        Some("exposure"),
        "exact DJI Info signature must win when Make is absent"
    );
}

#[test]
fn dji_make_does_not_select_info_without_the_exact_payload_signature() {
    let metadata = read_fixture(&tiff_with_makernote("DJI", b"[awb_dbg_info:white]"), ".tif");

    assert!(metadata.get("DJI:AWBDebugInfo").is_none());
}

/// ExifTool 13.59's `DJI::Info` table maps the APP7 diagnostic key
/// `FlightSpeed(X,Y,Z)` to `FlightSpeed` without trying to interpret its
/// comma-separated payload.  The main-side mapping was absent after the
/// app-segment parser was forward-ported.
#[test]
fn dji_app7_flight_speed_record_is_preserved_verbatim() {
    let metadata = parse_dji_dbg_app7(b"DJI-DBG\0[FlightSpeed(X,Y,Z):9,0,0]");

    assert_eq!(metadata.get_string("DJI:FlightSpeed"), Some("9,0,0"));
}
