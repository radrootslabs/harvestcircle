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
        val required: Map<String, String> =
            linkedMapOf(
                "schema" to "harvestcircle.product.v1",
                "product.name" to "HarvestCircle",
                "product.slug" to "harvestcircle",
                "kotlin.root_namespace" to "org.harvestcircle",
                "desktop.application_id" to "org.harvestcircle.desktop",
                "desktop.bundle_id" to "org.harvestcircle.desktop",
                "desktop.main_class" to "org.harvestcircle.desktop.MainKt",
                "ffi.kotlin_package" to "org.harvestcircle.ffi",
                "ffi.cdylib_name" to "harvestcircle_ffi",
                "database.qualifier" to "org",
                "database.organization" to "harvestcircle",
                "database.application" to "desktop",
                "database.filename" to "harvestcircle.sqlite3",
                "keyring.service" to "org.harvestcircle.desktop.nostr",
                "environment.prefix" to "HARVESTCIRCLE_",
                "vendor.name" to "Radroots Labs",
                "copyright.notice" to "Copyright © 2026 HarvestCircle contributors",
            )

        fun load(file: File): ProductCoordinates = parse(file.readText())

        fun parse(source: String): ProductCoordinates {
            val parsed = linkedMapOf<String, String>()
            source.lineSequence().forEachIndexed { index, raw ->
                val line = raw.trim()
                if (line.isEmpty() || line.startsWith('#')) return@forEachIndexed
                val separator = line.indexOf('=')
                require(separator > 0) { "Product coordinate line ${index + 1} is not key=value" }
                val key = line.substring(0, separator).trim()
                val value = line.substring(separator + 1).trim()
                require(key in required) { "Unknown product coordinate $key" }
                require(value.isNotEmpty() && value.none(Char::isISOControl)) {
                    "Product coordinate $key is empty or contains a control character"
                }
                require(parsed.put(key, value) == null) { "Duplicate product coordinate $key" }
            }
            require(parsed.keys == required.keys) {
                "Product coordinate keys do not match the required schema"
            }
            required.forEach { (key, expected) ->
                require(parsed.getValue(key) == expected) {
                    "Product coordinate $key does not match the approved value"
                }
            }
            val digest =
                MessageDigest
                    .getInstance("SHA-256")
                    .digest(source.toByteArray())
                    .joinToString("") { byte -> "%02x".format(byte) }
            return ProductCoordinates(parsed.toMap(), digest)
        }
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
        check(coordinates["product.name"] == "HarvestCircle")
        check(coordinates["desktop.application_id"] == "org.harvestcircle.desktop")
        check(coordinates.digest.matches(Regex("[0-9a-f]{64}")))
        check(runCatching { ProductCoordinates.parse(source + "\nschema=harvestcircle.product.v1") }.isFailure)
        check(runCatching { ProductCoordinates.parse(source + "\nunknown=value") }.isFailure)
        check(runCatching { ProductCoordinates.parse(source.substringAfter('\n')) }.isFailure)
        check(
            runCatching {
                ProductCoordinates.parse(
                    source.replace("product.slug=harvestcircle", "product.slug=other"),
                )
            }.isFailure,
        )

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
        check(
            runCatching {
                FfiCompatibilityBaseline.parse(
                    baselineSource + "\ncontract.id=harvestcircle-desktop-ffi-v4",
                )
            }.isFailure,
        )
        check(baseline["product.coordinate_digest"] == coordinates.digest)
        val provenance = sourceProvenanceFile.get().asFile
        check(baseline["source.provenance_digest"] == FfiCompatibilityBaseline.digest(provenance))
        check(
            provenance.readLines().contains(
                "foundation_baseline = \"${baseline["source.foundation_baseline"]}\"",
            ),
        )
        val nativeCompatibility = nativeCompatibilityFile.get().asFile.readText()
        listOf(
            "contract.id" to "EXPECTED_FFI_CONTRACT_ID",
            "contract.hash" to "EXPECTED_FFI_CONTRACT_HASH",
            "product.coordinate_digest" to "EXPECTED_PRODUCT_COORDINATE_DIGEST",
            "source.provenance_digest" to "EXPECTED_SOURCE_PROVENANCE_DIGEST",
            "source.foundation_baseline" to "EXPECTED_SOURCE_FOUNDATION_BASELINE",
        ).forEach { (key, constant) ->
            check(nativeCompatibility.contains("$constant = \"${baseline[key]}\""))
        }
    }
}
