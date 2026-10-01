# IPTC carrier controls

These tiny files wrap source-derived IIM datasets in PSD image resources, TIFF IFD0 `0x83bb`, EPS Photoshop DSC data, or PDF `/ImageResources`. `native-selected.json` and `native-selected-n.json` record only IPTC keys from pinned ExifTool 13.59 with `-j -a -G1:4 -s`. The pinned Perl executable and ExifTool source, exact baseline comparison, native commands, fixture hashes, and full unfiltered native JSON are retained under `${OXIDEX_OPS_DIR:-$HOME/oxidex-ops}/evidence/iptc-carrier-recovery-20261001/`.

`within-block` cases retain record versions as integers, Category `0042` as text, and two By-line datasets as one list. `two-blocks` cases retain distinct physical occurrences. `three-blocks.eps` checks Copy1 and Copy2; `pdf-same-value-two-blocks.pdf` checks that identical By-line values in separate directories remain distinct while ObjectName differs. `tests/iptc_carrier_occurrences.rs` compares every recorded IPTC key and value, including group and copy labels.

The converted controls in every carrier distinguish PrintConv-only enum labels from ValueConv dates/times under native `-n` and OxiDex `--no-print-conv` (OxiDex reserves `-n` for dry-run). The linked TIFF control proves one counter spans IFD0 and IFD1.

Leading-zero controls use EditorialUpdate `01`, Urgency `05`, and Category `0042`: normal output keeps their source PrintConv labels, while raw output retains the exact strings `01`, `05`, and `0042` in all four carriers.

EPS representation controls follow PostScript.pm: raw 8BIM bytes outside a `%%BeginPhotoshop` DSC block are ignored, a raw-plus-hex replay contributes one physical occurrence, and two separate Photoshop DSC blocks with equal IPTC payloads remain two occurrences. `eps-one-hex-sort.eps` checks copy-aware group ordering; `eps-interleaved-list.eps` checks that a By-line list spanning a scalar stays at its first IIM record position under G1, G1:4, and G4.
