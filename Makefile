.DEFAULT_GOAL := help

# All Make configuration is owned here; host profiles are explicit arguments.
EXTBUILD ?= cargo extbuild
EXTBUILD_MACHINE_CONFIG ?=
ifneq ($(strip $(EXTBUILD_MACHINE_CONFIG)),)
override EXTBUILD += --machine-config "$(EXTBUILD_MACHINE_CONFIG)"
endif

GRADLE ?= ./gradlew
CARGO_MANIFEST := core/Cargo.toml
XTASK_MANIFEST := tools/xtask/Cargo.toml
export GRADLE CARGO_MANIFEST XTASK_MANIFEST
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
# Isolate portable builds from ambient workstation Gradle caches and init scripts.
GRADLE_USER_HOME ?= $(CURDIR)/.gradle/user-home
export GRADLE_USER_HOME
endif

# Serialize lifecycle targets that share native and Gradle output roots.
.NOTPARALLEL:
# Producers always describe this checkout, including ordinary development builds.
override HARVESTCIRCLE_BUILD_SOURCE_COMMIT := $(shell git rev-parse --verify HEAD 2>/dev/null)
override HARVESTCIRCLE_BUILD_SOURCE_DIRTY := $(if $(strip $(shell git status --porcelain --untracked-files=all 2>/dev/null)),true,false)
override HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN := 1.99.0
override HARVESTCIRCLE_BUILD_MODE := $(BUILD_MODE)
override SOURCE_DATE_EPOCH := $(shell git show -s --format=%ct HEAD 2>/dev/null)
export HARVESTCIRCLE_BUILD_SOURCE_COMMIT HARVESTCIRCLE_BUILD_SOURCE_DIRTY
export HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN HARVESTCIRCLE_BUILD_MODE SOURCE_DATE_EPOCH

# Make owns configuration and dependencies; scripts own command execution.
COMMANDS := \
    doctor repo-check lock metadata build-logic-check mode-check design-source-check \
    design-goldens-update format format-fix lint test check build \
    bindings dev-check dev run audit licenses \
    package host-package-check source-check integration-check host-ui-lifecycle-check clean linux-check
.PHONY: help _extbuild-doctor _require-governed development-check acceptance-check $(COMMANDS)

help:
	@scripts/commands.sh help

_extbuild-doctor:
	$(EXTBUILD) doctor

_require-governed:
	@scripts/commands.sh _require-governed

$(COMMANDS):
	@$(BUILD_RUNNER) scripts/commands.sh "$@"

ifeq ($(BUILD_MODE),governed)
doctor: _extbuild-doctor
endif
repo-check: doctor
lock: doctor
metadata: doctor
build-logic-check: doctor
mode-check: doctor
design-source-check: doctor
design-goldens-update: doctor
format: doctor
format-fix: doctor
lint: doctor
test: doctor
check: repo-check format lint test mode-check
build: doctor
bindings: doctor
dev-check: doctor
dev: doctor
run: doctor
audit: doctor
licenses: doctor
package: check
host-package-check: doctor
source-check: build-logic-check check bindings licenses dev-check
integration-check: build-logic-check check
host-ui-lifecycle-check: doctor
acceptance-check: integration-check host-package-check
clean: doctor
development-check: source-check integration-check
linux-check: _require-governed doctor
