# Forensic Parser Enhancements Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enhance forensic parsers with PCAP-NG options, full ASN.1 X.509 parsing, VBA decompression, PE anomaly detection, and comprehensive test coverage.

**Architecture:** Extend existing parser infrastructure using the established `FormatParser` trait pattern. Each enhancement adds new extraction capabilities while maintaining backwards compatibility. Test coverage uses synthetic test data via `TestReader` pattern.

**Tech Stack:** Rust, nom (parsing), sha2/sha1 (fingerprints), base64 (PEM), hex (encoding)

---

## Part 1: Enhanced PCAP-NG Option Parsing

### Task 1.1: Add PCAP-NG Interface Description Block (IDB) Options

**Files:**
- Modify: `src/parsers/specialized/pcap.rs:400-430`
- Test: `src/parsers/specialized/pcap.rs` (inline tests)

**Step 1: Write the failing test**

Add to `mod tests` in `pcap.rs`:

```rust
#[test]
fn test_parse_pcapng_idb_options() {
    // IDB with if_name option (code 2) = "eth0"
    let mut data = create_pcapng_shb();

    // Add Interface Description Block
    let idb_start = data.len();
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_parse_pcapng_idb_options -p oxidex -- --nocapture`
Expected: FAIL - `PCAPNG:InterfaceName` not found

**Step 3: Write minimal implementation**

Add IDB option parsing constants after line 68:

```rust
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
```

Modify `parse_pcapng` to extract IDB options. Add after line 420:

```rust
PCAPNG_BLOCK_IDB => {
    interface_count += 1;
    // Parse IDB options
    if block_length > 20 && offset + block_length as u64 <= file_size {
        if let Ok(idb_data) = reader.read(offset, block_length as usize) {
            // IDB header: link_type (2) + reserved (2) + snaplen (4) = 8 bytes after block header
            if idb_data.len() > 16 {
                let link_type = Self::read_u16(&idb_data[8..10], little_endian);
                let snaplen = Self::read_u32(&idb_data[12..16], little_endian);

                // First interface's link type
                if interface_count == 1 {
                    metadata.insert(
                        "PCAPNG:LinkType".to_string(),
                        TagValue::String(link_type.to_string()),
                    );
                    metadata.insert(
                        "PCAPNG:LinkTypeName".to_string(),
                        TagValue::String(Self::link_type_name(link_type as u32).to_string()),
                    );
                    metadata.insert(
                        "PCAPNG:SnapLen".to_string(),
                        TagValue::String(format!("{} bytes", snaplen)),
                    );
                }

                // Parse IDB options (starts at offset 16)
                let idb_opts = Self::parse_pcapng_idb_options(&idb_data[16..], little_endian);
                for (key, value) in idb_opts {
                    if !metadata.contains_key(&key) {
                        metadata.insert(key, value);
                    }
                }
            }
        }
    }
}
```

Add new method after `parse_pcapng_options`:

```rust
/// Parses PCAP-NG Interface Description Block options
fn parse_pcapng_idb_options(
    data: &[u8],
    little_endian: bool,
) -> Vec<(String, TagValue)> {
    let mut options = Vec::new();
    let mut offset = 0;

    while offset + 4 <= data.len() {
        let opt_code = Self::read_u16(&data[offset..offset + 2], little_endian);
        let opt_length = Self::read_u16(&data[offset + 2..offset + 4], little_endian) as usize;

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
                    let value = String::from_utf8_lossy(value_bytes).trim_matches('\0').to_string();
                    if !value.is_empty() {
                        options.push(("PCAPNG:InterfaceName".to_string(), TagValue::String(value)));
                    }
                }
                PCAPNG_OPT_IF_DESCRIPTION => {
                    let value = String::from_utf8_lossy(value_bytes).trim_matches('\0').to_string();
                    if !value.is_empty() {
                        options.push(("PCAPNG:InterfaceDescription".to_string(), TagValue::String(value)));
                    }
                }
                PCAPNG_OPT_IF_MACADDR if opt_length >= 6 => {
                    let mac = format!(
                        "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                        value_bytes[0], value_bytes[1], value_bytes[2],
                        value_bytes[3], value_bytes[4], value_bytes[5]
                    );
                    options.push(("PCAPNG:InterfaceMAC".to_string(), TagValue::String(mac)));
                }
                PCAPNG_OPT_IF_SPEED if opt_length >= 8 => {
                    let speed = if little_endian {
                        u64::from_le_bytes([
                            value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3],
                            value_bytes[4], value_bytes[5], value_bytes[6], value_bytes[7],
                        ])
                    } else {
                        u64::from_be_bytes([
                            value_bytes[0], value_bytes[1], value_bytes[2], value_bytes[3],
                            value_bytes[4], value_bytes[5], value_bytes[6], value_bytes[7],
                        ])
                    };
                    let speed_str = if speed >= 1_000_000_000 {
                        format!("{} Gbps", speed / 1_000_000_000)
                    } else if speed >= 1_000_000 {
                        format!("{} Mbps", speed / 1_000_000)
                    } else {
                        format!("{} bps", speed)
                    };
                    options.push(("PCAPNG:InterfaceSpeed".to_string(), TagValue::String(speed_str)));
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
                    options.push(("PCAPNG:TimestampResolution".to_string(), TagValue::String(resol_str)));
                }
                PCAPNG_OPT_IF_FILTER => {
                    // First byte is filter type (0=string), rest is filter
                    if opt_length > 1 {
                        let filter = String::from_utf8_lossy(&value_bytes[1..]).trim_matches('\0').to_string();
                        if !filter.is_empty() {
                            options.push(("PCAPNG:CaptureFilter".to_string(), TagValue::String(filter)));
                        }
                    }
                }
                PCAPNG_OPT_IF_OS => {
                    let value = String::from_utf8_lossy(value_bytes).trim_matches('\0').to_string();
                    if !value.is_empty() {
                        options.push(("PCAPNG:InterfaceOS".to_string(), TagValue::String(value)));
                    }
                }
                _ => {}
            }
        }

        offset += opt_length.div_ceil(4) * 4;
    }

    options
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_parse_pcapng_idb_options -p oxidex -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/specialized/pcap.rs
git commit -m "feat(pcap): add PCAP-NG IDB option parsing

Extract interface metadata: name, description, MAC, speed, timestamp
resolution, capture filter, and OS from Interface Description Blocks.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 1.2: Add PCAP-NG Enhanced Packet Block (EPB) Timestamp Parsing

**Files:**
- Modify: `src/parsers/specialized/pcap.rs`
- Test: `src/parsers/specialized/pcap.rs` (inline tests)

**Step 1: Write the failing test**

```rust
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_parse_pcapng_epb_timestamps -p oxidex -- --nocapture`
Expected: FAIL

**Step 3: Write minimal implementation**

Add to `parse_pcapng` in the EPB block handling:

```rust
PCAPNG_BLOCK_EPB | PCAPNG_BLOCK_SPB => {
    packet_count += 1;

    // Parse EPB timestamp for first and last packet
    if block_type == PCAPNG_BLOCK_EPB && block_length >= 32 {
        if let Ok(epb_data) = reader.read(offset, 32.min(block_length as usize)) {
            if epb_data.len() >= 20 {
                // Timestamp is at offset 8 (after block header)
                // Stored as two 32-bit values: high (upper 32 bits) and low (lower 32 bits)
                let ts_high = Self::read_u32(&epb_data[8..12], little_endian);
                let ts_low = Self::read_u32(&epb_data[12..16], little_endian);
                let timestamp_us = ((ts_high as u64) << 32) | (ts_low as u64);

                if first_packet_ts.is_none() {
                    first_packet_ts = Some(timestamp_us);
                }
                last_packet_ts = Some(timestamp_us);
            }
        }
    }
}
```

Add variables at start of `parse_pcapng`:

```rust
let mut first_packet_ts: Option<u64> = None;
let mut last_packet_ts: Option<u64> = None;
```

Add timestamp output after the main loop:

```rust
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

    if let Some(first_ts) = first_packet_ts {
        if last_ts >= first_ts {
            let duration_us = last_ts - first_ts;
            let duration_secs = duration_us / 1_000_000;
            metadata.insert(
                "PCAPNG:Duration".to_string(),
                TagValue::String(Self::format_duration(duration_secs as u32)),
            );
        }
    }
}
```

Add helper method:

```rust
/// Formats PCAP-NG timestamp (microseconds since epoch) to ISO 8601
fn format_pcapng_timestamp(timestamp_us: u64) -> String {
    let seconds = (timestamp_us / 1_000_000) as u32;
    Self::format_timestamp(seconds)
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_parse_pcapng_epb_timestamps -p oxidex -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/specialized/pcap.rs
git commit -m "feat(pcap): extract timestamps from PCAP-NG Enhanced Packet Blocks

Parse first/last packet timestamps and calculate capture duration from EPB.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 1.3: Add PCAP-NG Name Resolution Block (NRB) Parsing

**Files:**
- Modify: `src/parsers/specialized/pcap.rs`
- Test: `src/parsers/specialized/pcap.rs` (inline tests)

**Step 1: Write the failing test**

```rust
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_parse_pcapng_nrb -p oxidex -- --nocapture`
Expected: FAIL

**Step 3: Write minimal implementation**

Add block type constant:

```rust
const PCAPNG_BLOCK_NRB: u32 = 0x00000004; // Name Resolution Block
const PCAPNG_BLOCK_ISB: u32 = 0x00000005; // Interface Statistics Block
```

Add to the match statement:

```rust
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
    if let Ok(isb_data) = reader.read(offset, block_length as usize) {
        if isb_data.len() >= 24 {
            let isb_starttime_high = Self::read_u32(&isb_data[12..16], little_endian);
            let isb_starttime_low = Self::read_u32(&isb_data[16..20], little_endian);
            if isb_starttime_high != 0 || isb_starttime_low != 0 {
                let ts = ((isb_starttime_high as u64) << 32) | (isb_starttime_low as u64);
                metadata.insert(
                    "PCAPNG:CaptureStartTime".to_string(),
                    TagValue::String(Self::format_pcapng_timestamp(ts)),
                );
            }
        }
    }
}
```

Add helper method:

```rust
/// Counts name resolution records in NRB data
fn count_nrb_records(data: &[u8], little_endian: bool) -> u32 {
    let mut count = 0u32;
    let mut offset = 0;

    while offset + 4 <= data.len() {
        let record_type = Self::read_u16(&data[offset..offset + 2], little_endian);
        let record_length = Self::read_u16(&data[offset + 2..offset + 4], little_endian) as usize;

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
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_parse_pcapng_nrb -p oxidex -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/specialized/pcap.rs
git commit -m "feat(pcap): add PCAP-NG Name Resolution and Statistics Block parsing

Extract name resolution presence and record count, plus capture start time
from Interface Statistics Blocks.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Part 2: Full ASN.1 X.509 Certificate Field Extraction

### Task 2.1: Implement Complete TBSCertificate Parsing

**Files:**
- Modify: `src/parsers/specialized/x509.rs:448-527`
- Test: `src/parsers/specialized/x509.rs` (inline tests)

**Step 1: Write the failing test**

```rust
#[test]
fn test_extract_serial_number() {
    // Create a minimal DER certificate with serial number
    let cert = create_test_der_certificate();
    let reader = TestReader::new(cert);
    let parser = X509Parser;
    let metadata = parser.parse(&reader).unwrap();

    assert!(metadata.contains_key("X509:SerialNumber"));
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_extract_serial_number -p oxidex -- --nocapture`
Expected: FAIL

**Step 3: Write minimal implementation**

Replace `extract_certificate_info` with comprehensive parsing:

```rust
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

    let tbs_start = offset;
    let tbs_end = offset + tbs_length;
    if tbs_end > der.len() {
        return Err(ExifToolError::parse_error("TBS length exceeds data"));
    }

    // Parse version (optional, context-specific [0])
    let mut version = 1;
    if offset < tbs_end && der[offset] == ASN1_CONTEXT_0 {
        offset += 1;
        if let Some(ver_len) = Self::parse_asn1_length(der, &mut offset) {
            if offset < tbs_end && der[offset] == ASN1_INTEGER {
                offset += 1;
                if let Some(int_len) = Self::parse_asn1_length(der, &mut offset) {
                    if offset + int_len <= tbs_end && int_len > 0 {
                        version = der[offset] as u32 + 1;
                        offset += int_len;
                    }
                }
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
        if let Some(serial_len) = Self::parse_asn1_length(der, &mut offset) {
            if offset + serial_len <= tbs_end {
                let serial_bytes = &der[offset..offset + serial_len];
                let serial_hex = hex::encode(serial_bytes);
                metadata.insert(
                    "X509:SerialNumber".to_string(),
                    TagValue::String(serial_hex),
                );
                offset += serial_len;
            }
        }
    }

    // Parse signature algorithm
    if offset < tbs_end && der[offset] == ASN1_SEQUENCE {
        offset += 1;
        if let Some(sig_len) = Self::parse_asn1_length(der, &mut offset) {
            let sig_end = offset + sig_len;
            if sig_end <= tbs_end && offset < sig_end && der[offset] == ASN1_OID {
                offset += 1;
                if let Some(oid_len) = Self::parse_asn1_length(der, &mut offset) {
                    if offset + oid_len <= sig_end {
                        if let Some(oid) = Self::parse_oid(&der[offset..offset + oid_len]) {
                            metadata.insert(
                                "X509:SignatureAlgorithm".to_string(),
                                TagValue::String(Self::signature_algorithm_name(&oid).to_string()),
                            );
                        }
                    }
                }
            }
            offset = sig_end;
        }
    }

    // Parse issuer
    if offset < tbs_end && der[offset] == ASN1_SEQUENCE {
        offset += 1;
        if let Some(issuer_len) = Self::parse_asn1_length(der, &mut offset) {
            if offset + issuer_len <= tbs_end {
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
                    if let Some(time_len) = Self::parse_asn1_length(der, &mut offset) {
                        if offset + time_len <= validity_end {
                            if let Some(not_before) = Self::parse_asn1_time(time_tag, &der[offset..offset + time_len]) {
                                metadata.insert(
                                    "X509:NotBefore".to_string(),
                                    TagValue::String(not_before),
                                );
                            }
                            offset += time_len;
                        }
                    }
                }
                // NotAfter
                if offset < validity_end {
                    let time_tag = der[offset];
                    offset += 1;
                    if let Some(time_len) = Self::parse_asn1_length(der, &mut offset) {
                        if offset + time_len <= validity_end {
                            if let Some(not_after) = Self::parse_asn1_time(time_tag, &der[offset..offset + time_len]) {
                                metadata.insert(
                                    "X509:NotAfter".to_string(),
                                    TagValue::String(not_after.clone()),
                                );
                                // Calculate expiry status
                                let (is_expired, _days) = Self::calculate_expiry(&not_after);
                                metadata.insert(
                                    "X509:IsExpired".to_string(),
                                    TagValue::String(if is_expired { "Yes" } else { "No" }.to_string()),
                                );
                            }
                            offset += time_len;
                        }
                    }
                }
            }
        }
    }

    // Parse subject
    if offset < tbs_end && der[offset] == ASN1_SEQUENCE {
        offset += 1;
        if let Some(subject_len) = Self::parse_asn1_length(der, &mut offset) {
            if offset + subject_len <= tbs_end {
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
                    metadata.insert("X509:SubjectEmail".to_string(), TagValue::String(email.clone()));
                }
                offset += subject_len;
            }
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
                        if algo_end <= spki_end && offset < algo_end && der[offset] == ASN1_OID {
                            offset += 1;
                            if let Some(oid_len) = Self::parse_asn1_length(der, &mut offset) {
                                if offset + oid_len <= algo_end {
                                    if let Some(oid) = Self::parse_oid(&der[offset..offset + oid_len]) {
                                        metadata.insert(
                                            "X509:PublicKeyAlgorithm".to_string(),
                                            TagValue::String(Self::public_key_algorithm_name(&oid).to_string()),
                                        );
                                    }
                                }
                            }
                        }
                        offset = algo_end;
                    }
                }
                // Subject public key (BIT STRING)
                if offset < spki_end && der[offset] == ASN1_BIT_STRING {
                    offset += 1;
                    if let Some(key_len) = Self::parse_asn1_length(der, &mut offset) {
                        // Key size estimation (bits) - subtract 1 for unused bits indicator
                        let key_bits = (key_len - 1) * 8;
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
    metadata.insert("FileType".to_string(), TagValue::String("X.509".to_string()));

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
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_extract_serial_number -p oxidex -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/specialized/x509.rs
git commit -m "feat(x509): implement complete TBSCertificate field extraction

Extract serial number, signature algorithm, issuer DN, validity period,
subject DN, public key algorithm and approximate key size.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2.2: Add X.509 Extensions Parsing

**Files:**
- Modify: `src/parsers/specialized/x509.rs`
- Test: `src/parsers/specialized/x509.rs` (inline tests)

**Step 1: Write the failing test**

```rust
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
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_parse_basic_constraints -p oxidex -- --nocapture`
Expected: FAIL

**Step 3: Write minimal implementation**

Add extension parsing helpers:

```rust
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
        if let Some(bool_len) = Self::parse_asn1_length(data, &mut offset) {
            if offset + bool_len <= seq_end && bool_len > 0 {
                is_ca = Some(data[offset] != 0);
                offset += bool_len;
            }
        }
    }

    // Parse pathLenConstraint INTEGER (optional)
    if offset < seq_end && data[offset] == ASN1_INTEGER {
        offset += 1;
        if let Some(int_len) = Self::parse_asn1_length(data, &mut offset) {
            if offset + int_len <= seq_end && int_len > 0 {
                path_len = Some(data[offset] as u32);
            }
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

    let unused_bits = data[offset];
    let key_usage_byte = data[offset + 1];

    // Key usage bits (from RFC 5280)
    if key_usage_byte & 0x80 != 0 { usages.push("digitalSignature"); }
    if key_usage_byte & 0x40 != 0 { usages.push("nonRepudiation"); }
    if key_usage_byte & 0x20 != 0 { usages.push("keyEncipherment"); }
    if key_usage_byte & 0x10 != 0 { usages.push("dataEncipherment"); }
    if key_usage_byte & 0x08 != 0 { usages.push("keyAgreement"); }
    if key_usage_byte & 0x04 != 0 { usages.push("keyCertSign"); }
    if key_usage_byte & 0x02 != 0 { usages.push("cRLSign"); }
    if key_usage_byte & 0x01 != 0 { usages.push("encipherOnly"); }

    usages
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_parse_basic_constraints -p oxidex -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/specialized/x509.rs
git commit -m "feat(x509): add certificate extension parsing

Add Basic Constraints and Key Usage extension parsing for forensic analysis.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Part 3: VBA Decompression for Code Extraction

### Task 3.1: Implement MS-OVBA Compression Algorithm

**Files:**
- Modify: `src/parsers/archive/ole.rs:526-584`
- Test: `src/parsers/archive/ole.rs` (inline tests)

**Step 1: Write the failing test**

```rust
#[test]
fn test_decompress_vba_simple() {
    // Create a simple compressed VBA chunk
    // Signature byte (0x01) + compressed data
    let compressed = vec![
        0x01,       // Signature byte
        0x00, 0x00, // Chunk header (size = small)
        0x00,       // Flag byte (all literals)
        b'H', b'e', b'l', b'l', b'o',
    ];

    let result = VBAAnalyzer::decompress_vba(&compressed);
    assert!(result.is_some());
    let decompressed = result.unwrap();
    assert_eq!(&decompressed, b"Hello");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_decompress_vba_simple -p oxidex -- --nocapture`
Expected: FAIL

**Step 3: Write minimal implementation**

Replace `decompress_vba` with proper MS-OVBA implementation:

```rust
/// MS-OVBA compression signature
const VBA_COMPRESSION_SIGNATURE: u8 = 0x01;

/// Decompresses VBA compressed data using MS-OVBA algorithm
///
/// The MS-OVBA compression format consists of:
/// - 1 byte signature (0x01)
/// - Compressed chunks, each with:
///   - 2 byte header (little-endian): bits 0-11 = size-1, bit 12 = compressed flag, bits 13-15 = signature (0b011)
///   - Compressed or raw data
///
/// Compressed chunks use a flag byte followed by 8 tokens:
/// - Flag bit 0 = literal byte
/// - Flag bit 1 = copy token (offset + length)
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

    while pos + 2 <= data.len() {
        // Read chunk header (2 bytes, little-endian)
        let chunk_header = u16::from_le_bytes([data[pos], data[pos + 1]]);
        pos += 2;

        // Parse header fields
        let chunk_size = (chunk_header & 0x0FFF) as usize + 1;
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

                        let token = u16::from_le_bytes([data[pos], data[pos + 1]]);
                        pos += 2;

                        // Calculate offset and length based on decompressed size
                        let decompressed_chunk_size = output.len() - chunk_start_output_len;
                        let (offset_bits, length_bits, length_mask) =
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
    let offset_bits = if decompressed_size <= 16 { 4 }
        else if decompressed_size <= 32 { 5 }
        else if decompressed_size <= 64 { 6 }
        else if decompressed_size <= 128 { 7 }
        else if decompressed_size <= 256 { 8 }
        else if decompressed_size <= 512 { 9 }
        else if decompressed_size <= 1024 { 10 }
        else if decompressed_size <= 2048 { 11 }
        else { 12 };

    let length_bits = 16 - offset_bits;
    let length_mask = (1u16 << length_bits) - 1;

    (offset_bits, length_bits, length_mask)
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_decompress_vba_simple -p oxidex -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/archive/ole.rs
git commit -m "feat(ole): implement MS-OVBA decompression algorithm

Add proper VBA decompression with copy token handling for extracting
VBA macro source code from compressed streams.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3.2: Extract VBA Module Source Code

**Files:**
- Modify: `src/parsers/archive/ole.rs`
- Test: `src/parsers/archive/ole.rs` (inline tests)

**Step 1: Write the failing test**

```rust
#[test]
fn test_extract_vba_code_snippet() {
    // Test extracting code from decompressed VBA
    let vba_code = b"Sub Test()\n  MsgBox \"Hello\"\nEnd Sub\n";
    let snippet = VBAAnalyzer::extract_code_snippet(vba_code, 50);
    assert!(snippet.contains("Sub Test"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_extract_vba_code_snippet -p oxidex -- --nocapture`
Expected: FAIL

**Step 3: Write minimal implementation**

```rust
/// Extracts a code snippet from decompressed VBA data
///
/// # Arguments
/// * `data` - Decompressed VBA data
/// * `max_length` - Maximum length of snippet to extract
fn extract_code_snippet(data: &[u8], max_length: usize) -> String {
    // Try to find actual VBA code patterns
    let text = String::from_utf8_lossy(data);

    // Look for Sub/Function declarations
    let code_start = text.find("Sub ")
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
```

Update `analyze_vba` to use decompression:

```rust
// In analyze_vba, after module enumeration:
// Try to extract code from modules
let mut code_snippets = Vec::new();
for entry in entries.iter() {
    if entry.entry_type != STGTY_STREAM || entry.size == 0 {
        continue;
    }

    // Skip known non-code streams
    if entry.name.starts_with('_') ||
       entry.name.eq_ignore_ascii_case("dir") ||
       entry.name.eq_ignore_ascii_case("PROJECT") {
        continue;
    }

    if let Some((snippet, _)) = Self::analyze_module(reader, entry, header) {
        if !snippet.is_empty() && snippet.len() > 10 {
            code_snippets.push(format!("{}:\n{}", entry.name, snippet));
        }
    }
}

if !code_snippets.is_empty() {
    metadata.insert(
        "OLE:VBACodePreview".to_string(),
        TagValue::String(code_snippets.join("\n---\n")),
    );
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_extract_vba_code_snippet -p oxidex -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/archive/ole.rs
git commit -m "feat(ole): extract VBA module source code snippets

Decompress VBA modules and extract code snippets for forensic preview.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Part 4: Additional PE Anomaly Detection

### Task 4.1: Add Section Entropy Calculation

**Files:**
- Create: `src/parsers/pe/anomaly_detector.rs`
- Modify: `src/parsers/pe/mod.rs`
- Test: `src/parsers/pe/anomaly_detector.rs` (inline tests)

**Step 1: Write the failing test**

Create new file with test:

```rust
//! PE anomaly detection for malware analysis
//!
//! Implements various heuristics to detect suspicious characteristics
//! in PE files that may indicate packing, obfuscation, or malicious intent.

use crate::core::{FileReader, MetadataMap, TagValue};
use crate::parsers::pe::structures::SectionHeader;

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
        let data: Vec<u8> = (0..256).cycle().take(1024).collect();
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
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_entropy -p oxidex -- --nocapture`
Expected: FAIL (file doesn't exist)

**Step 3: Write minimal implementation**

Create the file with implementation above, then add to `mod.rs`:

```rust
pub mod anomaly_detector;
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_entropy -p oxidex -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/pe/anomaly_detector.rs src/parsers/pe/mod.rs
git commit -m "feat(pe): add entropy calculation for section analysis

Implement Shannon entropy calculation to detect packed/encrypted sections.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4.2: Add PE Header Anomaly Detection

**Files:**
- Modify: `src/parsers/pe/anomaly_detector.rs`
- Test: `src/parsers/pe/anomaly_detector.rs` (inline tests)

**Step 1: Write the failing test**

```rust
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
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_detect_suspicious_section_names -p oxidex -- --nocapture`
Expected: FAIL

**Step 3: Write minimal implementation**

```rust
/// Known packer section names
const PACKER_SECTIONS: &[&str] = &[
    "UPX", "upx", ".upx",                    // UPX packer
    "ASPack", ".aspack", ".adata",           // ASPack
    "PECompact", ".petite",                  // PECompact/Petite
    ".nsp0", ".nsp1", ".nsp2",               // NSPack
    ".packed", ".pack",                       // Generic
    "Themida", ".Themida",                   // Themida
    ".vmprotect", ".vmp0", ".vmp1",          // VMProtect
    "Enigma", ".enigma",                     // Enigma
    ".rsrc", // Often modified by packers
];

/// Suspicious section characteristics
const SECTION_EXECUTABLE: u32 = 0x20000000;
const SECTION_WRITABLE: u32 = 0x80000000;
const SECTION_READABLE: u32 = 0x40000000;

impl AnomalyDetector {
    /// Detects anomalies in PE sections
    pub fn detect_section_anomalies(sections: &[SectionHeader]) -> Vec<String> {
        let mut anomalies = Vec::new();

        for section in sections {
            let name = Self::section_name_string(section);

            // Check for packer signatures
            for &packer_name in PACKER_SECTIONS {
                if name.contains(packer_name) || name.starts_with(packer_name) {
                    anomalies.push(format!("Packer signature: {} in section '{}'", packer_name, name));
                }
            }

            // Check for writable + executable sections (suspicious)
            if (section.characteristics & SECTION_EXECUTABLE != 0) &&
               (section.characteristics & SECTION_WRITABLE != 0) {
                anomalies.push(format!("Section '{}' is both writable and executable", name));
            }

            // Check for unusual section names (non-printable)
            if name.chars().any(|c| !c.is_ascii_graphic() && c != ' ') {
                anomalies.push(format!("Section with non-printable name: {:?}", section.name));
            }

            // Check for zero-sized sections with raw data
            if section.virtual_size == 0 && section.size_of_raw_data > 0 {
                anomalies.push(format!("Section '{}' has zero virtual size but non-zero raw size", name));
            }

            // Check for very large virtual size vs raw size (potential unpacking)
            if section.virtual_size > section.size_of_raw_data * 10 && section.size_of_raw_data > 0 {
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
        image_base: u64,
    ) -> MetadataMap {
        let mut metadata = MetadataMap::new();

        // Section anomalies
        let section_anomalies = Self::detect_section_anomalies(sections);
        if !section_anomalies.is_empty() {
            metadata.insert(
                "PE:SectionAnomalies".to_string(),
                TagValue::new_array(
                    section_anomalies.iter()
                        .map(|s| TagValue::String(s.clone()))
                        .collect()
                ),
            );
            metadata.insert(
                "PE:SuspiciousSections".to_string(),
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
                "PE:HighEntropySections".to_string(),
                TagValue::new_array(
                    high_entropy_sections.iter()
                        .map(|s| TagValue::String(s.clone()))
                        .collect()
                ),
            );
            metadata.insert(
                "PE:PossiblyPacked".to_string(),
                TagValue::String("Yes".to_string()),
            );
        }

        // Check entry point location
        let mut entry_in_section = false;
        for section in sections {
            if entry_point >= section.virtual_address &&
               entry_point < section.virtual_address + section.virtual_size {
                entry_in_section = true;
                let name = Self::section_name_string(section);

                // Entry point in non-code section is suspicious
                if !name.starts_with(".text") && !name.starts_with("CODE") {
                    metadata.insert(
                        "PE:UnusualEntrySection".to_string(),
                        TagValue::String(format!("Entry point in '{}'", name)),
                    );
                }
                break;
            }
        }

        if !entry_in_section {
            metadata.insert(
                "PE:EntryPointAnomaly".to_string(),
                TagValue::String("Entry point outside all sections".to_string()),
            );
        }

        metadata
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_detect_suspicious_section_names -p oxidex -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/parsers/pe/anomaly_detector.rs
git commit -m "feat(pe): add comprehensive PE anomaly detection

Detect packer signatures, suspicious section characteristics, high entropy
sections, and entry point anomalies for malware analysis.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Part 5: Comprehensive Test Coverage

### Task 5.1: Registry Parser Integration Tests

**Files:**
- Create: `tests/forensic/registry_tests.rs`
- Modify: `tests/forensic/mod.rs` (create if needed)

**Step 1: Write comprehensive tests**

```rust
//! Registry hive parser integration tests

use oxidex::parsers::specialized::registry::{parse_registry_metadata, RegistryParser};
use oxidex::core::{FileReader, TagValue};
use std::io;

/// Test FileReader implementation
struct TestReader {
    data: Vec<u8>,
}

impl TestReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl FileReader for TestReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start.saturating_add(length).min(self.data.len());
        if start > self.data.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "offset beyond end"));
        }
        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

fn create_registry_header(
    primary_seq: u32,
    secondary_seq: u32,
    hive_name: &str,
    hive_type: u32,
) -> Vec<u8> {
    let mut data = vec![0u8; 4096];
    data[0..4].copy_from_slice(b"regf");
    data[4..8].copy_from_slice(&primary_seq.to_le_bytes());
    data[8..12].copy_from_slice(&secondary_seq.to_le_bytes());
    data[12..20].copy_from_slice(&133000000000000000u64.to_le_bytes());
    data[20..24].copy_from_slice(&1u32.to_le_bytes());
    data[24..28].copy_from_slice(&5u32.to_le_bytes());
    data[28..32].copy_from_slice(&hive_type.to_le_bytes());
    data[36..40].copy_from_slice(&0x1000u32.to_le_bytes());
    data[40..44].copy_from_slice(&1048576u32.to_le_bytes());

    for (i, c) in hive_name.encode_utf16().enumerate() {
        if i * 2 + 1 < 64 {
            data[48 + i * 2..48 + i * 2 + 2].copy_from_slice(&c.to_le_bytes());
        }
    }

    data
}

#[test]
fn test_registry_clean_shutdown() {
    let data = create_registry_header(100, 100, "NTUSER.DAT", 0);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).unwrap();

    assert_eq!(metadata.get("Registry:SequenceValid"), Some(&TagValue::String("Yes".into())));
    assert!(!metadata.contains_key("ForensicNote"));
}

#[test]
fn test_registry_dirty_shutdown() {
    let data = create_registry_header(101, 100, "SYSTEM", 0);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).unwrap();

    assert_eq!(metadata.get("Registry:SequenceValid"), Some(&TagValue::String("No".into())));
    assert!(metadata.contains_key("ForensicNote"));
}

#[test]
fn test_registry_transaction_log() {
    let data = create_registry_header(50, 50, "SYSTEM.LOG1", 1);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).unwrap();

    assert_eq!(metadata.get("Registry:HiveType"), Some(&TagValue::String("Transaction Log".into())));
}

#[test]
fn test_registry_sam_hive() {
    let data = create_registry_header(10, 10, "SAM", 0);
    let reader = TestReader::new(data);
    let metadata = parse_registry_metadata(&reader).unwrap();

    assert!(metadata.get("Registry:HivePurpose")
        .map(|v| v.to_string().contains("Security Accounts"))
        .unwrap_or(false));
}

#[test]
fn test_registry_all_hive_types() {
    let hives = vec![
        ("NTUSER.DAT", "User profile"),
        ("SYSTEM", "System-wide"),
        ("SOFTWARE", "software"),
        ("SECURITY", "Security policy"),
        ("DEFAULT", "Default user"),
    ];

    for (name, expected_substr) in hives {
        let data = create_registry_header(1, 1, name, 0);
        let reader = TestReader::new(data);
        let metadata = parse_registry_metadata(&reader).unwrap();

        let purpose = metadata.get("Registry:HivePurpose")
            .map(|v| v.to_string().to_lowercase())
            .unwrap_or_default();

        assert!(
            purpose.contains(&expected_substr.to_lowercase()),
            "Hive {} should have purpose containing '{}', got '{}'",
            name, expected_substr, purpose
        );
    }
}
```

**Step 2: Create mod.rs and run tests**

Create `tests/forensic/mod.rs`:
```rust
mod registry_tests;
```

Run: `cargo test --test forensic -- --nocapture`
Expected: All PASS

**Step 3: Commit**

```bash
git add tests/forensic/
git commit -m "test(registry): add comprehensive registry parser tests

Cover clean/dirty shutdown, transaction logs, and all hive type detection.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5.2: EVTX Parser Integration Tests

**Files:**
- Create: `tests/forensic/evtx_tests.rs`
- Modify: `tests/forensic/mod.rs`

**Step 1: Write comprehensive tests**

```rust
//! EVTX event log parser integration tests

use oxidex::parsers::specialized::evtx::{parse_evtx_metadata, EvtxParser};
use oxidex::core::{FileReader, TagValue};
use std::io;

struct TestReader {
    data: Vec<u8>,
}

impl TestReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl FileReader for TestReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start.saturating_add(length).min(self.data.len());
        if start > self.data.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "offset beyond end"));
        }
        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

fn create_evtx_header(
    chunk_count: u16,
    dirty: bool,
    full: bool,
) -> Vec<u8> {
    let mut data = vec![0u8; 4096];

    // ElfFile signature
    data[0..8].copy_from_slice(b"ElfFile\0");

    // First chunk number (offset 8, 8 bytes)
    data[8..16].copy_from_slice(&0u64.to_le_bytes());

    // Last chunk number (offset 16, 8 bytes)
    data[16..24].copy_from_slice(&(chunk_count as u64 - 1).to_le_bytes());

    // Next record ID (offset 24, 8 bytes)
    data[24..32].copy_from_slice(&100u64.to_le_bytes());

    // Header size (offset 32, 4 bytes) = 128
    data[32..36].copy_from_slice(&128u32.to_le_bytes());

    // Minor version (offset 36, 2 bytes)
    data[36..38].copy_from_slice(&1u16.to_le_bytes());

    // Major version (offset 38, 2 bytes)
    data[38..40].copy_from_slice(&3u16.to_le_bytes());

    // Chunk count (offset 40, 2 bytes)
    data[40..42].copy_from_slice(&chunk_count.to_le_bytes());

    // Flags (offset 44, 4 bytes)
    let mut flags = 0u32;
    if dirty { flags |= 0x01; }
    if full { flags |= 0x02; }
    data[44..48].copy_from_slice(&flags.to_le_bytes());

    data
}

#[test]
fn test_evtx_basic_parsing() {
    let data = create_evtx_header(5, false, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).unwrap();

    assert_eq!(metadata.get("FileType"), Some(&TagValue::String("EVTX".into())));
    assert_eq!(metadata.get("EVTX:ChunkCount"), Some(&TagValue::String("5".into())));
}

#[test]
fn test_evtx_dirty_flag() {
    let data = create_evtx_header(10, true, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).unwrap();

    assert_eq!(metadata.get("EVTX:IsDirty"), Some(&TagValue::String("Yes".into())));
}

#[test]
fn test_evtx_full_flag() {
    let data = create_evtx_header(100, false, true);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).unwrap();

    assert_eq!(metadata.get("EVTX:IsFull"), Some(&TagValue::String("Yes".into())));
}

#[test]
fn test_evtx_version_extraction() {
    let data = create_evtx_header(1, false, false);
    let reader = TestReader::new(data);
    let metadata = parse_evtx_metadata(&reader).unwrap();

    assert_eq!(metadata.get("EVTX:Version"), Some(&TagValue::String("3.1".into())));
}
```

**Step 2: Add to mod.rs and run**

Add to `tests/forensic/mod.rs`:
```rust
mod evtx_tests;
```

Run: `cargo test evtx_tests -- --nocapture`
Expected: All PASS

**Step 3: Commit**

```bash
git add tests/forensic/evtx_tests.rs tests/forensic/mod.rs
git commit -m "test(evtx): add comprehensive EVTX parser tests

Cover basic parsing, dirty/full flags, version extraction.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5.3: Prefetch Parser Integration Tests

**Files:**
- Create: `tests/forensic/prefetch_tests.rs`
- Modify: `tests/forensic/mod.rs`

**Step 1: Write comprehensive tests**

```rust
//! Prefetch file parser integration tests

use oxidex::parsers::specialized::prefetch::{parse_prefetch_metadata, PrefetchParser};
use oxidex::core::{FileReader, TagValue};
use std::io;

struct TestReader {
    data: Vec<u8>,
}

impl TestReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl FileReader for TestReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start.saturating_add(length).min(self.data.len());
        if start > self.data.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "offset beyond end"));
        }
        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

fn create_prefetch_file(version: u32, exe_name: &str, run_count: u32) -> Vec<u8> {
    let mut data = vec![0u8; 512];

    // Version (offset 0, 4 bytes)
    data[0..4].copy_from_slice(&version.to_le_bytes());

    // Signature "SCCA" (offset 4, 4 bytes)
    data[4..8].copy_from_slice(b"SCCA");

    // File size (offset 12, 4 bytes)
    data[12..16].copy_from_slice(&512u32.to_le_bytes());

    // Executable name (offset 16, 60 bytes UTF-16LE)
    for (i, c) in exe_name.encode_utf16().take(29).enumerate() {
        data[16 + i * 2..16 + i * 2 + 2].copy_from_slice(&c.to_le_bytes());
    }

    // Hash (offset 76, 4 bytes)
    data[76..80].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());

    // Run count location varies by version
    // For version 17 (XP): offset 144
    // For version 23 (Vista/7): offset 152
    // For version 26 (8): offset 208
    // For version 30 (10): offset 208
    let run_count_offset = match version {
        17 => 144,
        23 => 152,
        26 | 30 => 208,
        _ => 144,
    };

    if run_count_offset + 4 <= data.len() {
        data[run_count_offset..run_count_offset + 4].copy_from_slice(&run_count.to_le_bytes());
    }

    data
}

#[test]
fn test_prefetch_windows_xp() {
    let data = create_prefetch_file(17, "NOTEPAD.EXE", 5);
    let reader = TestReader::new(data);
    let metadata = parse_prefetch_metadata(&reader).unwrap();

    assert_eq!(metadata.get("Prefetch:WindowsVersion"), Some(&TagValue::String("Windows XP".into())));
    assert!(metadata.get("Prefetch:ExecutableName")
        .map(|v| v.to_string().contains("NOTEPAD"))
        .unwrap_or(false));
}

#[test]
fn test_prefetch_windows_10() {
    let data = create_prefetch_file(30, "CMD.EXE", 100);
    let reader = TestReader::new(data);
    let metadata = parse_prefetch_metadata(&reader).unwrap();

    assert_eq!(metadata.get("Prefetch:WindowsVersion"), Some(&TagValue::String("Windows 10/11".into())));
}

#[test]
fn test_prefetch_run_count() {
    let data = create_prefetch_file(23, "CHROME.EXE", 42);
    let reader = TestReader::new(data);
    let metadata = parse_prefetch_metadata(&reader).unwrap();

    assert_eq!(metadata.get("Prefetch:RunCount"), Some(&TagValue::String("42".into())));
}

#[test]
fn test_prefetch_hash_extraction() {
    let data = create_prefetch_file(17, "TEST.EXE", 1);
    let reader = TestReader::new(data);
    let metadata = parse_prefetch_metadata(&reader).unwrap();

    assert!(metadata.contains_key("Prefetch:Hash"));
}
```

**Step 2: Add to mod.rs and run**

Add to `tests/forensic/mod.rs`:
```rust
mod prefetch_tests;
```

Run: `cargo test prefetch_tests -- --nocapture`
Expected: All PASS

**Step 3: Commit**

```bash
git add tests/forensic/prefetch_tests.rs tests/forensic/mod.rs
git commit -m "test(prefetch): add comprehensive Prefetch parser tests

Cover Windows XP through 10/11 versions, run count, hash extraction.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5.4: PCAP/PCAP-NG Integration Tests

**Files:**
- Create: `tests/forensic/pcap_tests.rs`
- Modify: `tests/forensic/mod.rs`

**Step 1: Write comprehensive tests**

```rust
//! PCAP/PCAP-NG parser integration tests

use oxidex::parsers::specialized::pcap::{parse_pcap_metadata, PCAPParser};
use oxidex::core::{FileReader, TagValue};
use std::io;

struct TestReader {
    data: Vec<u8>,
}

impl TestReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl FileReader for TestReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start.saturating_add(length).min(self.data.len());
        if start > self.data.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "offset beyond end"));
        }
        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

fn create_pcap_with_packets(link_type: u32, packet_count: u32) -> Vec<u8> {
    let mut data = Vec::new();

    // Global header (little-endian)
    data.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]); // Magic
    data.extend_from_slice(&2u16.to_le_bytes()); // Major
    data.extend_from_slice(&4u16.to_le_bytes()); // Minor
    data.extend_from_slice(&0i32.to_le_bytes()); // Timezone
    data.extend_from_slice(&0u32.to_le_bytes()); // Sigfigs
    data.extend_from_slice(&65535u32.to_le_bytes()); // Snaplen
    data.extend_from_slice(&link_type.to_le_bytes()); // Link type

    // Add packets
    for i in 0..packet_count {
        // Packet header
        data.extend_from_slice(&(1577836800 + i).to_le_bytes()); // Timestamp sec
        data.extend_from_slice(&0u32.to_le_bytes()); // Timestamp usec
        data.extend_from_slice(&64u32.to_le_bytes()); // Captured len
        data.extend_from_slice(&64u32.to_le_bytes()); // Original len

        // Packet data (64 bytes)
        data.extend_from_slice(&[0u8; 64]);
    }

    data
}

#[test]
fn test_pcap_ethernet() {
    let data = create_pcap_with_packets(1, 10);
    let reader = TestReader::new(data);
    let metadata = parse_pcap_metadata(&reader).unwrap();

    assert_eq!(metadata.get("FileType"), Some(&TagValue::String("PCAP".into())));
    assert_eq!(metadata.get("PCAP:LinkTypeName"), Some(&TagValue::String("Ethernet".into())));
    assert_eq!(metadata.get("PCAP:PacketCount"), Some(&TagValue::String("10".into())));
}

#[test]
fn test_pcap_wifi() {
    let data = create_pcap_with_packets(105, 5);
    let reader = TestReader::new(data);
    let metadata = parse_pcap_metadata(&reader).unwrap();

    assert_eq!(metadata.get("PCAP:LinkTypeName"), Some(&TagValue::String("IEEE 802.11 (WiFi)".into())));
}

#[test]
fn test_pcap_timestamps() {
    let data = create_pcap_with_packets(1, 3);
    let reader = TestReader::new(data);
    let metadata = parse_pcap_metadata(&reader).unwrap();

    assert!(metadata.contains_key("PCAP:FirstPacketTime"));
    assert!(metadata.contains_key("PCAP:LastPacketTime"));
}

#[test]
fn test_pcap_duration() {
    let data = create_pcap_with_packets(1, 100);
    let reader = TestReader::new(data);
    let metadata = parse_pcap_metadata(&reader).unwrap();

    // Duration should be ~99 seconds (100 packets, 1 second apart)
    assert!(metadata.contains_key("PCAP:Duration"));
}

#[test]
fn test_pcap_big_endian() {
    let mut data = Vec::new();

    // Global header (big-endian)
    data.extend_from_slice(&[0xa1, 0xb2, 0xc3, 0xd4]); // Magic
    data.extend_from_slice(&2u16.to_be_bytes());
    data.extend_from_slice(&4u16.to_be_bytes());
    data.extend_from_slice(&0i32.to_be_bytes());
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(&65535u32.to_be_bytes());
    data.extend_from_slice(&1u32.to_be_bytes());

    let reader = TestReader::new(data);
    let metadata = parse_pcap_metadata(&reader).unwrap();

    assert_eq!(metadata.get("PCAP:ByteOrder"), Some(&TagValue::String("Big-endian".into())));
}

#[test]
fn test_pcap_nanosecond() {
    let mut data = Vec::new();

    // Nanosecond PCAP magic
    data.extend_from_slice(&[0x4d, 0x3c, 0xb2, 0xa1]);
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&4u16.to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&65535u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());

    let reader = TestReader::new(data);
    let metadata = parse_pcap_metadata(&reader).unwrap();

    assert_eq!(metadata.get("PCAP:TimestampPrecision"), Some(&TagValue::String("Nanoseconds".into())));
}
```

**Step 2: Add to mod.rs and run**

Add to `tests/forensic/mod.rs`:
```rust
mod pcap_tests;
```

Run: `cargo test pcap_tests -- --nocapture`
Expected: All PASS

**Step 3: Commit**

```bash
git add tests/forensic/pcap_tests.rs tests/forensic/mod.rs
git commit -m "test(pcap): add comprehensive PCAP parser tests

Cover Ethernet, WiFi, timestamps, duration, endianness, nanosecond precision.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5.5: X.509 Certificate Integration Tests

**Files:**
- Create: `tests/forensic/x509_tests.rs`
- Modify: `tests/forensic/mod.rs`

**Step 1: Write comprehensive tests**

```rust
//! X.509 certificate parser integration tests

use oxidex::parsers::specialized::x509::{parse_x509_metadata, X509Parser};
use oxidex::core::{FileReader, TagValue};
use std::io;

struct TestReader {
    data: Vec<u8>,
}

impl TestReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl FileReader for TestReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start.saturating_add(length).min(self.data.len());
        if start > self.data.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "offset beyond end"));
        }
        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

#[test]
fn test_x509_pem_detection() {
    let pem = b"-----BEGIN CERTIFICATE-----\nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA\n-----END CERTIFICATE-----";
    let reader = TestReader::new(pem.to_vec());

    assert!(X509Parser::verify_signature(&reader).unwrap());
}

#[test]
fn test_x509_der_detection() {
    let mut der = vec![0x30, 0x82, 0x01, 0x00]; // SEQUENCE, long form
    der.extend_from_slice(&[0x30, 0x03, 0x02, 0x01, 0x00, 0x00]);
    let reader = TestReader::new(der);

    assert!(X509Parser::verify_signature(&reader).unwrap());
}

#[test]
fn test_x509_invalid_data() {
    let invalid = vec![0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09];
    let reader = TestReader::new(invalid);

    assert!(!X509Parser::verify_signature(&reader).unwrap());
}

#[test]
fn test_x509_fingerprints() {
    // Use a simple valid-ish DER structure
    let mut der = Vec::new();
    der.push(0x30); // SEQUENCE
    der.push(0x10); // Length 16
    der.push(0x30); // TBS SEQUENCE
    der.push(0x0E); // Length 14
    // Minimal content
    der.extend_from_slice(&[0x02, 0x01, 0x01]); // INTEGER 1 (version)
    der.extend_from_slice(&[0x02, 0x04, 0x12, 0x34, 0x56, 0x78]); // Serial
    der.extend_from_slice(&[0x30, 0x03, 0x06, 0x01, 0x00]); // AlgID

    let reader = TestReader::new(der);
    let metadata = parse_x509_metadata(&reader).unwrap();

    assert!(metadata.contains_key("X509:SHA256Fingerprint"));
    assert!(metadata.contains_key("X509:SHA1Fingerprint"));
}
```

**Step 2: Add to mod.rs and run**

Add to `tests/forensic/mod.rs`:
```rust
mod x509_tests;
```

Run: `cargo test x509_tests -- --nocapture`
Expected: All PASS

**Step 3: Commit**

```bash
git add tests/forensic/x509_tests.rs tests/forensic/mod.rs
git commit -m "test(x509): add comprehensive X.509 parser tests

Cover PEM/DER detection, fingerprint calculation, format validation.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5.6: OLE/VBA Integration Tests

**Files:**
- Create: `tests/forensic/ole_tests.rs`
- Modify: `tests/forensic/mod.rs`

**Step 1: Write comprehensive tests**

```rust
//! OLE/VBA parser integration tests

use oxidex::parsers::archive::ole::{OLEParser, VBAAnalyzer};
use oxidex::core::{FileReader, FormatParser, TagValue};
use std::io;

struct TestReader {
    data: Vec<u8>,
}

impl TestReader {
    fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl FileReader for TestReader {
    fn read(&self, offset: u64, length: usize) -> io::Result<&[u8]> {
        let start = offset as usize;
        let end = start.saturating_add(length).min(self.data.len());
        if start > self.data.len() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "offset beyond end"));
        }
        Ok(&self.data[start..end])
    }

    fn size(&self) -> u64 {
        self.data.len() as u64
    }
}

#[test]
fn test_suspicious_auto_open() {
    let code = b"Sub Auto_Open()\n  Shell \"cmd /c calc\"\nEnd Sub";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    assert!(patterns.iter().any(|p| p.contains("Auto_Open")));
    assert!(patterns.iter().any(|p| p.contains("Shell")));
}

#[test]
fn test_suspicious_wscript() {
    let code = b"Set obj = CreateObject(\"WScript.Shell\")\nobj.Run \"cmd\"";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    assert!(patterns.iter().any(|p| p.contains("WScript")));
    assert!(patterns.iter().any(|p| p.contains("CreateObject")));
}

#[test]
fn test_suspicious_powershell() {
    let code = b"Shell \"powershell -encodedcommand ABC123\"";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    assert!(patterns.iter().any(|p| p.contains("PowerShell")));
    assert!(patterns.iter().any(|p| p.contains("encodedcommand") || p.contains("Encoded")));
}

#[test]
fn test_suspicious_network() {
    let code = b"Set http = CreateObject(\"MSXML2.XMLHTTP\")\nhttp.Open \"GET\", \"http://evil.com\"";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    assert!(patterns.iter().any(|p| p.contains("XMLHTTP") || p.contains("Network")));
}

#[test]
fn test_suspicious_obfuscation() {
    let code = b"x = Chr(72) & Chr(101) & Chr(108) & Chr(108) & Chr(111)";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    assert!(patterns.iter().any(|p| p.contains("Chr")));
}

#[test]
fn test_excessive_concatenation() {
    let mut code = String::from("x = \"a\"");
    for _ in 0..30 {
        code.push_str(" & \"b\"");
    }

    let patterns = VBAAnalyzer::check_suspicious_patterns(code.as_bytes());

    assert!(patterns.iter().any(|p| p.contains("concatenation")));
}

#[test]
fn test_clean_vba_code() {
    let code = b"Sub CalculateSum()\n  Dim total As Integer\n  total = 1 + 2 + 3\n  MsgBox total\nEnd Sub";
    let patterns = VBAAnalyzer::check_suspicious_patterns(code);

    // Should have minimal suspicious patterns (maybe "Open" false positive)
    let serious_patterns: Vec<_> = patterns.iter()
        .filter(|p| !p.contains("File: Open"))
        .collect();

    assert!(serious_patterns.is_empty(), "Clean code flagged: {:?}", serious_patterns);
}

#[test]
fn test_ole_invalid_signature() {
    let data = vec![0u8; 512];
    let reader = TestReader::new(data);
    let parser = OLEParser;

    assert!(parser.parse(&reader).is_err());
}
```

**Step 2: Add to mod.rs and run**

Add to `tests/forensic/mod.rs`:
```rust
mod ole_tests;
```

Run: `cargo test ole_tests -- --nocapture`
Expected: All PASS

**Step 3: Final commit**

```bash
git add tests/forensic/ole_tests.rs tests/forensic/mod.rs
git commit -m "test(ole): add comprehensive OLE/VBA parser tests

Cover suspicious pattern detection: auto-exec, shell, PowerShell,
network access, obfuscation, and clean code validation.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Summary

This plan implements:

1. **PCAP-NG Enhancements** (Tasks 1.1-1.3)
   - Interface Description Block options (name, MAC, speed, filter)
   - Enhanced Packet Block timestamp parsing
   - Name Resolution and Statistics Block support

2. **X.509 Certificate Parsing** (Tasks 2.1-2.2)
   - Complete TBSCertificate field extraction
   - Serial number, signature algorithm, issuer/subject DN
   - Validity period, public key info
   - Extensions parsing (Basic Constraints, Key Usage)

3. **VBA Decompression** (Tasks 3.1-3.2)
   - MS-OVBA compression algorithm implementation
   - VBA module source code extraction
   - Code snippet preview in metadata

4. **PE Anomaly Detection** (Tasks 4.1-4.2)
   - Section entropy calculation
   - Packer signature detection
   - Suspicious section characteristics
   - Entry point anomaly detection

5. **Test Coverage** (Tasks 5.1-5.6)
   - Registry parser: 5 tests
   - EVTX parser: 4 tests
   - Prefetch parser: 4 tests
   - PCAP parser: 7 tests
   - X.509 parser: 4 tests
   - OLE/VBA parser: 8 tests

**Total: 32+ new tests across 6 test modules**
