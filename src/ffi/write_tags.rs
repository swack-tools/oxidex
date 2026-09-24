//! FFI tag writing functions
//!
//! Functions for setting tag values and writing metadata to files.

use std::ffi::CStr;
use std::os::raw::{c_char, c_int};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use crate::core::TagValue;

use super::context::{ExifToolHandle, handle_to_context, handle_to_context_mut};
use super::error::{
    EXIFTOOL_ERR_INTERNAL, EXIFTOOL_ERR_INVALID_TAG_VALUE, EXIFTOOL_ERR_NULL_POINTER, EXIFTOOL_OK,
    error_to_code, set_last_error,
};

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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
/// A group deletion (`GROUP:All`, such as `EXIF:All` or `GPS:All`) removes
/// no row of the handle: it is recorded and applied when the handle is next
/// written to a file (ExifTool's `-GROUP:All=`), which refuses it with
/// `EXIFTOOL_ERR_TAG_NOT_WRITTEN` when oxidex cannot delete that group from
/// the file. `exiftool_read_file` discards recorded group deletions.
///
/// # Returns
/// - `EXIFTOOL_OK` (always succeeds, even if tag didn't exist)
/// - `EXIFTOOL_ERR_NULL_POINTER` if handle or tag_name is NULL
///
/// # Thread Safety
/// Not thread-safe.
#[unsafe(no_mangle)]
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

        // `GROUP:All` (`EXIF:All`, `GPS:All`) is a group deletion, which no
        // row of the map names: record it for the next file write.
        if crate::writers::write_request::group_deletion(name_str).is_some() {
            if !context
                .group_deletions
                .iter()
                .any(|group| group == name_str)
            {
                context.group_deletions.push(name_str.to_string());
            }
            return EXIFTOOL_OK;
        }
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
/// - `EXIFTOOL_ERR_TAG_NOT_WRITTEN`: A requested change would not be written
///   (a group the file's writer cannot write, such as XMP in a JPEG, an
///   ungrouped name that does not resolve, or a change the read-back after
///   writing does not find). `exiftool_get_last_error_tag_count()` and
///   `exiftool_get_last_error_tag()` name every such tag.
///
/// # Requests and the guarantee
/// The handle's tags are the metadata the file should end up with: a tag
/// that is new or differs from the file is set, and a tag the file carries
/// that the handle lacks is deleted (derived `File:`, `Composite:` and
/// file-system rows excepted). `EXIFTOOL_OK` means every such change is in
/// the file, proven by reading it back; on any error nothing was written and
/// the file is byte-identical.
///
/// # Thread Safety
/// Not thread-safe with respect to the handle. Do not call concurrently with
/// any other operation on the same handle, including getters, mutations, or
/// destruction.
#[unsafe(no_mangle)]
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

        // The handle's map, plus any recorded `GROUP:All` deletions, in one
        // write transaction (see `write_metadata`).
        match crate::core::operations::write_metadata_and_delete_groups(
            path,
            &context.metadata,
            &context.group_deletions,
        ) {
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
