#![no_main]

use libfuzzer_sys::fuzz_target;
use exiftool_rs::parsers::pdf::parse_pdf_metadata;
use exiftool_rs::core::FileReader;
use std::io;

/// In-memory FileReader implementation for fuzzing.
/// Uses saturating arithmetic to prevent integer overflow panics.
struct FuzzReader {
    data: Vec<u8>,
}

impl FileReader for FuzzReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;

        // Use saturating arithmetic to prevent integer overflow panics
        let end = start.saturating_add(length).min(self.data.len());

        // Check if start is beyond data length
        if start >= self.data.len() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "offset beyond end of data"
            ));
        }

        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

fuzz_target!(|data: &[u8]| {
    // Create a fuzzing reader from the input data
    let reader = FuzzReader {
        data: data.to_vec(),
    };

    // Attempt to parse PDF metadata
    // We discard errors - we're looking for crashes/panics, not parse errors
    let _ = parse_pdf_metadata(&reader);
});
