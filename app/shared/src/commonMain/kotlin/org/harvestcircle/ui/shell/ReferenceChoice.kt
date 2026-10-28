package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.application.HubLocationPurpose
import org.harvestcircle.application.HubReferenceKind
import org.harvestcircle.application.HubReferenceRecord
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextField
import org.harvestcircle.designsystem.theme.HarvestCircleTheme

/** Bounded presentation of native reference projections; commands validate every choice in Rust. */
@Composable
internal fun ReferenceChoice(
    label: String,
    tag: String,
    records: List<HubReferenceRecord>,
    kind: HubReferenceKind,
    selected: String?,
    select: (String?) -> Unit,
    optional: Boolean = true,
    emptyLabel: String = "Unknown",
    purpose: HubLocationPurpose? = null,
) {
    var expanded by remember(tag) { mutableStateOf(false) }
    var search by remember(tag) { mutableStateOf("") }
    var page by remember(tag) { mutableIntStateOf(0) }
    val selectedRecord = records.find { it.id == selected }
    HarvestCircleShellText(label)
    HarvestCircleShellButton(
        selectedRecord?.let { choiceLabel(it, records) } ?: selected ?: emptyLabel,
        { expanded = !expanded },
        Modifier.testTag(tag),
    )
    if (expanded) {
        Column(verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            HarvestCircleShellTextField(
                search,
                {
                    if (it.length <= 1024) {
                        search = it
                        page = 0
                    }
                },
                "Find reference",
                "Name",
                inputModifier = Modifier.testTag("$tag-search"),
            )
            if (optional) {
                HarvestCircleShellButton(emptyLabel, {
                    select(null)
                    expanded = false
                }, Modifier.testTag("$tag-none"))
            }
            val matching =
                records.filter { record ->
                    record.value.kind == kind &&
                        (purpose == null || record.value.locationPurpose == purpose) &&
                        choiceLabel(record, records).contains(search, ignoreCase = true)
                }
            val visible = matching.drop(page * 64).take(64)
            if (visible.isEmpty()) HarvestCircleShellText("No matching references. Add the needed records in Reference data.")
            visible.forEach { record ->
                HarvestCircleShellButton(
                    choiceLabel(record, records),
                    {
                        select(record.id)
                        expanded = false
                    },
                    Modifier.testTag("$tag-${record.id}"),
                    primary = record.id == selected,
                )
            }
            if (page > 0) HarvestCircleShellButton("First choices", { page = 0 })
            if ((page + 1) * 64 < matching.size) HarvestCircleShellButton("More choices", { page += 1 }, Modifier.testTag("$tag-next"))
        }
    }
}

private fun choiceLabel(
    record: HubReferenceRecord,
    records: List<HubReferenceRecord>,
): String {
    val value = record.value
    val product = value.productId?.let { id -> records.find { it.id == id }?.value?.label }
    val prefix = product?.let { "$it · " }.orEmpty()
    val purpose = value.locationPurpose?.let { " · ${it.label}" }.orEmpty()
    return "$prefix${value.label}$purpose · ${record.id.takeLast(8)}"
}
