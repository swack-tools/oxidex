# Progressive clear regression

`inter_scan_comment.jpg` is the 32×32 RGB Pillow progressive JPEG from
`976-final-fd537-acceptance/jpeg-clear-probe.py` (2026-09-27). The pixel
formula is ((x*17+y*3)%256, (x*5+y*13)%256, (x*x+y*7)%256), quality 90.
A COM segment immediately before the second SOS contains literal FF D9.
`image_only.jpg` removes exactly that COM and the initial JFIF APP0 by
their declared lengths; every image byte and all ten scans remain identical.
The original Pillow probe decoded the injected fixture, and macOS `sips`
converted both checked-in files to byte-identical BMPs (SHA256
083a6f184c911fad331eb0a2f711f8d336832e131567d36bd90bb3058a73b3b8).
Pinned ExifTool 13.59 also truncates this input under -all=; parity is not
the acceptance oracle.

SHA256:
- inter_scan_comment.jpg: 7bc1ef536bcbde9a623c1a55668443aee8e7fe33f7ec89568e8a8f5a305ac9fc
- image_only.jpg: 9fee25970d3d6b95f978ea3001c0ae1f854b8803ba8077923e45e35e38d352d6
