package org.radroots.studio.accounts.state

import org.radroots.studio.accounts.model.AccountId

fun interface AccountIdFactory {
    fun nextId(): AccountId
}
