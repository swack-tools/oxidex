//! Leaf Lens Database
//!
//! This module is a compatibility shim for the consolidated lens data.
//! Prefer using `oxidex::parsers::tiff::makernotes::lens_data::leaf` directly.

#![allow(missing_docs)]

use super::lens_data::leaf;

pub fn lookup_leaf_lens(lens_id: u16) -> Option<String> {
    leaf::lookup(lens_id).map(|s| s.to_string())
}
