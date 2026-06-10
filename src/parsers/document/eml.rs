//! EML (Email Message) format parser for digital forensics
//!
//! This parser extracts forensically-critical metadata from EML files,
//! which are RFC 5322 format emails - plain text with headers and body
//! separated by a blank line.
//!
//! # Format Structure
//!
//! EML files are plain text with headers followed by a blank line and body:
//! ```text
//! From: sender@example.com
//! To: recipient@example.com
//! Subject: Test Email
//! Date: Mon, 1 Jan 2024 12:00:00 +0000
//!
//! Email body content...
//! ```
//!
//! # Forensic Metadata
//!
//! - Core headers: From, To, Cc, Bcc, Subject, Date, Message-ID
//! - Routing headers: Received (multiple), Return-Path, X-Originating-IP
//! - Authentication: Authentication-Results, DKIM-Signature, Received-SPF
//! - Client identification: User-Agent, X-Mailer, X-MS-Exchange-*, X-Google-*
//! - Content structure: Content-Type, MIME-Version, attachments
//! - Threading: In-Reply-To, References, Thread-Index, Thread-Topic

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::parsers::detection::TEXT_FORMAT_PROBE_SIZE;
use chrono::{DateTime, Utc};

/// EML parser for extracting forensic metadata from email files
pub struct EmlParser;

impl EmlParser {
    fn read_header_text(reader: &dyn FileReader) -> Result<String> {
        let size = (reader.size() as usize).min(TEXT_FORMAT_PROBE_SIZE);
        let content = reader.read(0, size)?;
        let header_end = content
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .or_else(|| content.windows(2).position(|window| window == b"\n\n"))
            .unwrap_or(content.len());

        Ok(String::from_utf8_lossy(&content[..header_end]).into_owned())
    }

    /// Verifies EML format by checking for common email headers
    ///
    /// # Arguments
    ///
    /// * `reader` - FileReader implementation for accessing file data
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - File appears to be a valid EML file
    /// * `Ok(false)` - File does not appear to be EML format
    /// * `Err(ExifToolError)` - I/O error reading file
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        if reader.size() < 10 {
            return Ok(false);
        }

        // Match the bounded text detection window so long header blocks remain parseable.
        let text = Self::read_header_text(reader)?;

        // Check for common email headers (case-insensitive)
        let text_lower = text.to_lowercase();
        let has_from = text_lower.contains("from:");
        let has_to = text_lower.contains("to:");
        let has_date = text_lower.contains("date:");
        let has_subject = text_lower.contains("subject:");

        // Must have at least 2 of these common headers
        let header_count = [has_from, has_to, has_date, has_subject]
            .iter()
            .filter(|&&x| x)
            .count();

        Ok(header_count >= 2)
    }

    /// Parse email headers and extract forensic metadata
    ///
    /// # Arguments
    ///
    /// * `reader` - FileReader implementation for accessing file data
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Extracted email metadata
    /// * `Err(ExifToolError)` - Parse error
    pub fn parse_email_content(reader: &dyn FileReader) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let headers_text = Self::read_header_text(reader)?;
        Self::parse_headers(&headers_text, &mut metadata)?;

        Ok(metadata)
    }

    /// Parse email headers and populate metadata map
    fn parse_headers(headers_text: &str, metadata: &mut MetadataMap) -> Result<()> {
        let mut current_header: Option<(String, String)> = None;
        let mut received_headers = Vec::new();

        // Normalize line endings and split
        let normalized = headers_text.replace("\r\n", "\n");
        for line in normalized.lines() {
            // Handle header continuation (lines starting with whitespace per RFC 5322)
            if line.starts_with(' ') || line.starts_with('\t') {
                if let Some((_, ref mut value)) = current_header {
                    value.push(' ');
                    value.push_str(line.trim());
                }
                continue;
            }

            // Save the previous header if any
            if let Some((key, value)) = current_header.take() {
                Self::store_header(&key, &value, &mut received_headers, metadata);
            }

            // Parse new header
            if let Some((key, value)) = line.split_once(':') {
                let key = key.trim().to_string();
                let value = value.trim().to_string();
                current_header = Some((key, value));
            }
        }

        // Store the last header
        if let Some((key, value)) = current_header {
            Self::store_header(&key, &value, &mut received_headers, metadata);
        }

        // Store Received headers as an array (in order)
        if !received_headers.is_empty() {
            metadata.insert(
                "EML:Received".to_string(),
                TagValue::Array(received_headers.into_iter().map(TagValue::String).collect()),
            );
        }

        Ok(())
    }

    /// Store a parsed header in the metadata map
    fn store_header(
        key: &str,
        value: &str,
        received_headers: &mut Vec<String>,
        metadata: &mut MetadataMap,
    ) {
        let key_lower = key.to_lowercase();

        match key_lower.as_str() {
            // Core headers
            "from" => {
                metadata.insert("EML:From".to_string(), TagValue::String(value.to_string()));
            }
            "to" => {
                metadata.insert("EML:To".to_string(), TagValue::String(value.to_string()));
            }
            "cc" => {
                metadata.insert("EML:Cc".to_string(), TagValue::String(value.to_string()));
            }
            "bcc" => {
                metadata.insert("EML:Bcc".to_string(), TagValue::String(value.to_string()));
            }
            "subject" => {
                metadata.insert(
                    "EML:Subject".to_string(),
                    TagValue::String(value.to_string()),
                );
            }
            "date" => {
                // Store original date string
                metadata.insert("EML:Date".to_string(), TagValue::String(value.to_string()));

                // Try to parse to ISO 8601
                if let Ok(dt) = Self::parse_email_date(value) {
                    metadata.insert("EML:DateTime".to_string(), TagValue::DateTime(dt));
                }
            }
            "message-id" => {
                metadata.insert(
                    "EML:MessageID".to_string(),
                    TagValue::String(value.to_string()),
                );
            }

            // Threading headers
            "in-reply-to" => {
                metadata.insert(
                    "EML:InReplyTo".to_string(),
                    TagValue::String(value.to_string()),
                );
            }
            "references" => {
                metadata.insert(
                    "EML:References".to_string(),
                    TagValue::String(value.to_string()),
                );
            }
            "thread-index" => {
                metadata.insert(
                    "EML:ThreadIndex".to_string(),
                    TagValue::String(value.to_string()),
                );
            }
            "thread-topic" => {
                metadata.insert(
                    "EML:ThreadTopic".to_string(),
                    TagValue::String(value.to_string()),
                );
            }

            // Routing headers (critical for forensics)
            "received" => {
                // Store multiple Received headers in order
                received_headers.push(value.to_string());
            }
            "return-path" => {
                metadata.insert(
                    "EML:ReturnPath".to_string(),
                    TagValue::String(value.to_string()),
                );
            }
            "x-originating-ip" => {
                metadata.insert(
                    "EML:OriginatingIP".to_string(),
                    TagValue::String(value.to_string()),
                );
            }

            // Authentication headers
            "authentication-results" => {
                metadata.insert(
                    "EML:AuthenticationResults".to_string(),
                    TagValue::String(value.to_string()),
                );
            }
            "dkim-signature" => {
                metadata.insert(
                    "EML:DKIMSignature".to_string(),
                    TagValue::String(value.to_string()),
                );
            }
            "received-spf" => {
                metadata.insert(
                    "EML:ReceivedSPF".to_string(),
                    TagValue::String(value.to_string()),
                );
            }

            // Client identification
            "user-agent" => {
                metadata.insert(
                    "EML:UserAgent".to_string(),
                    TagValue::String(value.to_string()),
                );
            }
            "x-mailer" => {
                metadata.insert(
                    "EML:XMailer".to_string(),
                    TagValue::String(value.to_string()),
                );
            }

            // Content structure
            "content-type" => {
                metadata.insert(
                    "EML:ContentType".to_string(),
                    TagValue::String(value.to_string()),
                );

                // Extract attachment information if present
                if let Some(filename) = Self::extract_filename(value) {
                    metadata.insert(
                        "EML:AttachmentFilename".to_string(),
                        TagValue::String(filename),
                    );
                }
            }
            "mime-version" => {
                metadata.insert(
                    "EML:MIMEVersion".to_string(),
                    TagValue::String(value.to_string()),
                );
            }
            "content-transfer-encoding" => {
                metadata.insert(
                    "EML:ContentTransferEncoding".to_string(),
                    TagValue::String(value.to_string()),
                );
            }
            "content-disposition" => {
                metadata.insert(
                    "EML:ContentDisposition".to_string(),
                    TagValue::String(value.to_string()),
                );

                // Extract attachment filename
                if let Some(filename) = Self::extract_filename(value) {
                    metadata.insert(
                        "EML:AttachmentFilename".to_string(),
                        TagValue::String(filename),
                    );
                }
            }

            // Microsoft Exchange headers
            key if key.starts_with("x-ms-exchange-") => {
                let tag_name = format!("EML:{}", key);
                metadata.insert(tag_name, TagValue::String(value.to_string()));
            }

            // Google headers
            key if key.starts_with("x-google-") => {
                let tag_name = format!("EML:{}", key);
                metadata.insert(tag_name, TagValue::String(value.to_string()));
            }

            _ => {
                // Store other headers with X- prefix
                if key_lower.starts_with("x-") {
                    let tag_name = format!("EML:{}", key);
                    metadata.insert(tag_name, TagValue::String(value.to_string()));
                }
            }
        }
    }

    /// Parse RFC 5322 email date to UTC DateTime
    fn parse_email_date(date_str: &str) -> Result<DateTime<Utc>> {
        // RFC 5322 date format: "Mon, 1 Jan 2024 12:00:00 +0000"
        // Try multiple common formats
        let formats = [
            "%a, %d %b %Y %H:%M:%S %z",
            "%d %b %Y %H:%M:%S %z",
            "%a, %d %b %Y %H:%M:%S %Z",
            "%Y-%m-%d %H:%M:%S %z",
        ];

        for format in &formats {
            if let Ok(dt) = DateTime::parse_from_str(date_str, format) {
                return Ok(dt.with_timezone(&Utc));
            }
        }

        Err(ExifToolError::parse_error(format!(
            "Could not parse email date: {}",
            date_str
        )))
    }

    /// Extract filename from Content-Type or Content-Disposition header
    fn extract_filename(header_value: &str) -> Option<String> {
        // Look for filename= or name= parameter
        for param in header_value.split(';') {
            let param = param.trim();
            if param.starts_with("filename=") || param.starts_with("name=") {
                let filename = param.split_once('=')?.1.trim();
                // Remove quotes if present
                let filename = filename.trim_matches('"').trim_matches('\'');
                return Some(filename.to_string());
            }
        }
        None
    }
}

impl FormatParser for EmlParser {
    /// Parses an EML file and extracts forensic metadata
    ///
    /// # Arguments
    ///
    /// * `reader` - FileReader implementation for accessing file data
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Successfully extracted metadata
    /// * `Err(ExifToolError)` - Invalid format or parse error
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid EML format"));
        }

        let mut metadata = MetadataMap::new();
        metadata.insert("FileType".to_string(), TagValue::String("EML".to_string()));
        metadata.insert(
            "FileSize".to_string(),
            TagValue::Integer(reader.size() as i64),
        );

        // Parse email content and merge with basic metadata
        let email_metadata = Self::parse_email_content(reader)?;
        for (key, value) in email_metadata {
            metadata.insert(key, value);
        }

        Ok(metadata)
    }

    /// Indicates whether this parser supports the given file format
    ///
    /// # Arguments
    ///
    /// * `format` - FileFormat to check
    ///
    /// # Returns
    ///
    /// * `true` if format is EML
    /// * `false` otherwise
    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::EML)
    }
}

/// Parses metadata from EML files.
///
/// This is a convenience function that creates an EmlParser and invokes it.
///
/// # Arguments
///
/// * `reader` - FileReader implementation for accessing file data
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
pub fn parse_eml_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = EmlParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock FileReader implementation for testing
    struct MockFileReader {
        data: Vec<u8>,
    }

    impl MockFileReader {
        fn new(data: Vec<u8>) -> Self {
            Self { data }
        }
    }

    impl FileReader for MockFileReader {
        fn read(&self, offset: u64, size: usize) -> std::io::Result<&[u8]> {
            let start = offset as usize;
            let end = (start + size).min(self.data.len());
            if start >= self.data.len() {
                return Ok(&[]);
            }
            Ok(&self.data[start..end])
        }

        fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    #[test]
    fn test_verify_signature_valid() {
        let eml_data =
            b"From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: Test\r\n\r\nBody";
        let reader = MockFileReader::new(eml_data.to_vec());

        let result = EmlParser::verify_signature(&reader);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_verify_signature_invalid() {
        let invalid_data = b"Not an email file";
        let reader = MockFileReader::new(invalid_data.to_vec());

        let result = EmlParser::verify_signature(&reader);
        assert!(result.is_ok());
        assert!(!result.unwrap());
    }

    #[test]
    fn test_parse_basic_headers() {
        let eml_data = b"From: Alice <alice@example.com>\r\n\
To: Bob <bob@example.com>\r\n\
Subject: Test Email\r\n\
Date: Mon, 1 Jan 2024 12:00:00 +0000\r\n\
Message-ID: <abc123@example.com>\r\n\
\r\n\
This is the email body.";

        let reader = MockFileReader::new(eml_data.to_vec());
        let parser = EmlParser;
        let result = parser.parse(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(
            metadata.get("EML:From").and_then(|v| v.as_string()),
            Some("Alice <alice@example.com>")
        );
        assert_eq!(
            metadata.get("EML:To").and_then(|v| v.as_string()),
            Some("Bob <bob@example.com>")
        );
        assert_eq!(
            metadata.get("EML:Subject").and_then(|v| v.as_string()),
            Some("Test Email")
        );
        assert_eq!(
            metadata.get("EML:MessageID").and_then(|v| v.as_string()),
            Some("<abc123@example.com>")
        );
    }

    #[test]
    fn test_parse_routing_headers() {
        let eml_data = b"From: sender@example.com\r\n\
To: recipient@example.com\r\n\
Received: from mail1.example.com by mail2.example.com\r\n\
Received: from [192.168.1.100] by mail1.example.com\r\n\
Return-Path: <sender@example.com>\r\n\
X-Originating-IP: 192.168.1.100\r\n\
\r\n\
Body";

        let reader = MockFileReader::new(eml_data.to_vec());
        let parser = EmlParser;
        let result = parser.parse(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(
            metadata.get("EML:ReturnPath").and_then(|v| v.as_string()),
            Some("<sender@example.com>")
        );
        assert_eq!(
            metadata
                .get("EML:OriginatingIP")
                .and_then(|v| v.as_string()),
            Some("192.168.1.100")
        );

        // Check Received headers array
        if let Some(TagValue::Array(received)) = metadata.get("EML:Received") {
            assert_eq!(received.len(), 2);
        } else {
            panic!("Expected Received headers array");
        }
    }

    #[test]
    fn test_parse_authentication_headers() {
        let eml_data = b"From: sender@example.com\r\n\
To: recipient@example.com\r\n\
Authentication-Results: example.com; spf=pass\r\n\
DKIM-Signature: v=1; a=rsa-sha256; d=example.com\r\n\
Received-SPF: pass\r\n\
\r\n\
Body";

        let reader = MockFileReader::new(eml_data.to_vec());
        let parser = EmlParser;
        let result = parser.parse(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(
            metadata
                .get("EML:AuthenticationResults")
                .and_then(|v| v.as_string()),
            Some("example.com; spf=pass")
        );
        assert!(metadata.get("EML:DKIMSignature").is_some());
        assert_eq!(
            metadata.get("EML:ReceivedSPF").and_then(|v| v.as_string()),
            Some("pass")
        );
    }

    #[test]
    fn test_parse_client_headers() {
        let eml_data = b"From: sender@example.com\r\n\
To: recipient@example.com\r\n\
User-Agent: Mozilla Thunderbird\r\n\
X-Mailer: Microsoft Outlook 16.0\r\n\
\r\n\
Body";

        let reader = MockFileReader::new(eml_data.to_vec());
        let parser = EmlParser;
        let result = parser.parse(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(
            metadata.get("EML:UserAgent").and_then(|v| v.as_string()),
            Some("Mozilla Thunderbird")
        );
        assert_eq!(
            metadata.get("EML:XMailer").and_then(|v| v.as_string()),
            Some("Microsoft Outlook 16.0")
        );
    }

    #[test]
    fn test_parse_content_headers() {
        let eml_data = b"From: sender@example.com\r\n\
To: recipient@example.com\r\n\
Content-Type: text/plain; charset=utf-8\r\n\
MIME-Version: 1.0\r\n\
Content-Transfer-Encoding: quoted-printable\r\n\
\r\n\
Body";

        let reader = MockFileReader::new(eml_data.to_vec());
        let parser = EmlParser;
        let result = parser.parse(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(
            metadata.get("EML:ContentType").and_then(|v| v.as_string()),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(
            metadata.get("EML:MIMEVersion").and_then(|v| v.as_string()),
            Some("1.0")
        );
        assert_eq!(
            metadata
                .get("EML:ContentTransferEncoding")
                .and_then(|v| v.as_string()),
            Some("quoted-printable")
        );
    }

    #[test]
    fn test_parse_threading_headers() {
        let eml_data = b"From: sender@example.com\r\n\
To: recipient@example.com\r\n\
In-Reply-To: <previous@example.com>\r\n\
References: <ref1@example.com> <ref2@example.com>\r\n\
Thread-Index: AQHabc123\r\n\
Thread-Topic: Discussion Topic\r\n\
\r\n\
Body";

        let reader = MockFileReader::new(eml_data.to_vec());
        let parser = EmlParser;
        let result = parser.parse(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(
            metadata.get("EML:InReplyTo").and_then(|v| v.as_string()),
            Some("<previous@example.com>")
        );
        assert_eq!(
            metadata.get("EML:References").and_then(|v| v.as_string()),
            Some("<ref1@example.com> <ref2@example.com>")
        );
        assert_eq!(
            metadata.get("EML:ThreadIndex").and_then(|v| v.as_string()),
            Some("AQHabc123")
        );
        assert_eq!(
            metadata.get("EML:ThreadTopic").and_then(|v| v.as_string()),
            Some("Discussion Topic")
        );
    }

    #[test]
    fn test_extract_filename() {
        let header1 = "attachment; filename=\"document.pdf\"";
        assert_eq!(
            EmlParser::extract_filename(header1),
            Some("document.pdf".to_string())
        );

        let header2 = "text/plain; name=readme.txt";
        assert_eq!(
            EmlParser::extract_filename(header2),
            Some("readme.txt".to_string())
        );
    }

    #[test]
    fn test_parse_email_date() {
        let date1 = "Mon, 1 Jan 2024 12:00:00 +0000";
        let result = EmlParser::parse_email_date(date1);
        assert!(result.is_ok());

        let date2 = "1 Jan 2024 12:00:00 +0000";
        let result = EmlParser::parse_email_date(date2);
        assert!(result.is_ok());
    }

    #[test]
    fn test_multiline_header() {
        // Note: The space before "that continues" must be explicit (not stripped by Rust's \ continuation)
        let eml_data = b"From: sender@example.com\r\nTo: recipient@example.com\r\nSubject: This is a long subject\r\n that continues on the next line\r\nDate: Mon, 1 Jan 2024 12:00:00 +0000\r\n\r\nBody";

        let reader = MockFileReader::new(eml_data.to_vec());
        let parser = EmlParser;
        let result = parser.parse(&reader);

        assert!(result.is_ok());
        let metadata = result.unwrap();

        assert_eq!(
            metadata.get("EML:Subject").and_then(|v| v.as_string()),
            Some("This is a long subject that continues on the next line")
        );
    }
}
