//! Code signature parsing
//!
//! This module handles parsing of Mach-O code signatures, including
//! the Code Directory, requirements, and entitlements.

use crate::io::EndianReader;
use nom::{
    IResult,
    number::complete::{be_u8, be_u32},
};

use super::structures::{
    BlobIndex, CodeDirectory, CodeSignatureInfo, SuperBlob, cs_magic, hash_type_name,
};

// =============================================================================
// SuperBlob Parsing
// =============================================================================

/// Parse a SuperBlob (container for all code signature data)
pub fn parse_super_blob(input: &[u8]) -> IResult<&[u8], SuperBlob> {
    let (input, magic) = be_u32(input)?;
    let (input, length) = be_u32(input)?;
    let (input, count) = be_u32(input)?;

    // Parse blob index entries
    let mut index = Vec::with_capacity(count as usize);
    let mut remaining = input;

    for _ in 0..count {
        let (input, blob_type) = be_u32(remaining)?;
        let (input, offset) = be_u32(input)?;
        index.push(BlobIndex { blob_type, offset });
        remaining = input;
    }

    Ok((
        remaining,
        SuperBlob {
            magic,
            length,
            count,
            index,
        },
    ))
}

// =============================================================================
// Code Directory Parsing
// =============================================================================

/// Parse a Code Directory blob
pub fn parse_code_directory(input: &[u8]) -> IResult<&[u8], CodeDirectory> {
    let full_input = input;

    let (input, magic) = be_u32(input)?;
    let (input, length) = be_u32(input)?;
    let (input, version) = be_u32(input)?;
    let (input, flags) = be_u32(input)?;
    let (input, hash_offset) = be_u32(input)?;
    let (input, ident_offset) = be_u32(input)?;
    let (input, n_special_slots) = be_u32(input)?;
    let (input, n_code_slots) = be_u32(input)?;
    let (input, code_limit) = be_u32(input)?;
    let (input, hash_size) = be_u8(input)?;
    let (input, hash_type) = be_u8(input)?;
    let (input, platform) = be_u8(input)?;
    let (input, page_size) = be_u8(input)?;
    let (input, _spare2) = be_u32(input)?;

    // Parse scatter offset (version >= 0x20100)
    let (input, _scatter_offset) = if version >= 0x20100 {
        be_u32(input)?
    } else {
        (input, 0)
    };

    // Parse team offset (version >= 0x20200)
    let (input, team_offset) = if version >= 0x20200 {
        be_u32(input)?
    } else {
        (input, 0)
    };

    // Extract identifier string
    let identifier = if ident_offset > 0 && (ident_offset as usize) < full_input.len() {
        parse_c_string(&full_input[ident_offset as usize..])
    } else {
        String::new()
    };

    // Extract team ID
    let team_id = if team_offset > 0 && (team_offset as usize) < full_input.len() {
        Some(parse_c_string(&full_input[team_offset as usize..]))
    } else {
        None
    };

    Ok((
        input,
        CodeDirectory {
            magic,
            length,
            version,
            flags,
            hash_offset,
            ident_offset,
            n_special_slots,
            n_code_slots,
            code_limit,
            hash_size,
            hash_type,
            platform,
            page_size,
            team_offset,
            identifier,
            team_id,
        },
    ))
}

// =============================================================================
// Code Signature Info Extraction
// =============================================================================

/// Parse code signature data and extract relevant information
pub fn parse_code_signature_info(data: &[u8]) -> Option<CodeSignatureInfo> {
    if data.len() < 12 {
        return None;
    }

    let mut info = CodeSignatureInfo {
        signature_size: data.len() as u32,
        ..Default::default()
    };

    // Check for SuperBlob magic
    let reader = EndianReader::big_endian(data);
    let Some(magic) = reader.u32_at(0) else {
        return Some(info);
    };
    if magic != cs_magic::CSMAGIC_EMBEDDED_SIGNATURE {
        return Some(info);
    }

    // Parse SuperBlob
    let super_blob = match parse_super_blob(data) {
        Ok((_, sb)) => sb,
        Err(_) => return Some(info),
    };

    info.is_signed = true;

    // Find and parse Code Directory, entitlements, and CMS signature
    for idx in &super_blob.index {
        if idx.blob_type == 0 {
            // CSSLOT_CODEDIRECTORY
            if (idx.offset as usize) < data.len()
                && let Ok((_, cd)) = parse_code_directory(&data[idx.offset as usize..])
            {
                info.identifier = if cd.identifier.is_empty() {
                    None
                } else {
                    Some(cd.identifier.clone())
                };
                info.team_id = cd.team_id.clone();
                info.hash_type = Some(hash_type_name(cd.hash_type).to_string());
                info.cd_version = cd.version;
                info.n_code_slots = cd.n_code_slots;
            }
        }

        // Check for entitlements
        if idx.blob_type == 5 {
            // CSSLOT_ENTITLEMENTS
            info.has_entitlements = true;

            // Try to extract entitlement keys
            if (idx.offset as usize) < data.len()
                && let Some(keys) = extract_entitlement_keys(&data[idx.offset as usize..])
            {
                info.entitlement_keys = keys;
            }
        }

        // Check for CMS signature
        if idx.blob_type == 0x10000 {
            // CSSLOT_SIGNATURESLOT
            info.has_cms_signature = true;

            // Try to extract signer name from CMS signature
            if (idx.offset as usize) < data.len()
                && let Some(signer) = extract_signer_from_cms(&data[idx.offset as usize..])
            {
                info.signer_name = Some(signer);
            }
        }
    }

    Some(info)
}

/// Extract entitlement keys from entitlements blob
fn extract_entitlement_keys(data: &[u8]) -> Option<Vec<String>> {
    // Entitlements are stored as a plist (XML) after the blob header
    if data.len() < 8 {
        return None;
    }

    // Skip blob header (magic + length)
    let reader = EndianReader::big_endian(data);
    let magic = reader.u32_at(0)?;
    if magic != cs_magic::CSMAGIC_BLOBWRAPPER && magic != 0xFADE7171 {
        // 0xFADE7171 is CSMAGIC_EMBEDDED_ENTITLEMENTS
        return None;
    }

    let plist_data = &data[8..];

    // Look for XML plist structure
    if plist_data.len() < 20 {
        return None;
    }

    // Convert to string for parsing
    let plist_str = String::from_utf8_lossy(plist_data);

    // Extract key names from <key>...</key> tags (simplified extraction)
    let mut keys = Vec::new();
    let mut pos = 0;

    while let Some(start) = plist_str[pos..].find("<key>") {
        let start_pos = pos + start + 5; // Skip "<key>"
        if let Some(end) = plist_str[start_pos..].find("</key>") {
            let key = plist_str[start_pos..start_pos + end].trim();
            // Filter for security-relevant entitlements
            if !key.is_empty()
                && key.len() < 200
                && (key.contains("com.apple")
                    || key.contains("security")
                    || key.contains("sandbox")
                    || key.contains("network")
                    || key.contains("keychain")
                    || key.contains("hardened"))
            {
                keys.push(key.to_string());
            }
            pos = start_pos + end + 6;
        } else {
            break;
        }

        // Limit to first 20 entitlements
        if keys.len() >= 20 {
            break;
        }
    }

    if keys.is_empty() { None } else { Some(keys) }
}

/// Extract signer common name from CMS signature blob
fn extract_signer_from_cms(data: &[u8]) -> Option<String> {
    // CMS signature is a PKCS#7 SignedData structure
    // This is a simplified extraction - full parsing would require ASN.1 DER parsing

    if data.len() < 8 {
        return None;
    }

    // Skip blob header (magic + length)
    let reader = EndianReader::big_endian(data);
    let magic = reader.u32_at(0)?;
    if magic != cs_magic::CSMAGIC_BLOBWRAPPER {
        return None;
    }

    let cms_data = &data[8..];

    // Look for common name OID (2.5.4.3) in the DER data
    // OID encoding: 0x55, 0x04, 0x03
    let cn_oid = &[0x55, 0x04, 0x03];

    // Search for CN OID in the certificate data
    for i in 0..cms_data.len().saturating_sub(20) {
        if cms_data[i..].starts_with(cn_oid) {
            // Try to extract the string value after the OID
            if let Some(name) = extract_der_string(&cms_data[i + 3..]) {
                // Filter for reasonable signer names
                if name.len() >= 3
                    && name.len() <= 200
                    && name.chars().all(|c| c.is_ascii_graphic() || c == ' ')
                {
                    // Skip common non-signer CNs
                    if !name.starts_with("Apple")
                        || name.contains("Developer")
                        || name.contains("Distribution")
                    {
                        return Some(name);
                    }
                }
            }
        }
    }

    None
}

/// Extract a DER-encoded string value
fn extract_der_string(data: &[u8]) -> Option<String> {
    if data.len() < 2 {
        return None;
    }

    // Check for string types: PrintableString (0x13), UTF8String (0x0C), IA5String (0x16)
    let tag = data[0];
    if !matches!(tag, 0x0C | 0x13 | 0x16) {
        return None;
    }

    let length = data[1] as usize;
    if length == 0 || data.len() < 2 + length {
        return None;
    }

    String::from_utf8(data[2..2 + length].to_vec()).ok()
}

/// Parse a null-terminated C string
fn parse_c_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

// =============================================================================
// Code Signature Flags
// =============================================================================

/// Code signature flags
pub mod cs_flags {
    /// If set, the code has been validated by the kernel
    pub const CS_VALID: u32 = 0x0000_0001;
    /// Dynamically valid
    pub const CS_ADHOC: u32 = 0x0000_0002;
    /// Has a code signature
    pub const CS_GET_TASK_ALLOW: u32 = 0x0000_0004;
    /// Has an entitlements blob
    pub const CS_INSTALLER: u32 = 0x0000_0008;
    /// Force hard protection
    pub const CS_FORCED_LV: u32 = 0x0000_0010;
    /// Invalid page found
    pub const CS_INVALID_ALLOWED: u32 = 0x0000_0020;
    /// Code is hard-signed
    pub const CS_HARD: u32 = 0x0000_0100;
    /// Code must be killed on failure
    pub const CS_KILL: u32 = 0x0000_0200;
    /// Dyld must verify
    pub const CS_CHECK_EXPIRATION: u32 = 0x0000_0400;
    /// Restrict privileges
    pub const CS_RESTRICT: u32 = 0x0000_0800;
    /// Enforcement of code signing
    pub const CS_ENFORCEMENT: u32 = 0x0000_1000;
    /// Library validation required
    pub const CS_REQUIRE_LV: u32 = 0x0000_2000;
    /// Code signature entitlements validated
    pub const CS_ENTITLEMENTS_VALIDATED: u32 = 0x0000_4000;
    /// NVRAM (platform code)
    pub const CS_NVRAM_UNRESTRICTED: u32 = 0x0000_8000;
    /// Code is runtime signed
    pub const CS_RUNTIME: u32 = 0x0001_0000;
    /// Linker signed
    pub const CS_LINKER_SIGNED: u32 = 0x0002_0000;
}

/// Decode code signature flags into names
pub fn decode_cs_flags(flags: u32) -> Vec<&'static str> {
    let mut result = Vec::new();

    if flags & cs_flags::CS_VALID != 0 {
        result.push("VALID");
    }
    if flags & cs_flags::CS_ADHOC != 0 {
        result.push("ADHOC");
    }
    if flags & cs_flags::CS_GET_TASK_ALLOW != 0 {
        result.push("GET_TASK_ALLOW");
    }
    if flags & cs_flags::CS_INSTALLER != 0 {
        result.push("INSTALLER");
    }
    if flags & cs_flags::CS_HARD != 0 {
        result.push("HARD");
    }
    if flags & cs_flags::CS_KILL != 0 {
        result.push("KILL");
    }
    if flags & cs_flags::CS_RESTRICT != 0 {
        result.push("RESTRICT");
    }
    if flags & cs_flags::CS_ENFORCEMENT != 0 {
        result.push("ENFORCEMENT");
    }
    if flags & cs_flags::CS_REQUIRE_LV != 0 {
        result.push("REQUIRE_LV");
    }
    if flags & cs_flags::CS_RUNTIME != 0 {
        result.push("RUNTIME");
    }
    if flags & cs_flags::CS_LINKER_SIGNED != 0 {
        result.push("LINKER_SIGNED");
    }

    result
}

/// Check if the signature appears to be ad-hoc signed
pub fn is_adhoc_signed(info: &CodeSignatureInfo) -> bool {
    !info.has_cms_signature && info.is_signed
}

/// Check if the signature has a valid team ID (Apple Developer)
pub fn has_developer_id(info: &CodeSignatureInfo) -> bool {
    info.team_id.as_ref().is_some_and(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_c_string() {
        assert_eq!(parse_c_string(b"hello\0world"), "hello");
        assert_eq!(parse_c_string(b"hello"), "hello");
        assert_eq!(parse_c_string(b"\0"), "");
    }

    #[test]
    fn test_decode_cs_flags() {
        let flags = cs_flags::CS_VALID | cs_flags::CS_HARD | cs_flags::CS_RUNTIME;
        let decoded = decode_cs_flags(flags);
        assert!(decoded.contains(&"VALID"));
        assert!(decoded.contains(&"HARD"));
        assert!(decoded.contains(&"RUNTIME"));
    }

    #[test]
    fn test_is_adhoc_signed() {
        let adhoc = CodeSignatureInfo {
            is_signed: true,
            has_cms_signature: false,
            ..Default::default()
        };
        assert!(is_adhoc_signed(&adhoc));

        let full = CodeSignatureInfo {
            is_signed: true,
            has_cms_signature: true,
            ..Default::default()
        };
        assert!(!is_adhoc_signed(&full));
    }

    #[test]
    fn test_has_developer_id() {
        let with_team = CodeSignatureInfo {
            team_id: Some("ABC123DEF".to_string()),
            ..Default::default()
        };
        assert!(has_developer_id(&with_team));

        let without_team = CodeSignatureInfo {
            team_id: None,
            ..Default::default()
        };
        assert!(!has_developer_id(&without_team));

        let empty_team = CodeSignatureInfo {
            team_id: Some(String::new()),
            ..Default::default()
        };
        assert!(!has_developer_id(&empty_team));
    }

    #[test]
    fn test_extract_der_string() {
        // UTF8String "Test"
        let utf8_string = vec![0x0C, 0x04, b'T', b'e', b's', b't'];
        assert_eq!(extract_der_string(&utf8_string), Some("Test".to_string()));

        // PrintableString "Hello"
        let printable = vec![0x13, 0x05, b'H', b'e', b'l', b'l', b'o'];
        assert_eq!(extract_der_string(&printable), Some("Hello".to_string()));

        // Invalid tag
        let invalid = vec![0x30, 0x04, b'T', b'e', b's', b't'];
        assert_eq!(extract_der_string(&invalid), None);
    }
}
