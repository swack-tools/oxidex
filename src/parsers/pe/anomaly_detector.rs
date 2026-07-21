//! PE anomaly detection for malware analysis
//!
//! Implements various heuristics to detect suspicious characteristics
//! in PE files that may indicate packing, obfuscation, or malicious intent.

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::parsers::pe::structures::SectionHeader;

/// Known packer section names
const PACKER_SECTIONS: &[&str] = &[
    "UPX",
    "upx",
    ".upx", // UPX packer
    "ASPack",
    ".aspack",
    ".adata", // ASPack
    "PECompact",
    ".petite", // PECompact/Petite
    ".nsp0",
    ".nsp1",
    ".nsp2", // NSPack
    ".packed",
    ".pack", // Generic
    "Themida",
    ".Themida", // Themida
    ".vmprotect",
    ".vmp0",
    ".vmp1", // VMProtect
    "Enigma",
    ".enigma", // Enigma
    ".rsrc",   // Often modified by packers
];

/// Suspicious section characteristics
const SECTION_EXECUTABLE: u32 = 0x20000000;
const SECTION_WRITABLE: u32 = 0x80000000;

/// PE anomaly detector for forensic analysis
pub struct AnomalyDetector;

impl AnomalyDetector {
    /// Calculates Shannon entropy of a data slice
    ///
    /// Returns value between 0.0 (uniform) and 8.0 (maximum randomness)
    pub fn calculate_entropy(data: &[u8]) -> f64 {
        if data.is_empty() {
            return 0.0;
        }

        let mut freq = [0u64; 256];
        for &byte in data {
            freq[byte as usize] += 1;
        }

        let len = data.len() as f64;
        let mut entropy = 0.0;

        for &count in freq.iter() {
            if count > 0 {
                let p = count as f64 / len;
                entropy -= p * p.log2();
            }
        }

        entropy
    }

    /// Detects anomalies in PE sections
    pub fn detect_section_anomalies(sections: &[SectionHeader]) -> Vec<String> {
        let mut anomalies = Vec::new();

        for section in sections {
            let name = Self::section_name_string(section);

            // Check for packer signatures
            for &packer_name in PACKER_SECTIONS {
                if name.contains(packer_name) || name.starts_with(packer_name) {
                    anomalies.push(format!(
                        "Packer signature: {} in section '{}'",
                        packer_name, name
                    ));
                }
            }

            // Check for writable + executable sections (suspicious)
            if (section.characteristics & SECTION_EXECUTABLE != 0)
                && (section.characteristics & SECTION_WRITABLE != 0)
            {
                anomalies.push(format!(
                    "Section '{}' is both writable and executable",
                    name
                ));
            }

            // Check for unusual section names (non-printable)
            if name.chars().any(|c| !c.is_ascii_graphic() && c != ' ') {
                anomalies.push(format!(
                    "Section with non-printable name: {:?}",
                    section.name
                ));
            }

            // Check for zero-sized sections with raw data
            if section.virtual_size == 0 && section.size_of_raw_data > 0 {
                anomalies.push(format!(
                    "Section '{}' has zero virtual size but non-zero raw size",
                    name
                ));
            }

            // Check for very large virtual size vs raw size (potential unpacking)
            if section.virtual_size > section.size_of_raw_data * 10 && section.size_of_raw_data > 0
            {
                anomalies.push(format!(
                    "Section '{}' virtual size ({}) >> raw size ({}) - possible unpacking target",
                    name, section.virtual_size, section.size_of_raw_data
                ));
            }
        }

        anomalies
    }

    /// Converts section name bytes to string
    fn section_name_string(section: &SectionHeader) -> String {
        let null_pos = section.name.iter().position(|&b| b == 0).unwrap_or(8);
        String::from_utf8_lossy(&section.name[..null_pos]).to_string()
    }

    /// Analyzes PE for common anomalies and returns metadata
    pub fn analyze(
        reader: &dyn FileReader,
        sections: &[SectionHeader],
        entry_point: u32,
        _image_base: u64,
    ) -> MetadataMap {
        let mut metadata = MetadataMap::new();

        // Section anomalies
        let section_anomalies = Self::detect_section_anomalies(sections);
        if !section_anomalies.is_empty() {
            metadata.insert(
                "EXE:SectionAnomalies".to_string(),
                TagValue::new_array(
                    section_anomalies
                        .iter()
                        .map(|s| TagValue::String(s.clone()))
                        .collect(),
                ),
            );
            metadata.insert(
                "EXE:SuspiciousSections".to_string(),
                TagValue::String("Yes".to_string()),
            );
        }

        // Calculate section entropies
        let mut high_entropy_sections = Vec::new();
        for section in sections {
            if section.size_of_raw_data > 0 && section.size_of_raw_data < 10_000_000 {
                let offset = section.pointer_to_raw_data as u64;
                let size = section.size_of_raw_data.min(65536) as usize; // Sample first 64KB

                if let Ok(data) = reader.read(offset, size) {
                    let entropy = Self::calculate_entropy(data);
                    let name = Self::section_name_string(section);

                    if entropy > 7.0 {
                        high_entropy_sections.push(format!("{}: {:.2}", name, entropy));
                    }
                }
            }
        }

        if !high_entropy_sections.is_empty() {
            metadata.insert(
                "EXE:HighEntropySections".to_string(),
                TagValue::new_array(
                    high_entropy_sections
                        .iter()
                        .map(|s| TagValue::String(s.clone()))
                        .collect(),
                ),
            );
            metadata.insert(
                "EXE:PossiblyPacked".to_string(),
                TagValue::String("Yes".to_string()),
            );
        }

        // Check entry point location
        let mut entry_in_section = false;
        for section in sections {
            if entry_point >= section.virtual_address
                && entry_point < section.virtual_address + section.virtual_size
            {
                entry_in_section = true;
                let name = Self::section_name_string(section);

                // Entry point in non-code section is suspicious
                if !name.starts_with(".text") && !name.starts_with("CODE") {
                    metadata.insert(
                        "EXE:UnusualEntrySection".to_string(),
                        TagValue::String(format!("Entry point in '{}'", name)),
                    );
                }
                break;
            }
        }

        if !entry_in_section {
            metadata.insert(
                "EXE:EntryPointAnomaly".to_string(),
                TagValue::String("Entry point outside all sections".to_string()),
            );
        }

        metadata
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entropy_uniform() {
        // All same bytes = 0 entropy
        let data = vec![0x41u8; 1000];
        let entropy = AnomalyDetector::calculate_entropy(&data);
        assert!(entropy < 0.1);
    }

    #[test]
    fn test_entropy_random() {
        // Random-looking data should have high entropy
        let data: Vec<u8> = (0u8..=255).cycle().take(1024).collect();
        let entropy = AnomalyDetector::calculate_entropy(&data);
        assert!(entropy > 7.0);
    }

    #[test]
    fn test_entropy_text() {
        // ASCII text should have medium entropy
        let data = b"This is some normal ASCII text for testing entropy calculation.";
        let entropy = AnomalyDetector::calculate_entropy(data);
        assert!(entropy > 3.0 && entropy < 6.0);
    }

    #[test]
    fn test_detect_suspicious_section_names() {
        let sections = vec![
            create_test_section(b".text\0\0\0", 0x1000, 0x2000),
            create_test_section(b"UPX0\0\0\0\0", 0x3000, 0x4000),
        ];

        let anomalies = AnomalyDetector::detect_section_anomalies(&sections);
        assert!(anomalies.iter().any(|a| a.contains("UPX")));
    }

    fn create_test_section(name: &[u8; 8], virtual_addr: u32, raw_ptr: u32) -> SectionHeader {
        SectionHeader {
            name: *name,
            virtual_size: 0x1000,
            virtual_address: virtual_addr,
            size_of_raw_data: 0x1000,
            pointer_to_raw_data: raw_ptr,
            pointer_to_relocations: 0,
            pointer_to_line_numbers: 0,
            number_of_relocations: 0,
            number_of_line_numbers: 0,
            characteristics: 0,
        }
    }
}
