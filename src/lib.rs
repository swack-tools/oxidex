//! ExifTool-RS: A modern, high-performance Rust reimplementation of ExifTool
//!
//! This library provides comprehensive metadata extraction, editing, and writing
//! capabilities for 300+ file formats. It follows a hexagonal architecture pattern
//! with clear separation between domain logic, application interfaces, and
//! infrastructure adapters.
//!
//! # Architecture
//!
//! The crate is organized into three main layers:
//!
//! - **Application Layer** (`cli`, `ffi`): User-facing interfaces (CLI, C FFI)
//! - **Domain Layer** (`core`): Format-agnostic metadata models and operations
//! - **Infrastructure Layer** (`parsers`, `writers`, `io`): Format-specific
//!   implementations and I/O abstraction
//!
//! # Example
//!
//! ```rust,ignore
//! use exiftool_rs::core::MetadataMap;
//!
//! // Future API example - not yet implemented
//! let metadata = MetadataMap::from_file("photo.jpg")?;
//! println!("Camera: {}", metadata.get("Make")?);
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]
#![allow(dead_code)] // Allow during initial development

// Application Layer
pub mod cli;
pub mod ffi;

// Domain Layer (Hexagonal Core)
pub mod core;

// Infrastructure Layer
pub mod io;
pub mod parsers;
pub mod writers;

// Supporting Modules
pub mod error;
pub mod tag_db;

/// Library version string
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
