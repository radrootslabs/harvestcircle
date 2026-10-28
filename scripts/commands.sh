#!/bin/sh
# Internal lifecycle dispatcher. The root Makefile supplies configuration.
set -eu

fail() { printf '%s\n' "HarvestCircle commands: $*" >&2; exit 2; }
[ "$#" -eq 1 ] || fail 'expected one command; use make help'
command_name=$1
repository_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repository_root"

case "$command_name" in
    help)
        printf '%s\n' \
            'Start: doctor dev run demo DEMO_RUN=run-name dev-check' \
            'Build: build check clean' \
            'Focused checks: repo-check format format-fix lint test ui-check hub-check build-logic-check mode-check bindings licenses source-check integration-check' \
            'Design: design-source-check design-goldens-update' \
            'Maintenance: metadata lock' \
            'Complete development: development-check' \
            'Packaging: package host-package-check' \
            'Other checks: audit host-ui-lifecycle-check' \
            'Linux qualification: BUILD_MODE=governed linux-check' \
            'Standalone is the default; add BUILD_MODE=governed and optional EXTBUILD_MACHINE_CONFIG=/path/to/profile.toml for extbuild.'
        exit 0
        ;;
    _require-governed)
        [ "${HARVESTCIRCLE_BUILD_MODE:-}" = governed ] || fail 'This check requires BUILD_MODE=governed'
        exit 0
        ;;
    doctor|repo-check|lock|metadata|build-logic-check|mode-check|design-source-check|\
    design-goldens-update|format|format-fix|lint|test|check|build|\
    bindings|dev-check|ui-check|hub-check|dev|demo|run|audit|licenses|package|\
    host-package-check|source-check|integration-check|host-ui-lifecycle-check|clean|linux-check) ;;
    *) fail "unknown command: $command_name; use make help" ;;
esac

case "${HARVESTCIRCLE_BUILD_MODE:-}" in
    standalone|governed) ;;
    *) fail 'configuration is unavailable; use the root Makefile' ;;
esac
[ -n "${GRADLE:-}" ] && [ -n "${CARGO_MANIFEST:-}" ] && [ -n "${XTASK_MANIFEST:-}" ] && [ -n "${HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN:-}" ] || fail 'configuration is incomplete; use the root Makefile'

case "$command_name" in
    doctor)
        printf '%s\n' "harvestcircle.build.mode=$HARVESTCIRCLE_BUILD_MODE"
        scripts/check-prerequisites.sh common
        exec "$GRADLE" --version
        ;;
    repo-check)
        exec cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" run --manifest-path "$XTASK_MANIFEST" --locked -- qualification-report
        ;;
    lock)
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" generate-lockfile --manifest-path "$CARGO_MANIFEST"
        exec cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" generate-lockfile --manifest-path "$XTASK_MANIFEST"
        ;;
    metadata)
        exec cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" metadata --manifest-path "$CARGO_MANIFEST" --locked --format-version 1 --no-deps
        ;;
    build-logic-check)
        exec "$GRADLE" --no-daemon -p build-logic :contracts:check :plugins:check :plugins:functionalTest
        ;;
    mode-check)
        exec scripts/test-commands.sh
        ;;
    design-source-check)
        exec cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" run --manifest-path "$XTASK_MANIFEST" --locked -- design-source-audit
        ;;
    design-goldens-update)
        exec "$GRADLE" --no-daemon -Pharvestcircle.updateMacosGoldens=true :app:shared:desktopTest --tests org.harvestcircle.ui.shell.HarvestCircleMacGoldenTest
        ;;
    format)
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" fmt --manifest-path "$CARGO_MANIFEST" --all -- --check
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" fmt --manifest-path "$XTASK_MANIFEST" --all -- --check
        exec "$GRADLE" --no-daemon :app:shared:ktlintCheck :app:desktop:ktlintCheck designFormatCheck
        ;;
    format-fix)
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" fmt --manifest-path "$CARGO_MANIFEST" --all
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" fmt --manifest-path "$XTASK_MANIFEST" --all
        exec "$GRADLE" --no-daemon :app:shared:ktlintFormat :app:desktop:ktlintFormat
        ;;
    lint)
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" clippy --manifest-path "$CARGO_MANIFEST" --workspace --all-targets --locked -- -D warnings
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" clippy --manifest-path "$XTASK_MANIFEST" --all-targets --locked -- -D warnings
        exec "$GRADLE" --no-daemon :app:shared:detektCommonMainSourceSet :app:shared:detektCommonTestSourceSet :app:desktop:detekt designLint
        ;;
    test)
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" test --manifest-path "$CARGO_MANIFEST" --workspace --locked
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" test --manifest-path "$XTASK_MANIFEST" --locked
        exec "$GRADLE" --no-daemon :app:shared:desktopTest :app:desktop:test designTest
        ;;
    check)
        exec "$GRADLE" --no-daemon :app:shared:check :app:desktop:check designCheck
        ;;
    build)
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" build --manifest-path "$CARGO_MANIFEST" --workspace --locked
        exec "$GRADLE" --no-daemon :app:desktop:build
        ;;
    bindings)
        exec "$GRADLE" --no-daemon :app:desktop:verifyUniFfiBindings :app:desktop:verifyReleaseNativeLibrary
        ;;
    dev-check)
        "$GRADLE" --no-daemon --configuration-cache --configuration-cache-problems=fail :app:desktop:hotRunArgfile
        exec "$GRADLE" --no-daemon :app:desktop:test \
            --tests org.harvestcircle.application.NativeGeneratedRecoveryTest \
            --tests org.harvestcircle.application.DefaultProfileStartupTest
        ;;
    hub-check)
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" test --manifest-path "$CARGO_MANIFEST" -p harvestcircle_domain --lib --locked hub::
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" test --manifest-path "$CARGO_MANIFEST" -p harvestcircle_storage --lib --locked hub::
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" test --manifest-path "$CARGO_MANIFEST" -p harvestcircle_runtime --lib --locked hub_
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" test --manifest-path "$CARGO_MANIFEST" -p harvestcircle_ffi --lib --locked hub_
        exec "$GRADLE" --no-daemon :app:shared:check :app:desktop:check :app:desktop:integrationTest designCheck
        ;;
    ui-check)
        exec "$GRADLE" --no-daemon :app:shared:check :app:desktop:check designCheck
        ;;
    dev)
        exec "$GRADLE" :app:desktop:hotRun
        ;;
    demo)
        case "${DEMO_RUN:-}" in
            ""|[!a-z]*|*[!a-z0-9-]*) fail 'DEMO_RUN must begin with a lowercase letter and contain only lowercase letters, digits and hyphens' ;;
        esac
        [ "${#DEMO_RUN}" -le 48 ] || fail 'DEMO_RUN must contain at most 48 characters'
        exec "$GRADLE" :app:desktop:run "--args=--demo $DEMO_RUN"
        ;;
    run)
        exec "$GRADLE" :app:desktop:run
        ;;
    audit)
        scripts/check-prerequisites.sh audit
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" audit --file core/Cargo.lock
        exec cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" deny --manifest-path "$CARGO_MANIFEST" check --config core/deny.toml advisories
        ;;
    licenses)
        scripts/check-prerequisites.sh licenses
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" deny --manifest-path "$CARGO_MANIFEST" check --config core/deny.toml licenses sources
        exec "$GRADLE" --no-daemon --no-parallel --no-configuration-cache :app:desktop:checkLicense :app:design_system:checkLicense :tools:design_catalog:checkLicense
        ;;
    package)
        exec "$GRADLE" --no-daemon :app:desktop:verifyHostPackage
        ;;
    host-package-check)
        exec "$GRADLE" --no-daemon :app:desktop:verifyHostPackage
        ;;
    source-check)
        exec "$GRADLE" --no-daemon :app:desktop:sourceReadiness
        ;;
    integration-check)
        exec "$GRADLE" --no-daemon :app:desktop:integrationTest :app:desktop:verifyTestBridgeIsolation
        ;;
    host-ui-lifecycle-check)
        exec "$GRADLE" --no-daemon :app:desktop:hostUiLifecycleTest
        ;;
    clean)
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" clean --manifest-path "$CARGO_MANIFEST"
        cargo "+$HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN" clean --manifest-path "$XTASK_MANIFEST"
        "$GRADLE" --no-daemon clean
        exec "$GRADLE" --no-daemon -p build-logic clean
        ;;
    linux-check)
        exec scripts/check-linux-x86_64.sh
        ;;
esac
