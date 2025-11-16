// Integration tests module
// This file makes the tests/integration/ subdirectory visible to Cargo

#[path = "integration/jpeg_tests.rs"]
mod jpeg_tests;

#[path = "integration/jpeg_write_tests.rs"]
mod jpeg_write_tests;

#[path = "integration/operations_tests.rs"]
mod operations_tests;

#[path = "integration/pdf_tests.rs"]
mod pdf_tests;

#[path = "integration/pdf_write_tests.rs"]
mod pdf_write_tests;

#[path = "integration/png_tests.rs"]
mod png_tests;

#[path = "integration/png_write_tests.rs"]
mod png_write_tests;

#[path = "integration/tiff_tests.rs"]
mod tiff_tests;

#[path = "integration/tiff_write_tests.rs"]
mod tiff_write_tests;

#[path = "integration/write_operations_tests.rs"]
mod write_operations_tests;

#[path = "integration/exiftool_comparison_tests.rs"]
mod exiftool_comparison_tests;

#[path = "integration/mp4_tests.rs"]
mod mp4_tests;

#[path = "integration/copy_metadata_tests.rs"]
mod copy_metadata_tests;

#[path = "integration/rename_tests.rs"]
mod rename_tests;

#[path = "integration/date_shift_tests.rs"]
mod date_shift_tests;

#[path = "integration/iptc_integration_test.rs"]
mod iptc_integration_test;
