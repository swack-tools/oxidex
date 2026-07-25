//! FFI handle lifecycle functions
//!
//! Functions for creating and destroying ExifTool handles.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;

use super::context::{ExifToolContext, ExifToolHandle};

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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
