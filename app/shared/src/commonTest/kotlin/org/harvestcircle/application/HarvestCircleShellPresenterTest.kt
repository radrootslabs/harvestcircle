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
    fun hcSc003HcSc004AdmittedConfirmationDispatchesExactEffectOnceAndClosesFromIdentityState() =
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
    fun hcSl006BusyRejectedConfirmationReturnsToReady() =
        runTest {
            val identityId = IdentityId.fromPublicKeyHex("04".repeat(32))
            val requestId = RemovalRequestId.from("removal-shell-busy")
            val identity =
                FakeIdentityPresentation(
                    presenterState(HarvestCircleRoute.IDENTITIES)
                        .withRemovalConfirmation(identityId, requestId)
                        .copy(busy = true, commandStatus = CommandStatus.RUNNING),
                )
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()
            val action = ConfirmationAction.RemoveLocalIdentity(identityId, requestId)

            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.Confirm(action)))
            assertEquals(
                ConfirmationPhase.Submitting,
                (presenter.state.value.overlays.current as FoundationOverlay.ConfirmAction).phase,
            )

            identity.state.value =
                identity.state.value.copy(
                    busy = true,
                    commandStatus = CommandStatus.REJECTED_BUSY,
                )
            runCurrent()

            assertEquals(
                ConfirmationPhase.Ready,
                (presenter.state.value.overlays.current as FoundationOverlay.ConfirmAction).phase,
            )
            presenter.close()
        }

    @Test
    fun replacedConfirmationRejectsThePriorTokenAndAdmitsOnlyTheCurrentToken() =
        runTest {
            val identityId = IdentityId.fromPublicKeyHex("04".repeat(32))
            val priorRequest = RemovalRequestId.from("removal-shell-prior")
            val currentRequest = RemovalRequestId.from("removal-shell-current")
            val identity =
                FakeIdentityPresentation(
                    presenterState(HarvestCircleRoute.IDENTITIES).withRemovalConfirmation(identityId, priorRequest),
                )
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            runCurrent()

            identity.state.value =
                identity.state.value.withRemovalConfirmation(identityId, currentRequest)
            runCurrent()
            assertEquals(
                ConfirmationAction.RemoveLocalIdentity(identityId, currentRequest),
                (presenter.state.value.overlays.current as FoundationOverlay.ConfirmAction).action,
            )

            presenter.dispatch(
                HarvestCircleShellIntent.Overlay(
                    OverlayIntent.Confirm(ConfirmationAction.RemoveLocalIdentity(identityId, priorRequest)),
                ),
            )
            assertTrue(identity.intents.isEmpty())

            presenter.dispatch(
                HarvestCircleShellIntent.Overlay(
                    OverlayIntent.Confirm(ConfirmationAction.RemoveLocalIdentity(identityId, currentRequest)),
                ),
            )
            assertEquals(
                listOf<HarvestCircleIntent>(HarvestCircleIntent.ConfirmIdentityRemoval(identityId, currentRequest)),
                identity.intents,
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
            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.OpenReference()))
            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.EditReference("note1candidate")))
            assertTrue(parser.inputs.isEmpty())
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
            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.OpenReference()))
            val edit = OverlayIntent.EditReference(privateReference)
            presenter.dispatch(HarvestCircleShellIntent.Overlay(edit))

            assertTrue(parser.inputs.isEmpty())
            assertEquals("EditReference(value=[REDACTED])", edit.toString())
            assertEquals(
                FoundationOverlay.OpenNostrReference("", ReferenceResult.PrivateKeyRejected),
                presenter.state.value.overlays.current,
            )
            assertTrue(privateReference !in presenter.state.value.toString())
            presenter.close()
        }

    @Test
    fun hcSl001AmbiguousHexNeverEntersStateOrParserOnEdit() =
        runTest {
            val hex = "0123456789abcdef".repeat(4)
            val identity = FakeIdentityPresentation(presenterState(HarvestCircleRoute.IDENTITIES))
            val parser = RecordingReferenceParser(NostrReferenceClassification.EventId, hex)
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this, parser)
            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.OpenReference()))

            listOf(hex, hex.uppercase(), "NoStR:$hex", " \t$hex\r\n").forEach { raw ->
                presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.EditReference(raw)))
                assertTrue(parser.inputs.isEmpty())
                assertEquals(
                    FoundationOverlay.OpenNostrReference("", ReferenceResult.AmbiguousHex),
                    presenter.state.value.overlays.current,
                )
                assertTrue(raw !in presenter.state.value.toString())
            }

            presenter.close()
        }

    @Test
    fun oversizedReferenceNeverEntersStateOrParser() =
        runTest {
            val identity = FakeIdentityPresentation(presenterState(HarvestCircleRoute.IDENTITIES))
            val parser = RecordingReferenceParser(NostrReferenceClassification.Invalid, null)
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this, parser)
            presenter.dispatch(HarvestCircleShellIntent.Overlay(OverlayIntent.OpenReference()))
            val oversized = "x".repeat(2 * 1024 * 1024)
            presenter.dispatch(
                HarvestCircleShellIntent.Overlay(
                    OverlayIntent.EditReference(oversized),
                ),
            )

            assertTrue(parser.inputs.isEmpty())
            assertTrue(oversized !in presenter.state.value.toString())
            assertEquals(
                FoundationOverlay.OpenNostrReference("", ReferenceResult.Invalid),
                presenter.state.value.overlays.current,
            )
            presenter.close()
        }

    @Test
    fun genericPrefilledReferenceOpenIsRejected() =
        runTest {
            val identity = FakeIdentityPresentation(presenterState(HarvestCircleRoute.IDENTITIES))
            val presenter = HarvestCircleShellPresenter(identity, BuildInfo.unknown(), this)
            presenter.dispatch(
                HarvestCircleShellIntent.Overlay(
                    OverlayIntent.Open(FoundationOverlay.OpenNostrReference("nsec1prefilled")),
                ),
            )

            assertNull(presenter.state.value.overlays.current)
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
