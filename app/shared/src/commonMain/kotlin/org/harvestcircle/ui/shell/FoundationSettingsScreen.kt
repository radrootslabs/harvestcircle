package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.appearance.MotionPreference
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.appearance.ThemePreference
import org.harvestcircle.application.BuildDirtyState
import org.harvestcircle.application.BuildInfo
import org.harvestcircle.designsystem.component.navigation.HarvestCircleTab
import org.harvestcircle.designsystem.component.navigation.HarvestCircleTabRow
import org.harvestcircle.designsystem.theme.HarvestCircleTheme
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.navigation.SettingsSection

object HarvestCircleProjectLinks {
    const val SOURCE = "https://github.com/radrootslabs/studio_app"
    const val LICENCE = "https://github.com/radrootslabs/studio_app/blob/dev/LICENSE"
}

data class FoundationSettingsActions(
    val selectSection: (SettingsSection) -> Unit,
    val setTheme: (ThemePreference) -> Unit,
    val setTextSize: (TextSizePreference) -> Unit,
    val setMotion: (MotionPreference) -> Unit,
)

@Composable
fun FoundationSettingsScreen(
    section: SettingsSection,
    appearance: AppearanceState,
    buildInfo: BuildInfo,
    actions: FoundationSettingsActions,
    platformActions: HarvestCirclePlatformActions,
) {
    val tabs =
        listOf(
            TemplateTab(TemplateSelectionKey("appearance"), "Appearance"),
            TemplateTab(TemplateSelectionKey("project"), "Project"),
        )
    val selected = if (section == SettingsSection.Appearance) tabs[0].key else tabs[1].key
    TabbedDetailTemplate(
        tabs = tabs,
        selected = selected,
        tabRail = { available, current ->
            HarvestCircleTabRow {
                available.forEach { tab ->
                    HarvestCircleTab(
                        label = tab.label,
                        selected = tab.key == current,
                        onClick = {
                            if (tab.key != current) {
                                actions.selectSection(
                                    if (tab.key.value == "appearance") SettingsSection.Appearance else SettingsSection.Project,
                                )
                            }
                        },
                        modifier = Modifier.testTag("settings-${tab.key.value}"),
                    )
                }
            }
        },
        detailPane = DetailPaneKind.Settings,
        detail = {
            when (section) {
                SettingsSection.Appearance -> AppearanceSettings(appearance, actions)
                SettingsSection.Project -> ProjectSettings(buildInfo, platformActions)
            }
        },
    )
}

@Composable
private fun AppearanceSettings(
    appearance: AppearanceState,
    actions: FoundationSettingsActions,
) {
    Column(
        Modifier.testTag("settings-appearance-panel"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.sectionGap),
    ) {
        ShellText("Theme", textRole = ShellTextRole.SectionTitle)
        OptionRow(
            listOf(ThemePreference.System, ThemePreference.Light, ThemePreference.Dark),
            appearance.theme,
            "theme",
            actions.setTheme,
        )
        ShellText("Text size", textRole = ShellTextRole.SectionTitle)
        OptionRow(
            listOf(TextSizePreference.Default, TextSizePreference.Large, TextSizePreference.VeryLarge),
            appearance.textSize,
            "text-size",
            actions.setTextSize,
        )
        ShellText("Motion", textRole = ShellTextRole.SectionTitle)
        OptionRow(
            listOf(MotionPreference.Standard, MotionPreference.Reduced),
            appearance.motion,
            "motion",
            actions.setMotion,
        )
    }
}

@Composable
private fun <T : Enum<T>> OptionRow(
    values: List<T>,
    selected: T,
    tagPrefix: String,
    select: (T) -> Unit,
) {
    HarvestCircleTabRow {
        values.forEach { value ->
            val label = value.label()
            HarvestCircleTab(
                label = label,
                selected = value == selected,
                onClick = { if (value != selected) select(value) },
                modifier = Modifier.testTag("$tagPrefix-${value.name.lowercase()}"),
            )
        }
    }
}

@Composable
private fun ProjectSettings(
    build: BuildInfo,
    platformActions: HarvestCirclePlatformActions,
) {
    Column(
        Modifier.testTag("settings-project-panel"),
        verticalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap),
    ) {
        ProjectFact("HarvestCircle version", build.productVersion)
        ProjectFact("Source commit", build.sourceCommit)
        ProjectFact("Source state", build.sourceDirty.label())
        ProjectFact("Radroots revision", build.radrootsRevision)
        ProjectFact("Kotlin", build.kotlinToolchain)
        ProjectFact("Compose Multiplatform", build.composeMultiplatformVersion)
        ProjectFact("Rust", build.rustToolchain)
        ProjectFact("Java", build.javaToolchain)
        ProjectFact("FFI contract", "${build.ffiContractId} ${build.ffiContractMajor}.${build.ffiContractMinor}")
        ProjectFact("FFI hash", build.ffiContractHash)
        ProjectFact(
            "Storage schema",
            "${build.minimumStorageSchemaVersion}..${build.currentStorageSchemaVersion}",
        )
        ProjectFact("Licence", "GPL-3.0-only")
        Row(horizontalArrangement = Arrangement.spacedBy(HarvestCircleTheme.shell.layout.inlineGap)) {
            ShellAction("Source", "Open HarvestCircle source", "project-open-source", onClick = platformActions.openSource)
            ShellAction(
                "Licence",
                "Open HarvestCircle licence",
                "project-open-licence",
                onClick = platformActions.openLicence,
            )
        }
    }
}

@Composable
private fun ProjectFact(
    label: String,
    value: String,
) {
    Column {
        ShellText(label, textRole = ShellTextRole.Secondary)
        ShellText(value, Modifier.testTag("project-${label.lowercase().replace(' ', '-')}"))
    }
}

private fun Enum<*>.label(): String =
    when (this) {
        TextSizePreference.VeryLarge -> "Very large"
        else -> name.lowercase().replaceFirstChar(Char::uppercaseChar)
    }

private fun BuildDirtyState.label(): String = name.lowercase().replaceFirstChar(Char::uppercaseChar)
