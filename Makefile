.DEFAULT_GOAL := help

GRADLE ?= ./gradlew
CARGO ?= cargo
CARGO_MANIFEST := core/Cargo.toml
EXTBUILD := cargo extbuild run --

.PHONY: help doctor format format-fix lint test check build bindings dev run audit licenses package release-check clean

help:
	@printf '%s\n' doctor format format-fix lint test check build bindings dev run audit licenses package release-check clean

doctor:
	cargo extbuild doctor
	$(EXTBUILD) java -version
	$(EXTBUILD) $(CARGO) --version
	$(EXTBUILD) $(GRADLE) --version

format: doctor
	$(EXTBUILD) $(CARGO) fmt --manifest-path $(CARGO_MANIFEST) --all -- --check
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:ktlintCheck

format-fix: doctor
	$(EXTBUILD) $(CARGO) fmt --manifest-path $(CARGO_MANIFEST) --all
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:ktlintFormat

lint: doctor
	$(EXTBUILD) $(CARGO) clippy --manifest-path $(CARGO_MANIFEST) --workspace --all-targets --locked -- -D warnings
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:detekt

test: doctor
	$(EXTBUILD) $(CARGO) test --manifest-path $(CARGO_MANIFEST) --workspace --locked
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:test

check: format lint test
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:check

build: doctor
	$(EXTBUILD) $(CARGO) build --manifest-path $(CARGO_MANIFEST) --workspace --locked
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:build

bindings: doctor
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:verifyUniFfiBindings :app:desktop:verifyReleaseNativeLibrary

dev: doctor
	$(EXTBUILD) $(GRADLE) :app:desktop:hotRun --mainClass org.radroots.studio.desktop.MainKt

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
	$(EXTBUILD) $(GRADLE) --no-daemon :app:desktop:verifyMacOsPackage

release-check: doctor
	$(EXTBUILD) $(CARGO) audit --file core/Cargo.lock
	$(EXTBUILD) $(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml advisories licenses sources
	$(EXTBUILD) $(GRADLE) --no-daemon --no-parallel --no-configuration-cache :app:desktop:releaseReadiness

clean: doctor
	$(EXTBUILD) $(CARGO) clean --manifest-path $(CARGO_MANIFEST)
	$(EXTBUILD) $(GRADLE) --no-daemon clean
