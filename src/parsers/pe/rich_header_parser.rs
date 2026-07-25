//! Rich Header parser
//!
//! This module parses the undocumented Rich Header structure present in PE files
//! compiled with Microsoft tools. The Rich Header contains information about the
//! tools and compilers used to build the executable.

use crate::io::EndianReader;

/// Rich Header Entry - represents a single tool/compiler used to build the PE
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RichHeaderEntry {
    /// Compiler/tool product ID
    pub product_id: u16,
    /// Build number of the tool
    pub build_number: u16,
    /// Number of times this tool was used
    pub use_count: u32,
}

/// Rich Header - undocumented structure between DOS stub and PE header
#[derive(Debug, Clone)]
pub struct RichHeader {
    /// XOR key used to encrypt the header (also serves as checksum)
    pub checksum: u32,
    /// List of compiler/tool entries
    pub entries: Vec<RichHeaderEntry>,
    /// Raw decrypted Rich header data (for hashing)
    pub raw_data: Vec<u8>,
}

impl RichHeader {
    /// Returns a formatted string with all compiler/tool entries
    pub fn compiler_info_string(&self) -> String {
        self.entries
            .iter()
            .map(|e| format!("{}.{} x{}", e.product_id, e.build_number, e.use_count))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Returns a comma-separated list of unique product IDs
    pub fn product_ids_string(&self) -> String {
        let mut product_ids: Vec<u16> = self.entries.iter().map(|e| e.product_id).collect();
        product_ids.sort_unstable();
        product_ids.dedup();
        product_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Returns the MD5 hash of the decrypted Rich header (for forensic comparison)
    pub fn hash_md5(&self) -> String {
        let digest = md5::compute(&self.raw_data);
        format!("{:x}", digest)
    }

    /// Returns a human-readable product name for a given product ID
    pub fn product_name(product_id: u16) -> &'static str {
        match product_id {
            0x00 => "Unknown",
            0x01 => "Import0",
            0x02 => "Linker510",
            0x03 => "Cvtomf510",
            0x04 => "Linker600",
            0x05 => "Cvtomf600",
            0x06 => "Cvtres500",
            0x0A => "Utc11_Basic",
            0x0B => "Utc11_C",
            0x0C => "Utc12_Basic",
            0x0D => "Utc12_C",
            0x0E => "Utc12_CPP",
            0x0F => "AliasObj60",
            0x10 => "VisualBasic60",
            0x11 => "Masm613",
            0x12 => "Masm710",
            0x13 => "Linker511",
            0x14 => "Cvtomf511",
            0x15 => "Masm614",
            0x16 => "Linker512",
            0x17 => "Cvtomf512",
            0x1C => "Utc12_C_Std",
            0x1D => "Utc12_CPP_Std",
            0x1E => "Utc12_C_Book",
            0x1F => "Utc12_CPP_Book",
            0x5D => "AliasObj70",
            0x5E => "VisualBasic70",
            0x5F => "Masm800",
            0x60 => "AliasObj71",
            0x61 => "Linker700",
            0x62 => "Cvtomf700",
            0x63 => "Utc13_Basic",
            0x64 => "Utc13_C",
            0x65 => "Utc13_CPP",
            0x66 => "Linker710",
            0x67 => "Cvtomf710",
            0x68 => "Export710",
            0x69 => "Implib710",
            0x6A => "Masm710_ML",
            0x83 => "Masm800_ML",
            0x84 => "AliasObj80",
            0x85 => "PhoenixPrerelease",
            0x86 => "Utc14_Basic",
            0x87 => "Utc14_C",
            0x88 => "Utc14_CPP",
            0x89 => "Utc15_Basic",
            0x8A => "Utc15_C",
            0x8B => "Utc15_CPP",
            0x8C => "Linker800",
            0x8D => "Cvtomf800",
            0x8E => "Export800",
            0x8F => "Implib800",
            0x90 => "Cvtres800",
            0x91 => "Masm800_ML64",
            0x92 => "AliasObj90",
            0x93 => "PhoenixBeta",
            0x94 => "Utc16_Basic",
            0x95 => "Utc16_C",
            0x96 => "Utc16_CPP",
            0x97 => "Utc17_Basic",
            0x98 => "Utc17_C",
            0x99 => "Utc17_CPP",
            0x9A => "Linker900",
            0x9B => "Cvtomf900",
            0x9C => "Export900",
            0x9D => "Implib900",
            0x9E => "Cvtres900",
            0x9F => "Masm900_ML",
            0xA0 => "Masm900_ML64",
            0xA1 => "AliasObj100",
            0xA2 => "Utc18_Basic",
            0xA3 => "Utc18_C",
            0xA4 => "Utc18_CPP",
            0xA5 => "Utc19_Basic",
            0xA6 => "Utc19_C",
            0xA7 => "Utc19_CPP",
            0xDB => "Cvtres1000",
            0xDC => "Export1000",
            0xDD => "Implib1000",
            0xDE => "Linker1000",
            0xDF => "Masm1000",
            _ => "Unknown",
        }
    }
}

/// Signature constants
const RICH_SIGNATURE: u32 = 0x68636952; // "Rich" in little-endian
const DANS_SIGNATURE: u32 = 0x536E6144; // "DanS" in little-endian

/// Attempts to find and parse the Rich Header from PE data
///
/// The Rich Header is located between the DOS stub and the PE header.
/// It is XOR-encrypted with a key that appears after the "Rich" signature.
///
/// # Arguments
/// * `data` - Raw PE file data starting from DOS header
/// * `dos_stub_end` - Offset where DOS stub ends (typically 0x80)
/// * `pe_offset` - Offset to the PE header (from DOS header e_lfanew)
///
/// # Returns
/// * `Some(RichHeader)` if a valid Rich Header was found
/// * `None` if no Rich Header exists or parsing failed
pub fn parse_rich_header(data: &[u8], dos_stub_end: usize, pe_offset: usize) -> Option<RichHeader> {
    // The Rich Header is between DOS stub and PE header
    if dos_stub_end >= pe_offset || pe_offset < dos_stub_end + 16 {
        return None;
    }

    // Search for "Rich" signature in the region between DOS stub and PE header
    let search_region = &data[dos_stub_end..pe_offset];
    let rich_pos = find_signature(search_region, RICH_SIGNATURE)?;
    let rich_offset = dos_stub_end + rich_pos;

    // The XOR key (checksum) follows immediately after "Rich"
    if rich_offset + 8 > data.len() {
        return None;
    }

    let reader = EndianReader::little_endian(data);
    let xor_key = reader.u32_at(rich_offset + 4)?;

    // Now search backwards for the start of the Rich Header (encrypted "DanS")
    let encrypted_dans = DANS_SIGNATURE ^ xor_key;
    let start_pos = find_signature_backwards(&data[dos_stub_end..rich_offset], encrypted_dans)?;
    let header_start = dos_stub_end + start_pos;

    // Extract and decrypt the Rich Header data
    let header_len = rich_offset - header_start;
    if !header_len.is_multiple_of(4) || header_len < 16 {
        return None;
    }

    let encrypted_data = &data[header_start..rich_offset];
    let encrypted_reader = EndianReader::little_endian(encrypted_data);
    let mut decrypted_data = Vec::with_capacity(encrypted_data.len());

    for i in (0..encrypted_data.len()).step_by(4) {
        if let Some(encrypted) = encrypted_reader.u32_at(i) {
            let decrypted = encrypted ^ xor_key;
            decrypted_data.extend_from_slice(&decrypted.to_le_bytes());
        }
    }

    // Verify "DanS" signature at the start
    if decrypted_data.len() < 4 {
        return None;
    }

    let decrypted_reader = EndianReader::little_endian(&decrypted_data);
    let dans_sig = decrypted_reader.u32_at(0)?;

    if dans_sig != DANS_SIGNATURE {
        return None;
    }

    // Parse entries: skip "DanS" (4 bytes) + 3 zero DWORDs (12 bytes)
    let mut entries = Vec::new();
    let mut offset = 16;

    while offset + 8 <= decrypted_data.len() {
        let compid = decrypted_reader.u32_at(offset).unwrap_or(0);
        let count = decrypted_reader.u32_at(offset + 4).unwrap_or(0);

        // compid format: (build_number << 16) | product_id
        let product_id = (compid & 0xFFFF) as u16;
        let build_number = (compid >> 16) as u16;

        // Skip padding entries (all zeros)
        if compid != 0 || count != 0 {
            entries.push(RichHeaderEntry {
                product_id,
                build_number,
                use_count: count,
            });
        }

        offset += 8;
    }

    Some(RichHeader {
        checksum: xor_key,
        entries,
        raw_data: decrypted_data,
    })
}

/// Find a 4-byte signature in a byte slice (forward search)
fn find_signature(data: &[u8], signature: u32) -> Option<usize> {
    let sig_bytes = signature.to_le_bytes();
    data.windows(4).position(|window| window == sig_bytes)
}

/// Find a 4-byte signature in a byte slice (backward search)
fn find_signature_backwards(data: &[u8], signature: u32) -> Option<usize> {
    let sig_bytes = signature.to_le_bytes();
    data.windows(4).rposition(|window| window == sig_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rich_header_with_sample_data() {
        // Create a sample Rich Header for testing
        // XOR key: 0x12345678
        let xor_key = 0x12345678u32;

        // Create encrypted "DanS" signature
        let dans_encrypted = DANS_SIGNATURE ^ xor_key;

        // Create some sample entries
        // Entry 1: Product 0x0095 (Utc16_C), Build 0x7809, Count 5
        let compid1 = 0x78090095u32 ^ xor_key;
        let count1 = 0x00000005u32 ^ xor_key;

        // Entry 2: Product 0x009A (Linker900), Build 0x7809, Count 1
        let compid2 = 0x7809009Au32 ^ xor_key;
        let count2 = 0x00000001u32 ^ xor_key;

        // Build encrypted Rich Header data
        let mut rich_data = Vec::new();
        rich_data.extend_from_slice(&dans_encrypted.to_le_bytes()); // "DanS" encrypted
        rich_data.extend_from_slice(&(0u32 ^ xor_key).to_le_bytes()); // Padding 1
        rich_data.extend_from_slice(&(0u32 ^ xor_key).to_le_bytes()); // Padding 2
        rich_data.extend_from_slice(&(0u32 ^ xor_key).to_le_bytes()); // Padding 3
        rich_data.extend_from_slice(&compid1.to_le_bytes()); // Entry 1 compid
        rich_data.extend_from_slice(&count1.to_le_bytes()); // Entry 1 count
        rich_data.extend_from_slice(&compid2.to_le_bytes()); // Entry 2 compid
        rich_data.extend_from_slice(&count2.to_le_bytes()); // Entry 2 count

        // Create full PE data with DOS stub, Rich Header, and PE header marker
        let dos_stub_end = 0x80;
        let pe_offset = 0x80 + rich_data.len() + 8; // +8 for "Rich" + checksum

        let mut pe_data = vec![0u8; pe_offset + 16];
        pe_data[dos_stub_end..dos_stub_end + rich_data.len()].copy_from_slice(&rich_data);
        pe_data[dos_stub_end + rich_data.len()..dos_stub_end + rich_data.len() + 4]
            .copy_from_slice(&RICH_SIGNATURE.to_le_bytes());
        pe_data[dos_stub_end + rich_data.len() + 4..dos_stub_end + rich_data.len() + 8]
            .copy_from_slice(&xor_key.to_le_bytes());

        // Parse the Rich Header
        let result = parse_rich_header(&pe_data, dos_stub_end, pe_offset);

        assert!(result.is_some());
        let rich = result.unwrap();

        assert_eq!(rich.checksum, xor_key);
        assert_eq!(rich.entries.len(), 2);

        assert_eq!(rich.entries[0].product_id, 0x0095);
        assert_eq!(rich.entries[0].build_number, 0x7809);
        assert_eq!(rich.entries[0].use_count, 5);

        assert_eq!(rich.entries[1].product_id, 0x009A);
        assert_eq!(rich.entries[1].build_number, 0x7809);
        assert_eq!(rich.entries[1].use_count, 1);
    }

    #[test]
    fn test_rich_header_compiler_info_string() {
        let rich = RichHeader {
            checksum: 0x12345678,
            entries: vec![
                RichHeaderEntry {
                    product_id: 0x95,
                    build_number: 0x7809,
                    use_count: 5,
                },
                RichHeaderEntry {
                    product_id: 0x9A,
                    build_number: 0x7809,
                    use_count: 1,
                },
            ],
            raw_data: vec![],
        };

        let info = rich.compiler_info_string();
        assert_eq!(info, "149.30729 x5, 154.30729 x1");
    }

    #[test]
    fn test_rich_header_product_ids_string() {
        let rich = RichHeader {
            checksum: 0x12345678,
            entries: vec![
                RichHeaderEntry {
                    product_id: 0x95,
                    build_number: 0x7809,
                    use_count: 5,
                },
                RichHeaderEntry {
                    product_id: 0x9A,
                    build_number: 0x7809,
                    use_count: 1,
                },
                RichHeaderEntry {
                    product_id: 0x95, // Duplicate
                    build_number: 0x7810,
                    use_count: 2,
                },
            ],
            raw_data: vec![],
        };

        let ids = rich.product_ids_string();
        assert_eq!(ids, "149, 154");
    }

    #[test]
    fn test_product_name_mapping() {
        assert_eq!(RichHeader::product_name(0x01), "Import0");
        assert_eq!(RichHeader::product_name(0x95), "Utc16_C");
        assert_eq!(RichHeader::product_name(0x9A), "Linker900");
        assert_eq!(RichHeader::product_name(0xDE), "Linker1000");
        assert_eq!(RichHeader::product_name(0xFF), "Unknown");
    }

    #[test]
    fn test_parse_rich_header_missing() {
        // PE data without Rich Header
        let pe_data = vec![0u8; 256];
        let result = parse_rich_header(&pe_data, 0x80, 0xF0);
        assert!(result.is_none());
    }
}
