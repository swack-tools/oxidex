#!/usr/bin/env bash
if [[ "$*" == *"-validate"* ]]; then
  echo "Warning: Non-standard count (1) for ExifIFD:ISO"
  exit 0
fi
if [[ "$*" == *"-j"* ]]; then
  echo '[{"ExifIFD:ISO": "400"}]'
  exit 0
fi
exit 0
