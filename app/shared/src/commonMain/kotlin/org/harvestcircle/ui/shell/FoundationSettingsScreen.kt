package org.harvestcircle.ui.shell

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import org.harvestcircle.appearance.AppearanceState
import org.harvestcircle.appearance.MotionPreference
import org.harvestcircle.appearance.TextSizePreference
import org.harvestcircle.appearance.ThemePreference
import org.harvestcircle.application.BuildDirtyState
import org.harvestcircle.application.BuildInfo
import org.harvestcircle.designsystem.shell.HarvestCircleShellButton
import org.harvestcircle.designsystem.shell.HarvestCircleShellPage
import org.harvestcircle.designsystem.shell.HarvestCircleShellPalette
import org.harvestcircle.designsystem.shell.HarvestCircleShellPanel
import org.harvestcircle.designsystem.shell.HarvestCircleShellTab
import org.harvestcircle.designsystem.shell.HarvestCircleShellText
import org.harvestcircle.designsystem.shell.HarvestCircleShellTextRole
import org.harvestcircle.identities.ui.HarvestCirclePlatformActions
import org.harvestcircle.navigation.SettingsSection

object HarvestCircleProjectLinks {
    const val SOURCE = "https://github.com/radrootslabs/harvestcircle"
    const val LICENCE = "https://github.com/radrootslabs/harvestcircle/blob/master/LICENSE"
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
    showSectionTabs: Boolean = true,
) {
    HarvestCircleShellPage(Modifier.testTag("template-tabbed-detail")) {
        if (showSectionTabs) {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                SettingsSection.entries.forEach { candidate ->
                    HarvestCircleShellTab(
                        label = candidate.label(),
                        selected = candidate == section,
                        onClick = { if (candidate != section) actions.selectSection(candidate) },
                        modifier = Modifier.testTag("settings-${candidate.name.lowercase()}"),
                    )
                }
            }
        }
        Box(
            Modifier
                .weight(1f)
                .verticalScroll(rememberScrollState())
                .testTag("bounded-detail-settings"),
        ) {
            when (section) {
                SettingsSection.Appearance -> AppearanceSettings(appearance, actions)
                SettingsSection.Project -> ProjectSettings(buildInfo, platformActions)
            }
        }
    }
}

@Composable
private fun AppearanceSettings(
    appearance: AppearanceState,
    actions: FoundationSettingsActions,
) {
    Column(Modifier.testTag("settings-appearance-panel"), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        SettingsChoicePanel("Theme") {
            OptionRow(
                listOf(ThemePreference.System, ThemePreference.Light, ThemePreference.Dark),
                appearance.theme,
                "theme",
                actions.setTheme,
            )
        }
        SettingsChoicePanel("Text size") {
            OptionRow(
                listOf(TextSizePreference.Default, TextSizePreference.Large, TextSizePreference.VeryLarge),
                appearance.textSize,
                "text-size",
                actions.setTextSize,
            )
        }
        SettingsChoicePanel("Motion") {
            OptionRow(
                listOf(MotionPreference.Standard, MotionPreference.Reduced),
                appearance.motion,
                "motion",
                actions.setMotion,
            )
        }
    }
}

@Composable
private fun SettingsChoicePanel(
    title: String,
    content: @Composable () -> Unit,
) {
    HarvestCircleShellPanel {
        HarvestCircleShellText(title, role = HarvestCircleShellTextRole.SectionTitle)
        content()
    }
}

@Composable
private fun <T : Enum<T>> OptionRow(
    values: List<T>,
    selected: T,
    tagPrefix: String,
    select: (T) -> Unit,
) {
    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        values.forEach { value ->
            HarvestCircleShellTab(
                label = value.label(),
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
    Column(Modifier.testTag("settings-project-panel"), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        HarvestCircleShellPanel {
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
            ProjectFact("Storage schema", "${build.minimumStorageSchemaVersion}..${build.currentStorageSchemaVersion}")
            ProjectFact("Licence", "GPL-3.0-only")
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            HarvestCircleShellButton(
                "Source",
                platformActions.openSource,
                Modifier.testTag("project-open-source"),
                primary = true,
            )
            HarvestCircleShellButton("Licence", platformActions.openLicence, Modifier.testTag("project-open-licence"))
        }
    }
}

@Composable
private fun ProjectFact(
    label: String,
    value: String,
) {
    Column {
        HarvestCircleShellText(
            label,
            role = HarvestCircleShellTextRole.Small,
            color = HarvestCircleShellPalette.contentSecondary,
        )
        HarvestCircleShellText(value, Modifier.testTag("project-${label.lowercase().replace(' ', '-')}"))
    }
}

private fun SettingsSection.label(): String = name.lowercase().replaceFirstChar(Char::uppercaseChar)

private fun Enum<*>.label(): String =
    when (this) {
        TextSizePreference.VeryLarge -> "Very large"
        else -> name.lowercase().replaceFirstChar(Char::uppercaseChar)
    }

private fun BuildDirtyState.label(): String = name.lowercase().replaceFirstChar(Char::uppercaseChar)
