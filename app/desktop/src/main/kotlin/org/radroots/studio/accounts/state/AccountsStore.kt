package org.radroots.studio.accounts.state

import androidx.compose.runtime.State
import androidx.compose.runtime.mutableStateOf
import org.radroots.studio.accounts.model.AccountsAction
import org.radroots.studio.accounts.model.AccountsState
import org.radroots.studio.accounts.model.requireValid

class AccountsStore(
    initialState: AccountsState,
    private val reducer: AccountsReducer,
) {
    private val mutableState = mutableStateOf(initialState.requireValid())

    val state: State<AccountsState>
        get() = mutableState

    fun dispatch(action: AccountsAction) {
        mutableState.value = reducer.reduce(mutableState.value, action)
    }
}
