"""Carrier-availability map for Step 28's corpus-synthesis harness.

Maps each of the 82 ExifTool modules that codegen.py emits ProcessBinaryData
tables from to whatever the combined-samples corpus (4,238 files, confirmed
against the pinned 13.59 oracle -- see synth_classify.py) can actually offer
as a carrier to write into.

Two carrier kinds:
  - ("vendor_dir", "Canon")   a manufacturer subdirectory of real-world JPEGs
                               (MakerNote tables read/write the same way
                               regardless of container -- JPEG suffices).
  - ("file", "CanonRaw.crw")  a single named exemplar file (ExifTool's own
                               t/images/-style naming: "<Module>.<ext>").

This mapping was built by hand from the corpus's actual directory listing
(13 manufacturer dirs + ~150 single-exemplar files under combined-samples/),
not guessed from module names alone -- see the classification report for the
raw listing it was built from. Confidence is annotated per entry because a
few module<->carrier associations are directory-level (the file/dir exists)
but tag-level presence is genuinely uncertain until the generation step
empirically writes into it (e.g. CanonCustom function tags may or may not be
populated on any single sample Canon JPEG). NONE means no plausible carrier
was found anywhere in the corpus.
"""

# module -> (kind, target, confidence, note)
#   kind: "vendor_dir" | "file" | "none"
#   confidence: "high" | "low"  (low = carrier file/dir exists but whether
#     THIS module's tags actually populate on it is unverified until the
#     generation/round-trip step runs)
CARRIER_MAP = {
    "AIFF": ("file", "AIFF.aif", "high", ""),
    "APE": ("file", "APE.ape", "high", ""),
    "ASF": ("file", "ASF.wmv", "high", ""),
    "BMP": ("file", "BMP.bmp", "high", ""),
    "BPG": ("file", "BPG.bpg", "high", ""),
    "Canon": ("vendor_dir", "Canon", "high", ""),
    "CanonCustom": ("vendor_dir", "Canon", "low", "custom-function tags are model/firmware dependent"),
    "CanonRaw": ("file", "CanonRaw.crw", "high", "CIFF/CRW-specific module, not any Canon file"),
    "CanonVRD": ("file", "CanonVRD.vrd", "high", ""),
    "Casio": ("none", None, "high", "no Casio file or dir in corpus"),
    "DJI": ("vendor_dir", "DJI", "high", ""),
    "DNG": ("file", "DNG.dng", "high", ""),
    "DPX": ("file", "DPX.dpx", "high", ""),
    "DSF": ("none", None, "high", "no .dsf DSD-audio file in corpus"),
    "DjVu": ("file", "DjVu.djvu", "high", ""),
    "EXE": ("file", "EXE.exe", "high", ""),
    "FLAC": ("file", "FLAC.flac", "high", ""),
    "FLIR": ("file", "FLIR.fpf", "high", ""),
    "FlashPix": ("file", "FlashPix.ppt", "low", "OLE/FlashPix compound doc, module match unverified"),
    "Font": ("file", "Font.ttf", "high", ""),
    "FotoStation": ("none", None, "high", "no dedicated exemplar in corpus"),
    "FujiFilm": ("vendor_dir", "FujiFilm", "high", ""),
    "GIF": ("file", "GIF.gif", "high", ""),
    "GIMP": ("file", "GIMP.xcf", "high", ""),
    "GM": ("none", None, "high", "no dedicated exemplar in corpus"),
    "GoPro": ("vendor_dir", "GoPro", "high", ""),
    "H264": ("none", None, "high", "no raw H264 elementary stream in corpus"),
    "HP": ("none", None, "high", "no dedicated exemplar in corpus"),
    "ICC_Profile": ("file", "ICC_Profile.icc", "high", ""),
    "ICO": ("file", "ICO.ico", "high", ""),
    "ID3": ("file", "MP3.mp3", "high", ""),
    "ISO": ("file", "ISO.iso", "high", ""),
    "ITC": ("file", "ITC.itc", "high", ""),
    "InfiRay": ("none", None, "high", "no InfiRay thermal file in corpus"),
    "JPEG": ("file", "<any .jpg>", "high", "native JPEG segment tables; corpus is 4085 jpgs"),
    "Jpeg2000": ("file", "Jpeg2000.jp2", "high", ""),
    "Kandao": ("none", None, "high", "no Kandao VR file in corpus"),
    "Kodak": ("none", None, "high", "no Kodak file or dir in corpus"),
    "KyoceraRaw": ("file", "KyoceraRaw.raw", "high", ""),
    "LNK": ("file", "LNK.lnk", "high", ""),
    "MNG": ("none", None, "high", "no .mng file in corpus"),
    "MOI": ("file", "MOI.moi", "high", ""),
    "MPEG": ("none", None, "high", "no .mpg/.mpeg elementary stream in corpus"),
    "MPF": ("none", None, "low", "no dedicated exemplar; may exist embedded in vendor JPEGs, unverified"),
    "MRC": ("file", "MRC.mrc", "high", ""),
    "MXF": ("file", "MXF.mxf", "high", ""),
    "Microsoft": ("none", None, "high", "no dedicated exemplar in corpus"),
    "Minolta": ("file", "Minolta.mrw", "high", ""),
    "MinoltaRaw": ("file", "Minolta.mrw", "high", ""),
    "Nikon": ("vendor_dir", "Nikon", "high", ""),
    "NikonCapture": ("vendor_dir", "Nikon", "low", "capture-editing tags not guaranteed on any given sample"),
    "NikonCustom": ("vendor_dir", "Nikon", "low", "custom-settings layout is model dependent (D5 vs others)"),
    "Nintendo": ("none", None, "high", "no Nintendo camera file in corpus"),
    "Olympus": ("vendor_dir", "Olympus", "high", ""),
    "Opus": ("file", "Opus.opus", "high", ""),
    "PCX": ("file", "PCX.pcx", "high", ""),
    "PGF": ("file", "PGF.pgf", "high", ""),
    "PNG": ("file", "PNG.png", "high", ""),
    "PSP": ("file", "PSP.psp", "high", ""),
    "Palm": ("file", "Palm.mobi", "high", ""),
    "Panasonic": ("vendor_dir", "Panasonic", "high", ""),
    "PanasonicRaw": ("file", "Panasonic.rw2", "high", ""),
    "Parrot": ("none", None, "high", "no Parrot drone file in corpus"),
    "Pentax": ("vendor_dir", "Pentax", "high", ""),
    "PhotoCD": ("file", "PhotoCD.pcd", "high", ""),
    "Photoshop": ("file", "Photoshop.psd", "high", "also present as APP13 in many corpus jpgs"),
    "QuickTime": ("file", "QuickTime.mov", "high", ""),
    "RIFF": ("file", "RIFF.avi", "high", ""),
    "Reconyx": ("none", None, "high", "no Reconyx trail-camera file in corpus"),
    "Red": ("file", "Red.r3d", "high", ""),
    "Ricoh": ("none", None, "high", "no Ricoh file or dir in corpus"),
    "Samsung": ("vendor_dir", "Samsung", "high", ""),
    "Sanyo": ("none", None, "high", "no Sanyo file in corpus"),
    "Sigma": ("file", "Sigma.x3f", "high", ""),
    "SigmaRaw": ("file", "Sigma.x3f", "high", ""),
    "Sony": ("vendor_dir", "Sony", "high", ""),
    "Stim": ("none", None, "high", "no dedicated exemplar in corpus"),
    "Theora": ("none", None, "high", "no .ogv Theora stream in corpus"),
    "Vorbis": ("file", "Vorbis.ogg", "high", ""),
    "WavPack": ("none", None, "high", "no .wv file in corpus"),
    "ZIP": ("file", "ZIP.zip", "high", ""),
    "ZISRAW": ("file", "ZISRAW.czi", "high", ""),
}

# Module/table pairs found reachable at runtime by grepping every non-test,
# non-font `find_table(...)` call site in src/ (see classification report for
# the full derivation, including the two model-dispatched cases: Sony's
# ExtraInfo/ExtraInfo2/ExtraInfo3 selection in amount.rs::extract_extra_info,
# and Canon's ShotInfo/AFInfo selection in raw/metadata.rs::canon_crw_tag_key).
# This set independently reproduces the task's stated "22 reachable" figure
# exactly.
REACHABLE = {
    ("Pentax", "MOV"),
    ("EXE", "Main"),
    ("PhotoCD", "Main"),
    ("DPX", "Main"),
    ("ID3", "v1"),
    ("AIFF", "Common"),
    ("Olympus", "DSS"),
    ("MPF", "MPImage"),
    ("Casio", "QVCI"),
    ("Canon", "CMP1"),
    ("Canon", "ShotInfo"),
    ("Canon", "AFInfo"),
    ("CanonVRD", "Ver2"),
    ("Canon", "FileInfo"),
    ("Canon", "CameraSettings"),
    ("Samsung", "Main"),
    ("Sony", "CameraInfo"),
    ("Sony", "Panorama"),
    ("Sony", "ExtraInfo"),
    ("Sony", "ExtraInfo2"),
    ("Sony", "ExtraInfo3"),
    ("DJI", "ThermalParams2"),
}
assert len(REACHABLE) == 22, f"expected 22 reachable tables, derived {len(REACHABLE)}"
