//! Fujifilm lens database
//!
//! This module is a compatibility shim for the consolidated lens data.
//! Prefer using `oxidex::parsers::tiff::makernotes::lens_data::fujifilm` directly.

#![allow(missing_docs)]

use super::lens_data::fujifilm;

pub fn lookup_lens_name(lens_id: u16) -> Option<String> {
    fujifilm::lookup(lens_id).map(|s| s.to_string())
}
