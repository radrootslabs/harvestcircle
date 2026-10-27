.DEFAULT_GOAL := help

# All Make configuration is owned here; host profiles are explicit arguments.
EXTBUILD ?= cargo extbuild
EXTBUILD_MACHINE_CONFIG ?=
ifneq ($(strip $(EXTBUILD_MACHINE_CONFIG)),)
override EXTBUILD += --machine-config "$(EXTBUILD_MACHINE_CONFIG)"
endif

GRADLE ?= ./gradlew
override CARGO := cargo +1.99.0
CARGO_MANIFEST := core/Cargo.toml
XTASK_MANIFEST := tools/xtask/Cargo.toml
BUILD_MODE ?= standalone
# Portable fixture Make processes select their own mode explicitly.
unexport BUILD_MODE
VALID_BUILD_MODES := standalone governed

ifeq ($(filter $(BUILD_MODE),$(VALID_BUILD_MODES)),)
$(error Unknown BUILD_MODE '$(BUILD_MODE)'; expected standalone or governed)
endif

ifeq ($(BUILD_MODE),governed)
override BUILD_RUNNER := $(EXTBUILD) run --
else
override BUILD_RUNNER :=
endif

# Serialize lifecycle targets that share native and Gradle output roots.
.NOTPARALLEL:
# Producers always describe this checkout, including ordinary development builds.
override HARVESTCIRCLE_BUILD_SOURCE_COMMIT := $(shell git rev-parse --verify HEAD 2>/dev/null)
override HARVESTCIRCLE_BUILD_SOURCE_DIRTY := $(if $(strip $(shell git status --porcelain --untracked-files=all 2>/dev/null)),true,false)
override HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN := 1.99.0
override SOURCE_DATE_EPOCH := $(shell git show -s --format=%ct HEAD 2>/dev/null)
export HARVESTCIRCLE_BUILD_SOURCE_COMMIT HARVESTCIRCLE_BUILD_SOURCE_DIRTY HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN SOURCE_DATE_EPOCH

.PHONY: help doctor _extbuild-doctor _require-governed repo-check lock metadata build-logic-check mode-check design-source-check design-goldens-update format format-fix lint test check build bindings api-check dev-check dev run audit licenses package host-package-check source-check integration-check development-check linux-check host-ui-lifecycle-check acceptance-check unsigned-release-check signing-check notarization-check release-check clean

help:
	@printf '%s\n' 'Start: doctor dev run dev-check' 'Build: build check clean' 'Focused checks: repo-check format format-fix lint test build-logic-check mode-check bindings api-check licenses source-check integration-check' 'Design: design-source-check design-goldens-update' 'Maintenance: metadata lock' 'Complete development: development-check' 'Packaging: package host-package-check unsigned-release-check' 'Other checks: audit host-ui-lifecycle-check signing-check notarization-check release-check' 'Linux qualification: BUILD_MODE=governed linux-check' 'Standalone is the default; add BUILD_MODE=governed and optional EXTBUILD_MACHINE_CONFIG=/path/to/profile.toml for extbuild.'

doctor:
	@printf '%s\n' "harvestcircle.build.mode=$(BUILD_MODE)"
	$(BUILD_RUNNER) tools/check-prerequisites.sh common
	$(BUILD_RUNNER) $(GRADLE) --version

_extbuild-doctor:
	$(EXTBUILD) doctor

_require-governed:
	@test "$(BUILD_MODE)" = governed || { printf '%s\n' 'This check requires BUILD_MODE=governed'; exit 2; }

ifeq ($(BUILD_MODE),governed)
doctor: _extbuild-doctor
endif

repo-check: doctor
	HARVESTCIRCLE_BUILD_MODE=$(BUILD_MODE) $(BUILD_RUNNER) $(CARGO) run --manifest-path $(XTASK_MANIFEST) --locked -- qualification-report

lock: doctor
	$(BUILD_RUNNER) $(CARGO) generate-lockfile --manifest-path $(CARGO_MANIFEST)
	$(BUILD_RUNNER) $(CARGO) generate-lockfile --manifest-path $(XTASK_MANIFEST)

metadata: doctor
	$(BUILD_RUNNER) $(CARGO) metadata --manifest-path $(CARGO_MANIFEST) --locked --format-version 1 --no-deps

build-logic-check: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon -p build-logic :contracts:check :plugins:check :plugins:functionalTest

mode-check: doctor
	$(BUILD_RUNNER) tools/test-build-modes.sh

design-source-check: doctor
	HARVESTCIRCLE_BUILD_MODE=$(BUILD_MODE) $(BUILD_RUNNER) $(CARGO) run --manifest-path $(XTASK_MANIFEST) --locked -- design-source-audit

design-goldens-update: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon -Pharvestcircle.updateMacosGoldens=true :app:shared:desktopTest --tests org.harvestcircle.ui.shell.HarvestCircleMacGoldenTest

format: doctor
	$(BUILD_RUNNER) $(CARGO) fmt --manifest-path $(CARGO_MANIFEST) --all -- --check
	$(BUILD_RUNNER) $(CARGO) fmt --manifest-path $(XTASK_MANIFEST) --all -- --check
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:shared:ktlintCheck :app:desktop:ktlintCheck designFormatCheck

format-fix: doctor
	$(BUILD_RUNNER) $(CARGO) fmt --manifest-path $(CARGO_MANIFEST) --all
	$(BUILD_RUNNER) $(CARGO) fmt --manifest-path $(XTASK_MANIFEST) --all
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:shared:ktlintFormat :app:desktop:ktlintFormat

lint: doctor
	$(BUILD_RUNNER) $(CARGO) clippy --manifest-path $(CARGO_MANIFEST) --workspace --all-targets --locked -- -D warnings
	$(BUILD_RUNNER) $(CARGO) clippy --manifest-path $(XTASK_MANIFEST) --all-targets --locked -- -D warnings
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:shared:detektCommonMainSourceSet :app:shared:detektCommonTestSourceSet :app:desktop:detekt designLint

test: doctor
	$(BUILD_RUNNER) $(CARGO) test --manifest-path $(CARGO_MANIFEST) --workspace --locked
	$(BUILD_RUNNER) $(CARGO) test --manifest-path $(XTASK_MANIFEST) --locked
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:shared:desktopTest :app:desktop:test designTest

check: repo-check format lint test mode-check
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:shared:check :app:desktop:check designCheck

build: doctor
	$(BUILD_RUNNER) $(CARGO) build --manifest-path $(CARGO_MANIFEST) --workspace --locked
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:build

bindings: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:verifyUniFfiBindings :app:desktop:verifyReleaseNativeLibrary

api-check: doctor
	$(BUILD_RUNNER) tools/check-prerequisites.sh api
	$(BUILD_RUNNER) tools/verify-storage-api.sh

dev-check: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon --configuration-cache --configuration-cache-problems=fail :app:desktop:hotRunArgfile
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:test --tests org.harvestcircle.application.NativeGeneratedRecoveryTest

dev: doctor
	$(BUILD_RUNNER) $(GRADLE) :app:desktop:hotRun

run: doctor
	$(BUILD_RUNNER) $(GRADLE) :app:desktop:run

audit: doctor
	$(BUILD_RUNNER) tools/check-prerequisites.sh audit
	$(BUILD_RUNNER) $(CARGO) audit --file core/Cargo.lock
	$(BUILD_RUNNER) $(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml advisories
	$(BUILD_RUNNER) $(GRADLE) --no-daemon --no-configuration-cache :app:desktop:dependencyCheckAnalyze :app:design_system:dependencyCheckAnalyze :tools:design_catalog:dependencyCheckAnalyze

licenses: doctor
	$(BUILD_RUNNER) tools/check-prerequisites.sh licenses
	$(BUILD_RUNNER) $(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml licenses sources
	$(BUILD_RUNNER) $(GRADLE) --no-daemon --no-parallel --no-configuration-cache :app:desktop:checkLicense :app:design_system:checkLicense :tools:design_catalog:checkLicense

package: check
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:verifyHostPackage

host-package-check: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:verifyHostPackage

source-check: build-logic-check check bindings api-check licenses dev-check
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:sourceReadiness

integration-check: build-logic-check check
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:integrationTest :app:desktop:verifyTestBridgeIsolation

host-ui-lifecycle-check: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:hostUiLifecycleTest

acceptance-check: integration-check host-package-check

clean: doctor
	$(BUILD_RUNNER) $(CARGO) clean --manifest-path $(CARGO_MANIFEST)
	$(BUILD_RUNNER) $(CARGO) clean --manifest-path $(XTASK_MANIFEST)
	$(BUILD_RUNNER) $(GRADLE) --no-daemon clean
	$(BUILD_RUNNER) $(GRADLE) --no-daemon -p build-logic clean

development-check: source-check integration-check

linux-check: _require-governed doctor
	$(EXTBUILD) run -- tools/run-linux-x86_64-development-check.sh

unsigned-release-check: _require-governed doctor
	$(BUILD_RUNNER) tools/check-prerequisites.sh licenses
	$(BUILD_RUNNER) $(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml licenses sources
	$(BUILD_RUNNER) $(GRADLE) --no-daemon --no-parallel --no-configuration-cache :app:desktop:unsignedReleaseReadiness

signing-check: _require-governed doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:signingReadiness

notarization-check: _require-governed doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:notarizationReadiness

release-check: _require-governed doctor
	$(BUILD_RUNNER) tools/check-prerequisites.sh audit
	$(BUILD_RUNNER) $(CARGO) audit --file core/Cargo.lock
	$(BUILD_RUNNER) $(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml advisories licenses sources
	$(BUILD_RUNNER) $(GRADLE) --no-daemon --no-parallel --no-configuration-cache :app:desktop:releaseReadiness
