//! FFI context and handle types
//!
//! This module defines the internal context structure that holds all state
//! for a handle, and the opaque handle type exposed to C.

use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::Mutex;

use crate::core::MetadataMap;

// ============================================================================
// Internal Context Structure
// ============================================================================

/// Internal context structure that holds all state for a handle.
/// This is the Rust object behind the opaque ExifToolHandle pointer.
pub struct ExifToolContext {
    /// The metadata map containing all loaded tags
    pub metadata: MetadataMap,
    /// Cache of CString instances for string returns.
    ///
    /// The returned pointer is owned by the handle. It remains valid across subsequent read-only getter calls, including concurrent getters, until the next successful `exiftool_read_file` on that handle or handle destruction. Copy the string before either event if it is needed afterward. File reads, tag mutations, file writes, and destruction must not overlap any operation on the same handle or use of its borrowed strings; callers must provide synchronization.
    pub string_cache: Mutex<Vec<CString>>,
    /// Iterator cache: stores tag names for iteration
    pub tag_names_cache: Vec<String>,
    /// Every mutation since the last `exiftool_read_file`, in call order:
    /// the key each `exiftool_set_tag_*` / `exiftool_remove_tag` named, and
    /// each `GROUP:All` deletion `exiftool_remove_tag` recorded
    /// (`"EXIF:All"`), which names no row of the map. `exiftool_write_file`
    /// applies the handle's changes in this order, as ExifTool applies a
    /// command's assignments: a group deletion removes what was set before
    /// it, never what was set after it. Cleared by `exiftool_read_file`.
    pub mutations: Vec<String>,
}

impl ExifToolContext {
    /// Creates a new empty context
    pub fn new() -> Self {
        Self {
            metadata: MetadataMap::new(),
            string_cache: Mutex::new(Vec::new()),
            tag_names_cache: Vec::new(),
            mutations: Vec::new(),
        }
    }

    /// Clears the string cache to free memory.
    ///
    /// The mutex synchronizes cache access itself, but callers must still hold
    /// the FFI contract's exclusive same-handle access for this mutating
    /// operation: clearing invalidates every previously returned pointer.
    pub fn clear_string_cache(&self) {
        self.string_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    /// Caches a CString and returns a pointer to its owned allocation.
    ///
    /// Moving the `CString` within the backing `Vec` does not move its heap
    /// allocation, so returned pointers survive later cache reallocations.
    pub fn cache_string(&self, s: CString) -> *const c_char {
        let ptr = s.as_ptr();
        self.string_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(s);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn string_cache_supports_concurrent_shared_handle_getters() {
        let context = ExifToolContext::new();

        thread::scope(|scope| {
            for worker in 0..8 {
                let context = &context;
                scope.spawn(move || {
                    for iteration in 0..128 {
                        let expected = format!("worker-{worker}-{iteration}");
                        let pointer =
                            context.cache_string(CString::new(expected.as_str()).unwrap());

                        let actual = unsafe { CStr::from_ptr(pointer) };
                        assert_eq!(actual.to_str().unwrap(), expected);
                    }
                });
            }
        });

        assert_eq!(context.string_cache.lock().unwrap().len(), 8 * 128);
    }

    #[test]
    fn shared_context_caches_strings_concurrently_without_invalidating_pointers() {
        let context = Arc::new(ExifToolContext::new());
        let first = context.cache_string(CString::new("first").unwrap()) as usize;

        let threads = (0..8)
            .map(|thread_index| {
                let context = Arc::clone(&context);
                thread::spawn(move || {
                    for value_index in 0..256 {
                        let value = CString::new(format!("{thread_index}:{value_index}"))
                            .expect("generated value has no NUL");
                        assert!(!context.cache_string(value).is_null());
                    }
                })
            })
            .collect::<Vec<_>>();
        for thread in threads {
            thread
                .join()
                .expect("concurrent cache writer must not panic");
        }

        let first = first as *const c_char;
        assert_eq!(
            unsafe { CStr::from_ptr(first) }.to_str().unwrap(),
            "first",
            "a returned CString pointer must survive cache Vec reallocations"
        );
    }
}
