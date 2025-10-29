#!/usr/bin/env python3
"""
Generate a minimal valid MP4 file with metadata for testing.
This creates a valid MP4 structure without actual video data.
"""

import struct

def write_atom(fourcc, data):
    """Create an atom with the given FourCC and data."""
    size = len(data) + 8
    # Handle FourCC as bytes if it's a string with special characters
    if isinstance(fourcc, str):
        fourcc_bytes = fourcc.encode('latin-1')
    else:
        fourcc_bytes = fourcc
    return struct.pack('>I', size) + fourcc_bytes + data

def create_ftyp():
    """Create an ftyp (file type) atom."""
    data = b'isom'  # major brand
    data += struct.pack('>I', 0)  # minor version
    data += b'isomiso2mp41'  # compatible brands
    return write_atom('ftyp', data)

def create_data_atom(text):
    """Create an iTunes-style data atom with UTF-8 text."""
    data = struct.pack('>I', 1)  # type indicator: 1 = UTF-8
    data += struct.pack('>I', 0)  # reserved
    data += text.encode('utf-8')
    return write_atom('data', data)

def create_metadata_item(fourcc, text):
    """Create a metadata item atom (e.g., ©nam, ©ART)."""
    data_atom = create_data_atom(text)
    return write_atom(fourcc, data_atom)

def create_ilst():
    """Create an ilst (item list) atom with sample metadata."""
    data = b''
    # Add various metadata tags
    data += create_metadata_item('\xa9nam', 'Sample Video Title')
    data += create_metadata_item('\xa9ART', 'Sample Artist')
    data += create_metadata_item('\xa9alb', 'Sample Album')
    data += create_metadata_item('\xa9day', '2024')
    data += create_metadata_item('\xa9cmt', 'Test MP4 file for ExifTool-RS')
    data += create_metadata_item('\xa9gen', 'Test Genre')
    data += create_metadata_item('\xa9cpy', 'Copyright 2024')
    return write_atom('ilst', data)

def create_hdlr():
    """Create a handler reference atom."""
    data = struct.pack('>I', 0)  # version/flags
    data += struct.pack('>I', 0)  # pre-defined
    data += b'mdir'  # handler type
    data += b'appl' + b'\x00' * 12  # reserved
    data += b'\x00'  # name (empty)
    return write_atom('hdlr', data)

def create_meta():
    """Create a meta atom with metadata."""
    data = struct.pack('>I', 0)  # version/flags
    data += create_hdlr()
    data += create_ilst()
    return write_atom('meta', data)

def create_udta():
    """Create a udta (user data) atom."""
    # Add classic QuickTime user data atoms as well
    classic_title = struct.pack('>H', 10)  # text length
    classic_title += struct.pack('>H', 0)  # language code
    classic_title += b'QT Title!!'
    qt_title = write_atom('\xa9nam', classic_title)

    data = qt_title + create_meta()
    return write_atom('udta', data)

def create_mvhd():
    """Create a movie header atom."""
    data = struct.pack('>I', 0)  # version/flags
    data += struct.pack('>I', 0)  # creation time
    data += struct.pack('>I', 0)  # modification time
    data += struct.pack('>I', 1000)  # timescale
    data += struct.pack('>I', 1000)  # duration
    data += struct.pack('>I', 0x00010000)  # preferred rate (1.0)
    data += struct.pack('>H', 0x0100)  # preferred volume (1.0)
    data += b'\x00' * 10  # reserved
    # Matrix (identity)
    data += struct.pack('>9I',
        0x00010000, 0, 0,
        0, 0x00010000, 0,
        0, 0, 0x40000000)
    data += struct.pack('>6I', 0, 0, 0, 0, 0, 0)  # pre-defined
    data += struct.pack('>I', 1)  # next track ID
    return write_atom('mvhd', data)

def create_moov():
    """Create a moov (movie) atom containing all metadata."""
    data = create_mvhd()
    data += create_udta()
    return write_atom('moov', data)

def create_mdat():
    """Create a minimal mdat (media data) atom."""
    # Empty media data
    return write_atom('mdat', b'')

def create_mp4_file(filename):
    """Create a complete MP4 file."""
    with open(filename, 'wb') as f:
        f.write(create_ftyp())
        f.write(create_moov())
        f.write(create_mdat())

    print(f"Created {filename}")

if __name__ == '__main__':
    create_mp4_file('tests/fixtures/mp4/sample.mp4')
    print("Sample MP4 file created successfully!")
