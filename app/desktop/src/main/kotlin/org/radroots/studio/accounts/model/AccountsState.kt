package org.radroots.studio.accounts.model

import java.net.URI

data class AddAccountDraft(
    val displayName: String = "",
    val serverUrl: String = "",
)

data class AccountsState(
    val accounts: List<Account> = emptyList(),
    val selectedAccountId: AccountId? = null,
    val addDraft: AddAccountDraft = AddAccountDraft(),
    val pendingRemovalAccountId: AccountId? = null,
    val problem: AccountsProblem? = null,
)

internal fun normalizeDisplayName(value: String): String? =
    value.trim().takeIf(String::isNotEmpty)

internal fun normalizeServerUrl(value: String): String? {
    val parsed = runCatching { URI(value.trim()) }.getOrNull() ?: return null
    val scheme = parsed.scheme?.lowercase() ?: return null
    val host = parsed.host?.lowercase() ?: return null
    if (scheme != "http" && scheme != "https") return null
    if (host.isEmpty() || parsed.userInfo != null || parsed.fragment != null) return null

    return runCatching {
        URI(
            scheme,
            null,
            host,
            parsed.port,
            parsed.path,
            parsed.query,
            null,
        ).normalize().toASCIIString()
    }.getOrNull()
}

internal fun AccountsState.requireValid(): AccountsState {
    require(accounts.map(Account::id).distinct().size == accounts.size) {
        "Account IDs must be unique"
    }
    accounts.forEach { account ->
        require(account.id.value.isNotBlank() && account.id.value == account.id.value.trim()) {
            "Account IDs must be trimmed and nonblank"
        }
        require(normalizeDisplayName(account.displayName) == account.displayName) {
            "Account display names must be trimmed and nonblank"
        }
        require(normalizeServerUrl(account.serverUrl) == account.serverUrl) {
            "Account server URLs must be normalized and valid"
        }
    }

    if (accounts.isEmpty()) {
        require(selectedAccountId == null) {
            "Selection must be empty when there are no accounts"
        }
    } else {
        require(selectedAccountId != null && accounts.any { it.id == selectedAccountId }) {
            "Selection must identify an existing account"
        }
    }
    require(
        pendingRemovalAccountId == null ||
            accounts.any { it.id == pendingRemovalAccountId },
    ) {
        "Pending removal must identify an existing account"
    }

    return this
}
