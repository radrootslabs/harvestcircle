package org.harvestcircle.application

sealed interface ReferenceInputAdmission {
    class Accepted(
        val value: String,
    ) : ReferenceInputAdmission {
        override fun toString(): String = "Accepted(value=[REDACTED])"
    }

    data object PrivateKeyShaped : ReferenceInputAdmission

    data object TooLarge : ReferenceInputAdmission

    data object AmbiguousHex : ReferenceInputAdmission
}

object ReferenceInputPolicy {
    const val MAX_REFERENCE_CHARS = 2048
    const val MAX_REFERENCE_UTF8_BYTES = 2048
    const val MAX_SENSITIVE_PREFIX_SCAN = 32

    fun admit(raw: String): ReferenceInputAdmission {
        if (raw.length > MAX_REFERENCE_CHARS) return ReferenceInputAdmission.TooLarge
        if (hasSensitivePrefix(raw)) return ReferenceInputAdmission.PrivateKeyShaped
        if (raw.encodeToByteArray().size > MAX_REFERENCE_UTF8_BYTES) return ReferenceInputAdmission.TooLarge
        if (hasAmbiguousHexShape(raw)) return ReferenceInputAdmission.AmbiguousHex
        return ReferenceInputAdmission.Accepted(raw)
    }

    private fun hasAmbiguousHexShape(raw: String): Boolean {
        var start = 0
        var end = raw.length
        while (start < end && raw[start].isAsciiWhitespace()) start += 1
        while (end > start && raw[end - 1].isAsciiWhitespace()) end -= 1
        if (end - start >= NOSTR_PREFIX.length && raw.matchesAsciiIgnoreCase(start, NOSTR_PREFIX)) {
            start += NOSTR_PREFIX.length
        }
        if (end - start != HEX_REFERENCE_LENGTH) return false
        return (start until end).all { index -> raw[index].isAsciiHexDigit() }
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

private const val HEX_REFERENCE_LENGTH = 64
private const val NOSTR_PREFIX = "nostr:"
private val PRIVATE_KEY_PREFIX = "nsec" + "1"
private val PRIVATE_PREFIXES = listOf(PRIVATE_KEY_PREFIX, "nostr:" + PRIVATE_KEY_PREFIX)

private fun String.matchesAsciiIgnoreCase(
    start: Int,
    expected: String,
): Boolean {
    if (start + expected.length > length) return false
    return expected.indices.all { offset -> this[start + offset].equals(expected[offset], ignoreCase = true) }
}

private fun Char.isAsciiHexDigit(): Boolean = this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'

private fun Char.isAsciiWhitespace(): Boolean =
    this == ' ' || this == '\t' || this == '\n' || this == '\r' || this == '\u000B' || this == '\u000C'
