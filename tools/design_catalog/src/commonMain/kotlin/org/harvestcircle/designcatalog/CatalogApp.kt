@file:OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)

package org.harvestcircle.designcatalog

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import org.harvestcircle.designsystem.component.HarvestCircleButtonVariant
import org.harvestcircle.designsystem.component.HarvestCircleContentTone
import org.harvestcircle.designsystem.component.HarvestCircleControlSize
import org.harvestcircle.designsystem.component.HarvestCircleFocusRing
import org.harvestcircle.designsystem.component.HarvestCircleTextRole
import org.harvestcircle.designsystem.component.action.HarvestCircleButton
import org.harvestcircle.designsystem.component.action.HarvestCircleIconButton
import org.harvestcircle.designsystem.component.container.HarvestCircleCard
import org.harvestcircle.designsystem.component.container.HarvestCircleCardPadding
import org.harvestcircle.designsystem.component.container.HarvestCircleCardVariant
import org.harvestcircle.designsystem.component.container.HarvestCircleDialog
import org.harvestcircle.designsystem.component.container.HarvestCircleGroupBox
import org.harvestcircle.designsystem.component.feedback.HarvestCircleBanner
import org.harvestcircle.designsystem.component.feedback.HarvestCircleBannerTone
import org.harvestcircle.designsystem.component.feedback.HarvestCircleProgressIndicator
import org.harvestcircle.designsystem.component.feedback.HarvestCircleProgressKind
import org.harvestcircle.designsystem.component.feedback.HarvestCircleTooltip
import org.harvestcircle.designsystem.component.input.HarvestCircleSearchField
import org.harvestcircle.designsystem.component.input.HarvestCircleTextField
import org.harvestcircle.designsystem.component.menu.HarvestCircleMenuOption
import org.harvestcircle.designsystem.component.menu.HarvestCirclePopupButton
import org.harvestcircle.designsystem.component.navigation.HarvestCircleNavigationItem
import org.harvestcircle.designsystem.component.navigation.HarvestCircleTab
import org.harvestcircle.designsystem.component.navigation.HarvestCircleTabRow
import org.harvestcircle.designsystem.component.selection.HarvestCircleCheckbox
import org.harvestcircle.designsystem.component.selection.HarvestCircleRadioButton
import org.harvestcircle.designsystem.component.selection.HarvestCircleSwitch
import org.harvestcircle.designsystem.component.utility.HarvestCircleHorizontalDivider
import org.harvestcircle.designsystem.icon.HarvestCircleIcons
import org.harvestcircle.designsystem.layout.HarvestCircleSidebar
import org.harvestcircle.designsystem.layout.HarvestCircleSidebarSectionHeader
import org.harvestcircle.designsystem.layout.HarvestCircleToolbar
import org.harvestcircle.designsystem.primitive.HarvestCircleAppSurface
import org.harvestcircle.designsystem.primitive.HarvestCircleText
import org.harvestcircle.designsystem.theme.HarvestCircleContrast
import org.harvestcircle.designsystem.theme.HarvestCircleDensity
import org.harvestcircle.designsystem.theme.HarvestCircleDesignTheme
import org.harvestcircle.designsystem.theme.HarvestCircleInputMode
import org.harvestcircle.designsystem.theme.HarvestCircleMotionMode
import org.harvestcircle.designsystem.theme.HarvestCircleTextScale
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.harvestcircle.designsystem.theme.HarvestCircleThemeConfig
import org.harvestcircle.designsystem.theme.HarvestCircleThemeMode

private enum class CatalogPage {
    Overview,
    Controls,
    Inputs,
    Navigation,
}

@Composable
public fun HarvestCircleCatalogApp() {
    var mode by remember { mutableStateOf(HarvestCircleThemeMode.System) }
    var contrast by remember { mutableStateOf(HarvestCircleContrast.Standard) }
    var density by remember { mutableStateOf(HarvestCircleDensity.Comfortable) }
    var motion by remember { mutableStateOf(HarvestCircleMotionMode.Full) }
    var inputMode by remember { mutableStateOf(HarvestCircleInputMode.Pointer) }
    var textScale by remember { mutableStateOf(HarvestCircleTextScale.Standard) }
    val focusRing = HarvestCircleFocusRing.WhenFocused
    var page by remember { mutableStateOf(CatalogPage.Overview) }

    HarvestCircleDesignTheme(
        config =
            HarvestCircleThemeConfig(
                mode = mode,
                contrast = contrast,
                density = density,
                motion = motion,
                inputMode = inputMode,
                textScale = textScale,
            ),
    ) {
        HarvestCircleAppSurface {
            Column(Modifier.fillMaxSize()) {
                HarvestCircleToolbar {
                    HarvestCircleText(
                        text = "HarvestCircle",
                        modifier = Modifier.weight(1f),
                        role = HarvestCircleTextRole.SubsectionTitle,
                    )

                    HarvestCirclePopupButton(
                        selectedValue = mode,
                        options = HarvestCircleThemeMode.entries.map { HarvestCircleMenuOption(it, it.name) },
                        onValueChange = { mode = it },
                        size = HarvestCircleControlSize.Small,
                        focusRing = focusRing,
                    )
                    HarvestCirclePopupButton(
                        selectedValue = contrast,
                        options = HarvestCircleContrast.entries.map { HarvestCircleMenuOption(it, it.name) },
                        onValueChange = { contrast = it },
                        size = HarvestCircleControlSize.Small,
                        focusRing = focusRing,
                    )
                    HarvestCirclePopupButton(
                        selectedValue = density,
                        options = HarvestCircleDensity.entries.map { HarvestCircleMenuOption(it, it.name) },
                        onValueChange = { density = it },
                        size = HarvestCircleControlSize.Small,
                        focusRing = focusRing,
                    )
                    HarvestCirclePopupButton(
                        selectedValue = motion,
                        options = HarvestCircleMotionMode.entries.map { HarvestCircleMenuOption(it, it.name) },
                        onValueChange = { motion = it },
                        size = HarvestCircleControlSize.Small,
                        focusRing = focusRing,
                    )
                    HarvestCirclePopupButton(
                        selectedValue = inputMode,
                        options = HarvestCircleInputMode.entries.map { HarvestCircleMenuOption(it, it.name) },
                        onValueChange = { inputMode = it },
                        size = HarvestCircleControlSize.Small,
                        focusRing = focusRing,
                    )
                    HarvestCirclePopupButton(
                        selectedValue = textScale,
                        options = HarvestCircleTextScale.entries.map { HarvestCircleMenuOption(it, it.name) },
                        onValueChange = { textScale = it },
                        size = HarvestCircleControlSize.Small,
                        focusRing = focusRing,
                    )
                }

                Row(Modifier.fillMaxSize()) {
                    CatalogSidebar(
                        selected = page,
                        onSelected = { page = it },
                        focusRing = focusRing,
                    )
                    CatalogContent(
                        page = page,
                        focusRing = focusRing,
                        modifier = Modifier.weight(1f),
                    )
                }
            }
        }
    }
}

@Composable
private fun CatalogSidebar(
    selected: CatalogPage,
    onSelected: (CatalogPage) -> Unit,
    focusRing: HarvestCircleFocusRing,
) {
    HarvestCircleSidebar(modifier = Modifier.width(220.dp)) {
        HarvestCircleSidebarSectionHeader("Library")
        CatalogPage.entries.forEach { page ->
            HarvestCircleNavigationItem(
                selected = selected == page,
                onClick = { onSelected(page) },
                label = page.name,
                icon =
                    when (page) {
                        CatalogPage.Overview -> HarvestCircleIcons.Info
                        CatalogPage.Controls -> HarvestCircleIcons.Add
                        CatalogPage.Inputs -> HarvestCircleIcons.Search
                        CatalogPage.Navigation -> HarvestCircleIcons.ChevronRight
                    },
                focusRing = focusRing,
            )
        }
    }
}

@Composable
private fun CatalogContent(
    page: CatalogPage,
    focusRing: HarvestCircleFocusRing,
    modifier: Modifier = Modifier,
) {
    LazyColumn(
        modifier = modifier.fillMaxHeight(),
        contentPadding =
            PaddingValues(
                horizontal = HarvestCircleTheme.shell.layout.pageInset,
                vertical = HarvestCircleTheme.foundation.spacing.xl,
            ),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.sectionGap),
    ) {
        item {
            HarvestCircleText(
                text = "Canonical macOS component system",
                role = HarvestCircleTextRole.PageTitle,
            )
            Spacer(Modifier.height(HarvestCircleTheme.foundation.spacing.sm))
            HarvestCircleText(
                text =
                    "When focus rings are enabled, press blank canvas space to clear the active " +
                        "focus and ring.",
                role = HarvestCircleTextRole.BodySmall,
                tone = HarvestCircleContentTone.Secondary,
            )
            Spacer(Modifier.height(HarvestCircleTheme.foundation.spacing.sm))
            HarvestCircleText(
                text =
                    "Foundation-rendered controls using the pre-Liquid-Glass AppKit visual grammar: " +
                        "compact metrics, quiet borders, restrained gradients, and no Material ripple.",
                role = HarvestCircleTextRole.Body,
                tone = HarvestCircleContentTone.Secondary,
            )
        }

        when (page) {
            CatalogPage.Overview -> {
                item { PaletteSection() }
                item { TypographySection() }
                item { FeedbackSection() }
            }

            CatalogPage.Controls -> {
                item { ButtonSection(focusRing) }
                item { SelectionSection(focusRing) }
                item { ContainerSection(focusRing) }
            }

            CatalogPage.Inputs -> {
                item { InputSection(focusRing) }
            }

            CatalogPage.Navigation -> {
                item { NavigationSection(focusRing) }
            }
        }
    }
}

@Composable
private fun CatalogSection(
    title: String,
    content: @Composable () -> Unit,
) {
    Column(
        modifier = Modifier.fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md),
    ) {
        HarvestCircleText(
            text = title,
            role = HarvestCircleTextRole.SectionTitle,
        )
        content()
    }
}

@Composable
private fun PaletteSection() {
    CatalogSection("Semantic surfaces") {
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md),
            verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md),
        ) {
            val colors = HarvestCircleTheme.foundation.colors
            listOf(
                "Canvas" to colors.surface.canvas,
                "Base" to colors.surface.base,
                "Raised" to colors.surface.raised,
                "Sunken" to colors.surface.sunken,
                "Selection" to colors.surface.selected,
                "Accent" to colors.action.primary.rest,
            ).forEach { (name, color) -> ColorSwatch(name, color) }
        }
    }
}

@Composable
private fun ColorSwatch(
    name: String,
    color: Color,
) {
    Column(
        modifier = Modifier.width(112.dp),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.xs),
    ) {
        Box(
            modifier =
                Modifier
                    .size(width = 112.dp, height = 52.dp)
                    .background(color, HarvestCircleTheme.foundation.shapes.card),
        )
        HarvestCircleText(
            text = name,
            role = HarvestCircleTextRole.LabelSmall,
            tone = HarvestCircleContentTone.Secondary,
        )
    }
}

@Composable
private fun TypographySection() {
    CatalogSection("HarvestCircle typography") {
        HarvestCircleGroupBox {
            Column(verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.sm)) {
                HarvestCircleText("Page title", role = HarvestCircleTextRole.PageTitle)
                HarvestCircleText("Section title", role = HarvestCircleTextRole.SectionTitle)
                HarvestCircleText("Subsection title", role = HarvestCircleTextRole.SubsectionTitle)
                HarvestCircleText("Body text uses the owned Inter family at compact desktop sizes.")
                HarvestCircleText("Secondary explanatory text", tone = HarvestCircleContentTone.Secondary)
                HarvestCircleText("val project = HarvestCircle()", role = HarvestCircleTextRole.Code)
            }
        }
    }
}

@Composable
private fun ButtonSection(focusRing: HarvestCircleFocusRing) {
    CatalogSection("Push and toolbar buttons") {
        HarvestCircleGroupBox {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md),
                verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md),
            ) {
                HarvestCircleButton(onClick = {}, focusRing = focusRing) {
                    HarvestCircleText("Default", role = HarvestCircleTextRole.Label, tone = HarvestCircleContentTone.Inherit)
                }
                HarvestCircleButton(onClick = {}, variant = HarvestCircleButtonVariant.Secondary, focusRing = focusRing) {
                    HarvestCircleText("Secondary", role = HarvestCircleTextRole.Label, tone = HarvestCircleContentTone.Inherit)
                }
                HarvestCircleButton(onClick = {}, variant = HarvestCircleButtonVariant.Ghost, focusRing = focusRing) {
                    HarvestCircleText("Borderless", role = HarvestCircleTextRole.Label, tone = HarvestCircleContentTone.Inherit)
                }
                HarvestCircleButton(onClick = {}, variant = HarvestCircleButtonVariant.Destructive, focusRing = focusRing) {
                    HarvestCircleText("Delete", role = HarvestCircleTextRole.Label, tone = HarvestCircleContentTone.Inherit)
                }
                HarvestCircleButton(onClick = {}, enabled = false, focusRing = focusRing) {
                    HarvestCircleText("Disabled", role = HarvestCircleTextRole.Label, tone = HarvestCircleContentTone.Inherit)
                }
                HarvestCircleButton(onClick = {}, loading = true, focusRing = focusRing) {
                    HarvestCircleText("Working", role = HarvestCircleTextRole.Label, tone = HarvestCircleContentTone.Inherit)
                }
                HarvestCircleTooltip("Add item") {
                    HarvestCircleIconButton(
                        onClick = {},
                        icon = HarvestCircleIcons.Add,
                        label = "Add item",
                        focusRing = focusRing,
                    )
                }
            }
        }
    }
}

@Composable
private fun SelectionSection(focusRing: HarvestCircleFocusRing) {
    var checked by remember { mutableStateOf(true) }
    var radio by remember { mutableIntStateOf(0) }
    var switched by remember { mutableStateOf(true) }

    CatalogSection("Selection controls") {
        HarvestCircleGroupBox {
            Column(verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.sm)) {
                HarvestCircleCheckbox(
                    checked = checked,
                    onCheckedChange = { checked = it },
                    label = "Enable automatic saves",
                    focusRing = focusRing,
                )
                HarvestCircleRadioButton(
                    selected = radio == 0,
                    onClick = { radio = 0 },
                    label = "Open in current window",
                    focusRing = focusRing,
                )
                HarvestCircleRadioButton(
                    selected = radio == 1,
                    onClick = { radio = 1 },
                    label = "Open in new window",
                    focusRing = focusRing,
                )
                HarvestCircleSwitch(
                    checked = switched,
                    onCheckedChange = { switched = it },
                    label = "Show inspector",
                    focusRing = focusRing,
                )
            }
        }
    }
}

@Composable
private fun InputSection(focusRing: HarvestCircleFocusRing) {
    var projectName by remember { mutableStateOf("HarvestCircle") }
    var search by remember { mutableStateOf("") }

    CatalogSection("Text and search fields") {
        HarvestCircleGroupBox(title = "Project") {
            Column(verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.lg)) {
                HarvestCircleTextField(
                    value = projectName,
                    onValueChange = { projectName = it },
                    modifier = Modifier.fillMaxWidth(),
                    label = "Name",
                    supportingText = "Labels remain outside the bezel; there are no floating labels.",
                    focusRing = focusRing,
                )
                HarvestCircleSearchField(
                    value = search,
                    onValueChange = { search = it },
                    placeholder = "Search",
                    clearLabel = "Clear search",
                    modifier = Modifier.fillMaxWidth(),
                    focusRing = focusRing,
                )
                HarvestCircleTextField(
                    value = "Invalid value",
                    onValueChange = {},
                    modifier = Modifier.fillMaxWidth(),
                    label = "Validation",
                    errorMessage = "Enter a valid value.",
                    focusRing = focusRing,
                )
            }
        }
    }
}

@Composable
private fun FeedbackSection() {
    CatalogSection("Feedback") {
        Column(verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md)) {
            HarvestCircleBanner(
                title = "Classic macOS chrome",
                message = "The component layer is independent of Material visual anatomy.",
            )
            HarvestCircleBanner(
                message = "A recoverable warning uses semantic feedback colors.",
                tone = HarvestCircleBannerTone.Warning,
            )
            Row(
                horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.lg),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                HarvestCircleProgressIndicator()
                HarvestCircleProgressIndicator(
                    modifier = Modifier.width(180.dp),
                    kind = HarvestCircleProgressKind.Linear,
                )
            }
        }
    }
}

@Composable
private fun ContainerSection(focusRing: HarvestCircleFocusRing) {
    var showDialog by remember { mutableStateOf(false) }

    CatalogSection("Panels and dialogs") {
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md),
            verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.md),
        ) {
            HarvestCircleCard(
                modifier = Modifier.width(220.dp),
                variant = HarvestCircleCardVariant.Outlined,
                padding = HarvestCircleCardPadding.Default,
            ) {
                Column(verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.sm)) {
                    HarvestCircleText("Group box", role = HarvestCircleTextRole.SubsectionTitle)
                    HarvestCircleText(
                        "Flat surfaces use borders before shadows.",
                        tone = HarvestCircleContentTone.Secondary,
                    )
                }
            }
            HarvestCircleButton(
                onClick = { showDialog = true },
                variant = HarvestCircleButtonVariant.Secondary,
                focusRing = focusRing,
            ) {
                HarvestCircleText("Show dialog", role = HarvestCircleTextRole.Label, tone = HarvestCircleContentTone.Inherit)
            }
        }
    }

    if (showDialog) {
        HarvestCircleDialog(
            onDismissRequest = { showDialog = false },
            title = "Delete project?",
            message = "This action cannot be undone.",
            confirmLabel = "Delete",
            onConfirm = { showDialog = false },
            dismissLabel = "Cancel",
            onDismiss = { showDialog = false },
            destructive = true,
            focusRing = focusRing,
        )
    }
}

@Composable
private fun NavigationSection(focusRing: HarvestCircleFocusRing) {
    var selectedTab by remember { mutableIntStateOf(0) }
    var format by remember { mutableStateOf("PNG") }

    CatalogSection("Segmented controls and popup buttons") {
        HarvestCircleGroupBox {
            Column(verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.foundation.spacing.lg)) {
                HarvestCircleTabRow {
                    listOf("General", "Editor", "Advanced").forEachIndexed { index, label ->
                        HarvestCircleTab(
                            selected = selectedTab == index,
                            onClick = { selectedTab = index },
                            label = label,
                            focusRing = focusRing,
                        )
                    }
                }

                HarvestCirclePopupButton(
                    selectedValue = format,
                    options =
                        listOf(
                            HarvestCircleMenuOption("PNG", "PNG Image"),
                            HarvestCircleMenuOption("JPG", "JPEG Image"),
                            HarvestCircleMenuOption("SVG", "SVG Vector"),
                        ),
                    onValueChange = { format = it },
                    focusRing = focusRing,
                )
            }
        }
        HarvestCircleHorizontalDivider()
        HarvestCircleText(
            text = "Selected tab: $selectedTab · Format: $format",
            tone = HarvestCircleContentTone.Secondary,
        )
    }
}
