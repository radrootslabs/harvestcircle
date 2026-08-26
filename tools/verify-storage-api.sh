#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
temporary_root=${TMPDIR:-/tmp}
output=$(mktemp "$temporary_root/harvestcircle-storage-api.XXXXXX")
trap 'rm -f "$output"' EXIT HUP INT TERM

cd "$repository_root"
cargo +nightly-2026-07-16 public-api \
  --manifest-path core/Cargo.toml \
  -p harvestcircle_storage \
  --all-features \
  -sss >"$output"
cmp core/compatibility/harvestcircle-storage-api-v1.txt "$output"
