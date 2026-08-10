package org.harvestcircle.application

data class BuildInfo(
    val sourceCommit: String,
    val sourceDirty: BuildDirtyState,
    val radrootsRevision: String,
    val rustToolchain: String,
    val javaToolchain: String,
    val kotlinToolchain: String,
    val provenanceDigest: String,
    val sourceDateEpoch: ULong,
    val ffiContractId: String,
    val ffiContractHash: String,
    val snapshotSchemaVersion: UInt,
    val minimumStorageSchemaVersion: UInt,
    val currentStorageSchemaVersion: UInt,
) {
    companion object {
        fun unknown(): BuildInfo =
            BuildInfo(
                sourceCommit = UNKNOWN_PROVENANCE,
                sourceDirty = BuildDirtyState.Unknown,
                radrootsRevision = UNKNOWN_PROVENANCE,
                rustToolchain = UNKNOWN_PROVENANCE,
                javaToolchain = UNKNOWN_PROVENANCE,
                kotlinToolchain = UNKNOWN_PROVENANCE,
                provenanceDigest = UNKNOWN_PROVENANCE,
                sourceDateEpoch = 0UL,
                ffiContractId = UNKNOWN_PROVENANCE,
                ffiContractHash = UNKNOWN_PROVENANCE,
                snapshotSchemaVersion = 0U,
                minimumStorageSchemaVersion = 0U,
                currentStorageSchemaVersion = 0U,
            )
    }

    val releaseReady: Boolean
        get() =
            sourceCommit != UNKNOWN_PROVENANCE &&
                radrootsRevision != UNKNOWN_PROVENANCE &&
                sourceDirty == BuildDirtyState.Clean &&
                rustToolchain != UNKNOWN_PROVENANCE &&
                javaToolchain != UNKNOWN_PROVENANCE &&
                kotlinToolchain != UNKNOWN_PROVENANCE &&
                provenanceDigest != UNKNOWN_PROVENANCE

    fun safeDiagnostics(): Map<String, String> =
        linkedMapOf(
            "sourceCommit" to sourceCommit,
            "sourceDirty" to sourceDirty.name.lowercase(),
            "radrootsRevision" to radrootsRevision,
            "rustToolchain" to rustToolchain,
            "javaToolchain" to javaToolchain,
            "kotlinToolchain" to kotlinToolchain,
            "provenanceDigest" to provenanceDigest,
            "sourceDateEpoch" to sourceDateEpoch.toString(),
            "ffiContractId" to ffiContractId,
            "ffiContractHash" to ffiContractHash,
            "snapshotSchemaVersion" to snapshotSchemaVersion.toString(),
            "storageSchemaRange" to "$minimumStorageSchemaVersion..$currentStorageSchemaVersion",
        )
}

enum class BuildDirtyState {
    Clean,
    Dirty,
    Unknown,
}

const val UNKNOWN_PROVENANCE = "unknown"
