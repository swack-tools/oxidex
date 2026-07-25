//! Dynamic library analysis
//!
//! This module provides utilities for analyzing dynamic library dependencies
//! extracted from Mach-O files.

use super::structures::{DylibCommand, load_command};

// =============================================================================
// Dylib Analysis
// =============================================================================

/// Statistics about dynamic libraries in a Mach-O file
#[derive(Debug, Clone, Default)]
pub struct DylibStats {
    /// Total number of linked dylibs
    pub dylib_count: usize,
    /// Number of regular (required) dylibs
    pub regular_count: usize,
    /// Number of weak dylibs (optional)
    pub weak_count: usize,
    /// Number of re-exported dylibs
    pub reexport_count: usize,
    /// Number of lazy-loaded dylibs
    pub lazy_count: usize,
    /// Number of upward dylibs
    pub upward_count: usize,
    /// The library's own ID (if this is a dylib)
    pub id_dylib: Option<String>,
}

impl DylibStats {
    /// Compute statistics from a list of dylib commands
    pub fn from_dylibs(dylibs: &[DylibCommand]) -> Self {
        let mut stats = DylibStats {
            dylib_count: dylibs.len(),
            ..Default::default()
        };

        for dylib in dylibs {
            match dylib.cmd {
                load_command::LC_LOAD_DYLIB => stats.regular_count += 1,
                load_command::LC_LOAD_WEAK_DYLIB => stats.weak_count += 1,
                load_command::LC_REEXPORT_DYLIB => stats.reexport_count += 1,
                load_command::LC_LAZY_LOAD_DYLIB => stats.lazy_count += 1,
                load_command::LC_LOAD_UPWARD_DYLIB => stats.upward_count += 1,
                load_command::LC_ID_DYLIB => {
                    stats.id_dylib = Some(dylib.name.clone());
                }
                _ => {}
            }
        }

        stats
    }
}

/// Categorizes a dylib by its load command type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DylibType {
    /// Regular required dylib (LC_LOAD_DYLIB)
    Regular,
    /// Weak/optional dylib (LC_LOAD_WEAK_DYLIB)
    Weak,
    /// Re-exported dylib (LC_REEXPORT_DYLIB)
    Reexport,
    /// Lazy-loaded dylib (LC_LAZY_LOAD_DYLIB)
    Lazy,
    /// Upward dylib (LC_LOAD_UPWARD_DYLIB)
    Upward,
    /// Dylib ID (LC_ID_DYLIB)
    Id,
    /// Unknown dylib type
    Unknown,
}

impl DylibType {
    /// Get the dylib type from a load command value
    pub fn from_cmd(cmd: u32) -> Self {
        match cmd {
            load_command::LC_LOAD_DYLIB => DylibType::Regular,
            load_command::LC_LOAD_WEAK_DYLIB => DylibType::Weak,
            load_command::LC_REEXPORT_DYLIB => DylibType::Reexport,
            load_command::LC_LAZY_LOAD_DYLIB => DylibType::Lazy,
            load_command::LC_LOAD_UPWARD_DYLIB => DylibType::Upward,
            load_command::LC_ID_DYLIB => DylibType::Id,
            _ => DylibType::Unknown,
        }
    }

    /// Get a human-readable name for the dylib type
    pub fn name(&self) -> &'static str {
        match self {
            DylibType::Regular => "Required",
            DylibType::Weak => "Weak (Optional)",
            DylibType::Reexport => "Re-exported",
            DylibType::Lazy => "Lazy-loaded",
            DylibType::Upward => "Upward",
            DylibType::Id => "Library ID",
            DylibType::Unknown => "Unknown",
        }
    }
}

/// Well-known system frameworks and their categories
pub mod known_frameworks {
    /// Core Apple frameworks that are typically present
    pub const CORE_FRAMEWORKS: &[&str] = &[
        "/System/Library/Frameworks/Foundation.framework",
        "/System/Library/Frameworks/CoreFoundation.framework",
        "/System/Library/Frameworks/Security.framework",
        "/System/Library/Frameworks/CoreServices.framework",
        "/System/Library/Frameworks/SystemConfiguration.framework",
    ];

    /// UI-related frameworks
    pub const UI_FRAMEWORKS: &[&str] = &[
        "/System/Library/Frameworks/AppKit.framework",
        "/System/Library/Frameworks/UIKit.framework",
        "/System/Library/Frameworks/SwiftUI.framework",
        "/System/Library/Frameworks/Cocoa.framework",
    ];

    /// Networking frameworks
    pub const NETWORK_FRAMEWORKS: &[&str] = &[
        "/System/Library/Frameworks/Network.framework",
        "/System/Library/Frameworks/CFNetwork.framework",
    ];

    /// Security-related frameworks
    pub const SECURITY_FRAMEWORKS: &[&str] = &[
        "/System/Library/Frameworks/Security.framework",
        "/System/Library/Frameworks/LocalAuthentication.framework",
        "/System/Library/Frameworks/CryptoKit.framework",
    ];
}

/// Categorize a dylib path
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DylibCategory {
    /// System library (in /usr/lib)
    SystemLibrary,
    /// System framework (in /System/Library/Frameworks)
    SystemFramework,
    /// Private framework (in /System/Library/PrivateFrameworks)
    PrivateFramework,
    /// User framework (using @rpath or @executable_path)
    UserFramework,
    /// Relative path using @loader_path
    LoaderRelative,
    /// Absolute path to user location
    UserAbsolute,
    /// Unknown or unrecognized path type
    Unknown,
}

impl DylibCategory {
    /// Categorize a dylib path
    pub fn from_path(path: &str) -> Self {
        if path.starts_with("/usr/lib/") {
            DylibCategory::SystemLibrary
        } else if path.starts_with("/System/Library/Frameworks/") {
            DylibCategory::SystemFramework
        } else if path.starts_with("/System/Library/PrivateFrameworks/") {
            DylibCategory::PrivateFramework
        } else if path.starts_with("@rpath/") || path.starts_with("@executable_path/") {
            DylibCategory::UserFramework
        } else if path.starts_with("@loader_path/") {
            DylibCategory::LoaderRelative
        } else if path.starts_with('/') {
            DylibCategory::UserAbsolute
        } else {
            DylibCategory::Unknown
        }
    }

    /// Get a human-readable name for the category
    pub fn name(&self) -> &'static str {
        match self {
            DylibCategory::SystemLibrary => "System Library",
            DylibCategory::SystemFramework => "System Framework",
            DylibCategory::PrivateFramework => "Private Framework",
            DylibCategory::UserFramework => "User Framework",
            DylibCategory::LoaderRelative => "Loader Relative",
            DylibCategory::UserAbsolute => "User Absolute",
            DylibCategory::Unknown => "Unknown",
        }
    }
}

/// Extract just the library name from a full path
///
/// Examples:
/// - "/usr/lib/libSystem.B.dylib" -> "libSystem.B.dylib"
/// - "@rpath/MyFramework.framework/MyFramework" -> "MyFramework"
/// - "/System/Library/Frameworks/Foundation.framework/Foundation" -> "Foundation"
pub fn extract_library_name(path: &str) -> String {
    // Handle framework paths
    if path.contains(".framework/")
        && let Some(pos) = path.rfind(".framework/")
    {
        let framework_name = &path[pos + 11..]; // Skip ".framework/"
        return framework_name.to_string();
    }

    // Handle regular library paths
    if let Some(pos) = path.rfind('/') {
        return path[pos + 1..].to_string();
    }

    path.to_string()
}

/// Get all dylib paths as a list
pub fn get_dylib_paths(dylibs: &[DylibCommand]) -> Vec<String> {
    dylibs
        .iter()
        .filter(|d| d.cmd != load_command::LC_ID_DYLIB)
        .map(|d| d.name.clone())
        .collect()
}

/// Get all dylib names (not full paths)
pub fn get_dylib_names(dylibs: &[DylibCommand]) -> Vec<String> {
    dylibs
        .iter()
        .filter(|d| d.cmd != load_command::LC_ID_DYLIB)
        .map(|d| extract_library_name(&d.name))
        .collect()
}

/// Check if a dylib is a system library
pub fn is_system_dylib(path: &str) -> bool {
    matches!(
        DylibCategory::from_path(path),
        DylibCategory::SystemLibrary
            | DylibCategory::SystemFramework
            | DylibCategory::PrivateFramework
    )
}

/// Categorize dylibs by their load type
pub fn categorize_dylibs(
    dylibs: &[DylibCommand],
) -> std::collections::HashMap<DylibType, Vec<String>> {
    use std::collections::HashMap;
    let mut categories: HashMap<DylibType, Vec<String>> = HashMap::new();

    for dylib in dylibs {
        let dtype = DylibType::from_cmd(dylib.cmd);
        categories
            .entry(dtype)
            .or_default()
            .push(dylib.name.clone());
    }

    categories
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_dylib(cmd: u32, name: &str) -> DylibCommand {
        DylibCommand {
            cmd,
            name: name.to_string(),
            timestamp: 0,
            current_version: 0x00010000,
            compatibility_version: 0x00010000,
        }
    }

    #[test]
    fn test_dylib_stats() {
        let dylibs = vec![
            create_test_dylib(load_command::LC_LOAD_DYLIB, "/usr/lib/libSystem.B.dylib"),
            create_test_dylib(load_command::LC_LOAD_DYLIB, "/usr/lib/libc++.1.dylib"),
            create_test_dylib(
                load_command::LC_LOAD_WEAK_DYLIB,
                "/usr/lib/libOptional.dylib",
            ),
            create_test_dylib(
                load_command::LC_REEXPORT_DYLIB,
                "/usr/lib/libReexport.dylib",
            ),
            create_test_dylib(load_command::LC_ID_DYLIB, "MyLibrary.dylib"),
        ];

        let stats = DylibStats::from_dylibs(&dylibs);
        assert_eq!(stats.dylib_count, 5);
        assert_eq!(stats.regular_count, 2);
        assert_eq!(stats.weak_count, 1);
        assert_eq!(stats.reexport_count, 1);
        assert_eq!(stats.id_dylib, Some("MyLibrary.dylib".to_string()));
    }

    #[test]
    fn test_dylib_type() {
        assert_eq!(
            DylibType::from_cmd(load_command::LC_LOAD_DYLIB),
            DylibType::Regular
        );
        assert_eq!(
            DylibType::from_cmd(load_command::LC_LOAD_WEAK_DYLIB),
            DylibType::Weak
        );
        assert_eq!(DylibType::from_cmd(0xFFFF), DylibType::Unknown);
    }

    #[test]
    fn test_dylib_category() {
        assert_eq!(
            DylibCategory::from_path("/usr/lib/libSystem.B.dylib"),
            DylibCategory::SystemLibrary
        );
        assert_eq!(
            DylibCategory::from_path("/System/Library/Frameworks/Foundation.framework/Foundation"),
            DylibCategory::SystemFramework
        );
        assert_eq!(
            DylibCategory::from_path("@rpath/MyFramework.framework/MyFramework"),
            DylibCategory::UserFramework
        );
        assert_eq!(
            DylibCategory::from_path("@loader_path/../Frameworks/Foo.framework/Foo"),
            DylibCategory::LoaderRelative
        );
    }

    #[test]
    fn test_extract_library_name() {
        assert_eq!(
            extract_library_name("/usr/lib/libSystem.B.dylib"),
            "libSystem.B.dylib"
        );
        assert_eq!(
            extract_library_name("/System/Library/Frameworks/Foundation.framework/Foundation"),
            "Foundation"
        );
        assert_eq!(
            extract_library_name("@rpath/MyFramework.framework/MyFramework"),
            "MyFramework"
        );
        assert_eq!(extract_library_name("libfoo.dylib"), "libfoo.dylib");
    }

    #[test]
    fn test_is_system_dylib() {
        assert!(is_system_dylib("/usr/lib/libSystem.B.dylib"));
        assert!(is_system_dylib(
            "/System/Library/Frameworks/Foundation.framework/Foundation"
        ));
        assert!(is_system_dylib(
            "/System/Library/PrivateFrameworks/Foo.framework/Foo"
        ));
        assert!(!is_system_dylib("@rpath/MyFramework.framework/MyFramework"));
        assert!(!is_system_dylib("/opt/local/lib/libfoo.dylib"));
    }

    #[test]
    fn test_get_dylib_paths() {
        let dylibs = vec![
            create_test_dylib(load_command::LC_LOAD_DYLIB, "/usr/lib/libSystem.B.dylib"),
            create_test_dylib(load_command::LC_ID_DYLIB, "MyLibrary.dylib"),
            create_test_dylib(load_command::LC_LOAD_DYLIB, "/usr/lib/libc++.1.dylib"),
        ];

        let paths = get_dylib_paths(&dylibs);
        assert_eq!(paths.len(), 2); // ID_DYLIB should be filtered out
        assert!(paths.contains(&"/usr/lib/libSystem.B.dylib".to_string()));
        assert!(paths.contains(&"/usr/lib/libc++.1.dylib".to_string()));
    }
}
