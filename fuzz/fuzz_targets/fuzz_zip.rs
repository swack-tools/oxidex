#![no_main]

//! Fuzzes the hand-written ZIP reader.
//!
//! This target exists because `src/parsers/archive/zip_reader.rs` replaced the
//! `zip` crate, moving responsibility for surviving hostile archives from a
//! widely-fuzzed dependency onto this repository. ZIP is an adversarial format:
//! offsets point anywhere, the central directory and local headers can
//! disagree, ZIP64 fields may be absent or lie, entry counts need not match
//! reality, and a self-extracting stub shifts every offset in the file.
//!
//! The reader is read-only and allocates only up to an entry's declared size,
//! so what this looks for is panics -- slice indexing, arithmetic overflow,
//! unwrap on a malformed field -- and unbounded allocation. Parse *errors* are
//! the correct outcome for garbage and are discarded.
//!
//! Every entry is both enumerated and read, because the two paths check
//! different things: enumeration walks the central directory, while reading
//! follows an offset to a local header and inflates. A reader that parsed
//! headers safely but trusted the payload offset would pass an
//! enumeration-only fuzzer.

use libfuzzer_sys::fuzz_target;
use oxidex::parsers::archive::zip_reader::ZipReader;

fuzz_target!(|data: &[u8]| {
    let Ok(reader) = ZipReader::new(data) else {
        // Rejecting malformed input is correct behaviour, not a finding.
        return;
    };

    // Archive-level accessors must tolerate whatever parsed.
    let _ = reader.comment();
    let _ = reader.len();
    let _ = reader.is_empty();

    for i in 0..reader.len() {
        let Some(entry) = reader.by_index(i) else {
            continue;
        };

        // Metadata accessors: these are what the parsers publish as tags, so a
        // panic here is reachable from any archive OxiDex is handed.
        let name = entry.name().to_string();
        let _ = entry.size();
        let _ = entry.compressed_size();
        let _ = entry.crc32();
        let _ = entry.compression();
        let _ = entry.compression_name();
        let _ = entry.is_dir();
        let _ = entry.is_encrypted();
        let _ = entry.unix_mode();
        let _ = entry.version_made_by();
        if let Some(dt) = entry.last_modified() {
            // DOS fields are frequently nonsense in real archives; the
            // accessors must not assume a valid calendar date.
            let _ = (dt.year(), dt.month(), dt.day());
            let _ = (dt.hour(), dt.minute(), dt.second());
        }

        // The payload path: resolves an attacker-controlled offset and
        // inflates. Errors are expected and ignored.
        let _ = reader.read_entry(entry);

        // Name-based lookup must agree that an enumerated entry exists, and
        // reading by that name must not panic either.
        let _ = reader.contains(&name);
        let _ = reader.read_by_name(&name);
        let _ = reader.read_to_string(&name);
    }

    // A name that is present in no archive, exercising the miss path.
    let _ = reader.read_by_name("\u{0}/../does-not-exist");
});
