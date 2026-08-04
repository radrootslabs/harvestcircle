.DEFAULT_GOAL := help

GRADLE ?= ./gradlew
CARGO ?= cargo
CARGO_MANIFEST := core/Cargo.toml

.PHONY: help doctor format lint test check build bindings dev run package clean

help:
	@printf '%s\n' doctor format lint test check build bindings dev run package clean

doctor:
	java -version
	$(CARGO) --version
	$(GRADLE) --version

format: doctor
	$(CARGO) fmt --manifest-path $(CARGO_MANIFEST) --all -- --check

lint: doctor
	$(CARGO) clippy --manifest-path $(CARGO_MANIFEST) --workspace --all-targets -- -D warnings

test: doctor
	$(CARGO) test --manifest-path $(CARGO_MANIFEST) --workspace
	$(GRADLE) --no-daemon :app:desktop:test

check: format lint test
	$(GRADLE) --no-daemon :app:desktop:check

build: doctor
	$(CARGO) build --manifest-path $(CARGO_MANIFEST) --workspace
	$(GRADLE) --no-daemon :app:desktop:build

bindings: doctor
	$(GRADLE) --no-daemon :app:desktop:generateUniFfiKotlin :app:desktop:stageReleaseNativeLibrary

dev: doctor
	$(GRADLE) :app:desktop:hotRun --mainClass org.radroots.studio.desktop.MainKt

run: doctor
	$(GRADLE) :app:desktop:run

package: doctor
	$(GRADLE) --no-daemon :app:desktop:packageDistributionForCurrentOS

clean: doctor
	$(GRADLE) --no-daemon :app:desktop:clean
