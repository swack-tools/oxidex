//! The proof of the generated arms: every probe `conv_oracle.py` ran through
//! the pinned ExifTool's own `FoundTag`/`GetValue` is replayed through the
//! arm, and the bytes must agree.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{Arm, Out, exif_main};
use crate::exiftool_tables::session::{ByteOrder, MemberVal, Session};

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

/// A probe as the walker would hand it to an arm. `None` for bytes that are
/// not UTF-8: the walker declines those before calling (`MemberVal::Str`
/// cannot carry them), so there is nothing to compare.
fn arg(v: &Value) -> Option<MemberVal> {
    Some(match v["t"].as_str().expect("type") {
        "undef" => MemberVal::Undef,
        "s" => MemberVal::Str(String::from_utf8(hex(v["hex"].as_str().expect("hex"))).ok()?),
        "i" => MemberVal::Int(v["v"].as_str().expect("v").parse().expect("int")),
        "f" => MemberVal::Float(v["v"].as_str().expect("v").parse().expect("float")),
        t => panic!("arg type {t}"),
    })
}

/// One ExifTool output: `undef`, a scalar's bytes, a SCALAR ref's bytes.
#[derive(Debug, PartialEq, Eq)]
enum Seen {
    Undef,
    Scalar(Vec<u8>),
    Ref(Vec<u8>),
    Other(String),
}

fn perl(o: &Value) -> Seen {
    match o["t"].as_str().expect("t") {
        "undef" => Seen::Undef,
        "s" => Seen::Scalar(hex(o["hex"].as_str().expect("hex"))),
        "ref" => Seen::Ref(hex(o["hex"].as_str().expect("hex"))),
        _ => Seen::Other(o["ref"].to_string()),
    }
}

fn scalar(v: &MemberVal) -> Seen {
    if v.is_defined() {
        Seen::Scalar(v.perl_string().into_bytes())
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
        s.set_member(k, arg(v).expect("member probe is UTF-8"))
            .expect("member type");
    }
    s
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
    let (mut checked, mut matched, mut unrepresentable) = (0usize, 0usize, 0usize);
    let mut declined: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut failures: Vec<String> = Vec::new();
    for (fid, field) in fields {
        let id = u16::from_str_radix(fid.trim_start_matches("0x"), 16).expect("id");
        for case in field["cases"].as_array().expect("cases") {
            checked += 1;
            let Some(val) = arg(&case["val"]) else {
                unrepresentable += 1;
                continue;
            };
            let s = session(case);
            let fail = |why: String| format!("{fid} {} val {}: {why}", field["name"], case["val"]);
            match exif_main::decode(&s, id, &val) {
                Arm::Decline(why) => *declined.entry(why).or_default() += 1,
                Arm::Suppress => {
                    let perl_suppressed = case.get("suppressed").is_some()
                        || (perl(&case["vc"]) == Seen::Undef && perl(&case["pc"]) == Seen::Undef);
                    if perl_suppressed {
                        matched += 1;
                    } else {
                        failures.push(fail(format!(
                            "arm suppressed; perl vc {:?} pc {:?}",
                            perl(&case["vc"]),
                            perl(&case["pc"])
                        )));
                    }
                }
                Arm::Report(r) => {
                    if case.get("suppressed").is_some() {
                        failures.push(fail("arm reported; perl suppressed".into()));
                        continue;
                    }
                    let vc = r.value.as_ref().map_or_else(|| scalar(&val), rust);
                    let pc = r.print.as_ref().map_or_else(
                        || r.value.as_ref().map_or_else(|| scalar(&val), rust),
                        rust,
                    );
                    let mut writes: BTreeMap<String, Seen> = BTreeMap::new();
                    for (k, v) in &r.writes {
                        writes.insert((*k).to_string(), scalar(v));
                    }
                    let want_writes: BTreeMap<String, Seen> = case["set"]
                        .as_object()
                        .expect("set")
                        .iter()
                        .map(|(k, v)| (k.clone(), perl(v)))
                        .collect();
                    let (pv, pp) = (perl(&case["vc"]), perl(&case["pc"]));
                    if vc == pv && pc == pp && writes == want_writes {
                        matched += 1;
                    } else {
                        failures.push(fail(format!(
                            "perl vc {pv:?} pc {pp:?} set {want_writes:?}; \
                             rust vc {vc:?} pc {pc:?} set {writes:?}"
                        )));
                    }
                }
            }
        }
    }
    let n_declined: usize = declined.values().sum();
    eprintln!(
        "conv oracle: {checked} probes over {} arms: {matched} byte-identical, \
         {n_declined} declined {declined:?}, {unrepresentable} non-UTF-8 probes the walker \
         declines before calling, {} disagreements",
        fields.len(),
        failures.len()
    );
    assert!(
        failures.is_empty(),
        "{} disagreement(s) with the pinned Perl:\n{}",
        failures.len(),
        failures.iter().take(60).cloned().collect::<Vec<_>>().join("\n")
    );
    assert_eq!(checked, matched + n_declined + unrepresentable);
}

#[test]
fn claimed_and_refused_are_disjoint_and_sorted() {
    assert!(exif_main::CLAIMED.windows(2).all(|w| w[0] < w[1]));
    for (id, name, why) in exif_main::REFUSED {
        assert!(!exif_main::claims(*id), "0x{id:04x} {name} both claimed and refused");
        assert!(!why.is_empty());
    }
}
