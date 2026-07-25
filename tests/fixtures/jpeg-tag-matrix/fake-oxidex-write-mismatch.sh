#!/usr/bin/env bash
if [[ "$*" == *"-j"* ]]; then
  echo '[{"ExifIFD:ISO": "777"}]'
  exit 0
fi
exit 0
