package org.radroots.studio.accounts.model

enum class LoginStatus {
    LoggedOut,
    LoggedIn,
}

data class Account(
    val id: AccountId,
    val displayName: String,
    val serverUrl: String,
    val loginStatus: LoginStatus,
)
