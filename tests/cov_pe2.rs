//! Coverage tests for PE parsers (wave 2): version_info_parser, signature_parser,
//! and metadata_extractor.
//!
//! Wave 1 (`cov_macho_pe_elf.rs`) covered the happy path for headers, sections,
//! and the low-level signature/structure helpers. This file goes after the
//! REMAINING uncovered paths:
//!   * `version_info_parser.rs` (was 0%): the entire VS_VERSION_INFO resource
//!     parser — VS_FIXEDFILEINFO, StringFileInfo, StringTable, String entries,
//!     VarFileInfo, the wide-string readers, and the many error/short branches.
//!   * `signature_parser.rs` (was 22%): full Authenticode certificate parsing —
//!     PKCS#7 SignedData with an embedded X.509 certificate carrying serial,
//!     issuer/subject Distinguished Names (CN + O), and validity dates encoded as
//!     both UTCTime and GeneralizedTime; long-form ASN.1 lengths.
//!   * `metadata_extractor.rs` (was 61%): `extract_version_info_metadata` (both
//!     the populated-flags and "(none)" branches) and `extract_signature_metadata`
//!     (every optional field set, and the not-present early return).
//!
//! Fixtures are driven through the public API:
//!   * `parse_version_info` / `parse_vs_fixed_file_info` directly.
//!   * `parse_win_certificate` / `parse_signature_info` directly.
//!   * `extract_version_info_metadata` / `extract_signature_metadata` directly.
//!   * a fully assembled PE (DOS + Rich + COFF + Optional + sections) with a
//!     `.rsrc` section carrying an RT_VERSION resource and a Security data
//!     directory pointing at an Authenticode WIN_CERTIFICATE, run through
//!     `parse_pe_metadata`.

#[path = "common/mod.rs"]
mod common;

use common::TestReader;

use oxidex::core::MetadataMap;
use oxidex::parsers::pe::metadata_extractor::{
    extract_signature_metadata, extract_version_info_metadata,
};
use oxidex::parsers::pe::parse_pe_metadata;
use oxidex::parsers::pe::signature_parser::{
    SignatureInfo, cert_revision, cert_type, parse_signature_info, parse_win_certificate,
};
use oxidex::parsers::pe::structures::{VsFixedFileInfo, resource_types, subsystem_types};
use oxidex::parsers::pe::version_info_parser::{parse_version_info, parse_vs_fixed_file_info};
use std::collections::HashMap;

// =============================================================================
// Wide-string + version-info fixture helpers
// =============================================================================

/// Encode an ASCII string as a null-terminated UTF-16LE wide string.
fn wide_z(s: &str) -> Vec<u8> {
    let mut v = Vec::new();
    for c in s.encode_utf16() {
        v.extend_from_slice(&c.to_le_bytes());
    }
    v.extend_from_slice(&0u16.to_le_bytes()); // null terminator
    v
}

/// Pad a buffer to the next 4-byte boundary (DWORD alignment), as required
/// between VERSION_INFO sub-structures.
fn align4(v: &mut Vec<u8>) {
    while v.len() % 4 != 0 {
        v.push(0);
    }
}

/// Build a raw 52-byte VS_FIXEDFILEINFO blob with the given version words.
fn fixed_file_info_bytes(
    file_ms: u32,
    file_ls: u32,
    prod_ms: u32,
    prod_ls: u32,
    flags_mask: u32,
    flags: u32,
    file_os: u32,
    file_type: u32,
    file_subtype: u32,
) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&0xFEEF04BDu32.to_le_bytes()); // signature
    d.extend_from_slice(&0x00010000u32.to_le_bytes()); // struct_version
    d.extend_from_slice(&file_ms.to_le_bytes());
    d.extend_from_slice(&file_ls.to_le_bytes());
    d.extend_from_slice(&prod_ms.to_le_bytes());
    d.extend_from_slice(&prod_ls.to_le_bytes());
    d.extend_from_slice(&flags_mask.to_le_bytes());
    d.extend_from_slice(&flags.to_le_bytes());
    d.extend_from_slice(&file_os.to_le_bytes());
    d.extend_from_slice(&file_type.to_le_bytes());
    d.extend_from_slice(&file_subtype.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes()); // file_date_ms
    d.extend_from_slice(&0u32.to_le_bytes()); // file_date_ls
    assert_eq!(d.len(), 52);
    d
}

/// Build a single "String" entry (key/value WCHAR pair) of a StringTable.
///
/// Layout: wLength, wValueLength (in WORDs), wType=1, key (wide+null, aligned),
/// value (wide+null). The whole structure is then padded so its length is the
/// stored wLength and it ends on a DWORD boundary.
fn version_string_entry(key: &str, value: &str) -> Vec<u8> {
    let key_bytes = wide_z(key);
    let value_bytes = wide_z(value);
    // wValueLength is the value length in WORDs (UTF-16 code units incl. null).
    let value_words = (value_bytes.len() / 2) as u16;

    let mut body = Vec::new();
    body.extend_from_slice(&0u16.to_le_bytes()); // wLength placeholder
    body.extend_from_slice(&value_words.to_le_bytes()); // wValueLength (WORDs)
    body.extend_from_slice(&1u16.to_le_bytes()); // wType = text
    body.extend_from_slice(&key_bytes);
    align4(&mut body);
    body.extend_from_slice(&value_bytes);
    let length = body.len() as u16;
    body[0..2].copy_from_slice(&length.to_le_bytes());
    align4(&mut body);
    body
}

/// Build a StringTable: header + 8-char language-id wide string + String entries.
fn string_table(lang_id: &str, entries: &[(&str, &str)]) -> Vec<u8> {
    let lang_bytes = wide_z(lang_id);

    let mut children = Vec::new();
    for (k, v) in entries {
        children.extend_from_slice(&version_string_entry(k, v));
    }

    let mut body = Vec::new();
    body.extend_from_slice(&0u16.to_le_bytes()); // wLength placeholder
    body.extend_from_slice(&0u16.to_le_bytes()); // wValueLength = 0
    body.extend_from_slice(&1u16.to_le_bytes()); // wType = text
    body.extend_from_slice(&lang_bytes);
    align4(&mut body);
    body.extend_from_slice(&children);
    let length = body.len() as u16;
    body[0..2].copy_from_slice(&length.to_le_bytes());
    align4(&mut body);
    body
}

/// Build a StringFileInfo block containing a single StringTable.
fn string_file_info(lang_id: &str, entries: &[(&str, &str)]) -> Vec<u8> {
    let key_bytes = wide_z("StringFileInfo");
    let table = string_table(lang_id, entries);

    let mut body = Vec::new();
    body.extend_from_slice(&0u16.to_le_bytes()); // wLength placeholder
    body.extend_from_slice(&0u16.to_le_bytes()); // wValueLength = 0
    body.extend_from_slice(&1u16.to_le_bytes()); // wType = text
    body.extend_from_slice(&key_bytes);
    align4(&mut body);
    body.extend_from_slice(&table);
    let length = body.len() as u16;
    body[0..2].copy_from_slice(&length.to_le_bytes());
    align4(&mut body);
    body
}

/// Build a VarFileInfo block (the "Translation" sibling of StringFileInfo).
/// This exercises the child-skipping loop in `find_string_file_info`.
fn var_file_info() -> Vec<u8> {
    let key_bytes = wide_z("VarFileInfo");
    // A single "Translation" Var entry with a 4-byte value (lang + codepage).
    let var_key = wide_z("Translation");
    let mut var = Vec::new();
    var.extend_from_slice(&0u16.to_le_bytes()); // wLength placeholder
    var.extend_from_slice(&4u16.to_le_bytes()); // wValueLength = 4 bytes
    var.extend_from_slice(&0u16.to_le_bytes()); // wType = binary
    var.extend_from_slice(&var_key);
    align4(&mut var);
    var.extend_from_slice(&0x0409u16.to_le_bytes()); // langid (US English)
    var.extend_from_slice(&0x04B0u16.to_le_bytes()); // codepage 1200
    let var_len = var.len() as u16;
    var[0..2].copy_from_slice(&var_len.to_le_bytes());
    align4(&mut var);

    let mut body = Vec::new();
    body.extend_from_slice(&0u16.to_le_bytes()); // wLength placeholder
    body.extend_from_slice(&0u16.to_le_bytes()); // wValueLength = 0
    body.extend_from_slice(&1u16.to_le_bytes()); // wType
    body.extend_from_slice(&key_bytes);
    align4(&mut body);
    body.extend_from_slice(&var);
    let length = body.len() as u16;
    body[0..2].copy_from_slice(&length.to_le_bytes());
    align4(&mut body);
    body
}

/// Build a complete VS_VERSION_INFO blob.
///
/// When `var_first` is true, the VarFileInfo child precedes StringFileInfo,
/// forcing the parser's child-search loop to skip past a sibling before it
/// finds StringFileInfo.
fn version_info_blob(
    fixed: &[u8],
    string_info: Option<Vec<u8>>,
    include_var: bool,
    var_first: bool,
) -> Vec<u8> {
    assert_eq!(fixed.len(), 52);
    let key = wide_z("VS_VERSION_INFO"); // 16 code units * 2 = 32 bytes

    // Assemble children.
    let mut children = Vec::new();
    let sfi = string_info;
    if var_first && include_var {
        children.extend_from_slice(&var_file_info());
    }
    if let Some(ref s) = sfi {
        children.extend_from_slice(s);
    }
    if !var_first && include_var {
        children.extend_from_slice(&var_file_info());
    }

    let mut body = Vec::new();
    body.extend_from_slice(&0u16.to_le_bytes()); // wLength placeholder
    body.extend_from_slice(&52u16.to_le_bytes()); // wValueLength = sizeof(FIXEDFILEINFO)
    body.extend_from_slice(&0u16.to_le_bytes()); // wType = binary
    body.extend_from_slice(&key); // "VS_VERSION_INFO\0"
    // The parser computes the FIXEDFILEINFO offset as align4(6 + 32) = 40.
    // header(6) + key(32) = 38, so add 2 bytes of padding to reach 40.
    align4(&mut body);
    assert_eq!(body.len(), 40);
    body.extend_from_slice(fixed); // VS_FIXEDFILEINFO at offset 40
    align4(&mut body); // already aligned (40 + 52 = 92)
    body.extend_from_slice(&children);
    let length = body.len() as u16;
    body[0..2].copy_from_slice(&length.to_le_bytes());
    body
}

// =============================================================================
// version_info_parser: direct tests
// =============================================================================

#[test]
fn test_parse_vs_fixed_file_info_direct() {
    let raw = fixed_file_info_bytes(
        0x0001_0002,
        0x0003_0004,
        0x0005_0006,
        0x0007_0008,
        0x3F,
        0x01 | 0x20,
        0x00040004,
        0x1,
        0x4,
    );
    let (rest, ffi) = parse_vs_fixed_file_info(&raw).expect("fixed file info");
    assert!(rest.is_empty());
    assert_eq!(ffi.signature, 0xFEEF04BD);
    assert_eq!(ffi.file_version(), "1.2.3.4");
    assert_eq!(ffi.product_version(), "5.6.7.8");
    assert_eq!(ffi.file_os_string(), "Windows NT 32-bit");
    assert_eq!(ffi.file_type_string(), "Application");
    let flags = ffi.file_flags_string();
    assert!(flags.contains(&"Debug"));
    assert!(flags.contains(&"Special build"));
}

#[test]
fn test_parse_version_info_full() {
    let fixed = fixed_file_info_bytes(
        0x0002_0000, // 2.0
        0x0001_0003, // .1.3
        0x0002_0000,
        0x0000_0000,
        0x3F,
        0x00, // no flags
        0x00040004,
        0x1,
        0x0,
    );
    let entries = [
        ("CompanyName", "OxiDex Test Corp"),
        ("FileDescription", "A test application"),
        ("FileVersion", "2.0.1.3"),
        ("InternalName", "testapp"),
        ("LegalCopyright", "Copyright (C) 2026"),
        ("OriginalFilename", "testapp.exe"),
        ("ProductName", "OxiDex Suite"),
        ("ProductVersion", "2.0"),
    ];
    let sfi = string_file_info("040904b0", &entries);
    let blob = version_info_blob(&fixed, Some(sfi), true, false);

    let (ffi, strings) = parse_version_info(&blob).expect("version info parse");
    assert_eq!(ffi.file_version(), "2.0.1.3");
    assert_eq!(
        strings.get("CompanyName").map(String::as_str),
        Some("OxiDex Test Corp")
    );
    assert_eq!(
        strings.get("OriginalFilename").map(String::as_str),
        Some("testapp.exe")
    );
    assert_eq!(
        strings.get("ProductName").map(String::as_str),
        Some("OxiDex Suite")
    );
    assert_eq!(strings.len(), entries.len());
}

#[test]
fn test_parse_version_info_var_before_string() {
    // VarFileInfo precedes StringFileInfo: exercises the sibling-skip path of
    // find_string_file_info (the `key != "StringFileInfo"` branch + advance).
    let fixed = fixed_file_info_bytes(
        0x0001_0000,
        0x0000_0000,
        0x0001_0000,
        0x0000_0000,
        0x3F,
        0x02, // Pre-release
        0x00000004,
        0x2,
        0x0,
    );
    let entries = [("ProductName", "VarFirst"), ("FileVersion", "1.0.0.0")];
    let sfi = string_file_info("040904b0", &entries);
    let blob = version_info_blob(&fixed, Some(sfi), true, true);

    let (ffi, strings) = parse_version_info(&blob).expect("version info var-first");
    assert_eq!(ffi.file_type_string(), "DLL");
    assert_eq!(
        strings.get("ProductName").map(String::as_str),
        Some("VarFirst")
    );
}

#[test]
fn test_parse_version_info_no_string_file_info() {
    // Only a VarFileInfo child: find_string_file_info returns None, so the
    // string map falls back to default() (empty).
    let fixed = fixed_file_info_bytes(
        0x0003_0000,
        0,
        0x0003_0000,
        0,
        0x3F,
        0,
        0x00040000,
        0x3,
        0x0,
    );
    let blob = version_info_blob(&fixed, None, true, false);
    let (ffi, strings) = parse_version_info(&blob).expect("no SFI");
    assert_eq!(ffi.file_type_string(), "Driver");
    assert!(strings.is_empty());
}

#[test]
fn test_parse_version_info_empty_string_table() {
    // StringFileInfo with a StringTable that has no String entries: the
    // String-parsing while-loop body is skipped, producing an empty map.
    let fixed = fixed_file_info_bytes(0x0001_0000, 0, 0, 0, 0x3F, 0, 0x00040004, 0x1, 0);
    let sfi = string_file_info("040904b0", &[]);
    let blob = version_info_blob(&fixed, Some(sfi), false, false);
    let (_ffi, strings) = parse_version_info(&blob).expect("empty table");
    assert!(strings.is_empty());
}

#[test]
fn test_parse_version_info_error_branches() {
    // Too short (< 6 bytes) -> None.
    assert!(parse_version_info(&[0u8; 4]).is_none());

    // Long enough for the 6-byte header but shorter than 6 + 32 -> None.
    let mut short = vec![0u8; 20];
    short[0] = 20; // wLength
    short[2] = 52; // wValueLength
    assert!(parse_version_info(&short).is_none());

    // Header present, key present, but wValueLength != 52 -> None.
    let key = wide_z("VS_VERSION_INFO");
    let mut bad_vlen = Vec::new();
    bad_vlen.extend_from_slice(&200u16.to_le_bytes()); // wLength
    bad_vlen.extend_from_slice(&40u16.to_le_bytes()); // wValueLength != 52
    bad_vlen.extend_from_slice(&0u16.to_le_bytes()); // wType
    bad_vlen.extend_from_slice(&key);
    align4(&mut bad_vlen);
    bad_vlen.extend_from_slice(&[0u8; 60]); // enough room for the offset+52 check
    assert!(parse_version_info(&bad_vlen).is_none());
}

#[test]
fn test_parse_version_info_empty_value_entry() {
    // A String entry whose value length is zero exercises the
    // `else { String::new() }` branch in parse_string_entry.
    let fixed = fixed_file_info_bytes(0x0001_0000, 0, 0, 0, 0x3F, 0, 0x00040004, 0x1, 0);
    // Build a String entry with an empty value: wValueLength = 0.
    let key_bytes = wide_z("Comments");
    let mut entry = Vec::new();
    entry.extend_from_slice(&0u16.to_le_bytes()); // wLength placeholder
    entry.extend_from_slice(&0u16.to_le_bytes()); // wValueLength = 0 (empty value)
    entry.extend_from_slice(&1u16.to_le_bytes()); // wType
    entry.extend_from_slice(&key_bytes);
    align4(&mut entry);
    let elen = entry.len() as u16;
    entry[0..2].copy_from_slice(&elen.to_le_bytes());
    align4(&mut entry);

    // Wrap it in a StringTable + StringFileInfo by hand.
    let lang_bytes = wide_z("040904b0");
    let mut table = Vec::new();
    table.extend_from_slice(&0u16.to_le_bytes());
    table.extend_from_slice(&0u16.to_le_bytes());
    table.extend_from_slice(&1u16.to_le_bytes());
    table.extend_from_slice(&lang_bytes);
    align4(&mut table);
    table.extend_from_slice(&entry);
    let tlen = table.len() as u16;
    table[0..2].copy_from_slice(&tlen.to_le_bytes());
    align4(&mut table);

    let sfi_key = wide_z("StringFileInfo");
    let mut sfi = Vec::new();
    sfi.extend_from_slice(&0u16.to_le_bytes());
    sfi.extend_from_slice(&0u16.to_le_bytes());
    sfi.extend_from_slice(&1u16.to_le_bytes());
    sfi.extend_from_slice(&sfi_key);
    align4(&mut sfi);
    sfi.extend_from_slice(&table);
    let slen = sfi.len() as u16;
    sfi[0..2].copy_from_slice(&slen.to_le_bytes());
    align4(&mut sfi);

    let blob = version_info_blob(&fixed, Some(sfi), false, false);
    let (_ffi, strings) = parse_version_info(&blob).expect("empty value");
    assert_eq!(strings.get("Comments").map(String::as_str), Some(""));
}

// =============================================================================
// metadata_extractor: version info + signature extraction
// =============================================================================

#[test]
fn test_extract_version_info_metadata_with_flags() {
    let ffi = VsFixedFileInfo {
        signature: 0xFEEF04BD,
        struct_version: 0x00010000,
        file_version_ms: 0x0001_0002,
        file_version_ls: 0x0003_0004,
        product_version_ms: 0x0005_0006,
        product_version_ls: 0x0007_0008,
        file_flags_mask: 0x3F,
        file_flags: 0x01 | 0x02 | 0x20, // Debug, Pre-release, Special build
        file_os: 0x00040004,
        file_type: 0x1,
        file_subtype: 7,
        file_date_ms: 0,
        file_date_ls: 0,
    };
    let mut strings = HashMap::new();
    strings.insert("CompanyName".to_string(), "Acme".to_string());
    strings.insert("ProductName".to_string(), "Widget".to_string());

    let mut md = MetadataMap::new();
    extract_version_info_metadata(&ffi, &strings, &mut md);

    assert_eq!(md.get_string("PE:FileVersionNumber"), Some("1.2.3.4"));
    assert_eq!(md.get_string("PE:ProductVersionNumber"), Some("5.6.7.8"));
    assert_eq!(md.get_string("PE:FileOS"), Some("Windows NT 32-bit"));
    assert_eq!(md.get_string("PE:ObjectFileType"), Some("Application"));
    assert_eq!(md.get_integer("PE:FileSubtype"), Some(7));
    let file_flags = md.get_string("PE:FileFlags").expect("file flags");
    assert!(file_flags.contains("Debug"));
    assert!(file_flags.contains("Pre-release"));
    assert!(file_flags.contains("Special build"));
    assert!(md.contains_key("PE:FileFlagsMask"));
    // String table entries get a PE: prefix.
    assert_eq!(md.get_string("PE:CompanyName"), Some("Acme"));
    assert_eq!(md.get_string("PE:ProductName"), Some("Widget"));
}

#[test]
fn test_extract_version_info_metadata_no_flags() {
    // file_flags == 0 -> file_flags_string() empty -> "(none)" branch.
    let ffi = VsFixedFileInfo {
        signature: 0xFEEF04BD,
        struct_version: 0x00010000,
        file_version_ms: 0,
        file_version_ls: 0,
        product_version_ms: 0,
        product_version_ls: 0,
        file_flags_mask: 0x3F,
        file_flags: 0,
        file_os: 0x00000004,
        file_type: 0x2,
        file_subtype: 0,
        file_date_ms: 0,
        file_date_ls: 0,
    };
    let strings: HashMap<String, String> = HashMap::new();
    let mut md = MetadataMap::new();
    extract_version_info_metadata(&ffi, &strings, &mut md);
    assert_eq!(md.get_string("PE:FileFlags"), Some("(none)"));
    assert_eq!(md.get_string("PE:FileOS"), Some("Windows 32-bit"));
    assert_eq!(md.get_string("PE:ObjectFileType"), Some("DLL"));
}

#[test]
fn test_extract_signature_metadata_full() {
    let sig = SignatureInfo {
        signature_present: true,
        signature_type: "PKCS#7".to_string(),
        certificate_count: 2,
        signer_common_name: Some("Example Signer".to_string()),
        signer_organization: Some("Example Org".to_string()),
        issuer_common_name: Some("Example CA".to_string()),
        issuer_organization: Some("Example Authority".to_string()),
        certificate_serial_number: Some("0A1B2C".to_string()),
        certificate_not_before: Some("2020-01-01T00:00:00Z".to_string()),
        certificate_not_after: Some("2030-01-01T00:00:00Z".to_string()),
        certificate_thumbprint: Some("DEADBEEF".to_string()),
        has_counter_signature: true,
        counter_signature_time: Some("2021-06-01T12:00:00Z".to_string()),
        signature_valid: true,
        certificate_expired: false,
    };
    let mut md = MetadataMap::new();
    extract_signature_metadata(&sig, &mut md);

    assert_eq!(md.get_integer("PE:SignaturePresent"), Some(1));
    assert_eq!(md.get_string("PE:SignatureType"), Some("PKCS#7"));
    assert_eq!(md.get_integer("PE:CertificateCount"), Some(2));
    assert_eq!(md.get_string("PE:SignerCommonName"), Some("Example Signer"));
    assert_eq!(md.get_string("PE:SignerOrganization"), Some("Example Org"));
    assert_eq!(md.get_string("PE:IssuerCommonName"), Some("Example CA"));
    assert_eq!(
        md.get_string("PE:IssuerOrganization"),
        Some("Example Authority")
    );
    assert_eq!(md.get_string("PE:CertificateSerialNumber"), Some("0A1B2C"));
    assert_eq!(
        md.get_string("PE:CertificateNotBefore"),
        Some("2020-01-01T00:00:00Z")
    );
    assert_eq!(
        md.get_string("PE:CertificateNotAfter"),
        Some("2030-01-01T00:00:00Z")
    );
    assert_eq!(md.get_string("PE:CertificateThumbprint"), Some("DEADBEEF"));
    assert_eq!(md.get_integer("PE:HasCounterSignature"), Some(1));
    assert_eq!(
        md.get_string("PE:CounterSignatureTime"),
        Some("2021-06-01T12:00:00Z")
    );
    assert_eq!(md.get_integer("PE:SignatureValid"), Some(1));
    assert_eq!(md.get_integer("PE:CertificateExpired"), Some(0));
}

#[test]
fn test_extract_signature_metadata_not_present() {
    // signature_present == false -> early return after the SignaturePresent tag.
    let sig = SignatureInfo {
        signature_present: false,
        ..Default::default()
    };
    let mut md = MetadataMap::new();
    extract_signature_metadata(&sig, &mut md);
    assert_eq!(md.get_integer("PE:SignaturePresent"), Some(0));
    assert!(!md.contains_key("PE:SignatureType"));
    assert!(!md.contains_key("PE:CertificateCount"));
}

// =============================================================================
// signature_parser: Authenticode certificate fixtures
// =============================================================================

/// Encode an ASN.1 TLV with a short-form length (content < 128 bytes).
fn der_tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    assert!(content.len() < 128, "use der_tlv_long for >=128 bytes");
    let mut v = Vec::with_capacity(2 + content.len());
    v.push(tag);
    v.push(content.len() as u8);
    v.extend_from_slice(content);
    v
}

/// Encode an ASN.1 TLV with a long-form length (2-byte length).
fn der_tlv_long(tag: u8, content: &[u8]) -> Vec<u8> {
    let len = content.len();
    let mut v = Vec::with_capacity(4 + len);
    v.push(tag);
    v.push(0x82); // long form, 2 length bytes
    v.push((len >> 8) as u8);
    v.push((len & 0xFF) as u8);
    v.extend_from_slice(content);
    v
}

/// Build an AttributeTypeAndValue SET -> SEQUENCE { OID, PrintableString }.
fn dn_attribute(oid: &[u8], string_tag: u8, value: &str) -> Vec<u8> {
    let oid_tlv = der_tlv(0x06, oid);
    let str_tlv = der_tlv(string_tag, value.as_bytes());
    let mut seq_content = Vec::new();
    seq_content.extend_from_slice(&oid_tlv);
    seq_content.extend_from_slice(&str_tlv);
    let seq = der_tlv(0x30, &seq_content);
    der_tlv(0x31, &seq) // SET
}

/// Build a Distinguished Name (RDNSequence) containing CN and O attributes.
fn distinguished_name(cn: &str, o: &str) -> Vec<u8> {
    const CN_OID: &[u8] = &[0x55, 0x04, 0x03];
    const O_OID: &[u8] = &[0x55, 0x04, 0x0A];
    let mut content = Vec::new();
    content.extend_from_slice(&dn_attribute(CN_OID, 0x13, cn)); // PrintableString
    content.extend_from_slice(&dn_attribute(O_OID, 0x0C, o)); // UTF8String
    der_tlv(0x30, &content)
}

/// Build a Validity SEQUENCE with notBefore (UTCTime) and notAfter
/// (GeneralizedTime), exercising both branches of parse_asn1_time.
fn validity(not_before_utc: &str, not_after_gen: &str) -> Vec<u8> {
    let nb = der_tlv(0x17, not_before_utc.as_bytes()); // UTCTime
    let na = der_tlv(0x18, not_after_gen.as_bytes()); // GeneralizedTime
    let mut content = Vec::new();
    content.extend_from_slice(&nb);
    content.extend_from_slice(&na);
    der_tlv(0x30, &content)
}

/// Build a TBSCertificate SEQUENCE:
///   [0] version, INTEGER serial, SEQUENCE sigAlg, SEQUENCE issuer,
///   SEQUENCE validity, SEQUENCE subject.
fn tbs_certificate(serial: &[u8], issuer: &[u8], val: &[u8], subject: &[u8]) -> Vec<u8> {
    // version [0] EXPLICIT { INTEGER 2 }
    let version_inner = der_tlv(0x02, &[0x02]);
    let version = der_tlv(0xA0, &version_inner);
    let serial_int = der_tlv(0x02, serial);
    // AlgorithmIdentifier SEQUENCE { OID sha256WithRSA }
    let alg_oid = der_tlv(
        0x06,
        &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B],
    );
    let sig_alg = der_tlv(0x30, &alg_oid);

    let mut content = Vec::new();
    content.extend_from_slice(&version);
    content.extend_from_slice(&serial_int);
    content.extend_from_slice(&sig_alg);
    content.extend_from_slice(issuer);
    content.extend_from_slice(val);
    content.extend_from_slice(subject);
    der_tlv_long(0x30, &content)
}

/// Build a complete X.509 certificate SEQUENCE wrapping a TBSCertificate plus
/// a (dummy) signatureAlgorithm and signatureValue.
fn x509_certificate(tbs: &[u8]) -> Vec<u8> {
    let alg_oid = der_tlv(
        0x06,
        &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B],
    );
    let sig_alg = der_tlv(0x30, &alg_oid);
    let sig_value = der_tlv(0x03, &[0x00, 0xAB, 0xCD]); // BIT STRING

    let mut content = Vec::new();
    content.extend_from_slice(tbs);
    content.extend_from_slice(&sig_alg);
    content.extend_from_slice(&sig_value);
    der_tlv_long(0x30, &content)
}

/// Wrap one or more certificates in a PKCS#7-ish outer SEQUENCE with a
/// context-specific [0] certificates section, matching what
/// parse_pkcs7_signed_data scans for.
fn pkcs7_signed_data(certs: &[Vec<u8>]) -> Vec<u8> {
    let mut certs_blob = Vec::new();
    for c in certs {
        certs_blob.extend_from_slice(c);
    }
    let certs_section = der_tlv_long(0xA0, &certs_blob); // [0] certificates

    // The outer SEQUENCE content begins with a content-type OID then the
    // certificates section. parse_pkcs7_signed_data scans bytewise for 0xA0.
    let content_type_oid = der_tlv(
        0x06,
        &[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x07, 0x02],
    ); // signedData
    let mut outer_content = Vec::new();
    outer_content.extend_from_slice(&content_type_oid);
    outer_content.extend_from_slice(&certs_section);
    der_tlv_long(0x30, &outer_content)
}

#[test]
fn test_parse_signature_info_full_certificate() {
    let serial = &[0x0A, 0x1B, 0x2C, 0x3D];
    let issuer = distinguished_name("Root CA", "Cert Authority Inc");
    let subject = distinguished_name("Example Signer", "Example Org");
    let val = validity("250101000000Z", "20350101000000Z");
    let tbs = tbs_certificate(serial, &issuer, &val, &subject);
    let cert = x509_certificate(&tbs);
    let pkcs7 = pkcs7_signed_data(&[cert]);

    let info = parse_signature_info(&pkcs7).expect("signature info");
    assert!(info.signature_present);
    assert!(info.signature_valid);
    assert_eq!(info.signature_type, "PKCS#7");
    assert_eq!(info.certificate_count, 1);
    assert_eq!(info.signer_common_name.as_deref(), Some("Example Signer"));
    assert_eq!(info.signer_organization.as_deref(), Some("Example Org"));
    assert_eq!(info.issuer_common_name.as_deref(), Some("Root CA"));
    assert_eq!(
        info.issuer_organization.as_deref(),
        Some("Cert Authority Inc")
    );
    assert_eq!(info.certificate_serial_number.as_deref(), Some("0A1B2C3D"));
    assert_eq!(
        info.certificate_not_before.as_deref(),
        Some("2025-01-01T00:00:00Z")
    );
    assert_eq!(
        info.certificate_not_after.as_deref(),
        Some("2035-01-01T00:00:00Z")
    );
    // thumbprint is a SHA-1 hex over the cert DER (40 hex chars).
    assert_eq!(
        info.certificate_thumbprint.as_deref().map(str::len),
        Some(40)
    );
    // notAfter is in the future -> not expired.
    assert!(!info.certificate_expired);
}

#[test]
fn test_parse_signature_info_expired_certificate() {
    let serial = &[0x01];
    let issuer = distinguished_name("Old CA", "Legacy Org");
    let subject = distinguished_name("Old Signer", "Legacy Org");
    // notAfter in the past -> certificate_expired should be true.
    let val = validity("000101000000Z", "20100101000000Z");
    let tbs = tbs_certificate(serial, &issuer, &val, &subject);
    let cert = x509_certificate(&tbs);
    let pkcs7 = pkcs7_signed_data(&[cert]);

    let info = parse_signature_info(&pkcs7).expect("expired info");
    // UTCTime 00 -> 2000 per the Y2K pivot.
    assert_eq!(
        info.certificate_not_before.as_deref(),
        Some("2000-01-01T00:00:00Z")
    );
    assert!(info.certificate_expired);
}

#[test]
fn test_parse_signature_info_multiple_certificates() {
    // Two certificates back-to-back exercise the parse loop's "remaining"
    // advance and the `certificate_count` accumulation.
    let make = |cn: &str, o: &str, serial: &[u8]| {
        let issuer = distinguished_name("Chain CA", "Chain Org");
        let subject = distinguished_name(cn, o);
        let val = validity("240101000000Z", "20340101000000Z");
        let tbs = tbs_certificate(serial, &issuer, &val, &subject);
        x509_certificate(&tbs)
    };
    let c1 = make("Leaf", "Leaf Org", &[0x11, 0x22]);
    let c2 = make("Intermediate", "Mid Org", &[0x33, 0x44]);
    let pkcs7 = pkcs7_signed_data(&[c1, c2]);

    let info = parse_signature_info(&pkcs7).expect("multi cert");
    assert_eq!(info.certificate_count, 2);
    // First cert is the signer.
    assert_eq!(info.signer_common_name.as_deref(), Some("Leaf"));
}

#[test]
fn test_parse_signature_info_invalid_structure() {
    // Not a valid SEQUENCE -> parse_pkcs7_signed_data errors -> signature_valid=false.
    let info = parse_signature_info(&[0xFF, 0x00, 0x01, 0x02]).expect("junk");
    assert!(info.signature_present);
    assert!(!info.signature_valid);
    assert_eq!(info.certificate_count, 0);
}

#[test]
fn test_parse_win_certificate_revisions() {
    // Revision 1.0 + X.509 type, with long-ish embedded data.
    let mut cert = Vec::new();
    let payload = vec![0x30u8, 0x03, 0x02, 0x01, 0x05];
    let total_len = (8 + payload.len()) as u32;
    cert.extend_from_slice(&total_len.to_le_bytes());
    cert.extend_from_slice(&cert_revision::WIN_CERT_REVISION_1_0.to_le_bytes());
    cert.extend_from_slice(&cert_type::WIN_CERT_TYPE_X509.to_le_bytes());
    cert.extend_from_slice(&payload);

    let (_, wc) = parse_win_certificate(&cert).expect("win cert rev1");
    assert_eq!(wc.dw_length, total_len);
    assert_eq!(wc.w_revision, cert_revision::WIN_CERT_REVISION_1_0);
    assert_eq!(wc.w_certificate_type, cert_type::WIN_CERT_TYPE_X509);
    assert_eq!(wc.certificate_data, payload);

    // dwLength smaller than 8 -> saturating_sub keeps cert_data length at 0.
    let mut tiny = Vec::new();
    tiny.extend_from_slice(&4u32.to_le_bytes());
    tiny.extend_from_slice(&cert_revision::WIN_CERT_REVISION_2_0.to_le_bytes());
    tiny.extend_from_slice(&cert_type::WIN_CERT_TYPE_PKCS_SIGNED_DATA.to_le_bytes());
    let (_, wc2) = parse_win_certificate(&tiny).expect("tiny win cert");
    assert_eq!(wc2.certificate_data.len(), 0);
}

// =============================================================================
// Full PE assembly: .rsrc RT_VERSION + Authenticode Security directory
// =============================================================================

/// Build a 64-byte DOS header with the chosen e_lfanew.
fn dos_header(e_lfanew: u32) -> Vec<u8> {
    let mut d = vec![0u8; 64];
    d[0] = b'M';
    d[1] = b'Z';
    d[0x3C..0x40].copy_from_slice(&e_lfanew.to_le_bytes());
    d
}

/// Build a 20-byte COFF header.
fn coff_header(machine: u16, n_sections: u16, opt_size: u16, characteristics: u16) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&machine.to_le_bytes());
    d.extend_from_slice(&n_sections.to_le_bytes());
    d.extend_from_slice(&0x5F00_0000u32.to_le_bytes()); // time_date_stamp
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&opt_size.to_le_bytes());
    d.extend_from_slice(&characteristics.to_le_bytes());
    d
}

/// Build a PE32+ optional header with 16 data directories.
fn optional_header_pe32plus(
    subsystem: u16,
    dll_characteristics: u16,
    data_dirs: &[(usize, u32, u32)],
) -> Vec<u8> {
    let mut d = Vec::new();
    d.extend_from_slice(&0x020Bu16.to_le_bytes()); // PE32+
    d.push(14);
    d.push(0);
    d.extend_from_slice(&0x1000u32.to_le_bytes());
    d.extend_from_slice(&0x2000u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0x1000u32.to_le_bytes());
    d.extend_from_slice(&0x1000u32.to_le_bytes());
    d.extend_from_slice(&0x140000000u64.to_le_bytes()); // image_base
    d.extend_from_slice(&0x1000u32.to_le_bytes());
    d.extend_from_slice(&0x200u32.to_le_bytes());
    d.extend_from_slice(&10u16.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d.extend_from_slice(&1u16.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d.extend_from_slice(&10u16.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0x10000u32.to_le_bytes());
    d.extend_from_slice(&0x400u32.to_le_bytes());
    d.extend_from_slice(&0x12345u32.to_le_bytes());
    d.extend_from_slice(&subsystem.to_le_bytes());
    d.extend_from_slice(&dll_characteristics.to_le_bytes());
    d.extend_from_slice(&0x100000u64.to_le_bytes());
    d.extend_from_slice(&0x1000u64.to_le_bytes());
    d.extend_from_slice(&0x100000u64.to_le_bytes());
    d.extend_from_slice(&0x1000u64.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&16u32.to_le_bytes());
    let mut dirs = [(0u32, 0u32); 16];
    for &(idx, rva, size) in data_dirs {
        dirs[idx] = (rva, size);
    }
    for (rva, size) in dirs {
        d.extend_from_slice(&rva.to_le_bytes());
        d.extend_from_slice(&size.to_le_bytes());
    }
    d
}

/// Build a 40-byte section header.
fn section_header(name: &str, vsize: u32, vaddr: u32, raw_size: u32, raw_ptr: u32) -> Vec<u8> {
    let mut d = Vec::new();
    let mut name_arr = [0u8; 8];
    let b = name.as_bytes();
    name_arr[..b.len().min(8)].copy_from_slice(&b[..b.len().min(8)]);
    d.extend_from_slice(&name_arr);
    d.extend_from_slice(&vsize.to_le_bytes());
    d.extend_from_slice(&vaddr.to_le_bytes());
    d.extend_from_slice(&raw_size.to_le_bytes());
    d.extend_from_slice(&raw_ptr.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d.extend_from_slice(&0x40000040u32.to_le_bytes());
    d
}

/// Build a 3-level .rsrc resource tree (Type -> ID -> Lang -> Data) pointing at
/// a VERSION_INFO blob, all packed into a single contiguous .rsrc buffer.
///
/// `rsrc_vaddr` is the section's RVA; `version_rva` is the RVA assigned to the
/// version data payload (it must lie inside the section's virtual range so the
/// mod.rs RVA-to-file-offset conversion succeeds). The version blob is appended
/// to the section buffer at the offset corresponding to `version_rva`.
fn build_rsrc_with_version(rsrc_vaddr: u32, version_rva: u32, version_blob: &[u8]) -> Vec<u8> {
    // Offsets WITHIN the .rsrc section (relative to its start).
    // The root (Type) directory is at offset 0.
    let id_dir_off = 0x30usize; // ID directory
    let lang_dir_off = 0x60usize; // Language directory
    let data_entry_off = 0x90usize; // ResourceDataEntry
    let version_blob_off = (version_rva - rsrc_vaddr) as usize; // payload offset

    let mut d = Vec::new();

    // --- Root (Type) directory at 0x00 ---
    d.extend_from_slice(&0u32.to_le_bytes()); // characteristics
    d.extend_from_slice(&0u32.to_le_bytes()); // time_date_stamp
    d.extend_from_slice(&0u16.to_le_bytes()); // major
    d.extend_from_slice(&0u16.to_le_bytes()); // minor
    d.extend_from_slice(&0u16.to_le_bytes()); // name entries
    d.extend_from_slice(&1u16.to_le_bytes()); // id entries
    d.extend_from_slice(&resource_types::RT_VERSION.to_le_bytes()); // name_id = 16
    d.extend_from_slice(&(0x80000000u32 | id_dir_off as u32).to_le_bytes()); // subdir

    while d.len() < id_dir_off {
        d.push(0);
    }
    // --- ID directory at 0x30 ---
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes()); // name entries
    d.extend_from_slice(&1u16.to_le_bytes()); // id entries
    d.extend_from_slice(&1u32.to_le_bytes()); // resource id = 1
    d.extend_from_slice(&(0x80000000u32 | lang_dir_off as u32).to_le_bytes());

    while d.len() < lang_dir_off {
        d.push(0);
    }
    // --- Language directory at 0x60 ---
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u32.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes());
    d.extend_from_slice(&0u16.to_le_bytes()); // name entries
    d.extend_from_slice(&1u16.to_le_bytes()); // id entries
    d.extend_from_slice(&1033u32.to_le_bytes()); // lang id (English US)
    d.extend_from_slice(&(data_entry_off as u32).to_le_bytes()); // data (high bit clear)

    while d.len() < data_entry_off {
        d.push(0);
    }
    // --- ResourceDataEntry at 0x90 ---
    d.extend_from_slice(&version_rva.to_le_bytes()); // data_rva (an RVA in-section)
    d.extend_from_slice(&(version_blob.len() as u32).to_le_bytes()); // size
    d.extend_from_slice(&0u32.to_le_bytes()); // codepage
    d.extend_from_slice(&0u32.to_le_bytes()); // reserved

    // --- VERSION_INFO payload at version_blob_off ---
    while d.len() < version_blob_off {
        d.push(0);
    }
    d.extend_from_slice(version_blob);
    d
}

#[test]
fn test_pe_full_with_rsrc_version_and_signature() {
    // Layout:
    //   DOS(64) | pad to e_lfanew | PE\0\0 | COFF(20) | Optional | section table
    //   | .text raw | .rsrc raw | <appended cert data at file offset>
    let e_lfanew = 0x80u32;
    let mut data = dos_header(e_lfanew);
    while data.len() < e_lfanew as usize {
        data.push(0);
    }

    // Section virtual + raw layout.
    let text_vaddr = 0x1000u32;
    let text_raw = 0x400u32;
    let text_raw_size = 0x200u32;

    let rsrc_vaddr = 0x2000u32;
    let rsrc_raw = 0x600u32;

    // Resource directory RVA (data dir index 2) — points into the .rsrc section.
    let resource_dir_rva = rsrc_vaddr;

    // Build the VERSION_INFO blob first to know its size.
    let fixed = fixed_file_info_bytes(
        0x0004_0001, // 4.1
        0x0002_0003, // .2.3
        0x0004_0000, // 4.0
        0x0000_0000,
        0x3F,
        0x01, // Debug flag
        0x00040004,
        0x1,
        0x0,
    );
    let entries = [
        ("CompanyName", "OxiDex"),
        ("FileDescription", "Coverage Fixture"),
        ("FileVersion", "4.1.2.3"),
        ("ProductName", "OxiDex PE"),
        ("ProductVersion", "4.1"),
    ];
    let sfi = string_file_info("040904b0", &entries);
    let version_blob = version_info_blob(&fixed, Some(sfi), true, false);

    // The version payload lives inside .rsrc at RVA rsrc_vaddr + 0x200.
    let version_rva = rsrc_vaddr + 0x200;
    let rsrc_buf = build_rsrc_with_version(rsrc_vaddr, version_rva, &version_blob);
    let rsrc_raw_size = ((rsrc_buf.len() as u32 + 0x1FF) / 0x200) * 0x200; // round to file align

    // Compute the Security directory (cert) — a FILE offset, not an RVA.
    // We'll append the WIN_CERTIFICATE after the .rsrc raw data.
    let cert_file_offset = rsrc_raw + rsrc_raw_size;

    // Build the Authenticode certificate (PKCS#7 with one X.509 cert).
    let serial = &[0x12, 0x34, 0x56];
    let issuer = distinguished_name("OxiDex Root CA", "OxiDex Authority");
    let subject = distinguished_name("OxiDex Code Signer", "OxiDex Inc");
    let val = validity("230101000000Z", "20330101000000Z");
    let tbs = tbs_certificate(serial, &issuer, &val, &subject);
    let cert = x509_certificate(&tbs);
    let pkcs7 = pkcs7_signed_data(&[cert]);
    // WIN_CERTIFICATE header (8 bytes) + pkcs7 payload.
    let cert_total_len = (8 + pkcs7.len()) as u32;
    let mut win_cert = Vec::new();
    win_cert.extend_from_slice(&cert_total_len.to_le_bytes());
    win_cert.extend_from_slice(&cert_revision::WIN_CERT_REVISION_2_0.to_le_bytes());
    win_cert.extend_from_slice(&cert_type::WIN_CERT_TYPE_PKCS_SIGNED_DATA.to_le_bytes());
    win_cert.extend_from_slice(&pkcs7);

    // Assemble the optional header data directories:
    //   index 2 = Resource (RVA, size), index 4 = Security (file offset, size).
    let opt = optional_header_pe32plus(
        subsystem_types::IMAGE_SUBSYSTEM_WINDOWS_GUI,
        0x4160,
        &[
            (2, resource_dir_rva, rsrc_buf.len() as u32),
            (4, cert_file_offset, win_cert.len() as u32),
        ],
    );
    let coff = coff_header(
        oxidex::parsers::pe::structures::machine_types::IMAGE_FILE_MACHINE_AMD64,
        2,
        opt.len() as u16,
        0x0022,
    );
    data.extend_from_slice(b"PE\0\0");
    data.extend_from_slice(&coff);
    data.extend_from_slice(&opt);
    // Section table: .text then .rsrc.
    data.extend_from_slice(&section_header(
        ".text",
        text_raw_size,
        text_vaddr,
        text_raw_size,
        text_raw,
    ));
    data.extend_from_slice(&section_header(
        ".rsrc",
        rsrc_buf.len() as u32,
        rsrc_vaddr,
        rsrc_raw_size,
        rsrc_raw,
    ));

    // Pad to .text raw, then .rsrc raw.
    while data.len() < text_raw as usize {
        data.push(0);
    }
    while data.len() < rsrc_raw as usize {
        data.push(0);
    }
    data.extend_from_slice(&rsrc_buf);
    // Pad to the cert file offset.
    while data.len() < cert_file_offset as usize {
        data.push(0);
    }
    data.extend_from_slice(&win_cert);

    let reader = TestReader::new(data);
    let md = parse_pe_metadata(&reader).expect("full pe parse");

    // PE header basics still work.
    assert_eq!(md.get_string("PE:ImageFormat"), Some("PE32+"));
    assert_eq!(md.get_string("PE:Subsystem"), Some("Windows GUI"));

    // VERSION_INFO from .rsrc -> metadata_extractor wired through.
    assert_eq!(md.get_string("PE:FileVersionNumber"), Some("4.1.2.3"));
    assert_eq!(md.get_string("PE:ProductVersionNumber"), Some("4.0.0.0"));
    assert_eq!(md.get_string("PE:CompanyName"), Some("OxiDex"));
    assert_eq!(md.get_string("PE:ProductName"), Some("OxiDex PE"));
    assert_eq!(md.get_string("PE:FileVersion"), Some("4.1.2.3"));
    assert!(md.contains_key("PE:FileFlags"));
    assert_eq!(md.get_string("PE:ObjectFileType"), Some("Application"));

    // Authenticode signature from the Security directory.
    assert_eq!(md.get_integer("PE:SignaturePresent"), Some(1));
    assert_eq!(md.get_string("PE:SignatureType"), Some("PKCS#7"));
    assert_eq!(md.get_integer("PE:CertificateCount"), Some(1));
    assert_eq!(
        md.get_string("PE:SignerCommonName"),
        Some("OxiDex Code Signer")
    );
    assert_eq!(md.get_string("PE:IssuerCommonName"), Some("OxiDex Root CA"));
    assert_eq!(md.get_integer("PE:SignatureValid"), Some(1));
    assert!(md.contains_key("PE:CertificateSerialNumber"));
    assert!(md.contains_key("PE:CertificateNotBefore"));
}

#[test]
fn test_pe_with_rsrc_no_version_resource() {
    // A .rsrc section whose Type directory has no RT_VERSION entry: the
    // find_resource_data path returns None and version metadata is absent,
    // while the rest of the PE still parses.
    let e_lfanew = 0x80u32;
    let mut data = dos_header(e_lfanew);
    while data.len() < e_lfanew as usize {
        data.push(0);
    }
    let rsrc_vaddr = 0x2000u32;
    let rsrc_raw = 0x600u32;

    // Resource buffer with a single Type entry that is NOT RT_VERSION (e.g. icon).
    let mut rsrc_buf = Vec::new();
    rsrc_buf.extend_from_slice(&0u32.to_le_bytes());
    rsrc_buf.extend_from_slice(&0u32.to_le_bytes());
    rsrc_buf.extend_from_slice(&0u16.to_le_bytes());
    rsrc_buf.extend_from_slice(&0u16.to_le_bytes());
    rsrc_buf.extend_from_slice(&0u16.to_le_bytes()); // name entries
    rsrc_buf.extend_from_slice(&1u16.to_le_bytes()); // id entries
    rsrc_buf.extend_from_slice(&resource_types::RT_ICON.to_le_bytes()); // type 3
    rsrc_buf.extend_from_slice(&0x80000030u32.to_le_bytes());
    while rsrc_buf.len() < 0x40 {
        rsrc_buf.push(0);
    }

    let rsrc_raw_size = 0x200u32;
    let opt = optional_header_pe32plus(
        subsystem_types::IMAGE_SUBSYSTEM_WINDOWS_CUI,
        0x0100,
        &[(2, rsrc_vaddr, rsrc_buf.len() as u32)],
    );
    let coff = coff_header(
        oxidex::parsers::pe::structures::machine_types::IMAGE_FILE_MACHINE_I386,
        2,
        opt.len() as u16,
        0x0102,
    );
    data.extend_from_slice(b"PE\0\0");
    data.extend_from_slice(&coff);
    data.extend_from_slice(&opt);
    data.extend_from_slice(&section_header(".text", 0x200, 0x1000, 0x200, 0x400));
    data.extend_from_slice(&section_header(
        ".rsrc",
        rsrc_buf.len() as u32,
        rsrc_vaddr,
        rsrc_raw_size,
        rsrc_raw,
    ));
    while data.len() < rsrc_raw as usize {
        data.push(0);
    }
    data.extend_from_slice(&rsrc_buf);
    while data.len() < (rsrc_raw + rsrc_raw_size) as usize {
        data.push(0);
    }

    let reader = TestReader::new(data);
    let md = parse_pe_metadata(&reader).expect("pe no version resource");
    assert_eq!(md.get_string("PE:Subsystem"), Some("Windows Console"));
    assert!(!md.contains_key("PE:FileVersionNumber"));
}
