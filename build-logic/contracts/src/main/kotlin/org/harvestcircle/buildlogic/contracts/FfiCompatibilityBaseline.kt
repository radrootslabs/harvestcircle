package org.harvestcircle.buildlogic.contracts

import java.io.File

public class FfiCompatibilityBaseline private constructor(
    private val values: Map<String, String>,
) {
    public operator fun get(key: String): String = values.getValue(key)

    public companion object {
        private val requiredKeys =
            linkedSetOf(
                "schema",
                "contract.id",
                "contract.major",
                "contract.minor",
                "contract.hash",
                "product.coordinate_digest",
                "snapshot.schema",
                "storage.schema.minimum",
                "storage.schema.current",
                "product.version",
                "package.version",
                "source.provenance_digest",
                "source.foundation_baseline",
            )

        public fun load(file: File): FfiCompatibilityBaseline = parse(file.readText())

        public fun parse(source: String): FfiCompatibilityBaseline {
            require(!source.startsWith('\uFEFF')) { "FFI baseline must not contain a UTF-8 BOM" }
            val values = linkedMapOf<String, String>()
            source.lineSequence().forEachIndexed { index, raw ->
                val line = raw.trim()
                if (line.isEmpty() || line.startsWith('#')) return@forEachIndexed
                val separator = line.indexOf('=')
                require(separator > 0) { "FFI baseline line ${index + 1} is not key=value" }
                val key = line.substring(0, separator).trim()
                val value = line.substring(separator + 1).trim()
                require(key in requiredKeys) { "Unknown FFI baseline key $key" }
                require(value.isNotEmpty() && value.none(Char::isISOControl)) {
                    "FFI baseline $key is empty or contains a control character"
                }
                require(values.put(key, value) == null) { "Duplicate FFI baseline key $key" }
            }
            require(values.keys == requiredKeys) { "FFI baseline keys do not match the v4 schema" }
            require(values.getValue("schema") == "harvestcircle.ffi.v4")
            require(values.getValue("contract.id") == "harvestcircle-desktop-ffi-v4")
            require(values.getValue("contract.major") == "4")
            require(values.getValue("contract.minor") == "1")
            require(values.getValue("snapshot.schema") == "1")
            require(values.getValue("storage.schema.minimum") == "5")
            require(values.getValue("storage.schema.current") == "10")
            listOf("contract.hash", "product.coordinate_digest", "source.provenance_digest").forEach { key ->
                require(values.getValue(key).isCanonicalHex(64))
            }
            require(values.getValue("source.foundation_baseline").isCanonicalHex(40))
            require(values.getValue("product.version").matches(Regex("[0-9]+\\.[0-9]+\\.[0-9]+(?:-[0-9A-Za-z.-]+)?")))
            require(values.getValue("package.version").matches(Regex("[1-9][0-9]*(?:\\.[0-9]+){0,2}")))
            return FfiCompatibilityBaseline(values.toMap())
        }
    }
}
