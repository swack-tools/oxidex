//! PCAP/PCAP-NG packet capture parser for network forensics
//!
//! Implements metadata extraction from PCAP (libpcap) and PCAP-NG (next generation)
//! packet capture files. These formats are used by network monitoring tools like
//! tcpdump, Wireshark, and other packet analyzers.
//!
//! # Format Structure
//!
//! ## PCAP Format
//!
//! ```text
//! Global Header (24 bytes):
//!   - magic_number: u32 (0xa1b2c3d4 BE or 0xd4c3b2a1 LE)
//!   - version_major: u16
//!   - version_minor: u16
//!   - thiszone: i32 (GMT offset, usually 0)
//!   - sigfigs: u32 (timestamp accuracy)
//!   - snaplen: u32 (max packet length)
//!   - network: u32 (link-layer type)
//!
//! Packet Header (16 bytes each):
//!   - ts_sec: u32 (timestamp seconds)
//!   - ts_usec: u32 (timestamp microseconds/nanoseconds)
//!   - incl_len: u32 (captured length)
//!   - orig_len: u32 (original length)
//!   - packet_data: [u8; incl_len]
//! ```
//!
//! ## PCAP-NG Format
//!
//! PCAP-NG uses a more complex block-based structure:
//! - Section Header Block (SHB): File metadata
//! - Interface Description Block (IDB): Interface information
//! - Enhanced Packet Block (EPB): Packet data with metadata
//! - Name Resolution Block (NRB): DNS/hostname resolution
//! - Interface Statistics Block (ISB): Capture statistics
//!
//! # References
//!
//! - PCAP Format: https://wiki.wireshark.org/Development/LibpcapFileFormat
//! - PCAP-NG Spec: https://github.com/pcapng/pcapng
//! - Link Layer Types: https://www.tcpdump.org/linktypes.html

#![allow(dead_code)]

use crate::core::{FileFormat, FileReader, FormatParser, MetadataMap, TagValue};
use crate::error::{ExifToolError, Result};
use crate::io::EndianReader;

/// PCAP magic numbers
const PCAP_MAGIC_BE: u32 = 0xa1b2c3d4; // Big-endian microsecond
const PCAP_MAGIC_LE: u32 = 0xd4c3b2a1; // Little-endian microsecond
const PCAP_MAGIC_NS_BE: u32 = 0xa1b23c4d; // Big-endian nanosecond
const PCAP_MAGIC_NS_LE: u32 = 0x4d3cb2a1; // Little-endian nanosecond

/// PCAP-NG Section Header Block magic
const PCAPNG_MAGIC: u32 = 0x0a0d0d0a;

/// PCAP global header size
const PCAP_GLOBAL_HEADER_SIZE: usize = 24;

/// PCAP packet header size
const PCAP_PACKET_HEADER_SIZE: usize = 16;

/// PCAP-NG block types
const PCAPNG_BLOCK_SHB: u32 = 0x0a0d0d0a; // Section Header Block
const PCAPNG_BLOCK_IDB: u32 = 0x00000001; // Interface Description Block
const PCAPNG_BLOCK_EPB: u32 = 0x00000006; // Enhanced Packet Block
const PCAPNG_BLOCK_SPB: u32 = 0x00000003; // Simple Packet Block
const PCAPNG_BLOCK_NRB: u32 = 0x00000004; // Name Resolution Block
const PCAPNG_BLOCK_ISB: u32 = 0x00000005; // Interface Statistics Block

/// PCAP-NG IDB option codes
const PCAPNG_OPT_IF_NAME: u16 = 2;
const PCAPNG_OPT_IF_DESCRIPTION: u16 = 3;
const PCAPNG_OPT_IF_IPV4ADDR: u16 = 4;
const PCAPNG_OPT_IF_IPV6ADDR: u16 = 5;
const PCAPNG_OPT_IF_MACADDR: u16 = 6;
const PCAPNG_OPT_IF_SPEED: u16 = 8;
const PCAPNG_OPT_IF_TSRESOL: u16 = 9;
const PCAPNG_OPT_IF_FILTER: u16 = 11;
const PCAPNG_OPT_IF_OS: u16 = 12;

/// Maximum reasonable snaplen to prevent parsing extremely large files
const MAX_REASONABLE_SNAPLEN: u32 = 262144; // 256KB

/// Link layer type constants (common types from tcpdump/libpcap)
/// See: https://www.tcpdump.org/linktypes.html
const LINKTYPE_NULL: u32 = 0; // BSD loopback
const LINKTYPE_ETHERNET: u32 = 1; // Ethernet (10Mb, 100Mb, 1000Mb, and up)
const LINKTYPE_IEEE802_5: u32 = 6; // IEEE 802.5 Token Ring
const LINKTYPE_PPP: u32 = 9; // PPP
const LINKTYPE_FDDI: u32 = 10; // FDDI
const LINKTYPE_RAW: u32 = 12; // Raw IP
const LINKTYPE_PPP_HDLC: u32 = 50; // PPP in HDLC-like framing
const LINKTYPE_PPP_ETHER: u32 = 51; // PPPoE
const LINKTYPE_IEEE802_11: u32 = 105; // IEEE 802.11 wireless
const LINKTYPE_LINUX_SLL: u32 = 113; // Linux cooked-mode capture
const LINKTYPE_PRISM: u32 = 119; // Prism monitor mode
const LINKTYPE_IEEE802_11_RADIOTAP: u32 = 127; // Radiotap link-layer info + 802.11
const LINKTYPE_IEEE802_11_AVS: u32 = 163; // AVS monitor mode
const LINKTYPE_LINUX_SLL2: u32 = 276; // Linux cooked-mode capture v2

/// PCAP/PCAP-NG packet capture parser
pub struct PCAPParser;

impl PCAPParser {
    /// Verifies PCAP or PCAP-NG signature by checking magic bytes
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the packet capture file
    ///
    /// # Returns
    ///
    /// * `Ok(true)` - Valid PCAP or PCAP-NG signature detected
    /// * `Ok(false)` - Invalid or missing signature
    /// * `Err` - I/O error reading the file
    pub fn verify_signature(reader: &dyn FileReader) -> Result<bool> {
        // Check file is large enough for magic number
        if reader.size() < 4 {
            return Ok(false);
        }

        // Check magic number (bytes 0-3) - match raw bytes directly
        let magic_bytes = reader.read(0, 4)?;

        Ok(matches!(
            magic_bytes,
            [0x0a, 0x0d, 0x0d, 0x0a]  // PCAP-NG
                | [0xa1, 0xb2, 0xc3, 0xd4]  // PCAP big-endian
                | [0xd4, 0xc3, 0xb2, 0xa1]  // PCAP little-endian
                | [0xa1, 0xb2, 0x3c, 0x4d]  // PCAP nanosecond big-endian
                | [0x4d, 0x3c, 0xb2, 0xa1] // PCAP nanosecond little-endian
        ))
    }

    /// Detects the specific format (PCAP or PCAP-NG) and endianness
    ///
    /// # Returns
    ///
    /// (format_name, is_little_endian, is_nanosecond)
    fn detect_format(reader: &dyn FileReader) -> Result<(&'static str, bool, bool)> {
        let magic_bytes = reader.read(0, 4)?;

        // Check raw bytes directly
        // Big-endian PCAP: [0xa1, 0xb2, 0xc3, 0xd4]
        // Little-endian PCAP: [0xd4, 0xc3, 0xb2, 0xa1]
        // Big-endian PCAP nanosecond: [0xa1, 0xb2, 0x3c, 0x4d]
        // Little-endian PCAP nanosecond: [0x4d, 0x3c, 0xb2, 0xa1]
        // PCAP-NG: [0x0a, 0x0d, 0x0d, 0x0a]

        match magic_bytes {
            [0x0a, 0x0d, 0x0d, 0x0a] => {
                let byte_order_magic = reader.read(8, 4)?;
                Ok((
                    "PCAP-NG",
                    Self::pcapng_is_little_endian(byte_order_magic)?,
                    false,
                ))
            }
            [0xa1, 0xb2, 0xc3, 0xd4] => Ok(("PCAP", false, false)), // Big-endian
            [0xd4, 0xc3, 0xb2, 0xa1] => Ok(("PCAP", true, false)),  // Little-endian
            [0xa1, 0xb2, 0x3c, 0x4d] => Ok(("PCAP", false, true)),  // Big-endian nanosecond
            [0x4d, 0x3c, 0xb2, 0xa1] => Ok(("PCAP", true, true)),   // Little-endian nanosecond
            _ => Err(ExifToolError::parse_error("Invalid PCAP/PCAP-NG signature")),
        }
    }

    fn pcapng_is_little_endian(byte_order_magic: &[u8]) -> Result<bool> {
        match byte_order_magic {
            [0x4d, 0x3c, 0x2b, 0x1a] => Ok(true),
            [0x1a, 0x2b, 0x3c, 0x4d] => Ok(false),
            _ => Err(ExifToolError::parse_error(
                "Invalid PCAP-NG byte-order magic",
            )),
        }
    }

    /// Maps link layer type number to human-readable name
    fn link_type_name(link_type: u32) -> &'static str {
        match link_type {
            LINKTYPE_NULL => "BSD Loopback",
            LINKTYPE_ETHERNET => "Ethernet",
            LINKTYPE_IEEE802_5 => "Token Ring",
            LINKTYPE_PPP => "PPP",
            LINKTYPE_FDDI => "FDDI",
            LINKTYPE_RAW => "Raw IP",
            LINKTYPE_PPP_HDLC => "PPP-HDLC",
            LINKTYPE_PPP_ETHER => "PPPoE",
            LINKTYPE_IEEE802_11 => "IEEE 802.11 (WiFi)",
            LINKTYPE_LINUX_SLL => "Linux Cooked Capture",
            LINKTYPE_PRISM => "Prism Monitor",
            LINKTYPE_IEEE802_11_RADIOTAP => "802.11 + Radiotap",
            LINKTYPE_IEEE802_11_AVS => "802.11 + AVS",
            LINKTYPE_LINUX_SLL2 => "Linux Cooked Capture v2",
            _ => "Unknown",
        }
    }

    /// Parses classic PCAP format
    fn parse_pcap(
        reader: &dyn FileReader,
        little_endian: bool,
        is_nanosecond: bool,
    ) -> Result<MetadataMap> {
        if reader.size() < PCAP_GLOBAL_HEADER_SIZE as u64 {
            return Err(ExifToolError::parse_error(
                "File too small for PCAP global header",
            ));
        }

        let mut metadata = MetadataMap::new();

        // Read global header
        let header = reader.read(0, PCAP_GLOBAL_HEADER_SIZE)?;

        // Create EndianReader based on detected byte order
        let r = if little_endian {
            EndianReader::little_endian(header)
        } else {
            EndianReader::big_endian(header)
        };

        // Parse version
        let version_major = r.u16_at(4).unwrap();
        let version_minor = r.u16_at(6).unwrap();
        metadata.insert(
            "PCAP:Version".to_string(),
            TagValue::String(format!("{}.{}", version_major, version_minor)),
        );
        metadata.insert(
            "PCAP:VersionMajor".to_string(),
            TagValue::String(version_major.to_string()),
        );
        metadata.insert(
            "PCAP:VersionMinor".to_string(),
            TagValue::String(version_minor.to_string()),
        );

        // Parse timezone offset (GMT to local correction)
        let thiszone = r.i32_at(8).unwrap();
        metadata.insert(
            "PCAP:TimeZone".to_string(),
            TagValue::String(format!("{} seconds", thiszone)),
        );

        // Parse timestamp accuracy (always 0 in practice)
        let sigfigs = r.u32_at(12).unwrap();
        metadata.insert(
            "PCAP:TimestampAccuracy".to_string(),
            TagValue::String(sigfigs.to_string()),
        );

        // Parse snaplen (maximum packet length)
        let snaplen = r.u32_at(16).unwrap();
        metadata.insert(
            "PCAP:SnapLen".to_string(),
            TagValue::String(format!("{} bytes", snaplen)),
        );

        // Parse link layer type
        let network = r.u32_at(20).unwrap();
        metadata.insert(
            "PCAP:LinkType".to_string(),
            TagValue::String(network.to_string()),
        );
        metadata.insert(
            "PCAP:LinkTypeName".to_string(),
            TagValue::String(Self::link_type_name(network).to_string()),
        );

        // Byte order
        metadata.insert(
            "PCAP:ByteOrder".to_string(),
            TagValue::String(
                if little_endian {
                    "Little-endian"
                } else {
                    "Big-endian"
                }
                .to_string(),
            ),
        );

        // Timestamp precision
        metadata.insert(
            "PCAP:TimestampPrecision".to_string(),
            TagValue::String(
                if is_nanosecond {
                    "Nanoseconds"
                } else {
                    "Microseconds"
                }
                .to_string(),
            ),
        );

        // Count packets and find timestamps
        let (packet_count, first_ts, last_ts) =
            Self::count_packets_and_timestamps(reader, little_endian, snaplen);

        metadata.insert(
            "PCAP:PacketCount".to_string(),
            TagValue::String(packet_count.to_string()),
        );

        if let Some(first) = first_ts {
            metadata.insert(
                "PCAP:FirstPacketTime".to_string(),
                TagValue::String(Self::format_timestamp(first)),
            );
        }

        if let Some(last) = last_ts {
            metadata.insert(
                "PCAP:LastPacketTime".to_string(),
                TagValue::String(Self::format_timestamp(last)),
            );

            if let Some(first) = first_ts
                && last >= first
            {
                let duration = last - first;
                metadata.insert(
                    "PCAP:Duration".to_string(),
                    TagValue::String(Self::format_duration(duration)),
                );
            }
        }

        Ok(metadata)
    }

    /// Counts packets and extracts first/last timestamps
    ///
    /// # Returns
    ///
    /// (packet_count, first_timestamp_seconds, last_timestamp_seconds)
    fn count_packets_and_timestamps(
        reader: &dyn FileReader,
        little_endian: bool,
        snaplen: u32,
    ) -> (u64, Option<u32>, Option<u32>) {
        let mut offset = PCAP_GLOBAL_HEADER_SIZE as u64;
        let file_size = reader.size();
        let mut count = 0u64;
        let mut first_ts: Option<u32> = None;
        let mut last_ts: Option<u32> = None;

        // Safety check: if snaplen is unreasonably large, cap it
        let safe_snaplen = snaplen.min(MAX_REASONABLE_SNAPLEN);

        while offset + PCAP_PACKET_HEADER_SIZE as u64 <= file_size {
            // Try to read packet header
            let Ok(pkt_header) = reader.read(offset, PCAP_PACKET_HEADER_SIZE) else {
                break;
            };

            // Create EndianReader for packet header
            let r = if little_endian {
                EndianReader::little_endian(pkt_header)
            } else {
                EndianReader::big_endian(pkt_header)
            };

            // Extract timestamp seconds
            let ts_sec = r.u32_at(0).unwrap();

            // Extract packet length
            let incl_len = r.u32_at(8).unwrap();

            // Validate packet length
            if incl_len > safe_snaplen || incl_len > 1_000_000 {
                // Invalid packet length, stop parsing
                break;
            }

            // Update timestamps
            if first_ts.is_none() {
                first_ts = Some(ts_sec);
            }
            last_ts = Some(ts_sec);

            count += 1;

            // Move to next packet
            offset += PCAP_PACKET_HEADER_SIZE as u64 + incl_len as u64;
        }

        (count, first_ts, last_ts)
    }

    /// Parses PCAP-NG format
    fn parse_pcapng(reader: &dyn FileReader, mut little_endian: bool) -> Result<MetadataMap> {
        let mut metadata = MetadataMap::new();
        let mut offset = 0u64;
        let file_size = reader.size();

        let mut section_count = 0u64;
        let mut interface_count = 0u64;
        let mut packet_count = 0u64;
        let mut hardware: Option<String> = None;
        let mut os: Option<String> = None;
        let mut application: Option<String> = None;
        let mut first_packet_ts: Option<u64> = None;
        let mut last_packet_ts: Option<u64> = None;

        // Parse blocks
        while offset + 12 <= file_size {
            // Read block header (block type + block length)
            let Ok(block_header) = reader.read(offset, 8) else {
                break;
            };

            if block_header.starts_with(&[0x0a, 0x0d, 0x0d, 0x0a]) {
                little_endian = Self::pcapng_is_little_endian(reader.read(offset + 8, 4)?)?;
            }

            // Create EndianReader for block header
            let r = if little_endian {
                EndianReader::little_endian(block_header)
            } else {
                EndianReader::big_endian(block_header)
            };

            let block_type = r.u32_at(0).unwrap();
            let block_length = r.u32_at(4).unwrap();

            // Validate block length
            if !(12..=1_000_000).contains(&block_length) {
                break;
            }

            match block_type {
                PCAPNG_BLOCK_SHB => {
                    section_count += 1;
                    // Try to parse Section Header Block options
                    if block_length > 28
                        && offset + block_length as u64 <= file_size
                        && let Ok(shb_data) = reader.read(offset, block_length as usize)
                    {
                        // Parse options (starts at offset 24 in SHB)
                        let opts = Self::parse_pcapng_options(&shb_data[24..], little_endian);
                        if let Some(hw) = opts.get("hardware") {
                            hardware = Some(hw.clone());
                        }
                        if let Some(o) = opts.get("os") {
                            os = Some(o.clone());
                        }
                        if let Some(app) = opts.get("userappl") {
                            application = Some(app.clone());
                        }
                    }
                }
                PCAPNG_BLOCK_IDB => {
                    interface_count += 1;
                    if block_length >= 20
                        && offset + block_length as u64 <= file_size
                        && let Ok(idb_data) = reader.read(offset, block_length as usize)
                    {
                        // IDB header: link_type (2) + reserved (2) + snaplen (4) = 8 bytes after block header
                        if idb_data.len() >= 20 {
                            let r = if little_endian {
                                EndianReader::little_endian(idb_data)
                            } else {
                                EndianReader::big_endian(idb_data)
                            };

                            let link_type = r.u16_at(8).unwrap();
                            let snaplen = r.u32_at(12).unwrap();

                            // First interface's link type
                            if interface_count == 1 {
                                metadata.insert(
                                    "PCAPNG:LinkType".to_string(),
                                    TagValue::String(link_type.to_string()),
                                );
                                metadata.insert(
                                    "PCAPNG:LinkTypeName".to_string(),
                                    TagValue::String(
                                        Self::link_type_name(link_type as u32).to_string(),
                                    ),
                                );
                                metadata.insert(
                                    "PCAPNG:SnapLen".to_string(),
                                    TagValue::String(format!("{} bytes", snaplen)),
                                );
                            }

                            if idb_data.len() > 20 {
                                let options_end = idb_data.len() - 4;
                                let idb_opts = Self::parse_pcapng_idb_options(
                                    &idb_data[16..options_end],
                                    little_endian,
                                );
                                for (key, value) in idb_opts {
                                    if !metadata.contains_key(&key) {
                                        metadata.insert(key, value);
                                    }
                                }
                            }
                        }
                    }
                }
                PCAPNG_BLOCK_EPB | PCAPNG_BLOCK_SPB => {
                    packet_count += 1;

                    // Parse EPB timestamp for first and last packet
                    if block_type == PCAPNG_BLOCK_EPB
                        && block_length >= 32
                        && let Ok(epb_data) = reader.read(offset, 32.min(block_length as usize))
                        && epb_data.len() >= 20
                    {
                        let r = if little_endian {
                            EndianReader::little_endian(epb_data)
                        } else {
                            EndianReader::big_endian(epb_data)
                        };

                        // Timestamp is at offset 8 (after block header)
                        // Stored as two 32-bit values: high (upper 32 bits) and low (lower 32 bits)
                        let ts_high = r.u32_at(8).unwrap();
                        let ts_low = r.u32_at(12).unwrap();
                        let timestamp_us = ((ts_high as u64) << 32) | (ts_low as u64);

                        if first_packet_ts.is_none() {
                            first_packet_ts = Some(timestamp_us);
                        }
                        last_packet_ts = Some(timestamp_us);
                    }
                }
                PCAPNG_BLOCK_NRB => {
                    metadata.insert(
                        "PCAPNG:HasNameResolution".to_string(),
                        TagValue::String("Yes".to_string()),
                    );
                    // Parse NRB records for forensic value
                    if let Ok(nrb_data) = reader.read(offset, block_length as usize) {
                        let record_count = Self::count_nrb_records(&nrb_data[8..], little_endian);
                        metadata.insert(
                            "PCAPNG:NameResolutionRecords".to_string(),
                            TagValue::String(record_count.to_string()),
                        );
                    }
                }
                PCAPNG_BLOCK_ISB => {
                    // Interface Statistics Block
                    if let Ok(isb_data) = reader.read(offset, block_length as usize)
                        && isb_data.len() >= 24
                    {
                        let r = if little_endian {
                            EndianReader::little_endian(isb_data)
                        } else {
                            EndianReader::big_endian(isb_data)
                        };

                        let isb_starttime_high = r.u32_at(12).unwrap();
                        let isb_starttime_low = r.u32_at(16).unwrap();
                        if isb_starttime_high != 0 || isb_starttime_low != 0 {
                            let ts =
                                ((isb_starttime_high as u64) << 32) | (isb_starttime_low as u64);
                            metadata.insert(
                                "PCAPNG:CaptureStartTime".to_string(),
                                TagValue::String(Self::format_pcapng_timestamp(ts)),
                            );
                        }
                    }
                }
                _ => {}
            }

            offset += block_length as u64;
        }

        metadata.insert(
            "PCAPNG:SectionCount".to_string(),
            TagValue::String(section_count.to_string()),
        );
        metadata.insert(
            "PCAPNG:InterfaceCount".to_string(),
            TagValue::String(interface_count.to_string()),
        );
        metadata.insert(
            "PCAPNG:PacketCount".to_string(),
            TagValue::String(packet_count.to_string()),
        );

        if let Some(hw) = hardware {
            metadata.insert("PCAPNG:Hardware".to_string(), TagValue::String(hw));
        }
        if let Some(o) = os {
            metadata.insert("PCAPNG:OS".to_string(), TagValue::String(o));
        }
        if let Some(app) = application {
            metadata.insert("PCAPNG:Application".to_string(), TagValue::String(app));
        }

        metadata.insert(
            "PCAPNG:ByteOrder".to_string(),
            TagValue::String(
                if little_endian {
                    "Little-endian"
                } else {
                    "Big-endian"
                }
                .to_string(),
            ),
        );

        // Add timestamp information
        if let Some(first_ts) = first_packet_ts {
            metadata.insert(
                "PCAPNG:FirstPacketTime".to_string(),
                TagValue::String(Self::format_pcapng_timestamp(first_ts)),
            );
        }

        if let Some(last_ts) = last_packet_ts {
            metadata.insert(
                "PCAPNG:LastPacketTime".to_string(),
                TagValue::String(Self::format_pcapng_timestamp(last_ts)),
            );

            if let Some(first_ts) = first_packet_ts
                && last_ts >= first_ts
            {
                let duration_us = last_ts - first_ts;
                let duration_secs = duration_us / 1_000_000;
                metadata.insert(
                    "PCAPNG:Duration".to_string(),
                    TagValue::String(Self::format_duration(duration_secs as u32)),
                );
            }
        }

        Ok(metadata)
    }

    /// Parses PCAP-NG options in a block
    ///
    /// Options are TLV (Type-Length-Value) encoded
    fn parse_pcapng_options(
        data: &[u8],
        little_endian: bool,
    ) -> std::collections::HashMap<String, String> {
        let mut options = std::collections::HashMap::new();
        let mut offset = 0;

        let r = if little_endian {
            EndianReader::little_endian(data)
        } else {
            EndianReader::big_endian(data)
        };

        while offset + 4 <= data.len() {
            let Some(opt_code) = r.u16_at(offset) else {
                break;
            };
            let Some(opt_length) = r.u16_at(offset + 2) else {
                break;
            };
            let opt_length = opt_length as usize;

            // opt_code 0 = end of options
            if opt_code == 0 {
                break;
            }

            offset += 4;

            // Validate length
            if offset + opt_length > data.len() {
                break;
            }

            // Extract option value
            if opt_length > 0 {
                let value_bytes = &data[offset..offset + opt_length];
                let value = String::from_utf8_lossy(value_bytes)
                    .trim_matches('\0')
                    .to_string();

                // Map common option codes
                let key = match opt_code {
                    2 => "hardware",
                    3 => "os",
                    4 => "userappl",
                    _ => continue,
                };

                if !value.is_empty() {
                    options.insert(key.to_string(), value);
                }
            }

            // Options are padded to 4-byte boundaries
            offset += opt_length.div_ceil(4) * 4;
        }

        options
    }

    /// Parses PCAP-NG Interface Description Block options
    fn parse_pcapng_idb_options(data: &[u8], little_endian: bool) -> Vec<(String, TagValue)> {
        let mut options = Vec::new();
        let mut offset = 0;

        let r = if little_endian {
            EndianReader::little_endian(data)
        } else {
            EndianReader::big_endian(data)
        };

        while offset + 4 <= data.len() {
            let Some(opt_code) = r.u16_at(offset) else {
                break;
            };
            let Some(opt_length) = r.u16_at(offset + 2) else {
                break;
            };
            let opt_length = opt_length as usize;

            if opt_code == 0 {
                break;
            }

            offset += 4;

            if offset + opt_length > data.len() {
                break;
            }

            if opt_length > 0 {
                let value_bytes = &data[offset..offset + opt_length];

                match opt_code {
                    PCAPNG_OPT_IF_NAME => {
                        let value = String::from_utf8_lossy(value_bytes)
                            .trim_matches('\0')
                            .to_string();
                        if !value.is_empty() {
                            options.push((
                                "PCAPNG:InterfaceName".to_string(),
                                TagValue::String(value),
                            ));
                        }
                    }
                    PCAPNG_OPT_IF_DESCRIPTION => {
                        let value = String::from_utf8_lossy(value_bytes)
                            .trim_matches('\0')
                            .to_string();
                        if !value.is_empty() {
                            options.push((
                                "PCAPNG:InterfaceDescription".to_string(),
                                TagValue::String(value),
                            ));
                        }
                    }
                    PCAPNG_OPT_IF_MACADDR if opt_length >= 6 => {
                        let mac = format!(
                            "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                            value_bytes[0],
                            value_bytes[1],
                            value_bytes[2],
                            value_bytes[3],
                            value_bytes[4],
                            value_bytes[5]
                        );
                        options.push(("PCAPNG:InterfaceMAC".to_string(), TagValue::String(mac)));
                    }
                    PCAPNG_OPT_IF_SPEED if opt_length >= 8 => {
                        let speed = r.u64_at(offset).unwrap();
                        let speed_str = if speed >= 1_000_000_000 {
                            format!("{} Gbps", speed / 1_000_000_000)
                        } else if speed >= 1_000_000 {
                            format!("{} Mbps", speed / 1_000_000)
                        } else {
                            format!("{} bps", speed)
                        };
                        options.push((
                            "PCAPNG:InterfaceSpeed".to_string(),
                            TagValue::String(speed_str),
                        ));
                    }
                    PCAPNG_OPT_IF_TSRESOL if opt_length >= 1 => {
                        let resol = value_bytes[0];
                        let resol_str = if resol & 0x80 != 0 {
                            let power = resol & 0x7F;
                            format!("2^-{} seconds", power)
                        } else {
                            let power = resol;
                            format!("10^-{} seconds", power)
                        };
                        options.push((
                            "PCAPNG:TimestampResolution".to_string(),
                            TagValue::String(resol_str),
                        ));
                    }
                    PCAPNG_OPT_IF_FILTER => {
                        // First byte is filter type (0=string), rest is filter
                        if opt_length > 1 {
                            let filter = String::from_utf8_lossy(&value_bytes[1..])
                                .trim_matches('\0')
                                .to_string();
                            if !filter.is_empty() {
                                options.push((
                                    "PCAPNG:CaptureFilter".to_string(),
                                    TagValue::String(filter),
                                ));
                            }
                        }
                    }
                    PCAPNG_OPT_IF_OS => {
                        let value = String::from_utf8_lossy(value_bytes)
                            .trim_matches('\0')
                            .to_string();
                        if !value.is_empty() {
                            options
                                .push(("PCAPNG:InterfaceOS".to_string(), TagValue::String(value)));
                        }
                    }
                    _ => {}
                }
            }

            offset += opt_length.div_ceil(4) * 4;
        }

        options
    }

    /// Formats Unix timestamp to ISO 8601
    fn format_timestamp(seconds: u32) -> String {
        // Simple conversion without external dependencies
        let total_seconds = seconds as u64;
        let days = total_seconds / 86400;
        let remaining = total_seconds % 86400;
        let hours = remaining / 3600;
        let minutes = (remaining % 3600) / 60;
        let secs = remaining % 60;

        // Calculate date from days since Unix epoch (1970-01-01)
        let (year, month, day) = Self::days_to_date(days);

        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
            year, month, day, hours, minutes, secs
        )
    }

    /// Converts days since Unix epoch to (year, month, day)
    fn days_to_date(days: u64) -> (u64, u32, u32) {
        let mut year = 1970u64;
        let mut remaining_days = days;

        // Calculate year
        loop {
            let days_in_year = if Self::is_leap_year(year) { 366 } else { 365 };
            if remaining_days < days_in_year {
                break;
            }
            remaining_days -= days_in_year;
            year += 1;
        }

        // Calculate month and day
        let days_in_months = if Self::is_leap_year(year) {
            [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        } else {
            [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
        };

        let mut month = 1u32;
        let mut day_in_month = remaining_days as u32;

        for &days_in_month in days_in_months.iter() {
            if day_in_month < days_in_month {
                break;
            }
            day_in_month -= days_in_month;
            month += 1;
        }

        (year, month, day_in_month + 1)
    }

    /// Checks if a year is a leap year
    fn is_leap_year(year: u64) -> bool {
        (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
    }

    /// Formats duration in seconds to human-readable string
    fn format_duration(seconds: u32) -> String {
        if seconds < 60 {
            format!("{} seconds", seconds)
        } else if seconds < 3600 {
            format!("{} minutes {} seconds", seconds / 60, seconds % 60)
        } else if seconds < 86400 {
            let hours = seconds / 3600;
            let minutes = (seconds % 3600) / 60;
            let secs = seconds % 60;
            format!("{} hours {} minutes {} seconds", hours, minutes, secs)
        } else {
            let days = seconds / 86400;
            let hours = (seconds % 86400) / 3600;
            format!("{} days {} hours", days, hours)
        }
    }

    /// Formats PCAP-NG timestamp (microseconds since epoch) to ISO 8601
    fn format_pcapng_timestamp(timestamp_us: u64) -> String {
        let seconds = (timestamp_us / 1_000_000) as u32;
        Self::format_timestamp(seconds)
    }

    /// Counts name resolution records in NRB data
    fn count_nrb_records(data: &[u8], little_endian: bool) -> u32 {
        let mut count = 0u32;
        let mut offset = 0;

        let r = if little_endian {
            EndianReader::little_endian(data)
        } else {
            EndianReader::big_endian(data)
        };

        while offset + 4 <= data.len() {
            let Some(record_type) = r.u16_at(offset) else {
                break;
            };
            let Some(record_length) = r.u16_at(offset + 2) else {
                break;
            };
            let record_length = record_length as usize;

            if record_type == 0 {
                break;
            }

            if record_type == 1 || record_type == 2 {
                // IPv4 or IPv6 record
                count += 1;
            }

            offset += 4 + record_length.div_ceil(4) * 4;
        }

        count
    }
}

impl FormatParser for PCAPParser {
    /// Parses metadata from a PCAP or PCAP-NG file
    ///
    /// # Arguments
    ///
    /// * `reader` - File reader providing access to the packet capture file
    ///
    /// # Returns
    ///
    /// * `Ok(MetadataMap)` - Extracted metadata including capture statistics
    /// * `Err(ExifToolError)` - Invalid signature or parse error
    fn parse(&self, reader: &dyn FileReader) -> Result<MetadataMap> {
        // Verify this is a valid PCAP or PCAP-NG file
        if !Self::verify_signature(reader)? {
            return Err(ExifToolError::parse_error("Invalid PCAP/PCAP-NG signature"));
        }

        let mut metadata = MetadataMap::new();

        // Detect format and endianness
        let (format_name, little_endian, is_nanosecond) = Self::detect_format(reader)?;

        // Basic file information
        metadata.insert(
            "FileType".to_string(),
            TagValue::String(format_name.to_string()),
        );
        metadata.insert(
            "FileSize".to_string(),
            TagValue::String(reader.size().to_string()),
        );

        // Parse format-specific metadata
        let format_metadata = if format_name == "PCAP-NG" {
            Self::parse_pcapng(reader, little_endian)?
        } else {
            Self::parse_pcap(reader, little_endian, is_nanosecond)?
        };

        // Merge format-specific metadata
        for (key, value) in format_metadata {
            metadata.insert(key, value);
        }

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
    /// * `true` - Parser supports PCAP or PCAP-NG format
    /// * `false` - Parser does not support the format
    fn supports_format(&self, format: FileFormat) -> bool {
        matches!(format, FileFormat::PCAP | FileFormat::PCAPNG)
    }
}

/// Parses metadata from PCAP/PCAP-NG packet capture files.
///
/// This is the public API function for parsing packet captures.
///
/// # Arguments
///
/// * `reader` - File reader providing access to the packet capture file
///
/// # Returns
///
/// * `Ok(MetadataMap)` - Successfully extracted metadata
/// * `Err(String)` - Parse error message
///
/// # Examples
///
/// ```no_run
/// use oxidex::parsers::specialized::pcap::parse_pcap_metadata;
/// use oxidex::io::MMapReader;
/// use std::path::Path;
///
/// # fn example() -> Result<(), String> {
/// let reader = MMapReader::new(Path::new("capture.pcap"))
///     .map_err(|e| e.to_string())?;
/// let metadata = parse_pcap_metadata(&reader)?;
/// println!("PCAP metadata: {:?}", metadata);
/// # Ok(())
/// # }
/// ```
pub fn parse_pcap_metadata(reader: &dyn FileReader) -> std::result::Result<MetadataMap, String> {
    let parser = PCAPParser;
    parser.parse(reader).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TestReader;

    /// Creates a minimal valid PCAP header (little-endian)
    fn create_pcap_header_le() -> Vec<u8> {
        let mut data = Vec::new();

        // Magic number (little-endian) - raw bytes that indicate little-endian
        data.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]);
        // Version major (2)
        data.extend_from_slice(&2u16.to_le_bytes());
        // Version minor (4)
        data.extend_from_slice(&4u16.to_le_bytes());
        // Timezone offset (0)
        data.extend_from_slice(&0i32.to_le_bytes());
        // Timestamp accuracy (0)
        data.extend_from_slice(&0u32.to_le_bytes());
        // Snaplen (65535)
        data.extend_from_slice(&65535u32.to_le_bytes());
        // Link type (1 = Ethernet)
        data.extend_from_slice(&1u32.to_le_bytes());

        data
    }

    /// Creates a minimal valid PCAP header (big-endian)
    fn create_pcap_header_be() -> Vec<u8> {
        let mut data = Vec::new();

        // Magic number (big-endian) - raw bytes that indicate big-endian
        data.extend_from_slice(&[0xa1, 0xb2, 0xc3, 0xd4]);
        // Version major (2)
        data.extend_from_slice(&2u16.to_be_bytes());
        // Version minor (4)
        data.extend_from_slice(&4u16.to_be_bytes());
        // Timezone offset (0)
        data.extend_from_slice(&0i32.to_be_bytes());
        // Timestamp accuracy (0)
        data.extend_from_slice(&0u32.to_be_bytes());
        // Snaplen (65535)
        data.extend_from_slice(&65535u32.to_be_bytes());
        // Link type (1 = Ethernet)
        data.extend_from_slice(&1u32.to_be_bytes());

        data
    }

    /// Creates a minimal valid PCAP-NG Section Header Block
    fn create_pcapng_shb() -> Vec<u8> {
        let mut data = Vec::new();

        // Block type (Section Header Block)
        data.extend_from_slice(&PCAPNG_MAGIC.to_le_bytes());
        // Block total length (28 bytes minimum)
        data.extend_from_slice(&28u32.to_le_bytes());
        // Byte order magic (0x1A2B3C4D = little-endian)
        data.extend_from_slice(&0x1A2B3C4Du32.to_le_bytes());
        // Major version (1)
        data.extend_from_slice(&1u16.to_le_bytes());
        // Minor version (0)
        data.extend_from_slice(&0u16.to_le_bytes());
        // Section length (-1 = not specified)
        data.extend_from_slice(&(-1i64).to_le_bytes());
        // Block total length (repeated)
        data.extend_from_slice(&28u32.to_le_bytes());

        data
    }

    #[test]
    fn test_verify_signature_pcap_le() {
        let data = create_pcap_header_le();
        let reader = TestReader::new(data);
        assert!(PCAPParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_pcap_be() {
        let data = create_pcap_header_be();
        let reader = TestReader::new(data);
        assert!(PCAPParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_pcapng() {
        let data = create_pcapng_shb();
        let reader = TestReader::new(data);
        assert!(PCAPParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_invalid() {
        let data = vec![0x00, 0x11, 0x22, 0x33];
        let reader = TestReader::new(data);
        assert!(!PCAPParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_verify_signature_too_small() {
        let data = vec![0xd4, 0xc3];
        let reader = TestReader::new(data);
        assert!(!PCAPParser::verify_signature(&reader).unwrap());
    }

    #[test]
    fn test_detect_format_pcap_le() {
        let data = create_pcap_header_le();
        let reader = TestReader::new(data);
        let (format, le, ns) = PCAPParser::detect_format(&reader).unwrap();
        assert_eq!(format, "PCAP");
        assert!(le);
        assert!(!ns);
    }

    #[test]
    fn test_detect_format_pcap_be() {
        let data = create_pcap_header_be();
        let reader = TestReader::new(data);
        let (format, le, ns) = PCAPParser::detect_format(&reader).unwrap();
        assert_eq!(format, "PCAP");
        assert!(!le);
        assert!(!ns);
    }

    #[test]
    fn test_detect_format_pcapng() {
        let data = create_pcapng_shb();
        let reader = TestReader::new(data);
        let (format, le, _ns) = PCAPParser::detect_format(&reader).unwrap();
        assert_eq!(format, "PCAP-NG");
        assert!(le);
    }

    #[test]
    fn test_link_type_name() {
        assert_eq!(PCAPParser::link_type_name(LINKTYPE_NULL), "BSD Loopback");
        assert_eq!(PCAPParser::link_type_name(LINKTYPE_ETHERNET), "Ethernet");
        assert_eq!(
            PCAPParser::link_type_name(LINKTYPE_IEEE802_11),
            "IEEE 802.11 (WiFi)"
        );
        assert_eq!(
            PCAPParser::link_type_name(LINKTYPE_LINUX_SLL),
            "Linux Cooked Capture"
        );
        assert_eq!(PCAPParser::link_type_name(9999), "Unknown");
    }

    #[test]
    fn test_parse_pcap_le() {
        let data = create_pcap_header_le();
        let reader = TestReader::new(data);
        let parser = PCAPParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("PCAP".to_string()))
        );
        assert_eq!(
            metadata.get("PCAP:Version"),
            Some(&TagValue::String("2.4".to_string()))
        );
        assert_eq!(
            metadata.get("PCAP:LinkTypeName"),
            Some(&TagValue::String("Ethernet".to_string()))
        );
        assert_eq!(
            metadata.get("PCAP:ByteOrder"),
            Some(&TagValue::String("Little-endian".to_string()))
        );
        assert_eq!(
            metadata.get("PCAP:PacketCount"),
            Some(&TagValue::String("0".to_string()))
        );
    }

    #[test]
    fn test_parse_pcapng() {
        let data = create_pcapng_shb();
        let reader = TestReader::new(data);
        let parser = PCAPParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("FileType"),
            Some(&TagValue::String("PCAP-NG".to_string()))
        );
        assert_eq!(
            metadata.get("PCAPNG:SectionCount"),
            Some(&TagValue::String("1".to_string()))
        );
        assert_eq!(
            metadata.get("PCAPNG:ByteOrder"),
            Some(&TagValue::String("Little-endian".to_string()))
        );
    }

    #[test]
    fn test_is_leap_year() {
        assert!(PCAPParser::is_leap_year(2000));
        assert!(PCAPParser::is_leap_year(2020));
        assert!(!PCAPParser::is_leap_year(1900));
        assert!(!PCAPParser::is_leap_year(2019));
    }

    #[test]
    fn test_format_timestamp() {
        // Unix epoch
        assert_eq!(PCAPParser::format_timestamp(0), "1970-01-01T00:00:00Z");

        // Known timestamp: 2020-01-01 00:00:00
        assert_eq!(
            PCAPParser::format_timestamp(1577836800),
            "2020-01-01T00:00:00Z"
        );
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(PCAPParser::format_duration(30), "30 seconds");
        assert_eq!(PCAPParser::format_duration(90), "1 minutes 30 seconds");
        assert_eq!(
            PCAPParser::format_duration(3661),
            "1 hours 1 minutes 1 seconds"
        );
        assert_eq!(PCAPParser::format_duration(90000), "1 days 1 hours");
    }

    #[test]
    fn test_supports_format() {
        let parser = PCAPParser;
        assert!(parser.supports_format(FileFormat::PCAP));
        assert!(parser.supports_format(FileFormat::PCAPNG));
        assert!(!parser.supports_format(FileFormat::JPEG));
        assert!(!parser.supports_format(FileFormat::PNG));
    }

    #[test]
    fn test_parse_invalid_signature() {
        let data = vec![0x00; 100];
        let reader = TestReader::new(data);
        let parser = PCAPParser;
        let result = parser.parse(&reader);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_pcapng_idb_options() {
        // IDB with if_name option (code 2) = "eth0"
        let mut data = create_pcapng_shb();

        // Add Interface Description Block
        data.extend_from_slice(&PCAPNG_BLOCK_IDB.to_le_bytes()); // Block type
        data.extend_from_slice(&32u32.to_le_bytes()); // Block length
        data.extend_from_slice(&1u16.to_le_bytes()); // Link type (Ethernet)
        data.extend_from_slice(&0u16.to_le_bytes()); // Reserved
        data.extend_from_slice(&65535u32.to_le_bytes()); // SnapLen
        // Option: if_name (code 2), length 4, "eth0"
        data.extend_from_slice(&2u16.to_le_bytes()); // opt_code
        data.extend_from_slice(&4u16.to_le_bytes()); // opt_length
        data.extend_from_slice(b"eth0");
        // End of options
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&32u32.to_le_bytes()); // Block length (repeated)

        let reader = TestReader::new(data);
        let parser = PCAPParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("PCAPNG:InterfaceName"),
            Some(&TagValue::String("eth0".to_string()))
        );
    }

    #[test]
    fn test_parse_pcapng_epb_timestamps() {
        let mut data = create_pcapng_shb();

        // Add IDB (required before EPB)
        data.extend_from_slice(&PCAPNG_BLOCK_IDB.to_le_bytes());
        data.extend_from_slice(&20u32.to_le_bytes()); // Block length
        data.extend_from_slice(&1u16.to_le_bytes()); // Link type
        data.extend_from_slice(&0u16.to_le_bytes()); // Reserved
        data.extend_from_slice(&65535u32.to_le_bytes()); // SnapLen
        data.extend_from_slice(&20u32.to_le_bytes()); // Block length (repeated)

        // Add EPB with known timestamp
        data.extend_from_slice(&PCAPNG_BLOCK_EPB.to_le_bytes());
        data.extend_from_slice(&36u32.to_le_bytes()); // Block length
        data.extend_from_slice(&0u32.to_le_bytes()); // Interface ID
        data.extend_from_slice(&0x0005E0FCu32.to_le_bytes()); // Timestamp high (Jan 1, 2020)
        data.extend_from_slice(&0x0u32.to_le_bytes()); // Timestamp low
        data.extend_from_slice(&4u32.to_le_bytes()); // Captured length
        data.extend_from_slice(&4u32.to_le_bytes()); // Original length
        data.extend_from_slice(&[0u8; 4]); // Packet data
        data.extend_from_slice(&36u32.to_le_bytes()); // Block length (repeated)

        let reader = TestReader::new(data);
        let parser = PCAPParser;
        let metadata = parser.parse(&reader).unwrap();

        assert!(metadata.contains_key("PCAPNG:FirstPacketTime"));
        assert_eq!(
            metadata.get("PCAPNG:PacketCount"),
            Some(&TagValue::String("1".to_string()))
        );
    }

    #[test]
    fn test_parse_pcapng_nrb() {
        let mut data = create_pcapng_shb();

        // Add Name Resolution Block
        let nrb_type: u32 = 0x00000004;
        data.extend_from_slice(&nrb_type.to_le_bytes());
        data.extend_from_slice(&28u32.to_le_bytes()); // Block length
        // NRB record: type 1 (IPv4), length, IP, name
        data.extend_from_slice(&1u16.to_le_bytes()); // Record type (IPv4)
        data.extend_from_slice(&12u16.to_le_bytes()); // Record length
        data.extend_from_slice(&[192, 168, 1, 1]); // IP address
        data.extend_from_slice(b"router\0\0"); // Name (padded to 4 bytes)
        // End of records
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&28u32.to_le_bytes()); // Block length (repeated)

        let reader = TestReader::new(data);
        let parser = PCAPParser;
        let metadata = parser.parse(&reader).unwrap();

        assert_eq!(
            metadata.get("PCAPNG:HasNameResolution"),
            Some(&TagValue::String("Yes".to_string()))
        );
    }
}
