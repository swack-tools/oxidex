//! Format parser dispatch
//!
//! This module handles dispatching to the appropriate format-specific parser
//! based on the detected file format.

use super::{FileFormat, FileReader, MetadataMap, ReadOptions};
use crate::error::{ExifToolError, Result};
use crate::parsers::archive::ar::parse_ar_metadata;
use crate::parsers::archive::gz::parse_gz_metadata;
use crate::parsers::archive::iso::parse_iso_metadata;
use crate::parsers::archive::ole::parse_ole_metadata;
use crate::parsers::archive::rar::parse_rar_metadata;
use crate::parsers::archive::sevenz::parse_7z_metadata;
use crate::parsers::archive::tar::parse_tar_metadata;
use crate::parsers::archive::zip::parse_zip_metadata;
use crate::parsers::audio::aac::parse_aac_metadata;
use crate::parsers::audio::aiff::parse_aiff_metadata;
use crate::parsers::audio::ape::parse_ape_metadata;
use crate::parsers::audio::dss::parse_dss_metadata;
use crate::parsers::audio::flac::parse_flac_metadata;
use crate::parsers::audio::mp3::parse_mp3_metadata;
use crate::parsers::audio::mpc::parse_mpc_metadata;
use crate::parsers::audio::ogg::parse_ogg_metadata;
use crate::parsers::audio::opus::parse_opus_metadata;
use crate::parsers::audio::ram::parse_ram_metadata;
use crate::parsers::audio::wav::parse_wav_metadata;
use crate::parsers::document::eml::parse_eml_metadata;
use crate::parsers::document::epub::parse_epub_metadata;
use crate::parsers::document::ics::parse_ics_metadata;
use crate::parsers::document::iwork::{
    parse_keynote_metadata, parse_numbers_metadata, parse_pages_metadata,
};
use crate::parsers::document::ooxml::parse_docx_metadata;
use crate::parsers::document::ooxml::parse_pptx_metadata;
use crate::parsers::document::ooxml::parse_xlsx_metadata;
use crate::parsers::document::tnef::parse_tnef_metadata;
use crate::parsers::font::otf::parse_otf_metadata;
use crate::parsers::font::ttf::parse_ttf_metadata;
use crate::parsers::font::woff::parse_woff_metadata;
use crate::parsers::font::woff2::parse_woff2_metadata;
// Note: AVIF uses parse_quicktime_metadata since AVIF is ISOBMFF-based
use crate::parsers::image::bmp::parse_bmp_metadata;
use crate::parsers::image::bpg::parse_bpg_metadata;
use crate::parsers::image::djvu::parse_djvu_metadata;
use crate::parsers::image::dpx::parse_dpx_metadata;
use crate::parsers::image::exr::parse_exr_metadata;
use crate::parsers::image::flif::parse_flif_metadata;
use crate::parsers::image::gif::parse_gif_metadata;
use crate::parsers::image::miff::parse_miff_metadata;
use crate::parsers::image::pcx::parse_pcx_metadata;
use crate::parsers::image::pfm::parse_pfm_metadata;
use crate::parsers::image::pgf::parse_pgf_metadata;
use crate::parsers::image::photocd::parse_pcd_metadata;
use crate::parsers::image::radiance::parse_radiance_metadata;
use crate::parsers::image::xcf::parse_xcf_metadata;
// Note: HEIF uses parse_quicktime_metadata since HEIF is ISOBMFF-based
use crate::parsers::canon_vrd::{parse_dr4_file, parse_vrd_file};
use crate::parsers::elf::parse_elf_metadata;
use crate::parsers::flir_fpf::parse_fpf_metadata;
use crate::parsers::icc::parse_icc_file;
use crate::parsers::image::ico::parse_ico_metadata;
use crate::parsers::image::jpeg2000::parse_jpeg2000_metadata;
use crate::parsers::image::jxl::parse_jxl_metadata;
use crate::parsers::image::psd::parse_psd_metadata;
use crate::parsers::image::svg::parse_svg_metadata;
use crate::parsers::image::webp::parse_webp_metadata;
use crate::parsers::image::wpg::parse_wpg_metadata;
use crate::parsers::macho::parse_macho_metadata;
use crate::parsers::mie::parse_mie_metadata;
use crate::parsers::pdf::parse_pdf_metadata;
use crate::parsers::pe::parse_pe_metadata;
use crate::parsers::png::parse_png_metadata;
use crate::parsers::quicktime::parse_quicktime_metadata;
use crate::parsers::specialized::aa::parse_aa_metadata;
use crate::parsers::specialized::dwg::parse_dwg_metadata;
use crate::parsers::specialized::dxf::parse_dxf_metadata;
use crate::parsers::specialized::evtx::parse_evtx_metadata;
use crate::parsers::specialized::fit::parse_fit_metadata;
use crate::parsers::specialized::fits::{parse_dicom_metadata, parse_fits_metadata};
use crate::parsers::specialized::gltf::parse_gltf_metadata;
use crate::parsers::specialized::hdf5::parse_hdf5_metadata;
use crate::parsers::specialized::itc::parse_itc_metadata;
use crate::parsers::specialized::lnk::parse_lnk_metadata;
use crate::parsers::specialized::lytro::parse_lytro_metadata;
use crate::parsers::specialized::moi::parse_moi_metadata;
use crate::parsers::specialized::mrc::parse_mrc_metadata;
use crate::parsers::specialized::obj::parse_obj_metadata;
use crate::parsers::specialized::pcap::parse_pcap_metadata;
use crate::parsers::specialized::plist::parse_plist_metadata;
use crate::parsers::specialized::prefetch::parse_prefetch_metadata;
use crate::parsers::specialized::registry::parse_registry_metadata;
use crate::parsers::specialized::sqlite::parse_sqlite_metadata;
use crate::parsers::specialized::stl::parse_stl_metadata;
use crate::parsers::specialized::x509::parse_x509_metadata;
use crate::parsers::text::eps::parse_eps_metadata;
use crate::parsers::text::html::parse_html_metadata;
use crate::parsers::text::txt::parse_txt_metadata;
use crate::parsers::text::vcf::parse_vcf_metadata;
use crate::parsers::video::asf::parse_asf_metadata;
use crate::parsers::video::avi::parse_avi_metadata;
use crate::parsers::video::flv::parse_flv_metadata;
use crate::parsers::video::mkv::parse_mkv_metadata;
use crate::parsers::video::mts::parse_mts_metadata;
use crate::parsers::video::mxf::parse_mxf_metadata;
use crate::parsers::video::webm::parse_webm_metadata;
use crate::parsers::xmp::generic_xml::parse_xml_file;
use crate::parsers::xmp::parse_xmp_file;

// Import format-specific parsers from operations module
use super::operations::{parse_casio_cam_metadata, parse_jpeg_metadata, parse_tiff_metadata};

/// Dispatches to the appropriate format parser based on file format.
///
/// This function encapsulates the large match statement for format-specific parsing,
/// applying a consistent error conversion pattern across all parsers.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the file data
/// * `format` - Detected file format
/// * `options` - Step 21 request-awareness (`ReadOptions`): which specific
///   tags were asked for and whether OxiDex's `--extended-output` namespace
///   is on. Only JPEG consults this today (`JPEGQualityEstimate`'s request
///   gate and the SOF/DQT diagnostic-tag extended gate); every other format
///   parser's signature is unchanged and simply does not receive it. See
///   `core::read_options` for why.
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully parsed metadata
/// * `Err(ExifToolError)` - Parse error or unsupported format
pub fn dispatch_format_parser(
    reader: &dyn FileReader,
    format: FileFormat,
    options: &ReadOptions,
) -> Result<MetadataMap> {
    match format {
        FileFormat::JPEG => parse_jpeg_metadata(reader, options),
        FileFormat::TIFF => parse_tiff_metadata(reader),
        FileFormat::PNG => parse_png_metadata(reader),
        FileFormat::PDF => parse_pdf_metadata(reader),
        FileFormat::PE => parse_pe_metadata(reader),
        FileFormat::QuickTime => {
            convert_string_error(parse_quicktime_metadata(reader), "QuickTime")
        }
        FileFormat::CasioCAM => parse_casio_cam_metadata(reader),
        FileFormat::CameraRaw(raw_format) => {
            // Parse camera raw format using raw metadata parser
            // Read entire file for raw parsing (raw formats need full file access)
            let size = reader.size() as usize;
            let data = reader.read(0, size)?;
            crate::parsers::raw::parse_raw_metadata(data, raw_format)
        }
        FileFormat::MKV => convert_string_error(parse_mkv_metadata(reader), "MKV"),
        FileFormat::WEBM => convert_string_error(parse_webm_metadata(reader), "WebM"),
        FileFormat::FLV => convert_string_error(parse_flv_metadata(reader), "FLV"),
        FileFormat::AVI => convert_string_error(parse_avi_metadata(reader), "AVI"),
        FileFormat::MTS => convert_string_error(parse_mts_metadata(reader), "MTS"),
        FileFormat::ASF => convert_string_error(parse_asf_metadata(reader), "ASF"),
        FileFormat::MXF => convert_string_error(parse_mxf_metadata(reader), "MXF"),
        FileFormat::MP3 => convert_string_error(parse_mp3_metadata(reader), "MP3"),
        FileFormat::FLAC => convert_string_error(parse_flac_metadata(reader), "FLAC"),
        FileFormat::AAC => convert_string_error(parse_aac_metadata(reader), "AAC"),
        FileFormat::WAV => convert_string_error(parse_wav_metadata(reader), "WAV"),
        FileFormat::AIFF => convert_string_error(parse_aiff_metadata(reader), "AIFF"),
        FileFormat::OGG => convert_string_error(parse_ogg_metadata(reader), "OGG"),
        FileFormat::OPUS => convert_string_error(parse_opus_metadata(reader), "Opus"),
        FileFormat::APE => convert_string_error(parse_ape_metadata(reader), "APE"),
        FileFormat::MPC => convert_string_error(parse_mpc_metadata(reader), "MPC"),
        FileFormat::RAM => convert_string_error(parse_ram_metadata(reader), "RAM"),
        FileFormat::DSS => convert_string_error(parse_dss_metadata(reader), "DSS"),
        FileFormat::ZIP => convert_string_error(parse_zip_metadata(reader), "ZIP"),
        FileFormat::DOCX => convert_string_error(parse_docx_metadata(reader), "DOCX"),
        FileFormat::XLSX => convert_string_error(parse_xlsx_metadata(reader), "XLSX"),
        FileFormat::PPTX => convert_string_error(parse_pptx_metadata(reader), "PPTX"),
        FileFormat::Pages => convert_string_error(parse_pages_metadata(reader), "Pages"),
        FileFormat::Numbers => convert_string_error(parse_numbers_metadata(reader), "Numbers"),
        FileFormat::Keynote => convert_string_error(parse_keynote_metadata(reader), "Keynote"),
        FileFormat::EPUB => convert_string_error(parse_epub_metadata(reader), "EPUB"),
        FileFormat::RAR => convert_string_error(parse_rar_metadata(reader), "RAR"),
        FileFormat::SevenZ => convert_string_error(parse_7z_metadata(reader), "7z"),
        FileFormat::ISO => convert_string_error(parse_iso_metadata(reader), "ISO"),
        FileFormat::TAR => convert_string_error(parse_tar_metadata(reader), "TAR"),
        FileFormat::AR => convert_string_error(parse_ar_metadata(reader), "AR"),
        FileFormat::GZ => convert_string_error(parse_gz_metadata(reader), "GZ"),
        // Font formats
        FileFormat::TTF => convert_string_error(parse_ttf_metadata(reader), "TTF"),
        FileFormat::OTF => convert_string_error(parse_otf_metadata(reader), "OTF"),
        FileFormat::WOFF => convert_string_error(parse_woff_metadata(reader), "WOFF"),
        FileFormat::WOFF2 => convert_string_error(parse_woff2_metadata(reader), "WOFF2"),
        // Advanced image formats
        // AVIF uses ISOBMFF container (same as MP4/HEIF), use QuickTime parser for full metadata
        FileFormat::AVIF => convert_string_error(parse_quicktime_metadata(reader), "AVIF"),
        // HEIF uses ISOBMFF container (same as MP4/MOV), use QuickTime parser for full EXIF extraction
        FileFormat::HEIF => convert_string_error(parse_quicktime_metadata(reader), "HEIF"),
        FileFormat::JXL => convert_string_error(parse_jxl_metadata(reader), "JXL"),
        FileFormat::Jpeg2000 => convert_string_error(parse_jpeg2000_metadata(reader), "JPEG 2000"),
        FileFormat::BPG => convert_string_error(parse_bpg_metadata(reader), "BPG"),
        FileFormat::EXR => convert_string_error(parse_exr_metadata(reader), "EXR"),
        FileFormat::DPX => convert_string_error(parse_dpx_metadata(reader), "DPX"),
        FileFormat::PCD => convert_string_error(parse_pcd_metadata(reader), "PCD"),
        FileFormat::PFM => convert_string_error(parse_pfm_metadata(reader), "PFM"),
        FileFormat::HDR => convert_string_error(parse_radiance_metadata(reader), "HDR"),
        FileFormat::FLIF => convert_string_error(parse_flif_metadata(reader), "FLIF"),
        FileFormat::XCF => convert_string_error(parse_xcf_metadata(reader), "XCF"),
        FileFormat::MIFF => convert_string_error(parse_miff_metadata(reader), "MIFF"),
        FileFormat::SVG => convert_string_error(parse_svg_metadata(reader), "SVG"),
        FileFormat::ICO => convert_string_error(parse_ico_metadata(reader), "ICO"),
        FileFormat::PSD => convert_string_error(parse_psd_metadata(reader), "PSD"),
        FileFormat::WPG => convert_string_error(parse_wpg_metadata(reader), "WPG"),
        FileFormat::DJVU => convert_string_error(parse_djvu_metadata(reader), "DjVu"),
        // Specialized formats
        FileFormat::ELF => convert_string_error(parse_elf_metadata(reader), "ELF"),
        FileFormat::MachO => convert_string_error(parse_macho_metadata(reader), "Mach-O"),
        FileFormat::DWG => convert_string_error(parse_dwg_metadata(reader), "DWG"),
        FileFormat::DXF => convert_string_error(parse_dxf_metadata(reader), "DXF"),
        FileFormat::STL => convert_string_error(parse_stl_metadata(reader), "STL"),
        FileFormat::OBJ => convert_string_error(parse_obj_metadata(reader), "OBJ"),
        FileFormat::GLTF => convert_string_error(parse_gltf_metadata(reader), "glTF"),
        FileFormat::FIT => convert_string_error(parse_fit_metadata(reader), "FIT"),
        FileFormat::FITS => convert_string_error(parse_fits_metadata(reader), "FITS"),
        FileFormat::DICOM => parse_dicom_metadata(reader),
        FileFormat::HDF5 => convert_string_error(parse_hdf5_metadata(reader), "HDF5"),
        FileFormat::VCF => convert_string_error(parse_vcf_metadata(reader), "VCF"),
        FileFormat::TXT => convert_string_error(parse_txt_metadata(reader), "TXT"),
        FileFormat::HTML => convert_string_error(parse_html_metadata(reader), "HTML"),
        FileFormat::LNK => convert_string_error(parse_lnk_metadata(reader), "LNK"),
        FileFormat::LFP => convert_string_error(parse_lytro_metadata(reader), "LFP"),
        FileFormat::SQLite => convert_string_error(parse_sqlite_metadata(reader), "SQLite"),
        FileFormat::ICS => convert_string_error(parse_ics_metadata(reader), "ICS"),
        FileFormat::EML => convert_string_error(parse_eml_metadata(reader), "EML"),
        FileFormat::TNEF => convert_string_error(parse_tnef_metadata(reader), "TNEF"),
        FileFormat::OLE => convert_string_error(parse_ole_metadata(reader), "OLE"),
        FileFormat::Prefetch => convert_string_error(parse_prefetch_metadata(reader), "Prefetch"),
        FileFormat::Registry => convert_string_error(parse_registry_metadata(reader), "Registry"),
        FileFormat::EVTX => convert_string_error(parse_evtx_metadata(reader), "EVTX"),
        FileFormat::Plist => convert_string_error(parse_plist_metadata(reader), "Plist"),
        FileFormat::PCAP | FileFormat::PCAPNG => {
            convert_string_error(parse_pcap_metadata(reader), "PCAP")
        }
        FileFormat::GIF => convert_string_error(parse_gif_metadata(reader), "GIF"),
        FileFormat::BMP => convert_string_error(parse_bmp_metadata(reader), "BMP"),
        FileFormat::WebP => convert_string_error(parse_webp_metadata(reader), "WebP"),
        FileFormat::X509 => convert_string_error(parse_x509_metadata(reader), "X.509"),
        FileFormat::ICC => parse_icc_file(reader),
        FileFormat::XMP => parse_xmp_file(reader),
        FileFormat::XML => parse_xml_file(reader),
        FileFormat::EPS => convert_string_error(parse_eps_metadata(reader), "EPS"),
        FileFormat::VRD => parse_vrd_file(reader),
        FileFormat::DR4 => parse_dr4_file(reader),
        FileFormat::FPF => parse_fpf_metadata(reader),
        FileFormat::MIE => convert_string_error(parse_mie_metadata(reader), "MIE"),
        FileFormat::MOI => convert_string_error(parse_moi_metadata(reader), "MOI"),
        FileFormat::ITC => convert_string_error(parse_itc_metadata(reader), "ITC"),
        FileFormat::PCX => convert_string_error(parse_pcx_metadata(reader), "PCX"),
        FileFormat::PGF => convert_string_error(parse_pgf_metadata(reader), "PGF"),
        FileFormat::MRC => convert_string_error(parse_mrc_metadata(reader), "MRC"),
        FileFormat::AA => convert_string_error(parse_aa_metadata(reader), "AA"),
        _ => Err(ExifToolError::unsupported_format(format!(
            "Format {:?} not yet supported in this iteration",
            format
        ))),
    }
}

/// Converts a Result<T, String> to Result<T, ExifToolError> with a formatted parse error.
///
/// This helper function provides a consistent error conversion pattern for parsers
/// that return String errors.
///
/// # Arguments
///
/// * `result` - The result to convert
/// * `format_name` - The name of the format for error messages (e.g., "PNG", "QuickTime")
///
/// # Returns
///
/// * `Ok(T)` - The successful value
/// * `Err(ExifToolError)` - A parse error with the format name included
fn convert_string_error<T>(result: std::result::Result<T, String>, format_name: &str) -> Result<T> {
    result.map_err(|e| ExifToolError::parse_error(format!("{} parse error: {}", format_name, e)))
}
