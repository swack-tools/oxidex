//! Text-based format parsers

#![allow(dead_code)]

pub mod eps;
pub mod html;
pub mod txt;
pub mod vcf;

pub use eps::{EPSParser, parse_eps_metadata};
pub use html::{HTMLParser, parse_html_metadata};
pub use txt::{TXTParser, parse_txt_metadata};
pub use vcf::VCFParser;
