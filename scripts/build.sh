#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(git -C "${script_dir}" rev-parse --show-toplevel)"
configuration="Debug"

if [[ "${1:-}" == "--release" ]]; then
  configuration="Release"
  shift
fi

if [[ "$#" -ne 0 ]]; then
  echo "usage: ./scripts/build.sh [--release]" >&2
  exit 2
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "radroots_studio_app host builds currently require macOS" >&2
  exit 1
fi

exec env CONFIGURATION="${configuration}" "${repo_root}/platforms/macos/Scripts/build_host.sh"
