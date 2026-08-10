package org.harvestcircle.application

data class BuildInfo(
    val productVersion: String,
    val distributionPackageVersion: String,
    val sourceCommit: String,
    val sourceDirty: BuildDirtyState,
    val radrootsRevision: String,
    val rustToolchain: String,
    val gradleToolchain: String,
    val javaToolchain: String,
    val kotlinToolchain: String,
    val composeMultiplatformVersion: String,
    val provenanceDigest: String,
    val sourceDateEpoch: ULong,
    val ffiContractId: String,
    val ffiContractMajor: UShort,
    val ffiContractMinor: UShort,
    val ffiContractHash: String,
    val snapshotSchemaVersion: UInt,
    val minimumStorageSchemaVersion: UInt,
    val currentStorageSchemaVersion: UInt,
    val eventRegistryState: EventRegistryState,
    val releaseCriteria: BuildReleaseCriteria,
) {
    companion object {
        fun unknown(): BuildInfo =
            BuildInfo(
                productVersion = UNKNOWN_PROVENANCE,
                distributionPackageVersion = UNKNOWN_PROVENANCE,
                sourceCommit = UNKNOWN_PROVENANCE,
                sourceDirty = BuildDirtyState.Unknown,
                radrootsRevision = UNKNOWN_PROVENANCE,
                rustToolchain = UNKNOWN_PROVENANCE,
                gradleToolchain = UNKNOWN_PROVENANCE,
                javaToolchain = UNKNOWN_PROVENANCE,
                kotlinToolchain = UNKNOWN_PROVENANCE,
                composeMultiplatformVersion = UNKNOWN_PROVENANCE,
                provenanceDigest = UNKNOWN_PROVENANCE,
                sourceDateEpoch = 0UL,
                ffiContractId = UNKNOWN_PROVENANCE,
                ffiContractMajor = 0.toUShort(),
                ffiContractMinor = 0.toUShort(),
                ffiContractHash = UNKNOWN_PROVENANCE,
                snapshotSchemaVersion = 0U,
                minimumStorageSchemaVersion = 0U,
                currentStorageSchemaVersion = 0U,
                eventRegistryState = EventRegistryState.NotApplicable,
                releaseCriteria = BuildReleaseCriteria.unknown(),
            )
    }

    val releaseReady: Boolean
        get() = releaseReadinessProblems.isEmpty()

    val releaseReadinessProblems: Set<ReleaseReadinessProblem>
        get() =
            buildSet {
                if (productVersion != releaseCriteria.productVersion) add(ReleaseReadinessProblem.ProductVersion)
                if (distributionPackageVersion != releaseCriteria.distributionPackageVersion) {
                    add(ReleaseReadinessProblem.DistributionPackageVersion)
                }
                if (!sourceCommit.isCanonicalHex(40)) add(ReleaseReadinessProblem.SourceCommit)
                if (sourceDirty != BuildDirtyState.Clean) add(ReleaseReadinessProblem.SourceDirty)
                if (!radrootsRevision.isCanonicalHex(40)) add(ReleaseReadinessProblem.RadrootsRevision)
                if (!rustToolchain.isKnownVersion()) add(ReleaseReadinessProblem.RustToolchain)
                if (!gradleToolchain.isKnownVersion()) add(ReleaseReadinessProblem.GradleToolchain)
                if (!javaToolchain.isKnownVersion()) add(ReleaseReadinessProblem.JavaToolchain)
                if (!kotlinToolchain.isKnownVersion()) add(ReleaseReadinessProblem.KotlinToolchain)
                if (!composeMultiplatformVersion.isKnownVersion()) add(ReleaseReadinessProblem.ComposeMultiplatform)
                if (!provenanceDigest.isCanonicalHex(64)) add(ReleaseReadinessProblem.ProvenanceDigest)
                if (sourceDateEpoch == 0UL) add(ReleaseReadinessProblem.SourceDateEpoch)
                if (ffiContractId != releaseCriteria.ffiContractId) add(ReleaseReadinessProblem.FfiContractId)
                if (ffiContractMajor != releaseCriteria.ffiContractMajor) add(ReleaseReadinessProblem.FfiContractMajor)
                if (ffiContractMinor != releaseCriteria.ffiContractMinor) add(ReleaseReadinessProblem.FfiContractMinor)
                if (!ffiContractHash.isCanonicalHex(64) || ffiContractHash != releaseCriteria.ffiContractHash) {
                    add(ReleaseReadinessProblem.FfiContractHash)
                }
                if (snapshotSchemaVersion == 0U || snapshotSchemaVersion != releaseCriteria.snapshotSchemaVersion) {
                    add(ReleaseReadinessProblem.SnapshotSchema)
                }
                if (
                    minimumStorageSchemaVersion == 0U ||
                    minimumStorageSchemaVersion > currentStorageSchemaVersion ||
                    currentStorageSchemaVersion < releaseCriteria.minimumStorageSchemaVersion ||
                    minimumStorageSchemaVersion > releaseCriteria.maximumStorageSchemaVersion
                ) {
                    add(ReleaseReadinessProblem.StorageSchema)
                }
                if (eventRegistryState != EventRegistryState.NotApplicable) {
                    add(ReleaseReadinessProblem.EventRegistry)
                }
            }

    fun safeDiagnostics(): Map<String, String> =
        linkedMapOf(
            "productVersion" to productVersion,
            "distributionPackageVersion" to distributionPackageVersion,
            "sourceCommit" to sourceCommit,
            "sourceDirty" to sourceDirty.name.lowercase(),
            "radrootsRevision" to radrootsRevision,
            "rustToolchain" to rustToolchain,
            "gradleToolchain" to gradleToolchain,
            "javaToolchain" to javaToolchain,
            "kotlinToolchain" to kotlinToolchain,
            "composeMultiplatformVersion" to composeMultiplatformVersion,
            "provenanceDigest" to provenanceDigest,
            "sourceDateEpoch" to sourceDateEpoch.toString(),
            "ffiContractId" to ffiContractId,
            "ffiContractVersion" to "$ffiContractMajor.$ffiContractMinor",
            "ffiContractHash" to ffiContractHash,
            "snapshotSchemaVersion" to snapshotSchemaVersion.toString(),
            "storageSchemaRange" to "$minimumStorageSchemaVersion..$currentStorageSchemaVersion",
            "eventRegistryState" to eventRegistryState.diagnosticName,
            "releaseReady" to releaseReady.toString(),
            "releaseReadinessProblems" to releaseReadinessProblems.joinToString(",") { it.diagnosticName },
        )
}

data class BuildReleaseCriteria(
    val productVersion: String,
    val distributionPackageVersion: String,
    val ffiContractId: String,
    val ffiContractMajor: UShort,
    val ffiContractMinor: UShort,
    val ffiContractHash: String,
    val snapshotSchemaVersion: UInt,
    val minimumStorageSchemaVersion: UInt,
    val maximumStorageSchemaVersion: UInt,
) {
    companion object {
        fun unknown(): BuildReleaseCriteria =
            BuildReleaseCriteria(
                productVersion = UNKNOWN_PROVENANCE,
                distributionPackageVersion = UNKNOWN_PROVENANCE,
                ffiContractId = UNKNOWN_PROVENANCE,
                ffiContractMajor = 0.toUShort(),
                ffiContractMinor = 0.toUShort(),
                ffiContractHash = UNKNOWN_PROVENANCE,
                snapshotSchemaVersion = 0U,
                minimumStorageSchemaVersion = 0U,
                maximumStorageSchemaVersion = 0U,
            )
    }
}

enum class EventRegistryState(
    val diagnosticName: String,
) {
    NotApplicable("not_applicable"),
    Unknown("unknown"),
}

enum class ReleaseReadinessProblem(
    val diagnosticName: String,
) {
    ProductVersion("product_version"),
    DistributionPackageVersion("distribution_package_version"),
    SourceCommit("source_commit"),
    SourceDirty("source_dirty"),
    RadrootsRevision("radroots_revision"),
    RustToolchain("rust_toolchain"),
    GradleToolchain("gradle_toolchain"),
    JavaToolchain("java_toolchain"),
    KotlinToolchain("kotlin_toolchain"),
    ComposeMultiplatform("compose_multiplatform"),
    ProvenanceDigest("provenance_digest"),
    SourceDateEpoch("source_date_epoch"),
    FfiContractId("ffi_contract_id"),
    FfiContractMajor("ffi_contract_major"),
    FfiContractMinor("ffi_contract_minor"),
    FfiContractHash("ffi_contract_hash"),
    SnapshotSchema("snapshot_schema"),
    StorageSchema("storage_schema"),
    EventRegistry("event_registry"),
}

enum class BuildDirtyState {
    Clean,
    Dirty,
    Unknown,
}

const val UNKNOWN_PROVENANCE = "unknown"

private fun String.isCanonicalHex(length: Int): Boolean = this.length == length && all { it in '0'..'9' || it in 'a'..'f' }

private fun String.isKnownVersion(): Boolean = this != UNKNOWN_PROVENANCE && isNotBlank() && length <= 128 && all { it.code in 0x21..0x7e }
