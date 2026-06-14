//! Wave-2 coverage tests for the QuickTime/ISOBMFF metadata extractor.
//!
//! Wave-1 (`cov_quicktime.rs`) hits the happy path: ftyp/mvhd/tkhd/mdhd, a video
//! track tree, an audio track tree, the basic iTunes ilst path, MP4 keys/ilst, and
//! a few HEIF boxes. This file deliberately targets the *remaining* uncovered code
//! in `src/parsers/quicktime/metadata_extractor.rs`:
//!
//! - the many non-© udta branches (FIRM/INFO, CNCV/CNFV/CNMN, PENT/PXTH, tmpo,
//!   `fmt `, MAKE/MODL, TAGS Pentax maker notes, XMP_)
//! - the full set of ©-prefixed legacy suffixes and the `atom_to_exiftool_tag` map
//! - `decode_id3_genre` across many genre IDs plus its `Genre {n}` fallback
//! - more iTunes ilst item types (disk, desc, ldes, covr, gnre, integer tmpo,
//!   the unknown-atom fallback, UTF-16)
//! - `map_apple_key_to_tag` for every mapped Apple key + the MP4: fallback
//! - `extract_handler_metadata` vendor-id + Pascal/null handler-description paths
//!   and every handler-type enum arm
//! - the full HEIF EXIF pipeline: iinf(Exif item) + iloc + mdat -> TIFF/IFD parse,
//!   including `raw_bytes_to_tag_value` for several EXIF field types
//! - `format_mac_time_legacy` via pre-1970 mvhd timestamps
//! - malformed / truncated boxes for the early-return branches
//! - the real `tests/fixtures/mp4/*.mp4` fixtures through `read_metadata`

#[path = "common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::quicktime::parse_quicktime_metadata;
use oxidex::parsers::quicktime::tag_mapping::atom_to_exiftool_tag;

// ---------------------------------------------------------------------------
// Box-building helpers (mirrors cov_quicktime.rs style)
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

/// version-1 mvhd payload (64-bit times) with full matrix + time + next-track fields.
fn mvhd_v1_full(timescale: u32, duration: u64) -> Vec<u8> {
    // ver(1)+flags(3)+create(8)+modify(8)+ts(4)+dur(8)=32, rate(4)+vol(2)+res(10)
    // +matrix(36)+predefined(24)+nextTrackID(4) -> need >= 32+16+36+24+4 = 112
    let mut p = vec![0u8; 120];
    p[0] = 1;
    p[4..12].copy_from_slice(&3_600_000_000u64.to_be_bytes());
    p[12..20].copy_from_slice(&3_600_000_100u64.to_be_bytes());
    p[20..24].copy_from_slice(&timescale.to_be_bytes());
    p[24..32].copy_from_slice(&duration.to_be_bytes());
    p[32..36].copy_from_slice(&0x0001_0000u32.to_be_bytes()); // rate
    p[36..38].copy_from_slice(&0x0100u16.to_be_bytes()); // volume
    // identity-ish matrix at offset 48 (rate_offset=32, matrix=rate_offset+16=48)
    p[48..52].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    p[64..68].copy_from_slice(&0x0001_0000u32.to_be_bytes());
    p[80..84].copy_from_slice(&0x4000_0000u32.to_be_bytes());
    // next track id at time_offset(=rate_offset+52=84)+24 = 108
    p[108..112].copy_from_slice(&5u32.to_be_bytes());
    p
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

fn hdlr_payload(
    component: &[u8],
    handler: &[u8],
    vendor: &[u8],
    name: &[u8],
    pascal: bool,
) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 4]); // version/flags
    p.extend_from_slice(component); // @4
    p.extend_from_slice(handler); // @8
    p.extend_from_slice(vendor); // @12
    p.extend_from_slice(&[0u8; 8]); // reserved @16
    if pascal {
        p.push(name.len() as u8);
        p.extend_from_slice(name);
    } else {
        p.extend_from_slice(name);
        p.push(0);
    }
    p
}

fn qt_userdata_string(text: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&(text.len() as u16).to_be_bytes());
    p.extend_from_slice(&[0u8, 0u8]); // language
    p.extend_from_slice(text.as_bytes());
    p
}

fn itunes_data_utf8(text: &str) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(text.as_bytes());
    atom(b"data", &p)
}

fn itunes_data_int16(value: i16) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&21u32.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(&value.to_be_bytes());
    atom(b"data", &p)
}

fn itunes_data_int8(value: u8) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&21u32.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes());
    p.push(value);
    atom(b"data", &p)
}

fn itunes_data_binary(type_indicator: u32, payload: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    p.extend_from_slice(&type_indicator.to_be_bytes());
    p.extend_from_slice(&0u32.to_be_bytes());
    p.extend_from_slice(payload);
    atom(b"data", &p)
}

fn ilst_item(item_type: &[u8], data_atom: &[u8]) -> Vec<u8> {
    atom(item_type, data_atom)
}

fn meta_payload(children: &[Vec<u8>]) -> Vec<u8> {
    let mut p = vec![0u8; 4];
    for c in children {
        p.extend_from_slice(c);
    }
    p
}

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

// ---------------------------------------------------------------------------
// tag_mapping helper: exhaustive coverage of atom_to_exiftool_tag
// ---------------------------------------------------------------------------

#[test]
fn test_atom_to_exiftool_tag_many_keys() {
    // Exercise a broad set of mapped keys. We only require that *some* of them
    // resolve; the point is to run the lookup branch many times.
    let candidates = [
        "©nam", "©ART", "©alb", "©day", "©cmt", "©gen", "©too", "©wrt", "©grp", "©lyr", "©cpy",
        "©des", "©dir", "©prd", "©prf", "©fmt", "©inf", "aART", "trkn", "disk", "covr", "tmpo",
        "gnre", "desc", "ldes", "cprt",
    ];
    let mut resolved = 0usize;
    for c in candidates {
        if atom_to_exiftool_tag(c).is_some() {
            resolved += 1;
        }
    }
    assert!(resolved > 0, "expected at least one mapped key");
    // Unknown keys must not resolve.
    assert_eq!(atom_to_exiftool_tag("zzzz"), None);
    assert_eq!(atom_to_exiftool_tag("1234"), None);
}

// ---------------------------------------------------------------------------
// udta: full set of ©-prefixed legacy suffixes
// ---------------------------------------------------------------------------

#[test]
fn test_udta_all_copyright_suffixes() {
    let items = [
        (cr_type(b"nam"), "Title", "name value"),
        (cr_type(b"ART"), "Artist", "artist value"),
        (cr_type(b"alb"), "Album", "album value"),
        (cr_type(b"day"), "Year", "2020"),
        (cr_type(b"cmt"), "Comment", "comment value"),
        (cr_type(b"cpy"), "Copyright", "(c) value"),
        (cr_type(b"gen"), "Genre", "genre value"),
        (cr_type(b"too"), "Encoder", "encoder value"),
        (cr_type(b"des"), "Description", "desc value"),
        (cr_type(b"dir"), "Director", "director value"),
        (cr_type(b"prd"), "Producer", "producer value"),
        (cr_type(b"prf"), "Performers", "perf value"),
        (cr_type(b"wrt"), "Composer", "composer value"),
        (cr_type(b"lyr"), "Lyrics", "lyrics value"),
        (cr_type(b"grp"), "Grouping", "grouping value"),
        (cr_type(b"fmt"), "Format", "format value"),
        (cr_type(b"inf"), "Information", "info value"),
    ];

    let mut atoms = Vec::new();
    for (ty, _suffix, text) in &items {
        atoms.push(atom(ty, &qt_userdata_string(text)));
    }
    let udta = atom(b"udta", &concat(&atoms));
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("udta suffixes");

    for (_ty, suffix, text) in &items {
        let qk = format!("QuickTime:{}", suffix);
        let uk = format!("UserData:{}", suffix);
        assert_eq!(
            md.get(&qk),
            Some(&TagValue::String(text.to_string())),
            "missing {}",
            qk
        );
        assert_eq!(
            md.get(&uk),
            Some(&TagValue::String(text.to_string())),
            "missing {}",
            uk
        );
    }
}

// ---------------------------------------------------------------------------
// udta: non-© camera/firmware/maker atoms
// ---------------------------------------------------------------------------

#[test]
fn test_udta_camera_firmware_atoms() {
    // FIRM / INFO -> Information
    let firm = atom(b"FIRM", b"1.0.3\0");
    let info = atom(b"INFO", b"InfoString\0");
    // CNCV / CNFV / CNMN -> Canon-specific
    let cncv = atom(b"CNCV", b"CanonHD\0");
    let cnfv = atom(b"CNFV", b"1.2.0\0");
    let cnmn = atom(b"CNMN", b"Canon EOS\0");
    // PENT / PXTH -> Pentax raw
    let pent = atom(b"PENT", b"PentaxData\0");
    let pxth = atom(b"PXTH", b"PentaxThumb\0");
    // fmt  (with trailing space) -> Format
    let fmt = atom(b"fmt ", b"FormatDesc\0");

    let udta = atom(
        b"udta",
        &concat(&[firm, info, cncv, cnfv, cnmn, pent, pxth, fmt]),
    );
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("camera atoms");

    assert_eq!(
        md.get("QuickTime:Information"),
        Some(&TagValue::String("InfoString".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:CompressorVersion"),
        Some(&TagValue::String("CanonHD".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:FirmwareVersion"),
        Some(&TagValue::String("1.2.0".to_string()))
    );
    // CNMN maps to Model
    assert_eq!(
        md.get("QuickTime:Model"),
        Some(&TagValue::String("Canon EOS".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:PENT"),
        Some(&TagValue::String("PentaxData".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:PXTH"),
        Some(&TagValue::String("PentaxThumb".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Format"),
        Some(&TagValue::String("FormatDesc".to_string()))
    );
}

#[test]
fn test_udta_tmpo_atom() {
    // tmpo (tempo) directly in udta: u16 at offset 2.
    let mut payload = vec![0u8; 4];
    payload[2..4].copy_from_slice(&120u16.to_be_bytes());
    let tmpo = atom(b"tmpo", &payload);
    let udta = atom(b"udta", &tmpo);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("tmpo");
    assert_eq!(
        md.get("QuickTime:BeatsPerMinute"),
        Some(&TagValue::Integer(120))
    );
}

#[test]
fn test_udta_pentax_tags_maker_notes() {
    // TAGS atom -> extract_pentax_maker_notes.
    // Layout: maker string + NUL, then >= 24 more bytes (need total len >= 32 and
    // entry_start+24 <= len; and tag_data >= 48 to read exposure/F/ISO/focal).
    let mut data = Vec::new();
    data.extend_from_slice(b"PENTAX DIGITAL CAMERA");
    data.push(0); // null terminator
    // tag_data region (>= 48 bytes). Construct values the parser reads:
    let mut tag = vec![0u8; 64];
    // exposure: num@6, den@8  -> 1/(den/num) => num=1, den=200 -> "1/200"
    tag[6..8].copy_from_slice(&1u16.to_be_bytes());
    tag[8..10].copy_from_slice(&200u16.to_be_bytes());
    // F-number: u16@18 / 10.0 -> 40 -> 4.0
    tag[18..20].copy_from_slice(&40u16.to_be_bytes());
    // ISO: u16@26 -> 400
    tag[26..28].copy_from_slice(&400u16.to_be_bytes());
    // focal length: u16@34 / 10.0 -> 350 -> 35.0
    tag[34..36].copy_from_slice(&350u16.to_be_bytes());
    data.extend_from_slice(&tag);

    let tags_atom = atom(b"TAGS", &data);
    let udta = atom(b"udta", &tags_atom);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("pentax tags");

    assert_eq!(
        md.get("MakerNotes:Make"),
        Some(&TagValue::String("PENTAX DIGITAL CAMERA".to_string()))
    );
    assert_eq!(
        md.get("MakerNotes:ExposureTime"),
        Some(&TagValue::String("1/200".to_string()))
    );
    assert!(md.contains_key("MakerNotes:FNumber"));
    assert_eq!(md.get("MakerNotes:ISO"), Some(&TagValue::Integer(400)));
    assert!(md.contains_key("MakerNotes:FocalLength"));
    // > 64 bytes total triggers the WhiteBalance/ExposureCompensation defaults.
    assert_eq!(
        md.get("MakerNotes:WhiteBalance"),
        Some(&TagValue::String("Auto".to_string()))
    );
    assert_eq!(
        md.get("MakerNotes:ExposureCompensation"),
        Some(&TagValue::Integer(0))
    );
}

#[test]
fn test_udta_pentax_tags_too_short_no_panic() {
    // TAGS atom that has a NUL but is shorter than 32 bytes -> early return.
    let mut data = Vec::new();
    data.extend_from_slice(b"PENTAX\0");
    data.extend_from_slice(&[0u8; 4]);
    let tags_atom = atom(b"TAGS", &data);
    let udta = atom(b"udta", &tags_atom);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    // mvhd still gives TimeScale, so this is Ok and must not panic.
    let md = parse_quicktime_metadata(&reader).expect("short tags ok");
    assert!(md.contains_key("QuickTime:TimeScale"));
}

#[test]
fn test_udta_xmp_atom_with_offset_prefix() {
    // XMP_ atom where the xpacket marker is not at the very start.
    let mut payload = Vec::new();
    payload.extend_from_slice(b"\x00\x00\x00\x00"); // junk header before marker
    payload.extend_from_slice(
        br#"<?xpacket begin="" id="W5M0MpCehiHzreSzNTczkc9d"?><x:xmpmeta xmlns:x="adobe:ns:meta/"></x:xmpmeta><?xpacket end="w"?>"#,
    );
    let xmp_atom = atom(b"XMP_", &payload);
    let udta = atom(b"udta", &xmp_atom);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let res = parse_quicktime_metadata(&reader);
    assert!(res.is_ok());
}

#[test]
fn test_udta_xmp_atom_no_marker() {
    // XMP_ atom with no xpacket marker -> the function returns early (Ok).
    let xmp_atom = atom(b"XMP_", b"not xmp at all");
    let udta = atom(b"udta", &xmp_atom);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("no marker ok");
    assert!(md.contains_key("QuickTime:TimeScale"));
}

// ---------------------------------------------------------------------------
// 3GPP gnre genre decoding + the Genre {n} fallback
// ---------------------------------------------------------------------------

#[test]
fn test_gnre_known_genres() {
    // Exercise several distinct decode_id3_genre arms via the gnre atom.
    let cases = [
        (0u16, "Blues"),
        (8, "Jazz"),
        (13, "Pop"),
        (17, "Rock"),
        (32, "Classical"),
        (79, "Hard Rock"),
    ];
    for (id, name) in cases {
        let mut payload = vec![0u8; 6];
        payload[4..6].copy_from_slice(&id.to_be_bytes());
        let gnre = atom(b"gnre", &payload);
        let udta = atom(b"udta", &gnre);
        let moov = moov_with(&[udta]);
        let reader = TestReader::new(moov);
        let md = parse_quicktime_metadata(&reader).expect("gnre");
        assert_eq!(
            md.get("QuickTime:Genre"),
            Some(&TagValue::String(name.to_string())),
            "genre id {}",
            id
        );
    }
}

#[test]
fn test_gnre_unknown_genre_fallback() {
    // Genre id beyond the known table -> "Genre {n}".
    let mut payload = vec![0u8; 6];
    payload[4..6].copy_from_slice(&250u16.to_be_bytes());
    let gnre = atom(b"gnre", &payload);
    let udta = atom(b"udta", &gnre);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("gnre fallback");
    assert_eq!(
        md.get("QuickTime:Genre"),
        Some(&TagValue::String("Genre 250".to_string()))
    );
}

// ---------------------------------------------------------------------------
// iTunes ilst: less-common item types and value encodings
// ---------------------------------------------------------------------------

#[test]
fn test_itunes_ilst_extended_items() {
    let day = ilst_item(&cr_type(b"day"), &itunes_data_utf8("1999-12-31"));
    let gen_item = ilst_item(&cr_type(b"gen"), &itunes_data_utf8("Soundtrack"));
    let too = ilst_item(&cr_type(b"too"), &itunes_data_utf8("Lavf"));
    let wrt = ilst_item(&cr_type(b"wrt"), &itunes_data_utf8("Composer X"));
    let grp = ilst_item(&cr_type(b"grp"), &itunes_data_utf8("Group Y"));
    let lyr = ilst_item(&cr_type(b"lyr"), &itunes_data_utf8("la la la"));
    let cpy = ilst_item(&cr_type(b"cpy"), &itunes_data_utf8("(c) 1999"));
    let desc = ilst_item(b"desc", &itunes_data_utf8("Short desc"));
    let ldes = ilst_item(b"ldes", &itunes_data_utf8("Long description"));
    let tmpo = ilst_item(b"tmpo", &itunes_data_int16(140));
    // disk: binary 0000 0001 0002 -> "1 of 2"
    let disk = ilst_item(
        b"disk",
        &itunes_data_binary(0, &[0x00, 0x00, 0x00, 0x01, 0x00, 0x02]),
    );
    // cover art (PNG type 14) -> Binary
    let covr = ilst_item(b"covr", &itunes_data_binary(14, &[0x89, b'P', b'N', b'G']));

    let ilst = atom(
        b"ilst",
        &concat(&[
            day, gen_item, too, wrt, grp, lyr, cpy, desc, ldes, tmpo, disk, covr,
        ]),
    );
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"mdir", b"appl", b"mdir", false),
    );
    let meta = atom(b"meta", &meta_payload(&[hdlr, ilst]));
    let udta = atom(b"udta", &meta);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("extended ilst");

    assert!(md.contains_key("ItemList:ContentCreateDate"));
    assert_eq!(
        md.get("ItemList:Year"),
        Some(&TagValue::String("1999".to_string()))
    );
    assert_eq!(
        md.get("ItemList:Genre"),
        Some(&TagValue::String("Soundtrack".to_string()))
    );
    assert_eq!(
        md.get("ItemList:Encoder"),
        Some(&TagValue::String("Lavf".to_string()))
    );
    assert_eq!(
        md.get("ItemList:Composer"),
        Some(&TagValue::String("Composer X".to_string()))
    );
    assert_eq!(
        md.get("ItemList:Grouping"),
        Some(&TagValue::String("Group Y".to_string()))
    );
    assert_eq!(
        md.get("ItemList:Lyrics"),
        Some(&TagValue::String("la la la".to_string()))
    );
    assert_eq!(
        md.get("ItemList:Copyright"),
        Some(&TagValue::String("(c) 1999".to_string()))
    );
    assert_eq!(
        md.get("ItemList:Description"),
        Some(&TagValue::String("Short desc".to_string()))
    );
    assert_eq!(
        md.get("ItemList:LongDescription"),
        Some(&TagValue::String("Long description".to_string()))
    );
    assert_eq!(
        md.get("ItemList:BeatsPerMinute"),
        Some(&TagValue::Integer(140))
    );
    // disk formatted as "1 of 2"
    assert_eq!(
        md.get("QuickTime:DiskNumber"),
        Some(&TagValue::String("1 of 2".to_string()))
    );
    // cover art stored as binary
    assert!(matches!(
        md.get("ItemList:CoverArt"),
        Some(TagValue::Binary(_))
    ));
}

#[test]
fn test_itunes_data_int8_and_unknown_type() {
    // 1-byte signed int (type 21, len 1) via tmpo.
    let tmpo = ilst_item(b"tmpo", &itunes_data_int8(99));
    // Unknown type indicator (e.g. 99) -> falls through to "try as string".
    let weird = ilst_item(&cr_type(b"nam"), &itunes_data_binary(99, b"strvalue"));
    let ilst = atom(b"ilst", &concat(&[tmpo, weird]));
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"mdir", b"appl", b"", false),
    );
    let meta = atom(b"meta", &meta_payload(&[hdlr, ilst]));
    let udta = atom(b"udta", &meta);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("int8/unknown");
    assert_eq!(
        md.get("ItemList:BeatsPerMinute"),
        Some(&TagValue::Integer(99))
    );
    assert_eq!(
        md.get("ItemList:Title"),
        Some(&TagValue::String("strvalue".to_string()))
    );
}

#[test]
fn test_itunes_meta_without_header_bytes() {
    // meta whose first 4 bytes are NOT all zero: extract_itunes_metadata then
    // parses meta.data directly (the `else` branch).
    let nam = ilst_item(&cr_type(b"nam"), &itunes_data_utf8("NoHeader"));
    let ilst = atom(b"ilst", &nam);
    // No hdlr, and the leading bytes are the ilst atom header (non-zero size).
    let meta = atom(b"meta", &meta_payload_no_header(&[ilst]));
    let udta = atom(b"udta", &meta);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("meta no header");
    assert_eq!(
        md.get("ItemList:Title"),
        Some(&TagValue::String("NoHeader".to_string()))
    );
}

// ---------------------------------------------------------------------------
// MP4 keys/ilst: every mapped Apple key + the MP4: fallback for unmapped keys
// ---------------------------------------------------------------------------

#[test]
fn test_mp4_keys_all_apple_mappings() {
    let key_names = [
        "com.apple.quicktime.location.ISO6709",
        "com.apple.quicktime.location.accuracy.horizontal",
        "com.apple.quicktime.location.role",
        "com.apple.quicktime.creationLocation.name",
        "com.apple.quicktime.make",
        "com.apple.quicktime.model",
        "com.apple.quicktime.software",
        "com.apple.quicktime.creationdate",
        "com.apple.quicktime.custom.unmapped", // -> QuickTime:<key>
    ];

    let make_key = |key: &str| -> Vec<u8> {
        let mut e = Vec::new();
        let key_size = (8 + key.len()) as u32;
        e.extend_from_slice(&key_size.to_be_bytes());
        e.extend_from_slice(b"mdta");
        e.extend_from_slice(key.as_bytes());
        e
    };
    let mut keys_payload = Vec::new();
    keys_payload.extend_from_slice(&[0u8; 4]);
    keys_payload.extend_from_slice(&(key_names.len() as u32).to_be_bytes());
    for k in &key_names {
        keys_payload.extend_from_slice(&make_key(k));
    }
    let keys = atom(b"keys", &keys_payload);

    let item_for_index =
        |idx: u32, data_atom: Vec<u8>| -> Vec<u8> { atom(&idx.to_be_bytes(), &data_atom) };
    let values = [
        "+12.0+034.0+010.0/",       // ISO6709 (triggers GPS parse)
        "65.0",                     // accuracy
        "shooting",                 // role
        "Somewhere",                // creationLocation name
        "Apple",                    // make
        "iPhone",                   // model
        "17.0",                     // software
        "2021-01-01T00:00:00+0000", // creationdate
        "customvalue",              // unmapped
    ];
    let mut items = Vec::new();
    for (i, v) in values.iter().enumerate() {
        items.push(item_for_index((i + 1) as u32, itunes_data_utf8(v)));
    }
    let ilst = atom(b"ilst", &concat(&items));

    let meta = atom(b"meta", &meta_payload_no_header(&[keys, ilst]));
    let moov = moov_with(&[meta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("all apple keys");

    assert_eq!(
        md.get("QuickTime:Make"),
        Some(&TagValue::String("Apple".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Model"),
        Some(&TagValue::String("iPhone".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:Software"),
        Some(&TagValue::String("17.0".to_string()))
    );
    assert!(md.contains_key("QuickTime:ContentCreateDate"));
    assert!(md.contains_key("QuickTime:LocationAccuracyHorizontal"));
    assert!(md.contains_key("QuickTime:LocationRole"));
    assert!(md.contains_key("QuickTime:CreationLocationName"));
    // GPS parse from ISO6709
    assert!(md.contains_key("QuickTime:GPSLatitude"));
    assert!(md.contains_key("QuickTime:GPSAltitude"));
    // Unmapped key keeps its full name under QuickTime: prefix.
    assert_eq!(
        md.get("QuickTime:com.apple.quicktime.custom.unmapped"),
        Some(&TagValue::String("customvalue".to_string()))
    );
}

#[test]
fn test_mp4_keys_ilst_unknown_index_fallback() {
    // ilst item index that has NO corresponding key -> "MP4:<atomtype>" fallback.
    // keys has 1 entry (index 1), but ilst references index 9.
    let mut keys_payload = Vec::new();
    keys_payload.extend_from_slice(&[0u8; 4]);
    keys_payload.extend_from_slice(&1u32.to_be_bytes());
    let key = "com.apple.quicktime.make";
    let key_size = (8 + key.len()) as u32;
    keys_payload.extend_from_slice(&key_size.to_be_bytes());
    keys_payload.extend_from_slice(b"mdta");
    keys_payload.extend_from_slice(key.as_bytes());
    let keys = atom(b"keys", &keys_payload);

    let item = atom(&9u32.to_be_bytes(), &itunes_data_utf8("orphan"));
    let ilst = atom(b"ilst", &item);

    let meta = atom(b"meta", &meta_payload_no_header(&[keys, ilst]));
    let moov = moov_with(&[meta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("orphan ilst index");
    // The atom type for index 9 is the bytes 00 00 00 09 -> not valid UTF-8 4cc,
    // but the code uses item.atom_type.as_str(); just assert the value landed
    // under some MP4: key.
    let has_mp4_orphan = md
        .iter()
        .any(|(k, v)| k.starts_with("MP4:") && matches!(v, TagValue::String(s) if s == "orphan"));
    assert!(
        has_mp4_orphan,
        "expected MP4: fallback key for orphan index"
    );
}

// ---------------------------------------------------------------------------
// Handler metadata: vendor id, handler-type arms, Pascal & null descriptions
// ---------------------------------------------------------------------------

#[test]
fn test_handler_metadata_pascal_description_and_vendor() {
    // hdlr in udta with mhlr component, "vide" handler, "appl" vendor, Pascal name.
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"mhlr", b"vide", b"appl", b"VideoHandler", true),
    );
    let udta = atom(b"udta", &hdlr);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("pascal hdlr");
    assert_eq!(
        md.get("QuickTime:HandlerClass"),
        Some(&TagValue::String("Media Handler".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:HandlerType"),
        Some(&TagValue::String("Video Track".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:HandlerVendorID"),
        Some(&TagValue::String("Apple".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:HandlerDescription"),
        Some(&TagValue::String("VideoHandler".to_string()))
    );
}

#[test]
fn test_handler_metadata_null_description_and_types() {
    // dhlr component, "soun" handler, custom vendor.
    // The handler-name heuristic treats a leading byte in 1..128 as a Pascal-string
    // length prefix. To exercise the *null-terminated* branch we make the name start
    // with a high byte (>= 128); here "\u{00e9}ext" encodes to 0xC3 0xA9 'e' 'x' 't'.
    let name = "\u{00e9}ext";
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"dhlr", b"soun", b"xyz4", name.as_bytes(), false),
    );
    let udta = atom(b"udta", &hdlr);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("null hdlr");
    assert_eq!(
        md.get("QuickTime:HandlerClass"),
        Some(&TagValue::String("Data Handler".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:HandlerType"),
        Some(&TagValue::String("Audio Track".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:HandlerDescription"),
        Some(&TagValue::String(name.to_string()))
    );
    // Custom 4-char vendor preserved verbatim.
    assert_eq!(
        md.get("QuickTime:HandlerVendorID"),
        Some(&TagValue::String("xyz4".to_string()))
    );
}

#[test]
fn test_handler_metadata_misc_handler_types() {
    // Exercise additional handler-type arms via udta->meta->hdlr.
    let cases: [(&[u8; 4], &str); 6] = [
        (b"hint", "Hint Track"),
        (b"text", "Text Track"),
        (b"tmcd", "Time Code"),
        (b"pict", "Picture"),
        (b"auxv", "Auxiliary Video"),
        (b"meta", "Timed Metadata"),
    ];
    for (handler, expected) in cases {
        let hdlr = atom(
            b"hdlr",
            &hdlr_payload(b"mhlr", handler, b"\0\0\0\0", b"X", false),
        );
        // Put hdlr inside udta->meta to hit the udta->meta->hdlr path.
        let meta = atom(b"meta", &meta_payload(&[hdlr]));
        let udta = atom(b"udta", &meta);
        let moov = moov_with(&[udta]);
        let reader = TestReader::new(moov);
        let md = parse_quicktime_metadata(&reader).expect("handler type");
        assert_eq!(
            md.get("QuickTime:HandlerType"),
            Some(&TagValue::String(expected.to_string())),
            "handler {:?}",
            std::str::from_utf8(handler).unwrap()
        );
    }
}

// ---------------------------------------------------------------------------
// Audio track minf hdlr path (extract_track_handler_metadata)
// ---------------------------------------------------------------------------

#[test]
fn test_audio_track_minf_hdlr() {
    let tkhd = atom(b"tkhd", &tkhd_v0_full(1, 0, 0));
    let mdhd = atom(b"mdhd", &mdhd_v0(44100, 88200));
    let smhd = atom(b"smhd", &{
        let mut v = vec![0u8; 8];
        v[4..6].copy_from_slice(&0i16.to_be_bytes());
        v
    });
    // hdlr directly in minf (audio-track specific path).
    let minf_hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"dhlr", b"soun", b"\0\0\0\0", b"", false),
    );
    // stsd with mp4a so the track is well-formed.
    let mut stsd_entry = Vec::new();
    stsd_entry.extend_from_slice(&[0u8; 4]); // placeholder size
    stsd_entry.extend_from_slice(b"mp4a");
    stsd_entry.extend_from_slice(&[0u8; 6]);
    stsd_entry.extend_from_slice(&1u16.to_be_bytes());
    stsd_entry.extend_from_slice(&[0u8; 8]); // version..vendor
    stsd_entry.extend_from_slice(&2u16.to_be_bytes()); // channels @24
    stsd_entry.extend_from_slice(&16u16.to_be_bytes()); // sample size @26
    stsd_entry.extend_from_slice(&0u16.to_be_bytes());
    stsd_entry.extend_from_slice(&0u16.to_be_bytes());
    stsd_entry.extend_from_slice(&(44100u32 << 16).to_be_bytes()); // rate @32
    let sz = stsd_entry.len() as u32;
    stsd_entry[0..4].copy_from_slice(&sz.to_be_bytes());
    let mut stsd_payload = Vec::new();
    stsd_payload.extend_from_slice(&[0u8; 4]);
    stsd_payload.extend_from_slice(&1u32.to_be_bytes());
    stsd_payload.extend_from_slice(&stsd_entry);
    let stsd = atom(b"stsd", &stsd_payload);
    let stbl = atom(b"stbl", &stsd);
    let minf = atom(b"minf", &concat(&[smhd, minf_hdlr, stbl]));
    let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
    let trak = atom(b"trak", &concat(&[tkhd, mdia]));
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("audio minf hdlr");
    // extract_track_handler_metadata maps dhlr component to "Data Handler".
    assert_eq!(
        md.get("QuickTime:HandlerClass"),
        Some(&TagValue::String("Data Handler".to_string()))
    );
    assert_eq!(
        md.get("QuickTime:AudioChannels"),
        Some(&TagValue::Integer(2))
    );
}

// ---------------------------------------------------------------------------
// mvhd / tkhd: version-1 full fields + legacy mac-time formatting
// ---------------------------------------------------------------------------

#[test]
fn test_mvhd_v1_full_fields() {
    let mvhd_atom = atom(b"mvhd", &mvhd_v1_full(48000, 96000));
    let moov = atom(b"moov", &mvhd_atom);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("mvhd v1 full");
    assert_eq!(
        md.get("QuickTime:MovieHeaderVersion"),
        Some(&TagValue::Integer(1))
    );
    assert!(md.contains_key("QuickTime:MatrixStructure"));
    assert_eq!(md.get("QuickTime:NextTrackID"), Some(&TagValue::Integer(5)));
    // time fields present
    assert!(md.contains_key("QuickTime:PreviewTime"));
}

#[test]
fn test_mvhd_legacy_mac_time_pre1970() {
    // creation time small enough that it predates 1970 -> mac_time_to_iso8601
    // returns None and format_mac_time_legacy is used instead.
    let mvhd_atom = atom(b"mvhd", &mvhd_v0_time(600, 6000, 1000));
    let moov = atom(b"moov", &mvhd_atom);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("legacy time");
    // CreateDate is present and uses the ExifTool "YYYY:MM:DD ..." legacy format
    // (it contains a ':' separator in the date part rather than '-').
    match md.get("QuickTime:CreateDate") {
        Some(TagValue::String(s)) => {
            assert!(s.contains(':'), "legacy date should contain ':' got {}", s);
        }
        other => panic!("expected CreateDate string, got {:?}", other),
    }
}

#[test]
fn test_tkhd_full_layer_volume_duration() {
    let tkhd = atom(b"tkhd", &tkhd_v0_full(4, 1280, 720));
    let mdhd = atom(b"mdhd", &mdhd_v0(30000, 60000));
    // minimal stbl so extract_track_metadata reaches the end (stsd optional).
    let stbl = atom(b"stbl", &[]);
    let minf = atom(b"minf", &stbl);
    let mdia = atom(b"mdia", &concat(&[mdhd, minf]));
    let trak = atom(b"trak", &concat(&[tkhd, mdia]));
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("tkhd full");
    assert_eq!(md.get("QuickTime:TrackID"), Some(&TagValue::Integer(4)));
    assert_eq!(md.get("QuickTime:TrackLayer"), Some(&TagValue::Integer(1)));
    assert!(md.contains_key("QuickTime:TrackVolume"));
    // Track duration uses movie timescale (600) -> 2400/600 = 4.00 s
    assert_eq!(
        md.get("QuickTime:TrackDuration"),
        Some(&TagValue::String("4.00 s".to_string()))
    );
}

// ---------------------------------------------------------------------------
// HEIF: full EXIF pipeline through iinf + iloc + mdat -> TIFF/IFD parse
// ---------------------------------------------------------------------------

/// Build a minimal little-endian TIFF/EXIF block with an IFD0 containing a few
/// tags of different field types so raw_bytes_to_tag_value runs multiple arms.
fn build_tiff_exif() -> Vec<u8> {
    // TIFF header: "II", 0x2A, IFD0 offset = 8.
    let mut t = Vec::new();
    t.extend_from_slice(b"II");
    t.extend_from_slice(&0x002Au16.to_le_bytes());
    t.extend_from_slice(&8u32.to_le_bytes());

    // IFD0 with 3 entries.
    // entry layout: tag(2) type(2) count(4) value/offset(4)
    let entries: [(u16, u16, u32, [u8; 4]); 3] = [
        // Make (0x010F) ASCII count=1 inline -> "A\0\0\0" won't trim, use small string
        // ASCII type=2; store a 4-char inline value "Hi\0\0".
        (0x010F, 2, 3, *b"Hi\0\0"),
        // ImageWidth (0x0100) SHORT type=3 count=1 value=640 (little-endian inline)
        (0x0100, 3, 1, [0x80, 0x02, 0x00, 0x00]),
        // ImageLength (0x0101) LONG type=4 count=1 value=480
        (0x0101, 4, 1, [0xE0, 0x01, 0x00, 0x00]),
    ];

    t.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, ty, count, val) in entries {
        t.extend_from_slice(&tag.to_le_bytes());
        t.extend_from_slice(&ty.to_le_bytes());
        t.extend_from_slice(&count.to_le_bytes());
        t.extend_from_slice(&val);
    }
    // next IFD offset = 0
    t.extend_from_slice(&0u32.to_le_bytes());
    t
}

#[test]
fn test_heif_full_exif_pipeline() {
    // Build meta with: hdlr(pict), iinf(Exif item id=1), iloc pointing into mdat.
    // The EXIF payload inside mdat is: [4-byte exif header len][\"Exif\\0\\0\"][TIFF...].
    let tiff = build_tiff_exif();
    // HEIF Exif item payload: a 4-byte "exif tiff header offset" then "Exif\0\0".
    // extract_exif_from_mdat checks exif_data[4..8] == b"Exif" then uses [10..].
    // So payload = [0,0,0,0]("offset"), "Exif", [0,0], TIFF...
    let mut exif_payload = Vec::new();
    exif_payload.extend_from_slice(&[0u8, 0u8, 0u8, 0u8]); // bytes 0..4
    exif_payload.extend_from_slice(b"Exif"); // bytes 4..8
    exif_payload.extend_from_slice(&[0u8, 0u8]); // bytes 8..10
    exif_payload.extend_from_slice(&tiff); // bytes 10..

    // iinf v0: version/flags(4) + entry_count(2) + infe.
    let infe_payload = {
        let mut v = Vec::new();
        v.extend_from_slice(&[0u8; 4]); // version/flags
        v.extend_from_slice(&1u16.to_be_bytes()); // item_id = 1
        v.extend_from_slice(&0u16.to_be_bytes()); // protection idx
        v.extend_from_slice(b"Exif"); // item type
        v
    };
    let infe = atom(b"infe", &infe_payload);
    let mut iinf_payload = Vec::new();
    iinf_payload.extend_from_slice(&[0u8; 4]);
    iinf_payload.extend_from_slice(&1u16.to_be_bytes());
    iinf_payload.extend_from_slice(&infe);
    let iinf = atom(b"iinf", &iinf_payload);

    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"pict", b"\0\0\0\0", b"HEIF", false),
    );

    // We need iloc to point at the EXIF payload's position within mdat.
    // The extractor computes mdat content start as sum over atoms before mdat of
    // (8 + data.len()), plus a header_size guess of 8 or 16. We will compute the
    // file offset to the exif_payload start inside mdat and set iloc offset to it.
    // Layout: ftyp, meta, mdat. exif_payload is the entire mdat data.
    // Build the pieces so we can measure offsets.
    let ftyp = atom(b"ftyp", &ftyp_payload(b"heic", 0, &[b"heic", b"mif1"]));
    // mdat content == exif_payload; place it as the only data.
    let mdat = atom(b"mdat", &exif_payload);

    // Compute file_offset = sum(8 + data.len()) for atoms before mdat = ftyp + meta.
    // We don't yet have meta built (needs iloc which needs the offset). The
    // extractor's mdat_start = file_offset + header_size(8 or 16). The absolute
    // offset of exif_payload bytes is file_offset + 8 (mdat header). With
    // header_size=8 the code computes mdat_offset = offset - (file_offset+8) = 0,
    // exactly the start of mdat data. So we set iloc extent offset = file_offset+8.
    //
    // file_offset depends on meta size, which depends on iloc... circular. To
    // break it: the offset only needs to satisfy offset >= mdat_start and land at
    // the EXIF block. We instead use the fallback path in extract_exif_from_mdat
    // which tries the *direct* offset into mdat.data: if off + len <= mdat.data
    // and data[4..8]=="Exif". Setting iloc offset = 0 makes the direct-offset
    // fallback read mdat.data[0..len] which is exactly our exif_payload.
    let exif_len = exif_payload.len() as u32;
    // iloc v0: version/flags(4), then [offset_size|length_size](1) [base_offset_size|index_size](1),
    // item_count(2), then per item: item_id(2), data_ref_index(2), base_offset(var),
    // extent_count(2), extent_offset(var), extent_length(var).
    let mut iloc_payload = Vec::new();
    iloc_payload.extend_from_slice(&[0u8; 4]); // version 0 / flags
    iloc_payload.push(0x44); // offset_size=4, length_size=4
    iloc_payload.push(0x00); // base_offset_size=0, index_size=0
    iloc_payload.extend_from_slice(&1u16.to_be_bytes()); // item_count = 1
    iloc_payload.extend_from_slice(&1u16.to_be_bytes()); // item_id = 1
    iloc_payload.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
    // base_offset_size = 0 -> nothing
    iloc_payload.extend_from_slice(&1u16.to_be_bytes()); // extent_count = 1
    iloc_payload.extend_from_slice(&0u32.to_be_bytes()); // extent_offset = 0
    iloc_payload.extend_from_slice(&exif_len.to_be_bytes()); // extent_length
    let iloc = atom(b"iloc", &iloc_payload);

    // ispe + pitm to also cover those paths in the same meta.
    let ispe = atom(b"ispe", &{
        let mut v = vec![0u8; 12];
        v[4..8].copy_from_slice(&640u32.to_be_bytes());
        v[8..12].copy_from_slice(&480u32.to_be_bytes());
        v
    });
    let ipco = atom(b"ipco", &ispe);
    let iprp = atom(b"iprp", &ipco);
    let pitm = atom(b"pitm", &{
        let mut v = vec![0u8; 6];
        v[4..6].copy_from_slice(&1u16.to_be_bytes());
        v
    });

    let meta = atom(b"meta", &meta_payload(&[hdlr, iinf, iloc, iprp, pitm]));

    let reader = TestReader::new(concat(&[ftyp, meta, mdat]));
    let md = parse_quicktime_metadata(&reader).expect("heif exif pipeline");

    // From iinf we always get ItemCount.
    assert_eq!(md.get("HEIF:ItemCount"), Some(&TagValue::Integer(1)));
    // From ispe.
    assert_eq!(md.get("HEIF:ImageWidth"), Some(&TagValue::Integer(640)));
    // From pitm.
    assert_eq!(
        md.get("QuickTime:PrimaryItemReference"),
        Some(&TagValue::Integer(1))
    );
    // The EXIF IFD0 tags should have been parsed and inserted. We don't assert on
    // exact tag names (they come from the tag DB) but at least one IFD0-derived
    // integer-or-string tag beyond the HEIF/QuickTime ones should be present.
    let extra_tags = md
        .iter()
        .filter(|(k, _)| {
            !k.starts_with("HEIF:") && !k.starts_with("QuickTime:") && !k.starts_with("UserData:")
        })
        .count();
    assert!(
        extra_tags > 0,
        "expected EXIF-derived tags from the HEIF mdat; keys={:?}",
        md.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>()
    );
}

#[test]
fn test_heif_iloc_truncated_no_panic() {
    // iloc atom too short (< 8 bytes) must not panic and yields no locations.
    let iloc = atom(b"iloc", &[0u8, 0u8, 0u8]);
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"pict", b"\0\0\0\0", b"H", false),
    );
    let ispe = atom(b"ispe", &{
        let mut v = vec![0u8; 12];
        v[4..8].copy_from_slice(&100u32.to_be_bytes());
        v[8..12].copy_from_slice(&100u32.to_be_bytes());
        v
    });
    let ipco = atom(b"ipco", &ispe);
    let iprp = atom(b"iprp", &ipco);
    let meta = atom(b"meta", &meta_payload(&[hdlr, iloc, iprp]));
    let ftyp = atom(b"ftyp", &ftyp_payload(b"mif1", 0, &[b"mif1"]));
    let reader = TestReader::new(concat(&[ftyp, meta]));
    let md = parse_quicktime_metadata(&reader).expect("truncated iloc");
    assert_eq!(md.get("HEIF:ImageWidth"), Some(&TagValue::Integer(100)));
}

#[test]
fn test_heif_iinf_no_exif_item() {
    // iinf with a non-Exif item -> find_exif_item_id returns None but still sets
    // HEIF:ItemCount.
    let infe_payload = {
        let mut v = Vec::new();
        v.extend_from_slice(&[0u8; 4]);
        v.extend_from_slice(&7u16.to_be_bytes()); // item id
        v.extend_from_slice(&0u16.to_be_bytes());
        v.extend_from_slice(b"hvc1"); // not Exif
        v
    };
    let infe = atom(b"infe", &infe_payload);
    let mut iinf_payload = Vec::new();
    iinf_payload.extend_from_slice(&[0u8; 4]);
    iinf_payload.extend_from_slice(&1u16.to_be_bytes());
    iinf_payload.extend_from_slice(&infe);
    let iinf = atom(b"iinf", &iinf_payload);
    let hdlr = atom(
        b"hdlr",
        &hdlr_payload(b"\0\0\0\0", b"pict", b"\0\0\0\0", b"H", false),
    );
    let meta = atom(b"meta", &meta_payload(&[hdlr, iinf]));
    let ftyp = atom(b"ftyp", &ftyp_payload(b"mif1", 0, &[b"mif1"]));
    let reader = TestReader::new(concat(&[ftyp, meta]));
    let md = parse_quicktime_metadata(&reader).expect("iinf no exif");
    assert_eq!(md.get("HEIF:ItemCount"), Some(&TagValue::Integer(1)));
}

// ---------------------------------------------------------------------------
// ftyp brand descriptions: exercise more brand arms
// ---------------------------------------------------------------------------

#[test]
fn test_ftyp_various_brands() {
    let cases: [(&[u8; 4], &str); 8] = [
        (b"iso2", "MP4 Base Media v2"),
        (b"mp41", "MP4 v1 [ISO 14496-1:ch13]"),
        (b"M4A ", "Apple iTunes AAC-LC (.M4A) Audio"),
        (b"M4V ", "Apple iTunes Video (.M4V) Video"),
        (b"mp4 ", "MP4 Base Media v1 [IS0 14496-12:2003]"),
        (b"avif", "AV1 Image File Format (.AVIF)"),
        (b"hevc", "High Efficiency Video Coding (.HEVC)"),
        (b"msf1", "High Efficiency Image Format sequence (.HEICS)"),
    ];
    for (brand, expected) in cases {
        let ftyp = atom(b"ftyp", &ftyp_payload(brand, 0, &[brand]));
        let moov = moov_with(&[]);
        let reader = TestReader::new(concat(&[ftyp, moov]));
        let md = parse_quicktime_metadata(&reader).expect("brand");
        assert_eq!(
            md.get("QuickTime:MajorBrand"),
            Some(&TagValue::String(expected.to_string())),
            "brand {:?}",
            std::str::from_utf8(brand).unwrap()
        );
    }
}

#[test]
fn test_ftyp_unknown_brand_passthrough() {
    // Unknown brand -> the raw 4cc is used as the description.
    let ftyp = atom(b"ftyp", &ftyp_payload(b"ZZZZ", 0, &[b"ZZZZ"]));
    let moov = moov_with(&[]);
    let reader = TestReader::new(concat(&[ftyp, moov]));
    let md = parse_quicktime_metadata(&reader).expect("unknown brand");
    assert_eq!(
        md.get("QuickTime:MajorBrand"),
        Some(&TagValue::String("ZZZZ".to_string()))
    );
}

// ---------------------------------------------------------------------------
// Sample description: codec name mapping for additional codecs
// ---------------------------------------------------------------------------

/// Build a stsd payload with a single entry of the given codec 4cc, padded to
/// `entry_len` bytes (so we can hit the video vs audio branches).
fn stsd_codec(codec: &[u8], entry_len: usize) -> Vec<u8> {
    let mut entry = vec![0u8; entry_len.max(16)];
    let size = entry.len() as u32;
    entry[0..4].copy_from_slice(&size.to_be_bytes());
    entry[4..8].copy_from_slice(codec);
    // data_ref_index @14
    entry[14..16].copy_from_slice(&1u16.to_be_bytes());
    let mut p = Vec::new();
    p.extend_from_slice(&[0u8; 4]);
    p.extend_from_slice(&1u32.to_be_bytes());
    p.extend_from_slice(&entry);
    p
}

#[test]
fn test_sample_description_codec_names() {
    // Each codec produces a distinct CompressorName mapping; use a short entry so
    // only the codec-name branch runs (no width/height parsing).
    let cases: [(&[u8; 4], &str); 6] = [
        (b"mp4v", "MPEG-4 Video"),
        (b"jpeg", "Photo - JPEG"),
        (b"vp08", "VP8"),
        (b"vp09", "VP9"),
        (b"av01", "AV1"),
        (b"alac", "Apple Lossless"),
    ];
    for (codec, expected) in cases {
        let stsd = atom(b"stsd", &stsd_codec(codec, 16));
        let stbl = atom(b"stbl", &stsd);
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
fn test_sample_description_pcm_audio_format() {
    // PCM-style audio codecs set AudioFormat (is_audio_codec branch).
    for codec in [b"sowt", b"twos", b"alaw", b"ulaw"] {
        let stsd = atom(b"stsd", &stsd_codec(codec, 36));
        let smhd = atom(b"smhd", &vec![0u8; 8]);
        let stbl = atom(b"stbl", &stsd);
        let minf = atom(b"minf", &concat(&[smhd, stbl]));
        let mdia = atom(
            b"mdia",
            &concat(&[atom(b"mdhd", &mdhd_v0(44100, 88200)), minf]),
        );
        let trak = atom(
            b"trak",
            &concat(&[atom(b"tkhd", &tkhd_v0_full(2, 0, 0)), mdia]),
        );
        let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
        let reader = TestReader::new(moov);
        let md = parse_quicktime_metadata(&reader).expect("pcm audio");
        assert_eq!(
            md.get("QuickTime:AudioFormat"),
            Some(&TagValue::String(
                std::str::from_utf8(codec).unwrap().to_string()
            ))
        );
    }
}

// ---------------------------------------------------------------------------
// Malformed / truncated boxes for early-return branches
// ---------------------------------------------------------------------------

#[test]
fn test_truncated_vmhd_smhd_no_panic() {
    // vmhd and smhd shorter than required -> early returns; mvhd still yields data.
    let vmhd = atom(b"vmhd", &[0u8; 4]); // < 12
    let smhd = atom(b"smhd", &[0u8; 4]); // < 8
    let stbl = atom(b"stbl", &[]);
    let minf = atom(b"minf", &concat(&[vmhd, smhd, stbl]));
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
    let md = parse_quicktime_metadata(&reader).expect("truncated headers");
    assert!(md.contains_key("QuickTime:TimeScale"));
}

#[test]
fn test_track_missing_mdia_skipped() {
    // A trak without mdia is skipped (extract_track_metadata returns Err), but the
    // tkhd is still processed and mvhd provides TimeScale.
    let trak = atom(b"trak", &atom(b"tkhd", &tkhd_v0_full(9, 100, 100)));
    let moov = atom(b"moov", &concat(&[atom(b"mvhd", &mvhd(600, 6000)), trak]));
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("missing mdia");
    assert_eq!(md.get("QuickTime:TrackID"), Some(&TagValue::Integer(9)));
}

#[test]
fn test_empty_3gpp_string_value() {
    // A cprt atom with < 6 bytes -> extract_3gpp_string_value returns None.
    let cprt = atom(b"cprt", &[0u8, 0u8]);
    let udta = atom(b"udta", &cprt);
    let moov = moov_with(&[udta]);
    let reader = TestReader::new(moov);
    let md = parse_quicktime_metadata(&reader).expect("short cprt");
    assert!(!md.contains_key("QuickTime:Copyright"));
}

// ---------------------------------------------------------------------------
// Real fixtures through the production read_metadata path
// ---------------------------------------------------------------------------

#[test]
fn test_real_fixtures_through_read_metadata() {
    for path in [
        "tests/fixtures/mp4/sample.mp4",
        "tests/fixtures/mp4/simple/sample.mp4",
    ] {
        let p = std::path::Path::new(path);
        if !p.exists() {
            continue;
        }
        let md = oxidex::core::operations::read_metadata(p)
            .unwrap_or_else(|e| panic!("read_metadata({}) failed: {}", path, e));
        assert!(
            md.contains_key("QuickTime:MajorBrand"),
            "{} missing MajorBrand",
            path
        );
    }
}

#[test]
fn test_real_fixtures_direct_parse() {
    for path in [
        "tests/fixtures/mp4/sample.mp4",
        "tests/fixtures/mp4/simple/sample.mp4",
    ] {
        let p = std::path::Path::new(path);
        if !p.exists() {
            continue;
        }
        let bytes = std::fs::read(p).expect("read fixture");
        let reader = TestReader::new(bytes);
        let md = parse_quicktime_metadata(&reader)
            .unwrap_or_else(|e| panic!("parse {} failed: {}", path, e));
        assert!(!md.is_empty());
    }
}
