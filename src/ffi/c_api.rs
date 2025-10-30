//! C FFI bindings
//!
//! This module provides C-compatible function exports for the library.
//!
//! # Safety
//!
//! All functions are designed to be safe to call from C. Panics are caught at the FFI
//! boundary and converted to error codes. However, callers must follow the documented
//! contracts (e.g., not passing NULL where prohibited, destroying handles properly).

// Clippy warning about raw pointer dereferencing in public functions is not applicable
// to FFI functions - they must accept raw pointers and cannot be marked `unsafe` on the
// Rust side because they're called from C.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::ptr;

use crate::core::operations::{read_metadata, write_metadata};
use crate::core::{MetadataMap, TagValue};
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
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Sets the last error message for the current thread.
fn set_last_error(msg: String) {
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = msg;
    });
}

/// Converts a Rust ExifToolError to a C error code and sets the error message.
fn error_to_code(err: &ExifToolError) -> c_int {
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
// Internal Context Structure
// ============================================================================

/// Internal context structure that holds all state for a handle.
/// This is the Rust object behind the opaque ExifToolHandle pointer.
struct ExifToolContext {
    /// The metadata map containing all loaded tags
    metadata: MetadataMap,
    /// Cache of CString instances for string returns
    /// This ensures strings remain valid until the next API call
    string_cache: Vec<CString>,
    /// Iterator cache: stores tag names for iteration
    tag_names_cache: Vec<String>,
}

impl ExifToolContext {
    /// Creates a new empty context
    fn new() -> Self {
        Self {
            metadata: MetadataMap::new(),
            string_cache: Vec::new(),
            tag_names_cache: Vec::new(),
        }
    }

    /// Clears the string cache to free memory
    fn clear_string_cache(&mut self) {
        self.string_cache.clear();
    }

    /// Caches a CString and returns a pointer to it
    fn cache_string(&mut self, s: CString) -> *const c_char {
        let ptr = s.as_ptr();
        self.string_cache.push(s);
        ptr
    }

    /// Rebuilds the tag names cache for iteration
    fn rebuild_tag_cache(&mut self) {
        self.tag_names_cache = self.metadata.keys().cloned().collect();
    }
}

// ============================================================================
// Opaque Handle Type
// ============================================================================

/// Opaque handle type for C API.
/// C code receives a pointer to this type but cannot access its contents.
#[repr(C)]
pub struct ExifToolHandle {
    _private: [u8; 0],
}

/// Converts a raw pointer back to a reference.
/// Returns None if the pointer is NULL.
///
/// # Safety
/// The pointer must be a valid pointer previously created by `Box::into_raw()`
/// and not yet reclaimed.
unsafe fn handle_to_context<'a>(handle: *const ExifToolHandle) -> Option<&'a ExifToolContext> {
    if handle.is_null() {
        None
    } else {
        Some(&*(handle as *const ExifToolContext))
    }
}

/// Converts a raw pointer back to a mutable reference.
/// Returns None if the pointer is NULL.
///
/// # Safety
/// The pointer must be a valid pointer previously created by `Box::into_raw()`
/// and not yet reclaimed.
unsafe fn handle_to_context_mut<'a>(
    handle: *mut ExifToolHandle,
) -> Option<&'a mut ExifToolContext> {
    if handle.is_null() {
        None
    } else {
        Some(&mut *(handle as *mut ExifToolContext))
    }
}

// ============================================================================
// Handle Lifecycle Functions
// ============================================================================

/// Creates a new ExifTool handle.
///
/// # Returns
/// - Pointer to opaque handle on success
/// - NULL if allocation fails (out of memory)
///
/// # Memory
/// The returned handle must be destroyed with `exiftool_destroy()` to prevent memory leaks.
///
/// # Thread Safety
/// This function is thread-safe. Each call returns an independent handle.
#[no_mangle]
pub extern "C" fn exiftool_create() -> *mut ExifToolHandle {
    // Catch any panics during allocation
    let result = catch_unwind(|| {
        let context = Box::new(ExifToolContext::new());
        Box::into_raw(context) as *mut ExifToolHandle
    });

    match result {
        Ok(handle) => handle,
        Err(_) => {
            // Panic during allocation - return NULL
            ptr::null_mut()
        }
    }
}

/// Destroys a handle and frees all associated resources.
///
/// # Arguments
/// - `handle`: Handle to destroy (can be NULL, in which case this is a no-op)
///
/// # Safety
/// After this call, the handle pointer is invalid and must not be used.
/// Passing NULL is safe and does nothing.
///
/// # Thread Safety
/// Not thread-safe. Do not call concurrently with other operations on the same handle.
#[no_mangle]
pub extern "C" fn exiftool_destroy(handle: *mut ExifToolHandle) {
    if handle.is_null() {
        return;
    }

    // Catch any panics during destruction
    let _ = catch_unwind(AssertUnwindSafe(|| unsafe {
        // Reclaim ownership of the Box and let it drop
        let _ = Box::from_raw(handle as *mut ExifToolContext);
    }));
}

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
#[no_mangle]
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
#[no_mangle]
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
#[no_mangle]
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
#[no_mangle]
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
#[no_mangle]
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
#[no_mangle]
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
#[no_mangle]
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

// ============================================================================
// Metadata Writing Functions
// ============================================================================

/// Sets a tag value to a string.
///
/// # Arguments
/// - `handle`: Handle to modify (must not be NULL)
/// - `tag_name`: Tag name (must not be NULL)
/// - `value`: String value to set (null-terminated UTF-8, must not be NULL)
///
/// # Returns
/// - `EXIFTOOL_OK` on success
/// - `EXIFTOOL_ERR_NULL_POINTER` if any parameter is NULL
///
/// # Thread Safety
/// Not thread-safe. Do not call concurrently on the same handle.
#[no_mangle]
pub extern "C" fn exiftool_set_tag_string(
    handle: *mut ExifToolHandle,
    tag_name: *const c_char,
    value: *const c_char,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        if handle.is_null() || tag_name.is_null() || value.is_null() {
            set_last_error("NULL pointer provided".to_string());
            return EXIFTOOL_ERR_NULL_POINTER;
        }

        let context = match handle_to_context_mut(handle) {
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

        let value_str = match CStr::from_ptr(value).to_str() {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("Invalid UTF-8 in value: {}", e));
                return EXIFTOOL_ERR_INVALID_TAG_VALUE;
            }
        };

        // Set the tag
        context.metadata.insert(
            name_str.to_string(),
            TagValue::new_string(value_str.to_string()),
        );
        // Rebuild tag cache since we modified metadata
        context.rebuild_tag_cache();

        EXIFTOOL_OK
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            set_last_error("Internal error: unexpected panic".to_string());
            EXIFTOOL_ERR_INTERNAL
        }
    }
}

/// Sets a tag value to an integer.
///
/// # Arguments
/// - `handle`: Handle to modify (must not be NULL)
/// - `tag_name`: Tag name (must not be NULL)
/// - `value`: Integer value to set
///
/// # Returns
/// - `EXIFTOOL_OK` on success
/// - `EXIFTOOL_ERR_NULL_POINTER` if handle or tag_name is NULL
///
/// # Thread Safety
/// Not thread-safe.
#[no_mangle]
pub extern "C" fn exiftool_set_tag_integer(
    handle: *mut ExifToolHandle,
    tag_name: *const c_char,
    value: i64,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        if handle.is_null() || tag_name.is_null() {
            set_last_error("NULL pointer provided".to_string());
            return EXIFTOOL_ERR_NULL_POINTER;
        }

        let context = match handle_to_context_mut(handle) {
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

        // Set the tag
        context
            .metadata
            .insert(name_str.to_string(), TagValue::new_integer(value));
        // Rebuild tag cache since we modified metadata
        context.rebuild_tag_cache();

        EXIFTOOL_OK
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            set_last_error("Internal error: unexpected panic".to_string());
            EXIFTOOL_ERR_INTERNAL
        }
    }
}

/// Sets a tag value to a floating-point number.
///
/// # Arguments
/// - `handle`: Handle to modify (must not be NULL)
/// - `tag_name`: Tag name (must not be NULL)
/// - `value`: Float value to set
///
/// # Returns
/// - `EXIFTOOL_OK` on success
/// - `EXIFTOOL_ERR_NULL_POINTER` if handle or tag_name is NULL
/// - `EXIFTOOL_ERR_INVALID_TAG_VALUE` if value is NaN or infinity
///
/// # Thread Safety
/// Not thread-safe.
#[no_mangle]
pub extern "C" fn exiftool_set_tag_float(
    handle: *mut ExifToolHandle,
    tag_name: *const c_char,
    value: f64,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        if handle.is_null() || tag_name.is_null() {
            set_last_error("NULL pointer provided".to_string());
            return EXIFTOOL_ERR_NULL_POINTER;
        }

        // Validate float value
        if value.is_nan() || value.is_infinite() {
            set_last_error("Float value cannot be NaN or infinity".to_string());
            return EXIFTOOL_ERR_INVALID_TAG_VALUE;
        }

        let context = match handle_to_context_mut(handle) {
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

        // Set the tag
        context
            .metadata
            .insert(name_str.to_string(), TagValue::new_float(value));
        // Rebuild tag cache since we modified metadata
        context.rebuild_tag_cache();

        EXIFTOOL_OK
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            set_last_error("Internal error: unexpected panic".to_string());
            EXIFTOOL_ERR_INTERNAL
        }
    }
}

/// Removes a tag from the metadata.
///
/// # Arguments
/// - `handle`: Handle to modify (must not be NULL)
/// - `tag_name`: Tag name to remove (must not be NULL)
///
/// # Returns
/// - `EXIFTOOL_OK` (always succeeds, even if tag didn't exist)
/// - `EXIFTOOL_ERR_NULL_POINTER` if handle or tag_name is NULL
///
/// # Thread Safety
/// Not thread-safe.
#[no_mangle]
pub extern "C" fn exiftool_remove_tag(
    handle: *mut ExifToolHandle,
    tag_name: *const c_char,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        if handle.is_null() || tag_name.is_null() {
            set_last_error("NULL pointer provided".to_string());
            return EXIFTOOL_ERR_NULL_POINTER;
        }

        let context = match handle_to_context_mut(handle) {
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

        // Remove the tag (no error if it doesn't exist)
        context.metadata.remove(name_str);
        // Rebuild tag cache since we modified metadata
        context.rebuild_tag_cache();

        EXIFTOOL_OK
    }));

    match result {
        Ok(code) => code,
        Err(_) => {
            set_last_error("Internal error: unexpected panic".to_string());
            EXIFTOOL_ERR_INTERNAL
        }
    }
}

/// Writes metadata to a file.
///
/// # Arguments
/// - `handle`: Handle containing metadata to write (must not be NULL)
/// - `filepath`: Path to file to write (null-terminated UTF-8, must not be NULL)
///
/// # Returns
/// - `EXIFTOOL_OK` on success
/// - Error code on failure
///
/// # Errors
/// - `EXIFTOOL_ERR_NULL_POINTER`: handle or filepath is NULL
/// - `EXIFTOOL_ERR_IO`: File not writable, disk full, permission denied
/// - `EXIFTOOL_ERR_UNSUPPORTED_FORMAT`: File format doesn't support writing
/// - `EXIFTOOL_ERR_INVALID_TAG_VALUE`: Metadata validation failed
///
/// # Thread Safety
/// Thread-safe for read-only access to handle.
#[no_mangle]
pub extern "C" fn exiftool_write_file(
    handle: *const ExifToolHandle,
    filepath: *const c_char,
) -> c_int {
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        if handle.is_null() || filepath.is_null() {
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

        let path_str = match CStr::from_ptr(filepath).to_str() {
            Ok(s) => s,
            Err(e) => {
                set_last_error(format!("Invalid UTF-8 in file path: {}", e));
                return EXIFTOOL_ERR_INVALID_TAG_VALUE;
            }
        };

        let path = Path::new(path_str);

        // Call Rust write_metadata function
        match write_metadata(path, &context.metadata) {
            Ok(()) => EXIFTOOL_OK,
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

// ============================================================================
// Error Handling Functions
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
#[no_mangle]
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
