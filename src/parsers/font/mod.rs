//! Font format parsers

mod mac_charset;
pub mod otf;
pub mod pfm;
pub mod ttf;
pub mod woff;
pub mod woff2;

pub use otf::OTFParser;
pub use pfm::PrinterFontMetricsParser;
pub use ttf::TTFParser;
pub use woff::WOFFParser;
pub use woff2::WOFF2Parser;
