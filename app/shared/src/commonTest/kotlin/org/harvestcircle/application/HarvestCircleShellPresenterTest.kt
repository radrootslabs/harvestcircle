package org.harvestcircle.application

import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.harvestcircle.design.MotionPreference
import org.harvestcircle.design.ThemePreference
import org.harvestcircle.navigation.AppRoute
import org.harvestcircle.product.ScreenKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class HarvestCircleShellPresenterTest {
    @Test
    fun shellComposesRootNavigationAppearanceAndOverlays() =
        runTest {
            val identity = FakeIdentityPresentation(presenterState(HarvestCircleRoute.IDENTITIES))
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()
            assertTrue(presenter.state.value.root is ShellRoot.BootstrapCanvas)

            presenter.dispatch(HarvestCircleShellIntent.EnterReadOnly)
            presenter.dispatch(HarvestCircleShellIntent.Navigate(ScreenKey.Network))
            presenter.dispatch(HarvestCircleShellIntent.SetTheme(ThemePreference.Dark))
            presenter.dispatch(HarvestCircleShellIntent.SetMotion(MotionPreference.Reduced))
            presenter.dispatch(
                HarvestCircleShellIntent.Overlay(
                    OverlayIntent.Open(FoundationOverlay.Status(StatusOverlayKey.Sync)),
                ),
            )

            assertEquals(AppRoute.Network, presenter.state.value.currentRoute)
            assertEquals(ThemePreference.Dark, presenter.state.value.appearance.theme)
            assertEquals(FoundationOverlay.Status(StatusOverlayKey.Sync), presenter.state.value.overlays.current)
            presenter.close()
        }

    @Test
    fun identityCommandsRemainOwnedByTheExistingPresenter() =
        runTest {
            val identity = FakeIdentityPresentation(presenterState(HarvestCircleRoute.IDENTITIES))
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            presenter.dispatch(HarvestCircleShellIntent.Identity(HarvestCircleIntent.ChooseCreateIdentity))
            assertEquals(1, identity.intents.size)
            assertTrue(identity.intents.single() === HarvestCircleIntent.ChooseCreateIdentity)
            presenter.close()
        }

    @Test
    fun admittedConfirmationDispatchesExactIdentityEffectOnceAndClosesFromIdentityState() =
        runTest {
            val identityId = IdentityId.fromPublicKeyHex("03".repeat(32))
            val requestId = RemovalRequestId.from("removal-shell-1")
            val identity =
                FakeIdentityPresentation(
                    presenterState(HarvestCircleRoute.IDENTITIES).withRemovalConfirmation(identityId, requestId),
                )
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()
            val confirmation = presenter.state.value.overlays.current as FoundationOverlay.ConfirmAction
            val action = ConfirmationAction.RemoveLocalIdentity(identityId, requestId)
            assertEquals(action, confirmation.action)

            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.Confirm(action)))
            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.Confirm(action)))

            assertEquals(
                listOf<HarvestCircleIntent>(HarvestCircleIntent.ConfirmIdentityRemoval(identityId, requestId)),
                identity.intents,
            )
            assertEquals(
                ConfirmationPhase.Submitting,
                (presenter.state.value.overlays.current as FoundationOverlay.ConfirmAction).phase,
            )

            identity.state.value =
                identity.state.value.copy(
                    removalConfirmation = null,
                    removalStatus = RemovalStatus.CONFIRMING,
                    busy = true,
                    commandStatus = CommandStatus.RUNNING,
                )
            runCurrent()
            assertNull(presenter.state.value.overlays.current)
            presenter.close()
        }

    @Test
    fun dismissalIsRequestBoundAndStaleActionsEmitNothing() =
        runTest {
            val identityId = IdentityId.fromPublicKeyHex("04".repeat(32))
            val requestId = RemovalRequestId.from("removal-shell-2")
            val identity =
                FakeIdentityPresentation(
                    presenterState(HarvestCircleRoute.IDENTITIES).withRemovalConfirmation(identityId, requestId),
                )
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()
            val stale = ConfirmationAction.RemoveLocalIdentity(identityId, RemovalRequestId.from("stale-shell"))
            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.DismissConfirmation(stale)))
            assertTrue(identity.intents.isEmpty())

            val exact = ConfirmationAction.RemoveLocalIdentity(identityId, requestId)
            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.DismissConfirmation(exact)))
            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.DismissConfirmation(exact)))

            assertEquals(
                listOf<HarvestCircleIntent>(HarvestCircleIntent.CancelIdentityRemoval(identityId, requestId)),
                identity.intents,
            )
            assertEquals(
                ConfirmationPhase.Dismissing,
                (presenter.state.value.overlays.current as FoundationOverlay.ConfirmAction).phase,
            )
            presenter.close()
        }

    @Test
    fun usableDegradationPreservesDashboardNavigation() =
        runTest {
            val identity = FakeIdentityPresentation(activePresenterState(ApplicationLifecycle.Ready, null, 1UL))
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()
            presenter.dispatch(HarvestCircleShellIntent.Navigate(ScreenKey.Network))

            identity.state.value =
                activePresenterState(
                    ApplicationLifecycle.Degraded,
                    problem(ApplicationErrorCategory.Network),
                    2UL,
                )
            runCurrent()

            assertEquals(AppRoute.Network, presenter.state.value.currentRoute)
            assertEquals(
                LocalUsability.UsableDegraded(DegradationReason.Network),
                presenter.state.value.localUsability,
            )
            presenter.close()
        }

    @Test
    fun storageDegradationLeavesTheDashboardForALifecycleCanvas() =
        runTest {
            val identity = FakeIdentityPresentation(activePresenterState(ApplicationLifecycle.Ready, null, 1UL))
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()

            identity.state.value =
                activePresenterState(
                    ApplicationLifecycle.Degraded,
                    problem(ApplicationErrorCategory.Storage),
                    2UL,
                )
            runCurrent()

            assertEquals(LocalUsability.Unusable, presenter.state.value.localUsability)
            assertTrue(presenter.state.value.root is ShellRoot.LifecycleCanvas)
            presenter.close()
        }

    @Test
    fun nativeReferenceClassificationControlsTheSyntaxOnlyResult() =
        runTest {
            val identity = FakeIdentityPresentation(presenterState(HarvestCircleRoute.IDENTITIES))
            val parser = RecordingReferenceParser(NostrReferenceClassification.Note, "note1canonical")
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this, parser)
            presenter.dispatch(
                HarvestCircleShellIntent.Overlay(
                    OverlayIntent.Open(FoundationOverlay.OpenNostrReference("note1candidate")),
                ),
            )
            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.SubmitReference))

            assertEquals(listOf("note1candidate"), parser.inputs)
            assertEquals(
                FoundationOverlay.OpenNostrReference("note1candidate", ReferenceResult.Unsupported),
                presenter.state.value.overlays.current,
            )
            presenter.close()
        }

    @Test
    fun privateKeyReferencesAreRejectedAndRemovedFromShellState() =
        runTest {
            val privateReference = "nsec1private"
            val identity = FakeIdentityPresentation(presenterState(HarvestCircleRoute.IDENTITIES))
            val parser = RecordingReferenceParser(NostrReferenceClassification.PrivateKeyRejected, null)
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this, parser)
            presenter.dispatch(
                HarvestCircleShellIntent.Overlay(OverlayIntent.Open(FoundationOverlay.OpenNostrReference())),
            )
            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.EditReference(privateReference)))

            assertEquals(listOf(privateReference), parser.inputs)
            assertEquals(
                FoundationOverlay.OpenNostrReference("", ReferenceResult.PrivateKeyRejected),
                presenter.state.value.overlays.current,
            )
            assertTrue(privateReference !in presenter.state.value.toString())
            presenter.close()
        }
}

private class RecordingReferenceParser(
    private val classification: NostrReferenceClassification,
    private val canonicalReference: String?,
) : NostrReferenceParser {
    val inputs = mutableListOf<String>()

    override fun parse(raw: String): NostrReferenceParseResult {
        inputs += raw
        return NostrReferenceParseResult(classification, canonicalReference)
    }
}

private class FakeIdentityPresentation(
    initial: HarvestCirclePresenterState,
) : IdentityPresentationPort {
    override val state = MutableStateFlow(initial)
    val intents = mutableListOf<HarvestCircleIntent>()

    override fun dispatch(intent: HarvestCircleIntent) {
        intents += intent
    }
}

private fun presenterState(route: HarvestCircleRoute): HarvestCirclePresenterState =
    HarvestCirclePresenterState(
        snapshot =
            ApplicationSnapshot(
                revision = SnapshotRevision(1UL),
                lifecycle = ApplicationLifecycle.Ready,
                lifecycleProblem = null,
                configuredRelays = emptyList(),
                identities = emptyList(),
                selectedIdentityId = null,
                session = SessionLifecycle.SignedOut,
                sessionSubjectIdentityId = null,
                sessionProblem = null,
                activeIdentity = null,
                recoverableProblem = null,
            ),
        route = route,
    )

private fun HarvestCirclePresenterState.withRemovalConfirmation(
    identityId: IdentityId,
    requestId: RemovalRequestId,
): HarvestCirclePresenterState =
    copy(
        removalConfirmation =
            IdentityRemovalConfirmation(
                identityId = identityId,
                requestId = requestId,
                deletesLocalCredential = true,
                signsOut = false,
                expiresAt = UnixSeconds(60),
            ),
        removalStatus = RemovalStatus.AWAITING_CONFIRMATION,
    )

private fun activePresenterState(
    lifecycle: ApplicationLifecycle,
    lifecycleProblem: ApplicationProblem?,
    revision: ULong,
): HarvestCirclePresenterState {
    val identity =
        IdentitySummary(
            id = IdentityId.fromPublicKeyHex("02".repeat(32)),
            npub = "npub1active",
            displayLabel = "Active identity",
            signer = SignerBindingSummary(SignerBindingKind.LocalKeyring, SignerAvailability.Available),
            createdAt = UnixSeconds(1),
            lastUsedAt = null,
        )
    return HarvestCirclePresenterState(
        ApplicationSnapshot(
            revision = SnapshotRevision(revision),
            lifecycle = lifecycle,
            lifecycleProblem = lifecycleProblem,
            configuredRelays = emptyList(),
            identities = listOf(identity),
            selectedIdentityId = identity.id,
            session = SessionLifecycle.Active,
            sessionSubjectIdentityId = identity.id,
            sessionProblem = null,
            activeIdentity =
                ActiveIdentity(
                    identity = identity,
                    relays = RelaySummary(emptyList(), RelayConnectionState.Degraded),
                    profileState = ProfileLoadState.Cached,
                    profile = null,
                ),
            recoverableProblem = null,
        ),
    )
}

private fun problem(category: ApplicationErrorCategory): ApplicationProblem =
    ApplicationProblem(
        code =
            if (category == ApplicationErrorCategory.Network) {
                ApplicationErrorCode.RelayConnectionFailed
            } else {
                ApplicationErrorCode.StorageUnavailable
            },
        category = category,
        retryable = true,
        recoveryAction = RecoveryAction.Retry,
        operationId = null,
        safeMessage = "Typed degraded problem.",
    )
