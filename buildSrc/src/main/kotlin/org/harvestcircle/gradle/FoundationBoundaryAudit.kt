package org.harvestcircle.gradle

import org.gradle.api.DefaultTask
import org.gradle.api.file.DirectoryProperty
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.Internal
import org.gradle.api.tasks.TaskAction
import java.nio.charset.StandardCharsets
import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.extension
import kotlin.io.path.name
import kotlin.io.path.readText

abstract class VerifyFoundationBoundaries : DefaultTask() {
    @get:Internal
    abstract val repositoryRoot: DirectoryProperty

    @get:Input
    abstract val gitAware: Property<Boolean>

    @TaskAction
    fun verify() {
        val root = repositoryRoot.get().asFile.toPath()
        val useGitInventory = gitAware.get() && Files.exists(root.resolve(".git"))
        val paths = if (useGitInventory) trackedPaths(root) else archivePaths(root)
        FoundationBoundaryAudit(root, paths).verify()
        if (!gitAware.get()) {
            verifyNegativeFixtures(root, paths)
        }
    }

    private fun trackedPaths(root: Path): List<String> {
        val process =
            ProcessBuilder("git", "-C", root.toString(), "ls-files", "-z")
                .redirectErrorStream(true)
                .start()
        val output = process.inputStream.readAllBytes()
        require(process.waitFor() == 0) {
            "Unable to enumerate tracked HarvestCircle sources: ${output.toString(StandardCharsets.UTF_8)}"
        }
        return output
            .toString(StandardCharsets.UTF_8)
            .split('\u0000')
            .filter(String::isNotEmpty)
            .filter { Files.exists(root.resolve(it)) }
            .sorted()
    }

    private fun archivePaths(root: Path): List<String> =
        Files.walk(root).use { paths ->
            paths
                .filter { path ->
                    path != root &&
                        shouldInspect(root.relativize(path).toString().replace('\\', '/'))
                }.map { root.relativize(it).toString().replace('\\', '/') }
                .sorted()
                .toList()
        }

    private fun shouldInspect(relative: String): Boolean {
        val segments = relative.split('/')
        return segments.none { it in setOf(".git", ".gradle", ".kotlin", ".idea", "build", "target", "out") }
    }

    private fun verifyNegativeFixtures(
        root: Path,
        paths: List<String>,
    ) {
        val fixtures =
            listOf(
                ".github/ISSUE_TEMPLATE/bug.md" to "# Bug report",
                "app/shared/src/commonMain/kotlin/org/harvestcircle/application/Leak.kt" to
                    ("import org.harvestcircle." + "ffi.BuildInfoDto"),
                "app/desktop/src/main/kotlin/org/harvestcircle/desktop/Blocking.kt" to
                    ("fun bad() = run" + "Blocking {}"),
                "app/desktop/src/main/kotlin/org/harvestcircle/desktop/Counter.kt" to
                    ("val bad = Atomic" + "Long(0)"),
                "core/target/generated/native.bin" to "generated",
                "config/credentials/release.key" to "not-a-real-key",
            )
        fixtures.forEach { (path, source) ->
            check(
                runCatching {
                    FoundationBoundaryAudit(root, paths + path, mapOf(path to source)).verify()
                }.isFailure,
            ) { "Foundation audit accepted negative fixture $path" }
        }
        val symlinkPath = "docs/escape.md"
        check(
            runCatching {
                FoundationBoundaryAudit(
                    root,
                    paths + symlinkPath,
                    overrides = mapOf(symlinkPath to "outside"),
                    symbolicLinks = setOf(symlinkPath),
                ).verify()
            }.isFailure,
        ) { "Foundation audit accepted symlink fixture $symlinkPath" }
        val provenancePath = "core/provenance/" + "stu" + "dio-import-v1.toml"
        val altered = root.resolve(provenancePath).readText().replace("09065a610d95e57acdc895a14c07580fa099e7c3", "0".repeat(40))
        check(
            runCatching {
                FoundationBoundaryAudit(root, paths, mapOf(provenancePath to altered)).verify()
            }.isFailure,
        ) { "Foundation audit accepted altered source provenance" }
    }
}

private class FoundationBoundaryAudit(
    private val root: Path,
    paths: List<String>,
    private val overrides: Map<String, String> = emptyMap(),
    private val symbolicLinks: Set<String> = emptySet(),
) {
    private val inventory = paths.distinct().sorted()
    private val legacyProduct = "stu" + "dio"
    private val provenancePath = "core/provenance/$legacyProduct-import-v1.toml"
    private val legacyRepository = "https://github.com/radrootslabs/${legacyProduct}_app"
    private val temporaryNamespace = listOf("org", "radroots", "harvestcircle").joinToString(".")
    private val textExtensions =
        setOf("gradle", "json", "kt", "kts", "lock", "md", "properties", "rs", "sql", "toml", "txt", "xml", "yaml", "yml")
    private val textNames =
        setOf(".gitattributes", ".gitignore", "AGENTS.md", "LICENSE", "Makefile", "NOTICE", "gradlew", "gradlew.bat")

    fun verify() {
        val findings = mutableListOf<String>()
        inventory.forEach { relative ->
            verifyPath(relative, findings)
            if (isText(relative)) {
                val source = overrides[relative] ?: readText(relative)
                verifyText(relative, source, findings)
            }
        }
        verifyExactContracts(findings)
        check(findings.isEmpty()) { findings.sorted().joinToString("\n") }
    }

    private fun verifyPath(
        relative: String,
        findings: MutableList<String>,
    ) {
        val normalized = relative.lowercase()
        if (normalized.startsWith("docs/") || normalized.startsWith("spec/") ||
            normalized.startsWith(".github/") || normalized.startsWith(".act/")) {
            findings += "$relative: forbidden repository root"
        }
        if (relative in symbolicLinks || Files.isSymbolicLink(root.resolve(relative))) {
            findings += "$relative: symbolic links are not allowed in public sources"
        }
        if (normalized.startsWith("core/target/") || normalized.contains("/build/") ||
            normalized.contains("/generated/") ||
            normalized.contains("generated/uniffi") || normalized.endsWith(".dylib") ||
            normalized.endsWith(".so") || normalized.endsWith(".dll") || normalized.endsWith(".class")
        ) {
            findings += "$relative: generated build output must not be source controlled"
        }
        if (normalized.endsWith(".pem") || normalized.endsWith(".key") || normalized.endsWith(".p12") ||
            normalized.endsWith(".pfx") || normalized.endsWith(".jks") || normalized.endsWith(".keystore") ||
            normalized.endsWith(".env") || normalized.contains("/credentials/")
        ) {
            findings += "$relative: credential or secret-shaped source path"
        }
        if (relative != provenancePath && normalized.contains(legacyProduct)) {
            findings += "$relative: legacy product name in source path"
        }
        val kotlinMarker = "/kotlin/"
        if (normalized.startsWith("app/") && normalized.contains(kotlinMarker) && normalized.endsWith(".kt")) {
            val packagePath = normalized.substringAfter(kotlinMarker)
            if (!packagePath.startsWith("org/harvestcircle/")) {
                findings += "$relative: Kotlin source is outside the final namespace"
            }
        }
    }

    private fun verifyText(
        relative: String,
        source: String,
        findings: MutableList<String>,
    ) {
        if (relative != provenancePath) {
            var inspected = if (relative == "core/Cargo.toml") source.replace(legacyRepository, "") else source
            if (relative == "NOTICE") {
                val legacyDisplayName = legacyProduct.replaceFirstChar { it.uppercase() }
                inspected =
                    inspected
                        .replace("Radroots $legacyDisplayName application work", "")
                        .replace("core/provenance/$legacyProduct-import-v1.toml", "")
            }
            if (inspected.lowercase().contains(legacyProduct)) {
                findings += "$relative: legacy product name outside the exact provenance allowlist"
            }
        }
        if (source.contains(temporaryNamespace) || source.contains(temporaryNamespace.replace('.', '/'))) {
            findings += "$relative: temporary product namespace"
        }
        val productionKotlin =
            relative.startsWith("app/") &&
                relative.endsWith(".kt") &&
                (relative.contains("/src/main/") || relative.contains("/src/commonMain/") || relative.contains("/src/desktopMain/"))
        if (productionKotlin && source.contains("run" + "Blocking")) {
            findings += "$relative: blocking coroutine bridge in application source"
        }
        if (productionKotlin && (source.contains("Atomic" + "Long") || source.contains("desktop" + "-operation:"))) {
            findings += "$relative: process-local operation counter"
        }
        if (relative.startsWith("app/shared/src/commonMain/") &&
            listOf("org.harvestcircle." + "ffi", "com.sun." + "jna", "java.", "javax.").any(source::contains)
        ) {
            findings += "$relative: platform dependency in shared common source"
        }
        inheritedPreferenceTokens().filter(source.lowercase()::contains).forEach { token ->
            findings += "$relative: inherited non-product preference $token"
        }
        val secretMarkers =
            listOf(
                "-----BEGIN " + "PRIVATE KEY-----",
                "AWS_" + "SECRET_ACCESS_KEY=",
                "gh" + "p_",
                "sk_" + "live_",
            )
        if (secretMarkers.any(source::contains)) {
            findings += "$relative: credential or private-key material in source text"
        }
        if (productionKotlin && source.lowercase().contains("nsec1")) {
            findings += "$relative: secret key literal in production Kotlin"
        }
    }

    private fun verifyExactContracts(findings: MutableList<String>) {
        val requiredPublicFiles =
            setOf(
                "README.md",
                "NOTICE",
                "CONTRIBUTING.md",
                "SECURITY.md",
                "LICENSE",
                "LICENSES/GPL-3.0-only.txt",
            )
        (requiredPublicFiles - inventory.toSet()).sorted().forEach { relative ->
            findings += "$relative: required public repository file is missing"
        }
        val cargo = text("core/Cargo.toml")
        if (cargo.lineSequence().count { it.trim() == "repository = \"$legacyRepository\"" } != 1) {
            findings += "core/Cargo.toml: legacy repository allowlist must be exact"
        }
        val provenance = text(provenancePath)
        if (!provenance.contains("source_repository = \"$legacyRepository\"") ||
            !provenance.contains("canonical_radroots_revision = \"09065a610d95e57acdc895a14c07580fa099e7c3\"") ||
            !provenance.contains("foundation_baseline = \"a2038b3e25b9e34f0b8fd001f26a8ed10b5772cb\"")
        ) {
            findings += "$provenancePath: exact source provenance changed"
        }
        val productCoordinates =
            runCatching {
                ProductCoordinates.parse(text("config/product/harvestcircle-v1.properties"))
            }.getOrElse { error ->
                findings += "config/product/harvestcircle-v1.properties: ${error.message}"
                null
            }
        val uniFfi = text("core/crates/harvestcircle_ffi/uniffi.toml")
        if (!uniFfi.contains("[crates.harvestcircle_ffi.bindings.kotlin]") ||
            productCoordinates == null ||
            !uniFfi.contains("package_name = \"${productCoordinates["ffi.kotlin_package"]}\"") ||
            !uniFfi.contains("cdylib_name = \"${productCoordinates["ffi.cdylib_name"]}\"")
        ) {
            findings += "core/crates/harvestcircle_ffi/uniffi.toml: final FFI identity changed"
        }
        val baseline = text("core/compatibility/harvestcircle-ffi-v4.properties")
        if (!baseline.contains("contract.id=harvestcircle-desktop-ffi-v4") || !baseline.contains("contract.major=4")) {
            findings += "core/compatibility/harvestcircle-ffi-v4.properties: FFI v4 identity changed"
        }
        val sharedBuild = text("app/shared/build.gradle.kts")
        if (Regex("(?m)^\\s*jvm\\(\"desktop\"\\)").findAll(sharedBuild).count() != 1 ||
            listOf("androidTarget", "iosArm", "iosX", "js(", "wasm").any(sharedBuild::contains)
        ) {
            findings += "app/shared/build.gradle.kts: shared KMP target boundary changed"
        }
    }

    private fun inheritedPreferenceTokens(): List<String> {
        val separator = "_"
        return listOf(
            listOf("use", "radroots", "dns").joinToString(separator),
            listOf("use", "radroots", "subnets").joinToString(separator),
            listOf("vpn", "on", "demand", "enabled").joinToString(separator),
            listOf("run", "as", "exit", "node").joinToString(separator),
            listOf("automatically", "check", "for", "updates").joinToString(separator),
            listOf("update", "channel").joinToString(separator),
            listOf("last", "update", "check", "summary").joinToString(separator),
            listOf("alternate", "server", "url").joinToString(separator),
        )
    }

    private fun isText(relative: String): Boolean {
        val path = Path.of(relative)
        return path.extension in textExtensions || path.name in textNames
    }

    private fun text(relative: String): String = overrides[relative] ?: readText(relative)

    private fun readText(relative: String): String {
        val path = root.resolve(relative)
        return if (Files.isRegularFile(path)) path.readText() else ""
    }
}
