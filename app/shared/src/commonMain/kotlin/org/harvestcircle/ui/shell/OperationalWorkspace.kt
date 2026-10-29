package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import org.harvestcircle.application.DispatchPresenter
import org.harvestcircle.application.HandlingPresenter
import org.harvestcircle.application.HandlingViewState
import org.harvestcircle.application.StockPresenter
import org.harvestcircle.application.StockViewState
import org.harvestcircle.application.TracePresenter
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.navigation.AppRoute

@Composable
internal fun OperationalWorkspace(
    route: AppRoute,
    stock: StockViewState,
    stockPresenter: StockPresenter,
    submitReceipt: () -> Unit,
    handling: HandlingViewState,
    handlingPresenter: HandlingPresenter,
    submitHandling: () -> Unit,
    openStock: () -> Unit,
    openReferences: () -> Unit,
    openTrace: ((String) -> Unit)? = null,
    browseTrace: (() -> Unit)? = null,
) {
    val showReceipt = stock.receiptOpen || stock.submission != null
    val showHandling =
        !showReceipt && (route == AppRoute.HubWork || handling.group != null || handling.formOpen || handling.submission != null)
    if (showHandling) {
        HandlingWorkspace(
            handling,
            handlingPresenter,
            submitHandling,
            finish = {
                handlingPresenter.finishRecorded()
                stockPresenter.refresh()
            },
            receive = { stockPresenter.beginReceipt() },
            openStock = { trace ->
                stockPresenter.showTrace(trace)
                handlingPresenter.closeRecord()
                openStock()
            },
            openReferences = openReferences,
            receiveEnabled = !stock.loading && stock.page != null,
            receiveLabel = if (stock.draft != null) "Resume receipt draft" else "Receive original packs",
            openTrace = openTrace,
            browseTrace = browseTrace,
        )
    } else {
        Column(Modifier.fillMaxSize()) {
            if (handling.loading) HarvestCircleShellText("Reading local handling records…")
            handling.message?.let { HarvestCircleShellText(it) }
            StockWorkspace(
                route,
                stock,
                stockPresenter,
                submitReceipt,
                openReferences,
                openHandling = handlingPresenter::inspectGroup,
                manageRoute = false,
                handlingEnabled = !handling.loading,
                openTrace = openTrace,
                browseTrace = browseTrace,
            )
        }
    }
}

/** Bind the real route, independently of the record pane currently being inspected. */
@Composable
internal fun OperationalRouteEffects(
    route: AppRoute,
    stock: StockPresenter?,
    handling: HandlingPresenter?,
    dispatch: DispatchPresenter?,
    trace: TracePresenter?,
    stockAvailable: Boolean,
    handlingAvailable: Boolean,
    dispatchAvailable: Boolean,
) {
    val availableForRoute =
        when (route) {
            AppRoute.HubWork, AppRoute.HubStock -> stockAvailable
            AppRoute.HubDispatch -> dispatchAvailable
            else -> false
        }
    LaunchedEffect(route, stock, handling, dispatch, trace, availableForRoute) {
        trace?.park()
        when (route) {
            AppRoute.HubWork, AppRoute.HubStock ->
                if (stockAvailable) {
                    stock?.hideReceipt()
                    stock?.closeInspection()
                    if (handlingAvailable) {
                        handling?.parkDraft()
                        handling?.closeRecord()
                        handling?.refresh()
                    }
                    stock?.refresh()
                }
            AppRoute.HubDispatch ->
                if (dispatchAvailable) {
                    dispatch?.park()
                    dispatch?.refresh()
                }
            else -> Unit
        }
    }
}
