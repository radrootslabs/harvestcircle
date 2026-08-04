.DEFAULT_GOAL := help

GRADLE ?= ./gradlew
CARGO ?= cargo
CARGO_MANIFEST := core/Cargo.toml

.PHONY: help doctor format format-fix lint test check build bindings dev run audit licenses package release-check clean

help:
	@printf '%s\n' doctor format format-fix lint test check build bindings dev run audit licenses package release-check clean

doctor:
	java -version
	$(CARGO) --version
	$(GRADLE) --version

format: doctor
	$(CARGO) fmt --manifest-path $(CARGO_MANIFEST) --all -- --check
	$(GRADLE) --no-daemon :app:desktop:ktlintCheck

format-fix: doctor
	$(CARGO) fmt --manifest-path $(CARGO_MANIFEST) --all
	$(GRADLE) --no-daemon :app:desktop:ktlintFormat

lint: doctor
	$(CARGO) clippy --manifest-path $(CARGO_MANIFEST) --workspace --all-targets -- -D warnings
	$(GRADLE) --no-daemon :app:desktop:detekt

test: doctor
	$(CARGO) test --manifest-path $(CARGO_MANIFEST) --workspace
	$(GRADLE) --no-daemon :app:desktop:test

check: format lint test
	$(GRADLE) --no-daemon :app:desktop:check

build: doctor
	$(CARGO) build --manifest-path $(CARGO_MANIFEST) --workspace
	$(GRADLE) --no-daemon :app:desktop:build

bindings: doctor
	$(GRADLE) --no-daemon :app:desktop:verifyUniFfiBindings :app:desktop:verifyReleaseNativeLibrary

dev: doctor
	$(GRADLE) :app:desktop:hotRun --mainClass org.radroots.studio.desktop.MainKt

run: doctor
	$(GRADLE) :app:desktop:run

audit: doctor
	$(CARGO) audit --file core/Cargo.lock
	$(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml advisories
	$(GRADLE) --no-daemon --no-configuration-cache :app:desktop:dependencyCheckAnalyze

licenses: doctor
	$(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml licenses sources
	$(GRADLE) --no-daemon --no-parallel --no-configuration-cache :app:desktop:checkLicense

package: check
	$(GRADLE) --no-daemon :app:desktop:verifyMacOsPackage

release-check: doctor
	$(CARGO) audit --file core/Cargo.lock
	$(CARGO) deny --manifest-path $(CARGO_MANIFEST) check --config core/deny.toml advisories licenses sources
	$(GRADLE) --no-daemon --no-parallel --no-configuration-cache :app:desktop:releaseReadiness

clean: doctor
	$(CARGO) clean --manifest-path $(CARGO_MANIFEST)
	$(GRADLE) --no-daemon clean
