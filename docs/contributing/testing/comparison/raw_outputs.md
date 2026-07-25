# Raw Output Comparisons

## JPEG Comparison
### Perl ExifTool Output
```
File Name                       : sample_with_exif_xmp.jpg
Directory                       : /Users/allen/Documents/git/exiftool-rs/tests/fixtures/jpeg/complex
File Size                       : 624 bytes
File Modification Date/Time     : 2025:10:30 06:06:49-05:00
File Access Date/Time           : 2025:11:09 18:16:24-06:00
File Inode Change Date/Time     : 2025:11:01 15:02:53-05:00
File Permissions                : -rw-r--r--
File Type                       : JPEG
File Type Extension             : jpg
MIME Type                       : image/jpeg
Exif Byte Order                 : Little-endian (Intel, II)
Make                            : TestCamera
Camera Model Name               : TM
Creator                         : John Doe
Rating                          : 5
Title                           : Sample Photo
Rights                          : Copyright 2024
```

### exiftool-rs Output
```

IFD0:Make: TestCamera
IFD0:Model: TM
XMP-dc:Rights: Copyright 2024
XMP-dc:Title: Sample Photo
XMP-xmp:Creator: John Doe
XMP-xmp:Rating: 5
```

## PNG Comparison
### Perl ExifTool Output
```
File Name                       : synthetic_exif_001.png
Directory                       : /Users/allen/Documents/git/exiftool-rs/tests/fixtures/png/complex
File Size                       : 3.5 kB
File Modification Date/Time     : 2025:10:30 06:57:59-05:00
File Access Date/Time           : 2025:11:15 20:40:21-06:00
File Inode Change Date/Time     : 2025:11:01 15:02:11-05:00
File Permissions                : -rw-r--r--
File Type                       : PNG
File Type Extension             : png
MIME Type                       : image/png
Image Width                     : 800
Image Height                    : 600
Bit Depth                       : 8
Color Type                      : Palette
Compression                     : Deflate/Inflate
Filter                          : Adaptive
Interlace                       : Noninterlaced
White Point X                   : 0.3127
White Point Y                   : 0.329
Red X                           : 0.64
Red Y                           : 0.33
Green X                         : 0.3
Green Y                         : 0.6
Blue X                          : 0.15
Blue Y                          : 0.06
Palette                         : (Binary data 741 bytes, use -b option to extract)
Background Color                : 246
Pixels Per Unit X               : 1
Pixels Per Unit Y               : 1
Pixel Units                     : Unknown
Modify Date                     : 2025:10:30 11:57:59
Warning                         : [minor] Text/EXIF chunk(s) found after PNG IDAT (may be ignored by some readers) [x13]
Exif Byte Order                 : Big-endian (Motorola, MM)
Make                            : PNG EXIF Test
Camera Model Name               : PNG Cam 1
X Resolution                    : 1
Y Resolution                    : 1
Resolution Unit                 : None
Artist                          : PNG Artist 1
Y Cb Cr Positioning             : Centered
Exif Version                    : 0232
Date/Time Original              : 2024:03:01 10:00:00
Components Configuration        : Y, Cb, Cr, -
Color Space                     : Uncalibrated
Datecreate                      : 2025-10-30T11:57:59+00:00
Datemodify                      : 2025-10-30T11:57:59+00:00
Datetimestamp                   : 2025-10-30T11:57:59+00:00
Exif Artist                     : PNG Artist 1
Exif Color Space                : 65535
Exif Components Configuration   : ...
Exif Date Time Original         : 2024:03:01 10:00:00
Exif Exif Offset                : 164
Exif Exif Version               : 0232
Exif Make                       : PNG EXIF Test
Exif Model                      : PNG Cam 1
Exif Y Cb Cr Positioning        : 1
Image Size                      : 800x600
Megapixels                      : 0.480
```

### exiftool-rs Output
```

ExifIFD:ColorSpace: Uncalibrated
ExifIFD:ComponentsConfiguration: Y, Cb, Cr, -
ExifIFD:DateTimeOriginal: 2024:03:01 10:00:00
ExifIFD:ExifVersion: 0232
IFD0:Artist: PNG Artist 1
IFD0:Make: PNG EXIF Test
IFD0:Model: PNG Cam 1
IFD0:ResolutionUnit: None
IFD0:XResolution: 1
IFD0:YCbCrPositioning: Centered
IFD0:YResolution: 1
PNG-pHYs:PixelUnits: Unknown
PNG-pHYs:PixelsPerUnitX: 1
PNG-pHYs:PixelsPerUnitY: 1
PNG:BackgroundColor: 246
PNG:BitDepth: 8
PNG:BlueX: 0.15
PNG:BlueY: 0.06
PNG:ColorType: Palette
PNG:Compression: Deflate/Inflate
PNG:ExifArtist: PNG Artist 1
PNG:ExifColorSpace: 65535
PNG:ExifComponentsConfiguration: ...
PNG:ExifDateTimeOriginal: 2024:03:01 10:00:00
PNG:ExifExifOffset: 164
PNG:ExifExifVersion: 0232
PNG:ExifMake: PNG EXIF Test
PNG:ExifModel: PNG Cam 1
PNG:ExifResolutionUnit: 1
PNG:ExifXResolution: (Binary, 8 bytes)
PNG:ExifYCbCrPositioning: 1
PNG:ExifYResolution: (Binary, 8 bytes)
PNG:Filter: Adaptive
PNG:GreenX: 0.3
PNG:GreenY: 0.6
PNG:ImageHeight: 600
PNG:ImageWidth: 800
PNG:Interlace: Noninterlaced
PNG:ModifyDate: 2025:10:30 11:57:59
PNG:Palette: (Binary data 741 bytes, use -b option to extract)
PNG:RedX: 0.64
PNG:RedY: 0.33
PNG:WhitePointX: 0.3127
PNG:WhitePointY: 0.329
PNG:tEXt:date:create: 2025-10-30T11:57:59+00:00
PNG:tEXt:date:modify: 2025-10-30T11:57:59+00:00
PNG:tEXt:date:timestamp: 2025-10-30T11:57:59+00:00
PNG:tEXt:exif:Artist: PNG Artist 1
PNG:tEXt:exif:ColorSpace: 65535
PNG:tEXt:exif:ComponentsConfiguration: ...
PNG:tEXt:exif:DateTimeOriginal: 2024:03:01 10:00:00
PNG:tEXt:exif:ExifOffset: 164
PNG:tEXt:exif:ExifVersion: 0232
PNG:tEXt:exif:Make: PNG EXIF Test
PNG:tEXt:exif:Model: PNG Cam 1
PNG:tEXt:exif:YCbCrPositioning: 1
```

## TIFF Comparison
### Perl ExifTool Output
```
File Name                       : big_endian_001.tif
Directory                       : /Users/allen/Documents/git/exiftool-rs/tests/fixtures/tiff/complex
File Size                       : 180 kB
File Modification Date/Time     : 2025:11:09 18:07:08-06:00
File Access Date/Time           : 2025:11:15 20:40:21-06:00
File Inode Change Date/Time     : 2025:11:11 16:50:52-06:00
File Permissions                : -rw-r--r--
File Type                       : TIFF
File Type Extension             : tif
MIME Type                       : image/tiff
Exif Byte Order                 : Big-endian (Motorola, MM)
Image Width                     : 200
Image Height                    : 150
Bits Per Sample                 : 16 16 16
Compression                     : Uncompressed
Photometric Interpretation      : RGB
Fill Order                      : Normal
Strip Offsets                   : 8
Orientation                     : Horizontal (normal)
Samples Per Pixel               : 3
Rows Per Strip                  : 150
Strip Byte Counts               : 180000
Planar Configuration            : Chunky
Page Number                     : 0 1
White Point                     : 0.3127000034 0.3289999962
Primary Chromaticities          : 0.6399999857 0.3300000131 0.3000000119 0.6000000238 0.150000006 0.05999999866
Image Size                      : 200x150
Megapixels                      : 0.030
```

### exiftool-rs Output
```

IFD0:BitsPerSample: 16 16 16
IFD0:Compression: Uncompressed
IFD0:FillOrder: Normal
IFD0:ImageHeight: 150
IFD0:ImageWidth: 200
IFD0:Orientation: Horizontal (normal)
IFD0:PageNumber: 0 1
IFD0:PhotometricInterpretation: RGB
IFD0:PlanarConfiguration: Chunky
IFD0:PrimaryChromaticities: 0.6399999857 0.3300000131 0.3000000119 0.6000000238 0.1500000060 0.0599999987
IFD0:RowsPerStrip: 150
IFD0:SamplesPerPixel: 3
IFD0:StripByteCounts: 180000
IFD0:StripOffsets: 8
IFD0:WhitePoint: 0.3127000034 0.3289999962
```

## PDF Comparison
### Perl ExifTool Output
```
File Name                       : Allen Swackhamer Resume.pdf
Directory                       : /Users/allen/Documents/git/exiftool-rs
File Size                       : 144 kB
File Modification Date/Time     : 2025:11:15 20:24:23-06:00
File Access Date/Time           : 2025:11:15 20:39:58-06:00
File Inode Change Date/Time     : 2025:11:15 20:24:24-06:00
File Permissions                : -rw-r--r--
File Type                       : PDF
File Type Extension             : pdf
MIME Type                       : application/pdf
PDF Version                     : 1.3
Linearized                      : No
Media Box                       : 0, 0, 612, 792
Page Count                      : 2
Profile CMM Type                : Linotronic
Profile Version                 : 2.1.0
Profile Class                   : Display Device Profile
Color Space Data                : RGB
Profile Connection Space        : XYZ
Profile Date Time               : 1998:02:09 06:49:00
Profile File Signature          : acsp
Primary Platform                : Microsoft Corporation
CMM Flags                       : Not Embedded, Independent
Device Manufacturer             : Hewlett-Packard
Device Model                    : sRGB
Device Attributes               : Reflective, Glossy, Positive, Color
Rendering Intent                : Perceptual
Connection Space Illuminant     : 0.9642 1 0.82491
Profile Creator                 : Hewlett-Packard
Profile ID                      : 0
Profile Copyright               : Copyright (c) 1998 Hewlett-Packard Company
Profile Description             : sRGB IEC61966-2.1
Media White Point               : 0.95045 1 1.08905
Media Black Point               : 0 0 0
Red Matrix Column               : 0.43607 0.22249 0.01392
Green Matrix Column             : 0.38515 0.71687 0.09708
Blue Matrix Column              : 0.14307 0.06061 0.7141
Device Mfg Desc                 : IEC http://www.iec.ch
Device Model Desc               : IEC 61966-2.1 Default RGB colour space - sRGB
Viewing Cond Desc               : Reference Viewing Condition in IEC61966-2.1
Viewing Cond Illuminant         : 19.6445 20.3718 16.8089
Viewing Cond Surround           : 3.92889 4.07439 3.36179
Viewing Cond Illuminant Type    : D50
Luminance                       : 76.03647 80 87.12462
Measurement Observer            : CIE 1931
Measurement Backing             : 0 0 0
Measurement Geometry            : Unknown
Measurement Flare               : 0.999%
Measurement Illuminant          : D65
Technology                      : Cathode Ray Tube Display
Red Tone Reproduction Curve     : (Binary data 2060 bytes, use -b option to extract)
Green Tone Reproduction Curve   : (Binary data 2060 bytes, use -b option to extract)
Blue Tone Reproduction Curve    : (Binary data 2060 bytes, use -b option to extract)
Producer                        : macOS Version 15.4.1 (Build 24E263) Quartz PDFContext
Create Date                     : 2025:05:18 23:44:09Z
Modify Date                     : 2025:05:18 23:44:09Z
```

### exiftool-rs Output
```

PDF:CreateDate: 2025:05:18 23:44:09Z
PDF:CreationDate: 2025:05:18 23:44:09Z
PDF:Linearized: No
PDF:MediaBox: 0, 0, 612, 792
PDF:ModDate: 2025:05:18 23:44:09Z
PDF:ModifyDate: 2025:05:18 23:44:09Z
PDF:PDFVersion: 1.3
PDF:PageCount: 2
PDF:Producer: macOS Version 15.4.1 \(Build 24E263\) Quartz PDFContext
Profile:BlueMatrixColumn: 0.14306640625 0.06060791015625 0.7140960693359375
Profile:BlueToneReproductionCurve: (Binary data 2060 bytes, use -b option to extract)
Profile:CMMFlags: Not Embedded, Independent
Profile:ColorSpaceData: RGB
Profile:ConnectionSpaceIlluminant: 0.964202880859375 1 0.8249053955078125
Profile:DeviceAttributes: Reflective, Glossy, Positive, Color
Profile:DeviceManufacturer: IEC 
Profile:DeviceMfgDesc: IEC http://www.iec.ch
Profile:DeviceModel: sRGB
Profile:DeviceModelDesc: IEC 61966-2.1 Default RGB colour space - sRGB
Profile:GreenMatrixColumn: 0.3851470947265625 0.7168731689453125 0.097076416015625
Profile:GreenToneReproductionCurve: (Binary data 2060 bytes, use -b option to extract)
Profile:Luminance: 76.03646850585938 80 87.12461853027344
Profile:MeasurementBacking: 0 0 0
Profile:MeasurementFlare: 0.99945068359375%
Profile:MeasurementGeometry: Unknown
Profile:MeasurementIlluminant: D65
Profile:MeasurementObserver: CIE 1931
Profile:MediaBlackPoint: 0 0 0
Profile:MediaWhitePoint: 0.9504547119140625 1 1.08905029296875
Profile:PrimaryPlatform: Microsoft Corporation
Profile:ProfileCMMType: Lino
Profile:ProfileClass: Display Device Profile
Profile:ProfileConnectionSpace: XYZ
Profile:ProfileCopyright: Copyright (c) 1998 Hewlett-Packard Company
Profile:ProfileCreator: HP  
Profile:ProfileDateTime: 1998:02:09 06:49:00
Profile:ProfileDescription: sRGB IEC61966-2.1
Profile:ProfileFileSignature: acsp
Profile:ProfileID: 0
Profile:ProfileVersion: 2.1.0
Profile:RedMatrixColumn: 0.436065673828125 0.2224884033203125 0.013916015625
Profile:RedToneReproductionCurve: (Binary data 2060 bytes, use -b option to extract)
Profile:RenderingIntent: Perceptual
Profile:Technology: Cathode Ray Tube Display
Profile:ViewingCondDesc: Reference Viewing Condition in IEC61966-2.1
Profile:ViewingCondIlluminant: 19.644500732421875 20.371795654296875 16.80889892578125
Profile:ViewingCondIlluminantType: D50
Profile:ViewingCondSurround: 3.92889404296875 4.0743865966796875 3.361785888671875
```

## MP4 Comparison
### Perl ExifTool Output
```
File Name                       : sample.mp4
Directory                       : /Users/allen/Documents/git/exiftool-rs/tests/fixtures/mp4
File Size                       : 507 bytes
File Modification Date/Time     : 2025:11:09 09:00:25-06:00
File Access Date/Time           : 2025:11:09 18:16:24-06:00
File Inode Change Date/Time     : 2025:11:11 16:50:52-06:00
File Permissions                : -rw-r--r--
File Type                       : MP4
File Type Extension             : mp4
MIME Type                       : video/mp4
Major Brand                     : MP4 Base Media v1 [IS0 14496-12:2003]
Minor Version                   : 0.0.0
Compatible Brands               : isom, iso2, mp41
Movie Header Version            : 0
Create Date                     : 0000:00:00 00:00:00
Modify Date                     : 0000:00:00 00:00:00
Time Scale                      : 1000
Duration                        : 1.00 s
Preferred Rate                  : 1
Preferred Volume                : 100.00%
Matrix Structure                : 1 0 0 0 1 0 0 0 1
Preview Time                    : 0 s
Preview Duration                : 0 s
Poster Time                     : 0 s
Selection Time                  : 0 s
Selection Duration              : 0 s
Current Time                    : 0 s
Next Track ID                   : 1
Handler Type                    : Metadata
Handler Vendor ID               : Apple
Title                           : Sample Video Title
Artist                          : Sample Artist
Album                           : Sample Album
Content Create Date             : 2024
Comment                         : Test MP4 file for ExifTool-RS
Genre                           : Test Genre
Copyright                       : Copyright 2024
Media Data Size                 : 0
Media Data Offset               : 507
Avg Bitrate                     : 0 bps
```

### exiftool-rs Output
```

ItemList:Album: Sample Album
ItemList:Artist: Sample Artist
ItemList:Comment: Test MP4 file for ExifTool-RS
ItemList:ContentCreateDate: 2024
ItemList:Copyright: Copyright 2024
ItemList:Genre: Test Genre
ItemList:Title: Sample Video Title
ItemList:Year: 2024
QuickTime:CompatibleBrands: [isom, iso2, mp41]
QuickTime:CreateDate: 0000:00:00 00:00:00
QuickTime:CurrentTime: 0 s
QuickTime:Duration: 1.00 s
QuickTime:HandlerType: Metadata
QuickTime:HandlerVendorID: Apple
QuickTime:MajorBrand: MP4 Base Media v1 [IS0 14496-12:2003]
QuickTime:MatrixStructure: 1 0 0 0 1 0 0 0 1
QuickTime:MediaDataOffset: 507
QuickTime:MediaDataSize: 0
QuickTime:MinorVersion: 0.0.0
QuickTime:ModifyDate: 0000:00:00 00:00:00
QuickTime:MovieHeaderVersion: 0
QuickTime:NextTrackID: 1
QuickTime:PosterTime: 0 s
QuickTime:PreferredRate: 1
QuickTime:PreferredVolume: 100.00%
QuickTime:PreviewDuration: 0 s
QuickTime:PreviewTime: 0 s
QuickTime:SelectionDuration: 0 s
QuickTime:SelectionTime: 0 s
QuickTime:TimeScale: 1000
QuickTime:Title: QT Title!!
UserData:Title: QT Title!!
```
