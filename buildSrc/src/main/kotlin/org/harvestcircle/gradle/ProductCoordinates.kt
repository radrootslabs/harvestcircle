package org.harvestcircle.gradle

import java.io.File
import java.security.MessageDigest
import org.gradle.api.DefaultTask
import org.gradle.api.file.RegularFileProperty
import org.gradle.api.tasks.InputFile
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction

class ProductCoordinates private constructor(
    private val values: Map<String, String>,
    val digest: String,
) {
    operator fun get(key: String): String = values.getValue(key)

    companion object {
        const val schema = "harvestcircle.product.v1"
        val requiredKeys: List<String> =
            listOf(
                "schema",
                "product.name",
                "product.slug",
                "kotlin.root_namespace",
                "desktop.application_id",
                "desktop.bundle_id",
                "desktop.main_class",
                "ffi.kotlin_package",
                "ffi.cdylib_name",
                "database.qualifier",
                "database.organization",
                "database.application",
                "database.filename",
                "keyring.service",
                "environment.prefix",
                "vendor.name",
                "copyright.notice",
            )

        fun load(file: File): ProductCoordinates = parse(file.readText())

        fun parse(source: String): ProductCoordinates {
            require(!source.startsWith('\uFEFF')) { "Product coordinates must not contain a UTF-8 BOM" }
            val parsed = linkedMapOf<String, String>()
            source.lineSequence().forEachIndexed { index, raw ->
                val line = raw.trim()
                if (line.isEmpty() || line.startsWith('#')) return@forEachIndexed
                val separator = line.indexOf('=')
                require(separator > 0) { "Product coordinate line ${index + 1} is not key=value" }
                val key = line.substring(0, separator).trim()
                val value = line.substring(separator + 1).trim()
                require(key in requiredKeys) { "Unknown product coordinate $key" }
                require(key.isNotEmpty() && key.none(Char::isISOControl) && value.isNotEmpty() && value.none(Char::isISOControl)) {
                    "Product coordinate $key is empty or contains a control character"
                }
                require(parsed.put(key, value) == null) { "Duplicate product coordinate $key" }
            }
            require(parsed.keys == requiredKeys.toSet()) {
                "Product coordinate keys do not match the required schema"
            }
            parsed.forEach(::validateCoordinate)
            val canonical =
                buildString {
                    requiredKeys.forEach { key ->
                        append(key).append('=').append(parsed.getValue(key)).append('\n')
                    }
                }
            val digest =
                MessageDigest
                    .getInstance("SHA-256")
                    .digest(canonical.toByteArray(Charsets.UTF_8))
                    .joinToString("") { byte -> "%02x".format(byte) }
            return ProductCoordinates(parsed.toMap(), digest)
        }

        private fun validateCoordinate(
            key: String,
            value: String,
        ) {
            val valid =
                when (key) {
                    "schema" -> value == schema
                    "product.name", "vendor.name", "copyright.notice" -> value.length <= 160
                    "product.slug",
                    "ffi.cdylib_name",
                    "database.qualifier",
                    "database.organization",
                    "database.application",
                    -> value.isLowerIdentifier()
                    "kotlin.root_namespace",
                    "desktop.application_id",
                    "desktop.bundle_id",
                    "desktop.main_class",
                    "ffi.kotlin_package",
                    "keyring.service",
                    -> value.isDottedIdentifier()
                    "database.filename" ->
                        value.endsWith(".sqlite3") &&
                            ".." !in value &&
                            value.all { it.isAsciiLetterOrDigit() || it in "._-" }
                    "environment.prefix" ->
                        value.firstOrNull()?.let { it in 'A'..'Z' } == true &&
                            value.endsWith('_') &&
                            value.all { it in 'A'..'Z' || it.isDigit() || it == '_' }
                    else -> false
                }
            require(valid) { "Product coordinate $key has an invalid value" }
        }

        private fun String.isLowerIdentifier(): Boolean =
            firstOrNull()?.let { it in 'a'..'z' } == true &&
                all { it in 'a'..'z' || it.isDigit() || it == '_' }

        private fun String.isDottedIdentifier(): Boolean =
            split('.').all { segment ->
                segment.firstOrNull()?.let { it.isAsciiLetter() || it == '_' } == true &&
                    segment.all { it.isAsciiLetterOrDigit() || it == '_' }
            }

        private fun Char.isAsciiLetter(): Boolean = this in 'a'..'z' || this in 'A'..'Z'

        private fun Char.isAsciiLetterOrDigit(): Boolean = isAsciiLetter() || isDigit()
    }
}

abstract class VerifyProductCoordinates : DefaultTask() {
    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val manifestFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val uniFfiConfigFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val ffiBaselineFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val sourceProvenanceFile: RegularFileProperty

    @get:InputFile
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val nativeCompatibilityFile: RegularFileProperty

    @TaskAction
    fun verify() {
        val source = manifestFile.get().asFile.readText()
        val coordinates = ProductCoordinates.parse(source)
        check(coordinates.digest.matches(Regex("[0-9a-f]{64}")))
        val equivalentSources =
            listOf(
                source.replace("\n", "\r\n"),
                source.trimEnd(),
                "# comment\n$source",
                source.lineSequence().joinToString("\n") { line ->
                    if (line.isBlank() || line.startsWith('#')) line else line.replaceFirst("=", " = ")
                },
            )
        equivalentSources.forEach { equivalent ->
            check(ProductCoordinates.parse(equivalent).digest == coordinates.digest)
        }
        check(runCatching { ProductCoordinates.parse("\uFEFF$source") }.isFailure)
        check(runCatching { ProductCoordinates.parse(source + "\nschema=${ProductCoordinates.schema}") }.isFailure)
        check(runCatching { ProductCoordinates.parse(source + "\nunknown=value") }.isFailure)
        check(runCatching { ProductCoordinates.parse(source.substringAfter('\n')) }.isFailure)
        check(runCatching { ProductCoordinates.parse(source.replaceCoordinate("product.slug", "INVALID")) }.isFailure)
        check(
            runCatching {
                ProductCoordinates.parse(source.replaceCoordinate("database.filename", "../other.sqlite3"))
            }.isFailure,
        )
        validCoordinateMutations.forEach { (key, replacement) ->
            val mutated = ProductCoordinates.parse(source.replaceCoordinate(key, replacement))
            check(mutated[key] == replacement)
            check(mutated.digest != coordinates.digest)
        }

        val uniFfiConfig = uniFfiConfigFile.get().asFile.readText()
        check(
            uniFfiConfig.contains(
                "package_name = \"${coordinates["ffi.kotlin_package"]}\"",
            ),
        )
        check(
            uniFfiConfig.contains(
                "cdylib_name = \"${coordinates["ffi.cdylib_name"]}\"",
            ),
        )

        val baseline = FfiCompatibilityBaseline.load(ffiBaselineFile.get().asFile)
        val baselineSource = ffiBaselineFile.get().asFile.readText()
        check(runCatching { FfiCompatibilityBaseline.parse(baselineSource + "\nunknown=value") }.isFailure)
        check(runCatching { FfiCompatibilityBaseline.parse(baselineSource.substringAfter('\n')) }.isFailure)
        check(runCatching { FfiCompatibilityBaseline.parse("\uFEFF$baselineSource") }.isFailure)
        check(
            runCatching {
                FfiCompatibilityBaseline.parse(
                    baselineSource.replace(
                        Regex("(?m)^contract\\.hash=.*$"),
                        "contract.hash=malformed",
                    ),
                )
            }.isFailure,
        )
        check(
            runCatching {
                FfiCompatibilityBaseline.parse(
                    baselineSource.replace(
                        Regex("(?m)^package\\.version=.*$"),
                        "package.version=invalid",
                    ),
                )
            }.isFailure,
        )
        check(
            runCatching {
                FfiCompatibilityBaseline.parse(
                    baselineSource + "\ncontract.id=harvestcircle-desktop-ffi-v4",
                )
            }.isFailure,
        )
        check(baseline["product.coordinate_digest"] == coordinates.digest)
        val provenanceSource = sourceProvenanceFile.get().asFile.readText()
        val provenance = SourceProvenance.parse(provenanceSource)
        check(baseline["source.provenance_digest"] == provenance.digest)
        check(provenance.foundationBaseline == baseline["source.foundation_baseline"])
        val equivalentProvenance =
            listOf(
                provenanceSource.replace("\n", "\r\n"),
                provenanceSource.trimEnd(),
                "# comment\n$provenanceSource",
                provenanceSource.replace(
                    "component = \"domain\"\ncommit = \"a4d7deebec3e2ce2c1daa455de6d79857839aed0\"",
                    "commit = \"a4d7deebec3e2ce2c1daa455de6d79857839aed0\"\ncomponent = \"domain\"",
                ),
            )
        equivalentProvenance.forEach { equivalent ->
            check(SourceProvenance.parse(equivalent).digest == provenance.digest)
        }
        check(runCatching { SourceProvenance.parse("\uFEFF$provenanceSource") }.isFailure)
        check(runCatching { SourceProvenance.parse("unknown = \"value\"\n$provenanceSource") }.isFailure)
        check(
            SourceProvenance.parse(
                provenanceSource.replace(
                    "a4d7deebec3e2ce2c1daa455de6d79857839aed0",
                    "b4d7deebec3e2ce2c1daa455de6d79857839aed0",
                ),
            ).digest != provenance.digest,
        )
        val nativeCompatibility = nativeCompatibilityFile.get().asFile.readText()
        check(nativeCompatibility.contains("NativeCompatibilityExpectations as Expected"))
        listOf(
            "contract.id",
            "contract.hash",
            "product.coordinate_digest",
            "source.provenance_digest",
            "source.foundation_baseline",
        ).forEach { key -> check(!nativeCompatibility.contains(baseline[key])) }
    }

    private fun String.replaceCoordinate(
        key: String,
        replacement: String,
    ): String =
        lineSequence().joinToString("\n") { line ->
            if (line.substringBefore('=', missingDelimiterValue = "") == key) "$key=$replacement" else line
        }

    private val validCoordinateMutations =
        linkedMapOf(
            "product.name" to "Harvest Circle Test",
            "product.slug" to "harvestcircle_test",
            "kotlin.root_namespace" to "org.example",
            "desktop.application_id" to "org.example.desktop",
            "desktop.bundle_id" to "org.example.bundle",
            "desktop.main_class" to "org.example.MainKt",
            "ffi.kotlin_package" to "org.example.ffi",
            "ffi.cdylib_name" to "example_ffi",
            "database.qualifier" to "com",
            "database.organization" to "example",
            "database.application" to "test",
            "database.filename" to "example.sqlite3",
            "keyring.service" to "org.example.desktop.nostr",
            "environment.prefix" to "EXAMPLE_",
            "vendor.name" to "Example Cooperative",
            "copyright.notice" to "Copyright Example contributors",
        ).also { mutations ->
            check(mutations.size + 1 == ProductCoordinates.requiredKeys.size)
        }
}
