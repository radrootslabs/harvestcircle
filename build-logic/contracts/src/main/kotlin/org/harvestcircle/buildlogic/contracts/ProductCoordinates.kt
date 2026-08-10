package org.harvestcircle.buildlogic.contracts

import java.io.File

public class ProductCoordinates private constructor(
    private val values: Map<String, String>,
    public val canonical: String,
) {
    public val digest: String = canonical.sha256()

    public operator fun get(key: String): String = values.getValue(key)

    public companion object {
        public const val SCHEMA: String = "harvestcircle.product.v1"
        public const val schema: String = SCHEMA

        public val requiredKeys: List<String> =
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

        public fun load(file: File): ProductCoordinates = parse(file.readText())

        public fun parse(source: String): ProductCoordinates {
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
                require(value.isNotEmpty() && value.none(Char::isISOControl)) {
                    "Product coordinate $key is empty or contains a control character"
                }
                require(parsed.put(key, value) == null) { "Duplicate product coordinate $key" }
            }
            require(parsed.keys == requiredKeys.toSet()) { "Product coordinate keys do not match the required schema" }
            parsed.forEach(::validateCoordinate)
            val canonical = requiredKeys.joinToString(separator = "\n", postfix = "\n") { key -> "$key=${parsed.getValue(key)}" }
            return ProductCoordinates(parsed.toMap(), canonical)
        }

        private fun validateCoordinate(
            key: String,
            value: String,
        ) {
            val valid =
                when (key) {
                    "schema" -> value == SCHEMA
                    "product.name", "vendor.name", "copyright.notice" -> value.length <= 160
                    "product.slug", "ffi.cdylib_name", "database.qualifier", "database.organization",
                    "database.application",
                    -> value.isLowerIdentifier()
                    "kotlin.root_namespace", "desktop.application_id", "desktop.bundle_id", "desktop.main_class",
                    "ffi.kotlin_package", "keyring.service",
                    -> value.isDottedIdentifier()
                    "database.filename" ->
                        value.endsWith(".sqlite3") && ".." !in value &&
                            value.all { it.isAsciiLetterOrDigit() || it in "._-" }
                    "environment.prefix" ->
                        value.firstOrNull() in 'A'..'Z' && value.endsWith('_') &&
                            value.all { it in 'A'..'Z' || it.isDigit() || it == '_' }
                    else -> false
                }
            require(valid) { "Product coordinate $key has an invalid value" }
        }
    }
}

private fun String.isLowerIdentifier(): Boolean =
    firstOrNull() in 'a'..'z' && all { it in 'a'..'z' || it.isDigit() || it == '_' }

private fun String.isDottedIdentifier(): Boolean =
    split('.').all { segment ->
        segment.firstOrNull()?.let { it.isAsciiLetter() || it == '_' } == true &&
            segment.all { it.isAsciiLetterOrDigit() || it == '_' }
    }

internal fun Char.isAsciiLetter(): Boolean = this in 'a'..'z' || this in 'A'..'Z'

internal fun Char.isAsciiLetterOrDigit(): Boolean = isAsciiLetter() || isDigit()
