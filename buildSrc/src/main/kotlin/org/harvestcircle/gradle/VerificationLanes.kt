package org.harvestcircle.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction

object VerificationLanes {
    private fun expected(environmentPrefix: String) =
        linkedMapOf(
            "schema" to "harvestcircle.verification-lanes.v1",
            "orchestration" to "github-actions",
            "source.command" to "make source-check",
            "source.runner" to "linux",
            "source.workflow" to ".github/workflows/source.yml",
            "source.permissions" to "contents:read",
            "source.credentials" to "none",
            "package.command" to "make package-check",
            "package.runners" to "linux,macos,windows",
            "package.workflow" to ".github/workflows/package.yml",
            "package.permissions" to "contents:read",
            "package.credentials" to "none",
            "provenance.commit" to environmentPrefix + "BUILD_SOURCE_COMMIT",
            "provenance.dirty" to environmentPrefix + "BUILD_SOURCE_DIRTY",
            "provenance.radroots" to environmentPrefix + "BUILD_RADROOTS_REVISION",
            "provenance.epoch" to "SOURCE_DATE_EPOCH",
            "signing.command" to "make signing-check",
            "signing.runner" to "macos",
            "signing.permissions" to "contents:read",
            "signing.credentials" to "signing",
            "notarization.command" to "make notarization-check",
            "notarization.runner" to "macos",
            "notarization.permissions" to "contents:read",
            "notarization.credentials" to "notarization",
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
}

abstract class VerifyVerificationLanes : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val policyFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val productManifestFile: RegularFileProperty

    @get:Internal
    abstract val repositoryRoot: DirectoryProperty

    @TaskAction
    fun verify() {
        val source = policyFile.get().asFile.readText()
        val environmentPrefix =
            ProductCoordinates.load(productManifestFile.get().asFile)["environment.prefix"]
        val policy = VerificationLanes.parse(source, environmentPrefix)
        check(policy.size == 24)
        check(runCatching { VerificationLanes.parse(source + "source.permissions=write", environmentPrefix) }.isFailure)
        check(
            runCatching {
                VerificationLanes.parse(source.replace("contents:read", "contents:write"), environmentPrefix)
            }.isFailure,
        )
        check(
            runCatching {
                VerificationLanes.parse(source.replace("credentials=none", "credentials=all"), environmentPrefix)
            }.isFailure,
        )
        check(
            runCatching {
                VerificationLanes.parse(source.replace("source.runner=linux", "source.runner=macos"), environmentPrefix)
            }.isFailure,
        )
        val root = repositoryRoot.get().asFile.toPath()
        val sourceWorkflow = root.resolve(policy.getValue("source.workflow")).toFile().readText()
        val packageWorkflow = root.resolve(policy.getValue("package.workflow")).toFile().readText()
        verifyWorkflow(sourceWorkflow, policy.getValue("source.command"))
        verifyWorkflow(packageWorkflow, policy.getValue("package.command"))
        check(sourceWorkflow.contains("runs-on: ubuntu-latest"))
        listOf("ubuntu-latest", "macos-latest", "windows-latest").forEach { runner ->
            check(packageWorkflow.contains("- $runner")) { "Package workflow is missing $runner" }
        }
        listOf(
            policy.getValue("provenance.commit"),
            policy.getValue("provenance.dirty"),
            policy.getValue("provenance.radroots"),
            policy.getValue("provenance.epoch"),
        ).forEach { variable ->
            check(sourceWorkflow.contains(variable)) { "Source workflow is missing provenance variable $variable" }
            check(packageWorkflow.contains(variable)) { "Package workflow is missing provenance variable $variable" }
        }
    }

    private fun verifyWorkflow(
        source: String,
        command: String,
    ) {
        check(Regex("(?m)^permissions:\\s*\\n\\s{2}contents: read$").containsMatchIn(source)) {
            "Workflow permissions must be contents: read"
        }
        check(source.contains("run: $command")) { "Workflow does not invoke $command" }
        check(source.contains("persist-credentials: false")) { "Checkout credentials must not persist" }
        val actionPins = Regex("(?m)^\\s*uses:\\s+[^@\\s]+@([0-9a-f]{40})(?:\\s+#.*)?$").findAll(source).toList()
        check(actionPins.isNotEmpty()) { "Workflow does not use any pinned actions" }
        check(source.lineSequence().filter { "uses:" in it }.count() == actionPins.size) {
            "Every workflow action must use a full immutable commit SHA"
        }
        val forbidden = listOf("contents: write", "id-token: write", "pull-requests: write", "secrets.", "publish", "deploy")
        forbidden.forEach { token ->
            check(!source.contains(token, ignoreCase = true)) { "Workflow contains forbidden capability $token" }
        }
    }
}
