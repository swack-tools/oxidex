//! Metadata extraction from Mach-O files
//!
//! This module orchestrates the extraction of metadata from Mach-O files,
//! converting parsed structures into a unified MetadataMap.

use crate::core::{MetadataMap, TagValue};

use super::dylib_parser::{DylibStats, get_dylib_names, get_dylib_paths};
use super::load_command_parser::LoadCommand;
use super::segment_parser::SegmentStats;
use super::signature_parser::{has_developer_id, is_adhoc_signed};
use super::structures::{
    BuildVersionCommand, DylibCommand, EntryPointCommand, MachHeader, MachOInfo, RpathCommand,
    SymtabCommand, UuidCommand, VersionMinCommand,
};
use super::symbol_parser::SymbolStats;
use super::version_parser::format_version_with_name;

// =============================================================================
// Metadata Extraction
// =============================================================================

/// Extract metadata from a parsed MachOInfo structure
pub fn extract_macho_metadata(info: &MachOInfo) -> MetadataMap {
    let mut metadata = MetadataMap::new();

    // Basic file type indicator
    metadata.insert(
        "EXE:FileFormat".to_string(),
        TagValue::String("Mach-O".to_string()),
    );

    // Extract header metadata
    if let Some(ref header) = info.header {
        extract_header_metadata(header, &mut metadata);
    }

    // Extract segment metadata
    if !info.segments.is_empty() {
        extract_segment_metadata(&info.segments, &mut metadata);
    }

    // Extract dylib metadata
    if !info.dylibs.is_empty() {
        extract_dylib_metadata(&info.dylibs, &mut metadata);
    }

    // Extract UUID
    if let Some(ref uuid) = info.uuid {
        extract_uuid_metadata(uuid, &mut metadata);
    }

    // Extract version info
    extract_version_metadata(info, &mut metadata);

    // Extract entry point
    if let Some(ref entry) = info.entry_point {
        extract_entry_point_metadata(entry, &mut metadata);
    }

    // Extract symbol table info
    if let Some(ref symtab) = info.symtab {
        extract_symbol_metadata(symtab, info.dysymtab.as_ref(), &mut metadata);
    }

    // Extract rpath info
    if !info.rpaths.is_empty() {
        extract_rpath_metadata(&info.rpaths, &mut metadata);
    }

    // Extract encryption info
    if let Some(ref enc) = info.encryption_info {
        extract_encryption_metadata(enc, &mut metadata);
    }

    // Extract code signature info
    if let Some(ref cs_info) = info.code_signature_info {
        extract_code_signature_metadata(cs_info, &mut metadata);
    }

    // Extract FAT binary info
    if info.is_from_fat {
        extract_fat_metadata(info, &mut metadata);
    }

    metadata
}

/// Extract metadata from the Mach-O header
fn extract_header_metadata(header: &MachHeader, metadata: &mut MetadataMap) {
    // CPU type
    metadata.insert(
        "EXE:CPUType".to_string(),
        TagValue::String(header.cpu_type_name().to_string()),
    );

    // CPU type raw value
    metadata.insert(
        "EXE:CPUTypeRaw".to_string(),
        TagValue::Integer(header.cputype as i64),
    );

    // CPU subtype
    metadata.insert(
        "EXE:CPUSubtype".to_string(),
        TagValue::String(header.cpu_subtype_name()),
    );

    // CPU subtype raw value
    metadata.insert(
        "EXE:CPUSubtypeRaw".to_string(),
        TagValue::Integer(header.cpusubtype as i64),
    );

    // File type
    metadata.insert(
        "EXE:FileType".to_string(),
        TagValue::String(header.file_type_name().to_string()),
    );

    // File type raw value
    metadata.insert(
        "EXE:FileTypeRaw".to_string(),
        TagValue::Integer(header.filetype as i64),
    );

    // Is 64-bit
    metadata.insert(
        "EXE:Is64Bit".to_string(),
        TagValue::Integer(if header.is_64bit { 1 } else { 0 }),
    );

    // Byte order
    metadata.insert(
        "EXE:ByteOrder".to_string(),
        TagValue::String(
            if header.is_swapped {
                "Little Endian"
            } else {
                "Big Endian"
            }
            .to_string(),
        ),
    );

    // Is byte swapped (relative to original PPC big-endian format)
    metadata.insert(
        "EXE:IsByteSwapped".to_string(),
        TagValue::Integer(if header.is_swapped { 1 } else { 0 }),
    );

    // Number of load commands
    metadata.insert(
        "EXE:LoadCommandCount".to_string(),
        TagValue::Integer(header.ncmds as i64),
    );

    // Size of load commands
    metadata.insert(
        "EXE:LoadCommandsSize".to_string(),
        TagValue::Integer(header.sizeofcmds as i64),
    );

    // Flags raw value
    metadata.insert(
        "EXE:Flags".to_string(),
        TagValue::Integer(header.flags as i64),
    );

    // Flags decoded
    let flag_names = header.flag_names();
    if !flag_names.is_empty() {
        metadata.insert(
            "EXE:FlagsDecoded".to_string(),
            TagValue::String(flag_names.join(", ")),
        );
    }

    // Specific flag indicators
    metadata.insert(
        "EXE:IsPIE".to_string(),
        TagValue::Integer(if header.flags & super::structures::flags::MH_PIE != 0 {
            1
        } else {
            0
        }),
    );

    metadata.insert(
        "EXE:HasTwoLevelNamespace".to_string(),
        TagValue::Integer(
            if header.flags & super::structures::flags::MH_TWOLEVEL != 0 {
                1
            } else {
                0
            },
        ),
    );

    metadata.insert(
        "EXE:AllowStackExecution".to_string(),
        TagValue::Integer(
            if header.flags & super::structures::flags::MH_ALLOW_STACK_EXECUTION != 0 {
                1
            } else {
                0
            },
        ),
    );
}

/// Extract segment-related metadata
fn extract_segment_metadata(
    segments: &[super::structures::SegmentCommand],
    metadata: &mut MetadataMap,
) {
    let stats = SegmentStats::from_segments(segments);

    metadata.insert(
        "EXE:SegmentCount".to_string(),
        TagValue::Integer(stats.segment_count as i64),
    );

    metadata.insert(
        "EXE:SectionCount".to_string(),
        TagValue::Integer(stats.section_count as i64),
    );

    if stats.text_size > 0 {
        metadata.insert(
            "EXE:TextSegmentSize".to_string(),
            TagValue::Integer(stats.text_size as i64),
        );
    }

    if stats.data_size > 0 {
        metadata.insert(
            "EXE:DataSegmentSize".to_string(),
            TagValue::Integer(stats.data_size as i64),
        );
    }

    if stats.linkedit_size > 0 {
        metadata.insert(
            "EXE:LinkeditSize".to_string(),
            TagValue::Integer(stats.linkedit_size as i64),
        );
    }

    metadata.insert(
        "EXE:TotalVMSize".to_string(),
        TagValue::Integer(stats.total_vmsize as i64),
    );

    metadata.insert(
        "EXE:HasPagezero".to_string(),
        TagValue::Integer(if stats.has_pagezero { 1 } else { 0 }),
    );

    // List segment names
    let segment_names: Vec<String> = segments.iter().map(|s| s.segname.clone()).collect();
    metadata.insert(
        "EXE:SegmentNames".to_string(),
        TagValue::String(segment_names.join(", ")),
    );
}

/// Extract dylib-related metadata
fn extract_dylib_metadata(dylibs: &[DylibCommand], metadata: &mut MetadataMap) {
    let stats = DylibStats::from_dylibs(dylibs);

    // Total dylib count (excluding ID_DYLIB)
    let load_count = stats.regular_count
        + stats.weak_count
        + stats.reexport_count
        + stats.lazy_count
        + stats.upward_count;
    metadata.insert(
        "EXE:DylibCount".to_string(),
        TagValue::Integer(load_count as i64),
    );

    if stats.weak_count > 0 {
        metadata.insert(
            "EXE:WeakDylibCount".to_string(),
            TagValue::Integer(stats.weak_count as i64),
        );
    }

    if stats.reexport_count > 0 {
        metadata.insert(
            "EXE:ReexportDylibCount".to_string(),
            TagValue::Integer(stats.reexport_count as i64),
        );
    }

    // Library ID (if this is a dylib)
    if let Some(ref id) = stats.id_dylib {
        metadata.insert("EXE:DylibID".to_string(), TagValue::String(id.clone()));
    }

    // List of dylib paths (limited)
    let paths = get_dylib_paths(dylibs);
    if !paths.is_empty() {
        let paths_str = if paths.len() <= 20 {
            paths.join(", ")
        } else {
            format!(
                "{}, ... ({} more)",
                paths[..20].join(", "),
                paths.len() - 20
            )
        };
        metadata.insert("EXE:DylibPaths".to_string(), TagValue::String(paths_str));
    }

    // List of dylib names (shorter)
    let names = get_dylib_names(dylibs);
    if !names.is_empty() {
        let names_str = if names.len() <= 30 {
            names.join(", ")
        } else {
            format!(
                "{}, ... ({} more)",
                names[..30].join(", "),
                names.len() - 30
            )
        };
        metadata.insert("EXE:DylibNames".to_string(), TagValue::String(names_str));
    }

    // Extract version info from first dylib (if this is a dylib)
    if let Some(dylib) = dylibs
        .iter()
        .find(|d| d.cmd == super::structures::load_command::LC_ID_DYLIB)
    {
        metadata.insert(
            "EXE:DylibCurrentVersion".to_string(),
            TagValue::String(dylib.current_version_string()),
        );
        metadata.insert(
            "EXE:DylibCompatVersion".to_string(),
            TagValue::String(dylib.compatibility_version_string()),
        );
    }
}

/// Extract UUID metadata
fn extract_uuid_metadata(uuid: &UuidCommand, metadata: &mut MetadataMap) {
    metadata.insert("EXE:UUID".to_string(), TagValue::String(uuid.uuid_string()));
}

/// Extract version-related metadata
fn extract_version_metadata(info: &MachOInfo, metadata: &mut MetadataMap) {
    // Prefer build_version over version_min
    if let Some(ref bv) = info.build_version {
        extract_build_version_metadata(bv, metadata);
    } else if let Some(ref vm) = info.version_min {
        extract_version_min_metadata(vm, metadata);
    }

    // Source version
    if let Some(ref sv) = info.source_version {
        metadata.insert(
            "EXE:SourceVersion".to_string(),
            TagValue::String(sv.version_string()),
        );
    }
}

/// Extract version_min command metadata
fn extract_version_min_metadata(vm: &VersionMinCommand, metadata: &mut MetadataMap) {
    let platform = vm.platform_name();
    let version = vm.version_string();

    metadata.insert(
        "EXE:Platform".to_string(),
        TagValue::String(platform.to_string()),
    );

    metadata.insert(
        "EXE:MinOSVersion".to_string(),
        TagValue::String(format_version_with_name(platform, &version)),
    );

    metadata.insert(
        "EXE:SDKVersion".to_string(),
        TagValue::String(vm.sdk_string()),
    );
}

/// Extract build_version command metadata
fn extract_build_version_metadata(bv: &BuildVersionCommand, metadata: &mut MetadataMap) {
    let platform = bv.platform_name();
    let version = bv.minos_string();

    metadata.insert(
        "EXE:Platform".to_string(),
        TagValue::String(platform.to_string()),
    );

    metadata.insert(
        "EXE:MinOSVersion".to_string(),
        TagValue::String(format_version_with_name(platform, &version)),
    );

    metadata.insert(
        "EXE:SDKVersion".to_string(),
        TagValue::String(bv.sdk_string()),
    );

    // Build tools
    if !bv.tools.is_empty() {
        let tools_str = bv
            .tools
            .iter()
            .map(|t| format!("{} {}", t.tool_name(), t.version_string()))
            .collect::<Vec<_>>()
            .join(", ");
        metadata.insert("EXE:BuildTools".to_string(), TagValue::String(tools_str));
    }
}

/// Extract entry point metadata
fn extract_entry_point_metadata(entry: &EntryPointCommand, metadata: &mut MetadataMap) {
    metadata.insert(
        "EXE:EntryPointOffset".to_string(),
        TagValue::Integer(entry.entryoff as i64),
    );

    if entry.stacksize > 0 {
        metadata.insert(
            "EXE:StackSize".to_string(),
            TagValue::Integer(entry.stacksize as i64),
        );
    }
}

/// Extract symbol table metadata
fn extract_symbol_metadata(
    symtab: &SymtabCommand,
    dysymtab: Option<&super::structures::DysymtabCommand>,
    metadata: &mut MetadataMap,
) {
    let stats = SymbolStats::from_commands(symtab, dysymtab);

    metadata.insert(
        "EXE:SymbolCount".to_string(),
        TagValue::Integer(stats.total_symbols as i64),
    );

    if stats.local_symbols > 0 {
        metadata.insert(
            "EXE:LocalSymbolCount".to_string(),
            TagValue::Integer(stats.local_symbols as i64),
        );
    }

    if stats.external_symbols > 0 {
        metadata.insert(
            "EXE:ExportedSymbolCount".to_string(),
            TagValue::Integer(stats.external_symbols as i64),
        );
    }

    if stats.undefined_symbols > 0 {
        metadata.insert(
            "EXE:ImportedSymbolCount".to_string(),
            TagValue::Integer(stats.undefined_symbols as i64),
        );
    }

    metadata.insert(
        "EXE:StringTableSize".to_string(),
        TagValue::Integer(stats.string_table_size as i64),
    );
}

/// Extract rpath metadata
fn extract_rpath_metadata(rpaths: &[RpathCommand], metadata: &mut MetadataMap) {
    metadata.insert(
        "EXE:RPathCount".to_string(),
        TagValue::Integer(rpaths.len() as i64),
    );

    let rpath_strs: Vec<String> = rpaths.iter().map(|r| r.path.clone()).collect();
    metadata.insert(
        "EXE:RPaths".to_string(),
        TagValue::String(rpath_strs.join(", ")),
    );
}

/// Extract encryption info metadata
fn extract_encryption_metadata(
    enc: &super::structures::EncryptionInfoCommand,
    metadata: &mut MetadataMap,
) {
    metadata.insert(
        "EXE:IsEncrypted".to_string(),
        TagValue::Integer(if enc.cryptid != 0 { 1 } else { 0 }),
    );

    if enc.cryptid != 0 {
        metadata.insert(
            "EXE:EncryptionType".to_string(),
            TagValue::Integer(enc.cryptid as i64),
        );
        metadata.insert(
            "EXE:EncryptedOffset".to_string(),
            TagValue::Integer(enc.cryptoff as i64),
        );
        metadata.insert(
            "EXE:EncryptedSize".to_string(),
            TagValue::Integer(enc.cryptsize as i64),
        );
    }
}

/// Extract code signature metadata
fn extract_code_signature_metadata(
    cs_info: &super::structures::CodeSignatureInfo,
    metadata: &mut MetadataMap,
) {
    metadata.insert(
        "EXE:IsSigned".to_string(),
        TagValue::Integer(if cs_info.is_signed { 1 } else { 0 }),
    );

    metadata.insert(
        "EXE:CodeSignatureSize".to_string(),
        TagValue::Integer(cs_info.signature_size as i64),
    );

    if let Some(ref ident) = cs_info.identifier {
        metadata.insert(
            "EXE:CodeSignatureID".to_string(),
            TagValue::String(ident.clone()),
        );
    }

    if let Some(ref team) = cs_info.team_id {
        metadata.insert(
            "EXE:TeamIdentifier".to_string(),
            TagValue::String(team.clone()),
        );
    }

    if let Some(ref hash) = cs_info.hash_type {
        metadata.insert(
            "EXE:CodeSignatureHashType".to_string(),
            TagValue::String(hash.clone()),
        );
    }

    metadata.insert(
        "EXE:HasCMSSignature".to_string(),
        TagValue::Integer(if cs_info.has_cms_signature { 1 } else { 0 }),
    );

    metadata.insert(
        "EXE:IsAdHocSigned".to_string(),
        TagValue::Integer(if is_adhoc_signed(cs_info) { 1 } else { 0 }),
    );

    metadata.insert(
        "EXE:HasDeveloperID".to_string(),
        TagValue::Integer(if has_developer_id(cs_info) { 1 } else { 0 }),
    );

    if let Some(ref signer) = cs_info.signer_name {
        metadata.insert(
            "EXE:SignerName".to_string(),
            TagValue::String(signer.clone()),
        );
    }

    if cs_info.n_code_slots > 0 {
        metadata.insert(
            "EXE:CodeSlotCount".to_string(),
            TagValue::Integer(cs_info.n_code_slots as i64),
        );
    }

    // Entitlements
    metadata.insert(
        "EXE:HasEntitlements".to_string(),
        TagValue::Integer(if cs_info.has_entitlements { 1 } else { 0 }),
    );

    if !cs_info.entitlement_keys.is_empty() {
        metadata.insert(
            "EXE:EntitlementKeys".to_string(),
            TagValue::String(cs_info.entitlement_keys.join(", ")),
        );
        metadata.insert(
            "EXE:EntitlementCount".to_string(),
            TagValue::Integer(cs_info.entitlement_keys.len() as i64),
        );
    }
}

/// Extract FAT/Universal binary metadata
fn extract_fat_metadata(info: &MachOInfo, metadata: &mut MetadataMap) {
    metadata.insert(
        "EXE:IsFromUniversalBinary".to_string(),
        TagValue::Integer(1),
    );

    if let Some(ref fat) = info.fat_header {
        metadata.insert(
            "EXE:UniversalArchCount".to_string(),
            TagValue::Integer(fat.nfat_arch as i64),
        );
    }

    if !info.fat_archs.is_empty() {
        let arch_names: Vec<String> = info
            .fat_archs
            .iter()
            .map(|a| a.cpu_type_name().to_string())
            .collect();
        metadata.insert(
            "EXE:UniversalArchitectures".to_string(),
            TagValue::String(arch_names.join(", ")),
        );
    }

    metadata.insert(
        "EXE:ArchitectureIndex".to_string(),
        TagValue::Integer(info.fat_arch_index as i64),
    );
}

// =============================================================================
// Helper: Populate MachOInfo from Load Commands
// =============================================================================

/// Populate a MachOInfo structure from parsed load commands
pub fn populate_macho_info(info: &mut MachOInfo, commands: &[LoadCommand]) {
    for cmd in commands {
        match cmd {
            LoadCommand::Segment(seg) => {
                info.segments.push(seg.clone());
            }
            LoadCommand::Dylib(dylib) => {
                info.dylibs.push(dylib.clone());
            }
            LoadCommand::Uuid(uuid) => {
                info.uuid = Some(uuid.clone());
            }
            LoadCommand::VersionMin(vm) => {
                info.version_min = Some(vm.clone());
            }
            LoadCommand::BuildVersion(bv) => {
                info.build_version = Some(bv.clone());
            }
            LoadCommand::SourceVersion(sv) => {
                info.source_version = Some(sv.clone());
            }
            LoadCommand::EntryPoint(ep) => {
                info.entry_point = Some(ep.clone());
            }
            LoadCommand::Symtab(st) => {
                info.symtab = Some(st.clone());
            }
            LoadCommand::Dysymtab(dst) => {
                info.dysymtab = Some(dst.clone());
            }
            LoadCommand::LinkeditData(ld) => match ld.cmd {
                super::structures::load_command::LC_CODE_SIGNATURE => {
                    info.code_signature = Some(ld.clone());
                }
                super::structures::load_command::LC_FUNCTION_STARTS => {
                    info.function_starts = Some(ld.clone());
                }
                super::structures::load_command::LC_DATA_IN_CODE => {
                    info.data_in_code = Some(ld.clone());
                }
                super::structures::load_command::LC_DYLD_INFO
                | super::structures::load_command::LC_DYLD_INFO_ONLY => {
                    info.dyld_info = Some(ld.clone());
                }
                _ => {}
            },
            LoadCommand::Rpath(rp) => {
                info.rpaths.push(rp.clone());
            }
            LoadCommand::EncryptionInfo(enc) => {
                info.encryption_info = Some(enc.clone());
            }
            LoadCommand::Unknown(_) => {
                // Skip unknown commands
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::macho::structures::{cpu_type, file_type, flags, magic};

    fn create_test_header() -> MachHeader {
        MachHeader {
            magic: magic::MH_MAGIC_64,
            cputype: cpu_type::CPU_TYPE_ARM64,
            cpusubtype: 0,
            filetype: file_type::MH_EXECUTE,
            ncmds: 10,
            sizeofcmds: 1024,
            flags: flags::MH_PIE | flags::MH_TWOLEVEL | flags::MH_DYLDLINK,
            reserved: 0,
            is_64bit: true,
            is_swapped: false,
        }
    }

    #[test]
    fn test_extract_header_metadata() {
        let header = create_test_header();
        let mut metadata = MetadataMap::new();

        extract_header_metadata(&header, &mut metadata);

        assert_eq!(metadata.get_string("EXE:CPUType").unwrap(), "ARM64");
        assert_eq!(metadata.get_string("EXE:FileType").unwrap(), "Executable");
        assert_eq!(metadata.get_integer("EXE:Is64Bit").unwrap(), 1);
        assert_eq!(metadata.get_integer("EXE:IsPIE").unwrap(), 1);
        assert_eq!(metadata.get_integer("EXE:HasTwoLevelNamespace").unwrap(), 1);
    }

    #[test]
    fn test_extract_uuid_metadata() {
        let uuid = UuidCommand {
            uuid: [
                0x55, 0x0E, 0x84, 0x00, 0xE2, 0x9B, 0x41, 0xD4, 0xA7, 0x16, 0x44, 0x66, 0x55, 0x44,
                0x00, 0x00,
            ],
        };
        let mut metadata = MetadataMap::new();

        extract_uuid_metadata(&uuid, &mut metadata);

        assert_eq!(
            metadata.get_string("EXE:UUID").unwrap(),
            "550E8400-E29B-41D4-A716-446655440000"
        );
    }

    #[test]
    fn test_extract_macho_metadata_basic() {
        let mut info = MachOInfo::new();
        info.header = Some(create_test_header());

        let metadata = extract_macho_metadata(&info);

        assert!(metadata.contains_key("EXE:FileFormat"));
        assert!(metadata.contains_key("EXE:CPUType"));
        assert!(metadata.contains_key("EXE:FileType"));
    }
}
