package org.harvestcircle.application

sealed interface ReferenceInputAdmission {
    class Accepted(
        val value: String,
    ) : ReferenceInputAdmission {
        override fun toString(): String = "Accepted(value=[REDACTED])"
    }

    data object PrivateKeyShaped : ReferenceInputAdmission

    data object TooLarge : ReferenceInputAdmission
}

object ReferenceInputPolicy {
    const val MAX_REFERENCE_CHARS = 2048
    const val MAX_REFERENCE_UTF8_BYTES = 2048
    const val MAX_SENSITIVE_PREFIX_SCAN = 32

    fun admit(raw: String): ReferenceInputAdmission {
        if (raw.length > MAX_REFERENCE_CHARS) return ReferenceInputAdmission.TooLarge
        if (hasSensitivePrefix(raw)) return ReferenceInputAdmission.PrivateKeyShaped
        if (raw.encodeToByteArray().size > MAX_REFERENCE_UTF8_BYTES) return ReferenceInputAdmission.TooLarge
        return ReferenceInputAdmission.Accepted(raw)
    }

    private fun hasSensitivePrefix(raw: String): Boolean {
        val scanEnd = minOf(raw.length, MAX_SENSITIVE_PREFIX_SCAN)
        var prefixStart = 0
        while (prefixStart < scanEnd && raw[prefixStart].isAsciiWhitespace()) prefixStart += 1
        if (prefixStart == scanEnd) return raw.length > scanEnd
        return PRIVATE_PREFIXES.any { prefix -> raw.matchesSensitivePrefix(prefixStart, scanEnd, prefix) }
    }

    private fun String.matchesSensitivePrefix(
        start: Int,
        scanEnd: Int,
        prefix: String,
    ): Boolean {
        val available = minOf(prefix.length, scanEnd - start)
        for (offset in 0 until available) {
            if (!this[start + offset].equals(prefix[offset], ignoreCase = true)) return false
        }
        return available == prefix.length || (start + available == scanEnd && length > scanEnd)
    }
}

private val PRIVATE_KEY_PREFIX = "nsec" + "1"
private val PRIVATE_PREFIXES = listOf(PRIVATE_KEY_PREFIX, "nostr:" + PRIVATE_KEY_PREFIX)

private fun Char.isAsciiWhitespace(): Boolean =
    this == ' ' || this == '\t' || this == '\n' || this == '\r' || this == '\u000B' || this == '\u000C'
