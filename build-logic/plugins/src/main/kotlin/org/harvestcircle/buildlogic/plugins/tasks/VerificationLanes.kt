package org.harvestcircle.buildlogic.plugins.tasks

import org.harvestcircle.buildlogic.contracts.ProductCoordinates
import org.gradle.api.DefaultTask
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.work.DisableCachingByDefault

object VerificationLanes {
    private fun expected(environmentPrefix: String) =
        linkedMapOf(
            "schema" to "harvestcircle.verification-lanes.v3",
            "orchestration" to "explicit-make-modes",
            "source.standalone.command" to "make source-check",
            "source.governed.command" to "make governed-source-check",
            "source.credentials" to "none",
            "integration.standalone.command" to "make integration-check",
            "integration.governed.command" to "make governed-integration-check",
            "integration.credentials" to "none",
            "development.standalone.command" to "make development-check",
            "development.governed.command" to "make governed-development-check",
            "development.macos_aarch64.command" to "make governed-development-check",
            "development.linux_x86_64.command" to "make governed-linux-x86_64-development-check",
            "development.runners" to "macos-aarch64,linux-x86_64",
            "development.credentials" to "none",
            "provenance.commit" to environmentPrefix + "BUILD_SOURCE_COMMIT",
            "provenance.dirty" to environmentPrefix + "BUILD_SOURCE_DIRTY",
            "provenance.radroots" to environmentPrefix + "BUILD_RADROOTS_REVISION",
            "provenance.epoch" to "SOURCE_DATE_EPOCH",
            "release.state" to "deferred-unclaimed",
            "release.activation" to "declared-release-candidate-and-fresh-authority",
            "release.network_advisories" to "deferred",
            "release.packages" to "deferred",
            "release.evidence" to "deferred",
            "release.signing" to "deferred",
            "release.nix_oci" to "deferred",
        )

    fun parse(
        source: String,
        environmentPrefix: String,
    ): Map<String, String> {
        val expected = expected(environmentPrefix)
        val parsed = linkedMapOf<String, String>()
        source.trimEnd('\n', '\r').lineSequence().forEachIndexed { index, raw ->
            val line = raw.trim()
            require(line.isNotEmpty() && !line.startsWith('#')) {
                "Verification lane policy contains an empty or comment line at ${index + 1}"
            }
            val separator = line.indexOf('=')
            require(separator > 0 && separator < line.lastIndex && line.indexOf('=', separator + 1) == -1) {
                "Verification lane policy contains malformed syntax at ${index + 1}"
            }
            val key = line.substring(0, separator)
            val value = line.substring(separator + 1)
            require(parsed.put(key, value) == null) { "Duplicate verification lane key: $key" }
        }
        require(parsed.keys == expected.keys) { "Verification lane keys do not match the authority" }
        require(parsed == expected) { "Verification lane values do not match the authority" }
        return parsed
    }

    fun verifyDevelopmentMakefile(source: String) {
        check(source.lineSequence().count { it == "override CARGO := cargo +1.97.1" } == 1) {
            "Development verification must pin the exact Rust toolchain"
        }
        val exactTargetHeaders =
            listOf(
                "source-check: build-logic-check check bindings api-check licenses dev-check",
                "integration-check: build-logic-check check",
                "development-check: development-provenance-check source-check integration-check",
                "governed-development-check:",
                "governed-linux-x86_64-development-check: governed-doctor",
            )
        exactTargetHeaders.forEach { header ->
            check(source.lineSequence().count { it == header } == 1) {
                "Development Make target differs from the governed boundary: $header"
            }
        }

        val developmentHeader =
            source
                .lineSequence()
                .filter { it.startsWith("development-check:") }
                .filterNot { it.startsWith("development-check: export ") }
                .single()
        val forbiddenDependencies =
            listOf("audit", "package", "release", "signing", "notarization", "sbom")
        check(forbiddenDependencies.none { it in developmentHeader.lowercase() }) {
            "Development verification must not activate a release integration"
        }
        check("development-provenance-check" in developmentHeader) {
            "Development verification must bind deterministic source provenance"
        }
    }
}

@DisableCachingByDefault(because = "Verification lane policy checks produce no reusable output")
abstract class VerifyVerificationLanes : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val policyFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val productManifestFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val makefileFile: RegularFileProperty

    @TaskAction
    fun verify() {
        val source = policyFile.get().asFile.readText()
        val environmentPrefix =
            ProductCoordinates.load(productManifestFile.get().asFile)["environment.prefix"]
        val policy = VerificationLanes.parse(source, environmentPrefix)
        check(policy.size == 25)
        val makefile = makefileFile.get().asFile.readText()
        policy.filterKeys { it.endsWith(".command") }.forEach { (key, command) ->
            val target = command.removePrefix("make ")
            check(command == "make $target" && Regex("(?m)^${Regex.escape(target)}:").containsMatchIn(makefile)) {
                "Verification lane $key does not name a Make target"
            }
        }
        check(policy.values.none { ".github/" in it || ".act/" in it }) {
            "Verification policy must not reference an orchestration root"
        }
        VerificationLanes.verifyDevelopmentMakefile(makefile)
    }
}
