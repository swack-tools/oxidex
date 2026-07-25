//! Specialized format parsers
//!
//! Note: ELF parser has been moved to `crate::parsers::elf` for comprehensive
//! implementation following the PE parser pattern. The re-export here is for
//! backward compatibility. The same applies to Mach-O parser which has been
//! moved to `crate::parsers::macho`.

pub mod dwg;
pub mod dxf;
pub mod evtx;
pub mod fits;
pub mod gltf;
pub mod hdf5;
pub mod lnk;
pub mod obj;
pub mod pcap;
pub mod plist;
pub mod prefetch;
pub mod registry;
pub mod sqlite;
pub mod stl;
pub mod x509;

pub use dwg::DWGParser;
pub use dxf::DXFParser;
pub use evtx::EVTXParser;
pub use fits::FITSParser;
pub use gltf::GLTFParser;
pub use hdf5::HDF5Parser;
pub use lnk::LNKParser;
pub use obj::OBJParser;
pub use pcap::PCAPParser;
pub use plist::PlistParser;
pub use prefetch::PrefetchParser;
pub use registry::RegistryParser;
pub use sqlite::SQLiteParser;
pub use stl::STLParser;
pub use x509::X509Parser;

// Re-export ELF parser from the new comprehensive module
pub use crate::parsers::elf::ELFParser;

// Re-export Mach-O parser from the new comprehensive module
pub use crate::parsers::macho::MachOParser;
