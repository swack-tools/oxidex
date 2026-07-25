//! Canon lens database
//!
//! This module is a compatibility shim for the consolidated lens data.
//! Prefer using `oxidex::parsers::tiff::makernotes::lens_data::canon` directly.

#![allow(missing_docs)]

use super::lens_data::canon;
pub use super::shared::{LensDatabase, StaticLensDb};

pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    canon::lookup(lens_id).map(|s| s.to_string())
}

pub fn get_lens_database() -> &'static impl LensDatabase {
    &canon::LENS_DB
}
