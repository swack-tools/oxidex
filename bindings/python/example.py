#!/usr/bin/env python3
"""
Example script demonstrating ExifTool-RS Python bindings.

This script shows how to:
1. Load a JPEG file and read its metadata
2. Extract specific EXIF tags
3. Handle errors properly
4. Use the context manager for automatic cleanup
"""

import sys
from pathlib import Path
from exiftool_rs import ExifTool, ExifToolError


def main():
    """Main example function."""
    # Path to sample JPEG (relative to this script's location)
    script_dir = Path(__file__).parent
    sample_file = script_dir / ".." / ".." / "tests" / "fixtures" / "jpeg" / "sample_with_exif.jpg"

    print("ExifTool-RS Python Bindings Example")
    print("=" * 50)
    print()

    # Example 1: Basic usage with context manager
    print("Example 1: Reading EXIF metadata from a JPEG file")
    print(f"File: {sample_file}")
    print()

    try:
        with ExifTool() as et:
            # Read metadata from file
            et.read_file(str(sample_file))

            # Get specific tags
            print("Camera Information:")
            print("-" * 50)

            make = et.get_tag("EXIF:Make")
            if make:
                print(f"  Make:         {make}")

            model = et.get_tag("EXIF:Model")
            if model:
                print(f"  Model:        {model}")

            datetime = et.get_tag("EXIF:DateTime")
            if datetime:
                print(f"  DateTime:     {datetime}")

            iso = et.get_tag("EXIF:ISO")
            if iso:
                print(f"  ISO:          {iso}")

            aperture = et.get_tag("EXIF:FNumber")
            if aperture:
                print(f"  Aperture:     {aperture}")

            shutter = et.get_tag("EXIF:ExposureTime")
            if shutter:
                print(f"  Shutter:      {shutter}")

            focal = et.get_tag("EXIF:FocalLength")
            if focal:
                print(f"  Focal Length: {focal}")

            print()

    except ExifToolError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1

    # Example 2: Listing all tags
    print("Example 2: Listing all available tags")
    print("-" * 50)

    try:
        with ExifTool() as et:
            et.read_file(str(sample_file))

            tag_count = et.get_tag_count()
            print(f"Total tags: {tag_count}")
            print()

            # Show first 10 tags
            print("First 10 tags:")
            for i in range(min(10, tag_count)):
                tag_name = et.get_tag_name_at(i)
                if tag_name:
                    tag_value = et.get_tag(tag_name)
                    # Truncate long values
                    value_str = str(tag_value)[:50]
                    if len(str(tag_value)) > 50:
                        value_str += "..."
                    print(f"  {tag_name}: {value_str}")

            if tag_count > 10:
                print(f"  ... and {tag_count - 10} more tags")

            print()

    except ExifToolError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1

    # Example 3: Error handling
    print("Example 3: Error handling")
    print("-" * 50)

    try:
        with ExifTool() as et:
            # Try to read a non-existent file
            et.read_file("/nonexistent/file.jpg")
    except ExifToolError as e:
        print(f"Expected error caught: {e}")
        print()

    # Example 4: Using get_all_tags()
    print("Example 4: Getting all tags as a dictionary")
    print("-" * 50)

    try:
        with ExifTool() as et:
            et.read_file(str(sample_file))

            all_tags = et.get_all_tags()
            print(f"Retrieved {len(all_tags)} tags")

            # Show some interesting tags if they exist
            interesting_tags = [
                "EXIF:Make",
                "EXIF:Model",
                "EXIF:DateTime",
                "File:FileType",
                "File:MIMEType",
            ]

            print("\nSelected tags:")
            for tag in interesting_tags:
                if tag in all_tags:
                    print(f"  {tag}: {all_tags[tag]}")

            print()

    except ExifToolError as e:
        print(f"Error: {e}", file=sys.stderr)
        return 1

    print("=" * 50)
    print("All examples completed successfully!")
    return 0


if __name__ == "__main__":
    sys.exit(main())
