# MakerNote Support

OxiDex supports extracting manufacturer-specific metadata (MakerNotes) from JPEG and TIFF files for 40+ camera manufacturers and software applications.

## Supported Manufacturers

### Traditional Cameras

| Manufacturer | Description |
|--------------|-------------|
| Canon | EOS series, PowerShot, etc. |
| Nikon | D-series, Z-series, Coolpix |
| Sony | Alpha series, Cyber-shot |
| Olympus | OM-D, PEN, Tough series |
| Panasonic | Lumix series |
| Pentax | K-series, Q-series |
| Fujifilm | X-series, GFX, FinePix |
| Leica | M-series, Q-series, SL-series |
| Sigma | fp series, Foveon cameras |
| Phase One | Medium format backs |
| Minolta | Legacy Maxxum/Dynax series |

### Smartphones

| Manufacturer | Description |
|--------------|-------------|
| Apple | iPhone series |
| Google | Pixel series |
| Samsung | Galaxy series |
| Microsoft | Lumia, Surface cameras |
| Qualcomm | Reference devices |

### Specialty Devices

| Manufacturer | Description |
|--------------|-------------|
| DJI | Mavic, Phantom, Inspire drones |
| FLIR | Thermal imaging cameras |
| GoPro | Hero series action cameras |
| RED | Digital cinema cameras |
| Reconyx | Wildlife trail cameras |
| InfiRay | Thermal cameras |
| Lytro | Light field cameras |
| Nintendo | 3DS cameras |
| Parrot | Anafi, Bebop drones |

### Legacy Cameras

| Manufacturer | Description |
|--------------|-------------|
| Casio | Exilim series |
| GE | General Electric cameras |
| HP | Photosmart series |
| JVC | Digital cameras and camcorders |
| Kodak | EasyShare, Professional series |
| Leaf | Digital backs |
| Motorola | Phone cameras |
| Ricoh | GR series, Caplio |
| Sanyo | Xacti series |

### Software Applications

| Software | Description |
|----------|-------------|
| Capture One | Phase One editing software |
| FotoStation | FotoWare asset management |
| GIMP | GNU Image Manipulation Program |
| Adobe InDesign | Page layout software |
| Nikon Capture NX | Nikon's editing software |
| Photo Mechanic | Photo management software |
| Adobe Photoshop | Image editing software |
| Scalado | Mobile imaging software |

**Total: 40+ supported manufacturers**

## How It Works

MakerNotes are proprietary binary data structures embedded in EXIF tag 0x927C. Each manufacturer uses a different format.

When OxiDex encounters a MakerNote tag during JPEG/TIFF parsing:

1. **Detects the camera make** from EXIF Make tag (0x010F)
2. **Dispatches to manufacturer parser** based on normalized make string
3. **Validates the header** (if parser provides validation)
4. **Extracts manufacturer-specific tags** (lens info, focus points, custom settings, etc.)
5. **Returns tags** with manufacturer prefix (e.g., "Canon:LensModel")

### Architecture

```
TIFF File Parser
    |
    +-- Read EXIF tags (including Make)
    |
    +-- Encounter MakerNote tag (0x927C)
    |
    +-- MakerNote Dispatcher
        |
        +-- Normalize make string (case-insensitive, trim whitespace)
        |
        +-- Match to parser:
            +-- Canon Parser
            +-- Nikon Parser
            +-- Sony Parser
            +-- ... (40+ parsers)
            |
            +-- Parse binary data
            |
            +-- Extract tags with manufacturer prefix
```

## Tag Naming Convention

MakerNote tags use the format: `{Manufacturer}:{TagName}`

### Examples

**Camera Tags:**
- `Canon:LensModel` - Canon lens model string
- `Canon:FirmwareVersion` - Camera firmware version
- `Canon:SerialNumber` - Camera body serial number
- `Nikon:ShutterCount` - Nikon shutter actuation count
- `Sony:LensType` - Sony lens type ID
- `Olympus:FocusMode` - Focus mode setting
- `Pentax:ExposureCompensation` - Exposure compensation value

**Specialty Device Tags:**
- `DJI:FlightSpeed` - DJI drone flight speed
- `DJI:Altitude` - Drone altitude
- `FLIR:Temperature` - Thermal camera temperature reading
- `GoPro:FrameRate` - GoPro video frame rate
- `RED:Compression` - RED cinema camera compression ratio

**Software Tags:**
- `Photoshop:Quality` - JPEG quality setting
- `CaptureOne:Variant` - Capture One variant name
- `GIMP:Version` - GIMP version used

## Usage Examples

### CLI

```bash
# Extract all metadata including MakerNotes
oxidex photo.jpg

# Extract specific Canon MakerNote tags
oxidex -Canon:LensModel -Canon:SerialNumber canon_photo.jpg

# Extract Nikon-specific tags
oxidex -Nikon:ShutterCount -Nikon:LensType nikon_photo.jpg

# Filter for all Canon tags
oxidex photo.jpg | grep "Canon:"

# Extract DJI drone metadata
oxidex -DJI:FlightSpeed -DJI:Altitude -DJI:GimbalPitch drone_photo.jpg
```

### Rust API

```rust
use oxidex::Metadata;

fn main() -> oxidex::Result<()> {
    let metadata = Metadata::from_path("photo.jpg")?;

    // Access standard EXIF tags
    if let Some(make) = metadata.get_string("EXIF:Make") {
        println!("Camera: {}", make);
    }

    // Access Canon-specific MakerNote tags
    if let Some(firmware) = metadata.get_string("Canon:FirmwareVersion") {
        println!("Canon Firmware: {}", firmware);
    }
    if let Some(serial) = metadata.get_string("Canon:SerialNumber") {
        println!("Camera Serial: {}", serial);
    }
    if let Some(lens) = metadata.get_string("Canon:LensModel") {
        println!("Lens: {}", lens);
    }

    Ok(())
}
```

## Implementation Details

### Byte Order Handling

MakerNotes can use different byte orders than the main TIFF file:
- **Canon**: Typically little-endian
- **Nikon**: Uses "Nikon" header with embedded byte order
- **Olympus**: Uses TIFF-style byte order marker
- **Most others**: Inherit from parent TIFF

The dispatcher passes the TIFF byte order to each parser, which can override it if needed.

### Header Validation

Most manufacturers include a signature header in their MakerNote data:
- **Canon**: No specific header, starts with tag array
- **Nikon**: "Nikon\0" header followed by TIFF-like structure
- **Olympus**: "OLYMPUS\0" header
- **Panasonic**: "Panasonic\0" header
- **Sony**: No header, direct IFD structure

The `validate_header()` method allows each parser to verify data integrity before parsing.

### Error Handling

The dispatcher uses graceful error handling:
- Unknown manufacturers are silently skipped (not all cameras have MakerNotes)
- Invalid headers result in skipping (no tag extraction)
- Parse errors are logged but don't fail the entire EXIF read
- Malformed data returns empty tag set

This ensures that MakerNote parsing errors don't prevent reading standard EXIF tags.

## Limitations

### Format Variations
- Not all camera makes have MakerNote parsers (unknown makes are silently skipped)
- Some manufacturers encrypt or obfuscate their MakerNote data (e.g., some Sony models)
- MakerNote formats may change between camera models within the same manufacturer
- Software-generated MakerNotes may have limited or non-standard metadata

### Data Quality
- MakerNote specifications are often undocumented or reverse-engineered
- Some tags may be camera model-specific and not present in all files
- Tag values may use proprietary encoding (e.g., bit flags, lookup tables)
- Date/time formats may vary from standard EXIF

### Performance
- MakerNote parsing adds overhead to EXIF reading (typically < 5% for most files)
- Large MakerNote blocks (e.g., Canon with extensive arrays) may impact performance
- Validation and header checking add minimal overhead but improve reliability

## Troubleshooting

### No MakerNote Tags Extracted

**Possible causes:**
1. Camera manufacturer not supported - check the supported list above
2. Invalid or corrupted MakerNote data - check file integrity
3. Encrypted MakerNote (some Sony, Samsung models)
4. Software-modified image with stripped MakerNotes

**Solutions:**
- Check `EXIF:Make` tag value matches supported manufacturer
- Try with original unedited image from camera
- Enable verbose logging to see dispatcher output

### Incorrect Tag Values

**Possible causes:**
1. Byte order mismatch
2. Camera model using variant format
3. Tag registry out of date with latest camera models

**Solutions:**
- Compare with ExifTool output for validation
- File issue with sample image and camera model
- Check if camera firmware is recent (may use new format)

## References

### Standards
- [EXIF 2.32 Specification](https://www.cipa.jp/std/documents/e/DC-X008-Translation-2019-E.pdf) - Official EXIF standard
- [TIFF 6.0 Specification](https://www.itu.int/itudoc/itu-t/com16/tiff-fx/docs/tiff6.pdf) - TIFF format (MakerNotes use TIFF-like IFD structures)

### Reference Implementations
- [ExifTool Tag Names](https://exiftool.org/TagNames/index.html) - Comprehensive tag database
- [ExifTool MakerNote Documentation](https://exiftool.org/TagNames/Canon.html) - Manufacturer-specific formats
