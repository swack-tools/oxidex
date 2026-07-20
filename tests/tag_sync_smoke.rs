//! End-to-end smoke test for the exiftool-listx-based tag sync pipeline.
//!
//! Skipped (not failed) when `exiftool` isn't on PATH, matching how other
//! exiftool-comparison tests in this repo handle the optional dependency.

use oxidex::tag_sync::{DOMAINS, generate_domain_yaml, parse_listx};
use std::process::Command;

fn exiftool_available() -> bool {
    Command::new("exiftool")
        .arg("-ver")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn real_exiftool_listx_parses_and_beats_the_current_type_coverage_baseline() {
    if !exiftool_available() {
        eprintln!("skipping: exiftool not found on PATH");
        return;
    }

    let output = Command::new("exiftool")
        .args(["-f", "-listx"])
        .output()
        .expect("failed to run exiftool -f -listx");
    assert!(output.status.success(), "exiftool -f -listx must succeed");

    let xml = String::from_utf8(output.stdout).expect("exiftool output must be valid UTF-8");
    let tags = parse_listx(&xml).expect("real exiftool -listx output must parse");

    // Baseline from the committed YAML as of 2026-07-19: 32,684 tags, 366
    // (1.1%) with a populated `type` field. The new pipeline must clear
    // both bars by a wide margin, since ExifTool resolves every tag's
    // writable/type attributes before emitting -listx.
    //
    // The exact tag count varies by installed exiftool version -- this
    // repo's CI installs whatever Ubuntu's apt repos currently carry
    // (libimage-exiftool-perl), which lags well behind a pinned recent
    // release (confirmed: 27,864 tags on CI vs 32,684+ on a current
    // 13.55 install) and will drift further as apt's package updates.
    // 20,000 is a floor comfortably below any realistic exiftool version
    // this test might run against, while still far too high for anything
    // but a genuinely healthy parse of a real -listx dump.
    assert!(
        tags.len() > 20_000,
        "expected >20,000 tags from a real exiftool -listx dump, got {}",
        tags.len()
    );

    let typed = tags.iter().filter(|t| t.type_name.is_some()).count();
    let typed_fraction = typed as f64 / tags.len() as f64;
    assert!(
        typed_fraction > 0.5,
        "expected over 50% of tags to carry a type (old pipeline: 1.1%), got {:.1}%",
        typed_fraction * 100.0
    );

    // Every domain must route at least one tag for a full parse.
    for domain in DOMAINS {
        let yaml = generate_domain_yaml(domain, &tags);
        assert!(
            yaml.lines().count() > 1,
            "domain '{domain}' produced no tags from a real exiftool dump"
        );
    }
}
