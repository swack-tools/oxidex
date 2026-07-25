//! Integration test for PE Rich Header extraction

use oxidex::parsers::pe::rich_header_parser::{RichHeader, RichHeaderEntry, parse_rich_header};

const RICH_SIGNATURE: u32 = 0x68636952; // "Rich" in little-endian
const DANS_SIGNATURE: u32 = 0x536E6144; // "DanS" in little-endian

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

#[test]
fn test_rich_header_hash() {
    let rich = RichHeader {
        checksum: 0x12345678,
        entries: vec![],
        raw_data: b"test data".to_vec(),
    };

    let hash = rich.hash_md5();
    // MD5 of "test data" is "eb733a00c0c9d336e65691a37ab54293"
    assert_eq!(hash, "eb733a00c0c9d336e65691a37ab54293");
}
