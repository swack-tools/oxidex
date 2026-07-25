//! Version information analysis
//!
//! This module provides utilities for analyzing version information
//! extracted from Mach-O files, including OS versions, SDK versions,
//! and build tool versions.

use super::structures::{BuildVersionCommand, SourceVersionCommand, VersionMinCommand};

// =============================================================================
// Version Information
// =============================================================================

/// Consolidated version information from a Mach-O file
#[derive(Debug, Clone, Default)]
pub struct VersionInfo {
    /// Target platform name (macOS, iOS, etc.)
    pub platform: Option<String>,
    /// Minimum OS version required
    pub min_os_version: Option<String>,
    /// SDK version used to build
    pub sdk_version: Option<String>,
    /// Source version string
    pub source_version: Option<String>,
    /// Build tool versions (compiler, linker, etc.)
    pub build_tools: Vec<BuildToolInfo>,
}

/// Information about a build tool
#[derive(Debug, Clone)]
pub struct BuildToolInfo {
    /// Tool name (Clang, Swift, ld, lld)
    pub name: String,
    /// Tool version string
    pub version: String,
}

impl VersionInfo {
    /// Create version info from a version_min command
    pub fn from_version_min(cmd: &VersionMinCommand) -> Self {
        VersionInfo {
            platform: Some(cmd.platform_name().to_string()),
            min_os_version: Some(cmd.version_string()),
            sdk_version: Some(cmd.sdk_string()),
            ..Default::default()
        }
    }

    /// Create version info from a build_version command
    pub fn from_build_version(cmd: &BuildVersionCommand) -> Self {
        let build_tools = cmd
            .tools
            .iter()
            .map(|t| BuildToolInfo {
                name: t.tool_name().to_string(),
                version: t.version_string(),
            })
            .collect();

        VersionInfo {
            platform: Some(cmd.platform_name().to_string()),
            min_os_version: Some(cmd.minos_string()),
            sdk_version: Some(cmd.sdk_string()),
            build_tools,
            ..Default::default()
        }
    }

    /// Add source version information
    pub fn with_source_version(mut self, cmd: &SourceVersionCommand) -> Self {
        self.source_version = Some(cmd.version_string());
        self
    }

    /// Merge with another VersionInfo, preferring non-None values from self
    pub fn merge(&mut self, other: &VersionInfo) {
        if self.platform.is_none() {
            self.platform = other.platform.clone();
        }
        if self.min_os_version.is_none() {
            self.min_os_version = other.min_os_version.clone();
        }
        if self.sdk_version.is_none() {
            self.sdk_version = other.sdk_version.clone();
        }
        if self.source_version.is_none() {
            self.source_version = other.source_version.clone();
        }
        if self.build_tools.is_empty() {
            self.build_tools = other.build_tools.clone();
        }
    }
}

/// Parse a version number packed as X.Y.Z in a u32
///
/// Format: ((X << 16) | (Y << 8) | Z)
pub fn parse_version(version: u32) -> (u32, u32, u32) {
    let major = (version >> 16) & 0xFFFF;
    let minor = (version >> 8) & 0xFF;
    let patch = version & 0xFF;
    (major, minor, patch)
}

/// Parse a source version packed as A.B.C.D.E in a u64
///
/// Format: ((A << 40) | (B << 30) | (C << 20) | (D << 10) | E)
pub fn parse_source_version(version: u64) -> (u64, u64, u64, u64, u64) {
    let a = (version >> 40) & 0xFFFFFF;
    let b = (version >> 30) & 0x3FF;
    let c = (version >> 20) & 0x3FF;
    let d = (version >> 10) & 0x3FF;
    let e = version & 0x3FF;
    (a, b, c, d, e)
}

/// Compare two version strings (e.g., "10.15.0" vs "11.0.0")
///
/// Returns:
/// - `Ordering::Less` if v1 < v2
/// - `Ordering::Equal` if v1 == v2
/// - `Ordering::Greater` if v1 > v2
pub fn compare_versions(v1: &str, v2: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;

    let parse_parts =
        |v: &str| -> Vec<u32> { v.split('.').filter_map(|s| s.parse::<u32>().ok()).collect() };

    let parts1 = parse_parts(v1);
    let parts2 = parse_parts(v2);

    let max_len = parts1.len().max(parts2.len());

    for i in 0..max_len {
        let p1 = parts1.get(i).copied().unwrap_or(0);
        let p2 = parts2.get(i).copied().unwrap_or(0);

        match p1.cmp(&p2) {
            Ordering::Equal => continue,
            other => return other,
        }
    }

    Ordering::Equal
}

/// Check if a version meets a minimum requirement
pub fn meets_min_version(version: &str, min_version: &str) -> bool {
    compare_versions(version, min_version) != std::cmp::Ordering::Less
}

/// Known macOS version names
pub fn macos_version_name(version: &str) -> Option<&'static str> {
    let major_minor = version.split('.').take(2).collect::<Vec<_>>().join(".");

    match major_minor.as_str() {
        "10.0" => Some("Cheetah"),
        "10.1" => Some("Puma"),
        "10.2" => Some("Jaguar"),
        "10.3" => Some("Panther"),
        "10.4" => Some("Tiger"),
        "10.5" => Some("Leopard"),
        "10.6" => Some("Snow Leopard"),
        "10.7" => Some("Lion"),
        "10.8" => Some("Mountain Lion"),
        "10.9" => Some("Mavericks"),
        "10.10" => Some("Yosemite"),
        "10.11" => Some("El Capitan"),
        "10.12" => Some("Sierra"),
        "10.13" => Some("High Sierra"),
        "10.14" => Some("Mojave"),
        "10.15" => Some("Catalina"),
        "11.0" | "11" => Some("Big Sur"),
        "12.0" | "12" => Some("Monterey"),
        "13.0" | "13" => Some("Ventura"),
        "14.0" | "14" => Some("Sonoma"),
        "15.0" | "15" => Some("Sequoia"),
        _ => None,
    }
}

/// Known iOS version names (major releases)
pub fn ios_version_name(version: &str) -> Option<&'static str> {
    let major = version.split('.').next()?;

    match major {
        "1" => Some("iPhone OS 1"),
        "2" => Some("iPhone OS 2"),
        "3" => Some("iPhone OS 3"),
        "4" => Some("iOS 4"),
        "5" => Some("iOS 5"),
        "6" => Some("iOS 6"),
        "7" => Some("iOS 7"),
        "8" => Some("iOS 8"),
        "9" => Some("iOS 9"),
        "10" => Some("iOS 10"),
        "11" => Some("iOS 11"),
        "12" => Some("iOS 12"),
        "13" => Some("iOS 13"),
        "14" => Some("iOS 14"),
        "15" => Some("iOS 15"),
        "16" => Some("iOS 16"),
        "17" => Some("iOS 17"),
        "18" => Some("iOS 18"),
        _ => None,
    }
}

/// Format a version string with optional codename
pub fn format_version_with_name(platform: &str, version: &str) -> String {
    let name = match platform {
        "macOS" => macos_version_name(version),
        "iOS" | "iOS Simulator" => ios_version_name(version),
        _ => None,
    };

    match name {
        Some(codename) => format!("{} ({})", version, codename),
        None => version.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::macho::structures::load_command;

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version(0x000B0000), (11, 0, 0));
        assert_eq!(parse_version(0x000A0F00), (10, 15, 0));
        assert_eq!(parse_version(0x000C0102), (12, 1, 2));
    }

    #[test]
    fn test_parse_source_version() {
        let version: u64 = (1 << 40) | (2 << 30) | (3 << 20) | (4 << 10) | 5;
        assert_eq!(parse_source_version(version), (1, 2, 3, 4, 5));
    }

    #[test]
    fn test_compare_versions() {
        use std::cmp::Ordering;

        assert_eq!(compare_versions("10.15.0", "11.0.0"), Ordering::Less);
        assert_eq!(compare_versions("11.0.0", "10.15.0"), Ordering::Greater);
        assert_eq!(compare_versions("11.0.0", "11.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("11.0", "11.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("11.1.0", "11.0.1"), Ordering::Greater);
    }

    #[test]
    fn test_meets_min_version() {
        assert!(meets_min_version("11.0.0", "10.15.0"));
        assert!(meets_min_version("10.15.0", "10.15.0"));
        assert!(!meets_min_version("10.14.0", "10.15.0"));
    }

    #[test]
    fn test_macos_version_name() {
        assert_eq!(macos_version_name("10.15.0"), Some("Catalina"));
        assert_eq!(macos_version_name("11.0.0"), Some("Big Sur"));
        assert_eq!(macos_version_name("14.0"), Some("Sonoma"));
        assert_eq!(macos_version_name("99.0.0"), None);
    }

    #[test]
    fn test_ios_version_name() {
        assert_eq!(ios_version_name("15.0"), Some("iOS 15"));
        assert_eq!(ios_version_name("17.2.1"), Some("iOS 17"));
        assert_eq!(ios_version_name("99"), None);
    }

    #[test]
    fn test_format_version_with_name() {
        assert_eq!(
            format_version_with_name("macOS", "14.0.0"),
            "14.0.0 (Sonoma)"
        );
        assert_eq!(format_version_with_name("iOS", "17.0.0"), "17.0.0 (iOS 17)");
        assert_eq!(format_version_with_name("tvOS", "15.0.0"), "15.0.0");
    }

    #[test]
    fn test_version_info_from_version_min() {
        let cmd = VersionMinCommand {
            cmd: load_command::LC_VERSION_MIN_MACOSX,
            version: 0x000B0000, // 11.0.0
            sdk: 0x000C0100,     // 12.1.0
        };

        let info = VersionInfo::from_version_min(&cmd);
        assert_eq!(info.platform, Some("macOS".to_string()));
        assert_eq!(info.min_os_version, Some("11.0.0".to_string()));
        assert_eq!(info.sdk_version, Some("12.1.0".to_string()));
    }

    #[test]
    fn test_version_info_from_build_version() {
        use crate::parsers::macho::structures::{BuildToolVersion, build_tool, platform};

        let cmd = BuildVersionCommand {
            platform: platform::PLATFORM_MACOS,
            minos: 0x000E0000, // 14.0.0
            sdk: 0x000E0100,   // 14.1.0
            ntools: 2,
            tools: vec![
                BuildToolVersion {
                    tool: build_tool::TOOL_CLANG,
                    version: 0x000F0000,
                },
                BuildToolVersion {
                    tool: build_tool::TOOL_LD,
                    version: 0x03E80000,
                },
            ],
        };

        let info = VersionInfo::from_build_version(&cmd);
        assert_eq!(info.platform, Some("macOS".to_string()));
        assert_eq!(info.min_os_version, Some("14.0.0".to_string()));
        assert_eq!(info.build_tools.len(), 2);
        assert_eq!(info.build_tools[0].name, "Clang");
    }
}
