//! Archive format parsers

pub mod gz;
pub mod iso;
pub mod ole;
pub mod rar;
pub mod sevenz;
pub mod tar;
pub mod zip;

pub use gz::GZParser;
pub use iso::ISOParser;
pub use ole::OLEParser;
pub use rar::RARParser;
pub use sevenz::SevenZParser;
pub use tar::TARParser;
pub use zip::ZipParser;
