//! PE Authenticode digital signature parser
//!
//! This module extracts digital signature information from PE files,
//! including certificate details, signer information, and counter-signatures.
//! Focus is on metadata extraction, not cryptographic validation.

use nom::{
    IResult,
    bytes::complete::{tag, take},
    number::complete::{le_u16, le_u32},
};

/// WIN_CERTIFICATE structure header (8 bytes minimum)
#[derive(Debug, Clone)]
pub struct WinCertificate {
    /// Total length including header (4 bytes)
    pub dw_length: u32,
    /// Certificate revision (2 bytes) - 0x0100 or 0x0200
    pub w_revision: u16,
    /// Certificate type (2 bytes) - 0x0002 for PKCS#7
    pub w_certificate_type: u16,
    /// PKCS#7 SignedData (variable length)
    pub certificate_data: Vec<u8>,
}

/// WIN_CERT_REVISION constants
pub mod cert_revision {
    /// Certificate revision 1.0
    pub const WIN_CERT_REVISION_1_0: u16 = 0x0100;
    /// Certificate revision 2.0
    pub const WIN_CERT_REVISION_2_0: u16 = 0x0200;
}

/// WIN_CERT_TYPE constants
pub mod cert_type {
    /// X.509 certificate format
    pub const WIN_CERT_TYPE_X509: u16 = 0x0001;
    /// PKCS#7 SignedData format
    pub const WIN_CERT_TYPE_PKCS_SIGNED_DATA: u16 = 0x0002;
    /// Reserved certificate type
    pub const WIN_CERT_TYPE_RESERVED_1: u16 = 0x0003;
    /// Timestamp signed certificate
    pub const WIN_CERT_TYPE_TS_STACK_SIGNED: u16 = 0x0004;
}

/// Parsed digital signature information
#[derive(Debug, Clone, Default)]
pub struct SignatureInfo {
    /// Whether a signature is present
    pub signature_present: bool,
    /// Signature type (e.g., "PKCS#7")
    pub signature_type: String,
    /// Number of certificates in the chain
    pub certificate_count: usize,
    /// Signer's Common Name (CN)
    pub signer_common_name: Option<String>,
    /// Signer's Organization (O)
    pub signer_organization: Option<String>,
    /// Issuer's Common Name (CN)
    pub issuer_common_name: Option<String>,
    /// Issuer's Organization (O)
    pub issuer_organization: Option<String>,
    /// Certificate serial number (hex string)
    pub certificate_serial_number: Option<String>,
    /// Certificate valid from date (RFC 3339 format)
    pub certificate_not_before: Option<String>,
    /// Certificate valid until date (RFC 3339 format)
    pub certificate_not_after: Option<String>,
    /// Certificate SHA-1 thumbprint (hex string)
    pub certificate_thumbprint: Option<String>,
    /// Whether a counter-signature is present
    pub has_counter_signature: bool,
    /// Counter-signature timestamp
    pub counter_signature_time: Option<String>,
    /// Whether signature structure is valid (not cryptographic validation)
    pub signature_valid: bool,
    /// Whether certificate has expired
    pub certificate_expired: bool,
}

/// Parse WIN_CERTIFICATE structure
pub fn parse_win_certificate(input: &[u8]) -> IResult<&[u8], WinCertificate> {
    let (input, dw_length) = le_u32(input)?;
    let (input, w_revision) = le_u16(input)?;
    let (input, w_certificate_type) = le_u16(input)?;

    // Certificate data length is total length minus header (8 bytes)
    let cert_data_len = (dw_length.saturating_sub(8)) as usize;
    let (input, certificate_data) = take(cert_data_len)(input)?;

    Ok((
        input,
        WinCertificate {
            dw_length,
            w_revision,
            w_certificate_type,
            certificate_data: certificate_data.to_vec(),
        },
    ))
}

/// Parse PKCS#7 SignedData and extract certificate information
pub fn parse_signature_info(cert_data: &[u8]) -> Option<SignatureInfo> {
    let mut info = SignatureInfo {
        signature_present: true,
        signature_type: "PKCS#7".to_string(),
        signature_valid: true,
        ..Default::default()
    };

    // Parse ASN.1 DER structure to find certificates
    match parse_pkcs7_signed_data(cert_data) {
        Ok(pkcs7_info) => {
            info.certificate_count = pkcs7_info.certificates.len();

            // Extract information from the first certificate (signer)
            if let Some(first_cert) = pkcs7_info.certificates.first() {
                info.signer_common_name = first_cert.subject_cn.clone();
                info.signer_organization = first_cert.subject_o.clone();
                info.issuer_common_name = first_cert.issuer_cn.clone();
                info.issuer_organization = first_cert.issuer_o.clone();
                info.certificate_serial_number = Some(first_cert.serial_number.clone());
                info.certificate_not_before = first_cert.not_before.clone();
                info.certificate_not_after = first_cert.not_after.clone();
                info.certificate_thumbprint = Some(first_cert.thumbprint.clone());

                // Check if certificate is expired
                if let Some(ref not_after) = first_cert.not_after {
                    info.certificate_expired = is_certificate_expired(not_after);
                }
            }

            // Check for counter-signature
            info.has_counter_signature = pkcs7_info.has_counter_signature;
            info.counter_signature_time = pkcs7_info.counter_signature_time;

            Some(info)
        }
        Err(_) => {
            info.signature_valid = false;
            Some(info)
        }
    }
}

/// PKCS#7 SignedData parsed information
#[derive(Debug, Clone, Default)]
struct Pkcs7Info {
    certificates: Vec<CertificateInfo>,
    has_counter_signature: bool,
    counter_signature_time: Option<String>,
}

/// X.509 Certificate information
#[derive(Debug, Clone, Default)]
struct CertificateInfo {
    subject_cn: Option<String>,
    subject_o: Option<String>,
    issuer_cn: Option<String>,
    issuer_o: Option<String>,
    serial_number: String,
    not_before: Option<String>,
    not_after: Option<String>,
    thumbprint: String,
}

/// Parse PKCS#7 SignedData ASN.1 structure
fn parse_pkcs7_signed_data(data: &[u8]) -> Result<Pkcs7Info, &'static str> {
    let mut pkcs7_info = Pkcs7Info::default();

    // Parse outer SEQUENCE
    let (_, data) = parse_asn1_sequence(data).map_err(|_| "Invalid PKCS#7 structure")?;

    // Look for ContentType OID (1.2.840.113549.1.7.2 = signedData)
    // Then parse the content for certificates

    // Find certificates section (context-specific tag [0])
    if let Some(certs_data) = find_certificates_section(data) {
        let mut remaining = certs_data;
        while !remaining.is_empty() {
            if let Ok((rest, cert_info)) = parse_x509_certificate(remaining) {
                pkcs7_info.certificates.push(cert_info);
                remaining = rest;
                if remaining.is_empty() {
                    break;
                }
            } else {
                break;
            }
        }
    }

    // Look for counter-signature in authenticated attributes
    if let Some(counter_sig_time) = find_counter_signature(data) {
        pkcs7_info.has_counter_signature = true;
        pkcs7_info.counter_signature_time = Some(counter_sig_time);
    }

    Ok(pkcs7_info)
}

/// Parse ASN.1 SEQUENCE tag and return content
fn parse_asn1_sequence(data: &[u8]) -> IResult<&[u8], &[u8]> {
    let (data, _) = tag([0x30u8].as_ref())(data)?; // SEQUENCE tag
    let (data, content) = parse_asn1_length_and_content(data)?;
    Ok((data, content))
}

/// Parse ASN.1 length encoding and return content
fn parse_asn1_length_and_content(data: &[u8]) -> IResult<&[u8], &[u8]> {
    if data.is_empty() {
        return Err(nom::Err::Error(nom::error::Error::new(
            data,
            nom::error::ErrorKind::Eof,
        )));
    }

    let length_byte = data[0];
    if length_byte & 0x80 == 0 {
        // Short form: length is in the first byte
        let length = length_byte as usize;
        let (data, _) = take(1usize)(data)?;
        let (data, content) = take(length)(data)?;
        Ok((data, content))
    } else {
        // Long form: first byte tells us how many bytes encode the length
        let num_length_bytes = (length_byte & 0x7F) as usize;
        let (data, _) = take(1usize)(data)?;
        let (data, length_bytes) = take(num_length_bytes)(data)?;

        // Decode length from bytes (big-endian)
        let mut length: usize = 0;
        for &byte in length_bytes {
            length = (length << 8) | byte as usize;
        }

        let (data, content) = take(length)(data)?;
        Ok((data, content))
    }
}

/// Find certificates section in PKCS#7 structure (context tag [0])
fn find_certificates_section(data: &[u8]) -> Option<&[u8]> {
    let mut offset = 0;
    while offset < data.len() {
        // Look for context-specific tag [0] (0xA0)
        if data[offset] == 0xA0
            && let Ok((_, content)) = parse_asn1_length_and_content(&data[offset + 1..])
        {
            return Some(content);
        }
        offset += 1;
    }
    None
}

/// Parse X.509 certificate from DER encoding
fn parse_x509_certificate(data: &[u8]) -> IResult<&[u8], CertificateInfo> {
    let cert_start = data;
    let (remaining, cert_data) = parse_asn1_sequence(data)?;

    let mut cert_info = CertificateInfo {
        thumbprint: calculate_sha1_hex(cert_start),
        ..Default::default()
    };

    // Parse TBSCertificate SEQUENCE
    if let Ok((_, tbs_data)) = parse_asn1_sequence(cert_data) {
        // Extract serial number (INTEGER after optional version)
        if let Some(serial) = extract_serial_number(tbs_data) {
            cert_info.serial_number = serial;
        }

        // Extract issuer DN
        if let Some(issuer_dn) = extract_distinguished_name(tbs_data, true) {
            cert_info.issuer_cn = issuer_dn.cn;
            cert_info.issuer_o = issuer_dn.o;
        }

        // Extract validity dates
        if let Some((not_before, not_after)) = extract_validity_dates(tbs_data) {
            cert_info.not_before = Some(not_before);
            cert_info.not_after = Some(not_after);
        }

        // Extract subject DN
        if let Some(subject_dn) = extract_distinguished_name(tbs_data, false) {
            cert_info.subject_cn = subject_dn.cn;
            cert_info.subject_o = subject_dn.o;
        }
    }

    Ok((remaining, cert_info))
}

/// Distinguished Name information
#[derive(Debug, Default)]
struct DistinguishedName {
    cn: Option<String>,
    o: Option<String>,
}

/// Extract serial number from TBSCertificate
fn extract_serial_number(tbs_data: &[u8]) -> Option<String> {
    let mut offset = 0;

    // Skip optional version (context tag [0])
    if offset < tbs_data.len()
        && tbs_data[offset] == 0xA0
        && let Ok((rest, _)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
    {
        offset = tbs_data.len() - rest.len();
    }

    // Parse serial number (INTEGER)
    if offset < tbs_data.len()
        && tbs_data[offset] == 0x02
        && let Ok((_, content)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
    {
        return Some(bytes_to_hex(content));
    }

    None
}

/// Extract Distinguished Name from TBSCertificate (issuer or subject)
fn extract_distinguished_name(tbs_data: &[u8], is_issuer: bool) -> Option<DistinguishedName> {
    let mut offset = 0;

    // Skip version, serial, signature algorithm
    // Version (optional)
    if offset < tbs_data.len()
        && tbs_data[offset] == 0xA0
        && let Ok((rest, _)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
    {
        offset = tbs_data.len() - rest.len();
    }

    // Serial number
    if offset < tbs_data.len()
        && tbs_data[offset] == 0x02
        && let Ok((rest, _)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
    {
        offset = tbs_data.len() - rest.len();
    }

    // Signature algorithm
    if offset < tbs_data.len()
        && tbs_data[offset] == 0x30
        && let Ok((rest, _)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
    {
        offset = tbs_data.len() - rest.len();
    }

    // Now we're at issuer DN (SEQUENCE)
    if offset < tbs_data.len()
        && tbs_data[offset] == 0x30
        && let Ok((rest, issuer_content)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
    {
        if is_issuer {
            return Some(parse_distinguished_name_content(issuer_content));
        }
        offset = tbs_data.len() - rest.len();

        // Skip validity (SEQUENCE)
        if offset < tbs_data.len()
            && tbs_data[offset] == 0x30
            && let Ok((rest, _)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
        {
            offset = tbs_data.len() - rest.len();
        }

        // Now we're at subject DN
        if offset < tbs_data.len()
            && tbs_data[offset] == 0x30
            && let Ok((_, subject_content)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
        {
            return Some(parse_distinguished_name_content(subject_content));
        }
    }

    None
}

/// Parse Distinguished Name content to extract CN and O
fn parse_distinguished_name_content(dn_data: &[u8]) -> DistinguishedName {
    let mut dn = DistinguishedName::default();
    let mut offset = 0;

    // OID for CN: 2.5.4.3
    let cn_oid = &[0x55, 0x04, 0x03];
    // OID for O: 2.5.4.10
    let o_oid = &[0x55, 0x04, 0x0A];

    while offset < dn_data.len() {
        // Each attribute is a SET containing a SEQUENCE
        if dn_data[offset] == 0x31 {
            // SET
            if let Ok((rest, set_content)) = parse_asn1_length_and_content(&dn_data[offset + 1..]) {
                // Parse SEQUENCE inside SET
                if !set_content.is_empty()
                    && set_content[0] == 0x30
                    && let Ok((_, seq_content)) = parse_asn1_length_and_content(&set_content[1..])
                {
                    // Parse OID
                    if !seq_content.is_empty()
                        && seq_content[0] == 0x06
                        && let Ok((attr_rest, oid_bytes)) =
                            parse_asn1_length_and_content(&seq_content[1..])
                    {
                        // Parse string value (various string types: 0x0C, 0x13, 0x14, 0x16, 0x1E)
                        if !attr_rest.is_empty() {
                            let string_tag = attr_rest[0];
                            if matches!(string_tag, 0x0C | 0x13 | 0x14 | 0x16 | 0x1E)
                                && let Ok((_, string_bytes)) =
                                    parse_asn1_length_and_content(&attr_rest[1..])
                            {
                                let value =
                                    String::from_utf8_lossy(string_bytes).trim().to_string();

                                // Check which OID this is
                                if oid_bytes == cn_oid {
                                    dn.cn = Some(value);
                                } else if oid_bytes == o_oid {
                                    dn.o = Some(value);
                                }
                            }
                        }
                    }
                }
                offset = dn_data.len() - rest.len();
            } else {
                break;
            }
        } else {
            offset += 1;
        }
    }

    dn
}

/// Extract validity dates from TBSCertificate
fn extract_validity_dates(tbs_data: &[u8]) -> Option<(String, String)> {
    let mut offset = 0;

    // Skip version, serial, signature algorithm, issuer
    // Version (optional)
    if offset < tbs_data.len()
        && tbs_data[offset] == 0xA0
        && let Ok((rest, _)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
    {
        offset = tbs_data.len() - rest.len();
    }

    // Serial number
    if offset < tbs_data.len()
        && tbs_data[offset] == 0x02
        && let Ok((rest, _)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
    {
        offset = tbs_data.len() - rest.len();
    }

    // Signature algorithm
    if offset < tbs_data.len()
        && tbs_data[offset] == 0x30
        && let Ok((rest, _)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
    {
        offset = tbs_data.len() - rest.len();
    }

    // Issuer
    if offset < tbs_data.len()
        && tbs_data[offset] == 0x30
        && let Ok((rest, _)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
    {
        offset = tbs_data.len() - rest.len();
    }

    // Now we're at validity (SEQUENCE containing two times)
    if offset < tbs_data.len()
        && tbs_data[offset] == 0x30
        && let Ok((_, validity_content)) = parse_asn1_length_and_content(&tbs_data[offset + 1..])
    {
        return parse_validity_content(validity_content);
    }

    None
}

/// Parse validity SEQUENCE to extract notBefore and notAfter
fn parse_validity_content(validity_data: &[u8]) -> Option<(String, String)> {
    let mut offset = 0;
    let mut not_before = None;
    let mut not_after = None;

    // Parse notBefore (UTCTime 0x17 or GeneralizedTime 0x18)
    if offset < validity_data.len() {
        let time_tag = validity_data[offset];
        if (time_tag == 0x17 || time_tag == 0x18)
            && let Ok((rest, time_bytes)) =
                parse_asn1_length_and_content(&validity_data[offset + 1..])
        {
            not_before = parse_asn1_time(time_bytes, time_tag == 0x17);
            offset = validity_data.len() - rest.len();
        }
    }

    // Parse notAfter
    if offset < validity_data.len() {
        let time_tag = validity_data[offset];
        if (time_tag == 0x17 || time_tag == 0x18)
            && let Ok((_, time_bytes)) = parse_asn1_length_and_content(&validity_data[offset + 1..])
        {
            not_after = parse_asn1_time(time_bytes, time_tag == 0x17);
        }
    }

    match (not_before, not_after) {
        (Some(nb), Some(na)) => Some((nb, na)),
        _ => None,
    }
}

/// Parse ASN.1 time (UTCTime or GeneralizedTime) to RFC 3339 format
fn parse_asn1_time(time_bytes: &[u8], is_utc_time: bool) -> Option<String> {
    let time_str = String::from_utf8_lossy(time_bytes);

    if is_utc_time {
        // UTCTime format: YYMMDDHHMMSSZ (13 chars)
        if time_str.len() >= 13 {
            let year = time_str[0..2].parse::<u32>().ok()?;
            // Y2K pivot: 50-99 = 1950-1999, 00-49 = 2000-2049
            let full_year = if year >= 50 { 1900 + year } else { 2000 + year };
            let month = &time_str[2..4];
            let day = &time_str[4..6];
            let hour = &time_str[6..8];
            let minute = &time_str[8..10];
            let second = &time_str[10..12];
            return Some(format!(
                "{:04}-{}-{}T{}:{}:{}Z",
                full_year, month, day, hour, minute, second
            ));
        }
    } else {
        // GeneralizedTime format: YYYYMMDDHHMMSSZ (15 chars)
        if time_str.len() >= 15 {
            let year = &time_str[0..4];
            let month = &time_str[4..6];
            let day = &time_str[6..8];
            let hour = &time_str[8..10];
            let minute = &time_str[10..12];
            let second = &time_str[12..14];
            return Some(format!(
                "{}-{}-{}T{}:{}:{}Z",
                year, month, day, hour, minute, second
            ));
        }
    }

    None
}

/// Find counter-signature in PKCS#7 structure
fn find_counter_signature(_data: &[u8]) -> Option<String> {
    // Counter-signatures are in authenticated attributes with OID 1.2.840.113549.1.9.6
    // This is a simplified implementation - full parsing would require more complex logic
    // For now, return None as counter-signature detection requires deeper PKCS#7 parsing
    None
}

/// Calculate SHA-1 hash and return as hex string
fn calculate_sha1_hex(data: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(data);
    let result = hasher.finalize();
    bytes_to_hex(&result)
}

/// Convert bytes to hex string
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<Vec<_>>()
        .join("")
}

/// Check if certificate is expired based on notAfter date
fn is_certificate_expired(not_after: &str) -> bool {
    // Parse RFC 3339 date and compare with current time
    use chrono::{DateTime, Utc};
    if let Ok(expiry_date) = DateTime::parse_from_rfc3339(not_after) {
        let now = Utc::now();
        return now > expiry_date;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_win_certificate_header() {
        // Create a minimal WIN_CERTIFICATE structure
        let mut data = vec![];
        data.extend_from_slice(&16u32.to_le_bytes()); // dwLength = 16
        data.extend_from_slice(&0x0200u16.to_le_bytes()); // wRevision = 2.0
        data.extend_from_slice(&0x0002u16.to_le_bytes()); // wCertificateType = PKCS#7
        data.extend_from_slice(&[0x30, 0x06, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06]); // dummy cert data

        let result = parse_win_certificate(&data);
        assert!(result.is_ok());

        let (_, cert) = result.unwrap();
        assert_eq!(cert.dw_length, 16);
        assert_eq!(cert.w_revision, cert_revision::WIN_CERT_REVISION_2_0);
        assert_eq!(
            cert.w_certificate_type,
            cert_type::WIN_CERT_TYPE_PKCS_SIGNED_DATA
        );
        assert_eq!(cert.certificate_data.len(), 8);
    }

    #[test]
    fn test_bytes_to_hex() {
        let bytes = vec![0x01, 0x23, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF];
        let hex = bytes_to_hex(&bytes);
        assert_eq!(hex, "0123456789ABCDEF");
    }

    #[test]
    fn test_parse_asn1_sequence_short_form() {
        // SEQUENCE with short-form length
        let data = vec![0x30, 0x05, 0x01, 0x02, 0x03, 0x04, 0x05];
        let result = parse_asn1_sequence(&data);
        assert!(result.is_ok());

        let (_, content) = result.unwrap();
        assert_eq!(content, &[0x01, 0x02, 0x03, 0x04, 0x05]);
    }

    #[test]
    fn test_parse_asn1_time_utc() {
        // UTCTime: 231201120000Z = Dec 1, 2023 12:00:00 UTC
        let time_bytes = b"231201120000Z";
        let result = parse_asn1_time(time_bytes, true);
        assert_eq!(result, Some("2023-12-01T12:00:00Z".to_string()));

        // UTCTime: 991231235959Z = Dec 31, 1999 23:59:59 UTC
        let time_bytes = b"991231235959Z";
        let result = parse_asn1_time(time_bytes, true);
        assert_eq!(result, Some("1999-12-31T23:59:59Z".to_string()));
    }

    #[test]
    fn test_parse_asn1_time_generalized() {
        // GeneralizedTime: 20231201120000Z = Dec 1, 2023 12:00:00 UTC
        let time_bytes = b"20231201120000Z";
        let result = parse_asn1_time(time_bytes, false);
        assert_eq!(result, Some("2023-12-01T12:00:00Z".to_string()));
    }

    #[test]
    fn test_is_certificate_expired() {
        // Test with expired date
        let expired_date = "2020-01-01T00:00:00Z";
        assert!(is_certificate_expired(expired_date));

        // Test with future date
        let future_date = "2030-12-31T23:59:59Z";
        assert!(!is_certificate_expired(future_date));
    }
}
