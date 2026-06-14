//! Coverage-focused integration tests for the QuickTime/ISOBMFF metadata extractor.
//!
//! These tests build synthetic ISOBMFF box trees (ftyp / moov / trak / mdia / udta /
//! meta / ilst / keys, etc.) byte-for-byte and drive them through the public
//! `parse_quicktime_metadata` entrypoint as well as the production `read_metadata`
//! path via a tempfile. The goal is to execute as many distinct lines in
//! `src/parsers/quicktime/metadata_extractor.rs` as possible.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::quicktime::parse_quicktime_metadata;
use oxidex::parsers::quicktime::tag_mapping::atom_to_exiftool_tag;

// ---------------------------------------------------------------------------
// Box-building helpers
// ---------------------------------------------------------------------------

/// Build a single ISOBMFF atom: [size:u32][type:4][payload...].
/// `size` includes the 8-byte header.
fn atom(atom_type: &[u8], payload: &[u8]) -> Vec<u8> {
    assert_eq!(atom_type.len(), 4, "atom type must be 4 bytes");
    let size = (payload.len() + 8) as u32;
    let mut v = Vec::with_capacity(payload.len() + 8);
    v.extend_from_slice(&size.to_be_bytes());
    v.extend_from_slice(atom_type);
    v.extend_from_slice(payload);
    v
}

/// Build a 4-byte © (0xA9) atom type, e.g. cr_type(b"nam") -> [0xA9,'n','a','m'].
fn cr_type(suffix: &[u8; 3]) -> [u8; 4] {
    [0xA9, suffix[0], suffix[1], suffix[2]]
}

/// Concatenate several byte vectors into one payload buffer.
fn concat(parts: &[Vec<u8>]) -> Vec<u8> {
    let mut v = Vec::new();
    for p in parts {
        v.extend_from_slice(p);
    }
    v
}

/// A minimal valid ftyp atom payload with the given major brand + compatible brands.
fn ftyp_payload(major: &[u8], minor: u32, compatible: &[&[u8]]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(major);
    p.extend_from_slice(&minor.to_be_bytes());
    for c in compatible {
        p.extend_from_slice(c);
    }
    p
}

/// Build a version-0 mvhd payload (>= 100 bytes) with the given timescale + duration.
fn mvhd_v0(timescale: u32, duration: u32) -> Vec<u8> {
    let mut p = vec![0u8; 100];
    // version=0, flags=0 already zero
    p[0] = 0;
    // creation_time @4, modification_time @8 (u32)
    p[4..8].copy_from_slice(&3_600_000_000u32.to_be_bytes()); // after 1970
    p[8..12].copy_from_slice(&3_600_000_100u32.to_be_bytes());
    // timescale @12
    p[12..16].copy_from_slice(&timescale.to_be_bytes());
    // duration @16
    p[16..20].copy_from_slice(&duration.to_be_bytes());
    // preferred rate @20 (16.16 fixed) = 0x00010000 -> 1.0
    p[20..24].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    // preferred volume @24 (8.8 fixed) = 0x0100 -> 1.0 = 100%
    p[24..26].copy_from_slice(&0x0100u16.to_be_bytes());
    // next track ID @96
    p[96..100].copy_from_slice(&3u32.to_be_bytes());
    p
}

/// Build a version-1 mvhd payload (64-bit times). Needs to be long enough.
fn mvhd_v1(timescale: u32, duration: u64) -> Vec<u8> {
    // v1 layout: ver(1)+flags(3)+create(8)+modify(8)+timescale(4)+duration(8)=32,
    // then rate(4)+volume(2)+reserved(10)+matrix(36)+predefined(24)+nextTrackID(4)
    let mut p = vec![0u8; 120];
    p[0] = 1;
    p[4..12].copy_from_slice(&3_600_000_000u64.to_be_bytes());
    p[12..20].copy_from_slice(&3_600_000_100u64.to_be_bytes());
    p[20..24].copy_from_slice(&timescale.to_be_bytes());
    p[24..32].copy_from_slice(&duration.to_be_bytes());
    // rate @32
    p[32..36].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    // volume @36
    p[36..38].copy_from_slice(&0x0100u16.to_be_bytes());
    p
}

/// Build a version-0 tkhd payload (>= 84 bytes).
fn tkhd_v0(track_id: u32, width: u32, height: u32, enabled: bool) -> Vec<u8> {
    let mut p = vec![0u8; 84];
    p[0] = 0; // version
    // flags @3, bit0 = enabled
    p[3] = if enabled { 0x01 } else { 0x00 };
    p[4..8].copy_from_slice(&3_600_000_000u32.to_be_bytes()); // create
    p[8..12].copy_from_slice(&3_600_000_050u32.to_be_bytes()); // modify
    p[12..16].copy_from_slice(&track_id.to_be_bytes()); // track id
    // reserved @16
    p[20..24].copy_from_slice(&1000u32.to_be_bytes()); // duration
    // layer @32
    p[32..34].copy_from_slice(&0u16.to_be_bytes());
    // volume @36 (8.8) -> 1.0
    p[36..38].copy_from_slice(&0x0100u16.to_be_bytes());
    // width @60 (16.16), height @64 (16.16)
    p[60..64].copy_from_slice(&(width << 16).to_be_bytes());
    p[64..68].copy_from_slice(&(height << 16).to_be_bytes());
    p
}

/// Build a version-1 tkhd payload (64-bit times, >= 92 bytes).
fn tkhd_v1(track_id: u32, width: u32, height: u32) -> Vec<u8> {
    let mut p = vec![0u8; 104];
    p[0] = 1;
    p[3] = 0x01; // enabled
    p[4..12].copy_from_slice(&3_600_000_000u64.to_be_bytes());
    p[12..20].copy_from_slice(&3_600_000_050u64.to_be_bytes());
    p[20..24].copy_from_slice(&track_id.to_be_bytes());
    p[28..36].copy_from_slice(&1000u64.to_be_bytes()); // duration
    // layer @44
    p[44..46].copy_from_slice(&0u16.to_be_bytes());
    // volume @46
    p[46..48].copy_from_slice(&0x0100u16.to_be_bytes());
    // width @76, height @80
    p[76..80].copy_from_slice(&(width << 16).to_be_bytes());
    p[80..84].copy_from_slice(&(height << 16).to_be_bytes());
    p
}

/// Build a version-0 mdhd payload with timescale + language ("eng").
fn mdhd_v0(timescale: u32, duration: u32) -> Vec<u8> {
    let mut p = vec![0u8; 24];
    p[0] = 0;
    p[4..8].copy_from_slice(&3_600_000_000u32.to_be_bytes());
    p[8..12].copy_from_slice(&3_600_000_050u32.to_be_bytes());
    p[12..16].copy_from_slice(&timescale.to_be_bytes());
    p[16..20].copy_from_slice(&duration.to_be_bytes());
    // language @20: pack "eng" -> each char 5 bits, minus 0x60
    // e=0x65->0x05, n=0x6e->0x0e, g=0x67->0x07
    let lang: u16 = ((0x05u16) << 10) | ((0x0eu16) << 5) | 0x07u16;
    p[20..22].copy_from_slice(&lang.to_be_bytes());
    p
}

/// Build a version-1 mdhd payload (64-bit times).
fn mdhd_v1(timescale: u32, duration: u64) -> Vec<u8> {
    let mut p = vec![0u8; 36];
    p[0] = 1;
    p[4..12].copy_from_slice(&3_600_000_000u64.to_be_bytes());
    p[12..20].copy_from_slice(&3_600_000_050u64.to_be_bytes());
    p[20..24].copy_from_slice(&timescale.to_be_bytes());
    p[24..32].copy_from_slice(&duration.to_be_bytes());
    let lang: u16 = ((0x05u16) << 10) | ((0x0eu16) << 5) | 0x07u16;
    p[32..34].copy_from_slice(&lang.to_be_bytes());
    p
}

/// Build an hdlr payload (>= 24 bytes) with the given component type + handler type.
fn hdlr_payload(component: &[u8], handler: &[u8], vendor: &[u8], name: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 4]); // version/flags
    p.extend_from_slice(component); // component type @4
    p.extend_from_slice(handler); // handler type @8
    p.extend_from_slice(vendor); // reserved/vendor @12
    p.extend_from_slice(&[0u8; 8]); // reserved @16
    p.extend_from_slice(name.as_bytes()); // null-terminated name @24
    p.push(0);
    p
}

/// Build a classic QuickTime user-data string atom payload:
/// [size:u16][lang:u16][text].
fn qt_userdata_string(text: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&(text.len() as u16).to_be_bytes());
    p.extend_from_slice(&[0u8, 0u8]); // language
    p.extend_from_slice(text.as_bytes());
    p
}

/// Build an iTunes "data" atom for a string value (type 1 = UTF-8).
fn itunes_data_utf8(text: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&1u32.to_be_bytes()); // type indicator = UTF-8
    p.extend_from_slice(&0u32.to_be_bytes()); // reserved
    p.extend_from_slice(text.as_bytes());
    atom(b"data", &p)
}

/// Build an iTunes "data" atom for a signed integer value (type 21).
fn itunes_data_int(value: i32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&21u32.to_be_bytes()); // type indicator = signed int
    p.extend_from_slice(&0u32.to_be_bytes()); // reserved
    p.extend_from_slice(&value.to_be_bytes());
    atom(b"data", &p)
}

/// Build an iTunes "data" atom for binary value (type 0), used for trkn/disk.
fn itunes_data_binary(payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&0u32.to_be_bytes()); // type indicator = implicit/binary
    p.extend_from_slice(&0u32.to_be_bytes()); // reserved
    p.extend_from_slice(payload);
    atom(b"data", &p)
}

/// Wrap an ilst-style item atom (e.g., ©nam) around a data atom.
fn ilst_item(item_type: &[u8], data_atom: &[u8]) -> Vec<u8> {
    atom(item_type, data_atom)
}

/// Build a `meta` atom payload (4-byte version/flags header + children).
fn meta_payload(children: &[Vec<u8>]) -> Vec<u8> {
    let mut p = vec![0u8; 4]; // version/flags
    for c in children {
        p.extend_from_slice(c);
    }
    p
}

/// Build a `meta` atom payload WITHOUT the version/flags header.
///
/// `extract_mp4_metadata` reaches keys/ilst via `meta.find_child(...)`, which parses
/// `meta.data` directly (no version/flags skip), so the moov->meta keys/ilst path
/// must have keys/ilst as the immediate first children.
fn meta_payload_no_header(children: &[Vec<u8>]) -> Vec<u8> {
    concat(children)
}

// ---------------------------------------------------------------------------
// Tests: tag-mapping helper (pure function)
// ---------------------------------------------------------------------------

#[test]
fn test_atom_to_exiftool_tag_mapping() {
    assert_eq!(atom_to_exiftool_tag("©nam"), Some("QuickTime:Title"));
    assert_eq!(atom_to_exiftool_tag("©ART"), Some("QuickTime:Artist"));
    assert_eq!(atom_to_exiftool_tag("aART"), Some("QuickTime:AlbumArtist"));
    assert_eq!(atom_to_exiftool_tag("nope"), None);
}

// ---------------------------------------------------------------------------
// Tests: signature validation / error paths
// ---------------------------------------------------------------------------

#[test]
fn test_empty_file_errors() {
    let reader = TestReader::new(vec![]);
    assert!(parse_quicktime_metadata(&reader).is_err());
}

#[test]
fn test_too_small_file_errors() {
    let reader = TestReader::new(vec![0u8; 8]);
    assert!(parse_quicktime_metadata(&reader).is_err());
}

#[test]
fn test_invalid_signature_errors() {
    // 16+ bytes but no recognizable atom type at bytes 4..8 and no moov/ftyp anywhere.
    let mut data = vec![0u8; 64];
    data[4..8].copy_from_slice(b"junk");
    let reader = TestReader::new(data);
    assert!(parse_quicktime_metadata(&reader).is_err());
}

#[test]
fn test_signature_via_moov_atom() {
    // First atom type is "free" (accepted), then a moov atom with mvhd so metadata exists.
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &mvhd);
    let free = atom(b"free", &[0u8; 8]);
    let data = concat(&[free, moov]);
    let reader = TestReader::new(data);
    let md = parse_quicktime_metadata(&reader).expect("should parse");
    assert!(md.contains_key("QuickTime:TimeScale"));
}

// ---------------------------------------------------------------------------
// Tests: ftyp / file-level metadata
// ---------------------------------------------------------------------------

#[test]
fn test_ftyp_brand_and_compatible_brands() {
    let ftyp = atom(
        b"ftyp",
        &ftyp_payload(b"mp42", 0x0001_0203, &[b"isom", b"mp41"]),
    );
    // Need a moov so the file is not "empty of metadata".
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &mvhd);
    let reader = TestReader::new(concat(&[ftyp, moov]));
    let md = parse_quicktime_metadata(&reader).expect("parse ftyp");
    assert_eq!(
        md.get("QuickTime:MajorBrand"),
        Some(&TagValue::String("MP4 v2 [ISO 14496-14]".to_string()))
    );
    // Minor version 0x00010203 -> "1.2.3"
    assert_eq!(
        md.get("QuickTime:MinorVersion"),
        Some(&TagValue::String("1.2.3".to_string()))
    );
    assert!(md.contains_key("QuickTime:CompatibleBrands"));
}

#[test]
fn test_ftyp_heif_and_qt_brands() {
    // qt brand
    let ftyp = atom(b"ftyp", &ftyp_payload(b"qt  ", 0, &[b"qt  "]));
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &mvhd);
    let reader = TestReader::new(concat(&[ftyp, moov]));
    let md = parse_quicktime_metadata(&reader).expect("qt brand");
    assert_eq!(
        md.get("QuickTime:MajorBrand"),
        Some(&TagValue::String("Apple QuickTime (.MOV/QT)".to_string()))
    );
}

#[test]
fn test_mdat_file_level_offsets() {
    let ftyp = atom(b"ftyp", &ftyp_payload(b"isom", 0, &[b"isom"]));
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &mvhd);
    let mdat = atom(b"mdat", &[0xAAu8; 32]);
    let reader = TestReader::new(concat(&[ftyp, moov, mdat]));
    let md = parse_quicktime_metadata(&reader).expect("mdat");
    assert_eq!(
        md.get("QuickTime:MediaDataSize"),
        Some(&TagValue::Integer(32))
    );
    assert!(md.contains_key("QuickTime:MediaDataOffset"));
}

// ---------------------------------------------------------------------------
// Tests: mvhd (movie header)
// ---------------------------------------------------------------------------

#[test]
fn test_mvhd_v0_full() {
    let mvhd = atom(b"mvhd", &mvhd_v0(1000, 10000));
    let moov = atom(b"moov", &mvhd);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("mvhd v0");
    assert_eq!(
        md.get("QuickTime:MovieHeaderVersion"),
        Some(&TagValue::Integer(0))
    );
    assert_eq!(
        md.get("QuickTime:TimeScale"),
        Some(&TagValue::Integer(1000))
    );
    // duration 10000 / 1000 = 10.00 s
    assert_eq!(
        md.get("QuickTime:Duration"),
        Some(&TagValue::String("10.00 s".to_string()))
    );
    assert!(md.contains_key("QuickTime:CreateDate"));
    assert!(md.contains_key("QuickTime:MatrixStructure"));
    assert!(md.contains_key("QuickTime:NextTrackID"));
    assert!(md.contains_key("QuickTime:PreferredRate"));
    assert!(md.contains_key("QuickTime:PreferredVolume"));
}

#[test]
fn test_mvhd_v1_64bit() {
    let mvhd = atom(b"mvhd", &mvhd_v1(48000, 96000));
    let moov = atom(b"moov", &mvhd);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("mvhd v1");
    assert_eq!(
        md.get("QuickTime:MovieHeaderVersion"),
        Some(&TagValue::Integer(1))
    );
    assert_eq!(
        md.get("QuickTime:TimeScale"),
        Some(&TagValue::Integer(48000))
    );
}

#[test]
fn test_mvhd_too_short_no_panic() {
    // mvhd data < 100 bytes: extractor returns early but mvhd presence alone
    // produces no metadata -> the whole file has no metadata -> error.
    let mvhd = atom(b"mvhd", &[0u8; 20]);
    let moov = atom(b"moov", &mvhd);
    let reader = TestReader::new(moov);
    let res = parse_quicktime_metadata(&reader);
    assert!(res.is_err());
}

// ---------------------------------------------------------------------------
// Tests: full track tree (tkhd / mdia{mdhd,minf{vmhd/smhd,stbl{stsd,stts}}})
// ---------------------------------------------------------------------------

/// Build a video sample-description (stsd) payload with one avc1 entry.
fn stsd_video(codec: &[u8], width: u16, height: u16) -> Vec<u8> {
    // stsd: version/flags(4) + entry_count(4) + entry
    // entry: size(4) + format(4) + reserved(6) + data_ref_index(2)
    //        + version(2)+revision(2)+vendor(4)+temporalQ(4)+spatialQ(4)
    //        + width(2)+height(2)+xres(4)+yres(4)+datasize(4)+framecount(2)
    //        + compressorname(32)+depth(2)+colortableid(2)
    let mut entry = Vec::new();
    // we'll fill size after building
    entry.extend_from_slice(&[0u8; 4]); // placeholder size
    entry.extend_from_slice(codec); // format @4
    entry.extend_from_slice(&[0u8; 6]); // reserved @8
    entry.extend_from_slice(&1u16.to_be_bytes()); // data_ref_index @14
    entry.extend_from_slice(&0u16.to_be_bytes()); // version @16
    entry.extend_from_slice(&0u16.to_be_bytes()); // revision @18
    entry.extend_from_slice(b"appl"); // vendor @20
    entry.extend_from_slice(&[0u8; 4]); // temporal quality @24
    entry.extend_from_slice(&[0u8; 4]); // spatial quality @28
    entry.extend_from_slice(&width.to_be_bytes()); // width @32
    entry.extend_from_slice(&height.to_be_bytes()); // height @34
    entry.extend_from_slice(&(72u32 << 16).to_be_bytes()); // xres @36 = 72
    entry.extend_from_slice(&(72u32 << 16).to_be_bytes()); // yres @40 = 72
    entry.extend_from_slice(&[0u8; 4]); // data size @44
    entry.extend_from_slice(&1u16.to_be_bytes()); // frame count @48
    entry.extend_from_slice(&[0u8; 32]); // compressor name @50
    entry.extend_from_slice(&24u16.to_be_bytes()); // depth @82
    entry.extend_from_slice(&0xFFFFu16.to_be_bytes()); // color table id @84
    // Now entry is 86 bytes; patch in its size.
    let entry_size = entry.len() as u32;
    entry[0..4].copy_from_slice(&entry_size.to_be_bytes());

    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 4]); // version/flags
    p.extend_from_slice(&1u32.to_be_bytes()); // entry count
    p.extend_from_slice(&entry);
    p
}

/// Build an audio sample-description (stsd) payload with one mp4a entry.
fn stsd_audio(codec: &[u8], channels: u16, sample_rate: u16) -> Vec<u8> {
    let mut entry = Vec::new();
    entry.extend_from_slice(&[0u8; 4]); // placeholder size
    entry.extend_from_slice(codec); // format @4
    entry.extend_from_slice(&[0u8; 6]); // reserved @8
    entry.extend_from_slice(&1u16.to_be_bytes()); // data_ref_index @14
    entry.extend_from_slice(&0u16.to_be_bytes()); // version @16
    entry.extend_from_slice(&0u16.to_be_bytes()); // revision @18
    entry.extend_from_slice(&[0u8; 4]); // vendor @20
    entry.extend_from_slice(&channels.to_be_bytes()); // channel count @24
    entry.extend_from_slice(&16u16.to_be_bytes()); // sample size @26
    entry.extend_from_slice(&0u16.to_be_bytes()); // compression id @28
    entry.extend_from_slice(&0u16.to_be_bytes()); // packet size @30
    entry.extend_from_slice(&((sample_rate as u32) << 16).to_be_bytes()); // sample rate @32
    let entry_size = entry.len() as u32;
    entry[0..4].copy_from_slice(&entry_size.to_be_bytes());

    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 4]);
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&entry);
    p
}

/// Build an stts payload with one entry (sample_count, sample_delta).
fn stts_payload(sample_count: u32, sample_delta: u32) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 4]); // version/flags
    p.extend_from_slice(&1u32.to_be_bytes()); // entry count
    p.extend_from_slice(&sample_count.to_be_bytes());
    p.extend_from_slice(&sample_delta.to_be_bytes());
    p
}

#[test]
fn test_full_video_track_tree() {
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));

    let tkhd = atom(b"tkhd", &tkhd_v0(1, 1920, 1080, true));
    let mdhd = atom(b"mdhd", &mdhd_v0(30000, 60000));
    let vmhd = atom(b"vmhd", &{
        let mut v = vec![0u8; 12];
        v[3] = 0x01; // flags
        v[4..6].copy_from_slice(&0x0000u16.to_be_bytes()); // srcCopy
        v[6..8].copy_from_slice(&1u16.to_be_bytes()); // opcolor R
        v
    });
    let stsd = atom(b"stsd", &stsd_video(b"avc1", 1920, 1080));
    let stts = atom(b"stts", &stts_payload(60, 1000)); // 30000/1000 = 30 fps
    let stbl = atom(b"stbl", &concat(&[stsd, stts]));
    let minf = atom(b"minf", &concat(&[vmhd, stbl]));
    let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
    let trak = atom(b"trak", &concat(&[tkhd, mdia]));
    let moov = atom(b"moov", &concat(&[mvhd, trak]));

    let ftyp = atom(b"ftyp", &ftyp_payload(b"isom", 0, &[b"isom"]));
    let reader = TestReader::new(concat(&[ftyp, moov]));
    let md = parse_quicktime_metadata(&reader).expect("full video track");

    assert_eq!(md.get("QuickTime:TrackID"), Some(&TagValue::Integer(1)));
    assert_eq!(
        md.get("QuickTime:TrackEnabled"),
        Some(&TagValue::String("Yes".to_string()))
    );
    assert!(md.contains_key("QuickTime:TrackWidth"));
    assert!(md.contains_key("QuickTime:TrackHeight"));
    assert!(md.contains_key("QuickTime:MediaTimeScale"));
    assert_eq!(
        md.get("QuickTime:MediaLanguageCode"),
        Some(&TagValue::String("eng".to_string()))
    );
    assert!(md.contains_key("QuickTime:GraphicsMode"));
    assert!(md.contains_key("QuickTime:OpColor"));
    assert_eq!(
        md.get("QuickTime:CompressorID"),
        Some(&TagValue::String("avc1".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:CompressorName"),
        Some(&TagValue::String("H.264/AVC".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:ImageWidth"),
        Some(&TagValue::Integer(1920))
    );
    assert_eq!(
        md.get("QuickTime:ImageHeight"),
        Some(&TagValue::Integer(1080))
    );
    assert_eq!(
        md.get("QuickTime:VendorID"),
        Some(&TagValue::String("Apple".to_string()))
    );
    assert!(md.contains_key("QuickTime:VideoFrameRate"));
}

#[test]
fn test_full_audio_track_tree() {
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));

    let tkhd = atom(b"tkhd", &tkhd_v0(2, 0, 0, true));
    let mdhd = atom(b"mdhd", &mdhd_v0(44100, 88200));
    let smhd = atom(b"smhd", &{
        let mut v = vec![0u8; 8];
        v[4..6].copy_from_slice(&0i16.to_be_bytes()); // balance 0
        v
    });
    // dinf -> dref with an 'alis' entry to exercise data-handler info.
    let dref = atom(b"dref", &{
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 4]); // version/flags
        p.extend_from_slice(&1u32.to_be_bytes()); // entry count
        // entry: size(4)+type(4)+version/flags(4)
        let entry = atom(b"alis", &[0u8; 4]);
        p.extend_from_slice(&entry);
        p
    });
    let dinf = atom(b"dinf", &dref);
    let stsd = atom(b"stsd", &stsd_audio(b"mp4a", 2, 44100));
    let stbl = atom(b"stbl", &stsd);
    let minf = atom(b"minf", &concat(&[smhd, dinf, stbl]));
    let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
    let trak = atom(b"trak", &concat(&[tkhd, mdia]));
    let moov = atom(b"moov", &concat(&[mvhd, trak]));

    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("audio track");

    assert_eq!(
        md.get("QuickTime:CompressorID"),
        Some(&TagValue::String("mp4a".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:AudioFormat"),
        Some(&TagValue::String("mp4a".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:AudioChannels"),
        Some(&TagValue::Integer(2))
    );
    assert!(md.contains_key("QuickTime:AudioSampleRate"));
    assert!(md.contains_key("QuickTime:AudioBitsPerSample"));
    assert!(md.contains_key("QuickTime:Balance"));
    assert_eq!(
        md.get("QuickTime:HandlerClass"),
        Some(&TagValue::String("Data Handler".to_string()))
    );
}

#[test]
fn test_tkhd_v1_and_tapt() {
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let tkhd = atom(b"tkhd", &tkhd_v1(7, 640, 480));

    // tapt with clef/prof/enof aperture atoms (each version+flags(4)+w(16.16)+h(16.16)).
    let aperture = |w: u32, h: u32| -> Vec<u8> {
        let mut v = vec![0u8; 12];
        v[4..8].copy_from_slice(&(w << 16).to_be_bytes());
        v[8..12].copy_from_slice(&(h << 16).to_be_bytes());
        v
    };
    let clef = atom(b"clef", &aperture(640, 480));
    let prof = atom(b"prof", &aperture(640, 480));
    let enof = atom(b"enof", &aperture(640, 480));
    let tapt = atom(b"tapt", &concat(&[clef, prof, enof]));

    let mdhd = atom(b"mdhd", &mdhd_v1(30000, 60000));
    let stsd = atom(b"stsd", &stsd_video(b"hev1", 640, 480));
    let stbl = atom(b"stbl", &stsd);
    let minf = atom(b"minf", &stbl);
    let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
    let trak = atom(b"trak", &concat(&[tkhd, tapt, mdia]));
    let moov = atom(b"moov", &concat(&[mvhd, trak]));

    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("tkhd v1 + tapt");

    assert_eq!(md.get("QuickTime:TrackID"), Some(&TagValue::Integer(7)));
    assert_eq!(
        md.get("QuickTime:TrackHeaderVersion"),
        Some(&TagValue::Integer(1))
    );
    assert!(md.contains_key("QuickTime:CleanApertureDimensions"));
    assert!(md.contains_key("QuickTime:ProductionApertureDimensions"));
    assert!(md.contains_key("QuickTime:EncodedPixelsDimensions"));
}

#[test]
fn test_two_tracks_get_suffix() {
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));

    let make_track = |id: u32, codec: &[u8]| -> Vec<u8> {
        let tkhd = atom(b"tkhd", &tkhd_v0(id, 320, 240, true));
        let mdhd = atom(b"mdhd", &mdhd_v0(30000, 60000));
        let stsd = atom(b"stsd", &stsd_video(codec, 320, 240));
        let stbl = atom(b"stbl", &stsd);
        let minf = atom(b"minf", &stbl);
        let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
        atom(b"trak", &concat(&[tkhd, mdia]))
    };

    let trak1 = make_track(1, b"avc1");
    let trak2 = make_track(2, b"mp4v");
    let moov = atom(b"moov", &concat(&[mvhd, trak1, trak2]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("two tracks");

    // First track: no suffix. Second track: _2 suffix.
    assert_eq!(md.get("QuickTime:TrackID"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get("QuickTime:TrackID_2"), Some(&TagValue::Integer(2)));
}

// ---------------------------------------------------------------------------
// Tests: udta classic user data (©nam, ©ART, ©xyz GPS, 3GPP atoms)
// ---------------------------------------------------------------------------

#[test]
fn test_udta_classic_user_data_and_gps() {
    let nam = atom(&cr_type(b"nam"), &qt_userdata_string("My Movie"));
    let art = atom(&cr_type(b"ART"), &qt_userdata_string("The Artist"));
    let cmt = atom(&cr_type(b"cmt"), &qt_userdata_string("A comment"));
    let xyz = atom(
        &cr_type(b"xyz"),
        &qt_userdata_string("+37.7749-122.4194+010.0/"),
    );
    let udta = atom(b"udta", &concat(&[nam, art, cmt, xyz]));
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &concat(&[mvhd, udta]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("udta classic");

    assert_eq!(
        md.get("QuickTime:Title"),
        Some(&TagValue::String("My Movie".to_string()))
    );
    assert_eq!(
        md.get("UserData:Title"),
        Some(&TagValue::String("My Movie".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Artist"),
        Some(&TagValue::String("The Artist".to_string()))
    );
    // GPS from ©xyz
    assert!(md.contains_key("QuickTime:GPSLatitude"));
    assert!(md.contains_key("QuickTime:GPSLongitude"));
    assert!(md.contains_key("QuickTime:GPSAltitude"));
    assert!(md.contains_key("QuickTime:GPSCoordinates"));
}

#[test]
fn test_udta_3gpp_atoms() {
    // 3GPP-style atoms: version/flags(4) + language(2) + text.
    let gpp_string = |text: &str| -> Vec<u8> {
        let mut p = Vec::new();
        p.extend_from_slice(&[0u8; 4]); // version/flags
        p.extend_from_slice(&[0u8, 0u8]); // language
        p.extend_from_slice(text.as_bytes());
        p.push(0);
        p
    };
    let cprt = atom(b"cprt", &gpp_string("(c) 2024"));
    let auth = atom(b"auth", &gpp_string("Author Name"));
    let titl = atom(b"titl", &gpp_string("3GPP Title"));
    let dscp = atom(b"dscp", &gpp_string("Desc"));
    let perf = atom(b"perf", &gpp_string("Performer"));
    let albm = atom(b"albm", &gpp_string("Album"));

    // yrrc (year) and gnre (genre id) numeric atoms.
    let yrrc = atom(b"yrrc", &{
        let mut v = vec![0u8; 6];
        v[4..6].copy_from_slice(&2024u16.to_be_bytes());
        v
    });
    let gnre = atom(b"gnre", &{
        let mut v = vec![0u8; 6];
        v[4..6].copy_from_slice(&17u16.to_be_bytes()); // "Rock"
        v
    });
    let make = atom(b"MAKE", b"Canon\0");
    let modl = atom(b"MODL", b"EOS R5\0");

    let udta = atom(
        b"udta",
        &concat(&[cprt, auth, titl, dscp, perf, albm, yrrc, gnre, make, modl]),
    );
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &concat(&[mvhd, udta]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("3gpp atoms");

    assert_eq!(
        md.get("QuickTime:Copyright"),
        Some(&TagValue::String("(c) 2024".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Author"),
        Some(&TagValue::String("Author Name".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Title"),
        Some(&TagValue::String("3GPP Title".to_string()))
    );
    assert_eq!(md.get("QuickTime:Year"), Some(&TagValue::Integer(2024)));
    assert_eq!(
        md.get("QuickTime:Genre"),
        Some(&TagValue::String("Rock".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Make"),
        Some(&TagValue::String("Canon".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Model"),
        Some(&TagValue::String("EOS R5".to_string()))
    );
}

#[test]
fn test_udta_xmp_atom() {
    let xmp = br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>XMP Title</dc:title></rdf:Description></rdf:RDF></x:xmpmeta><?xpacket end="w"?>"#;
    let xmp_atom = atom(b"XMP_", xmp);
    let udta = atom(b"udta", &xmp_atom);
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &concat(&[mvhd, udta]));
    let reader = TestReader::new(moov);
    // Just exercise the path; XMP parsing may or may not yield tags.
    let res = parse_quicktime_metadata(&reader);
    assert!(res.is_ok());
}

// ---------------------------------------------------------------------------
// Tests: iTunes-style metadata (udta -> meta -> ilst)
// ---------------------------------------------------------------------------

#[test]
fn test_itunes_ilst_metadata() {
    // ilst items
    let nam = ilst_item(&cr_type(b"nam"), &itunes_data_utf8("iTunes Title"));
    let art = ilst_item(&cr_type(b"ART"), &itunes_data_utf8("iTunes Artist"));
    let alb = ilst_item(&cr_type(b"alb"), &itunes_data_utf8("The Album"));
    let day = ilst_item(&cr_type(b"day"), &itunes_data_utf8("2021-05-06"));
    let tmpo = ilst_item(b"tmpo", &itunes_data_int(128));
    // trkn: binary 0000 0003 0009 0000 -> "3 of 9"
    let trkn = ilst_item(
        b"trkn",
        &itunes_data_binary(&[0x00, 0x00, 0x00, 0x03, 0x00, 0x09, 0x00, 0x00]),
    );
    let aart = ilst_item(b"aART", &itunes_data_utf8("Album Artist"));
    let ilst = atom(b"ilst", &concat(&[nam, art, alb, day, tmpo, trkn, aart]));

    // hdlr with mdir handler type marks metadata.
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"mdir", b"appl", "mdir"),
    );
    let meta = atom(b"meta", &meta_payload(&[hdlr, ilst]));
    let udta = atom(b"udta", &meta);
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &concat(&[mvhd, udta]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("itunes ilst");

    assert_eq!(
        md.get("ItemList:Title"),
        Some(&TagValue::String("iTunes Title".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Title"),
        Some(&TagValue::String("iTunes Title".to_string()))
    );
    assert_eq!(
        md.get("ItemList:Artist"),
        Some(&TagValue::String("iTunes Artist".to_string()))
    );
    assert_eq!(
        md.get("ItemList:AlbumArtist"),
        Some(&TagValue::String("Album Artist".to_string()))
    );
    // ©day -> ContentCreateDate + Year
    assert!(md.contains_key("ItemList:ContentCreateDate"));
    assert_eq!(
        md.get("ItemList:Year"),
        Some(&TagValue::String("2021".to_string()))
    );
    // trkn formatted "3 of 9"
    assert_eq!(
        md.get("QuickTime:TrackNumber"),
        Some(&TagValue::String("3 of 9".to_string()))
    );
    // handler type
    assert_eq!(
        md.get("QuickTime:HandlerType"),
        Some(&TagValue::String("Metadata".to_string()))
    );
}

#[test]
fn test_itunes_utf16_and_unknown_atom() {
    // UTF-16 BE "Hi" data atom (type 2).
    let utf16_data = {
        let mut p = Vec::new();
        p.extend_from_slice(&2u32.to_be_bytes()); // type 2 = UTF-16
        p.extend_from_slice(&0u32.to_be_bytes());
        p.extend_from_slice(&[0x00, 0x48, 0x00, 0x69]); // "Hi"
        atom(b"data", &p)
    };
    let cmt = ilst_item(&cr_type(b"cmt"), &utf16_data);
    // Unknown 4-char atom -> falls through to "ItemList:<type>".
    let xxxx = ilst_item(b"abcd", &itunes_data_utf8("custom"));
    let ilst = atom(b"ilst", &concat(&[cmt, xxxx]));
    let hdlr = atom(b"hdlr", &hdlr_payload(b"\0\0\0\0", b"mdir", b"appl", ""));
    let meta = atom(b"meta", &meta_payload(&[hdlr, ilst]));
    let udta = atom(b"udta", &meta);
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &concat(&[mvhd, udta]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("utf16/unknown");

    assert_eq!(
        md.get("ItemList:Comment"),
        Some(&TagValue::String("Hi".to_string()))
    );
    assert_eq!(
        md.get("ItemList:abcd"),
        Some(&TagValue::String("custom".to_string()))
    );
}

// ---------------------------------------------------------------------------
// Tests: MP4 keys/ilst metadata (moov -> meta -> keys + ilst)
// ---------------------------------------------------------------------------

#[test]
fn test_mp4_keys_ilst_metadata() {
    // keys atom: version/flags(4) + entry_count(4) + entries.
    // each entry: key_size(4) + namespace(4) + key_value.
    let make_key = |namespace: &[u8], key: &str| -> Vec<u8> {
        let mut e = Vec::new();
        let key_size = (8 + key.len()) as u32;
        e.extend_from_slice(&key_size.to_be_bytes());
        e.extend_from_slice(namespace);
        e.extend_from_slice(key.as_bytes());
        e
    };
    let k1 = make_key(b"mdta", "com.apple.quicktime.make");
    let k2 = make_key(b"mdta", "com.apple.quicktime.model");
    let k3 = make_key(b"mdta", "com.apple.quicktime.location.ISO6709");
    let mut keys_payload = Vec::new();
    keys_payload.extend_from_slice(&[0u8; 4]); // version/flags
    keys_payload.extend_from_slice(&3u32.to_be_bytes()); // entry count
    keys_payload.extend_from_slice(&concat(&[k1, k2, k3]));
    let keys = atom(b"keys", &keys_payload);

    // ilst items: atom type is a big-endian index (1-based).
    let item_for_index =
        |idx: u32, data_atom: Vec<u8>| -> Vec<u8> { atom(&idx.to_be_bytes(), &data_atom) };
    let i1 = item_for_index(1, itunes_data_utf8("Apple"));
    let i2 = item_for_index(2, itunes_data_utf8("iPhone 15"));
    let i3 = item_for_index(3, itunes_data_utf8("+37.0+122.0+010.0/"));
    let ilst = atom(b"ilst", &concat(&[i1, i2, i3]));

    let meta = atom(b"meta", &meta_payload_no_header(&[keys, ilst]));
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &concat(&[mvhd, meta]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("mp4 keys/ilst");

    assert_eq!(
        md.get("QuickTime:Make"),
        Some(&TagValue::String("Apple".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Model"),
        Some(&TagValue::String("iPhone 15".to_string()))
    );
    // ISO6709 -> GPS coordinates parsed
    assert!(md.contains_key("QuickTime:GPSLatitude"));
    assert!(md.contains_key("QuickTime:GPSLongitude"));
}

// ---------------------------------------------------------------------------
// Tests: HEIF root-level meta (iinf / iloc / ispe / pitm / hvcC)
// ---------------------------------------------------------------------------

#[test]
fn test_heif_root_meta_ispe_pitm() {
    // ispe atom: version/flags(4) + width(4) + height(4).
    let ispe = atom(b"ispe", &{
        let mut v = vec![0u8; 12];
        v[4..8].copy_from_slice(&4032u32.to_be_bytes());
        v[8..12].copy_from_slice(&3024u32.to_be_bytes());
        v
    });
    let ipco = atom(b"ipco", &ispe);
    let iprp = atom(b"iprp", &ipco);

    // pitm atom: version(1)+flags(3)+item_id(2 for v0).
    let pitm = atom(b"pitm", &{
        let mut v = vec![0u8; 6];
        v[4..6].copy_from_slice(&1u16.to_be_bytes());
        v
    });

    // hdlr with "pict" handler type.
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"pict", b"\0\0\0\0", "HEIF"),
    );

    let meta = atom(b"meta", &meta_payload(&[hdlr, iprp, pitm]));
    let ftyp = atom(b"ftyp", &ftyp_payload(b"heic", 0, &[b"heic", b"mif1"]));
    let reader = TestReader::new(concat(&[ftyp, meta]));
    let md = parse_quicktime_metadata(&reader).expect("heif root meta");

    assert_eq!(md.get("HEIF:ImageWidth"), Some(&TagValue::Integer(4032)));
    assert_eq!(md.get("HEIF:ImageHeight"), Some(&TagValue::Integer(3024)));
    assert_eq!(
        md.get("QuickTime:ImageSpatialExtent"),
        Some(&TagValue::String("4032x3024".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:PrimaryItemReference"),
        Some(&TagValue::Integer(1))
    );
    assert_eq!(
        md.get("QuickTime:HandlerType"),
        Some(&TagValue::String("Picture".to_string()))
    );
}

#[test]
fn test_heif_iinf_with_exif_item() {
    // iinf version 0: version/flags(4) + entry_count(2) + infe atoms.
    // infe: version/flags(4) + item_id(2) + protection(2) + item_type(4)...
    let infe_payload = {
        let mut v = Vec::new();
        v.extend_from_slice(&[0u8; 4]); // version/flags
        v.extend_from_slice(&5u16.to_be_bytes()); // item_id = 5
        v.extend_from_slice(&0u16.to_be_bytes()); // protection index
        v.extend_from_slice(b"Exif"); // item type
        v
    };
    let infe = atom(b"infe", &infe_payload);
    let mut iinf_payload = Vec::new();
    iinf_payload.extend_from_slice(&[0u8; 4]); // version 0 / flags
    iinf_payload.extend_from_slice(&1u16.to_be_bytes()); // entry count
    iinf_payload.extend_from_slice(&infe);
    let iinf = atom(b"iinf", &iinf_payload);

    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"pict", b"\0\0\0\0", "HEIF"),
    );
    let meta = atom(b"meta", &meta_payload(&[hdlr, iinf]));
    let ftyp = atom(b"ftyp", &ftyp_payload(b"mif1", 0, &[b"mif1"]));
    let reader = TestReader::new(concat(&[ftyp, meta]));
    let md = parse_quicktime_metadata(&reader).expect("heif iinf");

    assert_eq!(md.get("HEIF:ItemCount"), Some(&TagValue::Integer(1)));
}

#[test]
fn test_heif_hvcc_in_ipco() {
    // hvcC config record (>= 23 bytes).
    let mut hvcc = vec![0u8; 23];
    hvcc[0] = 1; // config version
    hvcc[1] = 0x01; // profile space=0, tier=0, profile_idc=1 (Main)
    hvcc[2..6].copy_from_slice(&0x8000_0000u32.to_be_bytes()); // compat: Main
    hvcc[12] = 120; // level idc
    hvcc[16] = 0x01; // chroma 4:2:0
    hvcc[17] = 0x00; // bit depth luma 8
    hvcc[18] = 0x00; // bit depth chroma 8
    let hvcc_atom = atom(b"hvcC", &hvcc);
    let ipco = atom(b"ipco", &hvcc_atom);
    let iprp = atom(b"iprp", &ipco);
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"pict", b"\0\0\0\0", "HEIF"),
    );
    let meta = atom(b"meta", &meta_payload(&[hdlr, iprp]));
    let ftyp = atom(b"ftyp", &ftyp_payload(b"heic", 0, &[b"heic"]));
    let reader = TestReader::new(concat(&[ftyp, meta]));
    let md = parse_quicktime_metadata(&reader).expect("heif hvcC");

    assert_eq!(
        md.get("QuickTime:HEVCConfigurationVersion"),
        Some(&TagValue::Integer(1))
    );
    assert_eq!(
        md.get("QuickTime:GeneralProfileIDC"),
        Some(&TagValue::String("Main".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:ChromaFormat"),
        Some(&TagValue::String("4:2:0".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:BitDepthLuma"),
        Some(&TagValue::Integer(8))
    );
}

// ---------------------------------------------------------------------------
// Tests: real fixture + production read_metadata path via tempfile
// ---------------------------------------------------------------------------

#[test]
fn test_real_fixture_sample_mp4_direct() {
    let bytes = std::fs::read("tests/fixtures/mp4/sample.mp4").expect("read sample.mp4");
    let reader = TestReader::new(bytes);
    let md = parse_quicktime_metadata(&reader).expect("parse sample.mp4");
    assert!(!md.is_empty());
    assert!(md.contains_key("QuickTime:MajorBrand"));
}

#[test]
fn test_production_read_metadata_synthetic_mp4() {
    use std::io::Write;
    // Build a synthetic but complete mp4 with ftyp + moov{mvhd, udta{©nam}}.
    let nam = atom(&cr_type(b"nam"), &qt_userdata_string("Prod Title"));
    let udta = atom(b"udta", &nam);
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &concat(&[mvhd, udta]));
    let ftyp = atom(b"ftyp", &ftyp_payload(b"isom", 0, &[b"isom", b"mp41"]));
    let data = concat(&[ftyp, moov]);

    let mut tmp = tempfile::Builder::new()
        .suffix(".mp4")
        .tempfile()
        .expect("tempfile");
    tmp.write_all(&data).expect("write");
    tmp.flush().expect("flush");

    let md = oxidex::core::operations::read_metadata(tmp.path()).expect("read_metadata");
    assert_eq!(
        md.get("QuickTime:Title"),
        Some(&TagValue::String("Prod Title".to_string()))
    );
    assert!(md.contains_key("QuickTime:TimeScale"));
}

#[test]
fn test_production_read_metadata_real_fixture() {
    let md = oxidex::core::operations::read_metadata(std::path::Path::new(
        "tests/fixtures/mp4/sample.mp4",
    ))
    .expect("read_metadata fixture");
    assert!(md.contains_key("QuickTime:MajorBrand"));
}

// ---------------------------------------------------------------------------
// Tests: malformed / edge-case inputs for error paths
// ---------------------------------------------------------------------------

#[test]
fn test_moov_without_metadata_errors() {
    // moov with only a too-short mvhd produces no metadata at all.
    let mvhd = atom(b"mvhd", &[0u8; 10]);
    let moov = atom(b"moov", &mvhd);
    let ftyp = atom(b"ftyp", &ftyp_payload(b"isom", 0, &[]));
    // ftyp brand alone yields MajorBrand, so this should still be Ok.
    let reader = TestReader::new(concat(&[ftyp, moov]));
    let md = parse_quicktime_metadata(&reader).expect("ftyp gives metadata");
    assert!(md.contains_key("QuickTime:MajorBrand"));
}

#[test]
fn test_truncated_ftyp_only() {
    // ftyp with brand but truncated minor version (only 4 bytes payload).
    let ftyp = atom(b"ftyp", b"mp42");
    // pad to satisfy 16-byte minimum for signature read
    let free = atom(b"free", &[0u8; 8]);
    let reader = TestReader::new(concat(&[ftyp, free]));
    // No moov, ftyp < 8 bytes payload -> brand not parsed (needs >= 8) -> no metadata -> err.
    let res = parse_quicktime_metadata(&reader);
    assert!(res.is_err());
}

#[test]
fn test_udta_empty_no_metadata() {
    // moov -> udta (empty) + mvhd present so still Ok.
    let udta = atom(b"udta", &[]);
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &concat(&[mvhd, udta]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("empty udta ok");
    assert!(md.contains_key("QuickTime:TimeScale"));
}

#[test]
fn test_classic_quicktime_starting_with_moov() {
    // No ftyp at all; file begins with moov (classic QuickTime).
    let nam = atom(&cr_type(b"nam"), &qt_userdata_string("Classic"));
    let udta = atom(b"udta", &nam);
    let mvhd = atom(b"mvhd", &mvhd_v0(600, 6000));
    let moov = atom(b"moov", &concat(&[mvhd, udta]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("classic moov");
    assert_eq!(
        md.get("UserData:Title"),
        Some(&TagValue::String("Classic".to_string()))
    );
}
