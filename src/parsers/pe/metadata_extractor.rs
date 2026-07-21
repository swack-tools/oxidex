//! Metadata extraction from PE headers

use std::collections::HashMap;

use crate::core::{MetadataMap, TagValue};
use crate::parsers::pe::clr_parser::DotNetInfo;
use crate::parsers::pe::signature_parser::SignatureInfo;
use crate::parsers::pe::structures::{
    CodeViewNB10, CodeViewRSDS, CoffHeader, DosHeader, ExportInfo, ImportFunction, ImportInfo,
    OptionalHeaderNT, OptionalHeaderStandard, VsFixedFileInfo, machine_types, subsystem_types,
};

/// Extract metadata from DOS header
pub fn extract_dos_metadata(header: &DosHeader, metadata: &mut MetadataMap) {
    metadata.insert(
        "EXE:DOSSignature".to_string(),
        TagValue::String(format!("{:#06X}", header.e_magic)),
    );
    metadata.insert(
        "EXE:PEHeaderOffset".to_string(),
        TagValue::Integer(header.e_lfanew as i64),
    );
}

/// Extract metadata from COFF header
pub fn extract_coff_metadata(header: &CoffHeader, metadata: &mut MetadataMap) {
    // Machine type
    let machine_name = match header.machine {
        machine_types::IMAGE_FILE_MACHINE_I386 => "Intel 386",
        machine_types::IMAGE_FILE_MACHINE_AMD64 => "x64 (AMD64)",
        machine_types::IMAGE_FILE_MACHINE_ARM => "ARM",
        machine_types::IMAGE_FILE_MACHINE_ARM64 => "ARM64",
        machine_types::IMAGE_FILE_MACHINE_IA64 => "Intel Itanium",
        machine_types::IMAGE_FILE_MACHINE_POWERPC => "PowerPC",
        _ => "Unknown",
    };
    metadata.insert(
        "EXE:MachineType".to_string(),
        TagValue::String(machine_name.to_string()),
    );
    metadata.insert(
        "EXE:MachineTypeRaw".to_string(),
        TagValue::Integer(header.machine as i64),
    );

    // Number of sections
    metadata.insert(
        "EXE:NumberOfSections".to_string(),
        TagValue::Integer(header.number_of_sections as i64),
    );

    // Timestamp (Unix epoch)
    if header.time_date_stamp > 0 {
        metadata.insert(
            "EXE:TimeStamp".to_string(),
            TagValue::Integer(header.time_date_stamp as i64),
        );

        // Convert to human-readable date if possible
        use chrono::{TimeZone, Utc};
        if let Some(dt) = Utc.timestamp_opt(header.time_date_stamp as i64, 0).single() {
            let timestamp_str = dt.format("%Y:%m:%d %H:%M:%S").to_string();
            metadata.insert(
                "EXE:CompileTime".to_string(),
                TagValue::String(timestamp_str.clone()),
            );
            // Add PE:Timestamp as string for ExifTool compatibility
            metadata.insert("EXE:Timestamp".to_string(), TagValue::String(timestamp_str));
        }
    }

    // Characteristics
    metadata.insert(
        "EXE:Characteristics".to_string(),
        TagValue::Integer(header.characteristics as i64),
    );

    // Add PE:Characteristics as hex string
    metadata.insert(
        "EXE:CharacteristicsHex".to_string(),
        TagValue::String(format!("0x{:04X}", header.characteristics)),
    );

    // Decode characteristic bit flags into human-readable strings
    // Reference: Microsoft PE/COFF specification IMAGE_FILE_HEADER.Characteristics
    use crate::core::decode_flags;

    const COFF_CHARACTERISTICS: &[(u32, &str)] = &[
        (0x0001, "No relocs"),
        (0x0002, "Executable"),
        (0x0004, "No line numbers"),
        (0x0008, "No symbols"),
        (0x0020, "Large address aware"),
        (0x0100, "32-bit"),
        (0x0200, "Bytes reversed lo"),
        (0x1000, "System file"),
        (0x2000, "DLL"),
        (0x4000, "Bytes reversed hi"),
    ];

    let flags = decode_flags(header.characteristics as u32, COFF_CHARACTERISTICS);

    // Insert decoded characteristics as comma-separated string
    if !flags.is_empty() {
        metadata.insert(
            "EXE:ImageFileCharacteristics".to_string(),
            TagValue::String(flags.join(", ")),
        );
    }

    // Decode common flags for FileType (kept for compatibility)
    let is_executable = (header.characteristics & 0x0002) != 0;
    let is_dll = (header.characteristics & 0x2000) != 0;
    let file_type = if is_dll {
        "DLL"
    } else if is_executable {
        "Executable"
    } else {
        "Object"
    };
    metadata.insert(
        "EXE:FileType".to_string(),
        TagValue::String(file_type.to_string()),
    );
}

/// Extract metadata from Optional Header
pub fn extract_optional_metadata(
    std_header: &OptionalHeaderStandard,
    nt_header: &OptionalHeaderNT,
    metadata: &mut MetadataMap,
) {
    // Image format
    let image_format = match std_header.magic {
        0x010B => "PE32",
        0x020B => "PE32+",
        _ => "Unknown",
    };
    metadata.insert(
        "EXE:ImageFormat".to_string(),
        TagValue::String(image_format.to_string()),
    );

    // PEType is an alias for ImageFormat (for ExifTool compatibility)
    metadata.insert(
        "EXE:PEType".to_string(),
        TagValue::String(image_format.to_string()),
    );

    // Linker version
    metadata.insert(
        "EXE:LinkerVersion".to_string(),
        TagValue::String(format!(
            "{}.{}",
            std_header.major_linker_version, std_header.minor_linker_version
        )),
    );

    // Code and data sizes
    metadata.insert(
        "EXE:CodeSize".to_string(),
        TagValue::Integer(std_header.size_of_code as i64),
    );
    metadata.insert(
        "EXE:InitializedDataSize".to_string(),
        TagValue::Integer(std_header.size_of_initialized_data as i64),
    );
    metadata.insert(
        "EXE:UninitializedDataSize".to_string(),
        TagValue::Integer(std_header.size_of_uninitialized_data as i64),
    );

    // Entry point
    metadata.insert(
        "EXE:EntryPoint".to_string(),
        TagValue::Integer(std_header.address_of_entry_point as i64),
    );

    // Add PE:EntryPoint as hexadecimal string for ExifTool compatibility
    metadata.insert(
        "EXE:EntryPointHex".to_string(),
        TagValue::String(format!("0x{:X}", std_header.address_of_entry_point)),
    );

    // Image base
    metadata.insert(
        "EXE:ImageBase".to_string(),
        TagValue::Integer(nt_header.image_base as i64),
    );

    // Add PE:ImageBase as hexadecimal string for ExifTool compatibility
    metadata.insert(
        "EXE:ImageBaseHex".to_string(),
        TagValue::String(format!("0x{:X}", nt_header.image_base)),
    );

    // Add PE:SizeOfImage
    metadata.insert(
        "EXE:SizeOfImage".to_string(),
        TagValue::Integer(nt_header.size_of_image as i64),
    );

    // OS version
    metadata.insert(
        "EXE:OSVersion".to_string(),
        TagValue::String(format!(
            "{}.{}",
            nt_header.major_operating_system_version, nt_header.minor_operating_system_version
        )),
    );

    // Image version
    metadata.insert(
        "EXE:ImageVersion".to_string(),
        TagValue::String(format!(
            "{}.{}",
            nt_header.major_image_version, nt_header.minor_image_version
        )),
    );

    // Subsystem
    let subsystem_name = match nt_header.subsystem {
        subsystem_types::IMAGE_SUBSYSTEM_UNKNOWN => "Unknown",
        subsystem_types::IMAGE_SUBSYSTEM_NATIVE => "Native (Driver)",
        subsystem_types::IMAGE_SUBSYSTEM_WINDOWS_GUI => "Windows GUI",
        subsystem_types::IMAGE_SUBSYSTEM_WINDOWS_CUI => "Windows Console",
        subsystem_types::IMAGE_SUBSYSTEM_OS2_CUI => "OS/2 Console",
        subsystem_types::IMAGE_SUBSYSTEM_POSIX_CUI => "POSIX Console",
        subsystem_types::IMAGE_SUBSYSTEM_EFI_APPLICATION => "EFI Application",
        subsystem_types::IMAGE_SUBSYSTEM_EFI_BOOT_SERVICE_DRIVER => "EFI Boot Service Driver",
        subsystem_types::IMAGE_SUBSYSTEM_EFI_RUNTIME_DRIVER => "EFI Runtime Driver",
        subsystem_types::IMAGE_SUBSYSTEM_XBOX => "Xbox",
        _ => "Unknown",
    };
    metadata.insert(
        "EXE:Subsystem".to_string(),
        TagValue::String(subsystem_name.to_string()),
    );
    metadata.insert(
        "EXE:SubsystemRaw".to_string(),
        TagValue::Integer(nt_header.subsystem as i64),
    );

    // Subsystem version
    metadata.insert(
        "EXE:SubsystemVersion".to_string(),
        TagValue::String(format!(
            "{}.{}",
            nt_header.major_subsystem_version, nt_header.minor_subsystem_version
        )),
    );

    // Checksum
    if nt_header.checksum != 0 {
        metadata.insert(
            "EXE:Checksum".to_string(),
            TagValue::Integer(nt_header.checksum as i64),
        );
    }

    // DLL Characteristics
    metadata.insert(
        "EXE:DllCharacteristics".to_string(),
        TagValue::Integer(nt_header.dll_characteristics as i64),
    );

    // Decode DLL characteristic bit flags
    // Reference: Microsoft PE/COFF specification IMAGE_OPTIONAL_HEADER.DllCharacteristics
    use crate::core::decode_flags;

    const DLL_CHARACTERISTICS: &[(u32, &str)] = &[
        (0x0020, "High entropy VA"),
        (0x0040, "Dynamic base"),
        (0x0080, "Force integrity"),
        (0x0100, "NX compatible"),
        (0x0200, "No isolation"),
        (0x0400, "No SEH"),
        (0x0800, "No bind"),
        (0x1000, "AppContainer"),
        (0x2000, "WDM driver"),
        (0x4000, "Control flow guard"),
        (0x8000, "Terminal server aware"),
    ];

    let dll_flags = decode_flags(nt_header.dll_characteristics as u32, DLL_CHARACTERISTICS);

    if !dll_flags.is_empty() {
        metadata.insert(
            "EXE:DllCharacteristicsDecoded".to_string(),
            TagValue::String(dll_flags.join(", ")),
        );
    }

    // Security features derived from DLL characteristics
    metadata.insert(
        "EXE:ASLR".to_string(),
        TagValue::Integer(if (nt_header.dll_characteristics & 0x0040) != 0 {
            1
        } else {
            0
        }),
    );

    metadata.insert(
        "EXE:DEP".to_string(),
        TagValue::Integer(if (nt_header.dll_characteristics & 0x0100) != 0 {
            1
        } else {
            0
        }),
    );

    metadata.insert(
        "EXE:ControlFlowGuard".to_string(),
        TagValue::Integer(if (nt_header.dll_characteristics & 0x4000) != 0 {
            1
        } else {
            0
        }),
    );
}

/// Extract metadata from VERSION_INFO resource
pub fn extract_version_info_metadata(
    fixed_info: &VsFixedFileInfo,
    strings: &HashMap<String, String>,
    metadata: &mut MetadataMap,
) {
    // Fixed file info
    metadata.insert(
        "EXE:FileVersionNumber".to_string(),
        TagValue::String(fixed_info.file_version()),
    );
    metadata.insert(
        "EXE:ProductVersionNumber".to_string(),
        TagValue::String(fixed_info.product_version()),
    );
    metadata.insert(
        "EXE:FileFlagsMask".to_string(),
        TagValue::String(format!("{:#06x}", fixed_info.file_flags_mask)),
    );

    let flags = fixed_info.file_flags_string();
    if !flags.is_empty() {
        metadata.insert(
            "EXE:FileFlags".to_string(),
            TagValue::String(flags.join(", ")),
        );
    } else {
        metadata.insert(
            "EXE:FileFlags".to_string(),
            TagValue::String("(none)".to_string()),
        );
    }

    metadata.insert(
        "EXE:FileOS".to_string(),
        TagValue::String(fixed_info.file_os_string().to_string()),
    );
    metadata.insert(
        "EXE:ObjectFileType".to_string(),
        TagValue::String(fixed_info.file_type_string().to_string()),
    );
    metadata.insert(
        "EXE:FileSubtype".to_string(),
        TagValue::Integer(fixed_info.file_subtype as i64),
    );

    // String file info
    for (key, value) in strings {
        let tag_name = format!("EXE:{}", key);
        metadata.insert(tag_name, TagValue::String(value.clone()));
    }
}

/// Extract metadata from CodeView RSDS debug info
pub fn extract_rsds_metadata(rsds: &CodeViewRSDS, metadata: &mut MetadataMap) {
    metadata.insert(
        "EXE:PDBFileName".to_string(),
        TagValue::String(rsds.pdb_file_name.clone()),
    );
    metadata.insert("EXE:PDBAge".to_string(), TagValue::Integer(rsds.age as i64));

    // Format GUID as string
    let guid_str = format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        rsds.guid[3],
        rsds.guid[2],
        rsds.guid[1],
        rsds.guid[0],
        rsds.guid[5],
        rsds.guid[4],
        rsds.guid[7],
        rsds.guid[6],
        rsds.guid[8],
        rsds.guid[9],
        rsds.guid[10],
        rsds.guid[11],
        rsds.guid[12],
        rsds.guid[13],
        rsds.guid[14],
        rsds.guid[15]
    );
    metadata.insert("EXE:PDBGUID".to_string(), TagValue::String(guid_str));
}

/// Extract metadata from CodeView NB10 debug info
pub fn extract_nb10_metadata(nb10: &CodeViewNB10, metadata: &mut MetadataMap) {
    metadata.insert(
        "EXE:PDBFileName".to_string(),
        TagValue::String(nb10.pdb_file_name.clone()),
    );
    metadata.insert("EXE:PDBAge".to_string(), TagValue::Integer(nb10.age as i64));

    // Convert timestamp to date
    use chrono::{TimeZone, Utc};
    if let Some(dt) = Utc.timestamp_opt(nb10.timestamp as i64, 0).single() {
        metadata.insert(
            "EXE:PDBCreateDate".to_string(),
            TagValue::String(dt.format("%Y:%m:%d %H:%M:%S").to_string()),
        );
    }

    metadata.insert(
        "EXE:PDBModifyDate".to_string(),
        TagValue::String("(same as create)".to_string()),
    );
}

/// Extract metadata from Rich Header
pub fn extract_rich_header_metadata(
    rich: &crate::parsers::pe::rich_header_parser::RichHeader,
    metadata: &mut MetadataMap,
) {
    // Indicate Rich Header presence
    metadata.insert(
        "EXE:RichHeaderPresent".to_string(),
        TagValue::String("Yes".to_string()),
    );

    // XOR key / checksum
    metadata.insert(
        "EXE:RichHeaderChecksum".to_string(),
        TagValue::String(format!("{:#010X}", rich.checksum)),
    );

    // Number of tool entries
    metadata.insert(
        "EXE:RichHeaderEntries".to_string(),
        TagValue::Integer(rich.entries.len() as i64),
    );

    // Compiler info string (formatted as "ProductID.BuildNumber xCount")
    let compiler_info = rich.compiler_info_string();
    if !compiler_info.is_empty() {
        metadata.insert(
            "EXE:RichCompilerInfo".to_string(),
            TagValue::String(compiler_info),
        );
    }

    // Product IDs (comma-separated list)
    let product_ids = rich.product_ids_string();
    if !product_ids.is_empty() {
        metadata.insert(
            "EXE:RichProductIDs".to_string(),
            TagValue::String(product_ids),
        );
    }

    // MD5 hash for forensic comparison
    metadata.insert(
        "EXE:RichHeaderHash".to_string(),
        TagValue::String(rich.hash_md5()),
    );
}

/// Extract metadata from Export Directory
pub fn extract_export_metadata(export_info: &ExportInfo, metadata: &mut MetadataMap) {
    // Set HasExports flag
    metadata.insert("EXE:HasExports".to_string(), TagValue::Integer(1));

    // Export DLL name
    metadata.insert(
        "EXE:ExportDLLName".to_string(),
        TagValue::String(export_info.dll_name.clone()),
    );

    // Export counts
    metadata.insert(
        "EXE:ExportCount".to_string(),
        TagValue::Integer(export_info.directory.number_of_functions as i64),
    );
    metadata.insert(
        "EXE:ExportNameCount".to_string(),
        TagValue::Integer(export_info.directory.number_of_names as i64),
    );

    // Base ordinal
    metadata.insert(
        "EXE:ExportBase".to_string(),
        TagValue::Integer(export_info.directory.base as i64),
    );

    // Timestamp
    if export_info.directory.time_date_stamp > 0 {
        metadata.insert(
            "EXE:ExportTimestamp".to_string(),
            TagValue::Integer(export_info.directory.time_date_stamp as i64),
        );

        // Convert to human-readable date if possible
        use chrono::{TimeZone, Utc};
        if let Some(dt) = Utc
            .timestamp_opt(export_info.directory.time_date_stamp as i64, 0)
            .single()
        {
            metadata.insert(
                "EXE:ExportCreateDate".to_string(),
                TagValue::String(dt.format("%Y:%m:%d %H:%M:%S").to_string()),
            );
        }
    }

    // Export characteristics
    metadata.insert(
        "EXE:ExportCharacteristics".to_string(),
        TagValue::Integer(export_info.directory.characteristics as i64),
    );

    // Forwarded export count
    if export_info.forwarded_count > 0 {
        metadata.insert(
            "EXE:ForwardedExportCount".to_string(),
            TagValue::Integer(export_info.forwarded_count as i64),
        );
    }

    // Exported functions (comma-separated list, limited to first 30)
    if !export_info.function_names.is_empty() {
        let functions_str = export_info.function_names.join(", ");
        metadata.insert(
            "EXE:ExportedFunctions".to_string(),
            TagValue::String(functions_str),
        );
    }
}

/// Extract metadata from digital signature
pub fn extract_signature_metadata(sig_info: &SignatureInfo, metadata: &mut MetadataMap) {
    // Signature presence
    metadata.insert(
        "EXE:SignaturePresent".to_string(),
        TagValue::Integer(if sig_info.signature_present { 1 } else { 0 }),
    );

    if !sig_info.signature_present {
        return;
    }

    // Signature type
    metadata.insert(
        "EXE:SignatureType".to_string(),
        TagValue::String(sig_info.signature_type.clone()),
    );

    // Certificate count
    metadata.insert(
        "EXE:CertificateCount".to_string(),
        TagValue::Integer(sig_info.certificate_count as i64),
    );

    // Signer information
    if let Some(ref cn) = sig_info.signer_common_name {
        metadata.insert(
            "EXE:SignerCommonName".to_string(),
            TagValue::String(cn.clone()),
        );
    }

    if let Some(ref org) = sig_info.signer_organization {
        metadata.insert(
            "EXE:SignerOrganization".to_string(),
            TagValue::String(org.clone()),
        );
    }

    // Issuer information
    if let Some(ref cn) = sig_info.issuer_common_name {
        metadata.insert(
            "EXE:IssuerCommonName".to_string(),
            TagValue::String(cn.clone()),
        );
    }

    if let Some(ref org) = sig_info.issuer_organization {
        metadata.insert(
            "EXE:IssuerOrganization".to_string(),
            TagValue::String(org.clone()),
        );
    }

    // Certificate details
    if let Some(ref serial) = sig_info.certificate_serial_number {
        metadata.insert(
            "EXE:CertificateSerialNumber".to_string(),
            TagValue::String(serial.clone()),
        );
    }

    if let Some(ref not_before) = sig_info.certificate_not_before {
        metadata.insert(
            "EXE:CertificateNotBefore".to_string(),
            TagValue::String(not_before.clone()),
        );
    }

    if let Some(ref not_after) = sig_info.certificate_not_after {
        metadata.insert(
            "EXE:CertificateNotAfter".to_string(),
            TagValue::String(not_after.clone()),
        );
    }

    if let Some(ref thumbprint) = sig_info.certificate_thumbprint {
        metadata.insert(
            "EXE:CertificateThumbprint".to_string(),
            TagValue::String(thumbprint.clone()),
        );
    }

    // Counter-signature information
    metadata.insert(
        "EXE:HasCounterSignature".to_string(),
        TagValue::Integer(if sig_info.has_counter_signature { 1 } else { 0 }),
    );

    if let Some(ref counter_time) = sig_info.counter_signature_time {
        metadata.insert(
            "EXE:CounterSignatureTime".to_string(),
            TagValue::String(counter_time.clone()),
        );
    }

    // Forensic indicators
    metadata.insert(
        "EXE:SignatureValid".to_string(),
        TagValue::Integer(if sig_info.signature_valid { 1 } else { 0 }),
    );

    metadata.insert(
        "EXE:CertificateExpired".to_string(),
        TagValue::Integer(if sig_info.certificate_expired { 1 } else { 0 }),
    );
}

/// List of suspicious imports that may indicate malicious behavior
const SUSPICIOUS_IMPORTS: &[&str] = &[
    "VirtualAlloc",
    "VirtualAllocEx",
    "VirtualProtect",
    "VirtualProtectEx",
    "WriteProcessMemory",
    "CreateRemoteThread",
    "CreateRemoteThreadEx",
    "SetWindowsHookEx",
    "GetProcAddress",
    "LoadLibrary",
    "LoadLibraryEx",
    "OpenProcess",
    "CreateToolhelp32Snapshot",
    "NtCreateThread",
    "NtCreateThreadEx",
    "RtlCreateUserThread",
    "QueueUserAPC",
    "SetThreadContext",
    "ResumeThread",
    "SuspendThread",
    "GetThreadContext",
];

/// Extract metadata from import information
pub fn extract_import_metadata(imports: &[ImportInfo], metadata: &mut MetadataMap) {
    if imports.is_empty() {
        return;
    }

    // Extract DLL names
    let dll_names: Vec<String> = imports.iter().map(|i| i.dll_name.clone()).collect();
    metadata.insert(
        "EXE:ImportedDLLs".to_string(),
        TagValue::String(dll_names.join(", ")),
    );

    // Count total imports
    let total_imports: usize = imports.iter().map(|i| i.functions.len()).sum();
    metadata.insert(
        "EXE:ImportCount".to_string(),
        TagValue::Integer(total_imports as i64),
    );

    // Count DLLs
    metadata.insert(
        "EXE:ImportedDLLCount".to_string(),
        TagValue::Integer(imports.len() as i64),
    );

    // Extract imported functions (limit to first 50)
    let mut function_list = Vec::new();
    let mut has_suspicious = false;

    for import in imports {
        for func in &import.functions {
            if function_list.len() >= 50 {
                break;
            }

            match func {
                ImportFunction::ByName { name, .. } => {
                    function_list.push(format!("{}:{}", import.dll_name, name));

                    // Check for suspicious imports
                    if SUSPICIOUS_IMPORTS.contains(&name.as_str()) {
                        has_suspicious = true;
                    }
                }
                ImportFunction::ByOrdinal { ordinal } => {
                    function_list.push(format!("{}:#{}", import.dll_name, ordinal));
                }
            }
        }

        if function_list.len() >= 50 {
            break;
        }
    }

    if !function_list.is_empty() {
        metadata.insert(
            "EXE:ImportedFunctions".to_string(),
            TagValue::String(function_list.join("; ")),
        );
    }

    // Set suspicious imports flag
    metadata.insert(
        "EXE:HasSuspiciousImports".to_string(),
        TagValue::Integer(if has_suspicious { 1 } else { 0 }),
    );
}

/// Extract metadata from .NET CLR information
pub fn extract_dotnet_metadata(dotnet_info: &DotNetInfo, metadata: &mut MetadataMap) {
    // Indicate .NET presence
    metadata.insert(
        "EXE:DotNet".to_string(),
        TagValue::Integer(if dotnet_info.is_dotnet { 1 } else { 0 }),
    );

    if !dotnet_info.is_dotnet {
        return;
    }

    // CLR version
    if let Some(ref clr_version) = dotnet_info.clr_version {
        metadata.insert(
            "EXE:CLRVersion".to_string(),
            TagValue::String(clr_version.clone()),
        );
    }

    // Assembly name
    if let Some(ref assembly_name) = dotnet_info.assembly_name {
        metadata.insert(
            "EXE:AssemblyName".to_string(),
            TagValue::String(assembly_name.clone()),
        );
    }

    // Assembly version
    if let Some(ref assembly_version) = dotnet_info.assembly_version {
        metadata.insert(
            "EXE:AssemblyVersion".to_string(),
            TagValue::String(assembly_version.clone()),
        );
    }

    // Assembly culture
    if let Some(ref culture) = dotnet_info.assembly_culture {
        metadata.insert(
            "EXE:AssemblyCulture".to_string(),
            TagValue::String(culture.clone()),
        );
    }

    // Public key token
    if let Some(ref token) = dotnet_info.public_key_token {
        metadata.insert(
            "EXE:PublicKeyToken".to_string(),
            TagValue::String(token.clone()),
        );
    }

    // Target framework
    if let Some(ref framework) = dotnet_info.target_framework {
        metadata.insert(
            "EXE:TargetFramework".to_string(),
            TagValue::String(framework.clone()),
        );
    }

    // Runtime flags
    metadata.insert(
        "EXE:ILOnly".to_string(),
        TagValue::Integer(if dotnet_info.il_only { 1 } else { 0 }),
    );

    metadata.insert(
        "EXE:StrongNameSigned".to_string(),
        TagValue::Integer(if dotnet_info.strong_name_signed { 1 } else { 0 }),
    );

    metadata.insert(
        "EXE:Requires32Bit".to_string(),
        TagValue::Integer(if dotnet_info.requires_32bit { 1 } else { 0 }),
    );

    metadata.insert(
        "EXE:Prefers32Bit".to_string(),
        TagValue::Integer(if dotnet_info.prefers_32bit { 1 } else { 0 }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_dos_metadata() {
        let header = DosHeader {
            e_magic: 0x5A4D,
            e_cblp: 0,
            e_cp: 0,
            e_crlc: 0,
            e_cparhdr: 0,
            e_minalloc: 0,
            e_maxalloc: 0,
            e_ss: 0,
            e_sp: 0,
            e_csum: 0,
            e_ip: 0,
            e_cs: 0,
            e_lfarlc: 0,
            e_ovno: 0,
            e_res: [0; 4],
            e_oemid: 0,
            e_oeminfo: 0,
            e_res2: [0; 10],
            e_lfanew: 0xF0,
        };

        let mut metadata = MetadataMap::new();
        extract_dos_metadata(&header, &mut metadata);

        assert!(metadata.contains_key("EXE:DOSSignature"));
        assert!(metadata.contains_key("EXE:PEHeaderOffset"));
    }

    #[test]
    fn test_extract_coff_metadata() {
        let header = CoffHeader {
            machine: machine_types::IMAGE_FILE_MACHINE_AMD64,
            number_of_sections: 5,
            time_date_stamp: 1609459200, // 2021-01-01 00:00:00 UTC
            pointer_to_symbol_table: 0,
            number_of_symbols: 0,
            size_of_optional_header: 0xF0,
            characteristics: 0x0022, // Executable, Large address aware
        };

        let mut metadata = MetadataMap::new();
        extract_coff_metadata(&header, &mut metadata);

        assert_eq!(
            metadata.get_string("EXE:MachineType").unwrap(),
            "x64 (AMD64)"
        );
        assert_eq!(metadata.get_integer("EXE:NumberOfSections").unwrap(), 5);
        assert!(metadata.contains_key("EXE:CompileTime"));
        assert_eq!(metadata.get_string("EXE:FileType").unwrap(), "Executable");
    }

    #[test]
    fn test_extract_rsds_metadata() {
        // Create a test RSDS structure
        let rsds = CodeViewRSDS {
            signature: *b"RSDS",
            guid: [
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
                0x0F, 0x10,
            ],
            age: 1,
            pdb_file_name: "test.pdb".to_string(),
        };

        let mut metadata = MetadataMap::new();
        extract_rsds_metadata(&rsds, &mut metadata);

        // Verify PDB file name
        assert_eq!(metadata.get_string("EXE:PDBFileName").unwrap(), "test.pdb");

        // Verify age
        assert_eq!(metadata.get_integer("EXE:PDBAge").unwrap(), 1);

        // Verify GUID format (note: byte order conversion in GUID formatting)
        assert!(metadata.contains_key("EXE:PDBGUID"));
        let guid = metadata.get_string("EXE:PDBGUID").unwrap();
        assert_eq!(guid.len(), 36); // GUID is 32 hex chars + 4 hyphens
        assert!(guid.contains('-'));
    }

    #[test]
    fn test_extract_nb10_metadata() {
        // Create a test NB10 structure
        let nb10 = CodeViewNB10 {
            signature: *b"NB10",
            offset: 0,
            timestamp: 1609459200, // 2021-01-01 00:00:00 UTC
            age: 1,
            pdb_file_name: "legacy.pdb".to_string(),
        };

        let mut metadata = MetadataMap::new();
        extract_nb10_metadata(&nb10, &mut metadata);

        // Verify PDB file name
        assert_eq!(
            metadata.get_string("EXE:PDBFileName").unwrap(),
            "legacy.pdb"
        );

        // Verify age
        assert_eq!(metadata.get_integer("EXE:PDBAge").unwrap(), 1);

        // Verify create date was set
        assert!(metadata.contains_key("EXE:PDBCreateDate"));
        let create_date = metadata.get_string("EXE:PDBCreateDate").unwrap();
        assert!(create_date.starts_with("2021:01:01"));

        // Verify modify date placeholder
        assert_eq!(
            metadata.get_string("EXE:PDBModifyDate").unwrap(),
            "(same as create)"
        );
    }

    #[test]
    fn test_extract_export_metadata() {
        use crate::parsers::pe::structures::ImageExportDirectory;

        // Create test export directory
        let directory = ImageExportDirectory {
            characteristics: 0,
            time_date_stamp: 1609459200, // 2021-01-01 00:00:00 UTC
            major_version: 0,
            minor_version: 0,
            name: 0x1000,
            base: 1,
            number_of_functions: 5,
            number_of_names: 3,
            address_of_functions: 0x2000,
            address_of_names: 0x3000,
            address_of_name_ordinals: 0x4000,
        };

        let export_info = ExportInfo {
            directory,
            dll_name: "test.dll".to_string(),
            function_names: vec!["Function1".to_string(), "Function2".to_string()],
            forwarded_count: 1,
        };

        let mut metadata = MetadataMap::new();
        extract_export_metadata(&export_info, &mut metadata);

        // Verify HasExports flag
        assert_eq!(metadata.get_integer("EXE:HasExports").unwrap(), 1);

        // Verify DLL name
        assert_eq!(
            metadata.get_string("EXE:ExportDLLName").unwrap(),
            "test.dll"
        );

        // Verify counts
        assert_eq!(metadata.get_integer("EXE:ExportCount").unwrap(), 5);
        assert_eq!(metadata.get_integer("EXE:ExportNameCount").unwrap(), 3);
        assert_eq!(metadata.get_integer("EXE:ExportBase").unwrap(), 1);

        // Verify timestamp
        assert_eq!(
            metadata.get_integer("EXE:ExportTimestamp").unwrap(),
            1609459200
        );
        assert!(metadata.contains_key("EXE:ExportCreateDate"));
        let create_date = metadata.get_string("EXE:ExportCreateDate").unwrap();
        assert!(create_date.starts_with("2021:01:01"));

        // Verify characteristics
        assert_eq!(
            metadata.get_integer("EXE:ExportCharacteristics").unwrap(),
            0
        );

        // Verify forwarded count
        assert_eq!(metadata.get_integer("EXE:ForwardedExportCount").unwrap(), 1);

        // Verify exported functions
        let exported_functions = metadata.get_string("EXE:ExportedFunctions").unwrap();
        assert!(exported_functions.contains("Function1"));
        assert!(exported_functions.contains("Function2"));
    }

    #[test]
    fn test_extract_import_metadata() {
        // Create test import info
        let imports = vec![
            ImportInfo {
                dll_name: "kernel32.dll".to_string(),
                functions: vec![
                    ImportFunction::ByName {
                        hint: 0,
                        name: "CreateFileW".to_string(),
                    },
                    ImportFunction::ByName {
                        hint: 1,
                        name: "VirtualAlloc".to_string(),
                    },
                    ImportFunction::ByOrdinal { ordinal: 123 },
                ],
            },
            ImportInfo {
                dll_name: "user32.dll".to_string(),
                functions: vec![ImportFunction::ByName {
                    hint: 0,
                    name: "MessageBoxW".to_string(),
                }],
            },
        ];

        let mut metadata = MetadataMap::new();
        extract_import_metadata(&imports, &mut metadata);

        // Verify imported DLLs
        let imported_dlls = metadata.get_string("EXE:ImportedDLLs").unwrap();
        assert!(imported_dlls.contains("kernel32.dll"));
        assert!(imported_dlls.contains("user32.dll"));

        // Verify import count
        assert_eq!(metadata.get_integer("EXE:ImportCount").unwrap(), 4);

        // Verify DLL count
        assert_eq!(metadata.get_integer("EXE:ImportedDLLCount").unwrap(), 2);

        // Verify imported functions
        let imported_functions = metadata.get_string("EXE:ImportedFunctions").unwrap();
        assert!(imported_functions.contains("kernel32.dll:CreateFileW"));
        assert!(imported_functions.contains("kernel32.dll:VirtualAlloc"));
        assert!(imported_functions.contains("kernel32.dll:#123"));
        assert!(imported_functions.contains("user32.dll:MessageBoxW"));

        // Verify suspicious imports flag (VirtualAlloc is suspicious)
        assert_eq!(metadata.get_integer("EXE:HasSuspiciousImports").unwrap(), 1);
    }

    #[test]
    fn test_extract_import_metadata_no_suspicious() {
        // Create test import info without suspicious imports
        let imports = vec![ImportInfo {
            dll_name: "user32.dll".to_string(),
            functions: vec![
                ImportFunction::ByName {
                    hint: 0,
                    name: "MessageBoxW".to_string(),
                },
                ImportFunction::ByName {
                    hint: 1,
                    name: "DefWindowProcW".to_string(),
                },
            ],
        }];

        let mut metadata = MetadataMap::new();
        extract_import_metadata(&imports, &mut metadata);

        // Verify no suspicious imports
        assert_eq!(metadata.get_integer("EXE:HasSuspiciousImports").unwrap(), 0);
    }

    #[test]
    fn test_extract_import_metadata_empty() {
        let imports = vec![];
        let mut metadata = MetadataMap::new();
        extract_import_metadata(&imports, &mut metadata);

        // Should not add any metadata for empty imports
        assert!(!metadata.contains_key("EXE:ImportedDLLs"));
    }

    #[test]
    fn test_extract_optional_metadata_dll_characteristics() {
        // Test DLL characteristics decoding
        let std_header = OptionalHeaderStandard {
            magic: 0x020B, // PE32+
            major_linker_version: 14,
            minor_linker_version: 0,
            size_of_code: 0x1000,
            size_of_initialized_data: 0x2000,
            size_of_uninitialized_data: 0,
            address_of_entry_point: 0x1000,
            base_of_code: 0x1000,
        };

        let nt_header = OptionalHeaderNT {
            image_base: 0x140000000,
            section_alignment: 0x1000,
            file_alignment: 0x200,
            major_operating_system_version: 10,
            minor_operating_system_version: 0,
            major_image_version: 1,
            minor_image_version: 0,
            major_subsystem_version: 10,
            minor_subsystem_version: 0,
            win32_version_value: 0,
            size_of_image: 0x10000,
            size_of_headers: 0x400,
            checksum: 0x12345,
            subsystem: subsystem_types::IMAGE_SUBSYSTEM_WINDOWS_GUI,
            dll_characteristics: 0x4160, // ASLR | DEP | NX | CFG
            size_of_stack_reserve: 0x100000,
            size_of_stack_commit: 0x1000,
            size_of_heap_reserve: 0x100000,
            size_of_heap_commit: 0x1000,
            loader_flags: 0,
            number_of_rva_and_sizes: 16,
            data_directories: vec![],
        };

        let mut metadata = MetadataMap::new();
        extract_optional_metadata(&std_header, &nt_header, &mut metadata);

        // Check DllCharacteristics raw value
        assert_eq!(
            metadata.get_integer("EXE:DllCharacteristics").unwrap(),
            0x4160
        );

        // Check decoded flags
        let decoded = metadata
            .get_string("EXE:DllCharacteristicsDecoded")
            .unwrap();
        assert!(decoded.contains("High entropy VA"));
        assert!(decoded.contains("Dynamic base"));
        assert!(decoded.contains("NX compatible"));
        assert!(decoded.contains("Control flow guard"));

        // Check security feature flags
        assert_eq!(metadata.get_integer("EXE:ASLR").unwrap(), 1);
        assert_eq!(metadata.get_integer("EXE:DEP").unwrap(), 1);
        assert_eq!(metadata.get_integer("EXE:ControlFlowGuard").unwrap(), 1);
    }

    #[test]
    fn test_extract_optional_metadata_subsystem_types() {
        let std_header = OptionalHeaderStandard {
            magic: 0x010B, // PE32
            major_linker_version: 14,
            minor_linker_version: 0,
            size_of_code: 0x1000,
            size_of_initialized_data: 0x2000,
            size_of_uninitialized_data: 0,
            address_of_entry_point: 0x1000,
            base_of_code: 0x1000,
        };

        // Test various subsystem types
        let test_cases = vec![
            (subsystem_types::IMAGE_SUBSYSTEM_NATIVE, "Native (Driver)"),
            (subsystem_types::IMAGE_SUBSYSTEM_WINDOWS_GUI, "Windows GUI"),
            (
                subsystem_types::IMAGE_SUBSYSTEM_WINDOWS_CUI,
                "Windows Console",
            ),
            (subsystem_types::IMAGE_SUBSYSTEM_OS2_CUI, "OS/2 Console"),
            (subsystem_types::IMAGE_SUBSYSTEM_POSIX_CUI, "POSIX Console"),
            (
                subsystem_types::IMAGE_SUBSYSTEM_EFI_APPLICATION,
                "EFI Application",
            ),
            (
                subsystem_types::IMAGE_SUBSYSTEM_EFI_BOOT_SERVICE_DRIVER,
                "EFI Boot Service Driver",
            ),
            (
                subsystem_types::IMAGE_SUBSYSTEM_EFI_RUNTIME_DRIVER,
                "EFI Runtime Driver",
            ),
            (subsystem_types::IMAGE_SUBSYSTEM_XBOX, "Xbox"),
        ];

        for (subsystem_type, expected_name) in test_cases {
            let nt_header = OptionalHeaderNT {
                image_base: 0x400000,
                section_alignment: 0x1000,
                file_alignment: 0x200,
                major_operating_system_version: 6,
                minor_operating_system_version: 1,
                major_image_version: 1,
                minor_image_version: 0,
                major_subsystem_version: 6,
                minor_subsystem_version: 1,
                win32_version_value: 0,
                size_of_image: 0x10000,
                size_of_headers: 0x400,
                checksum: 0,
                subsystem: subsystem_type,
                dll_characteristics: 0,
                size_of_stack_reserve: 0x100000,
                size_of_stack_commit: 0x1000,
                size_of_heap_reserve: 0x100000,
                size_of_heap_commit: 0x1000,
                loader_flags: 0,
                number_of_rva_and_sizes: 16,
                data_directories: vec![],
            };

            let mut metadata = MetadataMap::new();
            extract_optional_metadata(&std_header, &nt_header, &mut metadata);

            assert_eq!(
                metadata.get_string("EXE:Subsystem").unwrap(),
                expected_name,
                "Failed for subsystem type {}",
                subsystem_type
            );
        }
    }
}
