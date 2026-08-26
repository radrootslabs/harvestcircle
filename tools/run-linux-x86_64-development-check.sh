#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

fail() {
    printf '%s\n' "linux-x86_64 development check: $1" >&2
    exit 1
}

[ -n "${EXT_BUILD_RUN_ACTIVE:-}" ] || fail "must run through cargo extbuild"
[ -n "${EXT_BUILD_PROJECT_DIR:-}" ] || fail "EXT_BUILD_PROJECT_DIR is unavailable"
[ -n "${EXT_BUILD_TMPDIR:-}" ] || fail "EXT_BUILD_TMPDIR is unavailable"
command -v docker >/dev/null 2>&1 || fail "docker is unavailable"
docker info >/dev/null 2>&1 || fail "docker daemon is unavailable"

source_commit=$(git -C "$repository_root" rev-parse HEAD)
[ -n "$source_commit" ] || fail "source commit is unavailable"
[ -z "$(git -C "$repository_root" status --porcelain --untracked-files=all)" ] ||
    fail "source worktree must be clean"

source_epoch=$(git -C "$repository_root" show -s --format=%ct "$source_commit")
case "$source_epoch" in
    ''|*[!0-9]*) fail "source epoch is invalid" ;;
esac

source_lock="$repository_root/radroots.lib.source-lock.v1.toml"
[ -f "$source_lock" ] || fail "source lock is missing"
radroots_revision=$(sed -n 's/^revision = "\([0-9a-f]\{40\}\)"$/\1/p' "$source_lock")
[ "$(printf '%s\n' "$radroots_revision" | wc -l | tr -d ' ')" -eq 1 ] ||
    fail "source lock revision is not unique"
[ "${#radroots_revision}" -eq 40 ] || fail "source lock revision is invalid"

runner_root="$EXT_BUILD_PROJECT_DIR/linux-x86_64-development"
cargo_home="$runner_root/cargo-home"
cargo_target="$runner_root/cargo-target"
cargo_tool_target="$runner_root/cargo-tool-target"
cargo_tools="$runner_root/cargo-tools"
gradle_home="$runner_root/gradle-home"
gradle_build="$runner_root/gradle-build"
gradle_project_cache="$runner_root/gradle-project-cache"
container_home="$runner_root/home"
container_tmp="$runner_root/tmp"
container_jvm="$runner_root/jvm"

mkdir -p \
    "$cargo_home" \
    "$cargo_target" \
    "$cargo_tool_target" \
    "$cargo_tools" \
    "$gradle_home" \
    "$gradle_build" \
    "$gradle_project_cache" \
    "$container_home" \
    "$container_tmp" \
    "$container_jvm"
chmod 0700 "$container_tmp"
chmod 0755 "$container_jvm"

workspace=$(mktemp -d "$EXT_BUILD_TMPDIR/harvestcircle-linux-x86_64.XXXXXX")
cleanup() {
    find "$workspace" -depth -delete
}
trap cleanup EXIT HUP INT TERM

git clone --no-local --no-hardlinks "$repository_root" "$workspace/source"
git -C "$workspace/source" checkout --detach "$source_commit"
[ -z "$(git -C "$workspace/source" status --porcelain --untracked-files=all)" ] ||
    fail "isolated source clone is not clean"

runner_image="docker.io/library/rust:1.97.1-slim-trixie@sha256:fc0648ac2962539be80bd424729a20fd80f7b64bfba7e90bbd642aed6c697c5a"

docker run \
    --rm \
    --init \
    --platform linux/amd64 \
    --workdir /workspace/source \
    --env HOME=/workspace/home \
    --env CARGO_HOME=/workspace/cargo-home \
    --env CARGO_TARGET_DIR=/workspace/cargo-target \
    --env CARGO_BUILD_JOBS=2 \
    --env GRADLE_USER_HOME=/workspace/gradle-home \
    --env EXT_BUILD_GRADLE_BUILD_DIR=/workspace/gradle-build \
    --env HARVESTCIRCLE_BUILD_MODE=governed \
    --env HARVESTCIRCLE_BUILD_SOURCE_COMMIT="$source_commit" \
    --env HARVESTCIRCLE_BUILD_SOURCE_DIRTY=false \
    --env HARVESTCIRCLE_BUILD_RADROOTS_REVISION="$radroots_revision" \
    --env SOURCE_DATE_EPOCH="$source_epoch" \
    --mount "type=bind,src=$workspace/source,dst=/workspace/source" \
    --mount "type=bind,src=$container_home,dst=/workspace/home" \
    --mount "type=bind,src=$cargo_home,dst=/workspace/cargo-home" \
    --mount "type=bind,src=$cargo_target,dst=/workspace/cargo-target" \
    --mount "type=bind,src=$cargo_tool_target,dst=/workspace/cargo-tool-target" \
    --mount "type=bind,src=$cargo_tools,dst=/workspace/cargo-tools" \
    --mount "type=bind,src=$gradle_home,dst=/workspace/gradle-home" \
    --mount "type=bind,src=$gradle_build,dst=/workspace/gradle-build" \
    --mount "type=bind,src=$gradle_project_cache,dst=/workspace/gradle-project-cache" \
    --mount "type=bind,src=$container_tmp,dst=/tmp" \
    --mount "type=bind,src=$container_jvm,dst=/usr/lib/jvm" \
    "$runner_image" \
    sh -ceu '
        export DEBIAN_FRONTEND=noninteractive
        mkdir -p /tmp/apt-archives/partial
        chown _apt:root /tmp/apt-archives/partial
        chmod 0700 /tmp/apt-archives/partial
        apt-get update
        apt-get -o Dir::Cache::archives=/tmp/apt-archives install --yes --no-install-recommends \
            binutils ca-certificates file g++ git gosu libasound2-dev libfreetype-dev \
            libssl-dev libx11-dev libxext-dev libxi-dev libxrender-dev libxtst-dev \
            make openjdk-21-jdk pkg-config xz-utils zlib1g-dev
        rm -rf /var/lib/apt/lists/*
        chown -R '"$(id -u):$(id -g)"' /tmp/apt-archives
        chown -R '"$(id -u):$(id -g)"' /usr/lib/jvm
        exec gosu '"$(id -u):$(id -g)"' sh -ceu '\''
        export PATH=/workspace/cargo-tools/bin:/usr/local/cargo/bin:$PATH
        [ "$(uname -s)" = Linux ]
        [ "$(uname -m)" = x86_64 ]
        [ "$(git rev-parse HEAD)" = "$HARVESTCIRCLE_BUILD_SOURCE_COMMIT" ]
        [ -z "$(git status --porcelain --untracked-files=all)" ]
        rustc --version | grep -Eq "^rustc 1\\.97\\.1 "
        java -version 2>&1 | grep -Eq "version \"21\\."
        rustup component add clippy rustfmt
        if ! cargo deny --version 2>/dev/null | grep -Eq "0\\.19\\.8$"; then
            CARGO_TARGET_DIR=/workspace/cargo-tool-target \
                cargo install --root /workspace/cargo-tools cargo-deny --version 0.19.8 --locked
        fi
        cargo fmt --manifest-path core/Cargo.toml --all -- --check
        cargo clippy --manifest-path core/Cargo.toml --workspace --all-targets --locked -- -D warnings
        cargo test --manifest-path core/Cargo.toml --workspace --locked
        cargo deny --manifest-path core/Cargo.toml check --config core/deny.toml licenses sources
        cargo fmt --manifest-path tools/xtask/Cargo.toml --all -- --check
        cargo clippy --manifest-path tools/xtask/Cargo.toml --all-targets --locked -- -D warnings
        cargo test --manifest-path tools/xtask/Cargo.toml --locked
        cargo run --manifest-path tools/xtask/Cargo.toml --locked -- qualification-report
        tools/test-build-modes.sh
        ./gradlew --no-daemon --no-parallel --no-configuration-cache \
            --project-cache-dir /workspace/gradle-project-cache -p build-logic \
            :contracts:check :plugins:check :plugins:functionalTest
        ./gradlew --no-daemon --no-parallel --no-configuration-cache \
            --project-cache-dir /workspace/gradle-project-cache \
            :app:shared:ktlintCheck :app:desktop:ktlintCheck designFormatCheck \
            :app:shared:detektCommonMainSourceSet :app:shared:detektCommonTestSourceSet \
            :app:desktop:detekt designLint :app:shared:desktopTest :app:desktop:test \
            designTest :app:shared:check :app:desktop:check designCheck \
            :app:desktop:checkLicense :app:design_system:checkLicense \
            :tools:design_catalog:checkLicense :app:desktop:verifyUniFfiBindings \
            :app:desktop:verifyReleaseNativeLibrary :app:desktop:sourceReadiness \
            :app:desktop:integrationTest :app:desktop:verifyTestBridgeIsolation
        [ -z "$(git status --porcelain --untracked-files=all)" ]
        '\''
    '

printf '%s\n' "harvestcircle.linux_x86_64.development=pass"
printf '%s\n' "harvestcircle.source_commit=$source_commit"
printf '%s\n' "harvestcircle.radroots_revision=$radroots_revision"
