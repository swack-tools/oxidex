//! The proof of the generated arms: every probe `conv_oracle.py` ran through
//! the pinned ExifTool's own `FoundTag`/`GetValue` is replayed through the
//! arm, and the bytes must agree -- the `-n` value, the default value, every
//! member the pipeline created, changed or deleted, and every `Warn` request
//! it made.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{Arm, Entry, Out, claims_in, decoder_in, exif_main};
use crate::exiftool_tables::ifd_schema::{IfdFlags, IfdTable, IfdTag};
use crate::exiftool_tables::session::{ByteOrder, MemberVal, Session};
use crate::exiftool_tables::{GateA, Omitted, PrintConv, TagGroups};

const CAPTURE: &str =
    include_str!("../../../tools/exiftool-tables/testdata/conv_exif_main_outputs.json");

fn capture() -> Value {
    serde_json::from_str(CAPTURE).expect("capture is JSON")
}

fn hex(h: &str) -> Vec<u8> {
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).expect("hex"))
        .collect()
}

/// A probe as the walker hands it to an arm: a byte string stays bytes
/// (`MemberVal::Bytes` when it is not UTF-8).
fn arg(v: &Value) -> MemberVal {
    match v["t"].as_str().expect("type") {
        "undef" => MemberVal::Undef,
        "s" => MemberVal::from_bytes(hex(v["hex"].as_str().expect("hex"))),
        "i" => MemberVal::Int(v["v"].as_str().expect("v").parse().expect("int")),
        "f" => MemberVal::Float(v["v"].as_str().expect("v").parse().expect("float")),
        t => panic!("arg type {t}"),
    }
}

/// One ExifTool output: `undef`, a scalar's bytes, a SCALAR ref's bytes,
/// a deleted member. A Perl CHARACTER string (`u8`) holding a character
/// above 0x7F is its own kind: the runtime models byte strings only, so it
/// can never compare equal. One whose characters are all ASCII (Recompose's
/// `pack('C0U*')` of an empty list is a flagged `""`) is the same string as
/// the byte string in every context -- length, regex, comparison, print.
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    Undef,
    Scalar(Vec<u8>),
    Ref(Vec<u8>),
    Deleted,
    CharString(Vec<u8>),
    Other(String),
}

fn perl(o: &Value) -> Seen {
    let bytes = || hex(o["hex"].as_str().expect("hex"));
    if o.get("u8").is_some() && !bytes().is_ascii() {
        return Seen::CharString(bytes());
    }
    match o["t"].as_str().expect("t") {
        "undef" => Seen::Undef,
        "s" => Seen::Scalar(bytes()),
        "ref" => Seen::Ref(bytes()),
        "deleted" => Seen::Deleted,
        _ => Seen::Other(o["ref"].to_string()),
    }
}

fn scalar(v: &MemberVal) -> Seen {
    if v.is_defined() {
        Seen::Scalar(v.perl_bytes().into_owned())
    } else {
        Seen::Undef
    }
}

fn rust(o: &Out) -> Seen {
    match o {
        Out::Scalar(v) => scalar(v),
        Out::Binary(b) => Seen::Ref(b.clone()),
    }
}

fn session(case: &Value) -> Session {
    let mut s = Session::new();
    s.byte_order = Some(match case["byte_order"].as_str().expect("byte order") {
        "MM" => ByteOrder::BigEndian,
        _ => ByteOrder::LittleEndian,
    });
    for (k, v) in case["members"].as_object().expect("members") {
        s.set_member(k, arg(v)).expect("member type");
    }
    if let Some(opts) = case.get("options").and_then(Value::as_object) {
        for (k, v) in opts {
            s.set_option(k, arg(v));
        }
    }
    s
}

/// The members the arm's pipeline set: its session's changes (a helper's
/// `DecodeWarn...`, a deleted `WrongByteOrder`) and the `$$self{X} = ...`
/// writes it reports for its caller to apply.
fn members_set(
    before: &Session,
    after: &Session,
    writes: &[(&str, MemberVal)],
) -> BTreeMap<String, Seen> {
    let mut set: BTreeMap<String, Seen> = after
        .member_names()
        .filter(|k| !before.has_member(k) || before.member(k) != after.member(k))
        .map(|k| (k.to_string(), scalar(&after.member(k))))
        .collect();
    for k in before.member_names() {
        if !after.has_member(k) {
            set.insert(k.to_string(), Seen::Deleted);
        }
    }
    for (k, v) in writes {
        set.insert((*k).to_string(), scalar(v));
    }
    set
}

fn warnings_made(before: &Session, after: &Session) -> Vec<(Seen, i64)> {
    after.warnings()[before.warnings().len()..]
        .iter()
        .map(|w| (scalar(&w.message), w.ignorable))
        .collect()
}

fn perl_warnings(case: &Value) -> Vec<(Seen, i64)> {
    case.get("warnings")
        .and_then(Value::as_array)
        .map(|w| {
            w.iter()
                .map(|w| {
                    let ign = w
                        .get("ignorable")
                        .and_then(Value::as_str)
                        .map_or(0, |v| v.parse().expect("ignorable"));
                    (perl(w), ign)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn every_generated_arm_matches_the_pinned_perl_capture() {
    let cap = capture();
    assert_eq!(
        cap["capture"]["exiftool_version"].as_str(),
        Some(exif_main::EXIFTOOL_VERSION),
        "capture and arms come from different releases"
    );
    let fields = cap["fields"].as_object().expect("fields");
    let captured: Vec<u16> = fields
        .keys()
        .map(|k| u16::from_str_radix(k.trim_start_matches("0x"), 16).expect("id"))
        .collect();
    assert_eq!(
        captured,
        exif_main::CLAIMED,
        "the capture must cover exactly the generated arms (re-run conv_oracle.py --write)"
    );
    let (mut checked, mut matched, mut non_utf8) = (0usize, 0usize, 0usize);
    let mut declined: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    for (fid, field) in fields {
        let id = u16::from_str_radix(fid.trim_start_matches("0x"), 16).expect("id");
        for case in field["cases"].as_array().expect("cases") {
            checked += 1;
            let val = arg(&case["val"]);
            if matches!(val, MemberVal::Bytes(_)) {
                non_utf8 += 1;
            }
            let before = session(case);
            let mut s = before.clone();
            let fail = |why: String| {
                format!(
                    "{fid} {} val {} opts {}: {why}",
                    field["name"],
                    case["val"],
                    case.get("options").unwrap_or(&Value::Null)
                )
            };
            match exif_main::decode(&mut s, id, &val) {
                Arm::Decline(why) => *declined.entry(why).or_default() += 1,
                Arm::Suppress => {
                    // RawConv's undef (FoundTag stores nothing) or ValueConv's
                    // (GetValue returns nothing): either way no tag, and the
                    // side effects up to that point must agree.
                    let perl_suppressed = case.get("suppressed").is_some()
                        || (perl(&case["vc"]) == Seen::Undef && perl(&case["pc"]) == Seen::Undef);
                    let (warned, perl_warned) = (warnings_made(&before, &s), perl_warnings(case));
                    let set = members_set(&before, &s, &[]);
                    let want_set: BTreeMap<String, Seen> = case["set"]
                        .as_object()
                        .expect("set")
                        .iter()
                        .map(|(k, v)| (k.clone(), perl(v)))
                        .collect();
                    if perl_suppressed && perl_warned == warned && set == want_set {
                        matched += 1;
                    } else {
                        failures.push(fail(format!(
                            "arm suppressed; perl vc {:?} pc {:?} set {want_set:?} warn \
                             {perl_warned:?}; rust set {set:?} warn {warned:?}",
                            case.get("vc").map(perl),
                            case.get("pc").map(perl)
                        )));
                    }
                }
                Arm::Report(r) => {
                    if case.get("suppressed").is_some() {
                        failures.push(fail("arm reported; perl suppressed".into()));
                        continue;
                    }
                    let vc = r.value.as_ref().map_or_else(|| scalar(&val), rust);
                    let pc = r
                        .print
                        .as_ref()
                        .map_or_else(|| r.value.as_ref().map_or_else(|| scalar(&val), rust), rust);
                    let writes = members_set(&before, &s, &r.writes);
                    let want_writes: BTreeMap<String, Seen> = case["set"]
                        .as_object()
                        .expect("set")
                        .iter()
                        .map(|(k, v)| (k.clone(), perl(v)))
                        .collect();
                    let (warned, want_warned) = (warnings_made(&before, &s), perl_warnings(case));
                    let (pv, pp) = (perl(&case["vc"]), perl(&case["pc"]));
                    if vc == pv && pc == pp && writes == want_writes && warned == want_warned {
                        matched += 1;
                    } else {
                        failures.push(fail(format!(
                            "perl vc {pv:?} pc {pp:?} set {want_writes:?} warn {want_warned:?}; \
                             rust vc {vc:?} pc {pc:?} set {writes:?} warn {warned:?}"
                        )));
                    }
                }
            }
        }
    }
    let n_declined: usize = declined.values().sum();
    eprintln!(
        "conv oracle: {checked} probes over {} arms ({non_utf8} with non-UTF-8 byte input): \
         {matched} byte-identical, {n_declined} declined {declined:?}, {} disagreements",
        fields.len(),
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} disagreement(s) with the pinned Perl:\n{}",
        failures.len(),
        failures
            .iter()
            .take(60)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(checked, matched + n_declined);
}

#[test]
fn claimed_and_refused_are_disjoint_and_sorted() {
    assert!(exif_main::CLAIMED.windows(2).all(|w| w[0] < w[1]));
    for (id, name, why) in exif_main::REFUSED {
        assert!(
            !exif_main::claims(*id),
            "0x{id:04x} {name} both claimed and refused"
        );
        assert!(!why.is_empty());
    }
}

fn first_decode(_s: &mut Session, _id: u16, _val: &MemberVal) -> Arm {
    Arm::Decline("first")
}

fn second_decode(_s: &mut Session, _id: u16, _val: &MemberVal) -> Arm {
    Arm::Decline("second")
}

fn first_claims(id: u16) -> bool {
    id == 1
}

fn second_claims(id: u16) -> bool {
    id == 2
}

static SYNTHETIC_ENTRIES: &[Entry] = &[
    Entry {
        module: "First",
        table: "Main",
        decode: first_decode,
        claims: first_claims,
    },
    Entry {
        module: "Second",
        table: "Main",
        decode: second_decode,
        claims: second_claims,
    },
];

static SECOND_TAG: IfdTag = IfdTag {
    id: 2,
    name: "SecondTag",
    format: None,
    count: Some(1),
    writable: Some("int16u"),
    groups: TagGroups::NONE,
    flags: IfdFlags::NONE,
    condition: None,
    omitted: Omitted::NONE,
    raw_conv: None,
    value_conv: None,
    print_conv: PrintConv::None,
    subdir: None,
};

static SECOND_TABLE: IfdTable = IfdTable {
    module: "Second",
    table: "Main",
    group0: "Second",
    group1: "Second",
    group2: "Image",
    set_group1: None,
    priority: None,
    gate_a: GateA { blocked_by: &[] },
    tags: &[SECOND_TAG],
    variants: &[],
};

#[test]
fn multi_table_registry_dispatches_decoder_and_claims_together() {
    let decode = decoder_in(SYNTHETIC_ENTRIES, &SECOND_TABLE).expect("second decoder");
    let mut session = Session::new();
    assert_eq!(
        decode(&mut session, 2, &MemberVal::Int(0)),
        Arm::Decline("second")
    );
    assert!(claims_in(SYNTHETIC_ENTRIES, &SECOND_TABLE, &SECOND_TAG));
}
