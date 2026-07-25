//! Phase One lens database
//!
//! This module is a compatibility shim for the consolidated lens data.
//! Prefer using `oxidex::parsers::tiff::makernotes::lens_data::phaseone` directly.

#![allow(missing_docs)]

use super::lens_data::phaseone;

pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    phaseone::lookup(lens_id).map(|s| s.to_string())
}
