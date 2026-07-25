//! Video format detection
//!
//! Handles detection of video formats including MTS/M2TS transport streams.

/// Detect MPEG Transport Stream (MTS/M2TS) format
///
/// MTS uses sync byte 0x47 repeating every 188 bytes (standard TS)
/// or every 192 bytes (M2TS with 4-byte timestamp header).
///
/// # Arguments
///
/// * `data` - Magic bytes buffer (at least 576 bytes for reliable detection)
///
/// # Returns
///
/// `true` if MTS sync pattern detected
pub fn is_mts_stream(data: &[u8]) -> bool {
    // Standard TS: 188-byte packets
    if data.len() >= 564 && data[0] == 0x47 && data[188] == 0x47 && data[376] == 0x47 {
        return true;
    }

    // M2TS: 192-byte packets with timestamp
    if data.len() >= 576 && data[4] == 0x47 && data[196] == 0x47 && data[388] == 0x47 {
        return true;
    }

    false
}
