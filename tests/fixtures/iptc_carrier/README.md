# IPTC carrier controls

These tiny files wrap source-derived IIM datasets in PSD image resources, TIFF IFD0 `0x83bb`, EPS Photoshop DSC data, or PDF `/ImageResources`. `native-selected.json` and `native-selected-n.json` record only IPTC keys from pinned ExifTool 13.59 with `-j -a -G1:4 -s`. The pinned Perl executable and ExifTool source, exact baseline comparison, native commands, fixture hashes, and full unfiltered native JSON are retained under `${OXIDEX_OPS_DIR:-$HOME/oxidex-ops}/evidence/iptc-carrier-recovery-20261001/`.

`within-block` cases retain record versions as integers, Category `0042` as text, and two By-line datasets as one list. `two-blocks` cases retain distinct physical occurrences. `three-blocks.eps` checks Copy1 and Copy2; `pdf-same-value-two-blocks.pdf` checks that identical By-line values in separate directories remain distinct while ObjectName differs. `tests/iptc_carrier_occurrences.rs` compares every recorded IPTC key and value, including group and copy labels.

The converted controls in every carrier distinguish PrintConv-only enum labels from ValueConv dates/times under native `-n` and OxiDex `--no-print-conv` (OxiDex reserves `-n` for dry-run). The linked TIFF control proves one counter spans IFD0 and IFD1.

Leading-zero controls use EditorialUpdate `01`, Urgency `05`, and Category `0042`: normal output keeps their source PrintConv labels, while raw output retains the exact strings `01`, `05`, and `0042` in all four carriers.

EPS representation controls follow PostScript.pm: raw 8BIM bytes outside a `%%BeginPhotoshop` DSC block are ignored, a raw-plus-hex replay contributes one physical occurrence, and two separate Photoshop DSC blocks with equal IPTC payloads remain two occurrences. `eps-one-hex-sort.eps` checks copy-aware group ordering; `eps-interleaved-list.eps` checks that a By-line list spanning a scalar stays at its first IIM record position under G1, G1:4, and G4.

DSC recognition controls reject a marker embedded in another comment and Photoshop blocks inside `%%BeginDocument`/`%%EndDocument`; they accept a single-percent mixed-case `BeginPhotoshop`/matching `EndPhotoshop` pair. These follow pinned PostScript.pm line-anchored, case-insensitive token handling.

The EPS admission matrix pins these PostScript.pm boundaries with native normal and raw output:

| Control | Source boundary | IPTC By-line |
| --- | --- | --- |
| `eps-mismatched-document-end.eps` | one-percent EndDocument does not close a two-percent BeginDocument | none |
| `eps-nested-document.eps` | matching nested BeginDocument/EndDocument remains hidden until the outer end | Visible |
| `eps-dos-preview-document.eps` | DOS EPS preview bytes are outside the declared PostScript section | Visible |
| `eps-dos-invalid-offset.eps` | a declared PostScript offset without `%!PS` is rejected | none |
| `eps-dos-overlong-section.eps` | an overlong PostScript length is clamped to file end | Visible |
| `eps-icc-mode-hidden.eps` | Photoshop-looking lines inside an ICC profile block are payload | Visible |
| `eps-xml-mode-hidden.eps` | Photoshop-looking lines inside an explicit XML packet block are payload | Visible |
| `eps-binary-mode-hidden.eps` | declared binary bytes are skipped before DSC recognition | Visible |
| `eps-binary-mixed-newline-buffered.eps` | BeginBinary inside an alternate-newline buffer does not seek over subsequent DSC lines | Hidden, then Visible Copy1 |
| `eps-binary-mixed-last-segment.eps` | BeginBinary on the last buffered segment may seek after the queue empties | Visible |
| `eps-hex-odd-line.eps` | odd hex nibbles are padded per line before the next line | Alice |
| `eps-stray-xpacket-mode-hidden.eps` | a stray XMP packet hides Photoshop-looking lines, then scanning resumes after xpacket-end | Visible |

Linked BigTIFF uses the same physical IPTC counter across IFD0 and IFD1 as ordinary TIFF. The primary-CR EPS control keeps a single-percent Photoshop block intact when a later `BeginBinary: 1` line ends in CRLF: the seek consumes the LF rather than the marker's first percent sign.

The `eps-binary-{lf,cr,crlf}-{lf,cr,crlf}-skip{0,1}.eps` grid tests all 18 combinations of the initial record separator, the local `BeginBinary` line ending, and a zero- or one-byte seek. Its normal and raw golden dictionaries were obtained from pinned ExifTool 13.59; full native commands and fixture hashes are in `eps-newline-crossproduct/native-current-crossproduct.json` under the evidence directory. This grid guards the seek origin at the primary newline boundary, including the later-CRLF case above.
