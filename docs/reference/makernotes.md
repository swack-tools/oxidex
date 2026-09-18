# MakerNotes

A MakerNote is a vendor-specific block, usually in EXIF tag `0x927C`, and
every vendor lays it out differently. OxiDex chooses a vendor's tables the
way ExifTool does: by signature, or by the `Make` string. The logic is in
`src/parsers/tiff/makernote_dispatcher.rs`. Tags come out under the
vendor's family 1 group, for example `Canon:LensModel` or
`Nikon:ShutterCount`.

## Dispatch

| Routed by | Vendors |
| --- | --- |
| Signature, before `Make` is checked | Phase One (including Leaf-branded IIQ) |
| `Make` prefix | Olympus / OM Digital Solutions; Pentax / Asahi Optical; Ricoh Imaging (Pentax tables); General Imaging (GE); Samsung (Samsung bodies with a Pentax `AOC` note go to Pentax) |
| Exact `Make` | Canon, Nikon, Sony, Panasonic, FujiFilm, Leica, Minolta / Konica Minolta, Apple, DJI, FLIR, GoPro, InfiRay, Nintendo, Parrot, Reconyx, RED, Casio, GE, HP, JVC, Kodak, Motorola, Ricoh, Sanyo |
| Software that writes maker-note blocks | Capture One, FotoStation, GIMP, InDesign, Nikon Capture, Photoshop, Scalado |
| Separate path | Sigma (`src/core/tiff_helpers.rs`), Minolta MRW (`src/parsers/raw/minolta_makernote.rs`) |

Google, Microsoft and Qualcomm are deliberately not dispatched. The code
records why at each point.

Many vendor tables are transcribed from ExifTool's Perl and evaluated by the
generated engines in `src/exiftool_tables/`. The others are still hand
parsers, which the v2 work retires field by field. See
[Architecture](/architecture/#the-v2-direction).

## How complete is it?

Coverage differs a great deal from vendor to vendor and from model to model.
It is measured, not listed. The per-format
[ExifTool comparison](/reference/comparison/) and the
[corpus read observations](/reference/catalog-corpus-observed) report what
matches ExifTool 13.59 today. A tag that cannot be decoded exactly is
omitted rather than approximated.

An undecoded MakerNote block is kept as a hex fallback tag. It is hidden
unless you pass `--extended-output`.

## Examples

```bash
oxidex -j -a -G1 photo.jpg | grep '"Canon:'
oxidex -Canon:CanonFirmwareVersion -Canon:LensModel photo.jpg
oxidex -Nikon:ShutterCount photo.nef
```

```rust,no_run
use oxidex::core::operations::read_metadata;
use std::path::Path;

fn main() -> oxidex::error::Result<()> {
    let map = read_metadata(Path::new("photo.jpg"))?;
    for (key, value) in map.iter().filter(|(k, _)| k.starts_with("Canon:")) {
        println!("{key}: {value:?}");
    }
    Ok(())
}
```
