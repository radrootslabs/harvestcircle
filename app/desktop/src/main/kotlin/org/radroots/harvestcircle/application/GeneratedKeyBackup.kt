package org.radroots.harvestcircle.application

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
