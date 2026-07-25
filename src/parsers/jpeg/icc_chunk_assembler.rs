//! ICC Profile multi-chunk assembler for JPEG APP2 segments.
//!
//! ICC profiles larger than approximately 64KB must be split across multiple
//! APP2 segments in JPEG files. Each segment contains a portion of the profile
//! along with sequencing information to enable proper reassembly.
//!
//! # ICC Profile Chunk Format
//!
//! Each APP2 segment containing ICC profile data has this structure:
//! - Identifier: `ICC_PROFILE\0` (12 bytes)
//! - Chunk number: 1 byte (1-based index of this chunk)
//! - Total chunks: 1 byte (total number of chunks in the profile)
//! - Profile data: remaining bytes (portion of the ICC profile)
//!
//! # Usage
//!
//! ```ignore
//! use oxidex::parsers::jpeg::icc_chunk_assembler::IccChunkAssembler;
//!
//! let mut assembler = IccChunkAssembler::new();
//!
//! // Add chunks from APP2 segments (data includes the ICC_PROFILE header)
//! // Each chunk has format: ICC_PROFILE\0 + chunk_num + total_chunks + data
//! let app2_segment1_data = b"ICC_PROFILE\0\x01\x02<profile data part 1>";
//! let app2_segment2_data = b"ICC_PROFILE\0\x02\x02<profile data part 2>";
//!
//! assembler.add_chunk(app2_segment1_data)?;
//! assembler.add_chunk(app2_segment2_data)?;
//!
//! // Check if all chunks have been collected
//! if assembler.is_complete() {
//!     let profile_data = assembler.assemble()?;
//!     // profile_data now contains the complete ICC profile
//! }
//! # Ok::<(), oxidex::error::ExifToolError>(())
//! ```
//!
//! # Error Handling
//!
//! The assembler validates:
//! - Correct ICC_PROFILE identifier in each chunk
//! - Valid chunk numbers (1 to total_chunks)
//! - Consistent total chunk counts across all segments
//! - No duplicate chunk numbers
//! - All chunks present before assembly

use crate::error::{ExifToolError, Result};
use std::collections::HashMap;

/// The ICC_PROFILE identifier that appears at the start of each APP2 segment.
/// This 12-byte sequence includes the null terminator.
const ICC_PROFILE_IDENTIFIER: &[u8; 12] = b"ICC_PROFILE\0";

/// Minimum size for a valid ICC profile chunk: identifier (12) + chunk_num (1) + total (1).
const MIN_CHUNK_SIZE: usize = 14;

/// Maximum number of chunks allowed in a multi-part ICC profile.
/// This is a reasonable upper limit to prevent memory exhaustion attacks.
/// With each chunk holding up to ~64KB, this allows profiles up to ~16MB.
const MAX_CHUNKS: u8 = 255;

/// Assembles ICC profile data from multiple JPEG APP2 segments.
///
/// ICC profiles larger than the JPEG segment size limit (~64KB) are split
/// across multiple APP2 segments. This struct collects individual chunks
/// and reassembles them into a complete profile.
///
/// # Implementation Details
///
/// - Chunks are stored in a HashMap keyed by chunk number (1-based)
/// - The total chunk count is recorded from the first chunk received
/// - Assembly validates that all chunks are present and in sequence
/// - Chunk data is copied rather than referenced to allow flexible lifetimes
#[derive(Debug)]
pub struct IccChunkAssembler {
    /// Storage for chunk data, keyed by 1-based chunk number.
    chunks: HashMap<u8, Vec<u8>>,

    /// Total number of chunks expected, recorded from the first chunk.
    /// None until the first chunk is added.
    total_chunks: Option<u8>,
}

impl IccChunkAssembler {
    /// Creates a new ICC chunk assembler.
    ///
    /// # Returns
    ///
    /// A new `IccChunkAssembler` instance with no chunks collected.
    ///
    /// # Example
    ///
    /// ```
    /// use oxidex::parsers::jpeg::icc_chunk_assembler::IccChunkAssembler;
    ///
    /// let assembler = IccChunkAssembler::new();
    /// assert!(!assembler.is_complete());
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            chunks: HashMap::new(),
            total_chunks: None,
        }
    }

    /// Adds an ICC profile chunk from an APP2 segment.
    ///
    /// The input data should be the complete APP2 segment payload,
    /// starting with the "ICC_PROFILE\0" identifier.
    ///
    /// # Parameters
    ///
    /// - `data`: The APP2 segment data including the ICC_PROFILE header
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the chunk was successfully added
    /// - `Err(ExifToolError)` if the chunk is invalid or a duplicate
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Segment is too short to contain valid ICC data
    /// - ICC_PROFILE identifier is missing or incorrect
    /// - Chunk number is 0 (must be 1-based)
    /// - Chunk number exceeds the total chunk count
    /// - Total chunk count is inconsistent with previously added chunks
    /// - Duplicate chunk number is detected
    ///
    /// # Example
    ///
    /// ```
    /// use oxidex::parsers::jpeg::icc_chunk_assembler::IccChunkAssembler;
    ///
    /// let mut assembler = IccChunkAssembler::new();
    ///
    /// // Create a minimal valid chunk (chunk 1 of 1)
    /// let mut chunk = b"ICC_PROFILE\0".to_vec();
    /// chunk.push(1); // chunk number (1-based)
    /// chunk.push(1); // total chunks
    /// chunk.extend_from_slice(b"profile_data");
    ///
    /// assert!(assembler.add_chunk(&chunk).is_ok());
    /// ```
    pub fn add_chunk(&mut self, data: &[u8]) -> Result<()> {
        // Validate minimum size: identifier (12) + chunk_num (1) + total (1)
        if data.len() < MIN_CHUNK_SIZE {
            return Err(ExifToolError::parse_error(format!(
                "ICC profile chunk too short: {} bytes (minimum {} required)",
                data.len(),
                MIN_CHUNK_SIZE
            )));
        }

        // Validate ICC_PROFILE identifier
        if &data[0..12] != ICC_PROFILE_IDENTIFIER {
            return Err(ExifToolError::parse_error(
                "Invalid ICC_PROFILE identifier in APP2 segment",
            ));
        }

        // Extract chunk number and total count
        // Byte 12: current chunk number (1-based)
        // Byte 13: total number of chunks
        let chunk_number = data[12];
        let total = data[13];

        // Validate chunk number is within valid range (1 to total)
        if chunk_number == 0 {
            return Err(ExifToolError::parse_error(
                "ICC profile chunk number cannot be 0 (must be 1-based)",
            ));
        }

        if total == 0 {
            return Err(ExifToolError::parse_error(
                "ICC profile total chunk count cannot be 0",
            ));
        }

        if chunk_number > total {
            return Err(ExifToolError::parse_error(format!(
                "ICC profile chunk number {} exceeds total count {}",
                chunk_number, total
            )));
        }

        // Note: total is u8, so it's already bounded by 255 (MAX_CHUNKS)
        // No explicit check needed since u8 cannot exceed MAX_CHUNKS

        // Check for consistency with previously recorded total
        if let Some(expected_total) = self.total_chunks {
            if total != expected_total {
                return Err(ExifToolError::parse_error(format!(
                    "ICC profile chunk count mismatch: expected {}, got {}",
                    expected_total, total
                )));
            }
        } else {
            // First chunk establishes the expected total
            self.total_chunks = Some(total);
        }

        // Check for duplicate chunks
        if self.chunks.contains_key(&chunk_number) {
            return Err(ExifToolError::parse_error(format!(
                "Duplicate ICC profile chunk number {}",
                chunk_number
            )));
        }

        // Extract and store the chunk data (everything after the 14-byte header)
        let chunk_data = data[14..].to_vec();
        self.chunks.insert(chunk_number, chunk_data);

        Ok(())
    }

    /// Checks if all chunks have been collected.
    ///
    /// # Returns
    ///
    /// `true` if:
    /// - At least one chunk has been added (total_chunks is known)
    /// - The number of chunks collected equals the expected total
    ///
    /// `false` if:
    /// - No chunks have been added yet
    /// - Some chunks are still missing
    ///
    /// # Example
    ///
    /// ```
    /// use oxidex::parsers::jpeg::icc_chunk_assembler::IccChunkAssembler;
    ///
    /// let mut assembler = IccChunkAssembler::new();
    /// assert!(!assembler.is_complete()); // No chunks yet
    ///
    /// // Add a single-chunk profile
    /// let mut chunk = b"ICC_PROFILE\0".to_vec();
    /// chunk.push(1); // chunk 1
    /// chunk.push(1); // of 1
    /// chunk.extend_from_slice(b"data");
    /// assembler.add_chunk(&chunk).unwrap();
    ///
    /// assert!(assembler.is_complete()); // All chunks received
    /// ```
    pub fn is_complete(&self) -> bool {
        match self.total_chunks {
            Some(total) => self.chunks.len() == total as usize,
            None => false,
        }
    }

    /// Assembles collected chunks into a complete ICC profile.
    ///
    /// Chunks are concatenated in order (chunk 1 first, then 2, etc.)
    /// to reconstruct the original ICC profile data.
    ///
    /// # Returns
    ///
    /// - `Ok(Vec<u8>)` containing the complete ICC profile data
    /// - `Err(ExifToolError)` if the profile is incomplete
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No chunks have been added
    /// - One or more chunks are missing
    ///
    /// # Example
    ///
    /// ```
    /// use oxidex::parsers::jpeg::icc_chunk_assembler::IccChunkAssembler;
    ///
    /// let mut assembler = IccChunkAssembler::new();
    ///
    /// // Add complete single-chunk profile
    /// let mut chunk = b"ICC_PROFILE\0".to_vec();
    /// chunk.push(1);
    /// chunk.push(1);
    /// chunk.extend_from_slice(b"icc_profile_data");
    /// assembler.add_chunk(&chunk).unwrap();
    ///
    /// let profile = assembler.assemble().unwrap();
    /// assert_eq!(profile, b"icc_profile_data");
    /// ```
    pub fn assemble(&self) -> Result<Vec<u8>> {
        let total = self.total_chunks.ok_or_else(|| {
            ExifToolError::parse_error("Cannot assemble ICC profile: no chunks have been added")
        })?;

        // Verify all chunks are present and identify any missing ones
        let missing: Vec<u8> = (1..=total)
            .filter(|n| !self.chunks.contains_key(n))
            .collect();

        if !missing.is_empty() {
            return Err(ExifToolError::parse_error(format!(
                "Cannot assemble ICC profile: missing chunk(s): {:?}",
                missing
            )));
        }

        // Calculate total size for pre-allocation
        let total_size: usize = self.chunks.values().map(|c| c.len()).sum();
        let mut assembled = Vec::with_capacity(total_size);

        // Concatenate chunks in order (1 to total)
        for chunk_num in 1..=total {
            if let Some(chunk_data) = self.chunks.get(&chunk_num) {
                assembled.extend_from_slice(chunk_data);
            }
        }

        Ok(assembled)
    }

    /// Returns the number of chunks that have been collected so far.
    ///
    /// # Returns
    ///
    /// The number of unique chunks that have been successfully added.
    ///
    /// # Example
    ///
    /// ```
    /// use oxidex::parsers::jpeg::icc_chunk_assembler::IccChunkAssembler;
    ///
    /// let mut assembler = IccChunkAssembler::new();
    /// assert_eq!(assembler.chunk_count(), 0);
    ///
    /// // After adding a chunk
    /// let mut chunk = b"ICC_PROFILE\0".to_vec();
    /// chunk.push(1);
    /// chunk.push(2); // 2 chunks total
    /// chunk.extend_from_slice(b"data");
    /// assembler.add_chunk(&chunk).unwrap();
    ///
    /// assert_eq!(assembler.chunk_count(), 1);
    /// ```
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Returns the expected total number of chunks, if known.
    ///
    /// # Returns
    ///
    /// - `Some(n)` where n is the total chunk count from the first chunk added
    /// - `None` if no chunks have been added yet
    ///
    /// # Example
    ///
    /// ```
    /// use oxidex::parsers::jpeg::icc_chunk_assembler::IccChunkAssembler;
    ///
    /// let mut assembler = IccChunkAssembler::new();
    /// assert_eq!(assembler.expected_total(), None);
    ///
    /// let mut chunk = b"ICC_PROFILE\0".to_vec();
    /// chunk.push(1);
    /// chunk.push(3); // 3 chunks total
    /// chunk.extend_from_slice(b"data");
    /// assembler.add_chunk(&chunk).unwrap();
    ///
    /// assert_eq!(assembler.expected_total(), Some(3));
    /// ```
    pub fn expected_total(&self) -> Option<u8> {
        self.total_chunks
    }

    /// Resets the assembler to its initial state.
    ///
    /// All collected chunks are discarded and the total chunk count is cleared.
    /// This allows reusing the assembler for a new ICC profile.
    ///
    /// # Example
    ///
    /// ```
    /// use oxidex::parsers::jpeg::icc_chunk_assembler::IccChunkAssembler;
    ///
    /// let mut assembler = IccChunkAssembler::new();
    ///
    /// // Add a chunk
    /// let mut chunk = b"ICC_PROFILE\0".to_vec();
    /// chunk.push(1);
    /// chunk.push(1);
    /// chunk.extend_from_slice(b"data");
    /// assembler.add_chunk(&chunk).unwrap();
    /// assert!(assembler.is_complete());
    ///
    /// // Reset and verify state
    /// assembler.clear();
    /// assert!(!assembler.is_complete());
    /// assert_eq!(assembler.chunk_count(), 0);
    /// assert_eq!(assembler.expected_total(), None);
    /// ```
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.total_chunks = None;
    }
}

impl Default for IccChunkAssembler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper function to create a valid ICC chunk.
    ///
    /// # Parameters
    /// - `chunk_number`: 1-based chunk index
    /// - `total`: Total number of chunks
    /// - `data`: The profile data for this chunk
    fn create_chunk(chunk_number: u8, total: u8, data: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::with_capacity(14 + data.len());
        chunk.extend_from_slice(ICC_PROFILE_IDENTIFIER);
        chunk.push(chunk_number);
        chunk.push(total);
        chunk.extend_from_slice(data);
        chunk
    }

    // =========================================================================
    // Constructor Tests
    // =========================================================================

    #[test]
    fn test_new_creates_empty_assembler() {
        let assembler = IccChunkAssembler::new();
        assert!(!assembler.is_complete());
        assert_eq!(assembler.chunk_count(), 0);
        assert_eq!(assembler.expected_total(), None);
    }

    #[test]
    fn test_default_equals_new() {
        let new_assembler = IccChunkAssembler::new();
        let default_assembler = IccChunkAssembler::default();
        assert_eq!(new_assembler.chunk_count(), default_assembler.chunk_count());
        assert_eq!(
            new_assembler.expected_total(),
            default_assembler.expected_total()
        );
    }

    // =========================================================================
    // Single Chunk Tests
    // =========================================================================

    #[test]
    fn test_single_chunk_profile() {
        let mut assembler = IccChunkAssembler::new();
        let profile_data = b"complete ICC profile data";
        let chunk = create_chunk(1, 1, profile_data);

        assert!(assembler.add_chunk(&chunk).is_ok());
        assert!(assembler.is_complete());
        assert_eq!(assembler.chunk_count(), 1);
        assert_eq!(assembler.expected_total(), Some(1));

        let assembled = assembler.assemble().unwrap();
        assert_eq!(assembled, profile_data);
    }

    #[test]
    fn test_single_chunk_empty_data() {
        let mut assembler = IccChunkAssembler::new();
        let chunk = create_chunk(1, 1, &[]);

        assert!(assembler.add_chunk(&chunk).is_ok());
        assert!(assembler.is_complete());

        let assembled = assembler.assemble().unwrap();
        assert!(assembled.is_empty());
    }

    // =========================================================================
    // Multi-Chunk Tests
    // =========================================================================

    #[test]
    fn test_two_chunk_profile_in_order() {
        let mut assembler = IccChunkAssembler::new();
        let chunk1 = create_chunk(1, 2, b"first_half_");
        let chunk2 = create_chunk(2, 2, b"second_half");

        assert!(assembler.add_chunk(&chunk1).is_ok());
        assert!(!assembler.is_complete());
        assert_eq!(assembler.chunk_count(), 1);

        assert!(assembler.add_chunk(&chunk2).is_ok());
        assert!(assembler.is_complete());
        assert_eq!(assembler.chunk_count(), 2);

        let assembled = assembler.assemble().unwrap();
        assert_eq!(assembled, b"first_half_second_half");
    }

    #[test]
    fn test_two_chunk_profile_reverse_order() {
        let mut assembler = IccChunkAssembler::new();
        let chunk1 = create_chunk(1, 2, b"FIRST");
        let chunk2 = create_chunk(2, 2, b"SECOND");

        // Add chunks in reverse order
        assert!(assembler.add_chunk(&chunk2).is_ok());
        assert!(!assembler.is_complete());

        assert!(assembler.add_chunk(&chunk1).is_ok());
        assert!(assembler.is_complete());

        // Assembly should still produce correct order
        let assembled = assembler.assemble().unwrap();
        assert_eq!(assembled, b"FIRSTSECOND");
    }

    #[test]
    fn test_three_chunk_profile() {
        let mut assembler = IccChunkAssembler::new();
        let chunk1 = create_chunk(1, 3, b"AAA");
        let chunk2 = create_chunk(2, 3, b"BBB");
        let chunk3 = create_chunk(3, 3, b"CCC");

        // Add in mixed order: 2, 1, 3
        assert!(assembler.add_chunk(&chunk2).is_ok());
        assert!(assembler.add_chunk(&chunk1).is_ok());
        assert!(assembler.add_chunk(&chunk3).is_ok());

        assert!(assembler.is_complete());
        let assembled = assembler.assemble().unwrap();
        assert_eq!(assembled, b"AAABBBCCC");
    }

    // =========================================================================
    // Validation Error Tests
    // =========================================================================

    #[test]
    fn test_chunk_too_short() {
        let mut assembler = IccChunkAssembler::new();

        // Only identifier, missing chunk number and total
        let short_chunk = b"ICC_PROFILE\0".to_vec();
        let result = assembler.add_chunk(&short_chunk);

        assert!(result.is_err());
        match result.unwrap_err() {
            ExifToolError::ParseError { message, .. } => {
                assert!(message.contains("too short"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_invalid_identifier() {
        let mut assembler = IccChunkAssembler::new();

        // Wrong identifier
        let mut bad_chunk = b"WRONG_IDENT\0".to_vec();
        bad_chunk.push(1);
        bad_chunk.push(1);
        bad_chunk.extend_from_slice(b"data");

        let result = assembler.add_chunk(&bad_chunk);

        assert!(result.is_err());
        match result.unwrap_err() {
            ExifToolError::ParseError { message, .. } => {
                assert!(message.contains("Invalid ICC_PROFILE identifier"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_chunk_number_zero() {
        let mut assembler = IccChunkAssembler::new();
        let chunk = create_chunk(0, 1, b"data"); // Invalid: chunk 0

        let result = assembler.add_chunk(&chunk);

        assert!(result.is_err());
        match result.unwrap_err() {
            ExifToolError::ParseError { message, .. } => {
                assert!(message.contains("cannot be 0"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_total_zero() {
        let mut assembler = IccChunkAssembler::new();

        // Manually create chunk with total = 0
        let mut chunk = ICC_PROFILE_IDENTIFIER.to_vec();
        chunk.push(1); // chunk 1
        chunk.push(0); // total 0 - invalid
        chunk.extend_from_slice(b"data");

        let result = assembler.add_chunk(&chunk);

        assert!(result.is_err());
        match result.unwrap_err() {
            ExifToolError::ParseError { message, .. } => {
                assert!(message.contains("total chunk count cannot be 0"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_chunk_number_exceeds_total() {
        let mut assembler = IccChunkAssembler::new();
        let chunk = create_chunk(5, 3, b"data"); // chunk 5 of 3 - invalid

        let result = assembler.add_chunk(&chunk);

        assert!(result.is_err());
        match result.unwrap_err() {
            ExifToolError::ParseError { message, .. } => {
                assert!(message.contains("exceeds total"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_inconsistent_total_count() {
        let mut assembler = IccChunkAssembler::new();
        let chunk1 = create_chunk(1, 3, b"data1"); // says 3 total
        let chunk2 = create_chunk(2, 5, b"data2"); // says 5 total - inconsistent

        assert!(assembler.add_chunk(&chunk1).is_ok());
        let result = assembler.add_chunk(&chunk2);

        assert!(result.is_err());
        match result.unwrap_err() {
            ExifToolError::ParseError { message, .. } => {
                assert!(message.contains("mismatch"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_duplicate_chunk() {
        let mut assembler = IccChunkAssembler::new();
        let chunk1a = create_chunk(1, 2, b"first version");
        let chunk1b = create_chunk(1, 2, b"second version");

        assert!(assembler.add_chunk(&chunk1a).is_ok());
        let result = assembler.add_chunk(&chunk1b);

        assert!(result.is_err());
        match result.unwrap_err() {
            ExifToolError::ParseError { message, .. } => {
                assert!(message.contains("Duplicate"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    // =========================================================================
    // Assembly Error Tests
    // =========================================================================

    #[test]
    fn test_assemble_no_chunks() {
        let assembler = IccChunkAssembler::new();
        let result = assembler.assemble();

        assert!(result.is_err());
        match result.unwrap_err() {
            ExifToolError::ParseError { message, .. } => {
                assert!(message.contains("no chunks"));
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_assemble_incomplete_profile() {
        let mut assembler = IccChunkAssembler::new();
        let chunk1 = create_chunk(1, 3, b"first");
        let chunk3 = create_chunk(3, 3, b"third");

        // Add chunks 1 and 3, missing chunk 2
        assert!(assembler.add_chunk(&chunk1).is_ok());
        assert!(assembler.add_chunk(&chunk3).is_ok());
        assert!(!assembler.is_complete());

        let result = assembler.assemble();

        assert!(result.is_err());
        match result.unwrap_err() {
            ExifToolError::ParseError { message, .. } => {
                assert!(message.contains("missing"));
                assert!(message.contains("2")); // chunk 2 is missing
            }
            _ => panic!("Expected ParseError"),
        }
    }

    // =========================================================================
    // Clear/Reset Tests
    // =========================================================================

    #[test]
    fn test_clear_resets_state() {
        let mut assembler = IccChunkAssembler::new();
        let chunk = create_chunk(1, 2, b"data");

        assert!(assembler.add_chunk(&chunk).is_ok());
        assert_eq!(assembler.chunk_count(), 1);
        assert_eq!(assembler.expected_total(), Some(2));

        assembler.clear();

        assert_eq!(assembler.chunk_count(), 0);
        assert_eq!(assembler.expected_total(), None);
        assert!(!assembler.is_complete());
    }

    #[test]
    fn test_clear_allows_reuse() {
        let mut assembler = IccChunkAssembler::new();

        // First profile (2 chunks)
        let chunk1a = create_chunk(1, 2, b"profile1_part1");
        let chunk2a = create_chunk(2, 2, b"profile1_part2");
        assembler.add_chunk(&chunk1a).unwrap();
        assembler.add_chunk(&chunk2a).unwrap();

        let profile1 = assembler.assemble().unwrap();
        assert_eq!(profile1, b"profile1_part1profile1_part2");

        // Clear and reuse for second profile (1 chunk)
        assembler.clear();

        let chunk1b = create_chunk(1, 1, b"profile2_complete");
        assembler.add_chunk(&chunk1b).unwrap();

        let profile2 = assembler.assemble().unwrap();
        assert_eq!(profile2, b"profile2_complete");
    }

    // =========================================================================
    // Edge Cases
    // =========================================================================

    #[test]
    fn test_large_chunk_count() {
        let mut assembler = IccChunkAssembler::new();

        // Create profile with 10 chunks
        let total = 10u8;
        for i in 1..=total {
            let data = format!("chunk{:02}", i);
            let chunk = create_chunk(i, total, data.as_bytes());
            assert!(assembler.add_chunk(&chunk).is_ok());
        }

        assert!(assembler.is_complete());
        let assembled = assembler.assemble().unwrap();

        // Verify all chunks are in order
        let expected = "chunk01chunk02chunk03chunk04chunk05chunk06chunk07chunk08chunk09chunk10";
        assert_eq!(assembled, expected.as_bytes());
    }

    #[test]
    fn test_maximum_chunk_number() {
        let mut assembler = IccChunkAssembler::new();

        // Test with max allowed total (255)
        let chunk = create_chunk(255, 255, b"last_chunk");
        assert!(assembler.add_chunk(&chunk).is_ok());
        assert_eq!(assembler.expected_total(), Some(255));
    }

    #[test]
    fn test_binary_data_preservation() {
        let mut assembler = IccChunkAssembler::new();

        // Binary data including null bytes and high-value bytes
        let binary_data1: Vec<u8> = (0..128).collect();
        let binary_data2: Vec<u8> = (128..=255).collect();

        let chunk1 = create_chunk(1, 2, &binary_data1);
        let chunk2 = create_chunk(2, 2, &binary_data2);

        assembler.add_chunk(&chunk1).unwrap();
        assembler.add_chunk(&chunk2).unwrap();

        let assembled = assembler.assemble().unwrap();

        // Verify all 256 byte values are present in order
        let expected: Vec<u8> = (0..=255).collect();
        assert_eq!(assembled, expected);
    }

    // =========================================================================
    // Debug Trait Test
    // =========================================================================

    #[test]
    fn test_debug_output() {
        let assembler = IccChunkAssembler::new();
        let debug_str = format!("{:?}", assembler);
        assert!(debug_str.contains("IccChunkAssembler"));
    }
}
