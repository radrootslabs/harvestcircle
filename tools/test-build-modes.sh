#!/bin/sh
set -eu

# All execution below occurs in disposable fixtures with controlled tools.
unset MAKEFLAGS MAKEOVERRIDES MFLAGS EXTBUILD EXTBUILD_MACHINE_CONFIG
repository_root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
make_command=$(command -v make)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/harvestcircle-build-mode.XXXXXX")
cleanup() { find "$fixture" -depth -delete; }
trap cleanup EXIT HUP INT TERM
dispatch_root="$(CDPATH='' cd -- "$fixture" && pwd -P)/checkout with spaces"
dispatch_bin="$fixture/native tools"
mkdir -p "$dispatch_root/tools" "$dispatch_root/gradle/wrapper" "$dispatch_bin" "$fixture/host home/.gradle"
cp "$repository_root/gradle/wrapper/gradle-wrapper.properties" "$dispatch_root/gradle/wrapper/gradle-wrapper.properties"
# Ordinary host Gradle caches are deliberately broken throughout these tests.
ln -s "$fixture/missing ambient cache" "$fixture/host home/.gradle/caches"
cp "$repository_root/Makefile" "$dispatch_root/Makefile"
cp "$repository_root/tools/check-prerequisites.sh" "$dispatch_root/tools/check-prerequisites.sh"
ln -s "$(command -v sed)" "$dispatch_bin/sed"
cat > "$dispatch_bin/git" <<'EOF'
#!/bin/sh
case "$1" in
    rev-parse) printf '%040d\n' 1 ;;
    status) [ "${HCR_TEST_DIRTY:-false}" = false ] || printf '%s\n' ' M source.rs' ;;
    show) printf '%s\n' 1 ;;
    *) exit 92 ;;
esac
EOF
chmod +x "$dispatch_bin/git"
cat > "$fixture/dispatch-tool" <<'EOF'
#!/bin/sh
tool=${0##*/}
if [ "$tool" = gradlew ]; then
    printf '%s\n' "${GRADLE_USER_HOME:-unset}" >> "$HCR_TEST_GRADLE_LOG"
fi
printf '%s\t%s\t%s\t%s\t%s\t%s\t%s' "$tool" "$PWD" "${HCR_TEST_ROUTED:-no}" "${HARVESTCIRCLE_BUILD_SOURCE_COMMIT:-}" "${HARVESTCIRCLE_BUILD_SOURCE_DIRTY:-}" "${HARVESTCIRCLE_BUILD_RADROOTS_REVISION:-}" "${SOURCE_DATE_EPOCH:-}" >> "$HCR_TEST_LOG"
printf '\t%s' "$@" >> "$HCR_TEST_LOG"
printf '\n' >> "$HCR_TEST_LOG"
if [ "$tool" = "${HCR_TEST_FAIL_TOOL:-}" ]; then
    case " $* " in
        *" ${HCR_TEST_FAIL_ARGUMENT:-} "*) exit 91 ;;
    esac
fi
case "$tool" in
    node|npm|pnpm|npx|corepack) exit 94 ;;
    java) printf 'openjdk version "%s.0.12"\n' "${HCR_TEST_JAVA_MAJOR:-21}" >&2 ;;
    rustup)
        if [ "${1:-}" = component ]; then
            printf '%s\n' clippy-test rustfmt-test
        else
            printf '%s\n' 'rustc 1.99.0'
        fi
        ;;
    uname) printf '%s\n' Linux ;;

    cargo)
        if [ "${1:-}" = +1.99.0 ]; then shift; fi
        if [ "${1:-}" = extbuild ]; then
            [ "${HCR_TEST_EXTBUILD:-present}" = present ] || { printf '%s\n' 'cargo-extbuild unavailable' >&2; exit 93; }
            shift
            if [ "${1:-}" = --machine-config ]; then shift 2; fi
            case "${1:-}" in
                doctor) exit 0 ;;
                run)
                    [ "${2:-}" = -- ] || exit 92
                    shift 2
                    export HCR_TEST_ROUTED=yes
                    export GRADLE_USER_HOME="$HCR_TEST_GOVERNED_HOME"
                    exec "$@"
                    ;;
                *) exit 92 ;;
            esac
        fi
        ;;
esac
# These are disposable build-output stand-ins, never primary build caches.
if [ "${HCR_TEST_CLEAN:-no}" = yes ]; then
    [ "$PWD" = "$HCR_TEST_CHECKOUT" ] || exit 92
    case "$tool:$*" in
        'cargo:clean --manifest-path core/Cargo.toml') rm -rf core/target ;;
        'cargo:clean --manifest-path tools/xtask/Cargo.toml') rm -rf tools/xtask/target ;;
        'gradlew:--no-daemon clean') rm -rf build app/shared/build app/desktop/build ;;
        'gradlew:--no-daemon -p build-logic clean') rm -rf build-logic/build ;;
    esac
fi
EOF
for tool in cargo java rustup cc uname pkg-config cargo-deny cargo-public-api cargo-audit node npm pnpm npx corepack; do
    cp "$fixture/dispatch-tool" "$dispatch_bin/$tool"
    chmod +x "$dispatch_bin/$tool"
done
cp "$fixture/dispatch-tool" "$dispatch_root/gradlew"
chmod +x "$dispatch_root/gradlew"
for tool in test-build-modes.sh verify-storage-api.sh; do
    cp "$fixture/dispatch-tool" "$dispatch_root/tools/$tool"
    chmod +x "$dispatch_root/tools/$tool"
done

run_dispatch() {
    dispatch_mode=$1
    dispatch_target=$2
    dispatch_log=$3
    : > "$dispatch_log"
    : > "$dispatch_log.gradle-home"
    # Start outside the checkout: Make must establish the recipe cwd with -C.
    (cd "$fixture"
        unset GRADLE_USER_HOME
        if [ "${HCR_TEST_GRADLE_HOME+x}" = x ]; then export GRADLE_USER_HOME="$HCR_TEST_GRADLE_HOME"; fi
        HOME="$fixture/host home" HCR_TEST_GOVERNED_HOME="$fixture/governed cache" HCR_TEST_GRADLE_LOG="$dispatch_log.gradle-home" PATH="$dispatch_bin" JAVA_HOME='' MAKEFLAGS='' MFLAGS='' HCR_TEST_LOG="$dispatch_log" HCR_TEST_ROUTED=no \
        "$make_command" --no-print-directory -C "$dispatch_root" BUILD_MODE="$dispatch_mode" GRADLE=./gradlew EXTBUILD_MACHINE_CONFIG="${HCR_TEST_PROFILE:-}" "$dispatch_target")
}


for mode in standalone governed; do
    default_output=$("$make_command" --no-print-directory -C "$dispatch_root" BUILD_MODE="$mode")
    help_output=$("$make_command" --no-print-directory -C "$dispatch_root" BUILD_MODE="$mode" help)
    [ "$default_output" = "$help_output" ] || exit 1
    for target in doctor dev run dev-check build check source-check integration-check package; do
        run_dispatch "$mode" "$target" "$fixture/dispatch.log" > "$fixture/output.log" 2>&1 || { cat "$fixture/output.log"; exit 1; }
        if grep -Eq '^(node|npm|pnpm|npx|corepack)[[:space:]]' "$fixture/dispatch.log"; then
            printf '%s\n' "$target invoked an unrelated tool" >&2; exit 1
        fi
        [ "$(grep -c '^java[[:space:]]' "$fixture/dispatch.log")" -eq 1 ]
        awk -F '\t' '$4 != "0000000000000000000000000000000000000001" || $5 != "false" || $7 != "1" { exit 1 }' "$fixture/dispatch.log" || { cat "$fixture/dispatch.log"; exit 1; }
        while IFS="$(printf '\t')" read -r tool cwd _details; do
            [ "$cwd" = "$dispatch_root" ] || { printf '%s\n' "$target ran outside its source root"; exit 1; }
        done < "$fixture/dispatch.log"
        expected_home="$fixture/governed cache"
        if [ "$mode" = standalone ]; then expected_home="$dispatch_root/.gradle/user-home"; fi
        while IFS= read -r actual_home; do [ "$actual_home" = "$expected_home" ] || exit 1; done < "$fixture/dispatch.log.gradle-home"
        if [ "$mode" = standalone ]; then
            if grep -q "$(printf '\t')extbuild$(printf '\t')" "$fixture/dispatch.log"; then
                printf '%s\n' 'Standalone dispatch invoked extbuild' >&2; exit 1
            fi
        else
            grep -q "$(printf '\t')extbuild$(printf '\t')doctor$" "$fixture/dispatch.log"
        fi
        for missing in java cargo rustup gradlew cc; do
            missing_path="$dispatch_bin/$missing"
            if [ "$missing" = gradlew ]; then missing_path="$dispatch_root/gradlew"; fi
            mv "$missing_path" "$fixture/missing-tool"
            if run_dispatch "$mode" "$target" "$fixture/missing.log" > "$fixture/missing-output.log" 2>&1; then
                printf '%s\n' "$target succeeded without $missing" >&2; exit 1
            fi
            mv "$fixture/missing-tool" "$missing_path"
            grep -Eq '(missing|not found|No such file|unavailable)' "$fixture/missing-output.log"
        done
        printf '%s\n' "harvestcircle.dispatch.$mode.$target=cwd,provenance,doctor,no-node,missing-tools:pass"
    done
    for lane in source-check integration-check development-check; do
        lane_output=$(PATH="$dispatch_bin" "$make_command" --no-print-directory -n -C "$dispatch_root" BUILD_MODE="$mode" "$lane")
        [ "$(printf '%s\n' "$lane_output" | grep -c -- '-p build-logic')" -eq 1 ]
    done
    development_output=$(PATH="$dispatch_bin" "$make_command" --no-print-directory -n -C "$dispatch_root" BUILD_MODE="$mode" development-check)
    for forbidden in 'cargo audit' dependencyCheckAnalyze verifyHostPackage releaseReadiness unsignedReleaseReadiness signingReadiness notarizationReadiness packageDmg packageDeb; do
        if printf '%s\n' "$development_output" | grep -q "$forbidden"; then
            printf '%s\n' "development-check selected deferred release work: $forbidden" >&2; exit 1
        fi
    done
    [ "$(printf '%s\n' "$development_output" | grep -c -- '--configuration-cache --configuration-cache-problems=fail :app:desktop:hotRunArgfile')" -eq 1 ]
done

# Explicit standalone homes are honored; unusable paths fail before Gradle.
HCR_TEST_GRADLE_HOME="$fixture/custom cache with spaces" run_dispatch standalone dev-check "$fixture/custom.log" > "$fixture/custom-output.log" 2>&1
while IFS= read -r actual_home; do [ "$actual_home" = "$fixture/custom cache with spaces" ] || exit 1; done < "$fixture/custom.log.gradle-home"
unset HCR_TEST_GRADLE_HOME
mkdir -p "$fixture/invalid home" "$fixture/nested home/caches/9.5.0"
ln -s "$fixture/missing cache target" "$fixture/invalid home/caches"
ln -s "$fixture/missing jvms target" "$fixture/nested home/caches/9.5.0/jvms"
printf '%s\n' blocker > "$fixture/not a directory"
for invalid_home in '' "$fixture/invalid home" "$fixture/nested home" "$fixture/not a directory"; do
    if HCR_TEST_GRADLE_HOME="$invalid_home" run_dispatch standalone dev "$fixture/invalid.log" > "$fixture/invalid-output.log" 2>&1; then
        printf '%s\n' 'Unusable explicit Gradle home was accepted' >&2; exit 1
    fi
    grep -Eq 'GRADLE_USER_HOME|Gradle cache' "$fixture/invalid-output.log"
    [ ! -s "$fixture/invalid.log.gradle-home" ]
done
unset HCR_TEST_GRADLE_HOME
[ -L "$fixture/host home/.gradle/caches" ] && [ ! -e "$fixture/host home/.gradle/caches" ]
printf '%s\n' 'harvestcircle.gradle-home=isolated-default,explicit-override,broken-ambient,invalid-paths,governed-routing:pass'

# A profile with spaces remains an explicit optional argument, never a local include.
HCR_TEST_PROFILE="$fixture/profile with spaces.toml" run_dispatch governed dev-check "$fixture/profile.log" > "$fixture/profile-output.log" 2>&1
HCR_TEST_DIRTY=true run_dispatch standalone dev-check "$fixture/dirty.log" > "$fixture/dirty-output.log" 2>&1
awk -F '\t' '$5 != "true" { exit 1 }' "$fixture/dirty.log"
unset HCR_TEST_PROFILE HCR_TEST_DIRTY

for target in unsigned-release-check signing-check notarization-check release-check linux-check; do
    if run_dispatch standalone "$target" "$fixture/release.log" > "$fixture/release-output.log" 2>&1; then exit 1; fi
    grep -q 'requires BUILD_MODE=governed' "$fixture/release-output.log"
    [ ! -s "$fixture/release.log" ]
done
if "$make_command" --no-print-directory -C "$dispatch_root" BUILD_MODE=unsupported help > "$fixture/unknown.log" 2>&1; then exit 1; fi
grep -q "Unknown BUILD_MODE 'unsupported'" "$fixture/unknown.log"
for retired in native-dev governed-development-check affected-check; do
    if "$make_command" --no-print-directory -C "$dispatch_root" "$retired" > "$fixture/retired.log" 2>&1; then exit 1; fi
done

# Wrong host selection and unavailable extbuild fail before a workload.
if HCR_TEST_JAVA_MAJOR=17 run_dispatch standalone dev "$fixture/java.log" > "$fixture/java-output.log" 2>&1; then exit 1; fi
grep -q 'select JDK 21' "$fixture/java-output.log"
if HCR_TEST_FAIL_TOOL=rustup HCR_TEST_FAIL_ARGUMENT=1.99.0 run_dispatch standalone dev "$fixture/rustup.log" > "$fixture/rustup-output.log" 2>&1; then exit 1; fi
grep -q 'install Rust 1.99.0' "$fixture/rustup-output.log"
if HCR_TEST_EXTBUILD=missing run_dispatch governed dev "$fixture/extbuild.log" > "$fixture/extbuild-output.log" 2>&1; then exit 1; fi
grep -q 'cargo-extbuild unavailable' "$fixture/extbuild-output.log"
unset HCR_TEST_JAVA_MAJOR HCR_TEST_FAIL_TOOL HCR_TEST_FAIL_ARGUMENT
HCR_TEST_EXTBUILD=missing run_dispatch standalone dev "$fixture/portable.log" > "$fixture/portable-output.log" 2>&1
unset HCR_TEST_EXTBUILD

# Scoped verification tools fail before their associated workloads.
for pair in 'licenses cargo-deny' 'api-check cargo-public-api' 'audit cargo-audit'; do
    scoped_target=${pair%% *}
    scoped_tool=${pair#* }
    mv "$dispatch_bin/$scoped_tool" "$fixture/missing-tool"
    if run_dispatch standalone "$scoped_target" "$fixture/scoped.log" > "$fixture/scoped-output.log" 2>&1; then exit 1; fi
    mv "$fixture/missing-tool" "$dispatch_bin/$scoped_tool"
    grep -q "$scoped_tool is missing" "$fixture/scoped-output.log"
done

# Fail each selected command rather than silently accepting a partial graph.
for mode in standalone governed; do
    for target in dev check build clean; do
        if HCR_TEST_FAIL_TOOL=gradlew HCR_TEST_FAIL_ARGUMENT=--version \
            run_dispatch "$mode" "$target" "$fixture/native-failure.log" > "$fixture/native-failure-output.log" 2>&1; then
            printf '%s\n' "$target ignored native doctor failure" >&2; exit 1
        fi
        grep -q 'Error 91' "$fixture/native-failure-output.log"
        [ "$(grep -c '^gradlew[[:space:]]' "$fixture/native-failure.log")" -eq 1 ]
        failure_tool=cargo
        failure_argument=$target
        case "$target" in
            dev) failure_tool=gradlew; failure_argument=:app:desktop:hotRun ;;
            check) failure_argument=fmt ;;
        esac
        if HCR_TEST_FAIL_TOOL="$failure_tool" HCR_TEST_FAIL_ARGUMENT="$failure_argument" \
            run_dispatch "$mode" "$target" "$fixture/native-failure.log" > "$fixture/native-failure-output.log" 2>&1; then
            printf '%s\n' "$target ignored failed $failure_argument" >&2; exit 1
        fi
        grep -q 'Error 91' "$fixture/native-failure-output.log"
        tail -n 1 "$fixture/native-failure.log" | grep -q "$(printf '\t')$failure_argument\($(printf '\t')\|\$\)"
    done
    printf '%s\n' "harvestcircle.dispatch-failure.$mode=native-doctor,native-command,stop-on-failure:pass"
done

# Shell function environment assignments can persist in POSIX shells.
unset HCR_TEST_FAIL_TOOL HCR_TEST_FAIL_ARGUMENT

# Clean executes only in the owned fixture. Preserve
# dirty unrelated source, sibling source and desktop user-state sentinels.
ln -s "$(command -v rm)" "$dispatch_bin/rm"
for mode in standalone governed; do
    for path in core/target tools/xtask/target build app/shared/build app/desktop/build build-logic/build; do
        mkdir -p "$dispatch_root/$path"
        printf '%s\n' disposable > "$dispatch_root/$path/output"
    done
    mkdir -p "$dispatch_root/unrelated-source"
    printf '%s\n' 'preserve:unrelated-source' > "$dispatch_root/unrelated-source/sentinel"
    for path in 'sibling checkout' 'desktop user state'; do
        mkdir -p "$fixture/$path"
        printf '%s\n' "preserve:$path" > "$fixture/$path/sentinel"
    done
    HCR_TEST_CLEAN=yes HCR_TEST_CHECKOUT="$dispatch_root" \
        run_dispatch "$mode" clean "$fixture/clean-dispatch.log" > "$fixture/clean-output.log" 2>&1 || { cat "$fixture/clean-output.log"; exit 1; }
    clean_count=$(awk -F '\t' '($1 == "cargo" && $9 == "clean") || ($1 == "gradlew" && ($9 == "clean" || $11 == "clean")) { n++ } END { print n+0 }' "$fixture/clean-dispatch.log")
    [ "$clean_count" -eq 4 ]
    for path in core/target tools/xtask/target build app/shared/build app/desktop/build build-logic/build; do
        [ ! -e "$dispatch_root/$path" ] || exit 1
    done
    [ "$(cat "$dispatch_root/unrelated-source/sentinel")" = 'preserve:unrelated-source' ] || exit 1
    for path in 'sibling checkout' 'desktop user state'; do
        [ "$(cat "$fixture/$path/sentinel")" = "preserve:$path" ] || exit 1
    done
    printf '%s\n' "harvestcircle.clean.$mode=owned-output-removal,sentinel-preservation:pass"
done

printf '%s\n' 'harvestcircle.build-mode-contract=pass'
