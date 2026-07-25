//! Minolta Lens Database
//!
//! This module is a compatibility shim for the consolidated lens data.
//! Prefer using `oxidex::parsers::tiff::makernotes::lens_data::minolta` directly.

#![allow(missing_docs)]

use super::lens_data::minolta;
pub use super::shared::{LensDatabase, StaticLensDb};

pub fn lookup_minolta_lens(lens_id: u16) -> Option<String> {
    minolta::lookup(lens_id).map(|s| s.to_string())
}

pub fn get_lens_database() -> &'static impl LensDatabase {
    &minolta::LENS_DB
}
