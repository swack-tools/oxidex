#!/usr/bin/env bash
if [[ "$*" == *"-j"* ]]; then
  echo '[{"ExifIFD:0x8827": "400"}]'
  exit 0
fi
exit 0
