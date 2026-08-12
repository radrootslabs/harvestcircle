package org.harvestcircle.application

class ImportSecretDraft private constructor(
    private var characters: CharArray?,
) {
    val length: Int
        get() = characters?.size ?: 0

    fun revealForDisplay(): String = characters?.concatToString().orEmpty()

    fun take(): String {
        val current = checkNotNull(characters) { "Import secret draft is no longer available" }
        return current.concatToString().also { clear() }
    }

    fun clear() {
        characters?.fill('\u0000')
        characters = null
    }

    override fun toString(): String = "ImportSecretDraft([REDACTED])"

    companion object {
        fun empty(): ImportSecretDraft = ImportSecretDraft(CharArray(0))

        fun from(value: String): ImportSecretDraft = ImportSecretDraft(value.take(MAX_IMPORT_SECRET_CHARS).toCharArray())
    }
}
