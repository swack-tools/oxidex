//! Every parser path whose `-a` order came from a `HashMap`, read repeatedly.
//!
//! `-a` output renders a file's occurrences in the order they were recorded
//! (`cli::tag_resolution` sorts by `TagOccurrence::order`). Wherever a parser
//! iterated a `HashMap` into the file's `MetadataMap` -- its own accumulator,
//! or a sub-map's winner projection before `TagSink` ordered it -- that order
//! was the map's, and std seeds every new `HashMap` afresh: text `-G1 -a -s`
//! differed between runs of one binary on 2,711 of the 4,238 corpus files.
//! Each read below builds new maps, so repeated reads in one process
//! exercise different iteration orders; the recorded sequence must not move.
//!
//! One file per path, usually from the selected ExifTool `t/images`. PCAPNG
//! uses the separately verified combined corpus because historical source
//! trees do not all carry that sample. This tests Rust read order, not parity
//! with the selected native ExifTool release.

use crate::core::operations::read_metadata_report;
use std::path::Path;

/// Reads per file. A path with only two hash-ordered entries repeats its
/// first order by chance with probability 2^-(READS-1).
const READS: usize = 12;

/// `(file, the path it exercises)`.
const PATHS: &[(&str, &str)] = &[
    // Sub-maps copied through `MetadataMap::iter()` / `into_iter()`, which
    // walked `TagSink`'s winner `HashMap` until it yielded file order.
    (
        "ExifTool.jpg",
        "JPEG APP6/APP10/APP11/APP14, Meta, Qualcomm, CanonVRD/PhotoMechanic/FotoStation/MIE trailers",
    ),
    ("AFCP.jpg", "AFCP trailer IPTC"),
    ("InfiRay.jpg", "InfiRay APP2/APP4/APP5/APP7/APP8/APP9"),
    ("GoPro.jpg", "APP6 GoPro"),
    ("PhotoMechanic.jpg", "PhotoMechanic trailer"),
    ("FotoStation.jpg", "FotoStation trailer"),
    ("Photoshop.psd", "PSD image resources"),
    ("CaptureOne.eip", "EIP embedded TIFF IFD0/ExifIFD"),
    ("MIFF.miff", "MIFF embedded EXIF"),
    ("Jpeg2000.jp2", "JP2 embedded TIFF + GeoTIFF"),
    ("Font.ttf", "TrueType name table"),
    ("Font.dfont", "dfont resource fork"),
    ("PCAP.pcapng", "PCAPNG blocks"),
    ("M2TS.mts", "M2TS H.264 SEI"),
    ("Geotag.log", "text file statistics"),
    // Parser-local `HashMap` accumulators.
    (
        "ExifTool.tif",
        "ICC_Profile tag table (icc::parse_icc_profile)",
    ),
    ("BPG.bpg", "ICC_Profile via embedded image"),
    (
        "GeoTiff.tif",
        "GeoTIFF key directory (geotiff_parser::parse_geotiff_keys)",
    ),
    ("PDF.pdf", "PDF Info dictionary (pdf::info_parser)"),
    ("HTML.html", "HTML meta collection (text::html)"),
    ("EXE.exe", "PE StringFileInfo (pe::version_info_parser)"),
    (
        "FujiFilm.raf",
        "RAF container + RAF MakerNote (raw::raf_parser)",
    ),
    ("Minolta.mrw", "MRW TTW MakerNote (raw::minolta_makernote)"),
    (
        "CanonRaw.crw",
        "CRW Canon CIFF records (canon::parse_canon_ciff_records)",
    ),
];

/// Every recorded occurrence, in `order`, with its family-1 group and value.
/// The access time is the clock -- each read moves it -- not the parser.
fn recorded_sequence(path: &Path) -> Vec<String> {
    let report =
        read_metadata_report(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    report
        .metadata
        .all_occurrences()
        .filter(|(key, _)| key != "File:FileAccessDate")
        .map(|(key, o)| format!("{key} [{}] {:?}", o.group1.as_ref(), o.raw))
        .collect()
}

#[test]
fn every_formerly_hash_ordered_path_records_the_same_sequence_on_every_read() {
    let mut failures = Vec::new();
    let release = crate::exiftool_oracle::repo_pin();
    for (file, exercises) in PATHS {
        // These two source-tree fixtures did not exist in the selected
        // native releases. Keep every other path under the same exact
        // repeated-sequence contract, and prove the stated absence.
        if matches!(
            (release, *file),
            ("11.78", "InfiRay.jpg" | "PCAP.pcapng") | ("12.64", "PCAP.pcapng")
        ) {
            let source = std::path::PathBuf::from(
                std::env::var_os("EXIFTOOL").expect("pinned native ExifTool"),
            );
            let missing = source
                .parent()
                .expect("native source tree")
                .join("t/images")
                .join(file);
            assert!(
                !missing.exists(),
                "{file} unexpectedly exists in native {release}"
            );
            continue;
        }
        let Some(path) = crate::test_support::pinned_t_images_fixture_path(file) else {
            eprintln!("skip: configured t/images fixture {file} is absent");
            return;
        };
        let first = recorded_sequence(&path);
        assert!(first.len() > 3, "{file}: read {} occurrences", first.len());
        if let Some(read) = (1..READS).find(|_| recorded_sequence(&path) != first) {
            failures.push(format!("  {file} ({exercises}): read {read} differs"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} paths recorded a different sequence on a repeat read:\n{}",
        failures.len(),
        PATHS.len(),
        failures.join("\n")
    );
}
