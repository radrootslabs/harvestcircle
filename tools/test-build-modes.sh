#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
make_command=$(command -v make)
gradle_command=${GRADLE:-./gradlew}
fixture=$(mktemp -d "${TMPDIR:-/tmp}/harvestcircle-build-mode.XXXXXX")
cleanup() {
    find "$fixture" -depth -delete
}
trap cleanup EXIT HUP INT TERM

printf '%s\n' '#!/bin/sh' 'if [ "${1:-}" = +1.97.1 ]; then shift; fi' 'if [ "${1:-}" = extbuild ]; then printf "%s\n" "cargo-extbuild unavailable" >&2; else printf "%s\n" "cargo must not be invoked in standalone dry-run" >&2; fi' 'exit 93' > "$fixture/cargo"
chmod +x "$fixture/cargo"

standalone_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone -C "$repository_root" check)
if printf '%s\n' "$standalone_output" | grep -q 'cargo extbuild'; then
    printf '%s\n' 'standalone mode attempted to invoke extbuild' >&2
    exit 1
fi

for lane in source-check integration-check development-check; do
    for mode in standalone governed; do
        lane_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$repository_root" "$lane")
        build_logic_count=$(printf '%s\n' "$lane_output" | grep -c -- '-p build-logic')
        if [ "$build_logic_count" -ne 1 ]; then
            printf '%s\n' "$lane in $mode mode must invoke build-logic verification exactly once" >&2
            exit 1
        fi
    done
done

development_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone -C "$repository_root" development-check)
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
    stability_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$repository_root" build-logic-stability-check)
    stability_count=$(printf '%s\n' "$stability_output" | grep -c 'test-build-logic-stability.sh')
    if [ "$stability_count" -ne 1 ]; then
        printf '%s\n' "build-logic stability in $mode mode must invoke its qualification tool exactly once" >&2
        exit 1
    fi

    source_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$repository_root" source-check)
    if printf '%s\n' "$source_output" | grep -q 'test-build-logic-stability.sh'; then
        printf '%s\n' "ordinary source-check in $mode mode invoked the nondefault stability lane" >&2
        exit 1
    fi

    dev_check_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$repository_root" dev-check)
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

dev_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone -C "$repository_root" dev)
if ! printf '%s\n' "$dev_output" | grep -q ':app:desktop:hotRun'; then
    printf '%s\n' 'development command must invoke Compose hot reload' >&2
    exit 1
fi
if printf '%s\n' "$dev_output" | grep -q -- '--no-configuration-cache'; then
    printf '%s\n' 'development command disabled the qualified configuration cache' >&2
    exit 1
fi

for mode in standalone governed; do
    clean_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$repository_root" clean)
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

if "$make_command" --no-print-directory -C "$repository_root" BUILD_MODE=unsupported help > "$fixture/unknown.log" 2>&1; then
    printf '%s\n' 'unknown build mode was accepted' >&2
    exit 1
fi
grep -q "Unknown BUILD_MODE 'unsupported'" "$fixture/unknown.log"

if PATH="$fixture:$PATH" "$make_command" --no-print-directory -C "$repository_root" governed-doctor > "$fixture/governed.log" 2>&1; then
    printf '%s\n' 'governed mode succeeded without extbuild' >&2
    exit 1
fi
grep -q 'cargo-extbuild unavailable' "$fixture/governed.log"

if "$make_command" --no-print-directory -C "$repository_root" BUILD_MODE=standalone _release-check > "$fixture/release.log" 2>&1; then
    printf '%s\n' 'release execution accepted standalone mode' >&2
    exit 1
fi
grep -q 'release-check requires governed mode' "$fixture/release.log"

if "$make_command" --no-print-directory -C "$repository_root" BUILD_MODE=standalone _unsigned-release-check > "$fixture/unsigned-release.log" 2>&1; then
    printf '%s\n' 'unsigned release execution accepted standalone mode' >&2
    exit 1
fi
grep -q 'unsigned-release-check requires governed mode' "$fixture/unsigned-release.log"

unsigned_release_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=governed -C "$repository_root" _unsigned-release-check)
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

standalone_package_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone -C "$repository_root" package-check)
standalone_unsigned_gate_count=$(printf '%s\n' "$standalone_package_output" | grep -Fxc -- "$gradle_command --no-daemon --no-parallel --no-configuration-cache :app:desktop:unsignedReleaseReadiness" || true)
if [ "$standalone_unsigned_gate_count" -ne 1 ] || printf '%s\n' "$standalone_package_output" | grep -q 'cargo extbuild'; then
    printf '%s\n' 'standalone package-check must invoke the unsigned gate once without probing extbuild' >&2
    exit 1
fi

governed_package_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone -C "$repository_root" governed-package-check)
governed_unsigned_gate_count=$(printf '%s\n' "$governed_package_output" | grep -Fxc -- "cargo extbuild run -- $gradle_command --no-daemon --no-parallel --no-configuration-cache :app:desktop:unsignedReleaseReadiness" || true)
if [ "$governed_unsigned_gate_count" -ne 1 ]; then
    printf '%s\n' 'governed-package-check must invoke the extbuild-routed unsigned gate exactly once' >&2
    exit 1
fi

printf '%s\n' 'harvestcircle.build-mode-contract=pass'
