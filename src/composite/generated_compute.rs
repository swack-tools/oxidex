//! Composite conversions compiled from ExifTool's Perl ValueConv/PrintConv
//! text by codegen_composite.py's `$val[N]` grammar compiler
//! (`tools/exiftool-tables/exprs.py`'s `compile_composite`).
//!
//! DO NOT EDIT. Regenerate with `just regen-tables`.
//!
//! This is the automatic sibling of `compute.rs`: a Composite whose
//! ValueConv (and, if present, PrintConv) is pure arithmetic over `$val[N]`
//! (ExifTool's `@val` array) -- or, for a single-input composite, the bare
//! `$val` ExifTool.pm:3611-3612 aliases to `$val[0]` -- compiles here with
//! zero hand-written code. Anything outside that closed grammar is refused
//! and counted by this generator's own "no registered computation" triage
//! line instead of being approximated, the same rule `compute.rs`'s
//! hand-written match follows. `compute::compute` only ever consults this
//! file for a `(module, name)` pair it has no arm of its own for.
#![allow(unused_parens)] // exprs.py's compiled arithmetic is fully
// parenthesised by construction (see exprs.py's own _mk_binop et al.); that
// is correct and deliberate, not something worth post-processing away, the
// same call binary_tables.rs already makes for the same reason.
use super::compute::{Computed, Inputs, f, get};

pub(super) fn compute_generated(module: &str, name: &str, i: Inputs) -> Option<Computed> {
    match (module, name) {
        ("FLIR", "PeakSpectralSensitivity") => {
            let v0 = f(get(i, 0))?;
            let value: f64 = { ((14387.6515_f64) / (v0)) };
            Some(Computed {
                value: crate::exiftool_tables::exprs::perl_num(value),
                print: format!("{:.1} um", value),
            })
        }
        ("PanasonicRaw", "ImageHeight") => {
            let v0 = f(get(i, 0))?;
            let v1 = f(get(i, 1))?;
            let value: f64 = { ((v1) - (v0)) };
            Some(Computed {
                value: crate::exiftool_tables::exprs::perl_num(value),
                print: crate::exiftool_tables::exprs::perl_num(value),
            })
        }
        ("PanasonicRaw", "ImageWidth") => {
            let v0 = f(get(i, 0))?;
            let v1 = f(get(i, 1))?;
            let value: f64 = { ((v1) - (v0)) };
            Some(Computed {
                value: crate::exiftool_tables::exprs::perl_num(value),
                print: crate::exiftool_tables::exprs::perl_num(value),
            })
        }
        _ => None,
    }
}
