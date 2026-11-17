#!/usr/bin/env bash
set -euo pipefail

if [[ "${1:-v2}" != "v2" ]]; then
  echo "Usage: $0 [v2]" >&2
  exit 2
fi
name="sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p "$root/models"
if [[ -f "$root/models/$name/encoder.int8.onnx" ]]; then
  echo "Model already installed: $root/models/$name"
  exit 0
fi

archive="$root/models/$name.tar.bz2"
url="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/$name.tar.bz2"
curl --fail --location --retry 3 --output "$archive.part" "$url"
mv "$archive.part" "$archive"
tar -xjf "$archive" -C "$root/models"
rm "$archive"
echo "Installed model at $root/models/$name"
