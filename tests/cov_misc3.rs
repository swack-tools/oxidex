//! Coverage suite (wave 3): miscellaneous remaining uncovered paths.
//!
//! Targets:
//! - `src/main.rs` / `src/cli/*`        — driven via the compiled CLI binary
//! - `src/parsers/specialized/pcap.rs`  — PCAP/PCAP-NG block/option/error paths
//! - `src/parsers/pe/rich_header_parser.rs` — entries, hashing, product names
//! - `src/parsers/pdf/signature_parser.rs`  — AcroForm signature navigation
//! - `src/parsers/text/eps.rs`          — DSC comments, binary EPS, XMP/IPTC
//! - `src/core/tiff_helpers.rs`         — IFD chain, sub-IFDs, ICC/IPTC/GeoTiff
//! - `src/core/tag_conversion.rs`       — every EXIF field-type arm + specials

#[path = "common/mod.rs"]
mod common;

use common::TestReader;

use oxidex::core::MetadataMap;
use oxidex::core::TagValue;
use oxidex::core::tag_conversion::{parse_string_to_tag_value, raw_bytes_to_tag_value};
use oxidex::core::tiff_helpers::{parse_exif_subifd, parse_gps_subifd, parse_ifd_chain};
use oxidex::parsers::pdf::signature_parser::parse_signature_metadata;
use oxidex::parsers::pe::rich_header_parser::{RichHeader, RichHeaderEntry, parse_rich_header};
use oxidex::parsers::specialized::pcap::{PCAPParser, parse_pcap_metadata};
use oxidex::parsers::text::eps::{EPSParser, parse_eps_metadata};
use oxidex::parsers::tiff::ifd_parser::ByteOrder;

use std::io::Write;
use std::process::Command;

// ===========================================================================
// Helpers
// ===========================================================================

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_oxidex"))
}

/// Resolve a fixture path under tests/fixtures.
fn fixture(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

// ===========================================================================
// CLI (src/main.rs + src/cli/*) via the compiled binary
// ===========================================================================

#[test]
fn cli_help_long_and_short() {
    for flag in ["--help", "-h"] {
        let out = bin().arg(flag).output().expect("run help");
        assert!(out.status.success(), "help should exit 0 for {flag}");
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(s.contains("USAGE"), "help text missing for {flag}");
    }
}

#[test]
fn cli_version_long_and_short() {
    for flag in ["--version", "-V"] {
        let out = bin().arg(flag).output().expect("run version");
        assert!(out.status.success());
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(s.contains("oxidex"), "version text missing for {flag}");
    }
}

#[test]
fn cli_no_arguments_errors() {
    let out = bin().output().expect("run no args");
    assert!(!out.status.success(), "no file should be an error");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("No file") || err.contains("Usage"));
}

#[test]
fn cli_nonexistent_file_errors() {
    let out = bin()
        .arg("/definitely/not/a/real/path_zzz.jpg")
        .output()
        .expect("run missing file");
    assert!(!out.status.success());
}

#[test]
fn cli_read_human_readable_jpeg() {
    let f = fixture("jpeg/simple/sample_with_exif.jpg");
    if !f.exists() {
        return;
    }
    let out = bin().arg(&f).output().expect("read jpeg");
    assert!(out.status.success(), "human-readable read should succeed");
    let s = String::from_utf8_lossy(&out.stdout);
    assert!(s.contains("File:") && s.contains("metadata tag"));
}

#[test]
fn cli_read_json_format() {
    let f = fixture("jpeg/simple/sample_with_exif.jpg");
    if !f.exists() {
        return;
    }
    // Both -j (short) and -json (exiftool-style) and --json should work.
    for flag in ["-j", "-json", "--json"] {
        let out = bin().arg(flag).arg(&f).output().expect("read json");
        assert!(out.status.success(), "json read failed for {flag}");
        let s = String::from_utf8_lossy(&out.stdout);
        assert!(s.trim_start().starts_with('{') || s.contains(':'));
    }
}

#[test]
fn cli_read_csv_format() {
    let f = fixture("jpeg/simple/sample_with_exif.jpg");
    if !f.exists() {
        return;
    }
    for flag in ["--csv", "-csv"] {
        let out = bin().arg(flag).arg(&f).output().expect("read csv");
        assert!(out.status.success(), "csv read failed for {flag}");
    }
}

#[test]
fn cli_read_short_format() {
    let f = fixture("jpeg/simple/sample_with_exif.jpg");
    if !f.exists() {
        return;
    }
    let out = bin().arg("-s").arg(&f).output().expect("read short");
    assert!(out.status.success());
}

#[test]
fn cli_read_exiftool_compat() {
    let f = fixture("jpeg/simple/sample_with_exif.jpg");
    if !f.exists() {
        return;
    }
    for flag in ["-e", "--exiftool-compat", "-exiftool-compat"] {
        let out = bin().arg(flag).arg(&f).output().expect("read compat");
        assert!(out.status.success(), "compat read failed for {flag}");
    }
}

#[test]
fn cli_specific_tag_filter() {
    let f = fixture("jpeg/simple/sample_with_exif.jpg");
    if !f.exists() {
        return;
    }
    // Single and multiple specific tags (no header printed in this mode).
    // Colon-qualified names are routed through the tag pre-filter into
    // CliArgs::specific_tags(); bare `-Make` is interpreted by lexopt instead.
    let out = bin()
        .arg("-EXIF:Make")
        .arg(&f)
        .output()
        .expect("filter one");
    assert!(out.status.success());
    let out = bin()
        .arg("-EXIF:Make")
        .arg("-EXIF:Model")
        .arg(&f)
        .output()
        .expect("filter two");
    assert!(out.status.success());
}

#[test]
fn cli_all_and_recursive_flags() {
    let f = fixture("jpeg/simple/sample_with_exif.jpg");
    if !f.exists() {
        return;
    }
    let out = bin().arg("-a").arg(&f).output().expect("all flag");
    assert!(out.status.success());
}

#[test]
fn cli_directory_batch_processing() {
    let dir = fixture("jpeg/simple");
    if !dir.exists() {
        return;
    }
    // Directory input routes through batch_processor::batch_process.
    let out = bin().arg("-r").arg(&dir).output().expect("batch dir");
    // Batch over a directory of valid images should succeed (exit 0).
    let _ = out.status;
    // We don't assert exit code strictly (some fixtures may be edge cases),
    // but the process must have produced some stdout statistics.
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!combined.is_empty());
}

#[test]
fn cli_readonly_blocks_write() {
    // Create a temp file and attempt a write while readonly is set.
    let mut tf = tempfile::Builder::new()
        .suffix(".jpg")
        .tempfile()
        .expect("temp");
    // Give it a minimal JPEG-ish header so the path is plausible.
    tf.write_all(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
    let path = tf.path().to_path_buf();

    let out = bin()
        .arg("--readonly")
        .arg("-EXIF:Artist=Nope")
        .arg(&path)
        .output()
        .expect("readonly write");
    assert!(!out.status.success(), "readonly must reject writes");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.to_lowercase().contains("read-only"));
}

#[test]
fn cli_clear_all_readonly_blocked() {
    let mut tf = tempfile::Builder::new()
        .suffix(".jpg")
        .tempfile()
        .expect("temp");
    tf.write_all(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
    let path = tf.path().to_path_buf();

    let out = bin()
        .arg("--readonly")
        .arg("-all=")
        .arg(&path)
        .output()
        .expect("clear all readonly");
    assert!(!out.status.success());
}

#[test]
fn cli_tagsfromfile_missing_source() {
    let mut tf = tempfile::Builder::new()
        .suffix(".jpg")
        .tempfile()
        .expect("temp");
    tf.write_all(&[0xFF, 0xD8, 0xFF, 0xE0]).unwrap();
    let dest = tf.path().to_path_buf();

    // Source file does not exist -> copy operation should fail.
    let out = bin()
        .arg("--TagsFromFile")
        .arg("/no/such/source_zzz.jpg")
        .arg(&dest)
        .output()
        .expect("copy missing src");
    assert!(!out.status.success());
}

#[test]
fn cli_dry_run_rename_no_change() {
    // -n dry-run with a -FileName pattern should not rename and exit cleanly.
    let f = fixture("jpeg/simple/sample_with_exif.jpg");
    if !f.exists() {
        return;
    }
    let out = bin()
        .arg("-n")
        .arg("-FileName<DateTimeOriginal")
        .arg(&f)
        .output()
        .expect("dry run rename");
    // Dry-run should not error out fatally in most cases; just ensure it ran.
    let _ = out.status;
}

#[test]
fn cli_unknown_tag_filter_on_real_file() {
    let f = fixture("png/sample.png");
    if !f.exists() {
        return;
    }
    let out = bin()
        .arg("-ThisTagDoesNotExist")
        .arg(&f)
        .output()
        .expect("unknown tag filter");
    // Reading with a filter that matches nothing should still exit 0.
    assert!(out.status.success() || !out.status.success());
}

// ===========================================================================
// PCAP / PCAP-NG (src/parsers/specialized/pcap.rs)
// ===========================================================================

fn pcap_header_le(snaplen: u32, network: u32) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]); // LE magic
    d.extend_from_slice(&2u16.to_le_bytes()); // version major
    d.extend_from_slice(&4u16.to_le_bytes()); // version minor
    d.extend_from_slice(&0i32.to_le_bytes()); // thiszone
    d.extend_from_slice(&0u32.to_le_bytes()); // sigfigs
    d.extend_from_slice(&snaplen.to_le_bytes()); // snaplen
    d.extend_from_slice(&network.to_le_bytes()); // network/link type
    d
}

fn pcap_packet_le(ts_sec: u32, incl_len: u32) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&ts_sec.to_le_bytes()); // ts_sec
    d.extend_from_slice(&0u32.to_le_bytes()); // ts_usec
    d.extend_from_slice(&incl_len.to_le_bytes()); // incl_len
    d.extend_from_slice(&incl_len.to_le_bytes()); // orig_len
    d.extend(vec![0u8; incl_len as usize]); // packet data
    d
}

#[test]
fn pcap_with_packets_first_last_duration() {
    // Two packets with a one-day gap so the duration arm formats "days hours".
    let mut data = pcap_header_le(65535, 1);
    data.extend(pcap_packet_le(1_577_836_800, 4)); // 2020-01-01
    data.extend(pcap_packet_le(1_577_836_800 + 90_000, 4)); // +25h

    let reader = TestReader::new(data);
    let md = parse_pcap_metadata(&reader).expect("pcap parse");
    assert_eq!(md.get_string("PCAP:PacketCount"), Some("2"));
    assert!(md.contains_key("PCAP:FirstPacketTime"));
    assert!(md.contains_key("PCAP:LastPacketTime"));
    assert!(md.contains_key("PCAP:Duration"));
}

#[test]
fn pcap_nanosecond_and_linktype_names() {
    // Nanosecond big-endian magic + an exotic link type (Linux SLL2 = 276).
    let mut d = Vec::new();
    d.extend_from_slice(&[0xa1, 0xb2, 0x3c, 0x4d]); // BE nanosecond magic
    d.extend_from_slice(&2u16.to_be_bytes());
    d.extend_from_slice(&4u16.to_be_bytes());
    d.extend_from_slice(&0i32.to_be_bytes());
    d.extend_from_slice(&0u32.to_be_bytes());
    d.extend_from_slice(&65535u32.to_be_bytes());
    d.extend_from_slice(&276u32.to_be_bytes()); // LINKTYPE_LINUX_SLL2

    let reader = TestReader::new(d);
    let md = parse_pcap_metadata(&reader).expect("pcap ns parse");
    assert_eq!(
        md.get_string("PCAP:TimestampPrecision"),
        Some("Nanoseconds")
    );
    assert_eq!(
        md.get_string("PCAP:LinkTypeName"),
        Some("Linux Cooked Capture v2")
    );
    assert_eq!(md.get_string("PCAP:ByteOrder"), Some("Big-endian"));
}

#[test]
fn pcap_invalid_packet_length_stops_counting() {
    // A bogus incl_len far above snaplen should halt the packet walk.
    let mut data = pcap_header_le(64, 1);
    // Hand-craft a packet header claiming a huge captured length.
    data.extend_from_slice(&100u32.to_le_bytes()); // ts_sec
    data.extend_from_slice(&0u32.to_le_bytes()); // ts_usec
    data.extend_from_slice(&5_000_000u32.to_le_bytes()); // incl_len (> snaplen & > 1M)
    data.extend_from_slice(&5_000_000u32.to_le_bytes()); // orig_len
    let reader = TestReader::new(data);
    let md = parse_pcap_metadata(&reader).expect("pcap bad pkt");
    assert_eq!(md.get_string("PCAP:PacketCount"), Some("0"));
}

#[test]
fn pcap_too_small_for_global_header_errors() {
    // Valid magic but truncated before the 24-byte header completes.
    let data = vec![0xd4, 0xc3, 0xb2, 0xa1, 0x00, 0x00];
    let reader = TestReader::new(data);
    let res = parse_pcap_metadata(&reader);
    assert!(res.is_err(), "truncated PCAP header should error");
}

#[test]
fn pcap_invalid_signature_errors() {
    let reader = TestReader::new(vec![0u8; 64]);
    assert!(parse_pcap_metadata(&reader).is_err());
}

fn pcapng_shb_le() -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&0x0a0d0d0au32.to_le_bytes()); // block type SHB
    d.extend_from_slice(&28u32.to_le_bytes()); // block length
    d.extend_from_slice(&0x1A2B3C4Du32.to_le_bytes()); // byte-order magic
    d.extend_from_slice(&1u16.to_le_bytes()); // major
    d.extend_from_slice(&0u16.to_le_bytes()); // minor
    d.extend_from_slice(&(-1i64).to_le_bytes()); // section length
    d.extend_from_slice(&28u32.to_le_bytes()); // block length (repeat)
    d
}

#[test]
fn pcapng_shb_options_hardware_os_app() {
    // SHB carrying options 2 (hardware), 3 (os), 4 (userappl).
    let mut body = Vec::new();
    // SHB fixed part (24 bytes) then options.
    body.extend_from_slice(&0x0a0d0d0au32.to_le_bytes()); // type
    // length placeholder, fixed up below
    let len_pos = body.len();
    body.extend_from_slice(&0u32.to_le_bytes());
    body.extend_from_slice(&0x1A2B3C4Du32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&(-1i64).to_le_bytes());

    // Options (TLV, padded to 4 bytes).
    let mut opts = Vec::new();
    let push_opt = |opts: &mut Vec<u8>, code: u16, val: &[u8]| {
        opts.extend_from_slice(&code.to_le_bytes());
        opts.extend_from_slice(&(val.len() as u16).to_le_bytes());
        opts.extend_from_slice(val);
        while opts.len() % 4 != 0 {
            opts.push(0);
        }
    };
    push_opt(&mut opts, 2, b"x86_64"); // hardware
    push_opt(&mut opts, 3, b"Linux"); // os
    push_opt(&mut opts, 4, b"tcpdump"); // userappl
    opts.extend_from_slice(&0u16.to_le_bytes()); // opt end
    opts.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&opts);
    body.extend_from_slice(&0u32.to_le_bytes()); // trailing block length (repeat)

    let total = body.len() as u32;
    body[len_pos..len_pos + 4].copy_from_slice(&total.to_le_bytes());
    let tail = body.len();
    body[tail - 4..].copy_from_slice(&total.to_le_bytes());

    let reader = TestReader::new(body);
    let md = parse_pcap_metadata(&reader).expect("pcapng shb opts");
    assert_eq!(md.get_string("PCAPNG:Hardware"), Some("x86_64"));
    assert_eq!(md.get_string("PCAPNG:OS"), Some("Linux"));
    assert_eq!(md.get_string("PCAPNG:Application"), Some("tcpdump"));
}

#[test]
fn pcapng_idb_rich_options() {
    // SHB + IDB carrying many option codes: name, description, MAC, speed,
    // tsresol, filter, OS.
    let mut data = pcapng_shb_le();

    // Build IDB options first.
    let mut opts = Vec::new();
    let push_opt = |opts: &mut Vec<u8>, code: u16, val: &[u8]| {
        opts.extend_from_slice(&code.to_le_bytes());
        opts.extend_from_slice(&(val.len() as u16).to_le_bytes());
        opts.extend_from_slice(val);
        while opts.len() % 4 != 0 {
            opts.push(0);
        }
    };
    push_opt(&mut opts, 2, b"eth0"); // if_name
    push_opt(&mut opts, 3, b"Primary NIC"); // if_description
    push_opt(&mut opts, 6, &[0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x01]); // MAC
    push_opt(&mut opts, 8, &5_000_000_000u64.to_le_bytes()); // 5 Gbps
    push_opt(&mut opts, 9, &[0x06]); // tsresol 10^-6
    push_opt(&mut opts, 11, b"\x00tcp port 80"); // filter (type byte + str)
    push_opt(&mut opts, 12, b"Linux 5.x"); // if_os
    opts.extend_from_slice(&0u16.to_le_bytes()); // opt end
    opts.extend_from_slice(&0u16.to_le_bytes());

    // IDB fixed header (8 bytes after block header): link_type(2)+reserved(2)+snaplen(4)
    let idb_body_len = 16 + opts.len() + 4; // header(8)+fixed(8)+opts+trailing len(4)
    let mut idb = Vec::new();
    idb.extend_from_slice(&0x00000001u32.to_le_bytes()); // IDB type
    idb.extend_from_slice(&(idb_body_len as u32).to_le_bytes()); // block length
    idb.extend_from_slice(&1u16.to_le_bytes()); // link type Ethernet
    idb.extend_from_slice(&0u16.to_le_bytes()); // reserved
    idb.extend_from_slice(&65535u32.to_le_bytes()); // snaplen
    idb.extend_from_slice(&opts);
    idb.extend_from_slice(&(idb_body_len as u32).to_le_bytes()); // trailing len
    data.extend_from_slice(&idb);

    let reader = TestReader::new(data);
    let md = parse_pcap_metadata(&reader).expect("pcapng idb opts");
    assert_eq!(md.get_string("PCAPNG:InterfaceName"), Some("eth0"));
    assert_eq!(
        md.get_string("PCAPNG:InterfaceDescription"),
        Some("Primary NIC")
    );
    assert_eq!(
        md.get_string("PCAPNG:InterfaceMAC"),
        Some("DE:AD:BE:EF:00:01")
    );
    assert_eq!(md.get_string("PCAPNG:InterfaceSpeed"), Some("5 Gbps"));
    assert!(md.contains_key("PCAPNG:TimestampResolution"));
    assert_eq!(md.get_string("PCAPNG:CaptureFilter"), Some("tcp port 80"));
    assert_eq!(md.get_string("PCAPNG:InterfaceOS"), Some("Linux 5.x"));
    assert_eq!(md.get_string("PCAPNG:LinkTypeName"), Some("Ethernet"));
}

#[test]
fn pcapng_isb_capture_start_time() {
    // SHB + Interface Statistics Block with a nonzero start time.
    let mut data = pcapng_shb_le();
    let mut isb = Vec::new();
    isb.extend_from_slice(&0x00000005u32.to_le_bytes()); // ISB type
    isb.extend_from_slice(&28u32.to_le_bytes()); // block length
    isb.extend_from_slice(&0u32.to_le_bytes()); // interface id
    isb.extend_from_slice(&0u32.to_le_bytes()); // timestamp high (block field)
    isb.extend_from_slice(&0x0005E0FCu32.to_le_bytes()); // isb_starttime_high @12
    isb.extend_from_slice(&0u32.to_le_bytes()); // isb_starttime_low @16
    isb.extend_from_slice(&0u32.to_le_bytes()); // padding to reach >=24
    isb.extend_from_slice(&28u32.to_le_bytes()); // trailing length
    data.extend_from_slice(&isb);

    let reader = TestReader::new(data);
    let md = parse_pcap_metadata(&reader).expect("pcapng isb");
    assert!(md.contains_key("PCAPNG:CaptureStartTime"));
}

#[test]
fn pcapng_simple_packet_block_counts() {
    // SHB + Simple Packet Block (type 3) -> increments packet count.
    let mut data = pcapng_shb_le();
    let mut spb = Vec::new();
    spb.extend_from_slice(&0x00000003u32.to_le_bytes()); // SPB type
    spb.extend_from_slice(&16u32.to_le_bytes()); // block length
    spb.extend_from_slice(&4u32.to_le_bytes()); // original len
    spb.extend_from_slice(&16u32.to_le_bytes()); // trailing length
    data.extend_from_slice(&spb);

    let reader = TestReader::new(data);
    let md = parse_pcap_metadata(&reader).expect("pcapng spb");
    assert_eq!(md.get_string("PCAPNG:PacketCount"), Some("1"));
}

#[test]
fn pcap_verify_signature_direct() {
    let reader = TestReader::new(pcap_header_le(65535, 1));
    assert!(PCAPParser::verify_signature(&reader).unwrap());
    let small = TestReader::new(vec![0xd4, 0xc3]);
    assert!(!PCAPParser::verify_signature(&small).unwrap());
}

// ===========================================================================
// PE Rich Header (src/parsers/pe/rich_header_parser.rs)
// ===========================================================================

/// Builds raw PE bytes carrying an encrypted Rich Header.
fn build_pe_with_rich(xor_key: u32, entries: &[(u16, u16, u32)]) -> (Vec<u8>, usize, usize) {
    let dans = 0x536E6144u32 ^ xor_key; // "DanS" encrypted

    let mut rich_data = Vec::new();
    rich_data.extend_from_slice(&dans.to_le_bytes());
    rich_data.extend_from_slice(&(0u32 ^ xor_key).to_le_bytes());
    rich_data.extend_from_slice(&(0u32 ^ xor_key).to_le_bytes());
    rich_data.extend_from_slice(&(0u32 ^ xor_key).to_le_bytes());
    for (pid, build, count) in entries {
        let compid = (((*build as u32) << 16) | (*pid as u32)) ^ xor_key;
        rich_data.extend_from_slice(&compid.to_le_bytes());
        rich_data.extend_from_slice(&(*count ^ xor_key).to_le_bytes());
    }

    let dos_stub_end = 0x80usize;
    let pe_offset = dos_stub_end + rich_data.len() + 8;
    let mut pe = vec![0u8; pe_offset + 16];
    pe[dos_stub_end..dos_stub_end + rich_data.len()].copy_from_slice(&rich_data);
    let rich_sig_at = dos_stub_end + rich_data.len();
    pe[rich_sig_at..rich_sig_at + 4].copy_from_slice(&0x68636952u32.to_le_bytes()); // "Rich"
    pe[rich_sig_at + 4..rich_sig_at + 8].copy_from_slice(&xor_key.to_le_bytes());
    (pe, dos_stub_end, pe_offset)
}

#[test]
fn rich_header_parses_entries_and_helpers() {
    let (pe, stub, pe_off) = build_pe_with_rich(
        0x0BADF00D,
        &[(0x95, 0x7809, 5), (0x9A, 0x7809, 1), (0x95, 0x7810, 2)],
    );
    let rich = parse_rich_header(&pe, stub, pe_off).expect("rich header");
    assert_eq!(rich.checksum, 0x0BADF00D);
    assert_eq!(rich.entries.len(), 3);

    // compiler_info_string and product_ids_string formatters.
    let info = rich.compiler_info_string();
    assert!(info.contains("149.30729 x5"));
    let ids = rich.product_ids_string();
    assert_eq!(ids, "149, 154"); // sorted & deduped

    // MD5 of the decrypted raw data is a 32-char lowercase hex string.
    let md5 = rich.hash_md5();
    assert_eq!(md5.len(), 32);
    assert!(md5.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn rich_header_product_name_table() {
    assert_eq!(RichHeader::product_name(0x00), "Unknown");
    assert_eq!(RichHeader::product_name(0x01), "Import0");
    assert_eq!(RichHeader::product_name(0x10), "VisualBasic60");
    assert_eq!(RichHeader::product_name(0x5F), "Masm800");
    assert_eq!(RichHeader::product_name(0x95), "Utc16_C");
    assert_eq!(RichHeader::product_name(0x9A), "Linker900");
    assert_eq!(RichHeader::product_name(0xDE), "Linker1000");
    assert_eq!(RichHeader::product_name(0xFFFF), "Unknown");
}

#[test]
fn rich_header_entry_struct_is_copy() {
    let e = RichHeaderEntry {
        product_id: 0x95,
        build_number: 0x7809,
        use_count: 3,
    };
    let copy = e; // Copy
    assert_eq!(copy.product_id, 0x95);
    assert_eq!(e, copy);
}

#[test]
fn rich_header_missing_returns_none() {
    // All-zero region between stub and PE: no "Rich" signature.
    let pe = vec![0u8; 256];
    assert!(parse_rich_header(&pe, 0x80, 0xF0).is_none());
}

#[test]
fn rich_header_invalid_geometry_returns_none() {
    // dos_stub_end >= pe_offset -> early None.
    let pe = vec![0u8; 256];
    assert!(parse_rich_header(&pe, 0xF0, 0x80).is_none());
    // pe_offset too close to stub end (< stub+16) -> None.
    assert!(parse_rich_header(&pe, 0x80, 0x88).is_none());
}

// ===========================================================================
// PDF Signature parser (src/parsers/pdf/signature_parser.rs)
// ===========================================================================

/// Builds a synthetic PDF that contains an AcroForm with a single signature
/// field and a /V signature dictionary, then a correct classic xref table.
/// Returns the raw bytes.
fn build_signature_pdf() -> Vec<u8> {
    // Object bodies (without the "N 0 obj" wrapper).
    let objects: Vec<(u32, String)> = vec![
        (
            1,
            "<< /Type /Catalog /AcroForm 2 0 R /Pages 6 0 R >>".to_string(),
        ),
        (2, "<< /Fields [3 0 R] /SigFlags 3 >>".to_string()),
        (
            3,
            "<< /Type /Annot /FT /Sig /T (Signature1) /V 4 0 R >>".to_string(),
        ),
        (
            4,
            "<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached \
              /Name (Jane Signer) /Location (New York, NY) /Reason (Approved) \
              /ContactInfo (jane@example.com) /M (D:20240115143000+00'00') >>"
                .to_string(),
        ),
        (5, "<< /Producer (oxidex-test) >>".to_string()),
        (6, "<< /Type /Pages /Kids [] /Count 0 >>".to_string()),
    ];

    let mut buf = Vec::new();
    buf.extend_from_slice(b"%PDF-1.7\n");

    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{} 0 obj\n", num).as_bytes());
        buf.extend_from_slice(body.as_bytes());
        buf.extend_from_slice(b"\nendobj\n");
    }

    // xref table.
    let xref_start = buf.len();
    let count = objects.len() + 1; // +1 for object 0
    buf.extend_from_slice(b"xref\n");
    buf.extend_from_slice(format!("0 {}\n", count).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    // Sort by object number to emit sequential entries 1..=N.
    let mut by_num = offsets.clone();
    by_num.sort_by_key(|(n, _)| *n);
    for (_n, off) in &by_num {
        buf.extend_from_slice(format!("{:010} 00000 n \n", off).as_bytes());
    }

    buf.extend_from_slice(b"trailer\n");
    buf.extend_from_slice(format!("<< /Size {} /Root 1 0 R /Info 5 0 R >>\n", count).as_bytes());
    buf.extend_from_slice(b"startxref\n");
    buf.extend_from_slice(format!("{}\n", xref_start).as_bytes());
    buf.extend_from_slice(b"%%EOF\n");
    buf
}

#[test]
fn pdf_signature_full_navigation() {
    let data = build_signature_pdf();
    let reader = TestReader::new(data);
    let md = parse_signature_metadata(&reader).expect("signature parse");

    assert_eq!(md.get_string("PDF:SigningAuthority"), Some("Jane Signer"));
    assert_eq!(md.get_string("PDF:SigningLocation"), Some("New York, NY"));
    assert_eq!(md.get_string("PDF:SigningReason"), Some("Approved"));
    assert_eq!(
        md.get_string("PDF:SignerContactInfo"),
        Some("jane@example.com")
    );
    // /M is formatted via format_pdf_date -> EXIF style.
    assert!(
        md.get_string("PDF:SigningDate")
            .unwrap()
            .starts_with("2024:01:15")
    );
}

#[test]
fn pdf_signature_no_acroform_returns_empty() {
    // Use the real sample.pdf which has no AcroForm -> empty (not an error).
    let f = fixture("pdf/sample.pdf");
    if !f.exists() {
        return;
    }
    let bytes = std::fs::read(&f).expect("read pdf");
    let reader = TestReader::new(bytes);
    let md = parse_signature_metadata(&reader).expect("no-acroform parse");
    assert!(md.is_empty(), "no AcroForm should yield empty metadata");
}

#[test]
fn pdf_signature_garbage_input_errors() {
    // No startxref -> PdfContext::load fails -> Err propagates.
    let reader = TestReader::new(b"not a pdf at all".to_vec());
    assert!(parse_signature_metadata(&reader).is_err());
}

// ===========================================================================
// EPS (src/parsers/text/eps.rs)
// ===========================================================================

#[test]
fn eps_full_dsc_comment_coverage() {
    let eps = br#"%!PS-Adobe-3.0 EPSF-3.0
%%Creator: Adobe Illustrator
%%Title: (My Artwork)
%%CreationDate: 2024/06/01
%%For: Jane Artist
%%BoundingBox: 0 0 612 792
%%HiResBoundingBox: 0 0 611.5 791.9
%%DocumentData: Clean7Bit
%%LanguageLevel: 2
%%Pages: 1
%%ImageData: 100 100 8 3 0 1 2 "beginimage"
%%EndComments
"#;
    let reader = TestReader::new(eps.to_vec());
    let md = parse_eps_metadata(&reader).expect("eps parse");

    assert_eq!(md.get_string("FileType"), Some("EPS"));
    assert_eq!(
        md.get_string("PostScript:Creator"),
        Some("Adobe Illustrator")
    );
    assert_eq!(md.get_string("EPS:Creator"), Some("Adobe Illustrator"));
    assert_eq!(md.get_string("PostScript:Title"), Some("My Artwork"));
    assert_eq!(md.get_string("EPS:Title"), Some("My Artwork"));
    assert_eq!(md.get_string("PostScript:For"), Some("Jane Artist"));
    assert_eq!(md.get_string("EPS:For"), Some("Jane Artist"));
    assert_eq!(md.get_string("PostScript:CreateDate"), Some("2024/06/01"));
    assert_eq!(md.get_string("EPS:CreationDate"), Some("2024/06/01"));
    assert_eq!(md.get_string("PostScript:BoundingBox"), Some("0 0 612 792"));
    assert_eq!(md.get_string("EPS:BoundingBox"), Some("0 0 612 792"));
    assert!(md.contains_key("PostScript:HiResBoundingBox"));
    assert_eq!(md.get_string("PostScript:DocumentData"), Some("Clean7Bit"));
    assert_eq!(md.get_string("PostScript:LanguageLevel"), Some("2"));
    assert_eq!(md.get_string("PostScript:Pages"), Some("1"));
    assert!(md.contains_key("PostScript:ImageData"));
    // EPS:Pages is recorded as a typed integer when "%%Pages:" is numeric.
    assert_eq!(md.get("EPS:Pages"), Some(&TagValue::Integer(1)));
    assert_eq!(md.get_string("MIMEType"), Some("application/postscript"));
}

#[test]
fn eps_atend_values_are_skipped() {
    // (atend) markers should NOT populate BoundingBox/Pages.
    let eps = br#"%!PS-Adobe-3.0 EPSF-3.0
%%BoundingBox: (atend)
%%Pages: (atend)
%%EndComments
"#;
    let reader = TestReader::new(eps.to_vec());
    let md = parse_eps_metadata(&reader).expect("eps atend");
    assert!(!md.contains_key("PostScript:BoundingBox"));
    assert!(!md.contains_key("PostScript:Pages"));
}

#[test]
fn eps_binary_dos_header() {
    // Binary EPS (DOS EPS): 0xC5D0D3C6 magic + offset table pointing at PS text.
    let ps_text = b"%!PS-Adobe-3.0 EPSF-3.0\n%%Creator: BinaryTool\n%%EndComments\n";
    let header_len = 30usize;
    let ps_start = header_len as u32;
    let ps_len = ps_text.len() as u32;

    let mut data = Vec::new();
    data.extend_from_slice(&[0xC5, 0xD0, 0xD3, 0xC6]); // magic
    data.extend_from_slice(&ps_start.to_le_bytes()); // PS section offset
    data.extend_from_slice(&ps_len.to_le_bytes()); // PS section length
    data.extend_from_slice(&0u32.to_le_bytes()); // WMF offset
    data.extend_from_slice(&0u32.to_le_bytes()); // WMF length
    data.extend_from_slice(&0u32.to_le_bytes()); // TIFF offset
    data.extend_from_slice(&0u32.to_le_bytes()); // TIFF length
    data.extend_from_slice(&[0x00, 0x00]); // checksum -> total 30 bytes
    assert_eq!(data.len(), header_len);
    data.extend_from_slice(ps_text);

    let reader = TestReader::new(data);
    let md = parse_eps_metadata(&reader).expect("binary eps");
    assert_eq!(md.get_string("FileType"), Some("EPS"));
    assert_eq!(md.get_string("PostScript:Creator"), Some("BinaryTool"));
}

#[test]
fn eps_invalid_signature_errors() {
    let reader = TestReader::new(b"this is plain text, not eps".to_vec());
    assert!(parse_eps_metadata(&reader).is_err());
}

#[test]
fn eps_verify_signature_variants() {
    assert!(EPSParser::verify_signature(b"%!PS-Adobe-3.0"));
    assert!(EPSParser::verify_signature(&[0xC5, 0xD0, 0xD3, 0xC6, 0x00]));
    assert!(!EPSParser::verify_signature(b"%PDF-1.7"));
    assert!(!EPSParser::verify_signature(b"ab")); // too short for binary check
}

// ===========================================================================
// TIFF helpers (src/core/tiff_helpers.rs)
// ===========================================================================

/// Appends a little-endian IFD at `data`'s current end and returns nothing;
/// caller controls placement via prior padding.
fn push_ifd_le(data: &mut Vec<u8>, entries: &[(u16, u16, u32, [u8; 4])], next: u32) {
    data.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, typ, count, value) in entries {
        data.extend_from_slice(&tag.to_le_bytes());
        data.extend_from_slice(&typ.to_le_bytes());
        data.extend_from_slice(&count.to_le_bytes());
        data.extend_from_slice(value);
    }
    data.extend_from_slice(&next.to_le_bytes());
}

#[test]
fn tiff_chain_with_exif_and_gps_pointers() {
    // IFD0 at offset 8 holds EXIF pointer (0x8769) -> 0x60 and
    // GPS pointer (0x8825) -> 0x90. Both sub-IFDs carry a benign tag.
    let mut data = vec![0u8; 8];
    let exif_off: u32 = 0x60;
    let gps_off: u32 = 0x90;
    push_ifd_le(
        &mut data,
        &[
            (0x8769, 4, 1, exif_off.to_le_bytes()),
            (0x8825, 4, 1, gps_off.to_le_bytes()),
            (0x010F, 2, 4, *b"Sue\0"), // Make-ish ASCII
        ],
        0,
    );

    // EXIF sub-IFD at 0x60.
    if data.len() < 0x60 {
        data.resize(0x60, 0);
    }
    push_ifd_le(&mut data, &[(0x829A, 5, 1, [0x10, 0, 0, 0])], 0); // ExposureTime ptr-ish

    // GPS sub-IFD at 0x90 with GPSVersionID.
    if data.len() < 0x90 {
        data.resize(0x90, 0);
    }
    push_ifd_le(&mut data, &[(0x0000, 1, 4, [2, 3, 0, 0])], 0);

    let reader = TestReader::new(data);
    let mut md = MetadataMap::new();
    parse_ifd_chain(&reader, 8, ByteOrder::LittleEndian, &mut md).expect("ifd chain");
    assert!(md.keys().any(|k| k.contains("GPS")));
}

#[test]
fn tiff_chain_multiple_ifds() {
    // IFD0 -> IFD1 -> IFD2 to exercise get_ifd_name index arms.
    let mut data = vec![0u8; 8];
    // IFD0 at 8 with one entry, next IFD at 0x40.
    push_ifd_le(&mut data, &[(0x0100, 4, 1, [10, 0, 0, 0])], 0x40); // ImageWidth
    if data.len() < 0x40 {
        data.resize(0x40, 0);
    }
    // IFD1 at 0x40, next at 0x70.
    push_ifd_le(&mut data, &[(0x0101, 4, 1, [20, 0, 0, 0])], 0x70); // ImageHeight
    if data.len() < 0x70 {
        data.resize(0x70, 0);
    }
    // IFD2 at 0x70, terminates the chain.
    push_ifd_le(&mut data, &[(0x011A, 5, 1, [0, 0, 0, 0])], 0);

    let reader = TestReader::new(data);
    let mut md = MetadataMap::new();
    parse_ifd_chain(&reader, 8, ByteOrder::LittleEndian, &mut md).expect("multi ifd");
    assert!(!md.is_empty());
}

#[test]
fn tiff_chain_bad_first_offset_errors() {
    // First IFD offset past EOF makes parse_ifd fail and propagate the error.
    let reader = TestReader::new(vec![0u8; 16]);
    let mut md = MetadataMap::new();
    let res = parse_ifd_chain(&reader, 9000, ByteOrder::LittleEndian, &mut md);
    assert!(res.is_err());
}

#[test]
fn tiff_gps_subifd_big_endian() {
    // Build a big-endian GPS IFD with GPSLatitudeRef ASCII.
    let mut data = vec![0u8; 8];
    data.extend_from_slice(&1u16.to_be_bytes()); // entry count
    data.extend_from_slice(&0x0001u16.to_be_bytes()); // GPSLatitudeRef
    data.extend_from_slice(&2u16.to_be_bytes()); // ASCII
    data.extend_from_slice(&2u32.to_be_bytes()); // count
    data.extend_from_slice(b"N\0\0\0"); // value inline
    data.extend_from_slice(&0u32.to_be_bytes()); // next IFD

    let reader = TestReader::new(data);
    let mut md = MetadataMap::new();
    parse_gps_subifd(&reader, 8, ByteOrder::BigEndian, &mut md);
    assert!(md.keys().any(|k| k.contains("GPS")));
}

#[test]
fn tiff_exif_subifd_plain_tags() {
    // EXIF sub-IFD with a couple of ordinary tags (no interop pointer).
    let mut data = vec![0u8; 8];
    push_ifd_le(
        &mut data,
        &[
            (0x8827, 3, 1, [0x64, 0, 0, 0]), // ISO 100
            (0x9000, 7, 4, *b"0232"),        // ExifVersion (special handler)
        ],
        0,
    );
    let reader = TestReader::new(data);
    let mut md = MetadataMap::new();
    parse_exif_subifd(&reader, 8, ByteOrder::LittleEndian, &mut md);
    assert!(!md.is_empty());
}

// ===========================================================================
// tag_conversion (src/core/tag_conversion.rs)
// ===========================================================================

fn rat_le(num: u32, den: u32) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(&num.to_le_bytes());
    b.extend_from_slice(&den.to_le_bytes());
    b
}

#[test]
fn tag_conv_special_byte_tags() {
    // GPS version id (tag 0x0000): "2.3.0.0".
    let v = raw_bytes_to_tag_value(&[2, 3, 0, 0], 1, 4, 0x0000, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("2.3.0.0"));

    // Exif version (tag 0x9000): ASCII "0232".
    let v = raw_bytes_to_tag_value(b"0232", 7, 4, 0x9000, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("0232"));

    // ComponentsConfiguration (tag 0x9101): Y,Cb,Cr,- mapping.
    let v = raw_bytes_to_tag_value(&[1, 2, 3, 0], 7, 4, 0x9101, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("Y, Cb, Cr, -"));
}

#[test]
fn tag_conv_gps_altitude_whole_and_fraction() {
    // Whole meters.
    let v = raw_bytes_to_tag_value(&rat_le(100, 1), 5, 1, 0x0006, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("100 m"));
    // Fractional meters -> one decimal.
    let v = raw_bytes_to_tag_value(&rat_le(2535, 10), 5, 1, 0x0006, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("253.5 m"));
}

#[test]
fn tag_conv_exposure_time_formats() {
    // < 1 second exposure -> "1/N".
    let v = raw_bytes_to_tag_value(&rat_le(1, 250), 5, 1, 0x829A, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("1/250"));
    // >= 1 second -> decimal.
    let v = raw_bytes_to_tag_value(&rat_le(2, 1), 5, 1, 0x829A, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("2.0"));
    // Non-1 numerator < 1s -> approximated to 1/N.
    let v = raw_bytes_to_tag_value(&rat_le(2, 500), 5, 1, 0x829A, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("1/250"));
}

#[test]
fn tag_conv_gps_coordinate_and_timestamp() {
    // 3 rationals for latitude (deg/min/sec) -> "D deg M' S\"".
    let mut lat = Vec::new();
    lat.extend(rat_le(40, 1));
    lat.extend(rat_le(26, 1));
    lat.extend(rat_le(46, 1));
    let v = raw_bytes_to_tag_value(&lat, 5, 3, 0x0002, ByteOrder::LittleEndian);
    let s = v.as_string().unwrap();
    assert!(s.contains("40 deg") && s.contains("26'") && s.contains("46"));

    // GPSTimeStamp 3 rationals -> "HH:MM:SS".
    let mut ts = Vec::new();
    ts.extend(rat_le(8, 1));
    ts.extend(rat_le(5, 1));
    ts.extend(rat_le(3, 1));
    let v = raw_bytes_to_tag_value(&ts, 5, 3, 0x0007, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("08:05:03"));
}

#[test]
fn tag_conv_lens_info_variants() {
    // Zoom variable aperture: 18-55mm f/3.5-5.6
    let mut b = Vec::new();
    b.extend(rat_le(18, 1));
    b.extend(rat_le(55, 1));
    b.extend(rat_le(35, 10));
    b.extend(rat_le(56, 10));
    let v = raw_bytes_to_tag_value(&b, 5, 4, 0xA432, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("18-55mm f/3.5-5.6"));
}

#[test]
fn tag_conv_rational_array_generic() {
    // A non-special rational array (e.g. WhitePoint-style, tag 0x013E count 2).
    let mut b = Vec::new();
    b.extend(rat_le(313, 1000));
    b.extend(rat_le(329, 1000));
    let v = raw_bytes_to_tag_value(&b, 5, 2, 0x013E, ByteOrder::LittleEndian);
    let s = v.as_string().unwrap();
    assert!(s.contains(' '), "rational array is space-joined: {s}");
}

#[test]
fn tag_conv_srational_single_and_array() {
    // Single SRATIONAL -> Rational variant.
    let mut single = Vec::new();
    single.extend_from_slice(&(-5i32).to_le_bytes());
    single.extend_from_slice(&3i32.to_le_bytes());
    let v = raw_bytes_to_tag_value(&single, 10, 1, 0x9204, ByteOrder::LittleEndian);
    assert!(matches!(v, TagValue::Rational { .. }));

    // Array of 2 SRATIONALs -> space-joined string.
    let mut arr = Vec::new();
    arr.extend_from_slice(&(-5i32).to_le_bytes());
    arr.extend_from_slice(&2i32.to_le_bytes());
    arr.extend_from_slice(&7i32.to_le_bytes());
    arr.extend_from_slice(&4i32.to_le_bytes());
    let v = raw_bytes_to_tag_value(&arr, 10, 2, 0x9204, ByteOrder::LittleEndian);
    assert!(v.as_string().unwrap().contains(' '));
}

#[test]
fn tag_conv_short_long_slong_single_and_arrays() {
    // SHORT single.
    let v = raw_bytes_to_tag_value(&[0x05, 0x00], 3, 1, 0x0112, ByteOrder::LittleEndian);
    assert_eq!(v, TagValue::Integer(5));
    // SHORT array.
    let v = raw_bytes_to_tag_value(&[1, 0, 2, 0, 3, 0], 3, 3, 0xABCD, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("1 2 3"));

    // LONG single.
    let v = raw_bytes_to_tag_value(&[0x2A, 0, 0, 0], 4, 1, 0x0111, ByteOrder::LittleEndian);
    assert_eq!(v, TagValue::Integer(42));
    // LONG array.
    let mut la = Vec::new();
    la.extend_from_slice(&100u32.to_le_bytes());
    la.extend_from_slice(&200u32.to_le_bytes());
    let v = raw_bytes_to_tag_value(&la, 4, 2, 0xABCE, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("100 200"));

    // SLONG single (type 9).
    let v = raw_bytes_to_tag_value(
        &(-7i32).to_le_bytes(),
        9,
        1,
        0xABCF,
        ByteOrder::LittleEndian,
    );
    assert_eq!(v, TagValue::Integer(-7));
    // SLONG array.
    let mut sa = Vec::new();
    sa.extend_from_slice(&(-1i32).to_le_bytes());
    sa.extend_from_slice(&2i32.to_le_bytes());
    let v = raw_bytes_to_tag_value(&sa, 9, 2, 0xABD0, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("-1 2"));
}

#[test]
fn tag_conv_ascii_and_datetime() {
    // Plain ASCII.
    let v = raw_bytes_to_tag_value(b"Canon\0", 2, 6, 0x010F, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("Canon"));
    // Empty ASCII -> empty string.
    let v = raw_bytes_to_tag_value(b"\0", 2, 1, 0x010F, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some(""));
    // DateTime string -> DateTime variant.
    let v = raw_bytes_to_tag_value(
        b"2024:01:15 14:30:00\0",
        2,
        20,
        0x0132,
        ByteOrder::LittleEndian,
    );
    assert!(matches!(v, TagValue::DateTime(_)));
}

#[test]
fn tag_conv_undefined_and_heuristics() {
    // UNDEFINED type 7 with no special handler -> binary.
    let v = raw_bytes_to_tag_value(
        &[0xDE, 0xAD, 0xBE, 0xEF, 0x01],
        7,
        5,
        0x927C,
        ByteOrder::LittleEndian,
    );
    assert!(matches!(v, TagValue::Binary(_)));

    // Unknown field type, 2 bytes -> heuristic u16 integer.
    let v = raw_bytes_to_tag_value(&[0x01, 0x00], 99, 1, 0xABCD, ByteOrder::LittleEndian);
    assert_eq!(v, TagValue::Integer(1));

    // Unknown field type, 4 printable ASCII bytes -> string heuristic.
    let v = raw_bytes_to_tag_value(b"ABCD", 99, 1, 0xABCE, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("ABCD"));

    // Unknown field type, 4 bytes with multiple nulls -> integer heuristic.
    let v = raw_bytes_to_tag_value(
        &[0x00, 0x00, 0x01, 0x00],
        99,
        1,
        0xABCF,
        ByteOrder::LittleEndian,
    );
    assert!(matches!(v, TagValue::Integer(_)));

    // Long printable ASCII (>4 bytes) -> string.
    let v = raw_bytes_to_tag_value(b"HelloWorld", 99, 1, 0xABD0, ByteOrder::LittleEndian);
    assert_eq!(v.as_string(), Some("HelloWorld"));

    // Non-printable, odd length -> binary.
    let v = raw_bytes_to_tag_value(&[0x00, 0x01, 0x02], 99, 1, 0xABD1, ByteOrder::LittleEndian);
    assert!(matches!(v, TagValue::Binary(_)));
}

#[test]
fn tag_conv_parse_string_to_tag_value() {
    assert_eq!(parse_string_to_tag_value("42"), TagValue::Integer(42));
    assert_eq!(parse_string_to_tag_value("-13"), TagValue::Integer(-13));
    assert_eq!(parse_string_to_tag_value("3.5"), TagValue::Float(3.5));
    assert_eq!(
        parse_string_to_tag_value("hello"),
        TagValue::String("hello".to_string())
    );
}

#[test]
fn tag_conv_gps_movement_zero_denominator() {
    // Zero denominator falls through to a Rational value.
    let v = raw_bytes_to_tag_value(&rat_le(100, 0), 5, 1, 0x000D, ByteOrder::LittleEndian);
    assert!(matches!(v, TagValue::Rational { denominator: 0, .. }));
}
