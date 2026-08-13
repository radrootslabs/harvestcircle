package org.harvestcircle.designsystem.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.text.selection.LocalTextSelectionColors
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.remember

private val LocalHarvestCircleTheme =
    compositionLocalOf<HarvestCircleThemeSnapshot> {
        error("HarvestCircleDesignTheme is missing from the composition")
    }

/** Read-only access to the active, immutable HarvestCircle design token graph. */
public object HarvestCircleTheme {
    public val foundation: HarvestCircleFoundationTokens
        @Composable
        @ReadOnlyComposable
        get() = LocalHarvestCircleTheme.current.foundation

    public val shell: HarvestCircleShellTokens
        @Composable
        @ReadOnlyComposable
        get() = LocalHarvestCircleTheme.current.shell

    public val component: HarvestCircleComponentTokens
        @Composable
        @ReadOnlyComposable
        get() = LocalHarvestCircleTheme.current.component
}

/** Installs exactly one fail-fast token snapshot without a Material compatibility layer. */
@Composable
public fun HarvestCircleDesignTheme(
    config: HarvestCircleThemeConfig = HarvestCircleThemeConfig(),
    content: @Composable () -> Unit,
) {
    val dark = resolveHarvestCircleDarkTheme(config.mode, isSystemInDarkTheme())
    val typography = rememberHarvestCircleTypography(config.textScale.factor)
    val snapshot =
        remember(dark, config, typography) {
            createHarvestCircleThemeSnapshot(dark, config, typography)
        }
    CompositionLocalProvider(
        LocalHarvestCircleTheme provides snapshot,
        LocalTextSelectionColors provides snapshot.textSelectionColors,
        content = content,
    )
}

internal fun resolveHarvestCircleDarkTheme(
    mode: HarvestCircleThemeMode,
    systemDark: Boolean,
): Boolean =
    when (mode) {
        HarvestCircleThemeMode.System -> systemDark
        HarvestCircleThemeMode.Light -> false
        HarvestCircleThemeMode.Dark -> true
    }
