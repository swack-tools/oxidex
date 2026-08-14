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

#[path = "integration/pe_comparison.rs"]
mod pe_comparison;

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

#[path = "integration/infiray_tests.rs"]
mod infiray_tests;

#[path = "integration/samsung_app5_tests.rs"]
mod samsung_app5_tests;

#[path = "integration/dji_app4_tests.rs"]
mod dji_app4_tests;

#[path = "integration/exif_makernotes_tests.rs"]
mod exif_makernotes_tests;

#[path = "integration/canon_real_image_test.rs"]
mod canon_real_image_test;

#[path = "integration/canon_makernotes_phase3_tests.rs"]
mod canon_makernotes_phase3_tests;

#[path = "integration/nikon_makernotes_tests.rs"]
mod nikon_makernotes_tests;

#[path = "integration/sony_makernotes_tests.rs"]
mod sony_makernotes_tests;

#[path = "integration/fujifilm_makernotes_tests.rs"]
mod fujifilm_makernotes_tests;

#[path = "integration/panasonic_makernotes_tests.rs"]
mod panasonic_makernotes_tests;

#[path = "integration/olympus_makernotes_tests.rs"]
mod olympus_makernotes_tests;

#[path = "integration/pentax_makernotes_tests.rs"]
mod pentax_makernotes_tests;

#[path = "integration/leica_makernotes_tests.rs"]
mod leica_makernotes_tests;

#[path = "integration/sigma_makernotes_tests.rs"]
mod sigma_makernotes_tests;

#[path = "integration/phaseone_makernotes_tests.rs"]
mod phaseone_makernotes_tests;

// This one was never declared, so `tests/integration/apple_makernotes_tests.rs`
// never compiled and never ran -- which is how it kept asserting an
// `Apple:PortraitMode` at 0x0020, an `Apple:LensModel` at 0x0035 and an
// `Apple:FacingCamera` at 0x0032, none of which is a tag `%Apple::Main` has.
#[path = "integration/apple_makernotes_tests.rs"]
mod apple_makernotes_tests;

#[path = "integration/format_detection.rs"]
mod format_detection;

#[path = "integration/production_wiring_tests.rs"]
mod production_wiring_tests;

#[path = "integration/pe_tests.rs"]
mod pe_tests;

// Magika AI-powered detection tests (feature-gated)
#[cfg(feature = "magika")]
#[path = "integration/magika_detection_tests.rs"]
mod magika_detection_tests;

#[path = "integration/pe_import_test.rs"]
mod pe_import_test;

#[path = "integration/makernote_integration.rs"]
mod makernote_integration;

#[path = "integration/cli_feature_tests.rs"]
mod cli_feature_tests;

#[path = "integration/cli_batch_wiring_tests.rs"]
mod cli_batch_wiring_tests;

#[path = "integration/cli_batch_equivalence_tests.rs"]
mod cli_batch_equivalence_tests;

#[path = "integration/cli_typed_value_tests.rs"]
mod cli_typed_value_tests;

#[path = "integration/exif_tag_id_collision_tests.rs"]
mod exif_tag_id_collision_tests;

#[path = "integration/error_handling_tests.rs"]
mod error_handling_tests;

// Container / audio format integration tests. These were previously declared
// only in tests/integration/mod.rs, which no Cargo test root ever included, so
// they never compiled. Declare them here like every other integration module.
#[path = "integration/mkv_integration_tests.rs"]
mod mkv_integration_tests;

#[path = "integration/webm_integration_tests.rs"]
mod webm_integration_tests;

#[path = "integration/flv_integration_tests.rs"]
mod flv_integration_tests;

#[path = "integration/avi_integration_tests.rs"]
mod avi_integration_tests;

#[path = "integration/mts_integration_tests.rs"]
mod mts_integration_tests;

#[path = "integration/mp3_integration_tests.rs"]
mod mp3_integration_tests;

#[path = "integration/flac_integration_tests.rs"]
mod flac_integration_tests;

#[path = "integration/aac_integration_tests.rs"]
mod aac_integration_tests;

#[path = "integration/wav_integration_tests.rs"]
mod wav_integration_tests;

#[path = "integration/ogg_integration_tests.rs"]
mod ogg_integration_tests;

#[path = "integration/opus_integration_tests.rs"]
mod opus_integration_tests;

#[path = "integration/ape_integration_tests.rs"]
mod ape_integration_tests;

#[path = "integration/iso_integration_tests.rs"]
mod iso_integration_tests;

#[path = "integration/ram_integration_tests.rs"]
mod ram_integration_tests;

#[path = "integration/dss_integration_tests.rs"]
mod dss_integration_tests;

#[path = "integration/moi_integration_tests.rs"]
mod moi_integration_tests;

#[path = "integration/pcx_integration_tests.rs"]
mod pcx_integration_tests;

#[path = "integration/itc_integration_tests.rs"]
mod itc_integration_tests;

#[path = "integration/pgf_integration_tests.rs"]
mod pgf_integration_tests;

#[path = "integration/mrc_integration_tests.rs"]
mod mrc_integration_tests;

#[path = "integration/red_integration_tests.rs"]
mod red_integration_tests;

#[path = "integration/detected_not_parsed_tests.rs"]
mod detected_not_parsed_tests;

#[path = "integration/aa_integration_tests.rs"]
mod aa_integration_tests;

#[path = "integration/wpg_integration_tests.rs"]
mod wpg_integration_tests;

#[path = "integration/jxl_json_tests.rs"]
mod jxl_json_tests;

#[path = "integration/recursive_extension_coverage.rs"]
mod recursive_extension_coverage;

// No qualcomm/google/microsoft MakerNote test modules: those three suites were
// deleted rather than declared. Every tag name they asserted appears in zero
// ExifTool 13.59 source files, so declaring them would have pinned invented
// data as expected behaviour. See the commit that removed them for the
// name-by-name evidence.

#[path = "forensic/mod.rs"]
mod forensic;

#[path = "integration/detected_not_parsed_routing_tests.rs"]
mod detected_not_parsed_routing_tests;
