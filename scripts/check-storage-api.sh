#!/bin/sh
set -eu

[ "$#" -eq 0 ] || { printf '%s\n' "Usage: $0" >&2; exit 2; }

repository_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
temporary_root=${TMPDIR:-/tmp}
output=$(mktemp "$temporary_root/harvestcircle-storage-api.XXXXXX")
trap 'rm -f "$output"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

cd "$repository_root"
cargo +nightly-2026-07-16 public-api \
  --manifest-path core/Cargo.toml \
  -p harvestcircle_storage \
  --all-features \
  -sss >"$output"
cmp core/compatibility/harvestcircle-storage-api-v3.txt "$output"
