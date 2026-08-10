.DEFAULT_GOAL := help

GRADLE ?= ./gradlew
CARGO ?= cargo
CARGO_MANIFEST := core/Cargo.toml
EXTBUILD ?= $(if $(shell cargo extbuild --version 2>/dev/null),cargo extbuild run --)

.PHONY: help doctor lock metadata format format-fix lint test check build bindings dev run audit licenses package source-check package-check signing-check notarization-check release-check clean

help:
	@printf '%s\n' doctor lock metadata format format-fix lint test check build bindings dev run audit licenses package source-check package-check signing-check notarization-check release-check clean

doctor:
	$(if $(strip $(EXTBUILD)),cargo extbuild doctor,@:)
	$(EXTBUILD) java -version
	$(EXTBUILD) $(CARGO) --version
	$(EXTBUILD) $(GRADLE) --version

lock: doctor
	$(EXTBUILD) $(CARGO) generate-lockfile --manifest-path $(CARGO_MANIFEST)

metadata: doctor
	$(EXTBUILD) $(CARGO) metadata --manifest-path $(CARGO_MANIFEST) --locked --format-version 1 --no-deps

format: doctor
	$(EXTBUILD) $(CARGO) fmt --manifest-path $(CARGO_MANIFEST) --all -- --check
	$(EXTBUILD) $(GRADLE) --no-daemon :app:shared:ktlintCheck :app:desktop:ktlintCheck

format-fix: doctor
	$(EXTBUILD) $(CARGO) fmt --manifest-path $(CARGO_MANIFEST) --all
	$(EXTBUILD) $(GRADLE) --no-daemon :app:shared:ktlintFormat :app:desktop:ktlintFormat

lint: doctor
	$(EXTBUILD) $(CARGO) clippy --manifest-path $(CARGO_MANIFEST) --workspace --all-targets --locked -- -D warnings
	$(EXTBUILD) $(GRADLE) --no-daemon :app:shared:detektCommonMainSourceSet :app:shared:detektCommonTestSourceSet :app:desktop:detekt

test: doctor
	$(EXTBUILD) $(CARGO) test --manifest-path $(CARGO_MANIFEST) --workspace --locked
	$(EXTBUILD) $(GRADLE) --no-daemon :app:shared:desktopTest :app:desktop:test

check: format lint test
	$(EXTBUILD) $(GRADLE) --no-daemon :app:shared:check :app:desktop:check

build: doctor
	$(EXTBUILD) $(CARGO) build --manifest-path $(CARGO_MANIFEST) --workspace --locked
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:build

bindings: doctor
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:verifyUniFfiBindings :app:desktop:verifyReleaseNativeLibrary

dev: doctor
	$(EXTBUILD) $(GRADLE) :app:desktop:hotRun

run: doctor
	$(EXTBUILD) $(GRADLE) :app:desktop:run

audit: doctor
	$(EXTBUILD) $(CARGO) audit --file core/Cargo.lock
	$(EXTBUILD) $(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml advisories
	$(EXTBUILD) $(GRADLE) --no-daemon --no-configuration-cache :app:desktop:dependencyCheckAnalyze

licenses: doctor
	$(EXTBUILD) $(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml licenses sources
	$(EXTBUILD) $(GRADLE) --no-daemon --no-parallel --no-configuration-cache :app:desktop:checkLicense

package: check
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:verifyHostPackage

source-check: check bindings licenses
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:sourceReadiness

package-check: source-check
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:packageReadiness

signing-check: doctor
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:signingReadiness

notarization-check: doctor
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:notarizationReadiness

release-check: doctor
	$(EXTBUILD) $(CARGO) audit --file core/Cargo.lock
	$(EXTBUILD) $(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml advisories licenses sources
	$(EXTBUILD) $(GRADLE) --no-daemon --no-parallel --no-configuration-cache :app:desktop:releaseReadiness

clean: doctor
	$(EXTBUILD) $(CARGO) clean --manifest-path $(CARGO_MANIFEST)
	$(EXTBUILD) $(GRADLE) --no-daemon clean
