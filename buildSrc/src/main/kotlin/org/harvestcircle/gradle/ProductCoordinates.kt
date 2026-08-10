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
    }
}
