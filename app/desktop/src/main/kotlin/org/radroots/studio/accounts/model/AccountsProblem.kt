package org.radroots.studio.accounts.model

sealed interface AccountsProblem {
    data object BlankDisplayName : AccountsProblem

    data object InvalidServerUrl : AccountsProblem

    data object InvalidGeneratedAccountId : AccountsProblem

    data class DuplicateAccountId(
        val accountId: AccountId,
    ) : AccountsProblem

    data class AccountNotFound(
        val accountId: AccountId,
    ) : AccountsProblem

    data class RemovalTargetMismatch(
        val expectedAccountId: AccountId?,
        val actualAccountId: AccountId,
    ) : AccountsProblem
}
