.DEFAULT_GOAL := help

GRADLE ?= ./gradlew

.PHONY: help doctor check test build dev run package clean

help:
	@printf '%s\n' doctor check test build dev run package clean

doctor:
	$(GRADLE) --version

check: doctor
	$(GRADLE) --no-daemon :app:desktop:check

test: doctor
	$(GRADLE) --no-daemon :app:desktop:test

build: doctor
	$(GRADLE) --no-daemon :app:desktop:build

dev: doctor
	$(GRADLE) :app:desktop:hotRun --mainClass org.radroots.studio.desktop.MainKt

run: doctor
	$(GRADLE) :app:desktop:run

package: doctor
	$(GRADLE) --no-daemon :app:desktop:packageDistributionForCurrentOS

clean: doctor
	$(GRADLE) --no-daemon :app:desktop:clean
