#!/usr/bin/env bash
if [[ "$*" == *"-j"* ]]; then
  echo '[{"SourceFile": "t.jpg"}]'
  exit 0
fi
exit 0
