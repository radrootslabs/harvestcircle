#!/bin/sh
set -eu

# Command-line host routing must not leak into portable command fixtures.
unset MAKEFLAGS MAKEOVERRIDES MFLAGS EXTBUILD EXTBUILD_MACHINE_CONFIG

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
make_command=$(command -v make)
gradle_command=${GRADLE:-./gradlew}
fixture=$(mktemp -d "${TMPDIR:-/tmp}/harvestcircle-build-mode.XXXXXX")
cleanup() {
    find "$fixture" -depth -delete
}
trap cleanup EXIT HUP INT TERM
# Test root Makefile defaults in a disposable checkout.
makefile_fixture="$fixture/portable source"
mkdir -p "$makefile_fixture"
cp "$repository_root/Makefile" "$makefile_fixture/Makefile"
printf '%s\n' '#!/bin/sh' 'exit 0' > "$makefile_fixture/gradlew"
chmod +x "$makefile_fixture/gradlew"

printf '%s\n' '#!/bin/sh' 'if [ "${1:-}" = +1.99.0 ]; then shift; fi' 'if [ "${1:-}" = --version ]; then exit 0; fi' 'if [ "${1:-}" = extbuild ]; then printf "%s\n" "cargo-extbuild unavailable" >&2; else printf "%s\n" "cargo must not be invoked in standalone dry-run" >&2; fi' 'exit 93' > "$fixture/cargo"
chmod +x "$fixture/cargo"

standalone_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone -C "$makefile_fixture" check)
if printf '%s\n' "$standalone_output" | grep -q 'cargo extbuild'; then
    printf '%s\n' 'standalone mode attempted to invoke extbuild' >&2
    exit 1
fi

profile="$fixture/profile with spaces.toml"
profile_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=governed EXTBUILD_MACHINE_CONFIG="$profile" -C "$makefile_fixture" doctor)
printf '%s\n' "$profile_output" | grep -Fx "cargo extbuild --machine-config \"$profile\" doctor" >/dev/null
printf '%s\n' "$profile_output" | grep -Fx "cargo extbuild --machine-config \"$profile\" run -- java -version" >/dev/null
standalone_profile_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone EXTBUILD_MACHINE_CONFIG="$profile" -C "$makefile_fixture" doctor)
if printf '%s\n' "$standalone_profile_output" | grep -q extbuild; then
    printf '%s\n' 'standalone mode probed the optional machine profile' >&2
    exit 1
fi

for lane in source-check integration-check development-check; do
    for mode in standalone governed; do
        lane_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$makefile_fixture" "$lane")
        build_logic_count=$(printf '%s\n' "$lane_output" | grep -c -- '-p build-logic')
        if [ "$build_logic_count" -ne 1 ]; then
            printf '%s\n' "$lane in $mode mode must invoke build-logic verification exactly once" >&2
            exit 1
        fi
    done
done

development_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone -C "$makefile_fixture" development-check)
for forbidden in \
    'cargo audit' \
    'dependencyCheckAnalyze' \
    'verifyHostPackage' \
    'releaseReadiness' \
    'unsignedReleaseReadiness' \
    'signingReadiness' \
    'notarizationReadiness' \
    'packageDmg' \
    'packageDeb'
do
    if printf '%s\n' "$development_output" | grep -q "$forbidden"; then
        printf '%s\n' "development verification activated deferred integration: $forbidden" >&2
        exit 1
    fi
done

for mode in standalone governed; do
    source_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$makefile_fixture" source-check)

    dev_check_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$makefile_fixture" dev-check)
    dev_check_count=$(printf '%s\n' "$dev_check_output" | grep -c -- '--configuration-cache --configuration-cache-problems=fail :app:desktop:hotRunArgfile')
    if [ "$dev_check_count" -ne 1 ]; then
        printf '%s\n' "development readiness in $mode mode must verify the finite hot-reload argfile exactly once" >&2
        exit 1
    fi

    source_dev_check_count=$(printf '%s\n' "$source_output" | grep -c -- '--configuration-cache --configuration-cache-problems=fail :app:desktop:hotRunArgfile')
    if [ "$source_dev_check_count" -ne 1 ]; then
        printf '%s\n' "ordinary source-check in $mode mode must include development readiness exactly once" >&2
        exit 1
    fi
done

dev_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone -C "$makefile_fixture" dev)
if ! printf '%s\n' "$dev_output" | grep -q ':app:desktop:hotRun'; then
    printf '%s\n' 'development command must invoke Compose hot reload' >&2
    exit 1
fi
if printf '%s\n' "$dev_output" | grep -q -- '--no-configuration-cache'; then
    printf '%s\n' 'development command disabled the qualified configuration cache' >&2
    exit 1
fi

for mode in standalone governed; do
    clean_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$makefile_fixture" clean)
    build_runner_prefix=
    if [ "$mode" = governed ]; then
        build_runner_prefix='cargo extbuild run -- '
    fi
    root_clean_count=$(printf '%s\n' "$clean_output" | grep -Fxc -- "${build_runner_prefix}${gradle_command} --no-daemon clean" || true)
    included_clean_count=$(printf '%s\n' "$clean_output" | grep -Fxc -- "${build_runner_prefix}${gradle_command} --no-daemon -p build-logic clean" || true)
    if [ "$root_clean_count" -ne 1 ] || [ "$included_clean_count" -ne 1 ]; then
        printf '%s\n' "clean in $mode mode must clean the root and included builds exactly once" >&2
        exit 1
    fi
done

if "$make_command" --no-print-directory -C "$makefile_fixture" BUILD_MODE=unsupported help > "$fixture/unknown.log" 2>&1; then
    printf '%s\n' 'unknown build mode was accepted' >&2
    exit 1
fi
grep -q "Unknown BUILD_MODE 'unsupported'" "$fixture/unknown.log"

if PATH="$fixture:$PATH" "$make_command" --no-print-directory -C "$makefile_fixture" governed-doctor > "$fixture/governed.log" 2>&1; then
    printf '%s\n' 'governed mode succeeded without extbuild' >&2
    exit 1
fi
grep -q 'cargo-extbuild unavailable' "$fixture/governed.log"

if PATH="$fixture:$PATH" "$make_command" --no-print-directory -C "$makefile_fixture" BUILD_MODE=standalone _release-check > "$fixture/release.log" 2>&1; then
    printf '%s\n' 'release execution accepted standalone mode' >&2
    exit 1
fi
grep -q 'release-check requires governed mode' "$fixture/release.log"

if PATH="$fixture:$PATH" "$make_command" --no-print-directory -C "$makefile_fixture" BUILD_MODE=standalone _unsigned-release-check > "$fixture/unsigned-release.log" 2>&1; then
    printf '%s\n' 'unsigned release execution accepted standalone mode' >&2
    exit 1
fi
grep -q 'unsigned-release-check requires governed mode' "$fixture/unsigned-release.log"

unsigned_release_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=governed -C "$makefile_fixture" _unsigned-release-check)
if ! printf '%s\n' "$unsigned_release_output" | grep -q ':app:desktop:unsignedReleaseReadiness'; then
    printf '%s\n' 'unsigned release command did not select the unsigned readiness gate' >&2
    exit 1
fi
for forbidden in 'cargo audit' 'advisories' ':app:desktop:dependencyCheckAnalyze' ':app:desktop:releaseReadiness' ':app:desktop:signingReadiness' ':app:desktop:notarizationReadiness'; do
    if printf '%s\n' "$unsigned_release_output" | grep -q "$forbidden"; then
        printf '%s\n' "unsigned release command activated out-of-scope authority: $forbidden" >&2
        exit 1
    fi
done

standalone_package_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone -C "$makefile_fixture" package-check)
standalone_unsigned_gate_count=$(printf '%s\n' "$standalone_package_output" | grep -Fxc -- "$gradle_command --no-daemon --no-parallel --no-configuration-cache :app:desktop:unsignedReleaseReadiness" || true)
if [ "$standalone_unsigned_gate_count" -ne 1 ] || printf '%s\n' "$standalone_package_output" | grep -q 'cargo extbuild'; then
    printf '%s\n' 'standalone package-check must invoke the unsigned gate once without probing extbuild' >&2
    exit 1
fi

governed_package_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone -C "$makefile_fixture" governed-package-check)
governed_unsigned_gate_count=$(printf '%s\n' "$governed_package_output" | grep -Fxc -- "cargo extbuild run -- $gradle_command --no-daemon --no-parallel --no-configuration-cache :app:desktop:unsignedReleaseReadiness" || true)
if [ "$governed_unsigned_gate_count" -ne 1 ]; then
    printf '%s\n' 'governed-package-check must invoke the extbuild-routed unsigned gate exactly once' >&2
    exit 1
fi

# Run the real Make graph with only controlled native tools. The disposable
# dispatcher fixture deliberately has spaces, no ambient tool PATH, and finite
# stand-ins for hotRun, package checks, and the mode-check script itself.
dispatch_root="$(CDPATH= cd -- "$fixture" && pwd -P)/checkout with spaces"
dispatch_bin="$fixture/native tools"
mkdir -p "$dispatch_root/tools" "$dispatch_bin"
cp "$repository_root/Makefile" "$dispatch_root/Makefile"
ln -s "$(command -v sed)" "$dispatch_bin/sed"
cat > "$dispatch_bin/git" <<'EOF'
#!/bin/sh
case "$1" in
    rev-parse) printf '%040d\n' 1 ;;
    status) : ;;
    show) printf '%s\n' 1 ;;
    *) exit 92 ;;
esac
EOF
chmod +x "$dispatch_bin/git"
cat > "$fixture/dispatch-tool" <<'EOF'
#!/bin/sh
tool=${0##*/}
printf '%s\t%s\t%s\t%s\t%s\t%s\t%s' "$tool" "$PWD" "${HCR_TEST_ROUTED:-no}" "${HARVESTCIRCLE_BUILD_SOURCE_COMMIT:-}" "${HARVESTCIRCLE_BUILD_SOURCE_DIRTY:-}" "${HARVESTCIRCLE_BUILD_RADROOTS_REVISION:-}" "${SOURCE_DATE_EPOCH:-}" >> "$HCR_TEST_LOG"
printf '\t%s' "$@" >> "$HCR_TEST_LOG"
printf '\n' >> "$HCR_TEST_LOG"
if [ "$tool" = "${HCR_TEST_FAIL_TOOL:-}" ]; then
    case " $* " in
        *" ${HCR_TEST_FAIL_ARGUMENT:-} "*) exit 91 ;;
    esac
fi
case "$tool" in
    node|corepack)
        [ "${HCR_TEST_RUNTIME:-native}" = web ] || exit 94
        ;;
    java|gradlew)
        [ "${HCR_TEST_RUNTIME:-native}" = native ] || exit 94
        ;;
    npm|pnpm|npx) exit 94 ;;
    cargo)
        if [ "${1:-}" = +1.99.0 ]; then shift; fi
        if [ "${1:-}" = extbuild ]; then
            case "${2:-}" in
                doctor) exit 0 ;;
                run)
                    [ "${3:-}" = -- ] || exit 92
                    shift 3
                    export HCR_TEST_ROUTED=yes
                    exec "$@"
                    ;;
                *) exit 92 ;;
            esac
        fi
        [ "${HCR_TEST_RUNTIME:-native}" = native ] || exit 94
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
for tool in cargo java node npm pnpm npx corepack; do
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
    # Start outside the checkout: Make must establish the recipe cwd with -C.
    (cd "$fixture" && PATH="$dispatch_bin" MAKEFLAGS= MFLAGS= HCR_TEST_LOG="$dispatch_log" HCR_TEST_ROUTED=no \
        "$make_command" --no-print-directory -C "$dispatch_root" BUILD_MODE="$dispatch_mode" GRADLE=./gradlew "$dispatch_target")
}

for mode in standalone governed; do
    default_output=$("$make_command" --no-print-directory -C "$dispatch_root" BUILD_MODE="$mode")
    help_output=$("$make_command" --no-print-directory -C "$dispatch_root" BUILD_MODE="$mode" help)
    if [ "$default_output" != "$help_output" ]; then
        printf '%s\n' "default goal changed from help in $mode mode" >&2
        exit 1
    fi
    for target in doctor dev check build package-check; do
        alias="native-$target"
        original_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$makefile_fixture" "$target")
        alias_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$makefile_fixture" "$alias")
        if [ "$original_output" != "$alias_output" ]; then
            printf '%s\n' "$alias changed or duplicated the $target command graph in $mode mode" >&2
            exit 1
        fi
        run_dispatch "$mode" "$target" "$fixture/original-dispatch.log" > "$fixture/original-output.log" 2>&1
        run_dispatch "$mode" "$alias" "$fixture/alias-dispatch.log" > "$fixture/alias-output.log" 2>&1
        if ! cmp -s "$fixture/original-dispatch.log" "$fixture/alias-dispatch.log" || \
            ! cmp -s "$fixture/original-output.log" "$fixture/alias-output.log"; then
            diff -u "$fixture/original-dispatch.log" "$fixture/alias-dispatch.log" || true
            diff -u "$fixture/original-output.log" "$fixture/alias-output.log" || true
            printf '%s\n' "$alias changed controlled execution of $target in $mode mode" >&2
            exit 1
        fi
        if [ ! -s "$fixture/alias-dispatch.log" ] || \
            grep -Eq '^(node|npm|pnpm|npx|corepack)[[:space:]]' "$fixture/alias-dispatch.log"; then
            printf '%s\n' "$alias did not execute native tools exclusively in $mode mode" >&2
            exit 1
        fi
        if [ "$target" != doctor ]; then
            awk -F '\t' '
                $4 != "0000000000000000000000000000000000000001" || $5 != "false" || $7 != "1" {
                    print "native command omitted actual checkout provenance"; exit 1
                }
            ' "$fixture/alias-dispatch.log"
        fi
        if [ "$(grep -c '^java[[:space:]]' "$fixture/alias-dispatch.log")" -ne 1 ]; then
            printf '%s\n' "$alias must execute its doctor once in $mode mode" >&2
            exit 1
        fi
        while IFS="$(printf '\t')" read -r tool cwd details; do
            if [ "$cwd" != "$dispatch_root" ]; then
                printf '%s\n' "$alias invoked $tool outside the checkout in $mode mode" >&2
                exit 1
            fi
        done < "$fixture/alias-dispatch.log"
        for missing in java cargo gradlew; do
            missing_path="$dispatch_bin/$missing"
            if [ "$missing" = gradlew ]; then missing_path="$dispatch_root/gradlew"; fi
            mv "$missing_path" "$fixture/missing-tool"
            if run_dispatch "$mode" "$alias" "$fixture/missing-dispatch.log" > "$fixture/missing-output.log" 2>&1; then
                printf '%s\n' "$alias succeeded without $missing in $mode mode" >&2
                exit 1
            fi
            mv "$fixture/missing-tool" "$missing_path"
            grep -Eq '(not found|No such file)' "$fixture/missing-output.log"
        done
        if [ "$mode" = governed ]; then
            mv "$dispatch_bin/cargo" "$fixture/available-cargo"
            cp "$fixture/cargo" "$dispatch_bin/cargo"
            if run_dispatch "$mode" "$alias" "$fixture/missing-dispatch.log" > "$fixture/missing-output.log" 2>&1; then
                printf '%s\n' "$alias succeeded without extbuild" >&2
                exit 1
            fi
            grep -q 'cargo-extbuild unavailable' "$fixture/missing-output.log"
            mv "$fixture/available-cargo" "$dispatch_bin/cargo"
        fi
        printf '%s\n' "harvestcircle.native-alias.$mode.$alias=dry-run,execution,cwd,no-node,missing-tools:pass"
    done
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
    for path in unrelated-source; do
        mkdir -p "$dispatch_root/$path"
        printf '%s\n' "preserve:$path" > "$dispatch_root/$path/sentinel"
    done
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
    for path in unrelated-source; do
        [ "$(cat "$dispatch_root/$path/sentinel")" = "preserve:$path" ] || exit 1
    done
    for path in 'sibling checkout' 'desktop user state'; do
        [ "$(cat "$fixture/$path/sentinel")" = "preserve:$path" ] || exit 1
    done
    printf '%s\n' "harvestcircle.clean.$mode=owned-output-removal,sentinel-preservation:pass"
done

printf '%s\n' 'harvestcircle.native-alias-contract=pass'
printf '%s\n' 'harvestcircle.build-mode-contract=pass'
