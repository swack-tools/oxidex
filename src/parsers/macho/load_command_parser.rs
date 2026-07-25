//! Load command parser
//!
//! This module handles parsing of Mach-O load commands, which describe the layout
//! of the file in virtual memory and provide information about linked libraries,
//! symbol tables, code signatures, and more.

use nom::{
    IResult,
    bytes::complete::take,
    number::complete::{le_i32, le_u32, le_u64},
};

use super::structures::{
    BuildToolVersion, BuildVersionCommand, DylibCommand, DysymtabCommand, EncryptionInfoCommand,
    EntryPointCommand, LinkeditDataCommand, LoadCommandHeader, RpathCommand, Section,
    SegmentCommand, SourceVersionCommand, SymtabCommand, UuidCommand, VersionMinCommand,
    load_command,
};

// =============================================================================
// Load Command Header
// =============================================================================

/// Parse a load command header (cmd + cmdsize)
pub fn parse_load_command_header(input: &[u8]) -> IResult<&[u8], LoadCommandHeader> {
    let (input, cmd) = le_u32(input)?;
    let (input, cmdsize) = le_u32(input)?;
    Ok((input, LoadCommandHeader { cmd, cmdsize }))
}

// =============================================================================
// Segment Commands
// =============================================================================

/// Parse a 32-bit segment command (LC_SEGMENT)
pub fn parse_segment_command_32(input: &[u8]) -> IResult<&[u8], SegmentCommand> {
    // Skip cmd and cmdsize (already parsed in header)
    let (input, _cmd) = le_u32(input)?;
    let (input, _cmdsize) = le_u32(input)?;

    // Parse segment name (16 bytes, null-padded)
    let (input, segname_bytes) = take(16usize)(input)?;
    let segname = parse_name_16(segname_bytes);

    let (input, vmaddr) = le_u32(input)?;
    let (input, vmsize) = le_u32(input)?;
    let (input, fileoff) = le_u32(input)?;
    let (input, filesize) = le_u32(input)?;
    let (input, maxprot) = le_i32(input)?;
    let (input, initprot) = le_i32(input)?;
    let (input, nsects) = le_u32(input)?;
    let (input, flags) = le_u32(input)?;

    // Parse sections
    let (input, sections) = parse_sections_32(input, nsects)?;

    Ok((
        input,
        SegmentCommand {
            segname,
            vmaddr: vmaddr as u64,
            vmsize: vmsize as u64,
            fileoff: fileoff as u64,
            filesize: filesize as u64,
            maxprot,
            initprot,
            nsects,
            flags,
            sections,
        },
    ))
}

/// Parse a 64-bit segment command (LC_SEGMENT_64)
pub fn parse_segment_command_64(input: &[u8]) -> IResult<&[u8], SegmentCommand> {
    // Skip cmd and cmdsize (already parsed in header)
    let (input, _cmd) = le_u32(input)?;
    let (input, _cmdsize) = le_u32(input)?;

    // Parse segment name (16 bytes, null-padded)
    let (input, segname_bytes) = take(16usize)(input)?;
    let segname = parse_name_16(segname_bytes);

    let (input, vmaddr) = le_u64(input)?;
    let (input, vmsize) = le_u64(input)?;
    let (input, fileoff) = le_u64(input)?;
    let (input, filesize) = le_u64(input)?;
    let (input, maxprot) = le_i32(input)?;
    let (input, initprot) = le_i32(input)?;
    let (input, nsects) = le_u32(input)?;
    let (input, flags) = le_u32(input)?;

    // Parse sections
    let (input, sections) = parse_sections_64(input, nsects)?;

    Ok((
        input,
        SegmentCommand {
            segname,
            vmaddr,
            vmsize,
            fileoff,
            filesize,
            maxprot,
            initprot,
            nsects,
            flags,
            sections,
        },
    ))
}

/// Parse 32-bit sections
fn parse_sections_32(input: &[u8], count: u32) -> IResult<&[u8], Vec<Section>> {
    let mut sections = Vec::with_capacity(count as usize);
    let mut remaining = input;

    for _ in 0..count {
        let (input, sectname_bytes) = take(16usize)(remaining)?;
        let (input, segname_bytes) = take(16usize)(input)?;
        let (input, addr) = le_u32(input)?;
        let (input, size) = le_u32(input)?;
        let (input, offset) = le_u32(input)?;
        let (input, align) = le_u32(input)?;
        let (input, reloff) = le_u32(input)?;
        let (input, nreloc) = le_u32(input)?;
        let (input, flags) = le_u32(input)?;
        let (input, reserved1) = le_u32(input)?;
        let (input, reserved2) = le_u32(input)?;

        sections.push(Section {
            sectname: parse_name_16(sectname_bytes),
            segname: parse_name_16(segname_bytes),
            addr: addr as u64,
            size: size as u64,
            offset,
            align,
            reloff,
            nreloc,
            flags,
            reserved1,
            reserved2,
            reserved3: 0,
        });

        remaining = input;
    }

    Ok((remaining, sections))
}

/// Parse 64-bit sections
fn parse_sections_64(input: &[u8], count: u32) -> IResult<&[u8], Vec<Section>> {
    let mut sections = Vec::with_capacity(count as usize);
    let mut remaining = input;

    for _ in 0..count {
        let (input, sectname_bytes) = take(16usize)(remaining)?;
        let (input, segname_bytes) = take(16usize)(input)?;
        let (input, addr) = le_u64(input)?;
        let (input, size) = le_u64(input)?;
        let (input, offset) = le_u32(input)?;
        let (input, align) = le_u32(input)?;
        let (input, reloff) = le_u32(input)?;
        let (input, nreloc) = le_u32(input)?;
        let (input, flags) = le_u32(input)?;
        let (input, reserved1) = le_u32(input)?;
        let (input, reserved2) = le_u32(input)?;
        let (input, reserved3) = le_u32(input)?;

        sections.push(Section {
            sectname: parse_name_16(sectname_bytes),
            segname: parse_name_16(segname_bytes),
            addr,
            size,
            offset,
            align,
            reloff,
            nreloc,
            flags,
            reserved1,
            reserved2,
            reserved3,
        });

        remaining = input;
    }

    Ok((remaining, sections))
}

// =============================================================================
// Dylib Commands
// =============================================================================

/// Parse a dylib command (LC_LOAD_DYLIB, LC_ID_DYLIB, etc.)
pub fn parse_dylib_command(input: &[u8]) -> IResult<&[u8], DylibCommand> {
    let full_input = input;

    let (input, cmd) = le_u32(input)?;
    let (input, cmdsize) = le_u32(input)?;
    let (input, name_offset) = le_u32(input)?;
    let (input, timestamp) = le_u32(input)?;
    let (input, current_version) = le_u32(input)?;
    let (input, compatibility_version) = le_u32(input)?;

    // Parse name from offset within the command
    let name = if name_offset as usize <= cmdsize as usize {
        let name_bytes = &full_input[name_offset as usize..cmdsize as usize];
        parse_c_string(name_bytes)
    } else {
        String::new()
    };

    // Skip to end of command
    let consumed = 24; // 6 * 4 bytes
    let remaining_bytes = cmdsize as usize - consumed;
    let (input, _) = take(remaining_bytes)(input)?;

    Ok((
        input,
        DylibCommand {
            cmd,
            name,
            timestamp,
            current_version,
            compatibility_version,
        },
    ))
}

// =============================================================================
// UUID Command
// =============================================================================

/// Parse a UUID command (LC_UUID)
pub fn parse_uuid_command(input: &[u8]) -> IResult<&[u8], UuidCommand> {
    let (input, _cmd) = le_u32(input)?;
    let (input, _cmdsize) = le_u32(input)?;
    let (input, uuid_bytes) = take(16usize)(input)?;

    let mut uuid = [0u8; 16];
    uuid.copy_from_slice(uuid_bytes);

    Ok((input, UuidCommand { uuid }))
}

// =============================================================================
// Version Commands
// =============================================================================

/// Parse a version_min command (LC_VERSION_MIN_*)
pub fn parse_version_min_command(input: &[u8]) -> IResult<&[u8], VersionMinCommand> {
    let (input, cmd) = le_u32(input)?;
    let (input, _cmdsize) = le_u32(input)?;
    let (input, version) = le_u32(input)?;
    let (input, sdk) = le_u32(input)?;

    Ok((input, VersionMinCommand { cmd, version, sdk }))
}

/// Parse a build version command (LC_BUILD_VERSION)
pub fn parse_build_version_command(input: &[u8]) -> IResult<&[u8], BuildVersionCommand> {
    let (input, _cmd) = le_u32(input)?;
    let (input, _cmdsize) = le_u32(input)?;
    let (input, platform) = le_u32(input)?;
    let (input, minos) = le_u32(input)?;
    let (input, sdk) = le_u32(input)?;
    let (input, ntools) = le_u32(input)?;

    // Parse tool versions
    let (input, tools) = parse_build_tool_versions(input, ntools)?;

    Ok((
        input,
        BuildVersionCommand {
            platform,
            minos,
            sdk,
            ntools,
            tools,
        },
    ))
}

/// Parse build tool version entries
fn parse_build_tool_versions(input: &[u8], count: u32) -> IResult<&[u8], Vec<BuildToolVersion>> {
    let mut tools = Vec::with_capacity(count as usize);
    let mut remaining = input;

    for _ in 0..count {
        let (input, tool) = le_u32(remaining)?;
        let (input, version) = le_u32(input)?;
        tools.push(BuildToolVersion { tool, version });
        remaining = input;
    }

    Ok((remaining, tools))
}

/// Parse a source version command (LC_SOURCE_VERSION)
pub fn parse_source_version_command(input: &[u8]) -> IResult<&[u8], SourceVersionCommand> {
    let (input, _cmd) = le_u32(input)?;
    let (input, _cmdsize) = le_u32(input)?;
    let (input, version) = le_u64(input)?;

    Ok((input, SourceVersionCommand { version }))
}

// =============================================================================
// Entry Point Command
// =============================================================================

/// Parse a main entry point command (LC_MAIN)
pub fn parse_entry_point_command(input: &[u8]) -> IResult<&[u8], EntryPointCommand> {
    let (input, _cmd) = le_u32(input)?;
    let (input, _cmdsize) = le_u32(input)?;
    let (input, entryoff) = le_u64(input)?;
    let (input, stacksize) = le_u64(input)?;

    Ok((
        input,
        EntryPointCommand {
            entryoff,
            stacksize,
        },
    ))
}

// =============================================================================
// Symbol Table Commands
// =============================================================================

/// Parse a symbol table command (LC_SYMTAB)
pub fn parse_symtab_command(input: &[u8]) -> IResult<&[u8], SymtabCommand> {
    let (input, _cmd) = le_u32(input)?;
    let (input, _cmdsize) = le_u32(input)?;
    let (input, symoff) = le_u32(input)?;
    let (input, nsyms) = le_u32(input)?;
    let (input, stroff) = le_u32(input)?;
    let (input, strsize) = le_u32(input)?;

    Ok((
        input,
        SymtabCommand {
            symoff,
            nsyms,
            stroff,
            strsize,
        },
    ))
}

/// Parse a dynamic symbol table command (LC_DYSYMTAB)
pub fn parse_dysymtab_command(input: &[u8]) -> IResult<&[u8], DysymtabCommand> {
    let (input, _cmd) = le_u32(input)?;
    let (input, _cmdsize) = le_u32(input)?;
    let (input, ilocalsym) = le_u32(input)?;
    let (input, nlocalsym) = le_u32(input)?;
    let (input, iextdefsym) = le_u32(input)?;
    let (input, nextdefsym) = le_u32(input)?;
    let (input, iundefsym) = le_u32(input)?;
    let (input, nundefsym) = le_u32(input)?;
    let (input, tocoff) = le_u32(input)?;
    let (input, ntoc) = le_u32(input)?;
    let (input, modtaboff) = le_u32(input)?;
    let (input, nmodtab) = le_u32(input)?;
    let (input, extrefsymoff) = le_u32(input)?;
    let (input, nextrefsyms) = le_u32(input)?;
    let (input, indirectsymoff) = le_u32(input)?;
    let (input, nindirectsyms) = le_u32(input)?;
    let (input, extreloff) = le_u32(input)?;
    let (input, nextrel) = le_u32(input)?;
    let (input, locreloff) = le_u32(input)?;
    let (input, nlocrel) = le_u32(input)?;

    Ok((
        input,
        DysymtabCommand {
            ilocalsym,
            nlocalsym,
            iextdefsym,
            nextdefsym,
            iundefsym,
            nundefsym,
            tocoff,
            ntoc,
            modtaboff,
            nmodtab,
            extrefsymoff,
            nextrefsyms,
            indirectsymoff,
            nindirectsyms,
            extreloff,
            nextrel,
            locreloff,
            nlocrel,
        },
    ))
}

// =============================================================================
// Linkedit Data Commands
// =============================================================================

/// Parse a linkedit data command (LC_CODE_SIGNATURE, LC_FUNCTION_STARTS, etc.)
pub fn parse_linkedit_data_command(input: &[u8]) -> IResult<&[u8], LinkeditDataCommand> {
    let (input, cmd) = le_u32(input)?;
    let (input, _cmdsize) = le_u32(input)?;
    let (input, dataoff) = le_u32(input)?;
    let (input, datasize) = le_u32(input)?;

    Ok((
        input,
        LinkeditDataCommand {
            cmd,
            dataoff,
            datasize,
        },
    ))
}

// =============================================================================
// Rpath Command
// =============================================================================

/// Parse an rpath command (LC_RPATH)
pub fn parse_rpath_command(input: &[u8]) -> IResult<&[u8], RpathCommand> {
    let full_input = input;

    let (input, _cmd) = le_u32(input)?;
    let (input, cmdsize) = le_u32(input)?;
    let (input, path_offset) = le_u32(input)?;

    // Parse path from offset within the command
    let path = if path_offset as usize <= cmdsize as usize {
        let path_bytes = &full_input[path_offset as usize..cmdsize as usize];
        parse_c_string(path_bytes)
    } else {
        String::new()
    };

    // Skip to end of command
    let consumed = 12; // 3 * 4 bytes
    let remaining_bytes = cmdsize as usize - consumed;
    let (input, _) = take(remaining_bytes)(input)?;

    Ok((input, RpathCommand { path }))
}

// =============================================================================
// Encryption Info Command
// =============================================================================

/// Parse an encryption info command (LC_ENCRYPTION_INFO or LC_ENCRYPTION_INFO_64)
pub fn parse_encryption_info_command(
    input: &[u8],
    is_64bit: bool,
) -> IResult<&[u8], EncryptionInfoCommand> {
    let (input, _cmd) = le_u32(input)?;
    let (input, _cmdsize) = le_u32(input)?;
    let (input, cryptoff) = le_u32(input)?;
    let (input, cryptsize) = le_u32(input)?;
    let (input, cryptid) = le_u32(input)?;

    // 64-bit version has a padding field
    let input = if is_64bit {
        let (input, _pad) = le_u32(input)?;
        input
    } else {
        input
    };

    Ok((
        input,
        EncryptionInfoCommand {
            cryptoff,
            cryptsize,
            cryptid,
        },
    ))
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Parse a 16-byte null-padded name into a String
fn parse_name_16(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

/// Parse a null-terminated C string
fn parse_c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

// =============================================================================
// Load Command Dispatcher
// =============================================================================

/// Parsed load command variants
#[derive(Debug, Clone)]
pub enum LoadCommand {
    /// Segment command (LC_SEGMENT or LC_SEGMENT_64)
    Segment(SegmentCommand),
    /// Dynamic library command (LC_LOAD_DYLIB, LC_ID_DYLIB, etc.)
    Dylib(DylibCommand),
    /// UUID command (LC_UUID)
    Uuid(UuidCommand),
    /// Version min command (LC_VERSION_MIN_*)
    VersionMin(VersionMinCommand),
    /// Build version command (LC_BUILD_VERSION)
    BuildVersion(BuildVersionCommand),
    /// Source version command (LC_SOURCE_VERSION)
    SourceVersion(SourceVersionCommand),
    /// Entry point command (LC_MAIN)
    EntryPoint(EntryPointCommand),
    /// Symbol table command (LC_SYMTAB)
    Symtab(SymtabCommand),
    /// Dynamic symbol table command (LC_DYSYMTAB)
    Dysymtab(DysymtabCommand),
    /// Linkedit data command (LC_CODE_SIGNATURE, LC_FUNCTION_STARTS, etc.)
    LinkeditData(LinkeditDataCommand),
    /// Rpath command (LC_RPATH)
    Rpath(RpathCommand),
    /// Encryption info command (LC_ENCRYPTION_INFO*)
    EncryptionInfo(EncryptionInfoCommand),
    /// Unknown or unhandled load command
    Unknown(LoadCommandHeader),
}

/// Parse a single load command from the input
///
/// Returns the parsed command and advances past the entire command.
pub fn parse_load_command(input: &[u8], _is_64bit: bool) -> IResult<&[u8], LoadCommand> {
    // First peek at the header to determine command type and size
    let (_, header) = parse_load_command_header(input)?;

    // Ensure we have enough data for the entire command
    if input.len() < header.cmdsize as usize {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TooLarge,
        )));
    }

    // Take the entire command data
    let (remaining, cmd_data) = take(header.cmdsize as usize)(input)?;

    // Parse based on command type
    let cmd = match header.cmd {
        load_command::LC_SEGMENT => {
            let (_, seg) = parse_segment_command_32(cmd_data)?;
            LoadCommand::Segment(seg)
        }
        load_command::LC_SEGMENT_64 => {
            let (_, seg) = parse_segment_command_64(cmd_data)?;
            LoadCommand::Segment(seg)
        }
        load_command::LC_LOAD_DYLIB
        | load_command::LC_ID_DYLIB
        | load_command::LC_LOAD_WEAK_DYLIB
        | load_command::LC_REEXPORT_DYLIB
        | load_command::LC_LAZY_LOAD_DYLIB
        | load_command::LC_LOAD_UPWARD_DYLIB => {
            let (_, dylib) = parse_dylib_command(cmd_data)?;
            LoadCommand::Dylib(dylib)
        }
        load_command::LC_UUID => {
            let (_, uuid) = parse_uuid_command(cmd_data)?;
            LoadCommand::Uuid(uuid)
        }
        load_command::LC_VERSION_MIN_MACOSX
        | load_command::LC_VERSION_MIN_IPHONEOS
        | load_command::LC_VERSION_MIN_WATCHOS
        | load_command::LC_VERSION_MIN_TVOS => {
            let (_, ver) = parse_version_min_command(cmd_data)?;
            LoadCommand::VersionMin(ver)
        }
        load_command::LC_BUILD_VERSION => {
            let (_, build) = parse_build_version_command(cmd_data)?;
            LoadCommand::BuildVersion(build)
        }
        load_command::LC_SOURCE_VERSION => {
            let (_, src) = parse_source_version_command(cmd_data)?;
            LoadCommand::SourceVersion(src)
        }
        load_command::LC_MAIN => {
            let (_, entry) = parse_entry_point_command(cmd_data)?;
            LoadCommand::EntryPoint(entry)
        }
        load_command::LC_SYMTAB => {
            let (_, symtab) = parse_symtab_command(cmd_data)?;
            LoadCommand::Symtab(symtab)
        }
        load_command::LC_DYSYMTAB => {
            let (_, dysymtab) = parse_dysymtab_command(cmd_data)?;
            LoadCommand::Dysymtab(dysymtab)
        }
        load_command::LC_CODE_SIGNATURE
        | load_command::LC_SEGMENT_SPLIT_INFO
        | load_command::LC_FUNCTION_STARTS
        | load_command::LC_DATA_IN_CODE
        | load_command::LC_DYLIB_CODE_SIGN_DRS
        | load_command::LC_LINKER_OPTIMIZATION_HINT
        | load_command::LC_DYLD_EXPORTS_TRIE
        | load_command::LC_DYLD_CHAINED_FIXUPS => {
            let (_, linkedit) = parse_linkedit_data_command(cmd_data)?;
            LoadCommand::LinkeditData(linkedit)
        }
        load_command::LC_DYLD_INFO | load_command::LC_DYLD_INFO_ONLY => {
            // DYLD_INFO has a different structure but we'll treat it as linkedit for now
            let (_, linkedit) = parse_linkedit_data_command(cmd_data)?;
            LoadCommand::LinkeditData(linkedit)
        }
        load_command::LC_RPATH => {
            let (_, rpath) = parse_rpath_command(cmd_data)?;
            LoadCommand::Rpath(rpath)
        }
        load_command::LC_ENCRYPTION_INFO => {
            let (_, enc) = parse_encryption_info_command(cmd_data, false)?;
            LoadCommand::EncryptionInfo(enc)
        }
        load_command::LC_ENCRYPTION_INFO_64 => {
            let (_, enc) = parse_encryption_info_command(cmd_data, true)?;
            LoadCommand::EncryptionInfo(enc)
        }
        _ => LoadCommand::Unknown(header),
    };

    Ok((remaining, cmd))
}

/// Parse all load commands from input
pub fn parse_all_load_commands(
    input: &[u8],
    ncmds: u32,
    is_64bit: bool,
) -> IResult<&[u8], Vec<LoadCommand>> {
    let mut commands = Vec::with_capacity(ncmds as usize);
    let mut remaining = input;

    for _ in 0..ncmds {
        let (rest, cmd) = parse_load_command(remaining, is_64bit)?;
        commands.push(cmd);
        remaining = rest;
    }

    Ok((remaining, commands))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_name_16() {
        let bytes = b"__TEXT\0\0\0\0\0\0\0\0\0\0";
        assert_eq!(parse_name_16(bytes), "__TEXT");

        let bytes = b"__LINKEDIT123456";
        assert_eq!(parse_name_16(bytes), "__LINKEDIT123456");
    }

    #[test]
    fn test_parse_c_string() {
        assert_eq!(parse_c_string(b"hello\0world"), "hello");
        assert_eq!(parse_c_string(b"hello"), "hello");
        assert_eq!(parse_c_string(b"\0"), "");
    }

    #[test]
    fn test_parse_load_command_header() {
        let mut data = Vec::new();
        data.extend_from_slice(&load_command::LC_SEGMENT_64.to_le_bytes());
        data.extend_from_slice(&72u32.to_le_bytes()); // cmdsize

        let result = parse_load_command_header(&data);
        assert!(result.is_ok());

        let (_, header) = result.unwrap();
        assert_eq!(header.cmd, load_command::LC_SEGMENT_64);
        assert_eq!(header.cmdsize, 72);
    }

    #[test]
    fn test_parse_uuid_command() {
        let mut data = Vec::new();
        data.extend_from_slice(&load_command::LC_UUID.to_le_bytes());
        data.extend_from_slice(&24u32.to_le_bytes()); // cmdsize
        data.extend_from_slice(&[
            0x55, 0x0E, 0x84, 0x00, 0xE2, 0x9B, 0x41, 0xD4, 0xA7, 0x16, 0x44, 0x66, 0x55, 0x44,
            0x00, 0x00,
        ]);

        let result = parse_uuid_command(&data);
        assert!(result.is_ok());

        let (_, uuid) = result.unwrap();
        assert_eq!(uuid.uuid[0], 0x55);
        assert_eq!(uuid.uuid[15], 0x00);
    }

    #[test]
    fn test_parse_version_min_command() {
        let mut data = Vec::new();
        data.extend_from_slice(&load_command::LC_VERSION_MIN_MACOSX.to_le_bytes());
        data.extend_from_slice(&16u32.to_le_bytes()); // cmdsize
        data.extend_from_slice(&0x000B0000u32.to_le_bytes()); // version 11.0.0
        data.extend_from_slice(&0x000C0100u32.to_le_bytes()); // sdk 12.1.0

        let result = parse_version_min_command(&data);
        assert!(result.is_ok());

        let (_, ver) = result.unwrap();
        assert_eq!(ver.cmd, load_command::LC_VERSION_MIN_MACOSX);
        assert_eq!(ver.version_string(), "11.0.0");
        assert_eq!(ver.sdk_string(), "12.1.0");
    }

    #[test]
    fn test_parse_source_version_command() {
        let mut data = Vec::new();
        data.extend_from_slice(&load_command::LC_SOURCE_VERSION.to_le_bytes());
        data.extend_from_slice(&16u32.to_le_bytes()); // cmdsize
        // Version 1.2.3.4.5 encoded
        let version: u64 = (1 << 40) | (2 << 30) | (3 << 20) | (4 << 10) | 5;
        data.extend_from_slice(&version.to_le_bytes());

        let result = parse_source_version_command(&data);
        assert!(result.is_ok());

        let (_, src) = result.unwrap();
        assert_eq!(src.version_string(), "1.2.3.4.5");
    }

    #[test]
    fn test_parse_symtab_command() {
        let mut data = Vec::new();
        data.extend_from_slice(&load_command::LC_SYMTAB.to_le_bytes());
        data.extend_from_slice(&24u32.to_le_bytes()); // cmdsize
        data.extend_from_slice(&0x1000u32.to_le_bytes()); // symoff
        data.extend_from_slice(&100u32.to_le_bytes()); // nsyms
        data.extend_from_slice(&0x2000u32.to_le_bytes()); // stroff
        data.extend_from_slice(&0x500u32.to_le_bytes()); // strsize

        let result = parse_symtab_command(&data);
        assert!(result.is_ok());

        let (_, symtab) = result.unwrap();
        assert_eq!(symtab.symoff, 0x1000);
        assert_eq!(symtab.nsyms, 100);
        assert_eq!(symtab.stroff, 0x2000);
        assert_eq!(symtab.strsize, 0x500);
    }

    #[test]
    fn test_parse_entry_point_command() {
        let mut data = Vec::new();
        data.extend_from_slice(&load_command::LC_MAIN.to_le_bytes());
        data.extend_from_slice(&24u32.to_le_bytes()); // cmdsize
        data.extend_from_slice(&0x4000u64.to_le_bytes()); // entryoff
        data.extend_from_slice(&0x100000u64.to_le_bytes()); // stacksize

        let result = parse_entry_point_command(&data);
        assert!(result.is_ok());

        let (_, entry) = result.unwrap();
        assert_eq!(entry.entryoff, 0x4000);
        assert_eq!(entry.stacksize, 0x100000);
    }
}
