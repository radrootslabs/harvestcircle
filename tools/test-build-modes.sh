#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
make_command=$(command -v make)
fixture=$(mktemp -d "${TMPDIR:-/tmp}/harvestcircle-build-mode.XXXXXX")
cleanup() {
    find "$fixture" -depth -delete
}
trap cleanup EXIT HUP INT TERM

printf '%s\n' '#!/bin/sh' 'if [ "${1:-}" = extbuild ]; then printf "%s\n" "cargo-extbuild unavailable" >&2; else printf "%s\n" "cargo must not be invoked in standalone dry-run" >&2; fi' 'exit 93' > "$fixture/cargo"
chmod +x "$fixture/cargo"

standalone_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE=standalone -C "$repository_root" check)
if printf '%s\n' "$standalone_output" | grep -q 'cargo extbuild'; then
    printf '%s\n' 'standalone mode attempted to invoke extbuild' >&2
    exit 1
fi

for lane in source-check integration-check; do
    for mode in standalone governed; do
        lane_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$repository_root" "$lane")
        build_logic_count=$(printf '%s\n' "$lane_output" | grep -c -- '-p build-logic')
        if [ "$build_logic_count" -ne 1 ]; then
            printf '%s\n' "$lane in $mode mode must invoke build-logic verification exactly once" >&2
            exit 1
        fi
    done
done

for mode in standalone governed; do
    clean_output=$(PATH="$fixture:$PATH" "$make_command" --no-print-directory -n BUILD_MODE="$mode" -C "$repository_root" clean)
    root_clean_count=$(printf '%s\n' "$clean_output" | grep -Ec '(^| -- )\./gradlew --no-daemon clean$')
    included_clean_count=$(printf '%s\n' "$clean_output" | grep -Ec '(^| -- )\./gradlew --no-daemon -p build-logic clean$')
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

printf '%s\n' 'harvestcircle.build-mode-contract=pass'
