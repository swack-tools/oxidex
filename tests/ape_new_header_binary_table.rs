//! Step 28 gate B regression: the MAC 3.98+ audio header is read through
//! `Image::ExifTool::APE::NewHeader` (APE.pm:65-78), not through a second
//! hand-written copy of its byte offsets.
//!
//! # Why a real carrier and not a builder
//!
//! `src/parsers/audio/ape.rs`'s own unit tests construct a MAC descriptor
//! with `build_ape`, which is fine for the tag-item walker but cannot observe
//! a layout defect: the bytes are written at the offsets the test author
//! believes in, so a decode that reads the wrong offset in the same wrong way
//! passes. The expectations below are the pinned ExifTool 13.59 oracle's own
//! output for `t/images/APE.ape`, a file neither this repository nor this
//! test wrote:
//!
//! ```text
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -G1 -MAC:all APE.ape
//! [MAC]  Compression Level  : 3000
//! [MAC]  Blocks Per Frame   : 73728
//! [MAC]  Final Frame Blocks : 42662
//! [MAC]  Total Frames       : 2
//! [MAC]  Bits Per Sample    : 16
//! [MAC]  Channels           : 2
//! [MAC]  Sample Rate        : 44100
//! $ /tmp/oxidex-exiftool-cache/exiftool-pinned.sh -G1 -Composite:Duration APE.ape
//! [Composite] Duration      : 2.64 s
//! ```
//!
//! `APE.ape` reports version 3990 in its descriptor, so it exercises
//! `NewHeader` only -- `APE::OldHeader` has no carrier anywhere in the 4,238
//! file corpus, which is exactly why it is wired but NOT on the gate B
//! allowlist (see `src/exiftool_tables/enabled.rs`).

use oxidex::exiftool_tables::find_table;
use oxidex::io::buffered_reader::BufferedReader;
use oxidex::parsers::audio::ape::parse_ape_metadata;
use std::path::Path;

const APE_FIXTURE: &str = "/tmp/oxidex-exiftool-cache/exiftool/t/images/APE.ape";

/// The allowlist line is the reviewable unit; this asserts the line is
/// actually in force, so a revert of it fails loudly here rather than
/// silently changing which `ReadValue` the record goes through.
#[test]
fn ape_new_header_is_on_the_step_28_allowlist() {
    let table = find_table("APE", "NewHeader").expect("APE::NewHeader is generated");
    assert!(
        table.gate_a.passes(),
        "gate A must pass: {:?}",
        table.gate_a.blocked_by
    );
    assert!(
        table.enabled(),
        "APE::NewHeader must be on the gate B allowlist"
    );
}

/// `APE::OldHeader` passes gate A and is reached by `parse_old_header`, but
/// the corpus carries no 3.97-or-earlier file, so nothing can measure it.
/// Pinning that keeps a future "enable it too, it looks fine" from being a
/// silent change.
#[test]
fn ape_old_header_is_reached_but_deliberately_unmeasured() {
    let table = find_table("APE", "OldHeader").expect("APE::OldHeader is generated");
    assert!(
        table.gate_a.passes(),
        "gate A passes: {:?}",
        table.gate_a.blocked_by
    );
    assert!(
        !table.enabled(),
        "APE::OldHeader has no corpus carrier, so it must not be allowlisted"
    );
}

#[test]
#[ignore = "requires the pinned ExifTool fixture cache"]
fn reads_every_mac_new_header_field_the_pinned_oracle_reports() {
    let reader = BufferedReader::new(Path::new(APE_FIXTURE)).expect("open APE fixture");
    let metadata = parse_ape_metadata(&reader).expect("parse APE fixture");

    // The seven `APE::NewHeader` keys, in the Perl's own order.
    assert_eq!(metadata.get_integer("APE:CompressionLevel"), Some(3000));
    assert_eq!(metadata.get_integer("APE:BlocksPerFrame"), Some(73728));
    assert_eq!(metadata.get_integer("APE:FinalFrameBlocks"), Some(42662));
    assert_eq!(metadata.get_integer("APE:TotalFrames"), Some(2));
    assert_eq!(metadata.get_integer("APE:BitsPerSample"), Some(16));
    assert_eq!(metadata.get_integer("APE:Channels"), Some(2));
    assert_eq!(metadata.get_integer("APE:SampleRate"), Some(44100));

    // `APE::Composite::Duration` (APE.pm:81-93) is a Composite, not a field
    // of the binary table, so the fold must NOT have taken it with the
    // offsets it deleted.
    assert_eq!(metadata.get_string("APE:Duration"), Some("2.64 s"));

    // APE.pm:71 comments FormatFlags out at key 1: not a tag ExifTool emits,
    // so not one the table walk may invent either.
    assert!(!metadata.contains_key("APE:FormatFlags"));
}
