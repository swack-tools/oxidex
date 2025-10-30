Description: Create a scanner for each file type exiftools has in native rust
Following tag values found in vairous file types
Tag_Value_Index:
  - JPEG
  - EXIF
  - IPTC
  - XMP
  - GPS
  - GeoTiff
  - PLUS
  - ICC_Profile
  - PrintIM
  - Photoshop
  - Apple
  - Canon
  - CanonCustom
  - CanonVRD
  - Casio
  - DJI
  - FLIR
  - FujiFilm
  - GE
  - Google
  - GoPro
  - HP
  - JVC
  - Kodak
  - Leaf
  - Lytro
  - Minolta
  - Motorola
  - Nikon
  - NikonCapture
  - NikonCustom
  - NikonSettings
  - Nintendo
  - Olympus
  - Panasonic
  - Pentax
  - PhaseOne
  - Reconyx
  - Ricoh
  - Samsung
  - Sanyo
  - Sigma
  - Sony
  - SonyIDC
  - Unknown
  - DNG
  - JSON
  - CBOR
  - PLIST
  - CanonRaw
  - KyoceraRaw
  - MinoltaRaw
  - PanasonicRaw
  - SigmaRaw
  - JFIF
  - FlashPix
  - MPF
  - InfiRay
  - Stim
  - Scalado
  - Qualcomm
  - Jpeg2000
  - APP12
  - AFCP
  - DarwinCore
  - FotoStation
  - PhotoMechanic
  - Microsoft
  - GIMP
  - MIE
  - Trailer
  - GIF
  - BMP
  - BPG
  - WPG
  - ICO
  - PICT
  - PNG
  - MNG
  - FLIF
  - DjVu
  - DPX
  - OpenEXR
  - ZISRAW
  - MRC
  - LIF
  - MIFF
  - PCX
  - PGF
  - PSP
  - PhotoCD
  - Radiance
  - PFM
  - PDF
  - PostScript
  - ID3
  - ITC
  - QuickTime
  - RIFF
  - FLAC
  - GM
  - Parrot
  - AAC
  - Ogg
  - Vorbis
  - Opus
  - Theora
  - DSF
  - WavPack
  - APE
  - Audible
  - MPC
  - MPEG
  - M2TS
  - H264
  - MISB
  - Matroska
  - MOI
  - MXF
  - DV
  - Flash
  - Real
  - Red
  - AIFF
  - ASF
  - TNEF
  - WTV
  - DICOM
  - FITS
  - XISF
  - HTML
  - Palm
  - Torrent
  - EXE
  - LNK
  - PCAP
  - Font
  - VCard
  - Text
  - RSRC
  - Rawzor
  - ZIP
  - RTF
  - OOXML
  - iWork
  - ISO
  - MacOS
  - Extra
  - Composite
  - Shortcuts
  - MWG
File_Format_Index:
  JPEG:
    supported_tags:
      - EXIF
      - IPTC
      - XMP
      - GPS
      - ICC_Profile
      - JFIF
      - Photoshop
      - FlashPix
      - MPF
      - Canon
      - CanonRaw
      - Nikon
      - Sony
      - Olympus
      - Panasonic
      - FujiFilm
      - Pentax
      - Casio
      - DJI
      - FLIR
      - InfiRay
      - GoPro
      - Qualcomm
      - PhotoMechanic
      - FotoStation
      - MIE
      - Jpeg2000
      - APP12
      - AFCP
    description: "JPEG files support 15 APP markers for various metadata types"

  PNG:
    supported_tags:
      - EXIF
      - XMP
      - ICC_Profile
      - IPTC
      - Photoshop
      - Text
    description: "PNG stores metadata in chunks (eXIf, tXMP, iCCP, tEXt, zTXt, iTXt)"

  TIFF:
    supported_tags:
      - EXIF
      - IPTC
      - XMP
      - GPS
      - GeoTiff
      - ICC_Profile
      - DNG
      - PrintIM
      - Canon
      - Nikon
      - Sony
      - Olympus
      - Panasonic
      - FujiFilm
      - Pentax
      - Minolta
      - Leaf
      - PhaseOne
    description: "TIFF-based format, foundation for many RAW formats"

  QuickTime:
    supported_tags:
      - XMP
      - GPS
      - CBOR
      - Canon
      - GoPro
      - DJI
      - Sony
      - Panasonic
      - Samsung
      - FLIR
      - Parrot
    description: "MOV/MP4 files with ItemList, UserData, Keys, and Stream metadata"

  PDF:
    supported_tags:
      - XMP
      - ICC_Profile
      - Photoshop
      - JUMBF
      - Jpeg2000
    description: "PDF up to v2.0 with Info, Encrypt, Root, AcroForm, Signature tags"

  RIFF:
    supported_tags:
      - EXIF
      - XMP
      - ICC_Profile
      - ID3
    description: "WAV, AVI, WebP files with LIST chunks (INFO and exif)"

  GIF:
    supported_tags:
      - XMP
      - ICC_Profile
      - IPTC
    description: "GIF89a with extension blocks for metadata"

  BMP:
    supported_tags:
      - EXIF
      - XMP
    description: "Windows Bitmap format with limited metadata support"

  FLAC:
    supported_tags:
      - Vorbis
      - ID3
      - XMP
    description: "Free Lossless Audio Codec with Vorbis comments"

  MPEG:
    supported_tags:
      - ID3
      - XMP
    description: "MPEG audio/video with ID3v2 tags"

  Matroska:
    supported_tags:
      - XMP
      - Theora
      - Opus
      - Vorbis
    description: "MKV/WEBM container format"

  DNG:
    supported_tags:
      - EXIF
      - XMP
      - GPS
      - ICC_Profile
      - Canon
      - Nikon
      - Sony
      - Olympus
      - Panasonic
      - FujiFilm
      - Pentax
    description: "Adobe Digital Negative, TIFF-based RAW format"

  RAW_Formats:
    supported_tags:
      - EXIF
      - XMP
      - GPS
      - Canon
      - CanonRaw
      - Nikon
      - Sony
      - Olympus
      - Panasonic
      - FujiFilm
      - Pentax
      - Minolta
      - MinoltaRaw
      - Sigma
      - SigmaRaw
      - Casio
      - Samsung
      - PhaseOne
      - Leaf
      - KyoceraRaw
      - PanasonicRaw
    description: "Camera-specific RAW formats (CR2, NEF, ARW, ORF, RAF, etc.)"

  PostScript:
    supported_tags:
      - XMP
      - ICC_Profile
      - Photoshop
    description: "EPS/PS files with DSC comments"

  DICOM:
    supported_tags:
      - XMP
    description: "Medical imaging format with extensive tag structure"

  AIFF:
    supported_tags:
      - ID3
      - XMP
    description: "Audio Interchange File Format"

  ASF:
    supported_tags:
      - XMP
      - ID3
    description: "Windows Media Audio/Video container"

  MXF:
    supported_tags:
      - XMP
    description: "Material Exchange Format for professional video"

  Flash:
    supported_tags:
      - XMP
    description: "SWF Flash files"

  OOXML:
    supported_tags:
      - XMP
      - Microsoft
    description: "Office Open XML (DOCX, XLSX, PPTX)"

  iWork:
    supported_tags:
      - XMP
      - Apple
    description: "Apple iWork documents (Pages, Numbers, Keynote)"

  ZIP:
    supported_tags:
      - XMP
    description: "ZIP archives with comment metadata"

  RTF:
    supported_tags:
      - XMP
    description: "Rich Text Format"

  HTML:
    supported_tags:
      - XMP
    description: "HTML files with meta tags"

  Font:
    supported_tags:
      - XMP
    description: "TrueType and OpenType fonts"

  EXE:
    supported_tags:
      - XMP
    description: "Windows executables with resource metadata"

  VCard:
    supported_tags:
      - Text
    description: "Electronic business card format"