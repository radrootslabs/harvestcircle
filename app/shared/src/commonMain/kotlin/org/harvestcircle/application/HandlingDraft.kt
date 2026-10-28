package org.harvestcircle.application

import kotlin.time.Instant

enum class HandlingAction { Move, Replan, NewWork, OpenDiscrepancy, ResolveDiscrepancy }

enum class MovementScope { WholeGroup, IdentifiedPortion }

data class HandlingVersions(
    val stock: ULong,
    val catalog: ULong,
)

data class HandlingFields(
    val targetId: String? = null,
    val scope: MovementScope = MovementScope.WholeGroup,
    val quantity: String = "",
    val physicalLabel: String = "",
    val physicalObservation: String = "",
    val observation: String = "",
    val occurrenceKind: HubOccurrenceKind = HubOccurrenceKind.Unknown,
    val occurrenceStart: String = "",
    val occurrenceEnd: String = "",
)

/** Retained presentation input; native commands validate every physical decision. */
sealed interface HandlingDraft {
    val fields: HandlingFields
    val versions: HandlingVersions

    data class Move(
        val selected: HubGroupView,
        override val fields: HandlingFields = HandlingFields(),
    ) : HandlingDraft {
        override val versions get() = HandlingVersions(selected.stockRevision, selected.catalogRevision)
    }

    data class Replan(
        val selected: HubWorkRow,
        override val versions: HandlingVersions,
        override val fields: HandlingFields = HandlingFields(),
    ) : HandlingDraft

    data class NewWork(
        val selected: HubGroupView,
        override val fields: HandlingFields = HandlingFields(),
    ) : HandlingDraft {
        override val versions get() = HandlingVersions(selected.stockRevision, selected.catalogRevision)
    }

    data class OpenDiscrepancy(
        val selected: HubGroupView,
        override val fields: HandlingFields = HandlingFields(),
    ) : HandlingDraft {
        override val versions get() = HandlingVersions(selected.stockRevision, selected.catalogRevision)
    }

    data class ResolveDiscrepancy(
        val selected: HubDiscrepancyRow,
        override val versions: HandlingVersions,
        override val fields: HandlingFields = HandlingFields(),
    ) : HandlingDraft
}

internal fun HandlingDraft.edit(fields: HandlingFields): HandlingDraft =
    when (this) {
        is HandlingDraft.Move -> copy(fields = fields)
        is HandlingDraft.Replan -> copy(fields = fields)
        is HandlingDraft.NewWork -> copy(fields = fields)
        is HandlingDraft.OpenDiscrepancy -> copy(fields = fields)
        is HandlingDraft.ResolveDiscrepancy -> copy(fields = fields)
    }

internal fun HandlingDraft.input(): HubHandlingInput =
    when (this) {
        is HandlingDraft.Move ->
            HubHandlingInput.Move(
                versions.stock,
                versions.catalog,
                selected.group.id,
                selected.group.revision,
                requireNotNull(fields.targetId),
                when (fields.scope) {
                    MovementScope.WholeGroup -> selected.group.wholePacks
                    MovementScope.IdentifiedPortion -> requireNotNull(fields.quantity.toUIntOrNull())
                },
                when (fields.scope) {
                    MovementScope.WholeGroup -> null
                    MovementScope.IdentifiedPortion -> HubPhysicalIdentification(fields.physicalLabel, fields.physicalObservation)
                },
                fields.occurrence(),
                fields.observation,
            )
        is HandlingDraft.Replan ->
            HubHandlingInput.Replan(
                versions.stock,
                versions.catalog,
                selected.work.id,
                selected.work.quantity.revision,
                requireNotNull(fields.targetId),
                fields.observation,
            )
        is HandlingDraft.NewWork ->
            HubHandlingInput.NewWork(
                versions.stock,
                versions.catalog,
                listOf(selected.group.id),
                requireNotNull(fields.targetId),
                fields.observation,
            )
        is HandlingDraft.OpenDiscrepancy ->
            HubHandlingInput.OpenDiscrepancy(
                versions.stock,
                versions.catalog,
                selected.trace.id,
                selected.group.id,
                fields.observation,
            )
        is HandlingDraft.ResolveDiscrepancy ->
            HubHandlingInput.ResolveDiscrepancy(
                versions.stock,
                versions.catalog,
                selected.value.id,
                selected.value.revision,
                fields.observation,
            )
    }

private fun HandlingFields.occurrence(): HubOccurrence =
    when (occurrenceKind) {
        HubOccurrenceKind.Unknown -> HubOccurrence(HubOccurrenceKind.Unknown)
        HubOccurrenceKind.Known -> HubOccurrence(HubOccurrenceKind.Known, Instant.parse(occurrenceStart.trim()).epochSeconds)
        HubOccurrenceKind.Interval ->
            HubOccurrence(
                HubOccurrenceKind.Interval,
                Instant.parse(occurrenceStart.trim()).epochSeconds,
                Instant.parse(occurrenceEnd.trim()).epochSeconds,
            )
    }

internal fun HandlingFields.bounded() =
    listOf(
        quantity,
        physicalLabel,
        physicalObservation,
        observation,
        occurrenceStart,
        occurrenceEnd,
    ).all { it.length <= 1024 }
