//! Unit tests for audio parsers and document formats

#[path = "unit/audio/flac_tests.rs"]
mod flac_tests;

#[path = "unit/audio/mp3_tests.rs"]
mod mp3_tests;

#[path = "unit/format_detection/phase1_tests.rs"]
mod phase1_tests;

// Phase 2: Archive and document format tests
#[path = "unit/archive/zip_tests.rs"]
mod zip_tests;

#[path = "unit/document/ooxml_tests.rs"]
mod ooxml_tests;

#[path = "unit/document/iwork_tests.rs"]
mod iwork_tests;

#[path = "unit/document/epub_tests.rs"]
mod epub_tests;
