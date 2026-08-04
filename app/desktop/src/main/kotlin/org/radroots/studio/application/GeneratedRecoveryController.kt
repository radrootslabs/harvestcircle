package org.radroots.studio.application

class GeneratedKeyBackup internal constructor(
    val npub: String,
    nsec: String,
) {
    private var recoveryText: String? = nsec

    internal fun revealNsec(): String =
        checkNotNull(recoveryText) {
            "Generated recovery material is no longer available"
        }

    internal fun clear() {
        recoveryText = null
    }

    override fun toString(): String = "GeneratedKeyBackup(npub=$npub, nsec=[REDACTED])"
}

internal class GeneratedRecoveryController : AutoCloseable {
    private var active: GeneratedKeyBackup? = null

    fun begin(
        npub: String,
        nsec: String,
    ): GeneratedKeyBackup {
        check(active == null) { "Generated-key recovery is already active" }
        return GeneratedKeyBackup(npub, nsec).also { active = it }
    }

    fun acknowledge(): Boolean {
        val recovery = active ?: return false
        recovery.clear()
        active = null
        return true
    }

    override fun close() {
        acknowledge()
    }
}
