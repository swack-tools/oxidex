//! Native callback replay for the inactive generated `ProcessSerialData` tables.
//!
//! This test intentionally compares only the surface that the native probe
//! observes: `FoundTag` input values, `GetTagInfo` selection/group facts, and
//! bounded `ReadValue` progression through the selected `Real::AudioV3`,
//! `Real::AudioV4`, `Canon::AFInfo`, and `Canon::AFInfo2` tables. The probe
//! itself records that a `FoundTag` callback does **not** prove final ExifTool
//! key/group reporting, so these assertions do not claim carrier activation
//! or final metadata-group parity.
//!
//! Run explicitly with the canonical native environment:
//!
//! ```text
//! env -u PERL5LIB -u PERLLIB -u PERL5OPT \
//!   EXIFTOOL_PERL=perl \
//!   OXIDEX_PINNED_EXIFTOOL=/path/to/pinned-exiftool \
//!   cargo test --test serial_directory_native -- --ignored
//! ```

use std::collections::HashMap;
use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use oxidex::core::TagValue;
use oxidex::exiftool_oracle::repo_pin;
use oxidex::exiftool_tables::{
    find_serial_table, process_serial_directory, Ctx, Emitted, MemberValue, SerialDir,
    SerialEmissionSink, SerialTable,
};
use oxidex::io::ByteOrder;
use serde_json::{json, Value};

const CANONICAL_PERL: &str = "v5.38.2";
const PROBE: &str = "tools/exiftool-tables/probe_serial_processor.pl";

#[derive(Clone, Copy, Debug)]
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
            Self::Ii => ByteOrder::Little,
            Self::Mm => ByteOrder::Big,
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
    /// Command name or absolute path. CI deliberately supplies `perl`.
    perl: String,
    source: PathBuf,
}

#[derive(Clone, Copy)]
struct ReplayCase<'a> {
    module: &'static str,
    table: &'static str,
    name: &'static str,
    order: Order,
    data: &'a [u8],
    dir_start: usize,
    dir_len: usize,
    unknown: bool,
    members: &'a [MemberSeed],
}

#[derive(Clone, Copy)]
enum MemberSeed {
    Text(&'static str, &'static str),
    Number(&'static str, i64),
}

impl MemberSeed {
    fn insert_native(self, members: &mut serde_json::Map<String, Value>) {
        match self {
            Self::Text(name, value) => {
                members.insert(name.to_owned(), Value::String(value.to_owned()));
            }
            Self::Number(name, value) => {
                members.insert(name.to_owned(), Value::Number(value.into()));
            }
        }
    }

    fn insert_rust(self, members: &mut HashMap<&'static str, MemberValue>) {
        match self {
            Self::Text(name, value) => {
                members.insert(name, MemberValue::Str(value.to_owned()));
            }
            Self::Number(name, value) => {
                members.insert(name, MemberValue::Num(value));
            }
        }
    }
}

impl NativeEnv {
    fn require() -> Self {
        let perl = env::var("EXIFTOOL_PERL").expect(
            "serial native replay was invoked: set EXIFTOOL_PERL to canonical Perl v5.38.2",
        );
        assert!(
            !perl.is_empty(),
            "EXIFTOOL_PERL must name an interpreter command or path"
        );
        let source = PathBuf::from(env::var("OXIDEX_PINNED_EXIFTOOL").expect(
            "serial native replay was invoked: set OXIDEX_PINNED_EXIFTOOL to ExifTool 13.59 source",
        ));
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
            repo_pin(),
            "serial native replay requires the repository-pinned ExifTool {}",
            repo_pin()
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

/// Construct the native numeric-key sequence for Canon::AFInfo. The table,
/// rather than this fixture, decides which index-11 alternative consumes the
/// supplied tail words.
fn afinfo(
    order: Order,
    point_count: u16,
    x: &[u16],
    y: &[u16],
    focus: &[u16],
    tail: &[u16],
) -> Vec<u8> {
    assert_eq!(x.len(), usize::from(point_count));
    assert_eq!(y.len(), usize::from(point_count));
    assert_eq!(focus.len(), (usize::from(point_count) + 15) / 16);
    let mut out = Vec::new();
    for value in [point_count, 1, 1_000, 800, 200, 150, 40, 30] {
        u16(&mut out, order, value);
    }
    for values in [x, y, focus, tail] {
        for value in values {
            u16(&mut out, order, *value);
        }
    }
    out
}

/// Construct Canon::AFInfo2 through its selected index-13 payload. The four
/// signed arrays and focus words are source-counted by NumAFPoints at slot 2.
fn afinfo2(
    order: Order,
    point_count: u16,
    area_words: &[u16],
    focus: &[u16],
    tail: &[u16],
) -> Vec<u8> {
    assert_eq!(area_words.len(), usize::from(point_count));
    assert_eq!(focus.len(), (usize::from(point_count) + 15) / 16);
    let mut out = Vec::new();
    for value in [99, 2, point_count, point_count, 1_000, 800, 200, 150] {
        u16(&mut out, order, value);
    }
    for values in [area_words, area_words, area_words, area_words, focus, tail] {
        for value in values {
            u16(&mut out, order, *value);
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn native_replay(native: &NativeEnv, case: &ReplayCase<'_>) -> Value {
    let mut members = serde_json::Map::new();
    for member in case.members {
        member.insert_native(&mut members);
    }
    let request = json!({
        "protocol": "oxidex.serial_processor.v1",
        "module": case.module,
        "table": case.table,
        "case": {
            "name": case.name,
            "byte_order": case.order.native(),
            "data_hex": hex(case.data),
            "dir_start": case.dir_start,
            "dir_len": case.dir_len,
            "unknown": if case.unknown { 1 } else { 0 },
            "verbose": 1,
            "members": members,
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
    case: &ReplayCase<'_>,
) -> (
    Vec<Emitted>,
    oxidex::exiftool_tables::SerialWalkResult,
    HashMap<&'static str, MemberValue>,
) {
    let mut sink = Sink {
        unknown: case.unknown,
        ..Sink::default()
    };
    let mut members: HashMap<&'static str, MemberValue> = HashMap::new();
    for member in case.members {
        member.insert_rust(&mut members);
    }
    let result = process_serial_directory(
        table,
        SerialDir {
            data: case.data,
            dir_start: case.dir_start,
            dir_len: case.dir_len,
            base: 0,
            data_pos: 0,
            byte_order: case.order.rust(),
        },
        &mut Ctx::new(&mut members),
        &mut sink,
    );
    (sink.rows, result, members)
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

fn emitted_value(rows: &[Emitted], name: &str) -> String {
    let row = rows
        .iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("missing emitted {name}"));
    row_value(&row.value)
}

fn emitted_count(rows: &[Emitted], name: &str) -> usize {
    rows.iter().filter(|row| row.name == name).count()
}

fn native_read(reply: &Value, serial_index: usize) -> (&str, i64, i64, &str) {
    let read = reply["read_values"]
        .as_array()
        .expect("native read_values array")
        .get(serial_index)
        .unwrap_or_else(|| panic!("missing native ReadValue at serial index {serial_index}"));
    (
        read["format"]["string"].as_str().expect("native format"),
        read["offset"]["numeric"].as_i64().expect("native offset"),
        read["count"]["numeric"].as_i64().expect("native count"),
        read["value"]["string"].as_str().expect("native raw value"),
    )
}

fn native_indices(reply: &Value, key: &str) -> Vec<i64> {
    reply[key]
        .as_array()
        .unwrap_or_else(|| panic!("native {key} array"))
        .iter()
        .map(|item| item["index"]["numeric"].as_i64().expect("native index"))
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

fn table(module: &str, name: &str) -> &'static SerialTable {
    let table = find_serial_table(module, name)
        .unwrap_or_else(|| panic!("{module}::{name} must be generated"));
    assert!(
        table.gate_a.passes(),
        "{module}::{name} gate A: {:?}",
        table.gate_a.blocked_by
    );
    table
}

fn assert_case(native: &NativeEnv, case: &ReplayCase<'_>) {
    let native_reply = native_replay(native, case);
    let table = table(case.module, case.table);
    let (rows, result, _) = rust_replay(table, case);
    assert!(
        !result.tainted,
        "{}: shared serial reader refused a native generated-table case: {result:?}",
        case.name
    );
    assert_eq!(
        rust_rows(&rows),
        native_rows(&native_reply),
        "{}: FoundTag input projection",
        case.name
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
            &ReplayCase {
                module: "Real",
                table: "AudioV3",
                name: "v3-normal",
                order,
                data: &v3,
                dir_start: 0,
                dir_len: v3.len(),
                unknown: false,
                members: &[],
            },
        );
        let v4 = real_v4(order);
        assert_case(
            &native,
            &ReplayCase {
                module: "Real",
                table: "AudioV4",
                name: "v4-normal",
                order,
                data: &v4,
                dir_start: 0,
                dir_len: v4.len(),
                unknown: false,
                members: &[],
            },
        );
    }

    let normal = real_v3(Order::Ii, b"Hi!", b"Me", b"C", b"Yo");
    for (name, unknown) in [("v3-unknown-off", false), ("v3-unknown-on", true)] {
        assert_case(
            &native,
            &ReplayCase {
                module: "Real",
                table: "AudioV3",
                name,
                order: Order::Ii,
                data: &normal,
                dir_start: 0,
                dir_len: normal.len(),
                unknown,
                members: &[],
            },
        );
    }

    let zero = real_v3(Order::Ii, b"", b"", b"", b"");
    let nul = real_v3(Order::Ii, b"H\0!", b"Me", b"C", b"Yo");
    let truncated = &normal[..15]; // title count remains 3 but no title byte fits.
    for (name, data) in [
        ("v3-zero-strings", zero.as_slice()),
        ("v3-nul-string", nul.as_slice()),
        ("v3-truncated-string", truncated),
    ] {
        assert_case(
            &native,
            &ReplayCase {
                module: "Real",
                table: "AudioV3",
                name,
                order: Order::Ii,
                data,
                dir_start: 0,
                dir_len: data.len(),
                unknown: false,
                members: &[],
            },
        );
    }

    let mut bounded = vec![0xa5, 0x5a, 0xff];
    bounded.extend_from_slice(&normal);
    assert_case(
        &native,
        &ReplayCase {
            module: "Real",
            table: "AudioV3",
            name: "v3-bounded-nonzero-offset",
            order: Order::Ii,
            data: &bounded,
            dir_start: 3,
            dir_len: normal.len(),
            unknown: false,
            members: &[],
        },
    );
}

#[test]
#[ignore = "requires EXIFTOOL_PERL v5.38.2 and OXIDEX_PINNED_EXIFTOOL; run explicitly"]
fn canon_afinfo_generated_tables_replay_pinned_native_contract() {
    // Contract: shared-pilot/serial-afinfo-integration-20260913/
    // rust-replay-contract-20260913/serial-afinfo-rust-replay-contract.json.
    // The probe observes raw FoundTag input/cursor facts. Final DecodeBits,
    // IntEnum, and RawConv state assertions below use the paired direct-native
    // contract rather than treating callback interception as final rendering.
    let native = NativeEnv::require();
    let focus_words = [1_u16, 0x8000];
    let points = vec![0x8000, 0xffff, 0x7fff]
        .into_iter()
        .cycle()
        .take(17)
        .collect::<Vec<_>>();

    for order in [Order::Ii, Order::Mm] {
        let payload = afinfo(order, 17, &points, &points, &focus_words, &[3, 4]);
        let case = ReplayCase {
            module: "Canon",
            table: "AFInfo",
            name: "afinfo-signed-multiword",
            order,
            data: &payload,
            dir_start: 0,
            dir_len: payload.len(),
            unknown: false,
            members: &[],
        };
        let reply = native_replay(&native, &case);
        let table = table(case.module, case.table);
        let (rows, result, _) = rust_replay(table, &case);
        assert!(!result.tainted, "{order:?}: {result:?}");
        assert_eq!(
            native_indices(&reply, "get_tag_info"),
            (0..=12).collect::<Vec<_>>()
        );
        assert_eq!(native_read(&reply, 10), ("int16s", 84, 2, "1 -32768"));
        assert_eq!(emitted_value(&rows, "AFPointsInFocus"), "0,31");
        assert_eq!(
            emitted_value(&rows, "AFAreaXPositions"),
            "-32768 -1 32767 -32768 -1 32767 -32768 -1 32767 -32768 -1 32767 -32768 -1 32767 -32768 -1"
        );
        assert_source_groups(table, &reply);
    }

    let zero = afinfo(Order::Ii, 0, &[], &[], &[], &[1, 3]);
    let zero_case = ReplayCase {
        module: "Canon",
        table: "AFInfo",
        name: "afinfo-zero-count",
        order: Order::Ii,
        data: &zero,
        dir_start: 0,
        dir_len: zero.len(),
        unknown: false,
        members: &[],
    };
    let zero_reply = native_replay(&native, &zero_case);
    let (zero_rows, zero_result, _) = rust_replay(table("Canon", "AFInfo"), &zero_case);
    assert!(!zero_result.tainted, "{zero_result:?}");
    assert_eq!(native_read(&zero_reply, 8), ("int16s", 16, 0, ""));
    assert!(!zero_rows.iter().any(|row| row.name == "AFAreaXPositions"));
    assert!(!zero_rows.iter().any(|row| row.name == "AFPointsInFocus"));

    let mut truncated_bytes = afinfo(Order::Ii, 1, &[0x8000], &[0x7fff], &[1], &[3, 4]);
    truncated_bytes.pop(); // Keep the first PrimaryAFPoint but deny the second.
    let truncated = ReplayCase {
        name: "afinfo-truncated-final",
        data: &truncated_bytes,
        dir_len: truncated_bytes.len(),
        ..zero_case
    };
    let truncated_reply = native_replay(&native, &truncated);
    let (truncated_rows, truncated_result, _) = rust_replay(table("Canon", "AFInfo"), &truncated);
    assert!(!truncated_result.tainted, "{truncated_result:?}");
    assert_eq!(
        native_indices(&truncated_reply, "get_tag_info"),
        (0..=12).collect::<Vec<_>>()
    );
    assert_eq!(native_read(&truncated_reply, 11), ("int16u", 22, 1, "3"));
    assert_eq!(emitted_count(&truncated_rows, "PrimaryAFPoint"), 1);

    let one = afinfo(
        Order::Ii,
        1,
        &[0xfff9],
        &[9],
        &[1],
        &[101, 102, 103, 104, 105, 106, 107, 108, 9],
    );
    let hidden = ReplayCase {
        module: "Canon",
        table: "AFInfo",
        name: "afinfo-hidden-unknown-consumes-eight",
        order: Order::Ii,
        data: &one,
        dir_start: 0,
        dir_len: one.len(),
        unknown: false,
        members: &[
            MemberSeed::Text("Model", "PowerShot G7"),
            MemberSeed::Number("AFInfoCount", 36),
        ],
    };
    let hidden_reply = native_replay(&native, &hidden);
    let (hidden_rows, hidden_result, _) = rust_replay(table("Canon", "AFInfo"), &hidden);
    assert!(!hidden_result.tainted, "{hidden_result:?}");
    assert_eq!(
        native_read(&hidden_reply, 11),
        ("int16u", 22, 8, "101 102 103 104 105 106 107 108")
    );
    assert_eq!(native_read(&hidden_reply, 12), ("int16u", 38, 1, "9"));
    assert!(!hidden_rows
        .iter()
        .any(|row| row.name == "Canon_AFInfo_0x000b"));
    assert_eq!(emitted_value(&hidden_rows, "PrimaryAFPoint"), "9");

    let powershot = ReplayCase {
        name: "afinfo-powershot-no-count",
        members: &[MemberSeed::Text("Model", "PowerShot G7")],
        ..zero_case
    };
    let powershot_reply = native_replay(&native, &powershot);
    let (powershot_rows, powershot_result, _) = rust_replay(table("Canon", "AFInfo"), &powershot);
    assert!(!powershot_result.tainted, "{powershot_result:?}");
    assert_eq!(
        native_indices(&powershot_reply, "get_tag_info"),
        (0..=12).collect::<Vec<_>>()
    );
    assert_eq!(emitted_count(&powershot_rows, "PrimaryAFPoint"), 2);

    let visible = ReplayCase {
        unknown: true,
        name: "afinfo-visible-unknown",
        ..hidden
    };
    let visible_reply = native_replay(&native, &visible);
    let (visible_rows, visible_result, _) = rust_replay(table("Canon", "AFInfo"), &visible);
    assert!(!visible_result.tainted, "{visible_result:?}");
    assert!(native_rows(&visible_reply)
        .iter()
        .any(|(name, _)| name == "Canon_AFInfo_0x000b"));
    assert!(visible_rows
        .iter()
        .any(|row| row.name == "Canon_AFInfo_0x000b"));

    let eos = ReplayCase {
        name: "afinfo-eos-stops-at-eleven",
        members: &[MemberSeed::Text("Model", "EOS R7")],
        ..zero_case
    };
    let eos_reply = native_replay(&native, &eos);
    let (eos_rows, eos_result, _) = rust_replay(table("Canon", "AFInfo"), &eos);
    assert_eq!(
        native_indices(&eos_reply, "get_tag_info"),
        (0..=11).collect::<Vec<_>>()
    );
    assert_eq!(eos_result.no_matching_alternative, 1);
    assert!(!eos_rows.iter().any(|row| row.name == "PrimaryAFPoint"));
}

#[test]
#[ignore = "requires EXIFTOOL_PERL v5.38.2 and OXIDEX_PINNED_EXIFTOOL; run explicitly"]
fn canon_afinfo2_generated_table_replays_state_enum_and_trailing_count() {
    let native = NativeEnv::require();
    let one = [0xfff9_u16];
    let focus = [1_u16];
    let payload = afinfo2(Order::Ii, 1, &one, &focus, &[101, 42, 77]);
    let powershot = ReplayCase {
        module: "Canon",
        table: "AFInfo2",
        name: "afinfo2-powershot-trailing-add",
        order: Order::Ii,
        data: &payload,
        dir_start: 0,
        dir_len: payload.len(),
        unknown: true,
        members: &[MemberSeed::Text("Model", "PowerShot G7")],
    };
    let reply = native_replay(&native, &powershot);
    let table = table("Canon", "AFInfo2");
    let (rows, result, members) = rust_replay(table, &powershot);
    assert!(!result.tainted, "{result:?}");
    assert_eq!(
        native_indices(&reply, "get_tag_info"),
        (0..=14).collect::<Vec<_>>()
    );
    assert_eq!(native_read(&reply, 13), ("int16s", 26, 2, "101 42"));
    assert_eq!(native_read(&reply, 14), ("int16u", 30, 1, "77"));
    assert_eq!(emitted_value(&rows, "AFAreaMode"), "Single-point AF");
    assert_eq!(emitted_value(&rows, "AFPointsInFocus"), "0");
    assert_eq!(emitted_value(&rows, "Canon_AFInfo2_0x000d"), "101 42");
    assert_eq!(emitted_value(&rows, "PrimaryAFPoint"), "77");
    assert_eq!(members.get("NumAFPoints"), Some(&MemberValue::Num(1)));
    assert_source_groups(table, &reply);

    let unknown_mode = afinfo2(Order::Mm, 1, &one, &focus, &[101, 42, 77]);
    let unknown = ReplayCase {
        name: "afinfo2-mm-enum-miss",
        order: Order::Mm,
        data: &unknown_mode,
        ..powershot
    };
    // Native table data owns the enum map. Change only its raw slot-one word
    // so the pinned native miss and generated shared IntEnum must both render
    // ExifTool's exact unknown spelling.
    let mut unknown_bytes = unknown.data.to_vec();
    unknown_bytes[2..4].copy_from_slice(&3_u16.to_be_bytes());
    let unknown = ReplayCase {
        data: &unknown_bytes,
        ..unknown
    };
    let unknown_reply = native_replay(&native, &unknown);
    let (unknown_rows, unknown_result, _) = rust_replay(table, &unknown);
    assert!(!unknown_result.tainted, "{unknown_result:?}");
    assert_eq!(native_read(&unknown_reply, 1).3, "3");
    assert_eq!(emitted_value(&unknown_rows, "AFAreaMode"), "Unknown (3)");

    let absent_afinfo3 = ReplayCase {
        name: "afinfo2-afinfo3-absent",
        ..powershot
    };
    let (_, absent_result, _) = rust_replay(table, &absent_afinfo3);
    assert_eq!(absent_result.no_matching_alternative, 0);
    let present_afinfo3 = ReplayCase {
        name: "afinfo2-afinfo3-present-stops",
        members: &[
            MemberSeed::Text("Model", "PowerShot G7"),
            MemberSeed::Number("AFInfo3", 1),
        ],
        ..powershot
    };
    let present_reply = native_replay(&native, &present_afinfo3);
    let (present_rows, present_result, _) = rust_replay(table, &present_afinfo3);
    assert_eq!(
        native_indices(&present_reply, "get_tag_info"),
        (0..=14).collect::<Vec<_>>()
    );
    assert_eq!(present_result.no_matching_alternative, 1);
    assert!(!present_rows.iter().any(|row| row.name == "PrimaryAFPoint"));

    let eos = ReplayCase {
        name: "afinfo2-eos-selects-af-points",
        members: &[MemberSeed::Text("Model", "EOS R7")],
        ..powershot
    };
    let eos_reply = native_replay(&native, &eos);
    let (eos_rows, eos_result, _) = rust_replay(table, &eos);
    assert_eq!(
        native_indices(&eos_reply, "get_tag_info"),
        (0..=14).collect::<Vec<_>>()
    );
    assert_eq!(eos_result.no_matching_alternative, 1);
    assert_eq!(emitted_value(&eos_rows, "AFPointsSelected"), "0");
    assert!(!eos_rows.iter().any(|row| row.name == "PrimaryAFPoint"));
}
