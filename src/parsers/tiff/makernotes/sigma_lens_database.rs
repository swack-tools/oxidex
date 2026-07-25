//! Sigma lens database
//!
//! This module is a compatibility shim for the consolidated lens data.
//! Prefer using `oxidex::parsers::tiff::makernotes::lens_data::sigma` directly.

#![allow(missing_docs)]

use super::lens_data::sigma;
pub use super::shared::{LensDatabase, StaticLensDb};

pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    sigma::lookup(lens_id).map(|s| s.to_string())
}

pub fn get_lens_database() -> &'static impl LensDatabase {
    &sigma::LENS_DB
}
