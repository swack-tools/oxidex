//! Olympus lens database
//!
//! This module is a compatibility shim for the consolidated lens data.
//! Prefer using `oxidex::parsers::tiff::makernotes::lens_data::olympus` directly.

#![allow(missing_docs)]

use super::lens_data::olympus;
pub use super::shared::{LensDatabase, StaticLensDb};

pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    olympus::lookup(lens_id).map(|s| s.to_string())
}

pub fn get_lens_database() -> &'static impl LensDatabase {
    &olympus::LENS_DB
}

pub fn parse_hex_lens_id(hex_str: &str) -> Option<u16> {
    olympus::parse_hex_lens_id(hex_str)
}
