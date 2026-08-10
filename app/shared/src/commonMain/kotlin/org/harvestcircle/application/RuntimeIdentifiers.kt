package org.harvestcircle.application

private val lowercaseHex = Regex("[0-9a-f]{64}")
private val opaqueIdentifier = Regex("[A-Za-z0-9][A-Za-z0-9._:-]{0,127}")
private val canonicalUuidV7 = Regex("[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}")

@JvmInline
value class IdentityId private constructor(
    val value: String,
) {
    companion object {
        fun fromPublicKeyHex(value: String): IdentityId {
            require(lowercaseHex.matches(value)) { "Identity ID must be canonical lowercase public-key hex" }
            return IdentityId(value)
        }
    }
}

@JvmInline
value class OperationId private constructor(
    val value: String,
) {
    companion object {
        fun from(value: String): OperationId {
            require(canonicalUuidV7.matches(value)) { "Operation ID must be canonical UUIDv7 text" }
            return OperationId(value)
        }
    }
}

@JvmInline
value class RecoveryRequestId private constructor(
    val value: String,
) {
    companion object {
        fun from(value: String): RecoveryRequestId {
            require(opaqueIdentifier.matches(value)) { "Recovery request ID is malformed" }
            return RecoveryRequestId(value)
        }
    }
}

@JvmInline
value class RemovalRequestId private constructor(
    val value: String,
) {
    companion object {
        fun from(value: String): RemovalRequestId {
            require(opaqueIdentifier.matches(value)) { "Removal request ID is malformed" }
            return RemovalRequestId(value)
        }
    }
}

@JvmInline
value class SnapshotRevision(
    val value: ULong,
)

@JvmInline
value class UnixSeconds(
    val value: Long,
)

fun interface ApplicationClock {
    fun now(): UnixSeconds
}

fun interface OperationIdSource {
    fun next(): OperationId
}

data class RequestContext(
    val operationId: OperationId,
    val expectedRevision: SnapshotRevision,
    val deadlineMillis: ULong,
) {
    init {
        require(deadlineMillis in 1UL..30_000UL) { "Command deadline is outside the supported window" }
    }
}

class SecretKeyInput private constructor(
    private var secret: String?,
) {
    fun take(): String = checkNotNull(secret).also { secret = null }

    fun clear() {
        secret = null
    }

    override fun toString(): String = "SecretKeyInput([REDACTED])"

    companion object {
        fun from(value: String): SecretKeyInput {
            require(value.isNotBlank() && value.length <= 256 && value.none(Char::isISOControl)) {
                "Secret-key input is malformed"
            }
            return SecretKeyInput(value)
        }
    }
}
