//! FFI error codes and error handling utilities
//!
//! This module defines error codes returned by C API functions
//! and provides thread-local error message storage.

use std::cell::RefCell;
use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::panic::catch_unwind;

use crate::error::ExifToolError;

// ============================================================================
// Error Codes
// ============================================================================

/// Success code
pub const EXIFTOOL_OK: c_int = 0;
/// I/O error (file not found, permission denied, etc.)
pub const EXIFTOOL_ERR_IO: c_int = 1;
/// Parse error (corrupted file, invalid format)
pub const EXIFTOOL_ERR_PARSE: c_int = 2;
/// Tag not found
pub const EXIFTOOL_ERR_TAG_NOT_FOUND: c_int = 3;
/// Invalid tag value (type mismatch, out of range)
pub const EXIFTOOL_ERR_INVALID_TAG_VALUE: c_int = 4;
/// Unsupported format
pub const EXIFTOOL_ERR_UNSUPPORTED_FORMAT: c_int = 5;
/// NULL pointer provided
pub const EXIFTOOL_ERR_NULL_POINTER: c_int = 6;
/// Internal error (panic caught)
pub const EXIFTOOL_ERR_INTERNAL: c_int = 99;

// ============================================================================
// Thread-Local Error Storage
// ============================================================================

thread_local! {
    /// Thread-local storage for the last error message.
    /// Each thread maintains its own error state for thread-safety.
    pub static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Sets the last error message for the current thread.
pub fn set_last_error(msg: String) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = msg;
    });
}

/// Converts a Rust ExifToolError to a C error code and sets the error message.
pub fn error_to_code(err: &ExifToolError) -> c_int {
    let (code, msg) = match err {
        ExifToolError::IoError(e) => (EXIFTOOL_ERR_IO, format!("I/O error: {}", e)),
        ExifToolError::ParseError { message, offset } => {
            let msg = if let Some(off) = offset {
                format!("Parse error at offset {}: {}", off, message)
            } else {
                format!("Parse error: {}", message)
            };
            (EXIFTOOL_ERR_PARSE, msg)
        }
        ExifToolError::TagNotFound { tag_name } => (
            EXIFTOOL_ERR_TAG_NOT_FOUND,
            format!("Tag not found: {}", tag_name),
        ),
        ExifToolError::InvalidTagValue { tag_name, reason } => (
            EXIFTOOL_ERR_INVALID_TAG_VALUE,
            format!("Invalid value for tag '{}': {}", tag_name, reason),
        ),
        ExifToolError::UnsupportedFormat { message } => (
            EXIFTOOL_ERR_UNSUPPORTED_FORMAT,
            format!("Unsupported format: {}", message),
        ),
    };
    set_last_error(msg);
    code
}

// ============================================================================
// Error Retrieval Function
// ============================================================================

/// Retrieves the last error message.
///
/// # Returns
/// Pointer to null-terminated error message string.
/// Never returns NULL (returns "No error" if no error occurred).
///
/// # String Lifetime
/// The returned string is valid until:
/// - Next API call that sets an error on the same thread
/// - Thread termination
///
/// # Thread Safety
/// Thread-safe. Each thread has its own error message.
#[unsafe(no_mangle)]
pub extern "C" fn exiftool_get_last_error() -> *const c_char {
    // This function must never panic
    let result = catch_unwind(|| {
        LAST_ERROR.with(|e| {
            let err_msg = e.borrow();
            let msg = if err_msg.is_empty() {
                "No error"
            } else {
                err_msg.as_str()
            };

            // Create a static CString that lives for the lifetime of the thread
            // We use a thread-local cache for this
            thread_local! {
                static ERROR_CSTRING: RefCell<CString> = RefCell::new(CString::new("").unwrap());
            }

            ERROR_CSTRING.with(|cache| {
                *cache.borrow_mut() = CString::new(msg)
                    .unwrap_or_else(|_| CString::new("Error message contains null byte").unwrap());
                cache.borrow().as_ptr()
            })
        })
    });

    result.unwrap_or_else(|_| {
        // If we panic here, return a static string
        c"Internal error in error handler".as_ptr()
    })
}
