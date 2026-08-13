package org.harvestcircle.application

enum class SyncStatusLabel(
    val text: String,
) {
    NotYetObserved("Not yet observed"),
    Available("Synced"),
    Degraded("Limited connection"),
    Unavailable("Offline"),
}

enum class SignerStatusLabel(
    val text: String,
) {
    ReadOnly("Read-only"),
    SignedOut("Signed out"),
    Available("Local identity active"),
    CredentialMissing("Signer unavailable"),
}

enum class BannerSeverity { Information, Caution, Critical }

data class GlobalStatusBanner(
    val title: String,
    val message: String,
    val severity: BannerSeverity,
)

data class ShellStatusModel(
    val sync: SyncStatusLabel,
    val signer: SignerStatusLabel,
    val banner: GlobalStatusBanner?,
)

fun deriveShellStatus(state: HarvestCircleShellState): ShellStatusModel {
    val sync =
        when (
            state.identity.snapshot.activeIdentity
                ?.relays
                ?.state
        ) {
            RelayConnectionState.Connected -> SyncStatusLabel.Available
            RelayConnectionState.Degraded -> SyncStatusLabel.Degraded
            RelayConnectionState.Error, RelayConnectionState.Disconnected -> SyncStatusLabel.Unavailable
            RelayConnectionState.Connecting, null -> SyncStatusLabel.NotYetObserved
        }
    val signer =
        when {
            state.session.readOnly -> SignerStatusLabel.ReadOnly
            state.identity.snapshot.activeIdentity == null -> SignerStatusLabel.SignedOut
            state.identity.snapshot.activeIdentity.identity.signer.availability == SignerAvailability.Available ->
                SignerStatusLabel.Available
            else -> SignerStatusLabel.CredentialMissing
        }
    return ShellStatusModel(sync, signer, deriveBanner(state, sync, signer))
}

private fun deriveBanner(
    state: HarvestCircleShellState,
    sync: SyncStatusLabel,
    signer: SignerStatusLabel,
): GlobalStatusBanner? =
    when {
        state.identity.lastProblem?.category == ApplicationErrorCategory.Storage ->
            GlobalStatusBanner("Local data needs attention", "Review the local runtime status before continuing.", BannerSeverity.Critical)
        state.identity.problem != null ->
            GlobalStatusBanner("Identity action could not complete", state.identity.problem, BannerSeverity.Caution)
        sync == SyncStatusLabel.Unavailable ->
            GlobalStatusBanner("Offline", "Public views may be out of date.", BannerSeverity.Caution)
        sync == SyncStatusLabel.Degraded ->
            GlobalStatusBanner("Limited connection", "Some configured services are unavailable.", BannerSeverity.Caution)
        signer == SignerStatusLabel.CredentialMissing ->
            GlobalStatusBanner("Signer unavailable", "Signing and private actions are unavailable.", BannerSeverity.Caution)
        signer == SignerStatusLabel.SignedOut ->
            GlobalStatusBanner("No active identity", "Public browsing remains available.", BannerSeverity.Information)
        signer == SignerStatusLabel.ReadOnly ->
            GlobalStatusBanner("Read-only", "Signing and private actions are unavailable.", BannerSeverity.Information)
        else -> null
    }
