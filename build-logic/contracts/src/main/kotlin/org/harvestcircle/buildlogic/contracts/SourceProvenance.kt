package org.harvestcircle.buildlogic.contracts

public class SourceProvenance private constructor(
    private val root: Map<String, String>,
    private val imports: List<Map<String, String>>,
) {
    public val foundationBaseline: String get() = root.getValue("foundation_baseline")
    public val canonical: String
        get() =
            buildString {
                rootKeys.forEach { key -> append(key).append('=').append(root.getValue(key)).append('\n') }
                imports.forEach { entry ->
                    append("import.component=").append(entry.getValue("component")).append('\n')
                    append("import.commit=").append(entry.getValue("commit")).append('\n')
                }
            }
    public val digest: String get() = canonical.sha256()

    public companion object {
        private val rootKeys =
            linkedSetOf(
                "schema",
                "source_product",
                "source_repository",
                "foundation_baseline",
                "canonical_radroots_repository",
                "canonical_radroots_revision",
            )
        private val importKeys = linkedSetOf("component", "commit")
        private val assignment = Regex("^([A-Za-z0-9_]+)\\s*=\\s*\"([^\"]*)\"\\s*(?:#.*)?$")

        public fun parse(source: String): SourceProvenance {
            require(!source.startsWith('\uFEFF')) { "Source provenance must not contain a UTF-8 BOM" }
            val root = linkedMapOf<String, String>()
            val imports = mutableListOf<Map<String, String>>()
            var currentImport: LinkedHashMap<String, String>? = null
            fun completeImport() {
                currentImport?.let { entry ->
                    require(entry.keys == importKeys) { "Source provenance import keys do not match the contract" }
                    imports += entry.toMap()
                }
                currentImport = null
            }
            source.lineSequence().forEachIndexed { index, raw ->
                val line = raw.trim()
                if (line.isEmpty() || line.startsWith('#')) return@forEachIndexed
                if (line == "[[import]]") {
                    completeImport()
                    currentImport = linkedMapOf()
                    return@forEachIndexed
                }
                val match = requireNotNull(assignment.matchEntire(line)) {
                    "Source provenance line ${index + 1} is not a supported TOML string assignment"
                }
                val key = match.groupValues[1]
                val value = match.groupValues[2]
                require(value.isNotEmpty() && value.none(Char::isISOControl)) {
                    "Source provenance $key is empty or contains a control character"
                }
                val target = currentImport ?: root
                val allowed = if (currentImport == null) rootKeys else importKeys
                require(key in allowed) { "Unknown source provenance key $key" }
                require(target.put(key, value) == null) { "Duplicate source provenance key $key" }
            }
            completeImport()
            require(root.keys == rootKeys) { "Source provenance root keys do not match the contract" }
            require(root.getValue("schema") == "harvestcircle.source_provenance.v1")
            listOf("foundation_baseline", "canonical_radroots_revision").forEach { key ->
                require(root.getValue(key).isCanonicalHex(40)) { "Source provenance $key is not canonical" }
            }
            require(imports.isNotEmpty()) { "Source provenance imports must not be empty" }
            imports.forEach { require(it.getValue("commit").isCanonicalHex(40)) }
            require(imports.map { it.getValue("component") }.distinct().size == imports.size) {
                "Source provenance components must be unique"
            }
            return SourceProvenance(root.toMap(), imports.sortedBy { it.getValue("component") })
        }
    }
}
