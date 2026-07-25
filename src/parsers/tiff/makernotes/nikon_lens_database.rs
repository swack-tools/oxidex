//! Nikon lens database
//!
//! This module is a compatibility shim for the consolidated lens data.
//! Prefer using `oxidex::parsers::tiff::makernotes::lens_data::nikon` directly.

#![allow(missing_docs)]

use super::lens_data::nikon;
pub use super::shared::{LensDatabase, StaticLensDb};

pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    nikon::lookup(lens_id).map(|s| s.to_string())
}

pub fn get_lens_database() -> &'static impl LensDatabase {
    &nikon::LENS_DB
}
