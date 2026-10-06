#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-only
set -eu
fail() { printf '%s\n' "HarvestCircle aggregate: $*" >&2; exit 1; }
[ "$#" -eq 0 ] || fail "unexpected arguments"
repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_root"
# Keep each selected lane's explicit mode, without exporting command-line Make
# overrides into its standalone test fixtures or starting inherited parallel jobs.
unset MAKEFLAGS MAKEOVERRIDES MFLAGS
for tool in make rustup cargo java node corepack; do
    command -v "$tool" >/dev/null 2>&1 || fail "required tool is unavailable: $tool"
done
[ -x ./gradlew ] || fail "Gradle wrapper is unavailable"
java_major=$(java -version 2>&1 | sed -n 's/.*version "\([0-9]*\).*/\1/p' | head -n 1)
[ "$java_major" = 21 ] || fail "selected JDK must be 21; install/select it explicitly"
# Preflight before any consumer. Neither rustup run nor offline Corepack installs.
rustup run 1.97.1 cargo --version
# Refuse a wrapper download when its selected distribution is not installed.
gradle_version=$(sed -n 's@^distributionUrl=.*\/gradle-\([0-9.]*\)-bin.zip$@\1@p' gradle/wrapper/gradle-wrapper.properties)
[ -n "$gradle_version" ] || fail "unrecognized pinned Gradle distribution"
gradle_cache_root=${GRADLE_USER_HOME:-${HOME:?}/.gradle}
gradle_ready=false
for executable in "$gradle_cache_root/wrapper/dists/gradle-$gradle_version-bin/"*/"gradle-$gradle_version/bin/gradle"; do
    if [ -x "$executable" ]; then gradle_ready=true; fi
done
[ "$gradle_ready" = true ] || fail "pinned Gradle distribution is not cached; install explicitly"
export COREPACK_ENABLE_NETWORK=0
make --no-print-directory MAKEOVERRIDES= BUILD_MODE="${HARVESTCIRCLE_BUILD_MODE:-standalone}" web-doctor
for lane in repo-check native-check web-check web-integration-check interop-check; do
    printf '%s\n' "harvestcircle.aggregate.$lane=running"
    make --no-print-directory MAKEOVERRIDES= BUILD_MODE="${HARVESTCIRCLE_BUILD_MODE:-standalone}" \
        GRADLE='./gradlew -Porg.gradle.java.installations.auto-download=false' "$lane"
    printf '%s\n' "harvestcircle.aggregate.$lane=pass"
done
printf '%s\n' 'harvestcircle.aggregate.qualification=source_checks_only'
