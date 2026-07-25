#!/usr/bin/env bash
if [[ "$*" == *"-j"* ]]; then
  echo '[{"ExifIFD:ISO": "400"}]'
  exit 0
fi
exit 0
