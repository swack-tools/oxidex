//! Command-line interface layer
//!
//! This module contains the CLI implementation using clap for argument parsing,
//! output formatting (JSON/CSV/human-readable), and batch file processing.

#![allow(dead_code)]

pub mod args;
pub mod batch_processor;
pub mod output_formatter;
pub mod rename;
