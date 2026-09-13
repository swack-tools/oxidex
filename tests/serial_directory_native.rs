//! Native callback replay for the inactive generated `ProcessSerialData` tables.
//!
//! This test intentionally compares only the surface that the native probe
//! observes: `FoundTag` input values, `GetTagInfo` selection/group facts, and
//! bounded `ReadValue` progression through the selected `Real::AudioV3` and
//! `Real::AudioV4` tables. The probe itself records that a `FoundTag` callback
//! does **not** prove final ExifTool key/group reporting, so these assertions
//! do not claim carrier activation or final metadata-group parity.
//!
//! Run explicitly with the canonical native environment:
//!
//! ```text
//! env -u PERL5LIB -u PERLLIB -u PERL5OPT \
//!   EXIFTOOL_PERL=/path/to/perl5.38.2 \
//!   OXIDEX_PINNED_EXIFTOOL=/path/to/exiftool-13.59 \
//!   cargo test --test serial_directory_native -- --ignored
//! ```

use std::collections::HashMap;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use oxidex::core::TagValue;
use oxidex::exiftool_tables::{
    Ctx, Emitted, MemberValue, SerialDir, SerialEmissionSink, SerialTable, find_serial_table,
    process_serial_directory,
};
use oxidex::io::ByteOrder;
use serde_json::{Value, json};

const CANONICAL_PERL: &str = "v5.38.2";
const EXIFTOOL_VERSION: &str = "13.59";
const PROBE: &str = "tools/exiftool-tables/probe_serial_processor.pl";

#[derive(Clone, Copy)]
enum Order {
    Ii,
    Mm,
}

impl Order {
    const fn native(self) -> &'static str {
        match self {
            Self::Ii => "II",
            Self::Mm => "MM",
        }
    }

    const fn rust(self) -> ByteOrder {
        match self {
            Self::Ii => ByteOrder::LittleEndian,
            Self::Mm => ByteOrder::BigEndian,
        }
    }
}

#[derive(Default)]
struct Sink {
    rows: Vec<Emitted>,
    unknown: bool,
}

impl SerialEmissionSink for Sink {
    fn emit(&mut self, row: Emitted) {
        self.rows.push(row);
    }

    fn serial_enabled(&self, _table: &'static SerialTable) -> bool {
        true
    }

    fn unknown_enabled(&self) -> bool {
        self.unknown
    }
}

struct NativeEnv {
    perl: PathBuf,
    source: PathBuf,
}

impl NativeEnv {
    fn require() -> Self {
        let perl = PathBuf::from(env::var("EXIFTOOL_PERL").expect(
            "serial native replay was invoked: set EXIFTOOL_PERL to canonical Perl v5.38.2",
        ));
        let source = PathBuf::from(env::var("OXIDEX_PINNED_EXIFTOOL").expect(
            "serial native replay was invoked: set OXIDEX_PINNED_EXIFTOOL to ExifTool 13.59 source",
        ));
        assert!(
            perl.is_file(),
            "EXIFTOOL_PERL is not a file: {}",
            perl.display()
        );
        assert!(
            source.join("lib/Image/ExifTool/Real.pm").is_file(),
            "OXIDEX_PINNED_EXIFTOOL lacks lib/Image/ExifTool/Real.pm: {}",
            source.display()
        );
        assert!(
            source.join("lib/Image/ExifTool/Canon.pm").is_file(),
            "OXIDEX_PINNED_EXIFTOOL lacks lib/Image/ExifTool/Canon.pm: {}",
            source.display()
        );
        let version = Command::new(&perl)
            .args(["-e", "print $^V"])
            .output()
            .expect("run EXIFTOOL_PERL -e print $^V");
        assert!(
            version.status.success(),
            "EXIFTOOL_PERL version probe failed"
        );
        assert_eq!(
            String::from_utf8(version.stdout).expect("Perl version is UTF-8"),
            CANONICAL_PERL,
            "serial native replay requires canonical Perl {CANONICAL_PERL}"
        );
        let exiftool_version = Command::new(&perl)
            .args([
                "-Ilib",
                "-MImage::ExifTool",
                "-e",
                "print $Image::ExifTool::VERSION",
            ])
            .current_dir(&source)
            .env_remove("PERL5LIB")
            .env_remove("PERLLIB")
            .env_remove("PERL5OPT")
            .output()
            .expect("load selected Image::ExifTool source");
        assert!(
            exiftool_version.status.success(),
            "selected Image::ExifTool version probe failed"
        );
        assert_eq!(
            String::from_utf8(exiftool_version.stdout).expect("ExifTool version is UTF-8"),
            EXIFTOOL_VERSION,
            "serial native replay requires pinned ExifTool {EXIFTOOL_VERSION}"
        );
        Self { perl, source }
    }
}

fn u16(out: &mut Vec<u8>, order: Order, value: u16) {
    out.extend_from_slice(&match order {
        Order::Ii => value.to_le_bytes(),
        Order::Mm => value.to_be_bytes(),
    });
}

fn u32(out: &mut Vec<u8>, order: Order, value: u32) {
    out.extend_from_slice(&match order {
        Order::Ii => value.to_le_bytes(),
        Order::Mm => value.to_be_bytes(),
    });
}

fn real_v3(order: Order, title: &[u8], artist: &[u8], copyright: &[u8], comment: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    u16(&mut out, order, 2); // Channels
    for _ in 0..3 {
        u16(&mut out, order, 0); // Unknown[3]
    }
    u16(&mut out, order, 120); // BytesPerMinute
    u32(&mut out, order, 1000); // AudioBytes
    for value in [title, artist, copyright, comment] {
        out.push(u8::try_from(value.len()).expect("fixture length fits u8"));
        out.extend_from_slice(value);
    }
    out
}

fn real_v4(order: Order) -> Vec<u8> {
    let mut out = b"RA4!".to_vec(); // FourCC1 undef[4]
    u32(&mut out, order, 1000); // AudioFileSize
    u16(&mut out, order, 2); // Version2
    u32(&mut out, order, 32); // HeaderSize
    u16(&mut out, order, 7); // CodecFlavorID
    u32(&mut out, order, 512); // CodedFrameSize
    u32(&mut out, order, 9000); // AudioBytes
    u32(&mut out, order, 1200); // BytesPerMinute
    u32(&mut out, order, 0); // Unknown
    u16(&mut out, order, 1); // SubPacketH
    u16(&mut out, order, 256); // AudioFrameSize
    u16(&mut out, order, 64); // SubPacketSize
    u16(&mut out, order, 0); // Unknown
    u16(&mut out, order, 44_100); // SampleRate
    u16(&mut out, order, 0); // Unknown
    u16(&mut out, order, 16); // BitsPerSample
    u16(&mut out, order, 2); // Channels
    for value in [b"CO2!".as_slice(), b"CO3!".as_slice()] {
        out.push(u8::try_from(value.len()).expect("fixture length fits u8"));
        out.extend_from_slice(value);
    }
    out.push(0); // Unknown
    u16(&mut out, order, 0); // Unknown
    for value in [
        b"Title".as_slice(),
        b"Artist".as_slice(),
        b"C".as_slice(),
        b"Hi".as_slice(),
    ] {
        out.push(u8::try_from(value.len()).expect("fixture length fits u8"));
        out.extend_from_slice(value);
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn native_replay(
    native: &NativeEnv,
    table: &str,
    case: &str,
    order: Order,
    data: &[u8],
    dir_start: usize,
    dir_len: usize,
    unknown: bool,
) -> Value {
    let request = json!({
        "protocol": "oxidex.serial_processor.v1",
        "module": "Real",
        "table": table,
        "case": {
            "name": case,
            "byte_order": order.native(),
            "data_hex": hex(data),
            "dir_start": dir_start,
            "dir_len": dir_len,
            "unknown": if unknown { 1 } else { 0 },
            "verbose": 1,
            "members": {},
        },
    });
    let probe = Path::new(env!("CARGO_MANIFEST_DIR")).join(PROBE);
    assert!(
        probe.is_file(),
        "serial native probe is missing: {}",
        probe.display()
    );
    let mut child = Command::new(&native.perl)
        .arg(probe)
        .args(["--lib", "lib"])
        .current_dir(&native.source)
        .env_remove("PERL5LIB")
        .env_remove("PERLLIB")
        .env_remove("PERL5OPT")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start native serial probe");
    child
        .stdin
        .as_mut()
        .expect("native probe stdin")
        .write_all(format!("{request}\n").as_bytes())
        .expect("write native serial request");
    let output = child
        .wait_with_output()
        .expect("wait for native serial probe");
    assert!(
        output.status.success(),
        "native serial probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "native serial probe stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("native serial JSONL is UTF-8");
    let mut replies = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("valid native serial JSON"));
    let reply = replies.next().expect("one native serial reply");
    assert!(
        replies.next().is_none(),
        "one request must produce one reply"
    );
    assert_eq!(
        reply["ok"],
        Value::Bool(true),
        "native serial probe error: {reply}"
    );
    reply
}

fn rust_replay(
    table: &'static SerialTable,
    order: Order,
    data: &[u8],
    dir_start: usize,
    dir_len: usize,
    unknown: bool,
) -> (Vec<Emitted>, oxidex::exiftool_tables::SerialWalkResult) {
    let mut sink = Sink {
        unknown,
        ..Sink::default()
    };
    let mut members: HashMap<&'static str, MemberValue> = HashMap::new();
    let result = process_serial_directory(
        table,
        SerialDir {
            data,
            dir_start,
            dir_len,
            base: 0,
            data_pos: 0,
            byte_order: order.rust(),
        },
        &mut Ctx::new(&mut members),
        &mut sink,
    );
    (sink.rows, result)
}

fn native_rows(reply: &Value) -> Vec<(String, String)> {
    reply["found_tags"]
        .as_array()
        .expect("native found_tags array")
        .iter()
        .map(|row| {
            let name = row["tag_info"]["name"]["string"]
                .as_str()
                .expect("native tag name")
                .to_owned();
            let value = row["value"]["string"]
                .as_str()
                .expect("V3/V4 callback value is a byte or numeric scalar")
                .to_owned();
            (name, value)
        })
        .collect()
}

fn row_value(value: &TagValue) -> String {
    value
        .as_string()
        .map(str::to_owned)
        .or_else(|| value.as_integer().map(|value| value.to_string()))
        .unwrap_or_else(|| panic!("V3/V4 used subset must emit a string or integer: {value:?}"))
}

fn rust_rows(rows: &[Emitted]) -> Vec<(String, String)> {
    rows.iter()
        .map(|row| (row.name.to_owned(), row_value(&row.value)))
        .collect()
}

fn assert_source_groups(table: &'static SerialTable, reply: &Value) {
    let groups = &reply["selection"]["table"]["groups"]["values"];
    assert_eq!(groups["0"]["string"].as_str(), Some(table.group0));
    assert_eq!(groups["1"]["string"].as_str(), Some(table.group1));
    assert_eq!(groups["2"]["string"].as_str(), Some(table.group2));

    // This compares source facts exposed through native GetTagInfo to the
    // generated tag facts. It deliberately does not infer final metadata
    // groups from the FoundTag callback; the probe labels that unobservable.
    for row in reply["get_tag_info"]
        .as_array()
        .expect("native GetTagInfo array")
    {
        let Some(values) = row["result"]["groups"]["values"].as_object() else {
            continue;
        };
        let name = row["result"]["name"]["string"]
            .as_str()
            .expect("grouped native row name");
        let tag = table
            .entries
            .iter()
            .flat_map(|entry| entry.alternatives)
            .find(|tag| tag.name == name)
            .unwrap_or_else(|| panic!("generated serial row missing native group fact for {name}"));
        for (group, expected) in [
            ("0", tag.groups.g0),
            ("1", tag.groups.g1),
            ("2", tag.groups.g2),
        ] {
            match expected {
                Some(expected) => assert_eq!(
                    values[group]["string"].as_str(),
                    Some(expected),
                    "{name} group {group}"
                ),
                None => assert!(
                    values.get(group).is_none(),
                    "{name} unexpectedly has group {group}"
                ),
            }
        }
    }
}

fn table(name: &str) -> &'static SerialTable {
    let table =
        find_serial_table("Real", name).unwrap_or_else(|| panic!("Real::{name} must be generated"));
    assert!(
        table.gate_a.passes(),
        "Real::{name} gate A: {:?}",
        table.gate_a.blocked_by
    );
    table
}

fn assert_case(
    native: &NativeEnv,
    table_name: &str,
    case: &str,
    order: Order,
    data: &[u8],
    dir_start: usize,
    dir_len: usize,
    unknown: bool,
) {
    let native_reply = native_replay(
        native, table_name, case, order, data, dir_start, dir_len, unknown,
    );
    let table = table(table_name);
    let (rows, result) = rust_replay(table, order, data, dir_start, dir_len, unknown);
    assert!(
        !result.tainted,
        "{case}: shared serial reader refused a native V3/V4 subset case: {result:?}"
    );
    assert_eq!(
        rust_rows(&rows),
        native_rows(&native_reply),
        "{case}: FoundTag input projection"
    );
    assert_source_groups(table, &native_reply);
}

#[test]
#[ignore = "requires EXIFTOOL_PERL v5.38.2 and OXIDEX_PINNED_EXIFTOOL; run explicitly"]
fn real_audio_serial_tables_replay_pinned_native_callbacks() {
    let native = NativeEnv::require();
    for order in [Order::Ii, Order::Mm] {
        let v3 = real_v3(order, b"Hi!", b"Me", b"C", b"Yo");
        assert_case(
            &native,
            "AudioV3",
            "v3-normal",
            order,
            &v3,
            0,
            v3.len(),
            false,
        );
        let v4 = real_v4(order);
        assert_case(
            &native,
            "AudioV4",
            "v4-normal",
            order,
            &v4,
            0,
            v4.len(),
            false,
        );
    }

    let normal = real_v3(Order::Ii, b"Hi!", b"Me", b"C", b"Yo");
    assert_case(
        &native,
        "AudioV3",
        "v3-unknown-off",
        Order::Ii,
        &normal,
        0,
        normal.len(),
        false,
    );
    assert_case(
        &native,
        "AudioV3",
        "v3-unknown-on",
        Order::Ii,
        &normal,
        0,
        normal.len(),
        true,
    );

    let zero = real_v3(Order::Ii, b"", b"", b"", b"");
    assert_case(
        &native,
        "AudioV3",
        "v3-zero-strings",
        Order::Ii,
        &zero,
        0,
        zero.len(),
        false,
    );
    let nul = real_v3(Order::Ii, b"H\0!", b"Me", b"C", b"Yo");
    assert_case(
        &native,
        "AudioV3",
        "v3-nul-string",
        Order::Ii,
        &nul,
        0,
        nul.len(),
        false,
    );
    let truncated = &normal[..15]; // title count remains 3 but no title byte fits.
    assert_case(
        &native,
        "AudioV3",
        "v3-truncated-string",
        Order::Ii,
        truncated,
        0,
        truncated.len(),
        false,
    );

    let mut bounded = vec![0xa5, 0x5a, 0xff];
    bounded.extend_from_slice(&normal);
    assert_case(
        &native,
        "AudioV3",
        "v3-bounded-nonzero-offset",
        Order::Ii,
        &bounded,
        3,
        normal.len(),
        false,
    );
}
