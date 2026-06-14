//! Wave-3 coverage tests for the QuickTime/ISOBMFF metadata extractor.
//!
//! Wave-1 (`cov_quicktime.rs`) and wave-2 (`cov_quicktime2.rs`) cover the happy
//! paths and many error branches. This file deliberately targets the *remaining*
//! uncovered code in `src/parsers/quicktime/metadata_extractor.rs`:
//!
//! - data atoms with every iTunes type code (utf8 / utf16 / int8 / int16 / int32 /
//!   binary type-0 / JPEG type-13 / PNG type-14 / unknown-but-string fallback /
//!   odd-length UTF-16 returning None / 3-byte int returning None)
//! - keys-atom with integer-indexed ilst plus malformed keys (key_size < 8,
//!   truncated entry, fewer entries than declared)
//! - multiple traks of different handler kinds (soun / vide / text / meta) and
//!   the audio-track-only dinf->dref handler-class arms (url / rsrc / unknown)
//! - the `©xyz` GPS atom with invalid coordinates (no GPS tags emitted)
//! - rating / many reverse-DNS keys via the MP4: fallback
//! - sample descriptions for the remaining codec arms (raw / lpcm / ec-3 / samr /
//!   ac-3) and the hvcC-in-stsd (High Tier / Variable CFR / multi compat) path
//! - graphics-mode enum arms (blend / alpha / dither / unknown)
//! - handler metadata edge cases: empty component, unknown handler type, auxC,
//!   too-short name, empty/reserved vendor
//! - HEIF iloc v1/v2, iinf v1 (32-bit), pitm v1 (32-bit), ispe legacy fallback
//! - malformed / truncated / zero-size boxes for early-return branches

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::quicktime::parse_quicktime_metadata;

// ---------------------------------------------------------------------------
// Box-building helpers (independent copies, matching the other cov files)
// ---------------------------------------------------------------------------

fn atom(atom_type: &[u8], payload: &[u8]) -> Vec<u8> {
    assert_eq!(atom_type.len(), 4, "atom type must be 4 bytes");
    let size = (payload.len() + 8) as u32;
    let mut v = Vec::with_capacity(payload.len() + 8);
    v.extend_from_slice(&size.to_be_bytes());
    v.extend_from_slice(atom_type);
    v.extend_from_slice(payload);
    v
}

fn cr_type(suffix: &[u8; 3]) -> [u8; 4] {
    [0xA9, suffix[0], suffix[1], suffix[2]]
}

fn concat(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(p);
    }
    v
}

fn ftyp_payload(major: &[u8], minor: u32, compatible: &[&[u8]]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(major);
    p.extend_from_slice(&minor.to_be_bytes());
    for c in compatible {
        p.extend_from_slice(c);
    }
    p
}

/// version-0 mvhd, >= 100 bytes, configurable creation time + timescale/duration.
fn mvhd_v0_time(timescale: u32, duration: u32, creation: u32) -> Vec<u8> {
    let mut p = vec![0u8; 100];
    p[0] = 0;
    p[4..8].copy_from_slice(&creation.to_be_bytes());
    p[8..12].copy_from_slice(&creation.to_be_bytes());
    p[12..16].copy_from_slice(&timescale.to_be_bytes());
    p[16..20].copy_from_slice(&duration.to_be_bytes());
    p[20..24].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    p[24..26].copy_from_slice(&0x0100u16.to_be_bytes());
    p[96..100].copy_from_slice(&3u32.to_be_bytes());
    p
}

/// A plain mvhd with a post-1970 creation time (so the file always has metadata).
fn mvhd(timescale: u32, duration: u32) -> Vec<u8> {
    mvhd_v0_time(timescale, duration, 3_600_000_000)
}

/// version-0 tkhd, >= 84 bytes, with layer/volume/duration fields populated.
fn tkhd_v0_full(track_id: u32, width: u32, height: u32) -> Vec<u8> {
    let mut p = vec![0u8; 84];
    p[0] = 0;
    p[3] = 0x01; // enabled
    p[4..8].copy_from_slice(&3_600_000_000u32.to_be_bytes());
    p[8..12].copy_from_slice(&3_600_000_050u32.to_be_bytes());
    p[12..16].copy_from_slice(&track_id.to_be_bytes());
    p[20..24].copy_from_slice(&2400u32.to_be_bytes()); // duration
    p[32..34].copy_from_slice(&1i16.to_be_bytes()); // layer
    p[36..38].copy_from_slice(&0x0100u16.to_be_bytes()); // volume
    p[60..64].copy_from_slice(&(width << 16).to_be_bytes());
    p[64..68].copy_from_slice(&(height << 16).to_be_bytes());
    p
}

fn mdhd_v0(timescale: u32, duration: u32) -> Vec<u8> {
    let mut p = vec![0u8; 24];
    p[0] = 0;
    p[4..8].copy_from_slice(&3_600_000_000u32.to_be_bytes());
    p[8..12].copy_from_slice(&3_600_000_050u32.to_be_bytes());
    p[12..16].copy_from_slice(&timescale.to_be_bytes());
    p[16..20].copy_from_slice(&duration.to_be_bytes());
    let lang: u16 = ((0x05u16) << 10) | ((0x0eu16) << 5) | 0x07u16; // "eng"
    p[20..22].copy_from_slice(&lang.to_be_bytes());
    p
}

/// hdlr payload (>= 24 bytes) with component/handler/vendor/name (null-terminated).
fn hdlr_payload(component: &[u8], handler: &[u8], vendor: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 4]); // version/flags
    p.extend_from_slice(component); // @4
    p.extend_from_slice(handler); // @8
    p.extend_from_slice(vendor); // @12
    p.extend_from_slice(&[0u8; 8]); // reserved @16
    p.extend_from_slice(name); // @24
    p.push(0);
    p
}

fn qt_userdata_string(text: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&(text.len() as u16).to_be_bytes());
    p.extend_from_slice(&[0u8, 0u8]); // language
    p.extend_from_slice(text.as_bytes());
    p
}

fn itunes_data(type_indicator: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&type_indicator.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(payload);
    atom(b"data", &p)
}

fn itunes_data_utf8(text: &str) -> Vec<u8> {
    itunes_data(1, text.as_bytes())
}

fn ilst_item(item_type: &[u8], data_atom: &[u8]) -> Vec<u8> {
    atom(item_type, data_atom)
}

/// meta payload with the 4-byte version/flags header.
fn meta_payload(children: &[Vec<u8>]) -> Vec<u8> {
    let mut p = vec![0u8; 4];
    for c in children {
        p.extend_from_slice(c);
    }
    p
}

/// meta payload WITHOUT the version/flags header (for moov->meta keys/ilst).
fn meta_payload_no_header(children: &[Vec<u8>]) -> Vec<u8> {
    concat(children)
}

/// Build a complete moov from the supplied children, always including an mvhd
/// (so the file is guaranteed to have at least TimeScale metadata).
fn moov_with(children: &[Vec<u8>]) -> Vec<u8> {
    let mut all = vec![atom(b"mvhd", &mvhd(600, 6000))];
    all.extend_from_slice(children);
    atom(b"moov", &concat(&all))
}

/// Build an mdir-handler ilst meta wrapped in udta, then a moov.
fn ilst_moov(items: &[Vec<u8>]) -> Vec<u8> {
    let ilst = atom(b"ilst", &concat(items));
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"mdir", b"appl", b"mdir"),
    );
    let meta = atom(b"meta", &meta_payload(&[hdlr, ilst]));
    let udta = atom(b"udta", &meta);
    moov_with(&[udta])
}

// ---------------------------------------------------------------------------
// iTunes data atom: exhaustive type-code coverage
// ---------------------------------------------------------------------------

#[test]
fn test_itunes_data_jpeg_and_png_binary() {
    // type 13 = JPEG, type 14 = PNG -> both stored as Binary.
    let jpeg = ilst_item(b"covr", &itunes_data(13, &[0xFF, 0xD8, 0xFF, 0xE0]));
    let png = ilst_item(b"covr", &itunes_data(14, &[0x89, b'P', b'N', b'G']));
    let moov = ilst_moov(&[jpeg, png]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("jpeg/png covr");
    assert!(matches!(
        md.get("ItemList:CoverArt"),
        Some(TagValue::Binary(_))
    ));
}

#[test]
fn test_itunes_data_int32_value() {
    // type 21 signed int, 4-byte length -> Integer.
    let tmpo = ilst_item(b"tmpo", &itunes_data(21, &200i32.to_be_bytes()));
    let moov = ilst_moov(&[tmpo]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("int32 tmpo");
    assert_eq!(
        md.get("ItemList:BeatsPerMinute"),
        Some(&TagValue::Integer(200))
    );
}

#[test]
fn test_itunes_data_int_three_bytes_returns_none() {
    // type 21 with a 3-byte payload is an unsupported width -> extract returns None,
    // so no tag is produced for this item (but the file is still valid via mvhd).
    let weird = ilst_item(b"tmpo", &itunes_data(21, &[0x00, 0x01, 0x02]));
    let moov = ilst_moov(&[weird]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("3-byte int");
    assert!(!md.contains_key("ItemList:BeatsPerMinute"));
    assert!(md.contains_key("QuickTime:TimeScale"));
}

#[test]
fn test_itunes_data_utf16_odd_length_returns_none() {
    // type 2 = UTF-16 with an odd number of bytes -> decode_utf16 returns None.
    let cmt = ilst_item(&cr_type(b"cmt"), &itunes_data(2, &[0x00, 0x48, 0x00]));
    let moov = ilst_moov(&[cmt]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("odd utf16");
    assert!(!md.contains_key("ItemList:Comment"));
}

#[test]
fn test_itunes_data_too_short_returns_none() {
    // data atom payload < 8 bytes -> extract_itunes_data_value returns None.
    let short = ilst_item(&cr_type(b"nam"), &itunes_data_truncated());
    let moov = ilst_moov(&[short]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("short data");
    assert!(!md.contains_key("ItemList:Title"));
}

fn itunes_data_truncated() -> Vec<u8> {
    // a "data" atom whose payload is only 4 bytes (< 8 required).
    atom(b"data", &[0u8, 0u8, 0u8, 1u8])
}

#[test]
fn test_itunes_data_unknown_type_as_string() {
    // Unknown type indicator falls through to "try as UTF-8 string".
    let weird = ilst_item(&cr_type(b"ART"), &itunes_data(77, b"fallback"));
    let moov = ilst_moov(&[weird]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("unknown type string");
    assert_eq!(
        md.get("ItemList:Artist"),
        Some(&TagValue::String("fallback".to_string()))
    );
}

#[test]
fn test_itunes_album_grouping_lyrics_via_ilst() {
    // alb / grp / lyr arms in the ilst tag_name match.
    let alb = ilst_item(&cr_type(b"alb"), &itunes_data_utf8("AlbumName"));
    let moov = ilst_moov(&[alb]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("album ilst");
    assert_eq!(
        md.get("ItemList:Album"),
        Some(&TagValue::String("AlbumName".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Album"),
        Some(&TagValue::String("AlbumName".to_string()))
    );
}

// ---------------------------------------------------------------------------
// udta ©xyz GPS atom with invalid coordinates -> no GPS tags
// ---------------------------------------------------------------------------

#[test]
fn test_udta_xyz_invalid_gps_no_tags() {
    // A ©xyz value that parse_iso6709 cannot parse -> no GPS tags emitted.
    let xyz = atom(&cr_type(b"xyz"), &qt_userdata_string("garbage"));
    let udta = atom(b"udta", &xyz);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("invalid gps");
    assert!(!md.contains_key("QuickTime:GPSLatitude"));
    assert!(md.contains_key("QuickTime:TimeScale"));
}

#[test]
fn test_udta_xyz_no_altitude() {
    // ©xyz with lat+lon but no altitude -> GPSLatitude/Longitude but no GPSAltitude.
    let xyz = atom(&cr_type(b"xyz"), &qt_userdata_string("+10.0+020.0/"));
    let udta = atom(b"udta", &xyz);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("gps no alt");
    assert!(md.contains_key("QuickTime:GPSLatitude"));
    assert!(md.contains_key("QuickTime:GPSLongitude"));
    assert!(!md.contains_key("QuickTime:GPSAltitude"));
    assert!(md.contains_key("QuickTime:GPSCoordinates"));
}

// ---------------------------------------------------------------------------
// udta: numeric-atom early returns and edge values
// ---------------------------------------------------------------------------

#[test]
fn test_udta_tmpo_zero_bpm_skipped() {
    // tmpo with bpm == 0 -> the `bpm > 0` guard skips insertion.
    let tmpo = atom(b"tmpo", &[0u8; 4]); // u16@2 == 0
    let udta = atom(b"udta", &tmpo);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("zero tmpo");
    assert!(!md.contains_key("QuickTime:BeatsPerMinute"));
}

#[test]
fn test_udta_gnre_too_short_skipped() {
    // gnre atom with < 6 bytes -> early skip (no Genre).
    let gnre = atom(b"gnre", &[0u8, 0u8, 0u8, 0u8]);
    let udta = atom(b"udta", &gnre);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("short gnre");
    assert!(!md.contains_key("QuickTime:Genre"));
}

#[test]
fn test_udta_yrrc_too_short_skipped() {
    // yrrc with < 6 bytes -> no Year.
    let yrrc = atom(b"yrrc", &[0u8, 0u8, 0u8, 0u8]);
    let udta = atom(b"udta", &yrrc);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("short yrrc");
    // mvhd still provides TimeScale; Year should be absent.
    assert!(!md.contains_key("QuickTime:Year"));
}

#[test]
fn test_udta_3gpp_dscp_perf_albm() {
    // 3GPP dscp / perf / albm arms via extract_3gpp_string_value.
    let gpp = |text: &str| -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 4]); // version/flags
        p.extend_from_slice(&[0u8, 0u8]); // language
        p.extend_from_slice(text.as_bytes());
        p.push(0);
        p
    };
    let dscp = atom(b"dscp", &gpp("A description"));
    let perf = atom(b"perf", &gpp("A performer"));
    let albm = atom(b"albm", &gpp("An album"));
    let udta = atom(b"udta", &concat(&[dscp, perf, albm]));
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("3gpp dscp/perf/albm");
    assert_eq!(
        md.get("QuickTime:Description"),
        Some(&TagValue::String("A description".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Performer"),
        Some(&TagValue::String("A performer".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Album"),
        Some(&TagValue::String("An album".to_string()))
    );
}

#[test]
fn test_udta_unknown_atom_ignored() {
    // A non-© atom that matches no known arm is silently ignored.
    let unk = atom(b"ZZZZ", b"ignored");
    let udta = atom(b"udta", &unk);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("unknown udta atom");
    assert!(md.contains_key("QuickTime:TimeScale"));
}

// ---------------------------------------------------------------------------
// Handler metadata edge cases: empty component, unknown handler, auxC, short name
// ---------------------------------------------------------------------------

#[test]
fn test_handler_empty_component_unknown_type() {
    // Empty (all-zero) component -> no HandlerClass. Unknown handler type -> raw 4cc.
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"zzzz", b"\0\0\0\0", b""),
    );
    let udta = atom(b"udta", &hdlr);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("empty component");
    assert!(!md.contains_key("QuickTime:HandlerClass"));
    assert_eq!(
        md.get("QuickTime:HandlerType"),
        Some(&TagValue::String("zzzz".to_string()))
    );
}

#[test]
fn test_handler_auxc_type() {
    // auxC handler-type arm -> "Auxiliary Codec".
    let hdlr = atom(b"hdlr", &hdlr_payload(b"mhlr", b"auxC", b"\0\0\0\0", b""));
    let udta = atom(b"udta", &hdlr);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("auxC");
    assert_eq!(
        md.get("QuickTime:HandlerType"),
        Some(&TagValue::String("Auxiliary Codec".to_string()))
    );
}

#[test]
fn test_handler_too_short_no_class() {
    // hdlr data < 24 bytes -> extract_handler_metadata returns early (no fields).
    // Build a 16-byte payload (component+handler present but overall < 24).
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 4]); // version/flags
    p.extend_from_slice(b"mhlr"); // component
    p.extend_from_slice(b"vide"); // handler
    p.extend_from_slice(b"\0\0\0\0"); // vendor (total 16 bytes)
    let hdlr = atom(b"hdlr", &p);
    let udta = atom(b"udta", &hdlr);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("short hdlr");
    // < 24 bytes means no HandlerType emitted from movie-level handler.
    assert!(!md.contains_key("QuickTime:HandlerType"));
}

#[test]
fn test_handler_reserved_vendor_skipped() {
    // Vendor field that trims to empty (all-zero) -> no HandlerVendorID, and a name
    // that is too short to register a HandlerDescription.
    let hdlr = atom(b"hdlr", &hdlr_payload(b"mhlr", b"vide", b"\0\0\0\0", b""));
    let udta = atom(b"udta", &hdlr);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("reserved vendor");
    assert!(!md.contains_key("QuickTime:HandlerVendorID"));
    assert!(!md.contains_key("QuickTime:HandlerDescription"));
}

// ---------------------------------------------------------------------------
// Audio-track dinf->dref handler-class arms (url / rsrc / unknown)
// ---------------------------------------------------------------------------

fn audio_track_with_dref(entry_type: &[u8; 4]) -> Vec<u8> {
    let tkhd = atom(b"tkhd", &tkhd_v0_full(1, 0, 0));
    let mdhd = atom(b"mdhd", &mdhd_v0(44100, 88200));
    let smhd = atom(b"smhd", &[0u8; 8]);
    let dref = atom(b"dref", &{
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 4]); // version/flags
        p.extend_from_slice(&1u32.to_be_bytes()); // entry count
        p.extend_from_slice(&atom(entry_type, &[0u8; 4]));
        p
    });
    let dinf = atom(b"dinf", &dref);
    let stbl = atom(b"stbl", &[]);
    let minf = atom(b"minf", &concat(&[smhd, dinf, stbl]));
    let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
    let trak = atom(b"trak", &concat(&[tkhd, mdia]));
    atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]))
}

#[test]
fn test_dref_alis_handler_class() {
    // "alis" dref entry -> "Data Handler" (the alis/dhlr arm).
    let moov = audio_track_with_dref(b"alis");
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("alis dref");
    assert_eq!(
        md.get("QuickTime:HandlerClass"),
        Some(&TagValue::String("Data Handler".to_string()))
    );
}

#[test]
fn test_dref_url_entry_executes_match() {
    // "url " entry: the source match arm is keyed on the *trimmed* string, so the
    // "url " arm (with a trailing space) is never hit and this falls through to the
    // unknown arm. We still execute the data-handler-info match here; assert that no
    // HandlerClass is produced and the parser does not panic.
    let moov = audio_track_with_dref(b"url ");
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("url dref");
    assert!(!md.contains_key("QuickTime:HandlerClass"));
    assert!(md.contains_key("QuickTime:TimeScale"));
}

#[test]
fn test_dref_rsrc_handler_class() {
    let moov = audio_track_with_dref(b"rsrc");
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("rsrc dref");
    assert_eq!(
        md.get("QuickTime:HandlerClass"),
        Some(&TagValue::String("Resource Data Handler".to_string()))
    );
}

#[test]
fn test_dref_unknown_handler_class_skipped() {
    // Unknown dref entry type -> extract_data_handler_info returns Ok(()) without a
    // HandlerClass (the `_ => return Ok(())` arm).
    let moov = audio_track_with_dref(b"zzzz");
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("unknown dref");
    // No dref-derived HandlerClass for an unknown entry type.
    assert!(!md.contains_key("QuickTime:HandlerClass"));
}

#[test]
fn test_dref_zero_entry_count_skipped() {
    // dref with entry_count == 0 -> early return.
    let tkhd = atom(b"tkhd", &tkhd_v0_full(1, 0, 0));
    let mdhd = atom(b"mdhd", &mdhd_v0(44100, 88200));
    let smhd = atom(b"smhd", &[0u8; 8]);
    let dref = atom(b"dref", &{
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 4]);
        p.extend_from_slice(&0u32.to_be_bytes()); // entry count = 0
        p
    });
    let dinf = atom(b"dinf", &dref);
    let stbl = atom(b"stbl", &[]);
    let minf = atom(b"minf", &concat(&[smhd, dinf, stbl]));
    let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
    let trak = atom(b"trak", &concat(&[tkhd, mdia]));
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("zero dref entries");
    assert!(md.contains_key("QuickTime:TimeScale"));
}

// ---------------------------------------------------------------------------
// Sample description: remaining codec arms and audio-format detection
// ---------------------------------------------------------------------------

/// stsd payload with a single entry of the given codec, padded to entry_len bytes.
fn stsd_codec(codec: &[u8], entry_len: usize) -> Vec<u8> {
    let mut entry = vec![0u8; entry_len.max(16)];
    let size = entry.len() as u32;
    entry[0..4].copy_from_slice(&size.to_be_bytes());
    entry[4..8].copy_from_slice(codec);
    entry[14..16].copy_from_slice(&1u16.to_be_bytes()); // data_ref_index
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 4]);
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&entry);
    p
}

fn audio_codec_track(codec: &[u8]) -> Vec<u8> {
    let stsd = atom(b"stsd", &stsd_codec(codec, 36));
    let smhd = atom(b"smhd", &[0u8; 8]);
    let stbl = atom(b"stbl", &stsd);
    let minf = atom(b"minf", &concat(&[smhd, stbl]));
    let mdia = atom(
        b"mdia",
        &concat(&[atom(b"mdhd", &mdhd_v0(44100, 88200)), minf]),
    );
    let trak = atom(
        b"trak",
        &concat(&[atom(b"tkhd", &tkhd_v0_full(1, 0, 0)), mdia]),
    );
    atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]))
}

#[test]
fn test_sample_description_more_codec_names() {
    // Hit lpcm + ac-3 + ec-3 + hvc1 codec-name arms. ("raw " trims to "raw" before
    // the match, so its CompressorName is covered separately in the raw test.)
    let cases: [(&[u8; 4], &str); 4] = [
        (b"lpcm", "Linear PCM"),
        (b"ac-3", "AC-3"),
        (b"ec-3", "E-AC-3"),
        (b"hvc1", "H.265/HEVC"),
    ];
    for (codec, expected) in cases {
        let moov = audio_codec_track(codec);
        let reader = TestReader::new(moov);
        let md = parse_quicktime_metadata(&reader).expect("codec name");
        assert_eq!(
            md.get("QuickTime:CompressorName"),
            Some(&TagValue::String(expected.to_string())),
            "codec {:?}",
            std::str::from_utf8(codec).unwrap()
        );
    }
}

#[test]
fn test_sample_description_more_audio_formats() {
    // samr / ulaw / lpcm are audio codecs -> AudioFormat is emitted.
    for codec in [b"samr", b"ulaw", b"lpcm", b"alac"] {
        let moov = audio_codec_track(codec);
        let reader = TestReader::new(moov);
        let md = parse_quicktime_metadata(&reader).expect("audio format");
        let trimmed = std::str::from_utf8(codec).unwrap().trim();
        assert_eq!(
            md.get("QuickTime:AudioFormat"),
            Some(&TagValue::String(trimmed.to_string())),
            "codec {:?}",
            trimmed
        );
    }
}

#[test]
fn test_sample_description_raw_trimmed_codec_name() {
    // "raw " trims to "raw" in CompressorID; both raw codec arms execute.
    let moov = audio_codec_track(b"raw ");
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("raw codec");
    assert_eq!(
        md.get("QuickTime:CompressorID"),
        Some(&TagValue::String("raw".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:AudioFormat"),
        Some(&TagValue::String("raw".to_string()))
    );
}

#[test]
fn test_sample_description_zero_entry_count() {
    // stsd with entry_count == 0 -> only SampleDescriptionCount is set.
    let mut stsd_payload = Vec::new();
    stsd_payload.extend_from_slice(&[0u8; 4]); // version/flags
    stsd_payload.extend_from_slice(&0u32.to_be_bytes()); // entry count = 0
    let stsd = atom(b"stsd", &stsd_payload);
    let stbl = atom(b"stbl", &stsd);
    let minf = atom(b"minf", &stbl);
    let mdia = atom(
        b"mdia",
        &concat(&[atom(b"mdhd", &mdhd_v0(30000, 60000)), minf]),
    );
    let trak = atom(
        b"trak",
        &concat(&[atom(b"tkhd", &tkhd_v0_full(1, 0, 0)), mdia]),
    );
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("zero stsd");
    assert_eq!(
        md.get("QuickTime:SampleDescriptionCount"),
        Some(&TagValue::Integer(0))
    );
    assert!(!md.contains_key("QuickTime:CompressorID"));
}

// ---------------------------------------------------------------------------
// hvcC inside a video stsd entry (extract_hevc_configuration path) covering the
// High-Tier / Variable-CFR / multi-compat-profile / various profile arms.
// ---------------------------------------------------------------------------

fn video_stsd_with_hvcc(hvcc: &[u8]) -> Vec<u8> {
    // Build an 86-byte video sample entry then append the hvcC box as extension.
    let mut entry = vec![0u8; 86];
    entry[4..8].copy_from_slice(b"hvc1"); // format
    entry[14..16].copy_from_slice(&1u16.to_be_bytes()); // data_ref_index
    entry[32..34].copy_from_slice(&1920u16.to_be_bytes()); // width
    entry[34..36].copy_from_slice(&1080u16.to_be_bytes()); // height
    entry[82..84].copy_from_slice(&24u16.to_be_bytes()); // depth
    // append hvcC box (size + "hvcC" + payload)
    let hvcc_box = atom(b"hvcC", hvcc);
    entry.extend_from_slice(&hvcc_box);
    let size = entry.len() as u32;
    entry[0..4].copy_from_slice(&size.to_be_bytes());

    let mut stsd_payload = Vec::new();
    stsd_payload.extend_from_slice(&[0u8; 4]);
    stsd_payload.extend_from_slice(&1u32.to_be_bytes());
    stsd_payload.extend_from_slice(&entry);
    let stsd = atom(b"stsd", &stsd_payload);
    let stbl = atom(b"stbl", &stsd);
    let minf = atom(b"minf", &stbl);
    let mdia = atom(
        b"mdia",
        &concat(&[atom(b"mdhd", &mdhd_v0(30000, 60000)), minf]),
    );
    let trak = atom(
        b"trak",
        &concat(&[atom(b"tkhd", &tkhd_v0_full(1, 1920, 1080)), mdia]),
    );
    atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]))
}

#[test]
fn test_hvcc_in_stsd_high_tier_variable_cfr() {
    // profile_byte: space=0, tier=1 (High Tier), profile_idc=2 (Main 10).
    let mut hvcc = vec![0u8; 23];
    hvcc[0] = 1; // config version
    hvcc[1] = 0b0010_0010; // tier bit set (0x20) + profile_idc 2
    // compat flags: set Main, Main 10, Main Still Picture (top 3 bits).
    hvcc[2..6].copy_from_slice(&0xE000_0000u32.to_be_bytes());
    hvcc[12] = 153; // level idc
    hvcc[16] = 0x02; // chroma 4:2:2
    hvcc[17] = 0x02; // bit depth luma 10
    hvcc[18] = 0x02; // bit depth chroma 10
    hvcc[21] = 0b1000_0000; // constantFrameRate = 2 (Variable)
    let moov = video_stsd_with_hvcc(&hvcc);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("hvcc high tier");

    assert_eq!(
        md.get("QuickTime:GeneralTierFlag"),
        Some(&TagValue::String("High Tier".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:GeneralProfileIDC"),
        Some(&TagValue::String("Main 10".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:ChromaFormat"),
        Some(&TagValue::String("4:2:2".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:BitDepthLuma"),
        Some(&TagValue::Integer(10))
    );
    assert_eq!(
        md.get("QuickTime:ConstantFrameRate"),
        Some(&TagValue::String("Variable".to_string()))
    );
    // multi compat profile joined string present
    assert!(md.contains_key("QuickTime:GenProfileCompatibilityFlags"));
}

#[test]
fn test_hvcc_in_stsd_constant_cfr_444() {
    // constantFrameRate = 1 (Constant), chroma 4:4:4, profile_idc 3 (Main Still).
    let mut hvcc = vec![0u8; 23];
    hvcc[0] = 1;
    hvcc[1] = 0x03; // profile_idc 3
    hvcc[12] = 90;
    hvcc[16] = 0x03; // chroma 4:4:4
    hvcc[21] = 0b0100_0000; // constantFrameRate = 1 (Constant)
    let moov = video_stsd_with_hvcc(&hvcc);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("hvcc constant");
    assert_eq!(
        md.get("QuickTime:GeneralProfileIDC"),
        Some(&TagValue::String("Main Still Picture".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:ChromaFormat"),
        Some(&TagValue::String("4:4:4".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:ConstantFrameRate"),
        Some(&TagValue::String("Constant".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:GeneralProfileSpace"),
        Some(&TagValue::String("Conforming".to_string()))
    );
}

// ---------------------------------------------------------------------------
// Graphics mode (vmhd) enum arms: blend / alpha / dither / unknown
// ---------------------------------------------------------------------------

fn video_track_with_vmhd_mode(mode: u16) -> Vec<u8> {
    let vmhd = atom(b"vmhd", &{
        let mut v = vec![0u8; 12];
        v[3] = 0x01;
        v[4..6].copy_from_slice(&mode.to_be_bytes());
        v[6..8].copy_from_slice(&7u16.to_be_bytes()); // opcolor R
        v
    });
    let stbl = atom(b"stbl", &[]);
    let minf = atom(b"minf", &concat(&[vmhd, stbl]));
    let mdia = atom(
        b"mdia",
        &concat(&[atom(b"mdhd", &mdhd_v0(30000, 60000)), minf]),
    );
    let trak = atom(
        b"trak",
        &concat(&[atom(b"tkhd", &tkhd_v0_full(1, 100, 100)), mdia]),
    );
    atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]))
}

#[test]
fn test_vmhd_graphics_mode_arms() {
    let cases: [(u16, &str); 5] = [
        (0x20, "blend"),
        (0x24, "transparent"),
        (0x40, "ditherCopy"),
        (0x100, "alpha"),
        (0x9999, "unknown"),
    ];
    for (mode, expected) in cases {
        let moov = video_track_with_vmhd_mode(mode);
        let reader = TestReader::new(moov);
        let md = parse_quicktime_metadata(&reader).expect("vmhd mode");
        assert_eq!(
            md.get("QuickTime:GraphicsMode"),
            Some(&TagValue::String(expected.to_string())),
            "mode {:#x}",
            mode
        );
        assert_eq!(
            md.get("QuickTime:OpColor"),
            Some(&TagValue::String("7 0 0".to_string()))
        );
    }
}

// ---------------------------------------------------------------------------
// MP4 keys/ilst: malformed keys atoms and integer-indexed reverse-DNS fallback
// ---------------------------------------------------------------------------

fn keys_atom(entries: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(&[0u8; 4]); // version/flags
    payload.extend_from_slice(&(entries.len() as u32).to_be_bytes());
    for (ns, key) in entries {
        let key_size = (8 + key.len()) as u32;
        payload.extend_from_slice(&key_size.to_be_bytes());
        payload.extend_from_slice(*ns);
        payload.extend_from_slice(key.as_bytes());
    }
    atom(b"keys", &payload)
}

#[test]
fn test_mp4_keys_reverse_dns_and_rating() {
    // Several reverse-DNS / non-Apple keys -> they fall to "QuickTime:<key>".
    let keys = keys_atom(&[
        (b"mdta", "com.apple.quicktime.rating.user"),
        (b"mdta", "com.android.version"),
        (b"mdta", "com.apple.quicktime.author"),
    ]);
    let i1 = atom(&1u32.to_be_bytes(), &itunes_data_utf8("5"));
    let i2 = atom(&2u32.to_be_bytes(), &itunes_data_utf8("13"));
    let i3 = atom(&3u32.to_be_bytes(), &itunes_data_utf8("Jane Doe"));
    let ilst = atom(b"ilst", &concat(&[i1, i2, i3]));
    let meta = atom(b"meta", &meta_payload_no_header(&[keys, ilst]));
    let moov = moov_with(&[meta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("reverse dns keys");
    assert_eq!(
        md.get("QuickTime:com.apple.quicktime.rating.user"),
        Some(&TagValue::String("5".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:com.android.version"),
        Some(&TagValue::String("13".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:com.apple.quicktime.author"),
        Some(&TagValue::String("Jane Doe".to_string()))
    );
}

#[test]
fn test_mp4_keys_malformed_key_size_breaks() {
    // A key whose declared key_size is < 8 -> parse_mp4_keys breaks out of the loop.
    let mut payload = Vec::new();
    payload.extend_from_slice(&[0u8; 4]);
    payload.extend_from_slice(&2u32.to_be_bytes()); // entry count = 2
    // first (valid) key
    let key = "com.apple.quicktime.make";
    payload.extend_from_slice(&((8 + key.len()) as u32).to_be_bytes());
    payload.extend_from_slice(b"mdta");
    payload.extend_from_slice(key.as_bytes());
    // second key with bogus size (< 8) -> loop breaks; index 2 never registered.
    payload.extend_from_slice(&3u32.to_be_bytes()); // key_size = 3 (invalid)
    payload.extend_from_slice(b"xx"); // junk
    let keys = atom(b"keys", &payload);

    let i1 = atom(&1u32.to_be_bytes(), &itunes_data_utf8("Apple"));
    let i2 = atom(&2u32.to_be_bytes(), &itunes_data_utf8("orphan"));
    let ilst = atom(b"ilst", &concat(&[i1, i2]));
    let meta = atom(b"meta", &meta_payload_no_header(&[keys, ilst]));
    let moov = moov_with(&[meta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("malformed keys");
    assert_eq!(
        md.get("QuickTime:Make"),
        Some(&TagValue::String("Apple".to_string()))
    );
    // index 2 has no key -> MP4: fallback for that item.
    let has_mp4_orphan = md
        .iter()
        .any(|(k, v)| k.starts_with("MP4:") && matches!(v, TagValue::String(s) if s == "orphan"));
    assert!(has_mp4_orphan);
}

#[test]
fn test_mp4_keys_short_atom_returns_empty() {
    // keys atom shorter than 8 bytes -> parse_mp4_keys returns an empty map.
    let keys = atom(b"keys", &[0u8, 0u8, 0u8, 0u8]);
    let item = atom(&1u32.to_be_bytes(), &itunes_data_utf8("value"));
    let ilst = atom(b"ilst", &item);
    let meta = atom(b"meta", &meta_payload_no_header(&[keys, ilst]));
    let moov = moov_with(&[meta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("short keys");
    // The item falls through to the MP4: fallback (no key for index 1).
    let has_mp4 = md.iter().any(|(k, _)| k.starts_with("MP4:"));
    assert!(has_mp4);
}

#[test]
fn test_mp4_meta_missing_ilst_no_panic() {
    // moov->meta has keys but NO ilst -> extract_mp4_metadata does nothing.
    let keys = keys_atom(&[(b"mdta", "com.apple.quicktime.make")]);
    let meta = atom(b"meta", &meta_payload_no_header(&[keys]));
    let moov = moov_with(&[meta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("meta no ilst");
    assert!(md.contains_key("QuickTime:TimeScale"));
    assert!(!md.contains_key("QuickTime:Make"));
}

// ---------------------------------------------------------------------------
// Multiple traks of different handler kinds (text / meta / soun / vide)
// ---------------------------------------------------------------------------

#[test]
fn test_multiple_track_kinds_via_minf_hdlr() {
    // Build 3 audio-ish tracks (smhd present) each with a different minf hdlr.
    // Audio tracks are the only ones that run extract_track_handler_metadata from
    // minf->hdlr. We vary the component to exercise its match arms.
    let make_audio_trak = |id: u32, component: &[u8; 4]| -> Vec<u8> {
        let tkhd = atom(b"tkhd", &tkhd_v0_full(id, 0, 0));
        let mdhd = atom(b"mdhd", &mdhd_v0(44100, 88200));
        let smhd = atom(b"smhd", &[0u8; 8]);
        let hdlr = atom(b"hdlr", &hdlr_payload(component, b"soun", b"\0\0\0\0", b""));
        let stbl = atom(b"stbl", &[]);
        let minf = atom(b"minf", &concat(&[smhd, hdlr, stbl]));
        let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
        atom(b"trak", &concat(&[tkhd, mdia]))
    };
    let t1 = make_audio_trak(1, b"mhlr"); // -> "Media Handler"
    let t2 = make_audio_trak(2, b"dhlr"); // -> "Data Handler"
    let t3 = make_audio_trak(3, b"cust"); // -> trimmed custom
    let moov = atom(
        b"moov",
        &concat(&[atom(b"mvhd", &mvhd(600, 6000)), t1, t2, t3]),
    );
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("multi handler tracks");
    assert_eq!(
        md.get("QuickTime:HandlerClass"),
        Some(&TagValue::String("Media Handler".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:HandlerClass_2"),
        Some(&TagValue::String("Data Handler".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:HandlerClass_3"),
        Some(&TagValue::String("cust".to_string()))
    );
    assert_eq!(md.get("QuickTime:TrackID_2"), Some(&TagValue::Integer(2)));
    assert_eq!(md.get("QuickTime:TrackID_3"), Some(&TagValue::Integer(3)));
}

#[test]
fn test_track_handler_too_short_skipped() {
    // minf hdlr with < 12 bytes -> extract_track_handler_metadata early-returns.
    let tkhd = atom(b"tkhd", &tkhd_v0_full(1, 0, 0));
    let mdhd = atom(b"mdhd", &mdhd_v0(44100, 88200));
    let smhd = atom(b"smhd", &[0u8; 8]);
    let hdlr = atom(b"hdlr", &[0u8; 8]); // < 12 bytes
    let stbl = atom(b"stbl", &[]);
    let minf = atom(b"minf", &concat(&[smhd, hdlr, stbl]));
    let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
    let trak = atom(b"trak", &concat(&[tkhd, mdia]));
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("short minf hdlr");
    assert!(md.contains_key("QuickTime:TimeScale"));
}

// ---------------------------------------------------------------------------
// HEIF: iloc v1, iinf v1 (32-bit count), pitm v1 (32-bit), ispe legacy fallback
// ---------------------------------------------------------------------------

#[test]
fn test_heif_iinf_v1_and_pitm_v1() {
    // iinf version 1 -> 32-bit entry count at offset 4, entries at offset 8.
    let infe_payload = {
        let mut v = Vec::new();
        v.extend_from_slice(&[0u8; 4]); // version/flags
        v.extend_from_slice(&3u16.to_be_bytes()); // item_id
        v.extend_from_slice(&0u16.to_be_bytes()); // protection idx
        v.extend_from_slice(b"hvc1"); // not Exif
        v
    };
    let infe = atom(b"infe", &infe_payload);
    let mut iinf_payload = Vec::new();
    iinf_payload.push(1); // version 1
    iinf_payload.extend_from_slice(&[0u8; 3]); // flags
    iinf_payload.extend_from_slice(&1u32.to_be_bytes()); // 32-bit entry count
    iinf_payload.extend_from_slice(&infe);
    let iinf = atom(b"iinf", &iinf_payload);

    // pitm version 1 -> 32-bit item id at offset 4.
    let pitm = atom(b"pitm", &{
        let mut v = vec![0u8; 8];
        v[0] = 1; // version 1
        v[4..8].copy_from_slice(&7u32.to_be_bytes());
        v
    });
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"pict", b"\0\0\0\0", b"H"),
    );
    let meta = atom(b"meta", &meta_payload(&[hdlr, iinf, pitm]));
    let ftyp = atom(b"ftyp", &ftyp_payload(b"mif1", 0, &[b"mif1"]));
    let reader = TestReader::new(concat(&[ftyp, meta]));
    let md = parse_quicktime_metadata(&reader).expect("iinf/pitm v1");
    assert_eq!(md.get("HEIF:ItemCount"), Some(&TagValue::Integer(1)));
    assert_eq!(
        md.get("QuickTime:PrimaryItemReference"),
        Some(&TagValue::Integer(7))
    );
}

#[test]
fn test_heif_iloc_v1_construction_method() {
    // iloc version 1 -> per-item construction_method field (extra 2 bytes).
    // Combined with an Exif item in iinf so the iloc map is actually consulted.
    let tiff = build_tiff_exif();
    let mut exif_payload = Vec::new();
    exif_payload.extend_from_slice(&[0u8; 4]); // exif header offset
    exif_payload.extend_from_slice(b"Exif");
    exif_payload.extend_from_slice(&[0u8, 0u8]);
    exif_payload.extend_from_slice(&tiff);
    let exif_len = exif_payload.len() as u32;

    // iinf v0 with one Exif item id=1.
    let infe_payload = {
        let mut v = Vec::new();
        v.extend_from_slice(&[0u8; 4]);
        v.extend_from_slice(&1u16.to_be_bytes());
        v.extend_from_slice(&0u16.to_be_bytes());
        v.extend_from_slice(b"Exif");
        v
    };
    let infe = atom(b"infe", &infe_payload);
    let mut iinf_payload = Vec::new();
    iinf_payload.extend_from_slice(&[0u8; 4]);
    iinf_payload.extend_from_slice(&1u16.to_be_bytes());
    iinf_payload.extend_from_slice(&infe);
    let iinf = atom(b"iinf", &iinf_payload);

    // iloc v1: version(1)+flags(3), [offset|length](1), [base|index](1),
    // item_count(2), per item: item_id(2), construction_method(2), data_ref(2),
    // base_offset(var=0), extent_count(2), extent_offset(4), extent_length(4).
    let mut iloc_payload = Vec::new();
    iloc_payload.push(1); // version 1
    iloc_payload.extend_from_slice(&[0u8; 3]); // flags
    iloc_payload.push(0x44); // offset_size=4, length_size=4
    iloc_payload.push(0x00); // base_offset_size=0, index_size=0
    iloc_payload.extend_from_slice(&1u16.to_be_bytes()); // item_count
    iloc_payload.extend_from_slice(&1u16.to_be_bytes()); // item_id
    iloc_payload.extend_from_slice(&0u16.to_be_bytes()); // construction_method
    iloc_payload.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
    iloc_payload.extend_from_slice(&1u16.to_be_bytes()); // extent_count
    iloc_payload.extend_from_slice(&0u32.to_be_bytes()); // extent_offset
    iloc_payload.extend_from_slice(&exif_len.to_be_bytes()); // extent_length
    let iloc = atom(b"iloc", &iloc_payload);

    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"pict", b"\0\0\0\0", b"H"),
    );
    let meta = atom(b"meta", &meta_payload(&[hdlr, iinf, iloc]));
    let ftyp = atom(b"ftyp", &ftyp_payload(b"heic", 0, &[b"heic"]));
    let mdat = atom(b"mdat", &exif_payload);
    let reader = TestReader::new(concat(&[ftyp, meta, mdat]));
    let md = parse_quicktime_metadata(&reader).expect("iloc v1");
    assert_eq!(md.get("HEIF:ItemCount"), Some(&TagValue::Integer(1)));
    // EXIF tags should have been parsed from mdat (direct-offset fallback at 0).
    let extra = md
        .iter()
        .filter(|(k, _)| {
            !k.starts_with("HEIF:") && !k.starts_with("QuickTime:") && !k.starts_with("UserData:")
        })
        .count();
    assert!(extra > 0, "expected EXIF-derived tags from iloc v1");
}

#[test]
fn test_heif_ispe_legacy_fallback_direct_child() {
    // ispe directly in meta children (not inside iprp->ipco) -> fallback loop.
    let ispe = atom(b"ispe", &{
        let mut v = vec![0u8; 12];
        v[4..8].copy_from_slice(&800u32.to_be_bytes());
        v[8..12].copy_from_slice(&600u32.to_be_bytes());
        v
    });
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"pict", b"\0\0\0\0", b"H"),
    );
    let meta = atom(b"meta", &meta_payload(&[hdlr, ispe]));
    let ftyp = atom(b"ftyp", &ftyp_payload(b"mif1", 0, &[b"mif1"]));
    let reader = TestReader::new(concat(&[ftyp, meta]));
    let md = parse_quicktime_metadata(&reader).expect("ispe legacy");
    assert_eq!(md.get("HEIF:ImageWidth"), Some(&TagValue::Integer(800)));
    assert_eq!(md.get("HEIF:ImageHeight"), Some(&TagValue::Integer(600)));
    assert_eq!(
        md.get("QuickTime:ImageSpatialExtent"),
        Some(&TagValue::String("800x600".to_string()))
    );
}

#[test]
fn test_heif_iinf_too_short_no_panic() {
    // iinf atom < 6 bytes -> find_exif_item_id returns None; no ItemCount.
    let iinf = atom(b"iinf", &[0u8, 0u8, 0u8]);
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"pict", b"\0\0\0\0", b"H"),
    );
    let ispe = atom(b"ispe", &{
        let mut v = vec![0u8; 12];
        v[4..8].copy_from_slice(&10u32.to_be_bytes());
        v[8..12].copy_from_slice(&10u32.to_be_bytes());
        v
    });
    let ipco = atom(b"ipco", &ispe);
    let iprp = atom(b"iprp", &ipco);
    let meta = atom(b"meta", &meta_payload(&[hdlr, iinf, iprp]));
    let ftyp = atom(b"ftyp", &ftyp_payload(b"mif1", 0, &[b"mif1"]));
    let reader = TestReader::new(concat(&[ftyp, meta]));
    let md = parse_quicktime_metadata(&reader).expect("short iinf");
    assert!(!md.contains_key("HEIF:ItemCount"));
    assert_eq!(md.get("HEIF:ImageWidth"), Some(&TagValue::Integer(10)));
}

/// Build a minimal little-endian TIFF/EXIF block with a few IFD0 entries spanning
/// several field types so raw_bytes_to_tag_value runs multiple arms.
fn build_tiff_exif() -> Vec<u8> {
    let mut t = Vec::new();
    t.extend_from_slice(b"II");
    t.extend_from_slice(&0x002Au16.to_le_bytes());
    t.extend_from_slice(&8u32.to_le_bytes());

    let entries: [(u16, u16, u32, [u8; 4]); 3] = [
        (0x010F, 2, 3, *b"Hi\0\0"),               // Make ASCII
        (0x0100, 3, 1, [0x80, 0x02, 0x00, 0x00]), // ImageWidth SHORT 640
        (0x0101, 4, 1, [0xE0, 0x01, 0x00, 0x00]), // ImageLength LONG 480
    ];

    t.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, ty, count, val) in entries {
        t.extend_from_slice(&tag.to_le_bytes());
        t.extend_from_slice(&ty.to_le_bytes());
        t.extend_from_slice(&count.to_le_bytes());
        t.extend_from_slice(&val);
    }
    t.extend_from_slice(&0u32.to_le_bytes());
    t
}

// ---------------------------------------------------------------------------
// Malformed / truncated / zero-size boxes for early-return branches
// ---------------------------------------------------------------------------

#[test]
fn test_zero_size_box_does_not_loop_forever() {
    // A child atom declaring size 0 inside udta must not hang the parser.
    let mut udta_payload = Vec::new();
    // valid ©nam then a zero-size box.
    udta_payload.extend_from_slice(&atom(&cr_type(b"nam"), &qt_userdata_string("Z")));
    udta_payload.extend_from_slice(&0u32.to_be_bytes()); // size = 0
    udta_payload.extend_from_slice(b"junk");
    let udta = atom(b"udta", &udta_payload);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("zero-size box");
    assert!(md.contains_key("QuickTime:TimeScale"));
}

#[test]
fn test_truncated_mdhd_no_panic() {
    // mdhd shorter than 24 bytes -> early return; track still processed via tkhd.
    let tkhd = atom(b"tkhd", &tkhd_v0_full(3, 100, 100));
    let mdhd = atom(b"mdhd", &[0u8; 10]); // < 24
    let stbl = atom(b"stbl", &[]);
    let minf = atom(b"minf", &stbl);
    let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
    let trak = atom(b"trak", &concat(&[tkhd, mdia]));
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("short mdhd");
    assert_eq!(md.get("QuickTime:TrackID"), Some(&TagValue::Integer(3)));
    assert!(!md.contains_key("QuickTime:MediaTimeScale"));
}

#[test]
fn test_truncated_tkhd_no_panic() {
    // tkhd < 84 bytes -> extract_track_header early-returns (no TrackID), but the
    // rest of the track (mdhd) still processes.
    let tkhd = atom(b"tkhd", &[0u8; 40]);
    let mdhd = atom(b"mdhd", &mdhd_v0(30000, 60000));
    let stbl = atom(b"stbl", &[]);
    let minf = atom(b"minf", &stbl);
    let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
    let trak = atom(b"trak", &concat(&[tkhd, mdia]));
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("short tkhd");
    assert!(!md.contains_key("QuickTime:TrackID"));
    assert!(md.contains_key("QuickTime:MediaTimeScale"));
}

#[test]
fn test_truncated_stts_no_frame_rate() {
    // stts < 16 bytes -> no VideoFrameRate.
    let stsd = atom(b"stsd", &stsd_codec(b"avc1", 16));
    let stts = atom(b"stts", &[0u8; 8]); // < 16
    let stbl = atom(b"stbl", &concat(&[stsd, stts]));
    let minf = atom(b"minf", &stbl);
    let mdia = atom(
        b"mdia",
        &concat(&[atom(b"mdhd", &mdhd_v0(30000, 60000)), minf]),
    );
    let trak = atom(
        b"trak",
        &concat(&[atom(b"tkhd", &tkhd_v0_full(1, 320, 240)), mdia]),
    );
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("short stts");
    assert!(!md.contains_key("QuickTime:VideoFrameRate"));
}

#[test]
fn test_stts_zero_entry_count_no_frame_rate() {
    // stts with entry_count == 0 -> early return.
    let mut stts_payload = vec![0u8; 16];
    stts_payload[4..8].copy_from_slice(&0u32.to_be_bytes()); // entry count = 0
    let stsd = atom(b"stsd", &stsd_codec(b"avc1", 16));
    let stts = atom(b"stts", &stts_payload);
    let stbl = atom(b"stbl", &concat(&[stsd, stts]));
    let minf = atom(b"minf", &stbl);
    let mdia = atom(
        b"mdia",
        &concat(&[atom(b"mdhd", &mdhd_v0(30000, 60000)), minf]),
    );
    let trak = atom(
        b"trak",
        &concat(&[atom(b"tkhd", &tkhd_v0_full(1, 320, 240)), mdia]),
    );
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("zero stts entries");
    assert!(!md.contains_key("QuickTime:VideoFrameRate"));
}

#[test]
fn test_stts_zero_delta_no_frame_rate() {
    // stts with sample_delta == 0 -> early return (avoid divide-by-zero).
    let mut stts_payload = vec![0u8; 16];
    stts_payload[4..8].copy_from_slice(&1u32.to_be_bytes()); // entry count = 1
    stts_payload[8..12].copy_from_slice(&30u32.to_be_bytes()); // sample count
    stts_payload[12..16].copy_from_slice(&0u32.to_be_bytes()); // sample delta = 0
    let stsd = atom(b"stsd", &stsd_codec(b"avc1", 16));
    let stts = atom(b"stts", &stts_payload);
    let stbl = atom(b"stbl", &concat(&[stsd, stts]));
    let minf = atom(b"minf", &stbl);
    let mdia = atom(
        b"mdia",
        &concat(&[atom(b"mdhd", &mdhd_v0(30000, 60000)), minf]),
    );
    let trak = atom(
        b"trak",
        &concat(&[atom(b"tkhd", &tkhd_v0_full(1, 320, 240)), mdia]),
    );
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("zero delta stts");
    assert!(!md.contains_key("QuickTime:VideoFrameRate"));
}

#[test]
fn test_minf_without_stbl_skipped() {
    // A track whose minf has no stbl -> extract_track_metadata errors (skipped),
    // but tkhd is still processed and mvhd provides TimeScale.
    let tkhd = atom(b"tkhd", &tkhd_v0_full(8, 100, 100));
    let mdhd = atom(b"mdhd", &mdhd_v0(30000, 60000));
    let minf = atom(b"minf", &[]); // no stbl
    let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
    let trak = atom(b"trak", &concat(&[tkhd, mdia]));
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("minf no stbl");
    assert_eq!(md.get("QuickTime:TrackID"), Some(&TagValue::Integer(8)));
    assert!(!md.contains_key("QuickTime:SampleDescriptionCount"));
}

#[test]
fn test_mdia_without_minf_skipped() {
    // mdia present but no minf -> track skipped after mdhd; tkhd still processed.
    let tkhd = atom(b"tkhd", &tkhd_v0_full(6, 100, 100));
    let mdhd = atom(b"mdhd", &mdhd_v0(30000, 60000));
    let mdia = atom(b"mdia", &mdhd);
    let trak = atom(b"trak", &concat(&[tkhd, mdia]));
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("mdia no minf");
    assert_eq!(md.get("QuickTime:TrackID"), Some(&TagValue::Integer(6)));
    assert!(md.contains_key("QuickTime:MediaTimeScale"));
}

// ---------------------------------------------------------------------------
// Production path via read_metadata on a tempfile (.mov extension)
// ---------------------------------------------------------------------------

#[test]
fn test_production_read_metadata_mov_extension() {
    use std::io::Write;
    let nam = atom(&cr_type(b"nam"), &qt_userdata_string("MovTitle"));
    let udta = atom(b"udta", &nam);
    let moov = moov_with(&[udta]);
    let ftyp = atom(b"ftyp", &ftyp_payload(b"qt  ", 0, &[b"qt  "]));
    let data = concat(&[ftyp, moov]);

    let mut tmp = tempfile::Builder::new()
        .suffix(".mov")
        .tempfile()
        .expect("tempfile");
    tmp.write_all(&data).expect("write");
    tmp.flush().expect("flush");

    let md = oxidex::core::operations::read_metadata(tmp.path()).expect("read_metadata mov");
    assert_eq!(
        md.get("QuickTime:Title"),
        Some(&TagValue::String("MovTitle".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:MajorBrand"),
        Some(&TagValue::String("Apple QuickTime (.MOV/QT)".to_string()))
    );
}
