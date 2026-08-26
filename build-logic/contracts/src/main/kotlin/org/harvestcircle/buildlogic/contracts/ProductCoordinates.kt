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
                "storage.service_id",
                "storage.instance_id",
                "storage.database_filename",
                "storage.lock_filename",
                "storage.application_id",
                "storage.application_id_text",
                "storage.initial_schema_version",
                "legacy.database.qualifier",
                "legacy.database.organization",
                "legacy.database.application",
                "legacy.database.filename",
                "legacy.database.disposition",
                "platform.macos.architecture",
                "platform.linux.architecture",
                "limit.identities",
                "limit.unfinished_durable_operations",
                "limit.preference_value_utf8_bytes",
                "limit.relay_endpoints",
                "limit.relay_url_bytes",
                "limit.events_per_relay",
                "limit.events_total",
                "limit.observers",
                "limit.actor_mailbox",
                "limit.command_deadline_min_ms",
                "limit.command_deadline_max_ms",
                "backup.member_limit",
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
                    "product.slug", "ffi.cdylib_name",
                    -> value.isLowerIdentifier()
                    "kotlin.root_namespace", "desktop.application_id", "desktop.bundle_id", "desktop.main_class",
                    "ffi.kotlin_package", "keyring.service",
                    -> value.isDottedIdentifier()
                    "storage.service_id" -> value == "harvestcircle"
                    "storage.instance_id" -> value == "desktop"
                    "storage.database_filename" -> value == "state.sqlite"
                    "storage.lock_filename" -> value == "state.lock"
                    "storage.application_id" -> value == "1212371505"
                    "storage.application_id_text" -> value == "HCR1"
                    "storage.initial_schema_version" -> value == "1"
                    "legacy.database.qualifier" -> value == "org"
                    "legacy.database.organization" -> value == "harvestcircle"
                    "legacy.database.application" -> value == "desktop"
                    "legacy.database.filename" -> value == "harvestcircle.sqlite3"
                    "legacy.database.disposition" -> value == "untouched_and_unsupported"
                    "platform.macos.architecture" -> value == "aarch64"
                    "platform.linux.architecture" -> value == "x86_64"
                    "limit.identities" -> value == "256"
                    "limit.unfinished_durable_operations" -> value == "1024"
                    "limit.preference_value_utf8_bytes" -> value == "4096"
                    "limit.relay_endpoints" -> value == "16"
                    "limit.relay_url_bytes" -> value == "2048"
                    "limit.events_per_relay" -> value == "64"
                    "limit.events_total" -> value == "1024"
                    "limit.observers" -> value == "32"
                    "limit.actor_mailbox" -> value == "64"
                    "limit.command_deadline_min_ms" -> value == "1"
                    "limit.command_deadline_max_ms" -> value == "30000"
                    "backup.member_limit" -> value == "caller_supplied_positive"
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
