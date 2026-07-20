#!/usr/bin/env bash
if [[ "$*" == *"-validate"* ]]; then
  exit 0
fi
if [[ "$*" == *"-j"* ]]; then
  echo '[{"ExifIFD:ISO": "400"}]'
  exit 0
fi
exit 0
