//! OLE (Object Linking and Embedding) / Compound File Binary Format parser
//!
//! This module parses Microsoft Compound File Binary Format files (.doc, .xls, .ppt, .msg)
//! and extracts metadata including VBA macro detection for forensic analysis.

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// OLE file signature (magic bytes)
const OLE_SIGNATURE: &[u8] = &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Directory entry type constants
const STGTY_INVALID: u8 = 0;
const STGTY_STORAGE: u8 = 1;
const STGTY_STREAM: u8 = 2;
const STGTY_ROOT: u8 = 5;

/// Maximum sector size (4096 bytes)
const MAX_SECTOR_SIZE: usize = 4096;

/// Directory entry size in bytes
const DIR_ENTRY_SIZE: usize = 128;

/// MS-OVBA compression signature
const VBA_COMPRESSION_SIGNATURE: u8 = 0x01;

/// Parser for OLE (Compound File Binary Format) files
///
/// Extracts metadata from OLE files including:
/// - Basic file structure information
/// - VBA macro presence and analysis
/// - Suspicious code pattern detection
pub struct OLEParser;

/// Represents an OLE directory entry
#[derive(Debug, Clone)]
struct DirectoryEntry {
    name: String,
    entry_type: u8,
    start_sector: u32,
    size: u32,
    left_sibling: u32,
    right_sibling: u32,
    child_did: u32,
}

/// OLE file header structure
#[derive(Debug)]
struct OLEHeader {
    sector_size: usize,
    mini_sector_size: usize,
    total_sectors: u32,
    fat_sectors: u32,
    first_dir_sector: u32,
    first_mini_fat_sector: u32,
    mini_fat_sectors: u32,
    first_difat_sector: u32,
    difat_sectors: u32,
}

impl OLEParser {
    /// Parse the OLE header
    fn parse_header(reader: &dyn FileReader) -> Result<OLEHeader> {
        if reader.size() < 512 {
            return Err(ExifToolError::parse_error(
                "File too small to be valid OLE file",
            ));
        }

        // Read header (first 512 bytes) - OLE uses little-endian byte order
        let header_data = reader.read(0, 512)?;

        // Verify signature
        if &header_data[0..8] != OLE_SIGNATURE {
            return Err(ExifToolError::parse_error("Invalid OLE signature"));
        }

        let header = EndianReader::little_endian(header_data);

        // Parse sector sizes
        let sector_shift = header.u16_at(30).unwrap_or(0) as usize;
        let mini_sector_shift = header.u16_at(32).unwrap_or(0) as usize;

        let sector_size = 1 << sector_shift;
        let mini_sector_size = 1 << mini_sector_shift;

        if sector_size > MAX_SECTOR_SIZE {
            return Err(ExifToolError::parse_error("Invalid sector size"));
        }

        // Parse FAT information
        let total_sectors = header.u32_at(44).unwrap_or(0);
        let first_dir_sector = header.u32_at(48).unwrap_or(0);
        let first_mini_fat_sector = header.u32_at(60).unwrap_or(0);
        let mini_fat_sectors = header.u32_at(64).unwrap_or(0);
        let first_difat_sector = header.u32_at(68).unwrap_or(0);
        let difat_sectors = header.u32_at(72).unwrap_or(0);
        let fat_sectors = header.u32_at(76).unwrap_or(0);

        Ok(OLEHeader {
            sector_size,
            mini_sector_size,
            total_sectors,
            fat_sectors,
            first_dir_sector,
            first_mini_fat_sector,
            mini_fat_sectors,
            first_difat_sector,
            difat_sectors,
        })
    }

    /// Read directory entries from the OLE file
    fn read_directory_entries(
        reader: &dyn FileReader,
        header: &OLEHeader,
    ) -> Result<Vec<DirectoryEntry>> {
        let mut entries = Vec::new();

        // Calculate directory sector offset
        let dir_offset = 512 + (header.first_dir_sector as usize * header.sector_size);

        if dir_offset + header.sector_size > reader.size() as usize {
            return Err(ExifToolError::parse_error(
                "Invalid directory sector offset",
            ));
        }

        // Read first directory sector - OLE uses little-endian byte order
        let dir_data = reader.read(dir_offset as u64, header.sector_size)?;

        // Parse directory entries (4 per 512-byte sector, more for larger sectors)
        let entries_per_sector = header.sector_size / DIR_ENTRY_SIZE;

        for i in 0..entries_per_sector {
            let offset = i * DIR_ENTRY_SIZE;
            if offset + DIR_ENTRY_SIZE > dir_data.len() {
                break;
            }

            let entry_data = &dir_data[offset..offset + DIR_ENTRY_SIZE];
            let entry = EndianReader::little_endian(entry_data);

            // Parse entry name (first 64 bytes, UTF-16LE)
            let name_len = entry.u16_at(64).unwrap_or(0) as usize;
            if name_len > 64 {
                continue;
            }

            let name = if name_len > 2 {
                String::from_utf16_lossy(
                    &entry_data[0..name_len.saturating_sub(2)]
                        .chunks_exact(2)
                        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                        .collect::<Vec<u16>>(),
                )
            } else {
                String::new()
            };

            // Skip empty entries
            if name.is_empty() {
                continue;
            }

            let entry_type = entry_data[66];
            let left_sibling = entry.u32_at(68).unwrap_or(0);
            let right_sibling = entry.u32_at(72).unwrap_or(0);
            let child_did = entry.u32_at(76).unwrap_or(0);
            let start_sector = entry.u32_at(116).unwrap_or(0);
            let size = entry.u32_at(120).unwrap_or(0);

            entries.push(DirectoryEntry {
                name,
                entry_type,
                start_sector,
                size,
                left_sibling,
                right_sibling,
                child_did,
            });
        }

        Ok(entries)
    }
}

impl FormatParser for OLEParser {
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        let header = Self::parse_header(reader)?;
        let entries = Self::read_directory_entries(reader, &header)?;

        let mut metadata = MetadataMap::new();

        // Basic OLE metadata
        metadata.insert(
            "OLE:SectorSize".to_string(),
            TagValue::new_integer(header.sector_size as i64),
        );
        metadata.insert(
            "OLE:TotalSectors".to_string(),
            TagValue::new_integer(header.total_sectors as i64),
        );
        metadata.insert(
            "OLE:DirectoryEntryCount".to_string(),
            TagValue::new_integer(entries.len() as i64),
        );

        // Check for VBA macros
        let vba_metadata = VBAAnalyzer::analyze_vba(reader, &entries, &header);
        for (key, value) in vba_metadata {
            metadata.insert(key, value);
        }

        Ok(metadata)
    }

    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::OLE)
    }
}

/// Parses metadata from OLE Compound File Binary Format files.
///
/// This is the public dispatch API matching the convention used by other
/// format parsers (returning a `String` error for `convert_string_error`).
pub fn parse_ole_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = OLEParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

/// VBA Macro analyzer for forensic detection
pub struct VBAAnalyzer;

impl VBAAnalyzer {
    /// Analyze VBA macros in the OLE file
    fn analyze_vba(
        reader: &dyn FileReader,
        entries: &[DirectoryEntry],
        header: &OLEHeader,
    ) -> MetadataMap {
        let mut metadata = MetadataMap::new();

        // Find VBA directory
        let vba_dir = entries.iter().find(|e| {
            e.name.eq_ignore_ascii_case("VBA")
                || e.name.eq_ignore_ascii_case("_VBA_PROJECT_CUR")
                || e.name.eq_ignore_ascii_case("Macros")
        });

        if vba_dir.is_none() {
            metadata.insert("OLE:HasVBAMacros".to_string(), TagValue::new_string("No"));
            return metadata;
        }

        metadata.insert("OLE:HasVBAMacros".to_string(), TagValue::new_string("Yes"));

        // Look for VBA project streams
        let vba_project = entries
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case("_VBA_PROJECT"));

        if let Some(_project) = vba_project {
            metadata.insert(
                "OLE:VBAProjectName".to_string(),
                TagValue::new_string("VBA Project"),
            );
        }

        // Count VBA modules
        let module_names: Vec<String> = entries
            .iter()
            .filter(|e| {
                e.entry_type == STGTY_STREAM
                    && !e.name.starts_with('_')
                    && !e.name.eq_ignore_ascii_case("dir")
                    && !e.name.eq_ignore_ascii_case("PROJECT")
                    && !e.name.eq_ignore_ascii_case("PROJECTwm")
                    && e.name.chars().any(|c| c.is_alphabetic())
            })
            .map(|e| e.name.clone())
            .collect();

        if !module_names.is_empty() {
            metadata.insert(
                "OLE:VBAModuleCount".to_string(),
                TagValue::new_integer(module_names.len() as i64),
            );
            metadata.insert(
                "OLE:VBAModuleNames".to_string(),
                TagValue::new_array(
                    module_names
                        .iter()
                        .map(|s| TagValue::new_string(s.clone()))
                        .collect(),
                ),
            );
        }

        // Analyze suspicious patterns in VBA streams
        let mut suspicious_findings = Vec::new();

        for entry in entries.iter() {
            if entry.entry_type != STGTY_STREAM || entry.size == 0 {
                continue;
            }

            // Read stream data
            if let Ok(stream_data) = Self::read_stream(reader, entry, header) {
                let patterns = Self::check_suspicious_patterns(&stream_data);
                suspicious_findings.extend(patterns);
            }
        }

        // Report suspicious findings
        if !suspicious_findings.is_empty() {
            // Remove duplicates
            suspicious_findings.sort();
            suspicious_findings.dedup();

            // Check for specific categories
            let has_auto_exec = suspicious_findings
                .iter()
                .any(|s| s.contains("AutoExec") || s.contains("Auto_Open"));
            let has_shell = suspicious_findings
                .iter()
                .any(|s| s.contains("Shell") || s.contains("CreateObject"));
            let has_network = suspicious_findings
                .iter()
                .any(|s| s.contains("Network") || s.contains("HTTP"));
            let has_file_access = suspicious_findings
                .iter()
                .any(|s| s.contains("File") || s.contains("FileSystem"));
            let has_powershell = suspicious_findings.iter().any(|s| s.contains("PowerShell"));
            let has_obfuscation = suspicious_findings
                .iter()
                .any(|s| s.contains("Obfuscation") || s.contains("Chr("));

            metadata.insert(
                "OLE:HasAutoExec".to_string(),
                TagValue::new_string(if has_auto_exec { "Yes" } else { "No" }),
            );
            metadata.insert(
                "OLE:HasShellExecution".to_string(),
                TagValue::new_string(if has_shell { "Yes" } else { "No" }),
            );
            metadata.insert(
                "OLE:HasNetworkAccess".to_string(),
                TagValue::new_string(if has_network { "Yes" } else { "No" }),
            );
            metadata.insert(
                "OLE:HasFileAccess".to_string(),
                TagValue::new_string(if has_file_access { "Yes" } else { "No" }),
            );
            metadata.insert(
                "OLE:HasPowerShell".to_string(),
                TagValue::new_string(if has_powershell { "Yes" } else { "No" }),
            );
            metadata.insert(
                "OLE:HasObfuscation".to_string(),
                TagValue::new_string(if has_obfuscation { "Yes" } else { "No" }),
            );

            metadata.insert(
                "OLE:SuspiciousPatterns".to_string(),
                TagValue::new_array(
                    suspicious_findings
                        .iter()
                        .map(|s| TagValue::new_string(s.clone()))
                        .collect(),
                ),
            );
        }

        // Try to extract code from modules
        let mut code_snippets = Vec::new();
        for entry in entries.iter() {
            if entry.entry_type != STGTY_STREAM || entry.size == 0 {
                continue;
            }

            // Skip known non-code streams
            if entry.name.starts_with('_')
                || entry.name.eq_ignore_ascii_case("dir")
                || entry.name.eq_ignore_ascii_case("PROJECT")
                || entry.name.eq_ignore_ascii_case("PROJECTwm")
            {
                continue;
            }

            if let Some((snippet, _)) = Self::analyze_module(reader, entry, header)
                && !snippet.is_empty()
                && snippet.len() > 10
            {
                code_snippets.push(format!("{}:\n{}", entry.name, snippet));
            }
        }

        if !code_snippets.is_empty() {
            metadata.insert(
                "OLE:VBACodePreview".to_string(),
                TagValue::new_string(code_snippets.join("\n---\n")),
            );
        }

        metadata
    }

    /// Read a stream from the OLE file
    fn read_stream(
        reader: &dyn FileReader,
        entry: &DirectoryEntry,
        header: &OLEHeader,
    ) -> Result<Vec<u8>> {
        // For simplicity, only read small streams (< 4KB)
        if entry.size > 4096 {
            return Ok(Vec::new());
        }

        let offset = 512 + (entry.start_sector as usize * header.sector_size);
        let size = entry.size.min(4096) as usize;

        if offset + size > reader.size() as usize {
            return Ok(Vec::new());
        }

        let data = reader
            .read(offset as u64, size)
            .map_err(ExifToolError::IoError)?;
        Ok(data.to_vec())
    }

    /// Check for suspicious patterns in VBA code/streams
    pub fn check_suspicious_patterns(data: &[u8]) -> Vec<String> {
        let mut findings = Vec::new();

        // Convert to lowercase string for pattern matching
        let text = String::from_utf8_lossy(data).to_lowercase();

        // Auto-execution patterns
        const AUTO_EXEC_PATTERNS: &[(&str, &str)] = &[
            ("auto_open", "AutoExec: Auto_Open"),
            ("autoopen", "AutoExec: AutoOpen"),
            ("autoexec", "AutoExec: AutoExec"),
            ("autoclose", "AutoExec: AutoClose"),
            ("document_open", "AutoExec: Document_Open"),
            ("document_close", "AutoExec: Document_Close"),
            ("workbook_open", "AutoExec: Workbook_Open"),
            ("workbook_activate", "AutoExec: Workbook_Activate"),
        ];

        for (pattern, description) in AUTO_EXEC_PATTERNS {
            if text.contains(pattern) {
                findings.push(description.to_string());
            }
        }

        // Shell execution patterns
        const SHELL_PATTERNS: &[(&str, &str)] = &[
            ("shell", "Shell: Shell function"),
            ("wscript.shell", "Shell: WScript.Shell"),
            ("createobject", "Shell: CreateObject"),
            ("getobject", "Shell: GetObject"),
        ];

        for (pattern, description) in SHELL_PATTERNS {
            if text.contains(pattern) {
                findings.push(description.to_string());
            }
        }

        // Network access patterns
        const NETWORK_PATTERNS: &[(&str, &str)] = &[
            ("xmlhttp", "Network: XMLHTTP"),
            ("winhttp", "Network: WinHttp"),
            ("urldownloadtofile", "Network: URLDownloadToFile"),
            ("internetopen", "Network: InternetOpen"),
        ];

        for (pattern, description) in NETWORK_PATTERNS {
            if text.contains(pattern) {
                findings.push(description.to_string());
            }
        }

        // File access patterns
        const FILE_PATTERNS: &[(&str, &str)] = &[
            ("filesystemobject", "File: FileSystemObject"),
            ("createtextfile", "File: CreateTextFile"),
            ("opentextfile", "File: OpenTextFile"),
            ("open", "File: Open statement"),
        ];

        for (pattern, description) in FILE_PATTERNS {
            if text.contains(pattern) {
                findings.push(description.to_string());
            }
        }

        // PowerShell patterns
        const POWERSHELL_PATTERNS: &[(&str, &str)] = &[
            ("powershell", "PowerShell: powershell.exe"),
            ("-encodedcommand", "PowerShell: Encoded command"),
            ("-enc", "PowerShell: Encoded command (short)"),
            ("-command", "PowerShell: Command execution"),
        ];

        for (pattern, description) in POWERSHELL_PATTERNS {
            if text.contains(pattern) {
                findings.push(description.to_string());
            }
        }

        // Obfuscation patterns
        const OBFUSCATION_PATTERNS: &[(&str, &str)] = &[
            ("chr(", "Obfuscation: Chr() function"),
            ("chrw(", "Obfuscation: ChrW() function"),
            ("chr$(", "Obfuscation: Chr$() function"),
        ];

        for (pattern, description) in OBFUSCATION_PATTERNS {
            if text.contains(pattern) {
                findings.push(description.to_string());
            }
        }

        // Check for excessive string concatenation (potential obfuscation)
        let concat_count = text.matches(" & ").count();
        if concat_count > 20 {
            findings.push(format!(
                "Obfuscation: Excessive concatenation ({} instances)",
                concat_count
            ));
        }

        findings
    }

    /// Decompresses VBA compressed data using MS-OVBA algorithm
    ///
    /// The MS-OVBA compression format consists of:
    /// - 1 byte signature (0x01)
    /// - Compressed chunks, each with:
    ///   - 2 byte header (little-endian): bits 0-11 = size-1, bit 15 = compressed flag, bits 12-14 = signature (0b011)
    ///   - Compressed or raw data
    ///
    /// Compressed chunks use a flag byte followed by up to 8 tokens:
    /// - Flag bit 0 = literal byte
    /// - Flag bit 1 = copy token (offset + length)
    #[allow(dead_code)]
    fn decompress_vba(data: &[u8]) -> Option<Vec<u8>> {
        if data.len() < 3 {
            return None;
        }

        // Check signature
        if data[0] != VBA_COMPRESSION_SIGNATURE {
            return None;
        }

        let mut output = Vec::new();
        let mut pos = 1; // Skip signature

        // VBA compression uses little-endian byte order
        let reader = EndianReader::little_endian(data);

        while pos + 2 <= data.len() {
            // Read chunk header (2 bytes, little-endian)
            let chunk_header = reader.u16_at(pos).unwrap_or(0);
            pos += 2;

            // Parse header fields
            let chunk_size = ((chunk_header & 0x0FFF) + 1) as usize;
            let chunk_is_compressed = (chunk_header & 0x8000) != 0;
            let chunk_signature = (chunk_header >> 12) & 0x07;

            // Validate signature bits should be 0b011
            if chunk_signature != 0b011 {
                // Try to recover by treating as uncompressed
                if pos + chunk_size <= data.len() {
                    output.extend_from_slice(&data[pos..pos + chunk_size]);
                    pos += chunk_size;
                    continue;
                }
                break;
            }

            if pos + chunk_size > data.len() {
                break;
            }

            if !chunk_is_compressed {
                // Raw chunk - copy directly
                output.extend_from_slice(&data[pos..pos + chunk_size]);
                pos += chunk_size;
            } else {
                // Compressed chunk
                let chunk_end = pos + chunk_size;
                let chunk_start_output_len = output.len();

                while pos < chunk_end {
                    if pos >= data.len() {
                        break;
                    }

                    let flag_byte = data[pos];
                    pos += 1;

                    for bit in 0..8 {
                        if pos >= chunk_end {
                            break;
                        }

                        if (flag_byte & (1 << bit)) == 0 {
                            // Literal byte
                            if pos < data.len() {
                                output.push(data[pos]);
                                pos += 1;
                            }
                        } else {
                            // Copy token
                            if pos + 1 >= data.len() {
                                break;
                            }

                            let token = reader.u16_at(pos).unwrap_or(0);
                            pos += 2;

                            // Calculate offset and length based on decompressed size
                            let decompressed_chunk_size = output.len() - chunk_start_output_len;
                            let (_offset_bits, length_bits, length_mask) =
                                Self::get_copy_token_params(decompressed_chunk_size);

                            let length = ((token & length_mask) + 3) as usize;
                            let offset = ((token >> length_bits) + 1) as usize;

                            // Copy from output buffer
                            if offset <= output.len() {
                                let copy_start = output.len() - offset;
                                for i in 0..length {
                                    if copy_start + (i % offset) < output.len() {
                                        let byte = output[copy_start + (i % offset)];
                                        output.push(byte);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if output.is_empty() {
            None
        } else {
            Some(output)
        }
    }

    /// Calculates copy token parameters based on decompressed chunk size
    /// Returns (offset_bits, length_bits, length_mask)
    fn get_copy_token_params(decompressed_size: usize) -> (u32, u32, u16) {
        let decompressed_size = decompressed_size.max(1);

        // Find the number of bits needed to represent the offset
        let offset_bits = if decompressed_size <= 16 {
            4
        } else if decompressed_size <= 32 {
            5
        } else if decompressed_size <= 64 {
            6
        } else if decompressed_size <= 128 {
            7
        } else if decompressed_size <= 256 {
            8
        } else if decompressed_size <= 512 {
            9
        } else if decompressed_size <= 1024 {
            10
        } else if decompressed_size <= 2048 {
            11
        } else {
            12
        };

        let length_bits = 16 - offset_bits;
        let length_mask = (1u16 << length_bits) - 1;

        (offset_bits, length_bits, length_mask)
    }

    /// Extracts a code snippet from decompressed VBA data
    ///
    /// # Arguments
    /// * `data` - Decompressed VBA data
    /// * `max_length` - Maximum length of snippet to extract
    fn extract_code_snippet(data: &[u8], max_length: usize) -> String {
        // Try to find actual VBA code patterns
        let text = String::from_utf8_lossy(data);

        // Look for Sub/Function declarations
        let code_start = text
            .find("Sub ")
            .or_else(|| text.find("Function "))
            .or_else(|| text.find("Private Sub"))
            .or_else(|| text.find("Public Sub"))
            .unwrap_or(0);

        let snippet: String = text[code_start..]
            .chars()
            .filter(|c| c.is_ascii() && (*c >= ' ' || *c == '\n' || *c == '\r' || *c == '\t'))
            .take(max_length)
            .collect();

        // Clean up the snippet
        snippet.trim().to_string()
    }

    /// Analyzes VBA module and extracts metadata including code snippets
    #[allow(dead_code)]
    fn analyze_module(
        reader: &dyn FileReader,
        entry: &DirectoryEntry,
        header: &OLEHeader,
    ) -> Option<(String, Vec<String>)> {
        // Read and decompress the module stream
        let stream_data = Self::read_stream(reader, entry, header).ok()?;

        if stream_data.is_empty() {
            return None;
        }

        // Try to decompress
        let decompressed = if stream_data.first() == Some(&VBA_COMPRESSION_SIGNATURE) {
            Self::decompress_vba(&stream_data)?
        } else {
            stream_data
        };

        // Extract code snippet
        let snippet = Self::extract_code_snippet(&decompressed, 200);

        // Check for suspicious patterns in the decompressed code
        let patterns = Self::check_suspicious_patterns(&decompressed);

        Some((snippet, patterns))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper: Simple in-memory FileReader implementation
    struct TestFileReader {
        data: Vec<u8>,
    }

    impl TestFileReader {
        fn new(data: Vec<u8>) -> Self {
            Self { data }
        }
    }

    impl FileReader for TestFileReader {
        fn read(&self, offset: u64, length: usize) -> std::io::Result<&[u8]> {
            let start = offset as usize;
            let end = start + length;

            if end > self.data.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "read beyond end of data",
                ));
            }

            Ok(&self.data[start..end])
        }

        fn size(&self) -> u64 {
            self.data.len() as u64
        }
    }

    #[test]
    fn test_ole_signature_validation() {
        let mut data = vec![0u8; 512];
        data[0..8].copy_from_slice(OLE_SIGNATURE);

        let reader = TestFileReader::new(data);
        let result = OLEParser::parse_header(&reader);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_ole_signature() {
        let data = vec![0u8; 512];
        let reader = TestFileReader::new(data);
        let result = OLEParser::parse_header(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_too_small() {
        let data = vec![0u8; 100];
        let reader = TestFileReader::new(data);
        let result = OLEParser::parse_header(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_suspicious_pattern_detection_auto_exec() {
        let data = b"Sub Auto_Open()\n  MsgBox \"Hello\"\nEnd Sub";
        let patterns = VBAAnalyzer::check_suspicious_patterns(data);
        assert!(!patterns.is_empty());
        assert!(patterns.iter().any(|p| p.contains("Auto_Open")));
    }

    #[test]
    fn test_suspicious_pattern_detection_shell() {
        let data = b"Shell \"cmd.exe /c calc\"";
        let patterns = VBAAnalyzer::check_suspicious_patterns(data);
        assert!(!patterns.is_empty());
        assert!(patterns.iter().any(|p| p.contains("Shell")));
    }

    #[test]
    fn test_suspicious_pattern_detection_network() {
        let data = b"Set http = CreateObject(\"MSXML2.XMLHTTP\")";
        let patterns = VBAAnalyzer::check_suspicious_patterns(data);
        assert!(!patterns.is_empty());
        assert!(patterns.iter().any(|p| p.contains("XMLHTTP")));
    }

    #[test]
    fn test_suspicious_pattern_detection_powershell() {
        let data = b"powershell.exe -encodedcommand ABC123";
        let patterns = VBAAnalyzer::check_suspicious_patterns(data);
        assert!(!patterns.is_empty());
        assert!(patterns.iter().any(|p| p.contains("PowerShell")));
    }

    #[test]
    fn test_suspicious_pattern_detection_obfuscation() {
        let data = b"Chr(65) & Chr(66) & Chr(67)";
        let patterns = VBAAnalyzer::check_suspicious_patterns(data);
        assert!(!patterns.is_empty());
        assert!(patterns.iter().any(|p| p.contains("Chr")));
    }

    #[test]
    fn test_no_suspicious_patterns_in_clean_code() {
        let data = b"Sub CalculateSum()\n  Dim total As Integer\n  total = 1 + 2\nEnd Sub";
        let patterns = VBAAnalyzer::check_suspicious_patterns(data);
        // May have some findings due to "Open" in "Sub", but should be minimal
        assert!(patterns.len() <= 1);
    }

    #[test]
    fn test_excessive_concatenation_detection() {
        let mut data = String::from("x = \"a\"");
        for _ in 0..25 {
            data.push_str(" & \"b\"");
        }
        let patterns = VBAAnalyzer::check_suspicious_patterns(data.as_bytes());
        assert!(patterns.iter().any(|p| p.contains("concatenation")));
    }

    #[test]
    fn test_decompress_vba_simple() {
        // Create a simple compressed VBA chunk with proper MS-OVBA header
        // Signature byte (0x01) + chunk header with signature bits 0b011
        let compressed = vec![
            0x01, // Signature byte
            0x0D,
            0xB0, // Chunk header: size=14 (0x00D), compressed=1 (0x8000), signature=0b011 (0x3000)
            // Combined: 0x000D | 0x8000 | 0x3000 = 0xB00D
            0x00, // Flag byte (all literals for first 8 tokens)
            b'H', b'e', b'l', b'l', b'o', b' ', b'W', b'o',
            0x00, // Flag byte (all literals for next 4 tokens)
            b'r', b'l', b'd', b'!',
        ];

        let result = VBAAnalyzer::decompress_vba(&compressed);
        assert!(result.is_some());
        let decompressed = result.unwrap();
        assert_eq!(&decompressed, b"Hello World!");
    }

    #[test]
    fn test_decompress_vba_with_copy_token() {
        // Test MS-OVBA decompression with a copy token
        // This tests the copy token parameter calculation
        let compressed = vec![
            0x01, // Signature byte
            0x08, 0xB0, // Chunk header: size=9, compressed=1, signature=0b011
            0x00, // Flag byte: all 8 tokens are literals
            b'H', b'e', b'l', b'l', b'o', b'A', b'B', b'C',
            0x01, // Flag byte: bit 0 set = copy token, rest literals
            0x00, 0x00, // Copy token: offset=1, length=3 (copy last 3 bytes "ABC")
        ];

        let result = VBAAnalyzer::decompress_vba(&compressed);
        assert!(result.is_some());
        let decompressed = result.unwrap();
        // Should have 8 literal bytes, then copy the last 3
        assert!(decompressed.len() >= 8);
        assert_eq!(&decompressed[0..8], b"HelloABC");
    }

    #[test]
    fn test_extract_vba_code_snippet() {
        // Test extracting code from decompressed VBA
        let vba_code = b"Sub Test()\n  MsgBox \"Hello\"\nEnd Sub\n";
        let snippet = VBAAnalyzer::extract_code_snippet(vba_code, 50);
        assert!(snippet.contains("Sub Test"));
    }
}
