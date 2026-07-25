#!/usr/bin/env bash
if [[ "$*" == *"-j"* ]]; then
  echo '[{"ExifIFD:ISO": "888"}]'
  exit 0
fi
exit 0
