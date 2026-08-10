package org.harvestcircle.application

import org.harvestcircle.ffi.CompatibilityDescriptor
import org.harvestcircle.ffi.CompatibilityExpectation
import org.harvestcircle.application.generated.NativeCompatibilityExpectations as Expected

internal class NativeCompatibilityException :
    IllegalStateException(
        "The application and native runtime are incompatible.",
    )

internal fun verifyNativeCompatibility(descriptor: CompatibilityDescriptor): CompatibilityExpectation {
    val compatible =
        descriptor.contractId == Expected.ffiContractId &&
            descriptor.productVersion == Expected.productVersion &&
            descriptor.cargoPackageVersion == Expected.cargoPackageVersion &&
            descriptor.distributionPackageVersion == Expected.distributionPackageVersion &&
            descriptor.contractMajor == Expected.ffiContractMajor &&
            descriptor.contractMinor >= Expected.minimumFfiContractMinor &&
            descriptor.contractHash == Expected.ffiContractHash &&
            descriptor.productCoordinateDigest == Expected.productCoordinateDigest &&
            descriptor.snapshotSchemaVersion == Expected.snapshotSchema &&
            descriptor.currentSchemaVersion >= Expected.minimumStorageSchema &&
            descriptor.minimumSchemaVersion <= Expected.maximumStorageSchema &&
            descriptor.sourceProvenanceDigest == Expected.sourceProvenanceDigest &&
            descriptor.sourceFoundationBaseline == Expected.sourceFoundationBaseline
    if (!compatible) throw NativeCompatibilityException()
    return CompatibilityExpectation(
        contractId = Expected.ffiContractId,
        contractMajor = Expected.ffiContractMajor,
        minimumContractMinor = Expected.minimumFfiContractMinor,
        contractHash = Expected.ffiContractHash,
        productCoordinateDigest = Expected.productCoordinateDigest,
        snapshotSchemaVersion = Expected.snapshotSchema,
        minimumSchemaVersion = Expected.minimumStorageSchema,
        maximumSchemaVersion = Expected.maximumStorageSchema,
    )
}
