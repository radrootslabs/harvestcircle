#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
set -eu
fail() { printf '%s\n' "HarvestCircle interop check: $*" >&2; exit 1; }
[ "$#" -eq 0 ] || fail "unexpected arguments"
repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
for tool in node corepack cargo; do
    command -v "$tool" >/dev/null 2>&1 || fail "required tool is unavailable: $tool"
done
cd "$repository_root/web"
node tools/doctor.mjs
cd "$repository_root"
cargo +1.97.1 --version
cargo +1.97.1 test --manifest-path contracts/interop/food_availability/oracle/Cargo.toml --locked --test native_manifest -- --nocapture
cd "$repository_root/web"
corepack pnpm exec node --test tests/conformance/interop-consumer.test.ts
printf '%s\n' 'harvestcircle.interop.fixture_consumers=pass' 'harvestcircle.interop.qualification=source_only_no_Message_or_deployed_Tera_claim'
