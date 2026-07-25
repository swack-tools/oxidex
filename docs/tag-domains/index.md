# Tag Domains

OxiDex organizes metadata tags into semantic domains based on their purpose and the type of information they represent. This organization helps users and developers understand what metadata is available and where to find specific information.

## Domain Categories

### [Core](./core.md)
Fundamental metadata present in most files: file properties, timestamps, format identification, and basic technical information.

### [Camera](./camera.md)
Camera-specific metadata including exposure settings, lens information, focus data, flash settings, and manufacturer-specific maker notes.

### [Image](./image.md)
Image technical metadata: dimensions, resolution, color space, compression, and pixel-level information.

### [Media](./media.md)
Audio and video metadata: codecs, duration, bitrates, channels, and streaming information.

### [Document](./document.md)
Document metadata: author, title, creation date, revision history, and document-specific properties for PDFs, Office files, etc.

### [Specialty](./specialty.md)
Specialized metadata domains: GPS/location data, XMP metadata, IPTC news metadata, and format-specific extensions.

## Tag Organization

Tags are prefixed with their source group for clarity:

```
EXIF:Make           → Camera manufacturer (EXIF standard)
XMP:Creator         → Document creator (XMP/Dublin Core)
GPS:GPSLatitude     → Geographic latitude (GPS IFD)
IPTC:Keywords       → Content keywords (IPTC-IIM)
MakerNotes:LensID   → Lens identification (vendor-specific)
```

## See Also

- [Tag Database Reference](/reference/tag-database) - Complete tag listing
- [Architecture: Multi-Crate Tags](/architecture/multi-crate-tags) - How tags are organized across crates
