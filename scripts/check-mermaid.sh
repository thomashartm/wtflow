#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
mkdir -p target/mermaid
# One container validates all diagrams. The image's entrypoint is mmdc.
docker run --rm --entrypoint /bin/sh -v "$PWD:/data" minlag/mermaid-cli -c '
  set -eu
  for input in /data/testdata/golden/*.mmd; do
    mmdc -p /puppeteer-config.json -i "$input" -o "/data/target/mermaid/$(basename "$input" .mmd).svg"
  done
'
