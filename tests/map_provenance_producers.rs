//! Tripwire for per-occurrence provenance (#949 review): every public
//! function that hands out a `MetadataMap` built from a file must mark its
//! rows read, or a caller's untouched rows look like assignments to the
//! writers (an XP string re-encoded, a removal skipped). Readers record
//! through the same `MetadataMap::insert` a caller's assignment uses, so the
//! marking is each producer's job: its body runs inside
//! `crate::core::metadata_map::file_rows`, or it calls
//! `mark_read_complete` itself.
//!
//! This scans `src/` for every `pub fn ... -> Result<MetadataMap>` /
//! `-> MetadataMap` -- any `Result` spelling, with the default error or its
//! own (`Result<MetadataMap, String>`, `std::result::Result<..>`: #949
//! review, map_provenance_producers.rs:56) -- and every `FormatParser::parse`
//! impl, and fails naming each one that does neither. A function that returns a copy or a
//! projection of a map it was given (never a file's rows) is listed in
//! `DERIVED` with the reason; it carries each row's own provenance instead.

#[path = "common/fixtures.rs"]
mod fixtures;

use regex::Regex;
use std::path::Path;

/// Public functions that return a map derived from a map the caller gave
/// them, not from a file: they carry each row's provenance
/// (`MetadataMap::copy_provenance_from`, `set_last_assigned`, a clone).
const DERIVED: &[(&str, &str)] = &[
    ("format_for_exiftool", "formatted copy of its input"),
    ("strip_extended_only", "filtered copy of its input"),
    ("filter_tiff_writable_tags", "filtered copy of its input"),
    ("normalize_metadata_map", "renamed copy of its input"),
    ("without_print_conv", "ValueConv projection of its input"),
    ("into_map", "moves the wrapped map out"),
    ("into_metadata", "moves the report's map out"),
    ("new", "empty map"),
    ("with_capacity", "empty map"),
];

/// Producers whose marking is not in the first lines of their body: they
/// delegate to a marking producer, or mark on their way out.
const MARKED_ELSEWHERE: &[(&str, &str)] = &[
    (
        "read_metadata",
        "delegates to read_metadata_with_detector_and_options, which marks",
    ),
    (
        "read_metadata_with_detector",
        "delegates to read_metadata_with_detector_and_options, which marks",
    ),
    (
        "build_display_map",
        "a display projection; marks read before it returns",
    ),
];

/// A public function returning a map: `MetadataMap`, or a `Result` of one
/// under any spelling -- `Result<MetadataMap>`, `crate::error::Result<..>`,
/// `Result<MetadataMap, E>`, `std::result::Result<MetadataMap, E>`.
fn producer_regex() -> Regex {
    Regex::new(
        r"(?m)^[ \t]*pub fn (\w+)(?:<[^>]*>)?\s*\([^{;]*?\)\s*->\s*(?:(?:(?:std|core)::result::|crate::error::)?Result<\s*(?:crate::core::)?MetadataMap\s*(?:,[^{;]*?)?>|(?:crate::core::)?MetadataMap)\s*\{",
    )
    .unwrap()
}

#[test]
fn every_public_map_producer_marks_its_rows_read() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let producer = producer_regex();
    let trait_impl = Regex::new(
        r"(?m)^[ \t]*fn parse\(&self, reader: &dyn FileReader\) -> Result<MetadataMap>\s*\{",
    )
    .unwrap();
    let marks = |body: &str| body.contains("file_rows(") || body.contains("mark_read_complete()");
    let mut unmarked = Vec::new();
    let mut seen = 0;
    for entry in walkdir::WalkDir::new(&root) {
        let entry = entry.unwrap();
        if entry.path().extension().is_none_or(|ext| ext != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(entry.path()).unwrap();
        let rel = entry
            .path()
            .strip_prefix(&root)
            .unwrap()
            .display()
            .to_string();
        for (name, start) in producer
            .captures_iter(&text)
            .map(|c| (c[1].to_string(), c.get(0).unwrap().end()))
            .chain(
                trait_impl
                    .find_iter(&text)
                    .map(|m| ("FormatParser::parse".to_string(), m.end())),
            )
        {
            seen += 1;
            if DERIVED
                .iter()
                .chain(MARKED_ELSEWHERE)
                .any(|(listed, _)| *listed == name)
            {
                continue;
            }
            let body = &text[start..(start + 600).min(text.len())];
            if !marks(body) {
                unmarked.push(format!("{rel}: {name}"));
            }
        }
    }
    assert!(seen > 100, "the scan found only {seen} producers");
    assert!(
        unmarked.is_empty(),
        "public producers of file rows that do not mark them read \
         (wrap the body in `crate::core::metadata_map::file_rows`):\n{}",
        unmarked.join("\n")
    );
}

/// The shapes the matcher must see (#949 review): a scan that silently
/// skips a return spelling is how `parse_quicktime_metadata` and
/// `metadata_extractor::extract_metadata` went unmarked.
#[test]
fn the_producer_matcher_sees_every_result_spelling() {
    let producer = producer_regex();
    for signature in [
        "pub fn a(r: &dyn FileReader) -> Result<MetadataMap> {",
        "pub fn a(r: &dyn FileReader) -> crate::error::Result<MetadataMap> {",
        "pub fn a(r: &dyn FileReader) -> Result<MetadataMap, String> {",
        "pub fn a(r: &dyn FileReader) -> std::result::Result<MetadataMap, String> {",
        "pub fn a(\n    r: &dyn crate::core::FileReader,\n) -> std::result::Result<MetadataMap, String> {",
        "pub fn a(d: &[u8]) -> Result<crate::core::MetadataMap, Box<dyn std::error::Error>> {",
        "pub fn a() -> MetadataMap {",
    ] {
        assert!(producer.is_match(signature), "not matched: {signature}");
    }
    for signature in [
        "pub fn a() -> Result<Vec<MetadataMap>> {",
        "pub fn a() -> Option<MetadataMap> {",
        "pub fn a() -> (MetadataMap, MetadataMap) {",
    ] {
        assert!(!producer.is_match(signature), "matched: {signature}");
    }
}

type Producer = fn(&Path) -> Result<oxidex::core::MetadataMap, String>;

/// Each public producer the widened matcher newly covers, run on its pinned
/// t/images sample: every row it hands out is read from the file
/// (`MetadataMap::is_assigned` false), the provenance `write_metadata`
/// reads to tell a caller's set from a carried row. At e4d2d79a the
/// producers that build their map with `insert` handed out assignments.
/// (Producers with no t/images sample they parse -- `iWork.numbers` is the
/// pre-2013 XML format `parse_numbers_metadata` rejects -- are covered by the
/// tripwire above;
/// `metadata_extractor::extract_metadata` is crate-private and checked with
/// the other QuickTime producers in its own module.)
#[test]
fn newly_covered_producers_hand_out_read_rows() {
    use oxidex::parsers::*;
    macro_rules! on_reader {
        ($f:path) => {
            (|path: &Path| {
                let reader = oxidex::io::MMapReader::new(path).map_err(|e| e.to_string())?;
                $f(&reader)
            }) as Producer
        };
    }
    let producers: &[(&str, &str, Producer)] = &[
        (
            "parse_ar_metadata",
            "EXE.a",
            on_reader!(archive::ar::parse_ar_metadata),
        ),
        (
            "parse_eip_metadata",
            "CaptureOne.eip",
            on_reader!(archive::captureone::parse_eip_metadata),
        ),
        (
            "parse_gz_metadata",
            "ZIP.gz",
            on_reader!(archive::gz::parse_gz_metadata),
        ),
        (
            "parse_iso_metadata",
            "ISO.iso",
            on_reader!(archive::iso::parse_iso_metadata),
        ),
        (
            "parse_ole_metadata",
            "FlashPix.ppt",
            on_reader!(archive::ole::parse_ole_metadata),
        ),
        (
            "parse_rar_metadata",
            "ZIP.rar",
            on_reader!(archive::rar::parse_rar_metadata),
        ),
        (
            "parse_zip_metadata",
            "ZIP.zip",
            on_reader!(archive::zip::parse_zip_metadata),
        ),
        (
            "parse_dss_metadata",
            "Olympus.dss",
            on_reader!(audio::dss::parse_dss_metadata),
        ),
        (
            "parse_ram_metadata",
            "Real.ram",
            on_reader!(audio::ram::parse_ram_metadata),
        ),
        (
            "parse_real_audio_metadata",
            "Real.ra",
            on_reader!(audio::real_audio::parse_real_audio_metadata),
        ),
        (
            "parse_ics_metadata",
            "VCard.ics",
            on_reader!(document::ics::parse_ics_metadata),
        ),
        (
            "parse_indesign_metadata",
            "InDesign.indd",
            on_reader!(document::indesign::parse_indesign_metadata),
        ),
        (
            "parse_docx_metadata",
            "OOXML.docx",
            on_reader!(document::ooxml::parse_docx_metadata),
        ),
        (
            "parse_tnef_metadata",
            "TNEF.tnef",
            on_reader!(document::tnef::parse_tnef_metadata),
        ),
        (
            "parse_elf_metadata",
            "EXE.elf",
            on_reader!(elf::parse_elf_metadata),
        ),
        (
            "parse_afm_metadata",
            "Font.afm",
            on_reader!(font::afm::parse_afm_metadata),
        ),
        (
            "parse_pfb_metadata",
            "Font.pfb",
            on_reader!(font::pfb::parse_pfb_metadata),
        ),
        (
            "parse_printer_font_metrics",
            "Font.pfm",
            on_reader!(font::pfm::parse_printer_font_metrics),
        ),
        (
            "parse_ttf_metadata",
            "Font.ttf",
            on_reader!(font::ttf::parse_ttf_metadata),
        ),
        (
            "parse_bmp_metadata",
            "BMP.bmp",
            on_reader!(image::bmp::parse_bmp_metadata),
        ),
        (
            "parse_bpg_metadata",
            "BPG.bpg",
            on_reader!(image::bpg::parse_bpg_metadata),
        ),
        (
            "parse_czi_metadata",
            "ZISRAW.czi",
            on_reader!(image::czi::parse_czi_metadata),
        ),
        (
            "parse_djvu_metadata",
            "DjVu.djvu",
            on_reader!(image::djvu::parse_djvu_metadata),
        ),
        (
            "parse_dpx_metadata",
            "DPX.dpx",
            on_reader!(image::dpx::parse_dpx_metadata),
        ),
        (
            "parse_exr_metadata",
            "OpenEXR.exr",
            on_reader!(image::exr::parse_exr_metadata),
        ),
        (
            "parse_flif_metadata",
            "FLIF.flif",
            on_reader!(image::flif::parse_flif_metadata),
        ),
        (
            "parse_gif_metadata",
            "GIF.gif",
            on_reader!(image::gif::parse_gif_metadata),
        ),
        (
            "parse_heif_metadata",
            "QuickTime.heic",
            on_reader!(image::heif::parse_heif_metadata),
        ),
        (
            "parse_ico_metadata",
            "ICO.ico",
            on_reader!(image::ico::parse_ico_metadata),
        ),
        (
            "parse_jpeg2000_metadata",
            "Jpeg2000.jp2",
            on_reader!(image::jpeg2000::parse_jpeg2000_metadata),
        ),
        (
            "parse_jxl_metadata",
            "JXL.jxl",
            on_reader!(image::jxl::parse_jxl_metadata),
        ),
        (
            "parse_miff_metadata",
            "MIFF.miff",
            on_reader!(image::miff::parse_miff_metadata),
        ),
        (
            "parse_pcx_metadata",
            "PCX.pcx",
            on_reader!(image::pcx::parse_pcx_metadata),
        ),
        (
            "parse_pfm_metadata",
            "PFM.pfm",
            on_reader!(image::pfm::parse_pfm_metadata),
        ),
        (
            "parse_pgf_metadata",
            "PGF.pgf",
            on_reader!(image::pgf::parse_pgf_metadata),
        ),
        (
            "parse_pcd_metadata",
            "PhotoCD.pcd",
            on_reader!(image::photocd::parse_pcd_metadata),
        ),
        (
            "parse_pict_metadata",
            "PICT.pict",
            on_reader!(image::pict::parse_pict_metadata),
        ),
        (
            "parse_pmp_metadata",
            "Sony.pmp",
            on_reader!(image::pmp::parse_pmp_metadata),
        ),
        (
            "parse_ppm_metadata",
            "PPM.ppm",
            on_reader!(image::ppm::parse_ppm_metadata),
        ),
        (
            "parse_psd_metadata",
            "Photoshop.psd",
            on_reader!(image::psd::parse_psd_metadata),
        ),
        (
            "parse_psp_metadata",
            "PSP.psp",
            on_reader!(image::psp::parse_psp_metadata),
        ),
        (
            "parse_radiance_metadata",
            "Radiance.hdr",
            on_reader!(image::radiance::parse_radiance_metadata),
        ),
        (
            "parse_svg_metadata",
            "XMP.svg",
            on_reader!(image::svg::parse_svg_metadata),
        ),
        (
            "parse_webp_metadata",
            "RIFF.webp",
            on_reader!(image::webp::parse_webp_metadata),
        ),
        (
            "parse_wpg_metadata",
            "WPG.wpg",
            on_reader!(image::wpg::parse_wpg_metadata),
        ),
        (
            "parse_xcf_metadata",
            "GIMP.xcf",
            on_reader!(image::xcf::parse_xcf_metadata),
        ),
        (
            "parse_xisf_metadata",
            "XISF.xisf",
            on_reader!(image::xisf::parse_xisf_metadata),
        ),
        (
            "parse_macho_metadata",
            "EXE.macho",
            on_reader!(macho::parse_macho_metadata),
        ),
        (
            "parse_mie_metadata",
            "MIE.mie",
            on_reader!(mie::parse_mie_metadata),
        ),
        (
            "parse_quicktime_metadata",
            "QuickTime.mov",
            on_reader!(quicktime::parse_quicktime_metadata),
        ),
        (
            "parse_quicktime_metadata_from_bytes",
            "QuickTime.mov",
            (|path: &Path| {
                quicktime::parse_quicktime_metadata_from_bytes(
                    &std::fs::read(path).map_err(|e| e.to_string())?,
                )
            }) as Producer,
        ),
        (
            "parse_quicktime_metadata_from_bytes_with_options",
            "CanonRaw.cr3",
            (|path: &Path| {
                quicktime::parse_quicktime_metadata_from_bytes_with_options(
                    &std::fs::read(path).map_err(|e| e.to_string())?,
                    true,
                )
            }) as Producer,
        ),
        (
            "parse_rsrc_metadata",
            "Font.dfont",
            on_reader!(rsrc::parse_rsrc_metadata),
        ),
        (
            "parse_aa_metadata",
            "Audible.aa",
            on_reader!(specialized::aa::parse_aa_metadata),
        ),
        (
            "parse_fit_metadata",
            "Garmin.fit",
            on_reader!(specialized::fit::parse_fit_metadata),
        ),
        (
            "parse_fits_metadata",
            "FITS.fits",
            on_reader!(specialized::fits::parse_fits_metadata),
        ),
        (
            "parse_itc_metadata",
            "ITC.itc",
            on_reader!(specialized::itc::parse_itc_metadata),
        ),
        (
            "parse_lnk_metadata",
            "LNK.lnk",
            on_reader!(specialized::lnk::parse_lnk_metadata),
        ),
        (
            "parse_lytro_metadata",
            "Lytro.lfp",
            on_reader!(specialized::lytro::parse_lytro_metadata),
        ),
        (
            "parse_macos_metadata",
            "MacOS.macos",
            on_reader!(specialized::macos::parse_macos_metadata),
        ),
        (
            "parse_moi_metadata",
            "MOI.moi",
            on_reader!(specialized::moi::parse_moi_metadata),
        ),
        (
            "parse_mrc_metadata",
            "MRC.mrc",
            on_reader!(specialized::mrc::parse_mrc_metadata),
        ),
        (
            "parse_palm_metadata",
            "Palm.mobi",
            on_reader!(specialized::palm::parse_palm_metadata),
        ),
        (
            "parse_pcap_metadata",
            "PCAP.pcapng",
            on_reader!(specialized::pcap::parse_pcap_metadata),
        ),
        (
            "parse_plist_metadata",
            "PLIST-bin.plist",
            on_reader!(specialized::plist::parse_plist_metadata),
        ),
        (
            "parse_r3d_metadata",
            "Red.r3d",
            on_reader!(specialized::red::parse_r3d_metadata),
        ),
        (
            "parse_torrent_metadata",
            "Torrent.torrent",
            on_reader!(specialized::torrent::parse_torrent_metadata),
        ),
        (
            "parse_eps_metadata",
            "PostScript.eps",
            on_reader!(text::eps::parse_eps_metadata),
        ),
        (
            "parse_html_metadata",
            "HTML.html",
            on_reader!(text::html::parse_html_metadata),
        ),
        (
            "parse_csv_metadata",
            "Text.csv",
            on_reader!(text::txt::parse_csv_metadata),
        ),
        (
            "parse_txt_metadata",
            "Text1.txt",
            on_reader!(text::txt::parse_txt_metadata),
        ),
        (
            "parse_vcf_metadata",
            "VCard.vcf",
            on_reader!(text::vcf::parse_vcf_metadata),
        ),
        (
            "parse_asf_metadata",
            "ASF.wmv",
            on_reader!(video::asf::parse_asf_metadata),
        ),
        (
            "parse_avi_metadata",
            "RIFF.avi",
            on_reader!(video::avi::parse_avi_metadata),
        ),
        (
            "parse_dv_metadata",
            "DV.dv",
            on_reader!(video::dv::parse_dv_metadata),
        ),
        (
            "parse_flv_metadata",
            "Flash.flv",
            on_reader!(video::flv::parse_flv_metadata),
        ),
        (
            "parse_mkv_metadata",
            "Matroska.mkv",
            on_reader!(video::mkv::parse_mkv_metadata),
        ),
        (
            "parse_mp4_metadata",
            "QuickTime.m4a",
            on_reader!(video::mp4::parse_mp4_metadata),
        ),
        (
            "parse_mts_metadata",
            "M2TS.mts",
            on_reader!(video::mts::parse_mts_metadata),
        ),
        (
            "parse_mxf_metadata",
            "MXF.mxf",
            on_reader!(video::mxf::parse_mxf_metadata),
        ),
        (
            "parse_realmedia_metadata",
            "Real.rm",
            on_reader!(video::realmedia::parse_realmedia_metadata),
        ),
        (
            "parse_swf_metadata",
            "Flash.swf",
            on_reader!(video::swf::parse_swf_metadata),
        ),
        (
            "parse_wtv_metadata",
            "WTV.wtv",
            on_reader!(video::wtv::parse_wtv_metadata),
        ),
    ];
    let mut failures = Vec::new();
    let mut checked = 0;
    for (name, sample, produce) in producers {
        let Some(path) = fixtures::pinned_t_images_fixture_path(sample) else {
            continue;
        };
        let map = match produce(&path) {
            Ok(map) => map,
            Err(err) => {
                failures.push(format!("{name} on {sample}: {err}"));
                continue;
            }
        };
        checked += 1;
        if map.is_empty() {
            failures.push(format!("{name} on {sample}: no rows"));
        }
        let assigned: Vec<&String> = map.keys().filter(|key| map.is_assigned(key)).collect();
        if !assigned.is_empty() {
            failures.push(format!(
                "{name} on {sample}: {} of {} rows are assignments, e.g. {:?}",
                assigned.len(),
                map.len(),
                &assigned[..assigned.len().min(3)]
            ));
        }
    }
    if checked == 0 {
        eprintln!("skipping: no pinned t/images samples");
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
