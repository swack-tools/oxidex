//! Atomic file writing
//!
//! This module provides atomic file write operations using the temp-file-and-rename pattern.
//! This ensures that files are never left in a partially-written or corrupted state, even if
//! the program crashes or the system loses power during a write operation.
//!
//! # Safety Guarantees
//!
//! The atomic write pattern guarantees:
//!
//! 1. **Atomicity**: The file rename operation is atomic at the OS level - it either fully
//!    succeeds or fully fails with no intermediate state.
//! 2. **Durability**: Data is flushed to disk (via `fsync`) before the rename, ensuring
//!    persistence even in case of sudden power loss.
//! 3. **Cleanup**: Temporary files are automatically cleaned up on failure.
//!
//! # Example
//!
//! ```rust,no_run
//! use std::path::Path;
//! use exiftool_rs::writers::atomic_writer::write_atomic;
//!
//! let path = Path::new("/path/to/file.jpg");
//! let data = b"new file contents";
//!
//! write_atomic(path, data).expect("Failed to write file atomically");
//! ```

#![allow(dead_code)]

use crate::error::Result;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

/// Writes data to a file atomically.
///
/// This function implements the atomic write pattern by:
///
/// 1. Creating a temporary file in the same directory as the target file
/// 2. Writing all data to the temporary file
/// 3. Calling `fsync()` to ensure data is physically written to disk
/// 4. Atomically renaming the temporary file to the target path
///
/// The temporary file is created in the same directory as the target to ensure
/// the rename operation is atomic. Cross-filesystem renames are not atomic and
/// would break the safety guarantees.
///
/// # Parameters
///
/// * `path` - The target file path to write to. If the file exists, it will be
///   atomically replaced. The parent directory must exist.
/// * `data` - The bytes to write to the file.
///
/// # Returns
///
/// Returns `Ok(())` on success. The target file will contain exactly the provided
/// data, and any previous contents will be replaced.
///
/// # Errors
///
/// This function will return an error if:
///
/// * The parent directory of `path` does not exist
/// * There are insufficient permissions to create files in the parent directory
/// * There are insufficient permissions to write to the target path
/// * The disk is full or the write operation fails
/// * The `fsync()` operation fails
/// * The rename operation fails
///
/// In all error cases, the temporary file is automatically cleaned up and the
/// original target file (if it existed) remains unchanged.
///
/// # Example
///
/// ```rust,no_run
/// use std::path::Path;
/// use exiftool_rs::writers::atomic_writer::write_atomic;
///
/// let path = Path::new("output.jpg");
/// let metadata = b"EXIF data...";
///
/// match write_atomic(path, metadata) {
///     Ok(()) => println!("File written successfully"),
///     Err(e) => eprintln!("Failed to write file: {}", e),
/// }
/// ```
///
/// # Panics
///
/// This function does not panic under normal circumstances. All errors are
/// returned as `Result::Err`.
pub fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    // Get the parent directory where the temp file will be created.
    // This is required for atomic rename - both files must be on the same filesystem.
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Path has no parent directory: {}", path.display()),
        )
    })?;

    // Create a temporary file in the same directory as the target.
    // The tempfile crate generates a unique name automatically.
    let mut temp_file = NamedTempFile::new_in(parent)?;

    // Write all data to the temporary file.
    // The ? operator converts io::Error to ExifToolError automatically.
    temp_file.write_all(data)?;

    // Flush data to disk (fsync). This ensures that even if the system crashes
    // immediately after this point, the data is safely on disk.
    // sync_all() flushes both data and metadata.
    temp_file.as_file().sync_all()?;

    // Atomically rename the temporary file to the target path.
    // The persist() method handles the rename and returns PersistError on failure.
    // We convert PersistError to io::Error which then gets converted to ExifToolError.
    temp_file.persist(path).map_err(|e| e.error)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Read;

    #[test]
    fn test_write_atomic_success() {
        // Create a temporary directory for testing
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        let data = b"Hello, World!";

        // Write data atomically
        write_atomic(&test_file, data).unwrap();

        // Verify the file exists and contains the correct data
        let mut contents = Vec::new();
        fs::File::open(&test_file)
            .unwrap()
            .read_to_end(&mut contents)
            .unwrap();
        assert_eq!(contents, data);
    }

    #[test]
    fn test_write_atomic_overwrites_existing_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("existing.txt");

        // Create initial file with some data
        let initial_data = b"Initial content";
        fs::write(&test_file, initial_data).unwrap();

        // Atomically overwrite with new data
        let new_data = b"New content that is different";
        write_atomic(&test_file, new_data).unwrap();

        // Verify the file was overwritten
        let contents = fs::read(&test_file).unwrap();
        assert_eq!(contents, new_data);
    }

    #[test]
    fn test_write_atomic_empty_data() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("empty.txt");
        let data = b"";

        // Writing empty data should succeed
        write_atomic(&test_file, data).unwrap();

        // Verify the file exists and is empty
        let contents = fs::read(&test_file).unwrap();
        assert_eq!(contents, data);
        assert_eq!(contents.len(), 0);
    }

    #[test]
    fn test_write_atomic_large_data() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("large.bin");

        // Create a large data buffer (1 MB)
        let data: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();

        // Write large data atomically
        write_atomic(&test_file, &data).unwrap();

        // Verify the entire file was written correctly
        let contents = fs::read(&test_file).unwrap();
        assert_eq!(contents.len(), data.len());
        assert_eq!(contents, data);
    }

    #[test]
    fn test_write_atomic_nonexistent_parent_directory() {
        // Try to write to a file in a directory that doesn't exist
        let nonexistent_path = Path::new("/nonexistent/directory/file.txt");
        let data = b"test data";

        // This should fail because the parent directory doesn't exist
        let result = write_atomic(nonexistent_path, data);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_atomic_path_without_parent() {
        // Try to write to a path that has no parent (like "/" on Unix)
        // This should fail with an appropriate error
        let data = b"test data";

        // On Unix, this would be root; on Windows, a drive letter
        #[cfg(unix)]
        let path_without_parent = Path::new("/");

        #[cfg(windows)]
        let path_without_parent = Path::new("C:\\");

        let result = write_atomic(path_without_parent, data);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_atomic_creates_temp_in_same_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("test.txt");
        let data = b"Test data";

        // Before writing, the directory should only contain nothing
        let entries_before: Vec<_> = fs::read_dir(temp_dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(entries_before.len(), 0);

        // Write the file
        write_atomic(&test_file, data).unwrap();

        // After writing, only the target file should exist (temp file should be gone)
        let entries_after: Vec<_> = fs::read_dir(temp_dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(entries_after.len(), 1);
        assert_eq!(entries_after[0], test_file);
    }

    #[test]
    fn test_write_atomic_preserves_data_integrity() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("integrity.bin");

        // Write data with specific patterns
        let data: Vec<u8> = (0..10000).map(|i| ((i * 7) % 256) as u8).collect();
        write_atomic(&test_file, &data).unwrap();

        // Read back and verify every byte
        let contents = fs::read(&test_file).unwrap();
        assert_eq!(contents.len(), data.len());

        for (i, (&expected, &actual)) in data.iter().zip(contents.iter()).enumerate() {
            assert_eq!(
                expected, actual,
                "Byte mismatch at position {}: expected {}, got {}",
                i, expected, actual
            );
        }
    }

    #[test]
    fn test_write_atomic_unicode_in_filename() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("测试文件.txt");
        let data = b"Unicode filename test";

        // Writing to a file with Unicode characters in the name should work
        write_atomic(&test_file, data).unwrap();

        let contents = fs::read(&test_file).unwrap();
        assert_eq!(contents, data);
    }

    #[test]
    fn test_write_atomic_binary_data() {
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file = temp_dir.path().join("binary.dat");

        // Test with all possible byte values
        let data: Vec<u8> = (0u8..=255).collect();
        write_atomic(&test_file, &data).unwrap();

        let contents = fs::read(&test_file).unwrap();
        assert_eq!(contents, data);
    }
}
