"""
Python ctypes bindings for OxiDex C FFI.

This module provides a Pythonic wrapper around the OxiDex shared library
using Python's ctypes module.

Example:
    >>> from oxidex import Oxidex
    >>> with Oxidex() as ox:
    ...     ox.read_file("photo.jpg")
    ...     make = ox.get_tag("EXIF:Make")
    ...     print(f"Camera: {make}")
"""

import ctypes
import ctypes.util
import operator
from enum import IntEnum
import os
import sys
from typing import Optional


# Error codes from oxidex.h
OXIDEX_OK = 0
OXIDEX_ERR_IO = 1
OXIDEX_ERR_PARSE = 2
OXIDEX_ERR_TAG_NOT_FOUND = 3
OXIDEX_ERR_INVALID_TAG_VALUE = 4
OXIDEX_ERR_UNSUPPORTED_FORMAT = 5
OXIDEX_ERR_NULL_POINTER = 6
OXIDEX_ERR_TAG_NOT_WRITTEN = 7
OXIDEX_ERR_INTERNAL = 99

# The C ABI's integer type is int64_t.
INT64_MIN = -(2**63)
INT64_MAX = 2**63 - 1

# exiftool_write_file_with_outcome's outcomes (ExifTool WriteInfo's 1 / 2)
OXIDEX_WRITE_UPDATED = 1
OXIDEX_WRITE_UNCHANGED = 2


class ValueChannel(IntEnum):
    """The stored, ValueConv, and PrintConv stages of a tag occurrence."""

    STORED = 0
    VALUE_CONV = 1
    PRINT_CONV = 2


class OxidexError(Exception):
    """Exception raised by Oxidex operations."""

    def __init__(self, message: str, code: Optional[int] = None):
        super().__init__(message)
        self.code = code


class OxidexTagsNotWrittenError(OxidexError):
    """A write named tags that would not be written; nothing was written.

    ``tags`` lists ``(tag, reason)`` pairs, each tag spelled as the request
    spelled it.
    """

    def __init__(self, message: str, tags):
        super().__init__(message, OXIDEX_ERR_TAG_NOT_WRITTEN)
        self.tags = list(tags)


def _find_library() -> ctypes.CDLL:
    """
    Locate and load the OxiDex shared library.

    Attempts to find the library in the following locations:
    1. Common build directories relative to this script
    2. System library paths

    Returns:
        ctypes.CDLL: The loaded library

    Raises:
        OSError: If the library cannot be found or loaded
    """
    # Determine library name based on platform
    if sys.platform == "darwin":
        lib_name = "liboxidex.dylib"
    elif sys.platform == "win32":
        lib_name = "oxidex.dll"
    else:  # Linux and other Unix-like systems
        lib_name = "liboxidex.so"

    # An explicit path wins: tests/python_bindings.rs points it at the
    # library the running `cargo test` just built.
    explicit = os.environ.get("OXIDEX_LIBRARY")
    if explicit:
        return ctypes.CDLL(explicit)

    # Try common build directories relative to this script
    script_dir = os.path.dirname(os.path.abspath(__file__))
    search_paths = [
        # From bindings/python/
        os.path.join(script_dir, "..", "..", "target", "release", lib_name),
        os.path.join(script_dir, "..", "..", "target", "debug", lib_name),
        # From repo root
        os.path.join(script_dir, "target", "release", lib_name),
        os.path.join(script_dir, "target", "debug", lib_name),
        # Current directory
        os.path.join(script_dir, lib_name),
    ]

    # Try each path
    for path in search_paths:
        if os.path.exists(path):
            try:
                return ctypes.CDLL(path)
            except OSError:
                continue

    # Try system library path
    lib_path = ctypes.util.find_library("oxidex")
    if lib_path:
        try:
            return ctypes.CDLL(lib_path)
        except OSError:
            pass

    # Failed to find library
    raise OSError(
        f"Could not find {lib_name}. "
        "Please build the library with 'cargo build --lib --release' "
        "and ensure it's in one of the following locations:\n" +
        "\n".join(f"  - {path}" for path in search_paths) +
        "\n\nOr set LD_LIBRARY_PATH (Linux), DYLD_LIBRARY_PATH (macOS), "
        "or PATH (Windows) to include the directory containing the library."
    )


# Load the library
_lib = _find_library()


# Define function signatures
# Handle lifecycle
_lib.exiftool_create.restype = ctypes.c_void_p
_lib.exiftool_create.argtypes = []

_lib.exiftool_destroy.restype = None
_lib.exiftool_destroy.argtypes = [ctypes.c_void_p]

# Metadata reading
_lib.exiftool_read_file.restype = ctypes.c_int
_lib.exiftool_read_file.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

_lib.exiftool_get_tag_count.restype = ctypes.c_size_t
_lib.exiftool_get_tag_count.argtypes = [ctypes.c_void_p]

_lib.exiftool_get_tag_name_at.restype = ctypes.c_char_p
_lib.exiftool_get_tag_name_at.argtypes = [ctypes.c_void_p, ctypes.c_size_t]

_lib.exiftool_has_tag.restype = ctypes.c_int
_lib.exiftool_has_tag.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

# Tag access
_lib.exiftool_get_tag_string.restype = ctypes.c_char_p
_lib.exiftool_get_tag_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

_lib.exiftool_get_tag_string_in_channel.restype = ctypes.c_char_p
_lib.exiftool_get_tag_string_in_channel.argtypes = [
    ctypes.c_void_p,
    ctypes.c_char_p,
    ctypes.c_int,
]

_lib.exiftool_get_tag_integer.restype = ctypes.c_int
_lib.exiftool_get_tag_integer.argtypes = [
    ctypes.c_void_p,
    ctypes.c_char_p,
    ctypes.POINTER(ctypes.c_int64)
]

_lib.exiftool_get_tag_float.restype = ctypes.c_int
_lib.exiftool_get_tag_float.argtypes = [
    ctypes.c_void_p,
    ctypes.c_char_p,
    ctypes.POINTER(ctypes.c_double)
]

# Tag mutation and file writing
_lib.exiftool_set_tag_string.restype = ctypes.c_int
_lib.exiftool_set_tag_string.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p]

_lib.exiftool_set_tag_integer.restype = ctypes.c_int
_lib.exiftool_set_tag_integer.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int64]

_lib.exiftool_set_tag_float.restype = ctypes.c_int
_lib.exiftool_set_tag_float.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_double]

_lib.exiftool_remove_tag.restype = ctypes.c_int
_lib.exiftool_remove_tag.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

_lib.exiftool_write_file.restype = ctypes.c_int
_lib.exiftool_write_file.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

_lib.exiftool_write_file_with_outcome.restype = ctypes.c_int
_lib.exiftool_write_file_with_outcome.argtypes = [
    ctypes.c_void_p,
    ctypes.c_char_p,
    ctypes.POINTER(ctypes.c_int),
]

# Error handling
_lib.exiftool_get_last_error.restype = ctypes.c_char_p
_lib.exiftool_get_last_error.argtypes = []

_lib.exiftool_get_last_error_tag_count.restype = ctypes.c_size_t
_lib.exiftool_get_last_error_tag_count.argtypes = []

_lib.exiftool_get_last_error_tag.restype = ctypes.c_char_p
_lib.exiftool_get_last_error_tag.argtypes = [ctypes.c_size_t]

_lib.exiftool_get_last_error_tag_reason.restype = ctypes.c_char_p
_lib.exiftool_get_last_error_tag_reason.argtypes = [ctypes.c_size_t]


class Oxidex:
    """
    Python wrapper for OxiDex C FFI.

    Provides a Pythonic interface for reading EXIF metadata from images.

    Example:
        >>> with Oxidex() as ox:
        ...     ox.read_file("photo.jpg")
        ...     print(ox.get_tag("EXIF:Make"))
        Canon
    """

    def __init__(self):
        """
        Create a new Oxidex handle.

        Raises:
            OxidexError: If handle creation fails (out of memory)
        """
        self._handle = _lib.exiftool_create()
        if not self._handle:
            raise OxidexError("Failed to create Oxidex handle (out of memory)")

    def __del__(self):
        """Destroy the handle and free resources."""
        if hasattr(self, '_handle') and self._handle:
            _lib.exiftool_destroy(self._handle)
            self._handle = None

    def __enter__(self):
        """Context manager entry."""
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        """Context manager exit - ensures cleanup."""
        self.__del__()
        return False

    def _check_error(self, result: int) -> None:
        """
        Check error code and raise exception if needed.

        Args:
            result: Return code from C function

        Raises:
            OxidexError: If result is not OXIDEX_OK
        """
        if result != OXIDEX_OK:
            error_msg = _lib.exiftool_get_last_error()
            if error_msg:
                msg = error_msg.decode('utf-8', errors='replace')
            else:
                msg = f"Unknown error (code {result})"
            if result == OXIDEX_ERR_TAG_NOT_WRITTEN:
                tags = []
                for index in range(_lib.exiftool_get_last_error_tag_count()):
                    tag = _lib.exiftool_get_last_error_tag(index) or b""
                    reason = _lib.exiftool_get_last_error_tag_reason(index) or b""
                    tags.append(
                        (
                            tag.decode('utf-8', errors='replace'),
                            reason.decode('utf-8', errors='replace'),
                        )
                    )
                raise OxidexTagsNotWrittenError(msg, tags)
            raise OxidexError(msg, result)

    def read_file(self, filepath: str) -> None:
        """
        Read metadata from a file.

        Args:
            filepath: Path to the image file

        Raises:
            OxidexError: If reading fails (file not found, parse error, etc.)
        """
        if not self._handle:
            raise OxidexError("Oxidex handle has been destroyed")

        filepath_bytes = filepath.encode('utf-8')
        result = _lib.exiftool_read_file(self._handle, filepath_bytes)
        self._check_error(result)

    def get_tag_count(self) -> int:
        """
        Get the number of tags in loaded metadata.

        Returns:
            Number of tags (0 if no metadata loaded)
        """
        if not self._handle:
            return 0
        return _lib.exiftool_get_tag_count(self._handle)

    def get_tag_name_at(self, index: int) -> Optional[str]:
        """
        Get tag name by index.

        Args:
            index: Zero-based index (must be < tag count)

        Returns:
            Tag name or None if index is out of bounds
        """
        if not self._handle:
            return None

        c_str = _lib.exiftool_get_tag_name_at(self._handle, index)
        if c_str:
            return c_str.decode('utf-8', errors='replace')
        return None

    def has_tag(self, tag_name: str) -> bool:
        """
        Check if a tag exists in the metadata.

        Args:
            tag_name: Name of the tag to check (e.g., "EXIF:Make")

        Returns:
            True if tag exists, False otherwise
        """
        if not self._handle:
            return False

        tag_bytes = tag_name.encode('utf-8')
        return _lib.exiftool_has_tag(self._handle, tag_bytes) == 1

    def get_tag(self, tag_name: str) -> Optional[str]:
        """
        Get tag value as a string.

        Args:
            tag_name: Name of the tag (e.g., "EXIF:Make")

        Returns:
            Tag value as string, or None if tag doesn't exist or is not a string type
        """
        if not self._handle:
            return None

        tag_bytes = tag_name.encode('utf-8')
        c_str = _lib.exiftool_get_tag_string(self._handle, tag_bytes)
        if c_str:
            # IMPORTANT: Copy the string immediately before next API call
            return c_str.decode('utf-8', errors='replace')
        return None

    def get_tag_in_channel(self, tag_name: str, channel: ValueChannel) -> Optional[str]:
        """Get a string tag from an explicit stored, ValueConv, or PrintConv stage."""
        if not self._handle:
            return None

        c_str = _lib.exiftool_get_tag_string_in_channel(
            self._handle,
            tag_name.encode('utf-8'),
            int(channel),
        )
        if c_str:
            return c_str.decode('utf-8', errors='replace')
        return None

    def get_tag_integer(self, tag_name: str) -> Optional[int]:
        """
        Get tag value as an integer.

        Args:
            tag_name: Name of the tag

        Returns:
            Tag value as integer, or None if tag doesn't exist or is not an integer type
        """
        if not self._handle:
            return None

        tag_bytes = tag_name.encode('utf-8')
        value = ctypes.c_int64()
        result = _lib.exiftool_get_tag_integer(
            self._handle, tag_bytes, ctypes.byref(value)
        )

        if result == OXIDEX_OK:
            return value.value
        return None

    def get_tag_float(self, tag_name: str) -> Optional[float]:
        """
        Get tag value as a float.

        Args:
            tag_name: Name of the tag

        Returns:
            Tag value as float, or None if tag doesn't exist or is not a float type
        """
        if not self._handle:
            return None

        tag_bytes = tag_name.encode('utf-8')
        value = ctypes.c_double()
        result = _lib.exiftool_get_tag_float(
            self._handle, tag_bytes, ctypes.byref(value)
        )

        if result == OXIDEX_OK:
            return value.value
        return None

    def set_tag(self, tag_name: str, value: str) -> None:
        """Set a tag to a string value in the loaded metadata (not the file)."""
        if not self._handle:
            raise OxidexError("Oxidex handle has been destroyed")
        self._check_error(
            _lib.exiftool_set_tag_string(
                self._handle, tag_name.encode('utf-8'), value.encode('utf-8')
            )
        )

    def set_tag_integer(self, tag_name: str, value: int) -> None:
        """
        Set a tag to an integer value in the loaded metadata (not the file).

        Raises:
            OxidexError: If ``value`` is outside the C ABI's signed 64-bit
                range. ctypes would otherwise wrap it silently (2**63 becomes
                -2**63, 2**64 becomes 0) and the setter would report success.
        """
        if not self._handle:
            raise OxidexError("Oxidex handle has been destroyed")
        value = operator.index(value)
        if not INT64_MIN <= value <= INT64_MAX:
            raise OxidexError(
                f"Integer {value} for {tag_name} is outside the signed 64-bit range "
                f"[{INT64_MIN}, {INT64_MAX}]"
            )
        self._check_error(
            _lib.exiftool_set_tag_integer(self._handle, tag_name.encode('utf-8'), value)
        )

    def set_tag_float(self, tag_name: str, value: float) -> None:
        """Set a tag to a float value in the loaded metadata (not the file)."""
        if not self._handle:
            raise OxidexError("Oxidex handle has been destroyed")
        self._check_error(
            _lib.exiftool_set_tag_float(self._handle, tag_name.encode('utf-8'), value)
        )

    def remove_tag(self, tag_name: str) -> None:
        """Remove a tag from the loaded metadata (not the file)."""
        if not self._handle:
            raise OxidexError("Oxidex handle has been destroyed")
        self._check_error(_lib.exiftool_remove_tag(self._handle, tag_name.encode('utf-8')))

    def write_file(self, filepath: str) -> int:
        """
        Write the loaded metadata to a file.

        A tag that is new or differs from the file is set. A tag removed from
        a handle read from this same file is deleted; a handle read from
        another file (or never read) only sets. ``remove_tag("GROUP:All")``
        deletes the whole group.

        Returns:
            OXIDEX_WRITE_UPDATED when the file changed, OXIDEX_WRITE_UNCHANGED
            when every change was already in effect (the file is
            byte-identical) -- ExifTool WriteInfo's 1 / 2.

        Raises:
            OxidexTagsNotWrittenError: If a requested change would not be
                written; ``.tags`` names each tag. Nothing is written then.
            OxidexError: If the write fails otherwise. Nothing is written then.
        """
        if not self._handle:
            raise OxidexError("Oxidex handle has been destroyed")
        outcome = ctypes.c_int(0)
        self._check_error(
            _lib.exiftool_write_file_with_outcome(
                self._handle, filepath.encode('utf-8'), ctypes.byref(outcome)
            )
        )
        return outcome.value

    def get_all_tags(self) -> dict[str, Optional[str]]:
        """
        Get all tags as a dictionary.

        Returns:
            Dictionary mapping tag names to their string values
        """
        result = {}
        count = self.get_tag_count()
        for i in range(count):
            tag_name = self.get_tag_name_at(i)
            if tag_name:
                tag_value = self.get_tag(tag_name)
                result[tag_name] = tag_value
        return result
