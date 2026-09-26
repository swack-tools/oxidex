#!/usr/bin/env python3
"""Rebuild the two multi-EXIF-APP1 fixtures from their pinned ExifTool 13.59
sources (tests/jpeg_multi_exif_app1.rs checks the committed bytes against the
same construction whenever the sources are present).

  multi-app1-nikon-lsi1.jpg  t/images/Nikon.jpg with combined-samples
                             Nikon/NikonLS-50.jpg's EXIF APP1 inserted
                             directly after Nikon.jpg's own EXIF APP1.
  multi-app1-lsi1-nikon.jpg  the reverse: NikonLS-50.jpg with Nikon.jpg's
                             EXIF APP1 inserted after its own.

usage: make_fixtures.py EXIFTOOL_CACHE_DIR   (the pinned 13.59 cache: its
       exiftool/t/images and combined-samples directories)"""
import struct, sys
from pathlib import Path

def first_exif_app1(data):
    i = 2
    while True:
        marker = data[i + 1]
        length = struct.unpack(">H", data[i + 2:i + 4])[0]
        if marker == 0xE1 and data[i + 4:i + 10] == b"Exif\0\0":
            return i, i + 2 + length
        i += 2 + length

def insert_second(host, donor):
    _, end = first_exif_app1(host)
    start, stop = first_exif_app1(donor)
    return host[:end] + donor[start:stop] + host[end:]

cache = Path(sys.argv[1])
nikon = (cache / "exiftool/t/images/Nikon.jpg").read_bytes()
lsi = (cache / "combined-samples/Nikon/NikonLS-50.jpg").read_bytes()
out = Path(__file__).resolve().parent
(out / "multi-app1-nikon-lsi1.jpg").write_bytes(insert_second(nikon, lsi))
(out / "multi-app1-lsi1-nikon.jpg").write_bytes(insert_second(lsi, nikon))
