//! FFI context and handle types
//!
//! This module defines the internal context structure that holds all state
//! for a handle, and the opaque handle type exposed to C.

use std::ffi::CString;
use std::os::raw::c_char;

use crate::core::MetadataMap;

// ============================================================================
// Internal Context Structure
// ============================================================================

/// Internal context structure that holds all state for a handle.
/// This is the Rust object behind the opaque ExifToolHandle pointer.
pub struct ExifToolContext {
    /// The metadata map containing all loaded tags
    pub metadata: MetadataMap,
    /// Cache of CString instances for string returns
    /// This ensures strings remain valid until the next API call
    pub string_cache: Vec<CString>,
    /// Iterator cache: stores tag names for iteration
    pub tag_names_cache: Vec<String>,
}

impl ExifToolContext {
    /// Creates a new empty context
    pub fn new() -> Self {
        Self {
            metadata: MetadataMap::new(),
            string_cache: Vec::new(),
            tag_names_cache: Vec::new(),
        }
    }

    /// Clears the string cache to free memory
    pub fn clear_string_cache(&mut self) {
        self.string_cache.clear();
    }

    /// Caches a CString and returns a pointer to it
    pub fn cache_string(&mut self, s: CString) -> *const c_char {
        let ptr = s.as_ptr();
        self.string_cache.push(s);
        ptr
    }

    /// Rebuilds the tag names cache for iteration
    pub fn rebuild_tag_cache(&mut self) {
        self.tag_names_cache = self.metadata.keys().cloned().collect();
    }
}

impl Default for ExifToolContext {
    fn default() -> Self {
        Self::new()
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
pub unsafe fn handle_to_context<'a>(handle: *const ExifToolHandle) -> Option<&'a ExifToolContext> {
    if handle.is_null() {
        None
    } else {
        // SAFETY: Caller guarantees handle is a valid pointer from Box::into_raw()
        Some(unsafe { &*(handle as *const ExifToolContext) })
    }
}

/// Converts a raw pointer back to a mutable reference.
/// Returns None if the pointer is NULL.
///
/// # Safety
/// The pointer must be a valid pointer previously created by `Box::into_raw()`
/// and not yet reclaimed.
pub unsafe fn handle_to_context_mut<'a>(
    handle: *mut ExifToolHandle,
) -> Option<&'a mut ExifToolContext> {
    if handle.is_null() {
        None
    } else {
        // SAFETY: Caller guarantees handle is a valid pointer from Box::into_raw()
        Some(unsafe { &mut *(handle as *mut ExifToolContext) })
    }
}
