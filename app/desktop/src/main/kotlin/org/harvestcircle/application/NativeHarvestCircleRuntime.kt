package org.harvestcircle.application

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.selects.select
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import org.harvestcircle.ffi.AppSnapshotDto
import org.harvestcircle.ffi.GeneratedRecoveryRequest
import org.harvestcircle.ffi.HarvestCircleAppCore
import org.harvestcircle.ffi.HarvestCircleChangeObserver
import org.harvestcircle.ffi.HubContextDto
import org.harvestcircle.ffi.HubDepartureInputDto
import org.harvestcircle.ffi.HubDepartureReceiptDto
import org.harvestcircle.ffi.HubDiscrepancyPageDto
import org.harvestcircle.ffi.HubDispatchInputDto
import org.harvestcircle.ffi.HubDispatchPageDto
import org.harvestcircle.ffi.HubDispatchQueryDto
import org.harvestcircle.ffi.HubDispatchReceiptDto
import org.harvestcircle.ffi.HubDispatchViewDto
import org.harvestcircle.ffi.HubGroupViewDto
import org.harvestcircle.ffi.HubHandlingInputDto
import org.harvestcircle.ffi.HubHandlingQueryDto
import org.harvestcircle.ffi.HubHandlingReceiptDto
import org.harvestcircle.ffi.HubLineagePageDto
import org.harvestcircle.ffi.HubLineageQueryDto
import org.harvestcircle.ffi.HubReceiptInputDto
import org.harvestcircle.ffi.HubReferenceEditDto
import org.harvestcircle.ffi.HubReferencePageDto
import org.harvestcircle.ffi.HubReferenceQueryDto
import org.harvestcircle.ffi.HubReferenceReceiptDto
import org.harvestcircle.ffi.HubStockPageDto
import org.harvestcircle.ffi.HubStockQueryDto
import org.harvestcircle.ffi.HubStockReceiptDto
import org.harvestcircle.ffi.HubTreatmentPageDto
import org.harvestcircle.ffi.HubTreatmentQueryDto
import org.harvestcircle.ffi.HubTreatmentScopeDto
import org.harvestcircle.ffi.HubTreatmentSnapshotDto
import org.harvestcircle.ffi.HubWorkPageDto
import org.harvestcircle.ffi.IdentityCommandReceiptDto
import org.harvestcircle.ffi.IdentityDto
import org.harvestcircle.ffi.ObserverSubscription
import org.harvestcircle.ffi.RelayBootstrapInputDto
import org.harvestcircle.ffi.RelayDestinationDto
import org.harvestcircle.ffi.RelayEndpointDto
import org.harvestcircle.ffi.RemovalRequest
import org.harvestcircle.ffi.RequestContextDto
import org.harvestcircle.ffi.RuntimeOpenInputDto
import org.harvestcircle.ffi.ShutdownReceiptDto
import org.harvestcircle.ffi.SnapshotChangeDto
import org.harvestcircle.ffi.compatibilityDescriptor
import org.harvestcircle.ffi.generateOperationIdV7
import java.util.concurrent.atomic.AtomicBoolean
import org.harvestcircle.ffi.buildInfo as nativeBuildInfo

class NativeHarvestCircleRuntime internal constructor(
    private val native: NativeCorePort,
    private val handleIds: NativeHandleIdSource = GeneratedNativeHandleIdSource,
    override val buildInfo: BuildInfo = nativeBuildInfo().toBuildInfo(compatibilityDescriptor()),
    override val demoProfile: Boolean = false,
) : HarvestCircleRuntime,
    HarvestCircleHubRuntime,
    HarvestCircleReferenceRuntime,
    HarvestCircleStockRuntime,
    HarvestCircleHandlingRuntime,
    HarvestCircleDispatchRuntime,
    HarvestCircleDepartureRuntime {
    override suspend fun applyDeparture(
        context: RequestContext,
        input: HubDepartureInput,
    ): HubDepartureReceipt = callNative(context.operationId) { hubPort().applyDeparture(context.toNative(), input.toNative()).toModel() }

    override suspend fun departureOperation(operation: OperationId): HubDepartureReceipt? =
        callNative(operation) { hubPort().departureOperation(operation.value)?.toModel() }

    override suspend fun dispatchPage(query: HubDispatchQuery): HubDispatchPage =
        callNative { hubPort().dispatchPage(query.toNative()).toModel() }

    override suspend fun dispatchRecord(id: String): HubDispatchView = callNative { hubPort().dispatchRecord(id).toModel() }

    override suspend fun applyDispatch(
        context: RequestContext,
        input: HubDispatchInput,
    ): HubDispatchReceipt =
        callNative(context.operationId) {
            hubPort().applyDispatch(context.toNative(), input.toNative()).toModel()
        }

    override suspend fun dispatchOperation(operation: OperationId): HubDispatchReceipt? =
        callNative(operation) {
            hubPort().dispatchOperation(operation.value)?.toModel()
        }

    override suspend fun treatment(scope: HubTreatmentScope): HubTreatmentSnapshot =
        callNative { hubPort().treatment(scope.toNative()).toModel() }

    override suspend fun treatmentHistory(query: HubTreatmentQuery): HubTreatmentPage =
        callNative {
            hubPort().treatmentHistory(query.toNative()).toModel()
        }

    override suspend fun lineage(query: HubLineageQuery): HubLineagePage = callNative { hubPort().lineage(query.toNative()).toModel() }

    override suspend fun group(id: String): HubGroupView = callNative { hubPort().group(id).toModel() }

    override suspend fun work(query: HubHandlingQuery): HubWorkPage = callNative { hubPort().work(query.toNative()).toModel() }

    override suspend fun discrepancies(query: HubHandlingQuery): HubDiscrepancyPage =
        callNative {
            hubPort().discrepancies(query.toNative()).toModel()
        }

    override suspend fun applyHandling(
        context: RequestContext,
        input: HubHandlingInput,
    ): HubHandlingReceipt = callNative(context.operationId) { hubPort().applyHandling(context.toNative(), input.toNative()).toModel() }

    override suspend fun handlingOperation(operation: OperationId): HubHandlingReceipt? =
        callNative(operation) { hubPort().handlingOperation(operation.value)?.toModel() }

    override suspend fun stock(query: HubStockQuery): HubStockPage = callNative { hubPort().stock(query.toNative()).toModel() }

    override suspend fun receiveStock(
        context: RequestContext,
        input: HubReceiptInput,
    ): HubStockReceipt = callNative(context.operationId) { hubPort().receiveStock(context.toNative(), input.toNative()).toModel() }

    override suspend fun receiptOperation(operation: OperationId): HubStockReceipt? =
        callNative(operation) { hubPort().receiptOperation(operation.value)?.toModel() }

    override suspend fun hubContext(): HubContext? = callNative { hubPort().hubContext()?.toHubContext() }

    override suspend fun initializeHub(context: RequestContext): HubContext =
        callNative(context.operationId) { hubPort().initializeHub(context.toNative()).toHubContext() }

    override suspend fun references(query: HubReferenceQuery): HubReferencePage =
        callNative {
            hubPort().references(query.toNative()).toModel()
        }

    override suspend fun editReference(
        context: RequestContext,
        edit: HubReferenceEdit,
    ): HubReferenceReceipt = callNative(context.operationId) { hubPort().editReference(context.toNative(), edit.toNative()).toModel() }

    override suspend fun referenceOperation(operation: OperationId): HubReferenceReceipt? =
        callNative(operation) { hubPort().referenceOperation(operation.value)?.toModel() }

    override suspend fun referenceHistory(
        id: String,
        afterRevision: ULong,
        limit: UInt,
    ): List<HubReferenceReceipt> = callNative { hubPort().referenceHistory(id, afterRevision, limit).map { it.toModel() } }

    private fun hubPort(): NativeHubPort = checkNotNull(native as? NativeHubPort) { "Hub runtime is unavailable" }

    private val recoveryMutex = Mutex()
    private val recoveryHandles = mutableMapOf<RecoveryRequestId, NativeGeneratedRecoveryHandle>()
    private val removalMutex = Mutex()
    private val removalHandles = mutableMapOf<RemovalRequestId, NativeRemovalHandle>()
    private val shutdownMutex = Mutex()
    private var shutdownReceipt: ShutdownReceipt? = null

    override suspend fun bootstrap(): ApplicationSnapshot = callNative { native.bootstrap().toApplicationSnapshot() }

    override fun currentSnapshot(): ApplicationSnapshot =
        try {
            native.snapshot().toApplicationSnapshot()
        } catch (error: ApplicationFailure) {
            throw error
        } catch (error: Exception) {
            throw ApplicationFailure(error.toApplicationProblem())
        }

    override fun changes(): Flow<ApplicationChange> =
        flow {
            val acceptingCallbacks = AtomicBoolean(true)
            val deliveryFailure = CompletableDeferred<ApplicationFailure>()
            val initialChange = CompletableDeferred<ApplicationChange>()
            val latestChanges = Channel<ApplicationChange>(Channel.CONFLATED)
            var subscription: NativeSubscriptionHandle? = null
            try {
                withContext(NonCancellable) {
                    subscription =
                        callNative {
                            native.subscribe { change ->
                                if (!acceptingCallbacks.get()) return@subscribe
                                val mapped =
                                    try {
                                        change.toApplicationChange()
                                    } catch (_: Exception) {
                                        deliveryFailure.complete(observerDeliveryFailure())
                                        return@subscribe
                                    }
                                if (initialChange.complete(mapped)) return@subscribe
                                if (latestChanges.trySend(mapped).isFailure && acceptingCallbacks.get()) {
                                    deliveryFailure.complete(observerDeliveryFailure())
                                }
                            }
                        }
                }
                val initial =
                    select<ApplicationChange> {
                        deliveryFailure.onAwait { throw it }
                        initialChange.onAwait { it }
                    }
                emit(initial)
                while (true) {
                    val latest =
                        select<ApplicationChange> {
                            deliveryFailure.onAwait { throw it }
                            latestChanges.onReceive { it }
                        }
                    emit(latest)
                }
            } finally {
                acceptingCallbacks.set(false)
                latestChanges.close()
                withContext(NonCancellable) { subscription?.unsubscribe() }
            }
        }

    override suspend fun execute(command: ApplicationCommand): ApplicationCommandResult =
        when (command) {
            is ApplicationCommand.AcknowledgeGeneratedIdentity -> acknowledgeGeneratedIdentity(command)
            is ApplicationCommand.CancelGeneratedIdentity -> cancelGeneratedIdentity(command)
            is ApplicationCommand.ImportLocalIdentity -> importLocalIdentity(command)
            is ApplicationCommand.SelectIdentity -> updated { native.selectIdentity(command.identityId.value) }
            is ApplicationCommand.ActivateIdentity -> updated { native.activateIdentity(command.identityId.value) }
            ApplicationCommand.SignOut -> updated { native.signOut() }
            ApplicationCommand.RefreshActiveProfile -> updated { native.refreshActiveProfile() }
            is ApplicationCommand.ConfirmIdentityRemoval -> confirmIdentityRemoval(command)
        }

    override suspend fun prepareLocalIdentity(): GeneratedIdentityRecovery {
        val handle = callNative { native.beginGeneratedIdentity() }
        val requestId = RecoveryRequestId.from(handleIds.next("recovery"))
        return try {
            val identity = handle.identity().toIdentitySummary()
            val backup = GeneratedKeyBackup(identity.npub, handle.takeRecoverySecret())
            recoveryMutex.withLock { recoveryHandles[requestId] = handle }
            GeneratedIdentityRecovery(
                requestId = requestId,
                identity = identity,
                expiresAt = UnixSeconds(handle.expiresAtSeconds()),
                backup = backup,
            )
        } catch (error: CancellationException) {
            handle.close()
            throw error
        } catch (error: ApplicationFailure) {
            handle.close()
            throw error
        } catch (error: Exception) {
            handle.close()
            throw ApplicationFailure(error.toApplicationProblem())
        }
    }

    override suspend fun requestIdentityRemoval(identityId: IdentityId): IdentityRemovalRequest {
        val handle = callNative { native.requestIdentityRemoval(identityId.value) }
        val requestId = RemovalRequestId.from(handleIds.next("removal"))
        return try {
            val requestedIdentity = IdentityId.fromPublicKeyHex(handle.publicKeyHex())
            check(requestedIdentity == identityId) { "Native removal identity does not match the request" }
            removalMutex.withLock { removalHandles[requestId] = handle }
            IdentityRemovalRequest(
                requestId = requestId,
                identityId = requestedIdentity,
                deletesLocalCredential = handle.deletesLocalCredential(),
                signsOut = handle.signsOut(),
                expiresAt = UnixSeconds(handle.expiresAtSeconds()),
            )
        } catch (error: CancellationException) {
            handle.close()
            throw error
        } catch (error: ApplicationFailure) {
            handle.close()
            throw error
        } catch (error: Exception) {
            handle.close()
            throw ApplicationFailure(error.toApplicationProblem())
        }
    }

    override suspend fun cancelIdentityRemoval(requestId: RemovalRequestId): Boolean {
        val handle = removalMutex.withLock { removalHandles.remove(requestId) } ?: return false
        handle.close()
        return true
    }

    override suspend fun shutdown(): ShutdownReceipt =
        shutdownMutex.withLock {
            shutdownReceipt?.let { return@withLock it }
            closeOutstandingHandles()
            callNative { native.shutdown().toShutdownReceipt() }.also { receipt ->
                if (!receipt.closed) throw incompleteShutdown()
                native.close()
                shutdownReceipt = receipt
            }
        }

    private suspend fun acknowledgeGeneratedIdentity(command: ApplicationCommand.AcknowledgeGeneratedIdentity): ApplicationCommandResult {
        val handle =
            recoveryMutex.withLock { recoveryHandles.remove(command.requestId) }
                ?: throw missingHandle("generated identity recovery", command.context.operationId)
        return try {
            updated(command.context.operationId) {
                native.acknowledgeGeneratedIdentity(command.context.toNative(), handle)
            }
        } finally {
            handle.close()
        }
    }

    private suspend fun cancelGeneratedIdentity(command: ApplicationCommand.CancelGeneratedIdentity): ApplicationCommandResult {
        val handle =
            recoveryMutex.withLock { recoveryHandles.remove(command.requestId) }
                ?: throw missingHandle("generated identity recovery")
        return try {
            callNative { check(native.cancelGeneratedIdentity(handle)) { "Native recovery cancellation was incomplete" } }
            ApplicationCommandResult.Updated(currentSnapshot())
        } finally {
            handle.close()
        }
    }

    private suspend fun importLocalIdentity(command: ApplicationCommand.ImportLocalIdentity): ApplicationCommandResult {
        val bytes = command.secretKey.take().encodeToByteArray()
        return try {
            callNative(command.context.operationId) {
                native.importIdentity(command.context.toNative(), bytes).toApplicationResult()
            }
        } finally {
            bytes.fill(0)
            command.secretKey.clear()
        }
    }

    private suspend fun confirmIdentityRemoval(command: ApplicationCommand.ConfirmIdentityRemoval): ApplicationCommandResult {
        val handle =
            removalMutex.withLock { removalHandles.remove(command.requestId) }
                ?: throw missingHandle("identity removal", command.context.operationId)
        return try {
            updated(command.context.operationId) {
                native.confirmIdentityRemoval(command.context.toNative(), handle)
            }
        } finally {
            handle.close()
        }
    }

    private suspend fun updated(
        operationId: OperationId? = null,
        operation: suspend () -> AppSnapshotDto,
    ): ApplicationCommandResult = callNative(operationId) { ApplicationCommandResult.Updated(operation().toApplicationSnapshot()) }

    private suspend fun closeOutstandingHandles() {
        recoveryMutex.withLock {
            recoveryHandles.values.forEach(NativeGeneratedRecoveryHandle::close)
            recoveryHandles.clear()
        }
        removalMutex.withLock {
            removalHandles.values.forEach(NativeRemovalHandle::close)
            removalHandles.clear()
        }
    }

    private fun missingHandle(
        kind: String,
        operationId: OperationId? = null,
    ): ApplicationFailure =
        ApplicationFailure(
            ApplicationProblem(
                code = ApplicationErrorCode.InvalidApplicationState,
                category = ApplicationErrorCategory.Lifecycle,
                retryable = false,
                recoveryAction = RecoveryAction.None,
                operationId = operationId,
                safeMessage = "The $kind request is no longer available.",
            ),
        )

    private fun incompleteShutdown(): ApplicationFailure =
        ApplicationFailure(
            ApplicationProblem(
                code = ApplicationErrorCode.InvalidApplicationState,
                category = ApplicationErrorCategory.Lifecycle,
                retryable = false,
                recoveryAction = RecoveryAction.None,
                operationId = null,
                safeMessage = "The native runtime did not complete shutdown.",
            ),
        )

    private fun observerDeliveryFailure(): ApplicationFailure =
        ApplicationFailure(
            ApplicationProblem(
                code = ApplicationErrorCode.ObserverRegistrationFailed,
                category = ApplicationErrorCategory.Lifecycle,
                retryable = false,
                recoveryAction = RecoveryAction.RestartApplication,
                operationId = null,
                safeMessage = "Application updates could not be delivered.",
            ),
        )

    private suspend fun <T> callNative(
        fallbackOperationId: OperationId? = null,
        operation: suspend () -> T,
    ): T =
        try {
            operation()
        } catch (error: CancellationException) {
            throw error
        } catch (error: ApplicationFailure) {
            throw error
        } catch (error: Exception) {
            throw ApplicationFailure(error.toApplicationProblem(fallbackOperationId))
        }

    companion object {
        internal fun openDemo(runName: String): NativeHarvestCircleRuntime {
            val expectation = verifyNativeCompatibility(compatibilityDescriptor())
            return NativeHarvestCircleRuntime(
                UniFfiNativeCorePort(HarvestCircleAppCore.openDemoCompatible(expectation, runName)),
                buildInfo = nativeBuildInfo().toBuildInfo(compatibilityDescriptor()),
                demoProfile = true,
            )
        }

        internal fun open(configuration: DesktopRuntimeOpenConfiguration): NativeHarvestCircleRuntime {
            val expectation = verifyNativeCompatibility(compatibilityDescriptor())
            val build = nativeBuildInfo().toBuildInfo(compatibilityDescriptor())
            return NativeHarvestCircleRuntime(
                UniFfiNativeCorePort(HarvestCircleAppCore.openCompatible(expectation, configuration.toNative())),
                buildInfo = build,
            )
        }
    }
}

internal const val HARVESTCIRCLE_DEVELOPMENT_DATA_DIR_ENVIRONMENT = "HARVESTCIRCLE_DEVELOPMENT_DATA_DIR"
internal const val HARVESTCIRCLE_NOSTR_RELAYS_ENVIRONMENT = "HARVESTCIRCLE_NOSTR_RELAYS"
internal const val HARVESTCIRCLE_LOCAL_DEVELOPMENT_RELAY = "local|ws://localhost:8080"

internal data class DesktopRuntimeOpenConfiguration(
    val developmentMode: Boolean,
    val explicitDataDirectory: String?,
    val relayInput: RelayBootstrapInputDto,
) {
    init {
        require(developmentMode || explicitDataDirectory == null) {
            "An explicit data directory is available only in development mode"
        }
    }

    fun toNative(): RuntimeOpenInputDto =
        RuntimeOpenInputDto(
            developmentMode = developmentMode,
            explicitDataDirectory = explicitDataDirectory,
            relayInput = relayInput,
        )
}

internal fun desktopRuntimeOpenConfiguration(
    developmentMode: Boolean,
    explicitDataDirectory: String? =
        if (developmentMode) System.getenv(HARVESTCIRCLE_DEVELOPMENT_DATA_DIR_ENVIRONMENT) else null,
    configuredRelays: String? = System.getenv(HARVESTCIRCLE_NOSTR_RELAYS_ENVIRONMENT),
): DesktopRuntimeOpenConfiguration =
    DesktopRuntimeOpenConfiguration(
        developmentMode = developmentMode,
        explicitDataDirectory = explicitDataDirectory,
        relayInput = desktopRelayBootstrapInput(developmentMode, configuredRelays),
    )

internal fun desktopRelayBootstrapInput(
    developmentMode: Boolean,
    configuredValue: String? = System.getenv(HARVESTCIRCLE_NOSTR_RELAYS_ENVIRONMENT),
): RelayBootstrapInputDto {
    val configured = configuredValue?.trim().orEmpty()
    val entries =
        when {
            configured.isNotEmpty() -> configured.split(',').map(String::trim)
            developmentMode -> listOf(HARVESTCIRCLE_LOCAL_DEVELOPMENT_RELAY)
            else -> emptyList()
        }
    val endpoints =
        try {
            entries.map(::parseRelayEndpoint)
        } catch (_: IllegalArgumentException) {
            throw ApplicationFailure(
                ApplicationProblem(
                    ApplicationErrorCode.InvalidRelayConfiguration,
                    ApplicationErrorCategory.Network,
                    false,
                    RecoveryAction.CheckConfiguration,
                    null,
                    "The relay configuration is invalid. Check the configured relay destinations and restart HarvestCircle.",
                ),
            )
        }
    return RelayBootstrapInputDto(endpoints)
}

private fun parseRelayEndpoint(value: String): RelayEndpointDto {
    val parts = value.split('|', limit = 2)
    require(parts.size == 2 && parts.all(String::isNotBlank)) {
        "Relay entries must include an explicit destination classification"
    }
    val destination =
        when (parts[0].trim()) {
            "local" -> RelayDestinationDto.LOCAL
            "private" -> RelayDestinationDto.PRIVATE_NETWORK
            "public" -> RelayDestinationDto.PUBLIC
            else -> throw IllegalArgumentException("Relay destination classification is invalid")
        }
    return RelayEndpointDto(
        url = parts[1].trim(),
        destination = destination,
        read = true,
        write = true,
    )
}

internal fun interface NativeHandleIdSource {
    fun next(kind: String): String
}

private object GeneratedNativeHandleIdSource : NativeHandleIdSource {
    override fun next(kind: String): String = "$kind:${generateOperationIdV7()}"
}

internal interface NativeCorePort : AutoCloseable {
    fun snapshot(): AppSnapshotDto

    suspend fun bootstrap(): AppSnapshotDto

    suspend fun subscribe(onChange: (SnapshotChangeDto) -> Unit): NativeSubscriptionHandle

    suspend fun beginGeneratedIdentity(): NativeGeneratedRecoveryHandle

    suspend fun acknowledgeGeneratedIdentity(
        context: RequestContextDto,
        request: NativeGeneratedRecoveryHandle,
    ): AppSnapshotDto

    suspend fun cancelGeneratedIdentity(request: NativeGeneratedRecoveryHandle): Boolean

    suspend fun importIdentity(
        context: RequestContextDto,
        secretKey: ByteArray,
    ): IdentityCommandReceiptDto

    suspend fun selectIdentity(publicKeyHex: String): AppSnapshotDto

    suspend fun activateIdentity(publicKeyHex: String): AppSnapshotDto

    suspend fun signOut(): AppSnapshotDto

    suspend fun refreshActiveProfile(): AppSnapshotDto

    suspend fun requestIdentityRemoval(publicKeyHex: String): NativeRemovalHandle

    suspend fun confirmIdentityRemoval(
        context: RequestContextDto,
        request: NativeRemovalHandle,
    ): AppSnapshotDto

    suspend fun shutdown(): ShutdownReceiptDto
}

internal interface NativeGeneratedRecoveryHandle : AutoCloseable {
    fun identity(): IdentityDto

    fun expiresAtSeconds(): Long

    fun takeRecoverySecret(): String
}

internal interface NativeRemovalHandle : AutoCloseable {
    fun publicKeyHex(): String

    fun deletesLocalCredential(): Boolean

    fun signsOut(): Boolean

    fun expiresAtSeconds(): Long
}

internal fun interface NativeSubscriptionHandle {
    suspend fun unsubscribe()
}

private interface NativeHubPort {
    suspend fun applyDeparture(
        context: RequestContextDto,
        input: HubDepartureInputDto,
    ): HubDepartureReceiptDto

    suspend fun departureOperation(operation: String): HubDepartureReceiptDto?

    suspend fun dispatchPage(query: HubDispatchQueryDto): HubDispatchPageDto

    suspend fun dispatchRecord(id: String): HubDispatchViewDto

    suspend fun applyDispatch(
        context: RequestContextDto,
        input: HubDispatchInputDto,
    ): HubDispatchReceiptDto

    suspend fun dispatchOperation(operation: String): HubDispatchReceiptDto?

    suspend fun treatment(scope: HubTreatmentScopeDto): HubTreatmentSnapshotDto

    suspend fun treatmentHistory(query: HubTreatmentQueryDto): HubTreatmentPageDto

    suspend fun lineage(query: HubLineageQueryDto): HubLineagePageDto

    suspend fun group(id: String): HubGroupViewDto

    suspend fun work(query: HubHandlingQueryDto): HubWorkPageDto

    suspend fun discrepancies(query: HubHandlingQueryDto): HubDiscrepancyPageDto

    suspend fun applyHandling(
        context: RequestContextDto,
        input: HubHandlingInputDto,
    ): HubHandlingReceiptDto

    suspend fun handlingOperation(operation: String): HubHandlingReceiptDto?

    suspend fun stock(query: HubStockQueryDto): HubStockPageDto

    suspend fun receiveStock(
        context: RequestContextDto,
        input: HubReceiptInputDto,
    ): HubStockReceiptDto

    suspend fun receiptOperation(operation: String): HubStockReceiptDto?

    suspend fun references(query: HubReferenceQueryDto): HubReferencePageDto

    suspend fun editReference(
        context: RequestContextDto,
        edit: HubReferenceEditDto,
    ): HubReferenceReceiptDto

    suspend fun referenceOperation(operation: String): HubReferenceReceiptDto?

    suspend fun referenceHistory(
        id: String,
        after: ULong,
        limit: UInt,
    ): List<HubReferenceReceiptDto>

    suspend fun hubContext(): HubContextDto?

    suspend fun initializeHub(context: RequestContextDto): HubContextDto
}

private fun HubContextDto.toHubContext(): HubContext = HubContext(hubId, IdentityId.fromPublicKeyHex(ownerPublicKey), revision, sampleData)

private class UniFfiNativeCorePort(
    private val core: HarvestCircleAppCore,
) : NativeCorePort,
    NativeHubPort {
    override suspend fun applyDeparture(
        context: RequestContextDto,
        input: HubDepartureInputDto,
    ): HubDepartureReceiptDto = core.applyHubDeparture(context, input)

    override suspend fun departureOperation(operation: String): HubDepartureReceiptDto? = core.hubDepartureOperation(operation)

    override suspend fun dispatchPage(query: HubDispatchQueryDto): HubDispatchPageDto = core.hubDispatchPage(query)

    override suspend fun dispatchRecord(id: String): HubDispatchViewDto = core.hubDispatchRecord(id)

    override suspend fun applyDispatch(
        context: RequestContextDto,
        input: HubDispatchInputDto,
    ): HubDispatchReceiptDto = core.applyHubDispatch(context, input)

    override suspend fun dispatchOperation(operation: String): HubDispatchReceiptDto? = core.hubDispatchOperation(operation)

    override suspend fun treatment(scope: HubTreatmentScopeDto): HubTreatmentSnapshotDto = core.hubTreatment(scope)

    override suspend fun treatmentHistory(query: HubTreatmentQueryDto): HubTreatmentPageDto = core.hubTreatmentHistory(query)

    override suspend fun lineage(query: HubLineageQueryDto): HubLineagePageDto = core.hubLineage(query)

    override suspend fun group(id: String): HubGroupViewDto = core.hubGroup(id)

    override suspend fun work(query: HubHandlingQueryDto): HubWorkPageDto = core.hubWork(query)

    override suspend fun discrepancies(query: HubHandlingQueryDto): HubDiscrepancyPageDto = core.hubDiscrepancies(query)

    override suspend fun applyHandling(
        context: RequestContextDto,
        input: HubHandlingInputDto,
    ): HubHandlingReceiptDto = core.applyHubHandling(context, input)

    override suspend fun handlingOperation(operation: String): HubHandlingReceiptDto? = core.hubHandlingOperation(operation)

    override suspend fun stock(query: HubStockQueryDto): HubStockPageDto = core.hubStock(query)

    override suspend fun receiveStock(
        context: RequestContextDto,
        input: HubReceiptInputDto,
    ): HubStockReceiptDto = core.receiveHubStock(context, input)

    override suspend fun receiptOperation(operation: String): HubStockReceiptDto? = core.hubReceiptOperation(operation)

    override suspend fun hubContext(): HubContextDto? = core.hubContext()

    override suspend fun initializeHub(context: RequestContextDto): HubContextDto = core.initializeHub(context)

    override suspend fun references(query: HubReferenceQueryDto): HubReferencePageDto = core.hubReferences(query)

    override suspend fun editReference(
        context: RequestContextDto,
        edit: HubReferenceEditDto,
    ): HubReferenceReceiptDto = core.applyHubReference(context, edit)

    override suspend fun referenceOperation(operation: String): HubReferenceReceiptDto? = core.hubReferenceOperation(operation)

    override suspend fun referenceHistory(
        id: String,
        after: ULong,
        limit: UInt,
    ): List<HubReferenceReceiptDto> = core.hubReferenceHistory(id, after, limit)

    override fun snapshot(): AppSnapshotDto = core.snapshot()

    override suspend fun bootstrap(): AppSnapshotDto = core.bootstrap()

    override suspend fun subscribe(onChange: (SnapshotChangeDto) -> Unit): NativeSubscriptionHandle {
        val subscription =
            core.subscribeChangesV2(
                object : HarvestCircleChangeObserver {
                    override fun onChange(change: SnapshotChangeDto) = onChange(change)
                },
            )
        return UniFfiNativeSubscriptionHandle(subscription)
    }

    override suspend fun beginGeneratedIdentity(): NativeGeneratedRecoveryHandle =
        UniFfiGeneratedRecoveryHandle(core.beginGeneratedIdentity())

    override suspend fun acknowledgeGeneratedIdentity(
        context: RequestContextDto,
        request: NativeGeneratedRecoveryHandle,
    ): AppSnapshotDto = core.acknowledgeGeneratedIdentity(context, request.generated())

    override suspend fun cancelGeneratedIdentity(request: NativeGeneratedRecoveryHandle): Boolean =
        core.cancelGeneratedIdentity(request.generated())

    override suspend fun importIdentity(
        context: RequestContextDto,
        secretKey: ByteArray,
    ): IdentityCommandReceiptDto = core.importIdentity(context, secretKey)

    override suspend fun selectIdentity(publicKeyHex: String): AppSnapshotDto = core.selectIdentity(publicKeyHex)

    override suspend fun activateIdentity(publicKeyHex: String): AppSnapshotDto = core.activateIdentity(publicKeyHex)

    override suspend fun signOut(): AppSnapshotDto = core.signOut()

    override suspend fun refreshActiveProfile(): AppSnapshotDto = core.refreshActiveProfile()

    override suspend fun requestIdentityRemoval(publicKeyHex: String): NativeRemovalHandle =
        UniFfiRemovalHandle(core.requestIdentityRemoval(publicKeyHex))

    override suspend fun confirmIdentityRemoval(
        context: RequestContextDto,
        request: NativeRemovalHandle,
    ): AppSnapshotDto = core.confirmIdentityRemoval(context, request.generated())

    override suspend fun shutdown(): ShutdownReceiptDto = core.shutdownV2()

    override fun close() = core.close()

    private fun NativeGeneratedRecoveryHandle.generated(): GeneratedRecoveryRequest =
        (this as? UniFfiGeneratedRecoveryHandle)?.request
            ?: error("Generated recovery handle does not belong to this native runtime")

    private fun NativeRemovalHandle.generated(): RemovalRequest =
        (this as? UniFfiRemovalHandle)?.request
            ?: error("Removal handle does not belong to this native runtime")
}

private class UniFfiGeneratedRecoveryHandle(
    val request: GeneratedRecoveryRequest,
) : NativeGeneratedRecoveryHandle {
    override fun identity(): IdentityDto = request.identity()

    override fun expiresAtSeconds(): Long = request.expiresAtSeconds()

    override fun takeRecoverySecret(): String = request.takeRecoveryNsec()

    override fun close() = request.close()
}

private class UniFfiRemovalHandle(
    val request: RemovalRequest,
) : NativeRemovalHandle {
    override fun publicKeyHex(): String = request.publicKeyHex()

    override fun deletesLocalCredential(): Boolean = request.deletesLocalCredential()

    override fun signsOut(): Boolean = request.signsOut()

    override fun expiresAtSeconds(): Long = request.expiresAtSeconds()

    override fun close() = request.close()
}

private class UniFfiNativeSubscriptionHandle(
    private val subscription: ObserverSubscription,
) : NativeSubscriptionHandle {
    override suspend fun unsubscribe() {
        try {
            subscription.unsubscribe()
        } finally {
            subscription.close()
        }
    }
}
