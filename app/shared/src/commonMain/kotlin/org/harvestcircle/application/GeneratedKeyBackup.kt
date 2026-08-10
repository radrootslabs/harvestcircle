package org.harvestcircle.application

class GeneratedKeyBackup(
    val npub: String,
    nsec: String,
) {
    private var recoveryText: String? = nsec

    fun revealNsec(): String =
        checkNotNull(recoveryText) {
            "Generated recovery material is no longer available"
        }

    fun clear() {
        recoveryText = null
    }

    override fun toString(): String = "GeneratedKeyBackup(npub=$npub, nsec=[REDACTED])"
}
