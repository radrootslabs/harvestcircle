package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.DispatchPresenter
import org.harvestcircle.application.DispatchViewState
import org.harvestcircle.application.HubDispatchKind
import org.harvestcircle.application.HubDispatchLine
import org.harvestcircle.application.HubDispatchMaterial
import org.harvestcircle.application.HubDispatchStamp
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.material
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole

internal fun dispatchActionLabel(kind: HubDispatchKind) =
    when (kind) {
        HubDispatchKind.Prepare -> "Prepare identification"
        HubDispatchKind.Revise -> "Revise preparation"
        HubDispatchKind.Issue -> "Issue identification"
        HubDispatchKind.Replace -> "Replace identification"
        HubDispatchKind.MarkUnused -> "Mark identification unused"
    }

@Composable
internal fun DispatchEditor(
    state: DispatchViewState,
    presenter: DispatchPresenter,
    submit: () -> Unit,
) {
    val draft = requireNotNull(state.draft)
    HarvestCircleShellButton(
        "Back; keep identification draft",
        presenter::park,
        Modifier.testTag("dispatch-keep"),
        enabled = !state.loading,
    )
    HarvestCircleShellPanel(Modifier.testTag("dispatch-editor")) {
        HarvestCircleShellText(dispatchActionLabel(draft.kind), role = HarvestCircleShellTextRole.SectionTitle)
        draft.target?.let { target ->
            HarvestCircleShellText("Preparation ${target.id} · Revision ${target.revision} · ${target.state}")
            target.versions.lastOrNull()?.let {
                HarvestCircleShellText(
                    "Target identification: ${it.id} · Version ${it.number} · ${it.status}",
                )
            }
        }
        if (draft.kind == HubDispatchKind.Issue || draft.kind == HubDispatchKind.MarkUnused) {
            HarvestCircleShellText(
                if (draft.kind == HubDispatchKind.Issue) {
                    "Issue the reviewed preparation. Changed material must first be revised. Issuance does not grant departure permission."
                } else {
                    "Mark this current identification unused. This retains its as-issued material and does not remove stock or release holds."
                },
            )
            draft.target?.material()?.let { DispatchMaterial(it) }
        } else {
            ReferenceChoice(
                "Recipient",
                "dispatch-recipient",
                state.references,
                HubReferenceKind.Party,
                draft.recipientId,
                { presenter.edit(it, draft.observation) },
                optional = false,
                emptyLabel = "Choose the recipient",
            )
            HarvestCircleShellText(
                "Choose up to 64 whole physically identified groups. To select fewer packs, first record their observed physical separation in Stock.",
            )
            draft.lines.forEach { line ->
                DispatchLine(line)
                HarvestCircleShellButton("Remove this source group", {
                    presenter.removeGroup(line.groupId)
                }, Modifier.testTag("dispatch-remove-${line.groupId}"), enabled = !state.loading)
            }
            state.stock?.rows?.forEach { row ->
                HarvestCircleShellText("${row.product.value.label} · ${row.trace.variant.value.label} · ${row.location.value.label}")
                HarvestCircleShellButton(
                    "Select ${row.position.wholePacks} original packs · ${row.position.groupId}",
                    { presenter.selectGroup(row.position.groupId) },
                    Modifier.testTag("dispatch-group-${row.position.groupId}"),
                    enabled = !state.loading && draft.lines.size < 64 && draft.lines.none { it.groupId == row.position.groupId },
                )
            }
            if (state.stock?.more ==
                true
            ) {
                HarvestCircleShellButton(
                    "Next source groups",
                    presenter::nextStockPage,
                    Modifier.testTag("dispatch-stock-next"),
                    enabled = !state.loading,
                )
            }
        }
        HarvestCircleShellTextField(
            draft.observation,
            { presenter.edit(draft.recipientId, it) },
            "Manual confirmation",
            "Describe what you reviewed",
            inputModifier = Modifier.testTag("dispatch-observation"),
        )
        HarvestCircleShellText("Recording time and actor are captured by the native runtime. No actual departure is inferred.")
        HarvestCircleShellButton(
            "Review current context",
            presenter::reviewContext,
            Modifier.testTag("dispatch-review-context"),
            enabled = !state.loading,
        )
        if (state.needsReview) HarvestCircleShellText("Current context must be reviewed before submission. Input is retained.")
        state.review?.let { review ->
            HarvestCircleShellText(
                "Review current stock and references",
                role = HarvestCircleShellTextRole.BodyStrong,
            )
            HarvestCircleShellText(
                "Recipient: ${review.references
                    .find { it.id == review.draft.recipientId }
                    ?.value
                    ?.label ?: "Unavailable"}",
            )
            review.draft.target?.let { target ->
                HarvestCircleShellText("Current preparation revision ${target.revision} · ${target.state}")
                target.versions.lastOrNull()?.let {
                    HarvestCircleShellText("Current target identification: ${it.id} · Version ${it.number} · ${it.status}")
                }
            }
            if (draft.kind == HubDispatchKind.Issue) {
                HarvestCircleShellText(
                    "Issuance still checks the saved preparation. If these details changed, discard this issuance draft and revise the preparation first.",
                )
            }
            review.draft.lines.forEach { line ->
                val old = draft.lines.find { it.groupId == line.groupId }
                HarvestCircleShellText(
                    "Previously reviewed: ${old?.wholePacks ?: "Unavailable"} packs · ${old?.location?.value?.label ?: "Unavailable"}",
                )
                DispatchLine(line)
            }
            HarvestCircleShellButton(
                "Use reviewed context",
                presenter::acceptReview,
                Modifier.testTag("dispatch-accept-review"),
                enabled = !state.loading,
            )
        }
        HarvestCircleShellButton(
            dispatchActionLabel(draft.kind),
            submit,
            Modifier.testTag("dispatch-save"),
            primary = true,
            enabled = !state.loading && !state.needsReview && state.review == null,
        )
        HarvestCircleShellButton(
            "Discard unsubmitted draft",
            presenter::discard,
            Modifier.testTag("dispatch-discard"),
            enabled = !state.loading,
        )
    }
}

@Composable
internal fun DispatchStamp(
    value: HubDispatchStamp,
    label: String,
) {
    HarvestCircleShellText("$label: ${stockUtc(value.recordedAt.value)} · ${value.actor.value}")
    HarvestCircleShellText("Observation: ${value.observation}")
    HarvestCircleShellText("Operation: ${value.operationId.value}")
}

@Composable
internal fun DispatchMaterial(value: HubDispatchMaterial) {
    HarvestCircleShellText("Recipient: ${value.recipient.value.label} · Reference ${value.recipient.id} v${value.recipient.revision}")
    HarvestCircleShellText("${value.wholePacks} original packs · ${value.lines.size} source lines")
    HarvestCircleShellText("Captured stock ${value.capturedStockRevision} · Catalog ${value.capturedCatalogRevision}")
    HarvestCircleShellText("Sample basis: ${value.basis.id} v${value.basis.version} · ${value.basis.siteTimezone}")
    HarvestCircleShellText("Observation: ${value.observation}")
    value.lines.forEach { DispatchLine(it) }
}

@Composable
internal fun DispatchLine(value: HubDispatchLine) {
    HarvestCircleShellText("${value.product.value.label} · ${value.variant.value.label} · ${value.wholePacks} original packs")
    HarvestCircleShellText("${value.identification?.label ?: "Original received group"} · ${value.location.value.label}")
    value.identification?.let { HarvestCircleShellText(it.observation) }
    HarvestCircleShellText("Group ${value.groupId} · Revision ${value.groupRevision} · Receipt ${value.trace.id}")
    HarvestCircleShellText(
        "Supplier: ${value.trace.supplier
            ?.value
            ?.label ?: "Unknown"} · Producer: ${value.trace.producer
            ?.value
            ?.label ?: "Unknown"}",
    )
    val source = value.trace.source
    val issuer = source.issuer?.value?.label ?: "Unknown"
    HarvestCircleShellText("Source: ${source.certainty} · ${source.code ?: "Not established"}")
    HarvestCircleShellText("Source issuer: $issuer · Type: ${source.kind ?: "Unknown"}")
    HarvestCircleShellText("Source provenance: ${source.provenance ?: "Not established"}")
    HarvestCircleShellText("Original receipt occurrence: ${value.trace.occurrence.kind}")
    value.trace.occurrence.startUnixSeconds
        ?.let { HarvestCircleShellText("Occurrence start: ${stockUtc(it)}") }
    value.trace.occurrence.endUnixSeconds
        ?.let { HarvestCircleShellText("Occurrence end: ${stockUtc(it)}") }
    HarvestCircleShellText("Receipt recorded: ${stockUtc(value.trace.recordedAt.value)} · ${value.trace.actor.value}")
    HarvestCircleShellText("Original receipt observation: ${value.trace.observation}")
}
