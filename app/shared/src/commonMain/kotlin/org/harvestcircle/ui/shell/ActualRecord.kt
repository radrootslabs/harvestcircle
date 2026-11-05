package org.harvestcircle.ui.shell

import androidx.compose.runtime.Composable
import org.harvestcircle.application.ActualPreview
import org.harvestcircle.application.ActualResult
import org.harvestcircle.application.HubActualDuplicate
import org.harvestcircle.application.HubActualHoldAssessment
import org.harvestcircle.application.HubActualReport
import org.harvestcircle.application.HubActualReviewOriginal
import org.harvestcircle.application.HubOccurrence
import org.harvestcircle.application.HubTraceActualDeparture
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellText

@Composable
internal fun ActualReportRecord(report: HubActualReport) {
    HarvestCircleShellPanel {
        HarvestCircleShellText("Original unverified report ${report.id}")
        HarvestCircleShellText("Reporter ${report.actor.value} · Recorded ${stockUtc(report.recordedAt.value)}")
        HarvestCircleShellText("Original operation ${report.operationId.value}")
        HarvestCircleShellText("Reported packs ${report.reported.wholePacks ?: "Unknown"}")
        HarvestCircleShellText("Reported pack ${report.reported.variantId ?: "Unknown"}")
        HarvestCircleShellText("Reported recipient ${report.reported.recipientId ?: "Unknown"}")
        HarvestCircleShellText("Reported occurrence ${actualOccurrence(report.reported.occurrence)} · Clock ${report.clockAssessment}")
        HarvestCircleShellText("Reported source ${report.reported.sourceKind}; this is a claim, not verified attribution.")
        if (report.reported.sourceClaims.isEmpty()) {
            HarvestCircleShellText(
                "Unknown source: review affected scope globally; no exact stock association.",
            )
        }
        report.reported.sourceClaims.forEach {
            HarvestCircleShellText(
                "Claimed trace ${it.traceId} · Group ${it.groupId ?: "Unknown"} · Location ${it.locationId ?: "Unknown"}",
            )
        }
        HarvestCircleShellText(report.reported.observation)
        HarvestCircleShellText("Saving a report changes neither stock nor work. Original claims remain after review.")
    }
}

@Composable
internal fun ActualPreviewRecord(preview: ActualPreview) {
    HarvestCircleShellText("Current native preview · No effect has been recorded")
    when (preview) {
        is ActualPreview.Departure -> {
            ActualReportRecord(preview.value.report)
            ActualDepartureRecord(preview.value.proposed, preview.value.historicalHolds)
        }
        is ActualPreview.Link -> ActualDuplicateRecord(preview.value.proposed)
    }
}

@Composable
internal fun ActualResultRecord(result: ActualResult) {
    when (result) {
        is ActualResult.Reported -> ActualReportRecord(result.value)
        is ActualResult.Reviewed ->
            when (val original = result.value.original) {
                is HubActualReviewOriginal.Departure -> {
                    ActualReportRecord(original.report)
                    ActualDepartureRecord(original.original, original.historicalHolds)
                }
                is HubActualReviewOriginal.Duplicate -> ActualDuplicateRecord(original.original)
            }
    }
}

@Composable
private fun ActualDepartureRecord(
    original: HubTraceActualDeparture,
    holds: List<HubActualHoldAssessment>,
) {
    HarvestCircleShellPanel {
        HarvestCircleShellText("Separate reconciliation of an already reported physical departure")
        HarvestCircleShellText("Reviewer ${original.actor.value} · Recorded ${stockUtc(original.recordedAt.value)}")
        HarvestCircleShellText("Reviewed occurrence ${actualOccurrence(original.occurrence)}")
        HarvestCircleShellText("${original.wholePacks} whole packs · ${original.variant.value.label} · ${original.recipient.value.label}")
        HarvestCircleShellText("Current balancing group ${original.before.id} · ${original.balancingLocation.value.label}")
        HarvestCircleShellText("Native balance: ${original.before.wholePacks} before · ${original.onHandAfterWholePacks} after")
        HarvestCircleShellText(
            "Historical source ${original.sourceOperationId.value} · ${original.sourceGroupId}",
        )
        HarvestCircleShellText("Historical location ${original.historicalLocation.value.label}")
        HarvestCircleShellText("Source evidence: ${original.sourceEvidence}")
        HarvestCircleShellText("Quantity evidence: ${original.quantityEvidence}")
        HarvestCircleShellText("Recipient evidence: ${original.recipientEvidence}")
        HarvestCircleShellText("Timing evidence: ${original.timingEvidence}")
        HarvestCircleShellText("Known-departure comparison: ${original.duplicateReviewBasis}")
        original.workChanges.forEach {
            HarvestCircleShellText("Work ${it.before.id} · ${it.before.quantity} before · ${it.after.quantity} after")
        }
        if (holds.isEmpty()) HarvestCircleShellText("No historical hold finding in this native comparison. Current treatment is separate.")
        holds.forEach {
            when (it) {
                is HubActualHoldAssessment.Verified ->
                    HarvestCircleShellText(
                        "Verified historical scope · ${it.timing} · Hold ${it.original.stamp.id}",
                    )
                is HubActualHoldAssessment.Unresolved ->
                    HarvestCircleShellText(
                        "Unresolved scope · ${it.reason} · Conditional ${it.conditionalTiming} · ${it.original.stamp.id}",
                    )
            }
        }
        HarvestCircleShellText("Historical reconciliation grants no retrospective departure permission and never releases a current hold.")
    }
}

@Composable
private fun ActualDuplicateRecord(original: HubActualDuplicate) {
    ActualReportRecord(original.report)
    HarvestCircleShellPanel {
        HarvestCircleShellText("Linked to a known departure · Zero new stock or work effect")
        HarvestCircleShellText("Reviewer ${original.actor.value} · Recorded ${stockUtc(original.recordedAt.value)}")
        HarvestCircleShellText("Original operation ${original.known.operationId.value} · ${original.known.wholePacks} whole packs")
        HarvestCircleShellText("${original.known.variant.value.label} · Original recipient ${original.known.recipient.value.label}")
        original.interpretation?.let { meaning ->
            HarvestCircleShellText("Meaning at link review · ${meaning.effectiveWholePacks} whole packs")
            HarvestCircleShellText("Recipient at link review: ${meaning.recipient?.value?.label ?: "Unknown"}")
            HarvestCircleShellText(
                "Linked recipient amendments: ${meaning.recipientHistory.size}. Original and amended details remain in Trace.",
            )
        }
        HarvestCircleShellText("Original actor ${original.known.actor.value} · Recorded ${stockUtc(original.known.recordedAt.value)}")
        HarvestCircleShellText("Original occurrence ${actualOccurrence(original.known.occurrence)}")
        original.known.lines.forEach {
            HarvestCircleShellText("${it.groupId} · ${it.location.value.label} · ${it.wholePacks} whole packs")
        }
        HarvestCircleShellText("Verification: ${original.verification}")
        HarvestCircleShellText(
            "Linking retains the complete known population record and every original claim. It does not clear a breach or current hold.",
        )
    }
}

internal fun actualOccurrence(value: HubOccurrence): String =
    "${value.kind} · ${value.startUnixSeconds?.let(::stockUtc) ?: "Unknown"} · ${value.endUnixSeconds?.let(::stockUtc) ?: "Unknown"}"
