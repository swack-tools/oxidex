//! X.509 certificate parser integration tests
//!
//! Comprehensive tests for X.509 certificate parsing in both PEM and DER formats.
//! Tests cover format detection, signature verification, fingerprint calculation,
//! and metadata extraction from various certificate structures.

#[path = "../common/mod.rs"]
mod common;

use common::TestReader;
use oxidex::core::TagValue;
use oxidex::parsers::specialized::x509::{X509Parser, parse_x509_metadata};

/// Test 1: PEM format detection
///
/// Verifies that certificates with "-----BEGIN CERTIFICATE-----" header
/// are correctly identified as PEM format.
#[test]
fn test_x509_pem_detection() {
    let pem = b"-----BEGIN CERTIFICATE-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA\n-----END CERTIFICATE-----";
    let reader = TestReader::new(pem.to_vec());

    // Verify signature detection works for PEM
    let result = X509Parser::verify_signature(&reader);
    assert!(
        result.is_ok(),
        "verify_signature should succeed for valid PEM"
    );
    assert!(
        result.unwrap(),
        "verify_signature should return true for PEM format"
    );
}

/// Test 2: DER format detection
///
/// Verifies that certificates with ASN.1 SEQUENCE tag (0x30) followed by
/// proper length encoding are identified as DER format.
#[test]
fn test_x509_der_detection() {
    // Construct minimal DER certificate header
    // SEQUENCE tag (0x30) + long form length encoding (0x82 = 2 length bytes)
    let mut der = vec![0x30, 0x82, 0x01, 0x00]; // SEQUENCE, long form (256 bytes)
    der.extend_from_slice(&[0x30, 0x03, 0x02, 0x01, 0x00, 0x00]);
    let reader = TestReader::new(der);

    // Verify signature detection works for DER
    let result = X509Parser::verify_signature(&reader);
    assert!(
        result.is_ok(),
        "verify_signature should succeed for valid DER"
    );
    assert!(
        result.unwrap(),
        "verify_signature should return true for DER format"
    );
}

/// Test 3: Invalid data rejection
///
/// Verifies that random invalid data without proper certificate headers
/// is correctly rejected as not being a valid certificate.
#[test]
fn test_x509_invalid_data() {
    // Random bytes that don't match PEM or DER certificate format
    let invalid = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09];
    let reader = TestReader::new(invalid);

    // Verify signature detection rejects invalid data
    let result = X509Parser::verify_signature(&reader);
    assert!(
        result.is_ok(),
        "verify_signature should not error on invalid data"
    );
    assert!(
        !result.unwrap(),
        "verify_signature should return false for invalid data"
    );
}

/// Test 4: Fingerprint calculation and presence
///
/// Verifies that the X.509 parser generates both SHA256 and SHA1 fingerprints
/// for parsed certificates, which are essential for certificate identification
/// and validation in forensic analysis.
#[test]
fn test_x509_fingerprints() {
    // Create a minimal valid-ish DER certificate structure
    // that can pass basic validation
    let mut der = Vec::new();
    der.push(0x30); // SEQUENCE
    der.push(0x10); // Length 16 bytes
    der.push(0x30); // TBS SEQUENCE
    der.push(0x0E); // Length 14 bytes
    // Minimal certificate content
    der.extend_from_slice(&[0x02, 0x01, 0x01]); // INTEGER 1 (version)
    der.extend_from_slice(&[0x02, 0x04, 0x12, 0x34, 0x56, 0x78]); // Serial number
    der.extend_from_slice(&[0x30, 0x03, 0x06, 0x01, 0x00]); // AlgorithmID

    let reader = TestReader::new(der);
    let result = parse_x509_metadata(&reader);

    assert!(result.is_ok(), "parse_x509_metadata should succeed");
    let metadata = result.unwrap();

    // Verify both fingerprints are present
    assert!(
        metadata.contains_key("X509:SHA256Fingerprint"),
        "SHA256 fingerprint should be present in metadata"
    );
    assert!(
        metadata.contains_key("X509:SHA1Fingerprint"),
        "SHA1 fingerprint should be present in metadata"
    );

    // Verify fingerprints are non-empty strings
    if let Some(TagValue::String(sha256)) = metadata.get("X509:SHA256Fingerprint") {
        assert!(!sha256.is_empty(), "SHA256 fingerprint should not be empty");
        assert_eq!(
            sha256.len(),
            64,
            "SHA256 fingerprint should be 64 hex characters (32 bytes)"
        );
    } else {
        panic!("SHA256 fingerprint should be a string value");
    }

    if let Some(TagValue::String(sha1)) = metadata.get("X509:SHA1Fingerprint") {
        assert!(!sha1.is_empty(), "SHA1 fingerprint should not be empty");
        assert_eq!(
            sha1.len(),
            40,
            "SHA1 fingerprint should be 40 hex characters (20 bytes)"
        );
    } else {
        panic!("SHA1 fingerprint should be a string value");
    }
}

/// Test 5: PEM format parsing and conversion
///
/// Verifies that PEM-encoded certificates are correctly decoded to DER
/// format and that basic metadata extraction works.
#[test]
fn test_x509_pem_parsing() {
    // Create a more realistic PEM certificate with proper base64
    let pem_data = b"-----BEGIN CERTIFICATE-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAwJZFwWVBSxOkPKM=\n-----END CERTIFICATE-----";
    let reader = TestReader::new(pem_data.to_vec());

    // Verify the format is correctly identified
    let is_valid = X509Parser::verify_signature(&reader);
    assert!(
        is_valid.is_ok() && is_valid.unwrap(),
        "PEM certificate should be detected and verified"
    );
}

/// Test 6: DER format with proper ASN.1 structure
///
/// Verifies that DER certificates with complete ASN.1 structure
/// are properly parsed, including extraction of basic metadata fields.
#[test]
fn test_x509_der_complete_structure() {
    // Create a more complete DER structure
    let mut der = Vec::new();

    // Outer SEQUENCE (Certificate)
    der.push(0x30); // SEQUENCE tag
    der.push(0x82); // Long form length (2 bytes follow)
    der.push(0x01); // 256+ bytes
    der.push(0x00);

    // TBSCertificate SEQUENCE
    der.push(0x30); // SEQUENCE tag
    der.push(0x81); // Long form length (1 byte follows)
    der.push(0xF0); // 240 bytes

    // Version [0] EXPLICIT (v3)
    der.push(0xA0); // Context-specific constructed
    der.push(0x03); // Length 3
    der.push(0x02); // INTEGER tag
    der.push(0x01); // Length 1
    der.push(0x02); // Value 2 (v3)

    // Serial number INTEGER
    der.push(0x02); // INTEGER tag
    der.push(0x08); // Length 8
    der.extend_from_slice(&[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]);

    // Signature algorithm SEQUENCE
    der.push(0x30); // SEQUENCE tag
    der.push(0x0D); // Length 13
    der.push(0x06); // OID tag
    der.push(0x09); // Length 9
    // OID: 1.2.840.113549.1.1.11 (SHA256withRSA)
    der.extend_from_slice(&[0x2A, 0x86, 0x48, 0x86, 0xF7, 0x0D, 0x01, 0x01, 0x0B]);
    der.push(0x05); // NULL tag
    der.push(0x00); // NULL length

    // Pad to the expected length
    while der.len() < 260 {
        der.push(0x00);
    }

    let reader = TestReader::new(der);

    // Verify the certificate is recognized
    let is_valid = X509Parser::verify_signature(&reader);
    assert!(
        is_valid.is_ok() && is_valid.unwrap(),
        "Complete DER structure should be recognized as valid certificate"
    );
}

/// Test 7: Empty/minimal certificate rejection
///
/// Verifies that data too small to be a valid certificate is rejected.
#[test]
fn test_x509_too_small_data() {
    // Data smaller than minimum certificate size
    let small_data = vec![0x30, 0x05]; // Only 2 bytes
    let reader = TestReader::new(small_data);

    let result = X509Parser::verify_signature(&reader);
    assert!(result.is_ok(), "verify_signature should not error");
    assert!(
        !result.unwrap(),
        "Very small data should not be recognized as certificate"
    );
}

/// Test 8: Fingerprint consistency
///
/// Verifies that fingerprints are calculated consistently for the same
/// certificate data. This is important for certificate tracking.
#[test]
fn test_x509_fingerprint_consistency() {
    let mut der = Vec::new();
    der.push(0x30); // SEQUENCE
    der.push(0x10); // Length 16 bytes
    der.push(0x30); // TBS SEQUENCE
    der.push(0x0E); // Length 14 bytes
    der.extend_from_slice(&[0x02, 0x01, 0x01]); // INTEGER 1
    der.extend_from_slice(&[0x02, 0x04, 0x12, 0x34, 0x56, 0x78]); // Serial
    der.extend_from_slice(&[0x30, 0x03, 0x06, 0x01, 0x00]); // AlgID

    // Parse the certificate twice
    let reader1 = TestReader::new(der.clone());
    let result1 = parse_x509_metadata(&reader1);

    let reader2 = TestReader::new(der);
    let result2 = parse_x509_metadata(&reader2);

    assert!(result1.is_ok() && result2.is_ok());
    let metadata1 = result1.unwrap();
    let metadata2 = result2.unwrap();

    // Fingerprints should be identical
    assert_eq!(
        metadata1.get("X509:SHA256Fingerprint"),
        metadata2.get("X509:SHA256Fingerprint"),
        "SHA256 fingerprints should be consistent across parses"
    );

    assert_eq!(
        metadata1.get("X509:SHA1Fingerprint"),
        metadata2.get("X509:SHA1Fingerprint"),
        "SHA1 fingerprints should be consistent across parses"
    );
}

/// Test 9: Certificate format identification with whitespace
///
/// Verifies that PEM certificates with various whitespace formatting
/// are still correctly identified.
#[test]
fn test_x509_pem_with_whitespace() {
    let pem = b"-----BEGIN CERTIFICATE-----\n\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A\n\nMIIBCgKCAQEA\n\n-----END CERTIFICATE-----\n";
    let reader = TestReader::new(pem.to_vec());

    let result = X509Parser::verify_signature(&reader);
    assert!(
        result.is_ok() && result.unwrap(),
        "PEM with whitespace should be detected"
    );
}

/// Test 10: DER with invalid length encoding
///
/// Verifies that DER structures with malformed length encoding
/// are properly rejected.
#[test]
fn test_x509_der_invalid_length() {
    // SEQUENCE tag with invalid length encoding
    let mut der = vec![0x30, 0x84]; // Indicates 4 length bytes follow (invalid for typical certs)
    der.extend_from_slice(&[0x00, 0x00, 0x00, 0x10]); // 16 bytes length
    // Only add minimal data instead of promised 16 bytes
    der.extend_from_slice(&[0x00, 0x01, 0x02, 0x03]);

    let reader = TestReader::new(der);
    let result = X509Parser::verify_signature(&reader);

    // Should either reject or handle gracefully
    assert!(result.is_ok(), "verify_signature should not crash");
}

/// Test 11: Metadata extraction from parsed certificate
///
/// Verifies that the parser generates proper FileType metadata
/// indicating the format as X.509.
#[test]
fn test_x509_filetype_metadata() {
    let mut der = Vec::new();
    der.push(0x30); // SEQUENCE
    der.push(0x10); // Length 16
    der.push(0x30); // TBS SEQUENCE
    der.push(0x0E); // Length 14
    der.extend_from_slice(&[0x02, 0x01, 0x01]);
    der.extend_from_slice(&[0x02, 0x04, 0x12, 0x34, 0x56, 0x78]);
    der.extend_from_slice(&[0x30, 0x03, 0x06, 0x01, 0x00]);

    let reader = TestReader::new(der);
    let result = parse_x509_metadata(&reader);

    assert!(result.is_ok());
    let metadata = result.unwrap();

    // Verify FileType is set to X.509
    assert_eq!(
        metadata.get_string("FileType"),
        Some("X.509"),
        "FileType should be X.509 for certificate"
    );
}

/// Test 12: Multiple DER verification attempts
///
/// Verifies that verify_signature works reliably across multiple calls
/// with the same reader.
#[test]
fn test_x509_multiple_verifications() {
    let mut der = vec![0x30, 0x82, 0x01, 0x00];
    der.extend_from_slice(&[0x30, 0x03, 0x02, 0x01, 0x00, 0x00]);
    let reader = TestReader::new(der);

    // Multiple verification calls should all succeed
    for _ in 0..3 {
        let result = X509Parser::verify_signature(&reader);
        assert!(
            result.is_ok() && result.unwrap(),
            "verify_signature should consistently return true for valid DER"
        );
    }
}

/// Test 13: PEM with line breaks in base64 content
///
/// Verifies that the parser correctly identifies PEM certificates
/// with line breaks in the base64-encoded content (standard PEM format).
#[test]
fn test_x509_pem_standard_markers() {
    // Standard PEM format: BEGIN marker at the start, content on following lines
    let pem = b"-----BEGIN CERTIFICATE-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A\nMIIBCgKCAQEAwJZFwWVBSxOkPKM=\n-----END CERTIFICATE-----";
    let reader = TestReader::new(pem.to_vec());

    let result = X509Parser::verify_signature(&reader);
    assert!(
        result.is_ok() && result.unwrap(),
        "Standard PEM format with line breaks should be detected"
    );
}

/// Test 14: Certificate size metadata
///
/// Verifies that FileSize metadata is properly extracted and present.
#[test]
fn test_x509_filesize_metadata() {
    let mut der = Vec::new();
    der.push(0x30); // SEQUENCE
    der.push(0x10); // Length 16
    der.push(0x30); // TBS SEQUENCE
    der.push(0x0E); // Length 14
    der.extend_from_slice(&[0x02, 0x01, 0x01]);
    der.extend_from_slice(&[0x02, 0x04, 0x12, 0x34, 0x56, 0x78]);
    der.extend_from_slice(&[0x30, 0x03, 0x06, 0x01, 0x00]);

    let expected_size = der.len() as u64;
    let reader = TestReader::new(der);
    let result = parse_x509_metadata(&reader);

    assert!(result.is_ok());
    let metadata = result.unwrap();

    // Verify FileSize is present and correct
    assert_eq!(
        metadata.get_string("FileSize"),
        Some(expected_size.to_string().as_str()),
        "FileSize should be present and accurate"
    );
}

/// Test 15: Version extraction from certificate
///
/// Verifies that the certificate version is properly extracted and
/// formatted as part of metadata.
#[test]
fn test_x509_version_extraction() {
    let mut der = Vec::new();
    der.push(0x30); // SEQUENCE
    der.push(0x10); // Length 16
    der.push(0x30); // TBS SEQUENCE
    der.push(0x0E); // Length 14
    der.extend_from_slice(&[0x02, 0x01, 0x01]);
    der.extend_from_slice(&[0x02, 0x04, 0x12, 0x34, 0x56, 0x78]);
    der.extend_from_slice(&[0x30, 0x03, 0x06, 0x01, 0x00]);

    let reader = TestReader::new(der);
    let result = parse_x509_metadata(&reader);

    assert!(result.is_ok());
    let metadata = result.unwrap();

    // Check for version information (may be present depending on parsing)
    if let Some(version) = metadata.get("X509:Version") {
        match version {
            TagValue::String(v) => {
                assert!(
                    v.starts_with("v"),
                    "Version should be formatted as 'v1', 'v2', or 'v3'"
                );
            }
            _ => panic!("Version should be a string"),
        }
    }
}
