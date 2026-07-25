//! FFI tag reading functions
//!
//! Functions for reading metadata from files and accessing tag values.

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::ptr;

use crate::core::operations::read_metadata;

use super::context::{ExifToolContext, ExifToolHandle, handle_to_context, handle_to_context_mut};
use super::error::{
    EXIFTOOL_ERR_INTERNAL, EXIFTOOL_ERR_INVALID_TAG_VALUE, EXIFTOOL_ERR_NULL_POINTER,
    EXIFTOOL_ERR_TAG_NOT_FOUND, EXIFTOOL_OK, error_to_code, set_last_error,
};

// ============================================================================
// Metadata Reading Functions
// ============================================================================

/// Reads metadata from a file.
///
/// # Arguments
/// - `handle`: Handle to store metadata in (must not be NULL)
/// - `filepath`: Path to file (null-terminated UTF-8 string, must not be NULL)
///
/// # Returns
/// - `EXIFTOOL_OK` (0) on success
/// - Error code on failure
///
/// # Errors
/// - `EXIFTOOL_ERR_NULL_POINTER`: handle or filepath is NULL
/// - `EXIFTOOL_ERR_IO`: File not found, permission denied, or read error
/// - `EXIFTOOL_ERR_UNSUPPORTED_FORMAT`: File format not recognized
/// - `EXIFTOOL_ERR_PARSE`: File is corrupted or malformed
///
/// # Thread Safety
/// Not thread-safe. Do not call concurrently on the same handle.
#[unsafe(no_mangle)]
pub extern "C" fn exiftool_read_file(
    handle: *mut ExifToolHandle,
    filepath: *const c_char,
) -> c_int {
    // Catch panics at FFI boundary
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        // Check for NULL pointers
        if handle.is_null() || filepath.is_null() {
            set_last_error("NULL pointer provided".to_string());
            return EXIFTOOL_ERR_NULL_POINTER;
        }

        // Get mutable context
        let context = match handle_to_context_mut(handle) {
            Some(ctx) => ctx,
            None => {
                set_last_error("Invalid handle".to_string());
                return EXIFTOOL_ERR_NULL_POINTER;
            }
        };

        // Convert C string to Rust string
        let path_str = match CStr::from_ptr(filepath).to_str() {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("Invalid UTF-8 in file path: {}", e));
                return EXIFTOOL_ERR_INVALID_TAG_VALUE;
            }
        };

        let path = Path::new(path_str);

        // Call Rust read_metadata function
        match read_metadata(path) {
            Ok(metadata) => {
                // Clear string cache before loading new data
                context.clear_string_cache();
                context.metadata = metadata;
                // Rebuild tag cache for iteration
                context.rebuild_tag_cache();
                EXIFTOOL_OK
            }
            Err(e) => error_to_code(&e),
        }
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            set_last_error("Internal error: unexpected panic".to_string());
            EXIFTOOL_ERR_INTERNAL
        }
    }
}

/// Returns the number of tags in the metadata.
///
/// # Arguments
/// - `handle`: Handle to query (can be NULL)
///
/// # Returns
/// Number of tags (0 if handle is NULL or no metadata loaded)
///
/// # Thread Safety
/// Thread-safe for read-only access.
#[unsafe(no_mangle)]
pub extern "C" fn exiftool_get_tag_count(handle: *const ExifToolHandle) -> usize {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        if handle.is_null() {
            return 0;
        }

        let context = match handle_to_context(handle) {
            Some(ctx) => ctx,
            None => return 0,
        };

        context.metadata.len()
    }));

    result.unwrap_or(0)
}

/// Retrieves tag name by index.
///
/// # Arguments
/// - `handle`: Handle to query (must not be NULL)
/// - `index`: Zero-based index (must be less than tag count)
///
/// # Returns
/// - Pointer to null-terminated tag name string
/// - NULL if index is out of bounds or handle is NULL
///
/// # String Lifetime
/// Returned string is valid until:
/// - Next API call on same handle
/// - Handle destruction
///
/// # Thread Safety
/// Thread-safe for read-only access.
#[unsafe(no_mangle)]
pub extern "C" fn exiftool_get_tag_name_at(
    handle: *const ExifToolHandle,
    index: usize,
) -> *const c_char {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        if handle.is_null() {
            return ptr::null();
        }

        let context = match handle_to_context(handle) {
            Some(ctx) => ctx,
            None => return ptr::null(),
        };

        // Use cached tag names for stable ordering
        if index >= context.tag_names_cache.len() {
            return ptr::null();
        }

        let tag_name = &context.tag_names_cache[index];

        // Convert to CString and cache it
        match CString::new(tag_name.as_str()) {
            Ok(cstr) => {
                // SAFETY: We need to cast away the const to cache the string
                // This is safe because we only return a const pointer to C
                let ctx_mut = &mut *(handle as *mut ExifToolContext);
                ctx_mut.cache_string(cstr)
            }
            Err(_) => ptr::null(),
        }
    }));

    result.unwrap_or(ptr::null())
}

/// Checks if a tag exists.
///
/// # Arguments
/// - `handle`: Handle to query (can be NULL)
/// - `tag_name`: Tag name to check (must not be NULL)
///
/// # Returns
/// - 1 if tag exists
/// - 0 if tag does not exist, handle is NULL, or tag_name is NULL
///
/// # Thread Safety
/// Thread-safe for read-only access.
#[unsafe(no_mangle)]
pub extern "C" fn exiftool_has_tag(
    handle: *const ExifToolHandle,
    tag_name: *const c_char,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        if handle.is_null() || tag_name.is_null() {
            return 0;
        }

        let context = match handle_to_context(handle) {
            Some(ctx) => ctx,
            None => return 0,
        };

        let name_str = match CStr::from_ptr(tag_name).to_str() {
            Ok(s) => s,
            Err(_) => return 0,
        };

        if context.metadata.contains_key(name_str) {
            1
        } else {
            0
        }
    }));

    result.unwrap_or(0)
}

// ============================================================================
// Tag Access Functions
// ============================================================================

/// Retrieves tag value as a string.
///
/// # Arguments
/// - `handle`: Handle to query (must not be NULL)
/// - `tag_name`: Tag name (must not be NULL)
///
/// # Returns
/// - Pointer to null-terminated UTF-8 string
/// - NULL if tag doesn't exist or is not a String type
///
/// # String Lifetime
/// Returned string is valid until:
/// - Next API call on same handle
/// - Handle destruction
///
/// # Thread Safety
/// Thread-safe for read-only access.
#[unsafe(no_mangle)]
pub extern "C" fn exiftool_get_tag_string(
    handle: *const ExifToolHandle,
    tag_name: *const c_char,
) -> *const c_char {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        if handle.is_null() || tag_name.is_null() {
            return ptr::null();
        }

        let context = match handle_to_context(handle) {
            Some(ctx) => ctx,
            None => return ptr::null(),
        };

        let name_str = match CStr::from_ptr(tag_name).to_str() {
            Ok(s) => s,
            Err(_) => return ptr::null(),
        };

        // Get tag value and check if it's a string
        let value_str = match context.metadata.get_string(name_str) {
            Some(s) => s,
            None => return ptr::null(),
        };

        // Convert to CString and cache it
        match CString::new(value_str) {
            Ok(cstr) => {
                // SAFETY: Cast away const to cache string
                let ctx_mut = &mut *(handle as *mut ExifToolContext);
                ctx_mut.cache_string(cstr)
            }
            Err(_) => ptr::null(),
        }
    }));

    result.unwrap_or(ptr::null())
}

/// Retrieves tag value as a 64-bit integer.
///
/// # Arguments
/// - `handle`: Handle to query (must not be NULL)
/// - `tag_name`: Tag name (must not be NULL)
/// - `out_value`: Pointer to output variable (must not be NULL)
///
/// # Returns
/// - `EXIFTOOL_OK` on success
/// - `EXIFTOOL_ERR_TAG_NOT_FOUND` if tag doesn't exist
/// - `EXIFTOOL_ERR_INVALID_TAG_VALUE` if tag is not an Integer type
/// - `EXIFTOOL_ERR_NULL_POINTER` if any parameter is NULL
///
/// # Thread Safety
/// Thread-safe for read-only access.
#[unsafe(no_mangle)]
pub extern "C" fn exiftool_get_tag_integer(
    handle: *const ExifToolHandle,
    tag_name: *const c_char,
    out_value: *mut i64,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        if handle.is_null() || tag_name.is_null() || out_value.is_null() {
            set_last_error("NULL pointer provided".to_string());
            return EXIFTOOL_ERR_NULL_POINTER;
        }

        let context = match handle_to_context(handle) {
            Some(ctx) => ctx,
            None => {
                set_last_error("Invalid handle".to_string());
                return EXIFTOOL_ERR_NULL_POINTER;
            }
        };

        let name_str = match CStr::from_ptr(tag_name).to_str() {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("Invalid UTF-8 in tag name: {}", e));
                return EXIFTOOL_ERR_INVALID_TAG_VALUE;
            }
        };

        // Check if tag exists
        match context.metadata.get(name_str) {
            None => {
                set_last_error(format!("Tag not found: {}", name_str));
                EXIFTOOL_ERR_TAG_NOT_FOUND
            }
            Some(value) => {
                // Check if it's an integer
                match value.as_integer() {
                    Some(int_val) => {
                        *out_value = int_val;
                        EXIFTOOL_OK
                    }
                    None => {
                        set_last_error(format!("Tag '{}' is not an Integer type", name_str));
                        EXIFTOOL_ERR_INVALID_TAG_VALUE
                    }
                }
            }
        }
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            set_last_error("Internal error: unexpected panic".to_string());
            EXIFTOOL_ERR_INTERNAL
        }
    }
}

/// Retrieves tag value as a double-precision float.
///
/// # Arguments
/// - `handle`: Handle to query (must not be NULL)
/// - `tag_name`: Tag name (must not be NULL)
/// - `out_value`: Pointer to output variable (must not be NULL)
///
/// # Returns
/// - `EXIFTOOL_OK` on success
/// - `EXIFTOOL_ERR_TAG_NOT_FOUND` if tag doesn't exist
/// - `EXIFTOOL_ERR_INVALID_TAG_VALUE` if tag is not a Float type
/// - `EXIFTOOL_ERR_NULL_POINTER` if any parameter is NULL
///
/// # Thread Safety
/// Thread-safe for read-only access.
#[unsafe(no_mangle)]
pub extern "C" fn exiftool_get_tag_float(
    handle: *const ExifToolHandle,
    tag_name: *const c_char,
    out_value: *mut f64,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        if handle.is_null() || tag_name.is_null() || out_value.is_null() {
            set_last_error("NULL pointer provided".to_string());
            return EXIFTOOL_ERR_NULL_POINTER;
        }

        let context = match handle_to_context(handle) {
            Some(ctx) => ctx,
            None => {
                set_last_error("Invalid handle".to_string());
                return EXIFTOOL_ERR_NULL_POINTER;
            }
        };

        let name_str = match CStr::from_ptr(tag_name).to_str() {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("Invalid UTF-8 in tag name: {}", e));
                return EXIFTOOL_ERR_INVALID_TAG_VALUE;
            }
        };

        // Check if tag exists
        match context.metadata.get(name_str) {
            None => {
                set_last_error(format!("Tag not found: {}", name_str));
                EXIFTOOL_ERR_TAG_NOT_FOUND
            }
            Some(value) => {
                // Check if it's a float
                match value.as_float() {
                    Some(float_val) => {
                        *out_value = float_val;
                        EXIFTOOL_OK
                    }
                    None => {
                        set_last_error(format!("Tag '{}' is not a Float type", name_str));
                        EXIFTOOL_ERR_INVALID_TAG_VALUE
                    }
                }
            }
        }
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            set_last_error("Internal error: unexpected panic".to_string());
            EXIFTOOL_ERR_INTERNAL
        }
    }
}
