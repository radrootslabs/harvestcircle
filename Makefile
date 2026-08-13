.DEFAULT_GOAL := help

GRADLE ?= ./gradlew
CARGO ?= cargo
CARGO_MANIFEST := core/Cargo.toml
XTASK_MANIFEST := tools/xtask/Cargo.toml
BUILD_MODE ?= standalone
VALID_BUILD_MODES := standalone governed

ifeq ($(filter $(BUILD_MODE),$(VALID_BUILD_MODES)),)
$(error Unknown BUILD_MODE '$(BUILD_MODE)'; expected standalone or governed)
endif

ifeq ($(BUILD_MODE),governed)
override BUILD_RUNNER := cargo extbuild run --
else
override BUILD_RUNNER :=
endif

.PHONY: help doctor governed-doctor lock metadata build-logic-check build-logic-stability-check mode-check format format-fix lint test check governed-check build bindings dev-check dev run audit licenses foundation-check package host-package-check governed-package-check source-check governed-source-check package-check integration-check governed-integration-check acceptance-check signing-check _signing-check notarization-check _notarization-check release-check _release-check clean

help:
	@printf '%s\n' doctor governed-doctor lock metadata build-logic-check build-logic-stability-check mode-check format format-fix lint test check governed-check build bindings dev-check dev run audit licenses foundation-check package host-package-check governed-package-check source-check governed-source-check package-check integration-check governed-integration-check acceptance-check signing-check notarization-check release-check clean

doctor:
	@printf '%s\n' "harvestcircle.build.mode=$(BUILD_MODE)"
	$(BUILD_RUNNER) java -version
	$(BUILD_RUNNER) $(CARGO) --version
	$(BUILD_RUNNER) $(GRADLE) --version

governed-doctor:
	$(CARGO) extbuild doctor

ifeq ($(BUILD_MODE),governed)
doctor: governed-doctor
endif

lock: doctor
	$(BUILD_RUNNER) $(CARGO) generate-lockfile --manifest-path $(CARGO_MANIFEST)
	$(BUILD_RUNNER) $(CARGO) generate-lockfile --manifest-path $(XTASK_MANIFEST)

metadata: doctor
	$(BUILD_RUNNER) $(CARGO) metadata --manifest-path $(CARGO_MANIFEST) --locked --format-version 1 --no-deps

build-logic-check: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon -p build-logic :contracts:check :plugins:check :plugins:functionalTest

build-logic-stability-check: doctor
	$(BUILD_RUNNER) tools/test-build-logic-stability.sh

mode-check:
	tools/test-build-modes.sh

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

check: format lint test foundation-check mode-check
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:shared:check :app:desktop:check designCheck

governed-check:
	$(MAKE) --no-print-directory BUILD_MODE=governed check

build: doctor
	$(BUILD_RUNNER) $(CARGO) build --manifest-path $(CARGO_MANIFEST) --workspace --locked
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:build

bindings: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:verifyUniFfiBindings :app:desktop:verifyReleaseNativeLibrary

dev-check: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon --configuration-cache --configuration-cache-problems=fail :app:desktop:hotRunArgfile

dev: doctor
	$(BUILD_RUNNER) $(GRADLE) :app:desktop:hotRun

run: doctor
	$(BUILD_RUNNER) $(GRADLE) :app:desktop:run

audit: doctor
	$(BUILD_RUNNER) $(CARGO) audit --file core/Cargo.lock
	$(BUILD_RUNNER) $(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml advisories
	$(BUILD_RUNNER) $(GRADLE) --no-daemon --no-configuration-cache :app:desktop:dependencyCheckAnalyze

licenses: doctor
	$(BUILD_RUNNER) $(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml licenses sources
	$(BUILD_RUNNER) $(GRADLE) --no-daemon --no-parallel --no-configuration-cache :app:desktop:checkLicense

foundation-check: doctor
	HARVESTCIRCLE_BUILD_MODE=$(BUILD_MODE) $(BUILD_RUNNER) $(CARGO) run --manifest-path $(XTASK_MANIFEST) --locked -- qualification-report

package: check
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:verifyHostPackage

host-package-check: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:verifyHostPackage

governed-package-check:
	$(MAKE) --no-print-directory BUILD_MODE=governed host-package-check

source-check: build-logic-check check bindings licenses dev-check
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:sourceReadiness

governed-source-check:
	$(MAKE) --no-print-directory BUILD_MODE=governed source-check

package-check: source-check
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:packageReadiness

integration-check: build-logic-check check
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:integrationTest :app:desktop:verifyTestBridgeIsolation

governed-integration-check:
	$(MAKE) --no-print-directory BUILD_MODE=governed integration-check

acceptance-check: integration-check host-package-check

signing-check:
	$(MAKE) --no-print-directory BUILD_MODE=governed _signing-check

_signing-check: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:signingReadiness

notarization-check:
	$(MAKE) --no-print-directory BUILD_MODE=governed _notarization-check

_notarization-check: doctor
	$(BUILD_RUNNER) $(GRADLE) --no-daemon :app:desktop:notarizationReadiness

release-check:
	$(MAKE) --no-print-directory BUILD_MODE=governed _release-check

_release-check: doctor
	@test "$(BUILD_MODE)" = governed || { printf '%s\n' 'release-check requires governed mode'; exit 2; }
	$(BUILD_RUNNER) $(CARGO) audit --file core/Cargo.lock
	$(BUILD_RUNNER) $(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml advisories licenses sources
	$(BUILD_RUNNER) $(GRADLE) --no-daemon --no-parallel --no-configuration-cache :app:desktop:releaseReadiness

clean: doctor
	$(BUILD_RUNNER) $(CARGO) clean --manifest-path $(CARGO_MANIFEST)
	$(BUILD_RUNNER) $(CARGO) clean --manifest-path $(XTASK_MANIFEST)
	$(BUILD_RUNNER) $(GRADLE) --no-daemon clean
	$(BUILD_RUNNER) $(GRADLE) --no-daemon -p build-logic clean
