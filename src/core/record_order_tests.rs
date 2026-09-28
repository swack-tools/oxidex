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
use crate::test_support::FixtureConfig;
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

/// Prove absence in the fixture tree belonging to the oracle actually selected.
/// A cache fixture must not establish absence for an explicitly selected tree.
fn assert_selected_source_fixture_absent(
    config: &FixtureConfig,
    selected_binary: &Path,
    file: &str,
) {
    let directory = config
        .t_images_dir_for_mode()
        .expect("inspect pinned t/images directory")
        .expect("selected pinned t/images directory");
    let selected_directory = selected_binary
        .parent()
        .expect("selected ExifTool source tree")
        .join("t/images");
    assert_eq!(
        directory.canonicalize().expect("pinned t/images directory"),
        selected_directory
            .canonicalize()
            .expect("selected source t/images directory"),
        "fixture resolver selected a different ExifTool source tree"
    );
    assert!(
        !selected_directory.join(file).exists(),
        "{file} unexpectedly exists in selected native source"
    );
}

#[test]
fn source_fixture_absence_tracks_the_selected_oracle_tree() {
    let temp = tempfile::tempdir().expect("fixture roots");
    let source = temp.path().join("source");
    let cache = temp.path().join("cache");
    let cached_tree = cache.join("exiftool");
    for tree in [&source, &cached_tree] {
        std::fs::create_dir_all(tree.join("t/images")).expect("fixture tree");
    }
    let config = FixtureConfig::new(Some(source.clone()), cache.clone(), true);
    assert_selected_source_fixture_absent(&config, &source.join("exiftool"), "missing.jpg");
    assert!(
        std::panic::catch_unwind(|| {
            assert_selected_source_fixture_absent(
                &config,
                &cached_tree.join("exiftool"),
                "missing.jpg",
            );
        })
        .is_err(),
        "a different selected source cannot prove fixture absence"
    );

    let cache_only = FixtureConfig::new(None, cache, true);
    assert_selected_source_fixture_absent(
        &cache_only,
        &cached_tree.join("exiftool"),
        "missing.jpg",
    );
    std::fs::write(cached_tree.join("t/images/missing.jpg"), b"present").expect("selected fixture");
    assert!(
        std::panic::catch_unwind(|| {
            assert_selected_source_fixture_absent(
                &cache_only,
                &cached_tree.join("exiftool"),
                "missing.jpg",
            );
        })
        .is_err(),
        "a present fixture cannot satisfy absence"
    );
}

#[test]
fn every_formerly_hash_ordered_path_records_the_same_sequence_on_every_read() {
    let mut failures = Vec::new();
    let release = crate::exiftool_oracle::repo_pin();
    assert!(
        matches!(release, "11.78" | "12.64" | "13.59"),
        "unsupported native fixture release: {release}"
    );
    for (file, exercises) in PATHS {
        // These two source-tree fixtures did not exist in the selected
        // native releases. Keep every other path under the same exact
        // repeated-sequence contract, and prove the stated absence.
        if matches!(
            (release, *file),
            ("11.78", "InfiRay.jpg" | "PCAP.pcapng") | ("12.64", "PCAP.pcapng")
        ) {
            let oracle = crate::exiftool_oracle::resolve().expect("pinned native ExifTool oracle");
            assert!(
                oracle.is_verified(),
                "unverified native oracle: {}",
                oracle.display()
            );
            assert!(
                oracle.source != crate::exiftool_oracle::Source::Path,
                "native fixture absence needs a selected source tree"
            );
            let selected_binary = Path::new(oracle.argv.last().expect("oracle binary"));
            let config = FixtureConfig::from_environment(release);
            assert_selected_source_fixture_absent(&config, selected_binary, file);
            // PCAPNG still exercises Rust ordering from the independently
            // pinned combined corpus even when this native tree lacks it.
            if *file != "PCAP.pcapng" {
                continue;
            }
        }
        let path = if *file == "PCAP.pcapng" {
            crate::test_support::pinned_combined_fixture_path(file)
        } else {
            crate::test_support::pinned_t_images_fixture_path(file)
        };
        let Some(path) = path else {
            eprintln!("skip: configured fixture {file} is absent");
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
