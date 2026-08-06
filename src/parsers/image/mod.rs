//! Advanced image format parsers

pub mod avif;
pub mod bmp;
pub mod bpg;
pub mod embedded;
pub mod exr;
pub mod flif;
pub mod gif;
pub mod heif;
pub mod ico;
pub mod jxl;
pub mod miff;
pub mod pfm;
pub mod psd;
pub mod svg;
pub mod webp;

pub use avif::AVIFParser;
pub use bmp::BMPParser;
pub use bpg::BPGParser;
pub use exr::EXRParser;
pub use flif::FLIFParser;
pub use gif::GIFParser;
pub use heif::HEIFParser;
pub use ico::ICOParser;
pub use jxl::JXLParser;
pub use pfm::PFMParser;
pub use psd::PSDParser;
pub use svg::SVGParser;
pub use webp::WebPParser;
