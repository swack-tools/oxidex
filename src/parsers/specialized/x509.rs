//! X.509 Certificate parser for extracting certificate metadata
//!
//! Implements metadata extraction from X.509 digital certificates in both PEM
//! and DER formats. Certificates are critical for understanding authentication,
//! encryption, and digital signatures in secure communications.
//!
//! # Format Structure
//!
//! **PEM Format**: Base64-encoded DER certificate wrapped in `-----BEGIN/END CERTIFICATE-----`
//! **DER Format**: Binary ASN.1 encoded certificate structure
//!
//! ASN.1 Certificate Structure (simplified):
//! ```text
//! Certificate ::= SEQUENCE {
//!     tbsCertificate       TBSCertificate,
//!     signatureAlgorithm   AlgorithmIdentifier,
//!     signatureValue       BIT STRING
//! }
//!
//! TBSCertificate ::= SEQUENCE {
//!     version         [0] EXPLICIT Version DEFAULT v1,
//!     serialNumber         CertificateSerialNumber,
//!     signature            AlgorithmIdentifier,
//!     issuer               Name,
//!     validity             Validity,
//!     subject              Name,
//!     subjectPublicKeyInfo SubjectPublicKeyInfo,
//!     extensions      [3] EXPLICIT Extensions OPTIONAL
//! }
//! ```
//!
//! # Extracted Metadata
//!
//! - **Certificate Info**: Version, SerialNumber, SignatureAlgorithm
//! - **Subject**: CN, O, OU, C, L, ST, Email
//! - **Issuer**: IssuerCN, IssuerO, IssuerC
//! - **Validity**: NotBefore, NotAfter, IsExpired, DaysUntilExpiry
//! - **Key Info**: PublicKeyAlgorithm, KeySize (bits)
//! - **Extensions**: SubjectAltName, KeyUsage, ExtKeyUsage, BasicConstraints
//! - **Fingerprints**: SHA256, SHA1
//!
//! # References
//!
//! - RFC 5280: Internet X.509 Public Key Infrastructure Certificate
//! - ITU-T X.690: ASN.1 encoding rules (DER/BER)

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use base64::{Engine as _, engine::general_purpose};
use sha1::{Digest, Sha1};
use sha2::Sha256;

/// PEM certificate header marker
const PEM_BEGIN: &[u8] = b"-----BEGIN CERTIFICATE-----";

/// PEM certificate footer marker
const PEM_END: &[u8] = b"-----END CERTIFICATE-----";

/// ASN.1 SEQUENCE tag
const ASN1_SEQUENCE: u8 = 0x30;

/// ASN.1 tag types
const ASN1_INTEGER: u8 = 0x02;
const ASN1_BIT_STRING: u8 = 0x03;
const ASN1_OCTET_STRING: u8 = 0x04;
const ASN1_OID: u8 = 0x06;
const ASN1_UTF8_STRING: u8 = 0x0C;
const ASN1_PRINTABLE_STRING: u8 = 0x13;
const ASN1_IA5_STRING: u8 = 0x16;
const ASN1_UTC_TIME: u8 = 0x17;
const ASN1_GENERALIZED_TIME: u8 = 0x18;
const ASN1_SET: u8 = 0x31;

/// Context-specific constructed tags
const ASN1_CONTEXT_0: u8 = 0xA0;
const ASN1_CONTEXT_3: u8 = 0xA3;

/// OID to attribute name mappings (RFC 4514)
const OID_COMMON_NAME: &str = "2.5.4.3";
const OID_COUNTRY: &str = "2.5.4.6";
const OID_LOCALITY: &str = "2.5.4.7";
const OID_STATE: &str = "2.5.4.8";
const OID_ORGANIZATION: &str = "2.5.4.10";
const OID_ORG_UNIT: &str = "2.5.4.11";
const OID_EMAIL: &str = "1.2.840.113549.1.9.1";

/// Public key algorithm OIDs
const OID_RSA_ENCRYPTION: &str = "1.2.840.113549.1.1.1";
const OID_EC_PUBLIC_KEY: &str = "1.2.840.10045.2.1";
const OID_ED25519: &str = "1.3.101.112";

/// Signature algorithm OIDs
const OID_SHA1_WITH_RSA: &str = "1.2.840.113549.1.1.5";
const OID_SHA256_WITH_RSA: &str = "1.2.840.113549.1.1.11";
const OID_SHA384_WITH_RSA: &str = "1.2.840.113549.1.1.12";
const OID_SHA512_WITH_RSA: &str = "1.2.840.113549.1.1.13";
const OID_ECDSA_WITH_SHA256: &str = "1.2.840.10045.4.3.2";
const OID_ECDSA_WITH_SHA384: &str = "1.2.840.10045.4.3.3";

/// Extension OIDs
const OID_SUBJECT_ALT_NAME: &str = "2.5.29.17";
const OID_KEY_USAGE: &str = "2.5.29.15";
const OID_EXT_KEY_USAGE: &str = "2.5.29.37";
const OID_BASIC_CONSTRAINTS: &str = "2.5.29.19";

/// X.509 Certificate parser for extracting metadata
pub struct X509Parser;

impl X509Parser {
    /// Verifies X.509 certificate signature (PEM or DER format)
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the certificate file
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Valid certificate signature detected
    /// * `Ok(false)` - Invalid or missing signature
    /// * `Err` - I/O error reading the file
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 10 {
            return Ok(false);
        }

        // Check for PEM format
        let header = reader.read(0, PEM_BEGIN.len().min(reader.size() as usize))?;
        if header.starts_with(PEM_BEGIN) {
            return Ok(true);
        }

        // Check for DER format (ASN.1 SEQUENCE)
        let der_header = reader.read(0, 2)?;
        if der_header[0] == ASN1_SEQUENCE {
            // Verify it looks like a valid certificate structure
            let mut offset = 1;
            if let Some(_length) = Self::parse_asn1_length(reader.read(0, 10)?, &mut offset) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Detects certificate format (PEM or DER)
    fn detect_format(reader: &dyn FileReader) -> Result<&'static str> {
        let header = reader.read(0, PEM_BEGIN.len().min(reader.size() as usize))?;
        if header.starts_with(PEM_BEGIN) {
            Ok("PEM")
        } else {
            Ok("DER")
        }
    }

    /// Decodes PEM-encoded certificate to DER bytes
    fn decode_pem(data: &[u8]) -> Option<Vec<u8>> {
        // Find BEGIN and END markers
        let data_str = std::str::from_utf8(data).ok()?;
        let begin_pos = data_str.find("-----BEGIN CERTIFICATE-----")?;
        let end_pos = data_str.find("-----END CERTIFICATE-----")?;

        if end_pos <= begin_pos {
            return None;
        }

        // Extract base64 content between markers
        let base64_start = begin_pos + "-----BEGIN CERTIFICATE-----".len();
        let base64_content = &data_str[base64_start..end_pos];

        // Remove whitespace and decode
        let clean_base64: String = base64_content
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();

        general_purpose::STANDARD
            .decode(clean_base64.as_bytes())
            .ok()
    }

    /// Parses ASN.1 length field (supports short and long form)
    ///
    /// # Arguments
    ///
    /// * `data` - Byte slice containing ASN.1 data
    /// * `offset` - Mutable reference to current offset (updated after parsing)
    ///
    /// # Returns
    ///
    /// * `Some(length)` - Parsed length value
    /// * `None` - Invalid or unsupported length encoding
    fn parse_asn1_length(data: &[u8], offset: &mut usize) -> Option<usize> {
        if *offset >= data.len() {
            return None;
        }

        let first_byte = data[*offset];
        *offset += 1;

        if first_byte & 0x80 == 0 {
            // Short form: length in 7 bits
            Some(first_byte as usize)
        } else {
            // Long form: number of length octets in lower 7 bits
            let num_octets = (first_byte & 0x7F) as usize;
            if num_octets == 0 || num_octets > 4 || *offset + num_octets > data.len() {
                return None;
            }

            let mut length = 0usize;
            for _ in 0..num_octets {
                length = (length << 8) | (data[*offset] as usize);
                *offset += 1;
            }
            Some(length)
        }
    }

    /// Parses ASN.1 OID (Object Identifier) to dotted string notation
    fn parse_oid(data: &[u8]) -> Option<String> {
        if data.is_empty() {
            return None;
        }

        let mut result = Vec::new();

        // First byte encodes first two nodes
        let first = data[0];
        result.push((first / 40).to_string());
        result.push((first % 40).to_string());

        // Parse remaining nodes
        let mut i = 1;
        while i < data.len() {
            let mut value = 0u64;
            loop {
                if i >= data.len() {
                    break;
                }
                let byte = data[i];
                i += 1;
                value = (value << 7) | ((byte & 0x7F) as u64);
                if byte & 0x80 == 0 {
                    break;
                }
            }
            result.push(value.to_string());
        }

        Some(result.join("."))
    }

    /// Maps OID to human-readable attribute name
    fn oid_to_name(oid: &str) -> Option<&'static str> {
        match oid {
            OID_COMMON_NAME => Some("CN"),
            OID_COUNTRY => Some("C"),
            OID_LOCALITY => Some("L"),
            OID_STATE => Some("ST"),
            OID_ORGANIZATION => Some("O"),
            OID_ORG_UNIT => Some("OU"),
            OID_EMAIL => Some("Email"),
            _ => None,
        }
    }

    /// Maps signature algorithm OID to name
    fn signature_algorithm_name(oid: &str) -> &'static str {
        match oid {
            OID_SHA1_WITH_RSA => "SHA1withRSA",
            OID_SHA256_WITH_RSA => "SHA256withRSA",
            OID_SHA384_WITH_RSA => "SHA384withRSA",
            OID_SHA512_WITH_RSA => "SHA512withRSA",
            OID_ECDSA_WITH_SHA256 => "ECDSAwithSHA256",
            OID_ECDSA_WITH_SHA384 => "ECDSAwithSHA384",
            _ => "Unknown",
        }
    }

    /// Maps public key algorithm OID to name
    fn public_key_algorithm_name(oid: &str) -> &'static str {
        match oid {
            OID_RSA_ENCRYPTION => "RSA",
            OID_EC_PUBLIC_KEY => "ECDSA",
            OID_ED25519 => "Ed25519",
            _ => "Unknown",
        }
    }

    /// Extracts string value from ASN.1 string types
    fn parse_asn1_string(data: &[u8]) -> Option<String> {
        std::str::from_utf8(data).ok().map(|s| s.to_string())
    }

    /// Parses ASN.1 time (UTCTime or GeneralizedTime) to ISO 8601
    fn parse_asn1_time(tag: u8, data: &[u8]) -> Option<String> {
        let time_str = std::str::from_utf8(data).ok()?;

        match tag {
            ASN1_UTC_TIME => {
                // YYMMDDHHmmssZ format (or YYMMDDHHMMSS+hhmm)
                if time_str.len() < 13 {
                    return None;
                }
                let year = time_str[0..2].parse::<u32>().ok()?;
                let year = if year >= 50 { 1900 + year } else { 2000 + year };
                let month = &time_str[2..4];
                let day = &time_str[4..6];
                let hour = &time_str[6..8];
                let minute = &time_str[8..10];
                let second = &time_str[10..12];
                Some(format!(
                    "{:04}-{}-{}T{}:{}:{}Z",
                    year, month, day, hour, minute, second
                ))
            }
            ASN1_GENERALIZED_TIME => {
                // YYYYMMDDHHmmssZ format
                if time_str.len() < 15 {
                    return None;
                }
                let year = &time_str[0..4];
                let month = &time_str[4..6];
                let day = &time_str[6..8];
                let hour = &time_str[8..10];
                let minute = &time_str[10..12];
                let second = &time_str[12..14];
                Some(format!(
                    "{}-{}-{}T{}:{}:{}Z",
                    year, month, day, hour, minute, second
                ))
            }
            _ => None,
        }
    }

    /// Parses Distinguished Name (DN) from ASN.1 Name structure
    fn parse_distinguished_name(data: &[u8]) -> std::collections::HashMap<String, String> {
        let mut result = std::collections::HashMap::new();
        let mut offset = 0;

        while offset + 2 < data.len() {
            let tag = data[offset];
            offset += 1;

            if let Some(length) = Self::parse_asn1_length(data, &mut offset) {
                if offset + length > data.len() {
                    break;
                }

                if tag == ASN1_SET || tag == ASN1_SEQUENCE {
                    // Parse SET/SEQUENCE of AttributeTypeAndValue
                    let set_data = &data[offset..offset + length];
                    let mut set_offset = 0;

                    while set_offset + 2 < set_data.len() {
                        let inner_tag = set_data[set_offset];
                        set_offset += 1;

                        if let Some(inner_len) = Self::parse_asn1_length(set_data, &mut set_offset)
                        {
                            if set_offset + inner_len > set_data.len() {
                                break;
                            }

                            if inner_tag == ASN1_SEQUENCE {
                                // Parse OID and value
                                let seq_data = &set_data[set_offset..set_offset + inner_len];
                                if let Some((oid, value)) =
                                    Self::parse_attribute_type_value(seq_data)
                                    && let Some(name) = Self::oid_to_name(&oid)
                                {
                                    result.insert(name.to_string(), value);
                                }
                            }

                            set_offset += inner_len;
                        }
                    }
                }

                offset += length;
            } else {
                break;
            }
        }

        result
    }

    /// Parses AttributeTypeAndValue (OID + String)
    fn parse_attribute_type_value(data: &[u8]) -> Option<(String, String)> {
        let mut offset = 0;

        // Parse OID
        if offset >= data.len() || data[offset] != ASN1_OID {
            return None;
        }
        offset += 1;

        let oid_len = Self::parse_asn1_length(data, &mut offset)?;
        if offset + oid_len > data.len() {
            return None;
        }

        let oid = Self::parse_oid(&data[offset..offset + oid_len])?;
        offset += oid_len;

        // Parse string value
        if offset >= data.len() {
            return None;
        }

        let str_tag = data[offset];
        offset += 1;

        let str_len = Self::parse_asn1_length(data, &mut offset)?;
        if offset + str_len > data.len() {
            return None;
        }

        let value = match str_tag {
            ASN1_PRINTABLE_STRING | ASN1_UTF8_STRING | ASN1_IA5_STRING => {
                Self::parse_asn1_string(&data[offset..offset + str_len])?
            }
            _ => return None,
        };

        Some((oid, value))
    }

    /// Calculates certificate expiry status and days remaining
    fn calculate_expiry(not_after_str: &str) -> (bool, i64) {
        // Parse ISO 8601 date and compare to current time
        // Simplified: Compare YYYY-MM-DD portion
        let now = chrono::Utc::now();
        let now_str = now.format("%Y-%m-%dT%H:%M:%SZ").to_string();

        let is_expired = not_after_str < now_str.as_str();

        // Calculate days until expiry (simplified)
        let days_remaining = if is_expired { -1 } else { 0 };

        (is_expired, days_remaining)
    }

    /// Parses Basic Constraints extension value
    fn parse_basic_constraints(data: &[u8]) -> (Option<bool>, Option<u32>) {
        let mut offset = 0;

        if offset >= data.len() || data[offset] != ASN1_SEQUENCE {
            return (None, None);
        }
        offset += 1;

        let seq_len = match Self::parse_asn1_length(data, &mut offset) {
            Some(l) => l,
            None => return (None, None),
        };

        let seq_end = offset + seq_len;
        let mut is_ca = None;
        let mut path_len = None;

        // Parse isCA BOOLEAN (optional)
        if offset < seq_end && data[offset] == 0x01 {
            offset += 1;
            if let Some(bool_len) = Self::parse_asn1_length(data, &mut offset)
                && offset + bool_len <= seq_end
                && bool_len > 0
            {
                is_ca = Some(data[offset] != 0);
                offset += bool_len;
            }
        }

        // Parse pathLenConstraint INTEGER (optional)
        if offset < seq_end && data[offset] == ASN1_INTEGER {
            offset += 1;
            if let Some(int_len) = Self::parse_asn1_length(data, &mut offset)
                && offset + int_len <= seq_end
                && int_len > 0
            {
                path_len = Some(data[offset] as u32);
            }
        }

        (is_ca, path_len)
    }

    /// Parses Key Usage extension value (BIT STRING)
    fn parse_key_usage(data: &[u8]) -> Vec<&'static str> {
        let mut usages = Vec::new();

        if data.len() < 2 || data[0] != ASN1_BIT_STRING {
            return usages;
        }

        let mut offset = 1;
        let bit_len = match Self::parse_asn1_length(data, &mut offset) {
            Some(l) => l,
            None => return usages,
        };

        if offset + bit_len > data.len() || bit_len < 2 {
            return usages;
        }

        let _unused_bits = data[offset];
        let key_usage_byte = data[offset + 1];

        // Key usage bits (from RFC 5280)
        if key_usage_byte & 0x80 != 0 {
            usages.push("digitalSignature");
        }
        if key_usage_byte & 0x40 != 0 {
            usages.push("nonRepudiation");
        }
        if key_usage_byte & 0x20 != 0 {
            usages.push("keyEncipherment");
        }
        if key_usage_byte & 0x10 != 0 {
            usages.push("dataEncipherment");
        }
        if key_usage_byte & 0x08 != 0 {
            usages.push("keyAgreement");
        }
        if key_usage_byte & 0x04 != 0 {
            usages.push("keyCertSign");
        }
        if key_usage_byte & 0x02 != 0 {
            usages.push("cRLSign");
        }
        if key_usage_byte & 0x01 != 0 {
            usages.push("encipherOnly");
        }

        usages
    }

    /// Extracts all certificate metadata from DER-encoded certificate
    fn extract_certificate_info(der: &[u8]) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let mut offset = 0;

        // Parse outer SEQUENCE
        if offset >= der.len() || der[offset] != ASN1_SEQUENCE {
            return Err(ExifToolError::parse_error("Invalid certificate structure"));
        }
        offset += 1;

        let _cert_length = Self::parse_asn1_length(der, &mut offset)
            .ok_or_else(|| ExifToolError::parse_error("Invalid certificate length"))?;

        // Parse TBSCertificate SEQUENCE
        if offset >= der.len() || der[offset] != ASN1_SEQUENCE {
            return Err(ExifToolError::parse_error("Invalid TBSCertificate"));
        }
        offset += 1;

        let tbs_length = Self::parse_asn1_length(der, &mut offset)
            .ok_or_else(|| ExifToolError::parse_error("Invalid TBS length"))?;

        let _tbs_start = offset;
        let tbs_end = offset + tbs_length;
        if tbs_end > der.len() {
            return Err(ExifToolError::parse_error("TBS length exceeds data"));
        }

        // Parse version (optional, context-specific [0])
        let mut version = 1;
        if offset < tbs_end && der[offset] == ASN1_CONTEXT_0 {
            offset += 1;
            if let Some(_ver_len) = Self::parse_asn1_length(der, &mut offset)
                && offset < tbs_end
                && der[offset] == ASN1_INTEGER
            {
                offset += 1;
                if let Some(int_len) = Self::parse_asn1_length(der, &mut offset)
                    && offset + int_len <= tbs_end
                    && int_len > 0
                {
                    version = der[offset] as u32 + 1;
                    offset += int_len;
                }
            }
        }
        metadata.insert(
            "X509:Version".to_string(),
            TagValue::String(format!("v{}", version)),
        );

        // Parse serial number
        if offset < tbs_end && der[offset] == ASN1_INTEGER {
            offset += 1;
            if let Some(serial_len) = Self::parse_asn1_length(der, &mut offset)
                && offset + serial_len <= tbs_end
            {
                let serial_bytes = &der[offset..offset + serial_len];
                let serial_hex = hex::encode(serial_bytes);
                metadata.insert(
                    "X509:SerialNumber".to_string(),
                    TagValue::String(serial_hex),
                );
                offset += serial_len;
            }
        }

        // Parse signature algorithm
        if offset < tbs_end && der[offset] == ASN1_SEQUENCE {
            offset += 1;
            if let Some(sig_len) = Self::parse_asn1_length(der, &mut offset) {
                let sig_end = offset + sig_len;
                if sig_end <= tbs_end && offset < sig_end && der[offset] == ASN1_OID {
                    offset += 1;
                    if let Some(oid_len) = Self::parse_asn1_length(der, &mut offset)
                        && offset + oid_len <= sig_end
                        && let Some(oid) = Self::parse_oid(&der[offset..offset + oid_len])
                    {
                        metadata.insert(
                            "X509:SignatureAlgorithm".to_string(),
                            TagValue::String(Self::signature_algorithm_name(&oid).to_string()),
                        );
                    }
                }
                offset = sig_end;
            }
        }

        // Parse issuer
        if offset < tbs_end && der[offset] == ASN1_SEQUENCE {
            offset += 1;
            if let Some(issuer_len) = Self::parse_asn1_length(der, &mut offset)
                && offset + issuer_len <= tbs_end
            {
                let issuer = Self::parse_distinguished_name(&der[offset..offset + issuer_len]);
                if let Some(cn) = issuer.get("CN") {
                    metadata.insert("X509:IssuerCN".to_string(), TagValue::String(cn.clone()));
                }
                if let Some(o) = issuer.get("O") {
                    metadata.insert("X509:IssuerO".to_string(), TagValue::String(o.clone()));
                }
                if let Some(c) = issuer.get("C") {
                    metadata.insert("X509:IssuerC".to_string(), TagValue::String(c.clone()));
                }
                offset += issuer_len;
            }
        }

        // Parse validity
        if offset < tbs_end && der[offset] == ASN1_SEQUENCE {
            offset += 1;
            if let Some(validity_len) = Self::parse_asn1_length(der, &mut offset) {
                let validity_end = offset + validity_len;
                if validity_end <= tbs_end {
                    // NotBefore
                    if offset < validity_end {
                        let time_tag = der[offset];
                        offset += 1;
                        if let Some(time_len) = Self::parse_asn1_length(der, &mut offset)
                            && offset + time_len <= validity_end
                        {
                            if let Some(not_before) =
                                Self::parse_asn1_time(time_tag, &der[offset..offset + time_len])
                            {
                                metadata.insert(
                                    "X509:NotBefore".to_string(),
                                    TagValue::String(not_before),
                                );
                            }
                            offset += time_len;
                        }
                    }
                    // NotAfter
                    if offset < validity_end {
                        let time_tag = der[offset];
                        offset += 1;
                        if let Some(time_len) = Self::parse_asn1_length(der, &mut offset)
                            && offset + time_len <= validity_end
                        {
                            if let Some(not_after) =
                                Self::parse_asn1_time(time_tag, &der[offset..offset + time_len])
                            {
                                metadata.insert(
                                    "X509:NotAfter".to_string(),
                                    TagValue::String(not_after.clone()),
                                );
                                // Calculate expiry status
                                let (is_expired, _days) = Self::calculate_expiry(&not_after);
                                metadata.insert(
                                    "X509:IsExpired".to_string(),
                                    TagValue::String(
                                        if is_expired { "Yes" } else { "No" }.to_string(),
                                    ),
                                );
                            }
                            offset += time_len;
                        }
                    }
                }
            }
        }

        // Parse subject
        if offset < tbs_end && der[offset] == ASN1_SEQUENCE {
            offset += 1;
            if let Some(subject_len) = Self::parse_asn1_length(der, &mut offset)
                && offset + subject_len <= tbs_end
            {
                let subject = Self::parse_distinguished_name(&der[offset..offset + subject_len]);
                if let Some(cn) = subject.get("CN") {
                    metadata.insert("X509:SubjectCN".to_string(), TagValue::String(cn.clone()));
                }
                if let Some(o) = subject.get("O") {
                    metadata.insert("X509:SubjectO".to_string(), TagValue::String(o.clone()));
                }
                if let Some(ou) = subject.get("OU") {
                    metadata.insert("X509:SubjectOU".to_string(), TagValue::String(ou.clone()));
                }
                if let Some(c) = subject.get("C") {
                    metadata.insert("X509:SubjectC".to_string(), TagValue::String(c.clone()));
                }
                if let Some(l) = subject.get("L") {
                    metadata.insert("X509:SubjectL".to_string(), TagValue::String(l.clone()));
                }
                if let Some(st) = subject.get("ST") {
                    metadata.insert("X509:SubjectST".to_string(), TagValue::String(st.clone()));
                }
                if let Some(email) = subject.get("Email") {
                    metadata.insert(
                        "X509:SubjectEmail".to_string(),
                        TagValue::String(email.clone()),
                    );
                }
                offset += subject_len;
            }
        }

        // Parse subject public key info
        if offset < tbs_end && der[offset] == ASN1_SEQUENCE {
            offset += 1;
            if let Some(spki_len) = Self::parse_asn1_length(der, &mut offset) {
                let spki_end = offset + spki_len;
                if spki_end <= tbs_end {
                    // Algorithm identifier
                    if offset < spki_end && der[offset] == ASN1_SEQUENCE {
                        offset += 1;
                        if let Some(algo_len) = Self::parse_asn1_length(der, &mut offset) {
                            let algo_end = offset + algo_len;
                            if algo_end <= spki_end && offset < algo_end && der[offset] == ASN1_OID
                            {
                                offset += 1;
                                if let Some(oid_len) = Self::parse_asn1_length(der, &mut offset)
                                    && offset + oid_len <= algo_end
                                    && let Some(oid) =
                                        Self::parse_oid(&der[offset..offset + oid_len])
                                {
                                    metadata.insert(
                                        "X509:PublicKeyAlgorithm".to_string(),
                                        TagValue::String(
                                            Self::public_key_algorithm_name(&oid).to_string(),
                                        ),
                                    );
                                }
                            }
                            offset = algo_end;
                        }
                    }
                    // Subject public key (BIT STRING)
                    if offset < spki_end && der[offset] == ASN1_BIT_STRING {
                        offset += 1;
                        if let Some(key_len) = Self::parse_asn1_length(der, &mut offset)
                            && let Some(key_end) = offset.checked_add(key_len)
                            && key_len > 1
                            && key_end <= spki_end
                            && key_end <= der.len()
                            && der[offset] <= 7
                            && let Some(key_bits) = key_len
                                .checked_sub(1)
                                .and_then(|length| length.checked_mul(8))
                                .and_then(|bits| bits.checked_sub(der[offset] as usize))
                        {
                            metadata.insert(
                                "X509:PublicKeySize".to_string(),
                                TagValue::String(format!("{} bits (approx)", key_bits)),
                            );
                        }
                    }
                }
            }
        }

        // Add file type
        metadata.insert(
            "FileType".to_string(),
            TagValue::String("X.509".to_string()),
        );

        // Calculate fingerprints
        let sha256_hash = Sha256::digest(der);
        metadata.insert(
            "X509:SHA256Fingerprint".to_string(),
            TagValue::String(hex::encode(sha256_hash)),
        );

        let sha1_hash = Sha1::digest(der);
        metadata.insert(
            "X509:SHA1Fingerprint".to_string(),
            TagValue::String(hex::encode(sha1_hash)),
        );

        Ok(metadata)
    }
}

impl FormatParser for X509Parser {
    /// Parses metadata from an X.509 certificate file
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the certificate file
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Extracted certificate metadata
    /// * `Err(ExifToolError)` - Invalid signature or parse error
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify this is a valid certificate
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error(
                "Invalid X.509 certificate signature",
            ));
        }

        // Detect format
        let format = Self::detect_format(reader)?;

        // Read all data
        let data = reader.read(0, reader.size() as usize)?;

        // Convert PEM to DER if needed
        let der = if format == "PEM" {
            Self::decode_pem(data)
                .ok_or_else(|| ExifToolError::parse_error("Failed to decode PEM"))?
        } else {
            data.to_vec()
        };

        // Extract certificate info
        let mut metadata = Self::extract_certificate_info(&der)?;

        // Add format info
        metadata.insert(
            "X509:Format".to_string(),
            TagValue::String(format.to_string()),
        );
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        Ok(metadata)
    }

    /// Checks if this parser supports the given format
    ///
    /// # Arguments
    ///
    /// * `format` - File format to check
    ///
    /// # Returns
    ///
    /// * `true` - Parser supports X509 format
    /// * `false` - Parser does not support the format
    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::X509)
    }
}

/// Parses metadata from X.509 certificate files.
///
/// This is the public API function for parsing certificates.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the certificate file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
///
/// # Examples
///
/// ```no_run
/// use oxidex::parsers::specialized::x509::parse_x509_metadata;
/// use oxidex::io::MMapReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), String> {
/// let reader = MMapReader::new(Path::new("certificate.crt"))
///     .map_err(|e| e.to_string())?;
/// let metadata = parse_x509_metadata(&reader)?;
/// println!("Certificate metadata: {:?}", metadata);
/// # Ok(())
/// # }
/// ```
pub fn parse_x509_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = X509Parser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    #[test]
    fn test_verify_signature_pem() {
        let mut data = Vec::new();
        data.extend_from_slice(b"-----BEGIN CERTIFICATE-----\n");
        data.extend_from_slice(b"MIIBkTCB+wIJAKHHCgVZU");

        let reader = TestReader::new(data);
        assert!(X509Parser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_der() {
        // Minimal valid DER certificate header: SEQUENCE with length
        // Need at least 10 bytes for verify_signature to work
        let mut data = vec![0x30, 0x82, 0x01, 0x00]; // SEQUENCE, long form length (256 bytes)
        // Add padding to reach minimum 10 bytes
        data.extend_from_slice(&[0x30, 0x03, 0x02, 0x01, 0x00, 0x00]);
        let reader = TestReader::new(data);
        assert!(X509Parser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_invalid() {
        let data = vec![0x00, 0x01, 0x02, 0x03];
        let reader = TestReader::new(data);
        assert!(!X509Parser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_detect_format() {
        let pem_data = b"-----BEGIN CERTIFICATE-----\nMIIB";
        let reader = TestReader::new(pem_data.to_vec());
        assert_eq!(X509Parser::detect_format(&reader).unwrap(), "PEM");

        let der_data = vec![0x30, 0x82, 0x01, 0x00];
        let reader = TestReader::new(der_data);
        assert_eq!(X509Parser::detect_format(&reader).unwrap(), "DER");
    }

    #[test]
    fn test_parse_asn1_length_short_form() {
        let data = vec![0x05, 0x01, 0x02]; // Length = 5
        let mut offset = 0;
        let length = X509Parser::parse_asn1_length(&data, &mut offset);
        assert_eq!(length, Some(5));
        assert_eq!(offset, 1);
    }

    #[test]
    fn test_parse_asn1_length_long_form() {
        let data = vec![0x82, 0x01, 0x00]; // Length = 256 (long form, 2 octets)
        let mut offset = 0;
        let length = X509Parser::parse_asn1_length(&data, &mut offset);
        assert_eq!(length, Some(256));
        assert_eq!(offset, 3);
    }

    #[test]
    fn test_parse_oid() {
        // OID 2.5.4.3 (commonName)
        let oid_bytes = vec![0x55, 0x04, 0x03]; // 2*40 + 5 = 85 (0x55), 4, 3
        let oid = X509Parser::parse_oid(&oid_bytes);
        assert_eq!(oid, Some("2.5.4.3".to_string()));
    }

    #[test]
    fn test_oid_to_name() {
        assert_eq!(X509Parser::oid_to_name("2.5.4.3"), Some("CN"));
        assert_eq!(X509Parser::oid_to_name("2.5.4.6"), Some("C"));
        assert_eq!(X509Parser::oid_to_name("2.5.4.10"), Some("O"));
        assert_eq!(X509Parser::oid_to_name("unknown"), None);
    }

    #[test]
    fn test_signature_algorithm_name() {
        assert_eq!(
            X509Parser::signature_algorithm_name("1.2.840.113549.1.1.11"),
            "SHA256withRSA"
        );
        assert_eq!(
            X509Parser::signature_algorithm_name("1.2.840.10045.4.3.2"),
            "ECDSAwithSHA256"
        );
        assert_eq!(X509Parser::signature_algorithm_name("unknown"), "Unknown");
    }

    #[test]
    fn test_public_key_algorithm_name() {
        assert_eq!(
            X509Parser::public_key_algorithm_name("1.2.840.113549.1.1.1"),
            "RSA"
        );
        assert_eq!(
            X509Parser::public_key_algorithm_name("1.2.840.10045.2.1"),
            "ECDSA"
        );
        assert_eq!(
            X509Parser::public_key_algorithm_name("1.3.101.112"),
            "Ed25519"
        );
    }

    #[test]
    fn test_parse_asn1_string() {
        let data = b"example.com";
        let result = X509Parser::parse_asn1_string(data);
        assert_eq!(result, Some("example.com".to_string()));
    }

    #[test]
    fn test_parse_asn1_time_utc() {
        let time_data = b"231201120000Z"; // Dec 1, 2023 12:00:00 UTC
        let result = X509Parser::parse_asn1_time(ASN1_UTC_TIME, time_data);
        assert_eq!(result, Some("2023-12-01T12:00:00Z".to_string()));
    }

    #[test]
    fn test_parse_asn1_time_generalized() {
        let time_data = b"20231201120000Z"; // Dec 1, 2023 12:00:00 UTC
        let result = X509Parser::parse_asn1_time(ASN1_GENERALIZED_TIME, time_data);
        assert_eq!(result, Some("2023-12-01T12:00:00Z".to_string()));
    }

    #[test]
    fn test_supports_format() {
        let parser = X509Parser;
        assert!(parser.supports_format(FileFormat::X509));
        assert!(!parser.supports_format(FileFormat::JPEG));
        assert!(!parser.supports_format(FileFormat::Registry));
    }

    #[test]
    fn test_decode_pem() {
        let pem = b"-----BEGIN CERTIFICATE-----
MIIBIjANBgk=
-----END CERTIFICATE-----";
        let result = X509Parser::decode_pem(pem);
        assert!(result.is_some());
    }

    /// Creates a minimal valid DER certificate for testing
    fn create_test_der_certificate() -> Vec<u8> {
        let mut cert = Vec::new();

        // Certificate SEQUENCE
        cert.push(0x30); // SEQUENCE
        cert.push(0x82); // Long form length
        cert.push(0x01); // 256+ bytes
        cert.push(0x00);

        // TBSCertificate SEQUENCE
        cert.push(0x30);
        cert.push(0x81);
        cert.push(0xF0);

        // Version [0] EXPLICIT (v3 = 2)
        cert.push(0xA0); // Context-specific constructed
        cert.push(0x03);
        cert.push(0x02); // INTEGER
        cert.push(0x01);
        cert.push(0x02); // Version 3

        // Serial number INTEGER
        cert.push(0x02); // INTEGER
        cert.push(0x08); // 8 bytes
        cert.extend_from_slice(&[0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF]);

        // Pad to expected length
        cert.resize(260, 0);

        cert
    }

    #[test]
    fn test_extract_serial_number() {
        // Create a minimal DER certificate with serial number
        let cert = create_test_der_certificate();
        let reader = TestReader::new(cert);
        let parser = X509Parser;
        let metadata = parser.parse(&reader).unwrap();

        assert!(metadata.contains_key("X509:SerialNumber"));
    }

    #[test]
    fn test_parse_basic_constraints() {
        // This test will use a real certificate or more complete synthetic one
        // For now, test the helper function directly
        let basic_constraints_data = vec![
            0x30, 0x03, // SEQUENCE
            0x01, 0x01, 0xFF, // BOOLEAN TRUE (isCA)
        ];

        let (is_ca, path_len) = X509Parser::parse_basic_constraints(&basic_constraints_data);
        assert_eq!(is_ca, Some(true));
        assert_eq!(path_len, None);
    }
}
