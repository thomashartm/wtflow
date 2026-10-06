#!/usr/bin/env bash
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
wtflow_bin="$root/target/debug/wtflow"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for flow in testdata/golden/*.flow.yaml; do
  base=$(basename "$flow" .flow.yaml)
  "$wtflow_bin" check "$flow" > "$tmp/$base.lint.txt"
  "$wtflow_bin" render "$flow" > "$tmp/$base.mmd"
  diff -u "testdata/golden/$base.lint.txt" "$tmp/$base.lint.txt"
  diff -u "testdata/golden/$base.mmd" "$tmp/$base.mmd"
done
for snapshot in crates/wtflow-render/tests/snapshots/*.snap; do
  base=$(basename "$snapshot" .snap)
  sed '1,4d' "$snapshot" > "$tmp/$base.mmd"
  diff -u "testdata/golden/$base.mmd" "$tmp/$base.mmd"
done
