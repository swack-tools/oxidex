#!/usr/bin/env bash
# Fake exiftool for flag_noops tests: always reports success ("1 image files
# updated"), so any tag list run through it should come out with noop:None.
if [[ "$*" == *"-ver"* ]]; then echo "13.55"; exit 0; fi
echo "    1 image files updated"
exit 0
