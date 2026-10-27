#!/bin/sh
set -eu

fail() { printf '%s\n' "HarvestCircle prerequisites: $*" >&2; exit 2; }
require() { command -v "$1" >/dev/null 2>&1 || fail "$1 is missing; $2"; }

# Check existing ancestors without creating or repairing host directories/links.
check_cache_path() {
    cache_path=$1
    case "$cache_path" in /*) : ;; *) cache_path="$PWD/$cache_path" ;; esac
    while [ ! -d "$cache_path" ]; do
        [ ! -L "$cache_path" ] || fail "Gradle cache has a broken directory link: $cache_path; select a usable GRADLE_USER_HOME or BUILD_MODE=governed"
        [ ! -e "$cache_path" ] || fail "Gradle cache path is not a directory: $cache_path; select a usable GRADLE_USER_HOME or BUILD_MODE=governed"
        cache_path=${cache_path%/*}
        [ -n "$cache_path" ] || cache_path=/
    done
    [ -w "$cache_path" ] && [ -x "$cache_path" ] || fail "Gradle cache directory is not writable/searchable: $cache_path; select a usable GRADLE_USER_HOME or BUILD_MODE=governed"
}

case "${1:-}" in
    common)
        require git 'install Git and use a Git checkout'
        require cargo 'install Rust through rustup'
        require rustup 'install Rust through rustup'
        require java 'install and select JDK 21'
        require cc 'install the platform C/C++ build tools'
        [ -x ./gradlew ] || fail 'the executable Gradle wrapper is missing; restore the checkout'
        git rev-parse --verify HEAD >/dev/null 2>&1 || fail 'source identity is unavailable; use a Git clone'
        rustup run 1.99.0 rustc --version || fail 'install Rust 1.99.0 with rustup toolchain install 1.99.0 --component clippy --component rustfmt'
        components=$(rustup component list --toolchain 1.99.0 --installed)
        case "$components" in *clippy-*rustfmt-*) : ;; *) fail 'install clippy and rustfmt for Rust 1.99.0 with rustup component add --toolchain 1.99.0 clippy rustfmt' ;; esac
        selected_java=java
        if [ -n "${JAVA_HOME:-}" ]; then selected_java="$JAVA_HOME/bin/java"; fi
        [ "$selected_java" = java ] || [ -x "$selected_java" ] || fail 'JAVA_HOME must identify JDK 21'
        java_output=$("$selected_java" -version 2>&1) || fail 'the selected Java runtime cannot run'
        java_major=$(printf '%s\n' "$java_output" | sed -n 's/.*version "\([0-9]*\).*/\1/p' | sed -n '1p')
        [ "$java_major" = 21 ] || fail 'select JDK 21 using JAVA_HOME and PATH'
        case "$(uname -s)" in
            Darwin) require xcrun 'install Xcode Command Line Tools'; xcrun --find clang >/dev/null || fail 'select an installed Xcode Command Line Tools compiler' ;;
            Linux) require pkg-config 'install pkg-config and the OpenSSL development headers'; pkg-config --exists openssl || fail 'install the OpenSSL development headers' ;;
            *) fail 'the qualified development hosts are macOS and Linux' ;;
        esac
        [ -n "${GRADLE_USER_HOME:-}" ] || fail 'GRADLE_USER_HOME is empty; run through the root Makefile or select a usable directory'
        check_cache_path "$GRADLE_USER_HOME"
        check_cache_path "$GRADLE_USER_HOME/caches"
        gradle_version=$(sed -n 's@^distributionUrl=.*gradle-\([0-9][0-9.]*\)-bin.zip.*@\1@p' gradle/wrapper/gradle-wrapper.properties)
        [ -n "$gradle_version" ] || fail 'cannot determine the pinned Gradle version from the wrapper properties'
        check_cache_path "$GRADLE_USER_HOME/caches/$gradle_version/jvms"
        printf '%s\n' "harvestcircle.gradle.home=$GRADLE_USER_HOME"
        printf '%s\n' 'harvestcircle.prerequisites=ready'
        ;;
    licenses) require cargo-deny 'install cargo-deny with cargo install --locked cargo-deny' ;;
    api)
        require cargo-public-api 'install cargo-public-api with cargo install --locked cargo-public-api'
        rustup run nightly-2026-07-16 rustc --version || fail 'install the API snapshot toolchain with rustup toolchain install nightly-2026-07-16'
        ;;
    audit)
        require cargo-audit 'install cargo-audit with cargo install --locked cargo-audit'
        require cargo-deny 'install cargo-deny with cargo install --locked cargo-deny'
        ;;
    *) fail 'expected common, licenses, api or audit' ;;
esac
