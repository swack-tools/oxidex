# IPTC carrier controls

These nine tiny files wrap source-derived IIM datasets in PSD image resources, TIFF IFD0 `0x83bb`, EPS Photoshop DSC data, or PDF `/ImageResources`. `native-selected.json` records only IPTC keys from pinned ExifTool 13.59 with `-j -a -G1:4 -s`. The pinned Perl executable and ExifTool source, exact baseline comparison, native commands, fixture hashes, and full unfiltered native JSON are retained under `${OXIDEX_OPS_DIR:-$HOME/oxidex-ops}/evidence/iptc-carrier-recovery-20261001/`.

`within-block` cases retain record versions as integers, Category `0042` as text, and two By-line datasets as one list. `two-blocks` cases retain distinct physical occurrences. `three-blocks.eps` checks Copy1 and Copy2; `pdf-same-value-two-blocks.pdf` checks that identical By-line values in separate directories remain distinct while ObjectName differs. `tests/iptc_carrier_occurrences.rs` compares every recorded IPTC key and value, including group and copy labels.
