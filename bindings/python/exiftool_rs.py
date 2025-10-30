"""
Python ctypes bindings for ExifTool-RS C FFI.

This module provides a Pythonic wrapper around the ExifTool-RS shared library
using Python's ctypes module.

Example:
    >>> from exiftool_rs import ExifTool
    >>> with ExifTool() as et:
    ...     et.read_file("photo.jpg")
    ...     make = et.get_tag("EXIF:Make")
    ...     print(f"Camera: {make}")
"""

import ctypes
import ctypes.util
import os
import sys
from typing import Optional


# Error codes from exiftool_rs.h
EXIFTOOL_OK = 0
EXIFTOOL_ERR_IO = 1
EXIFTOOL_ERR_PARSE = 2
EXIFTOOL_ERR_TAG_NOT_FOUND = 3
EXIFTOOL_ERR_INVALID_TAG_VALUE = 4
EXIFTOOL_ERR_UNSUPPORTED_FORMAT = 5
EXIFTOOL_ERR_NULL_POINTER = 6
EXIFTOOL_ERR_INTERNAL = 99


class ExifToolError(Exception):
    """Exception raised by ExifTool operations."""
    pass


def _find_library() -> ctypes.CDLL:
    """
    Locate and load the ExifTool-RS shared library.

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
        lib_name = "libexiftool_rs.dylib"
    elif sys.platform == "win32":
        lib_name = "exiftool_rs.dll"
    else:  # Linux and other Unix-like systems
        lib_name = "libexiftool_rs.so"

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
    lib_path = ctypes.util.find_library("exiftool_rs")
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

# Error handling
_lib.exiftool_get_last_error.restype = ctypes.c_char_p
_lib.exiftool_get_last_error.argtypes = []


class ExifTool:
    """
    Python wrapper for ExifTool-RS C FFI.

    Provides a Pythonic interface for reading EXIF metadata from images.

    Example:
        >>> with ExifTool() as et:
        ...     et.read_file("photo.jpg")
        ...     print(et.get_tag("EXIF:Make"))
        Canon
    """

    def __init__(self):
        """
        Create a new ExifTool handle.

        Raises:
            ExifToolError: If handle creation fails (out of memory)
        """
        self._handle = _lib.exiftool_create()
        if not self._handle:
            raise ExifToolError("Failed to create ExifTool handle (out of memory)")

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
            ExifToolError: If result is not EXIFTOOL_OK
        """
        if result != EXIFTOOL_OK:
            error_msg = _lib.exiftool_get_last_error()
            if error_msg:
                msg = error_msg.decode('utf-8', errors='replace')
            else:
                msg = f"Unknown error (code {result})"
            raise ExifToolError(msg)

    def read_file(self, filepath: str) -> None:
        """
        Read metadata from a file.

        Args:
            filepath: Path to the image file

        Raises:
            ExifToolError: If reading fails (file not found, parse error, etc.)
        """
        if not self._handle:
            raise ExifToolError("ExifTool handle has been destroyed")

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

        if result == EXIFTOOL_OK:
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

        if result == EXIFTOOL_OK:
            return value.value
        return None

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
