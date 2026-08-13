package org.harvestcircle.designsystem.theme

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp
import org.harvestcircle.designsystem.generated.resources.Res
import org.harvestcircle.designsystem.generated.resources.inter_bold
import org.harvestcircle.designsystem.generated.resources.inter_medium
import org.harvestcircle.designsystem.generated.resources.inter_regular
import org.harvestcircle.designsystem.generated.resources.inter_semibold
import org.jetbrains.compose.resources.Font

/** Semantic typography roles backed by the bundled Inter UI family. */
@Immutable
public class HarvestCircleTypography internal constructor(
    public val display: TextStyle,
    public val pageTitle: TextStyle,
    public val sectionTitle: TextStyle,
    public val subsectionTitle: TextStyle,
    public val body: TextStyle,
    public val bodyStrong: TextStyle,
    public val bodySmall: TextStyle,
    public val label: TextStyle,
    public val labelSmall: TextStyle,
    public val code: TextStyle,
)

/** Resolves the exact Inter weights used by HarvestCircle without depending on host font installation. */
@Composable
internal fun rememberHarvestCircleTypography(scale: Float): HarvestCircleTypography {
    val sans =
        FontFamily(
            Font(
                resource = Res.font.inter_regular,
                weight = FontWeight.Normal,
                style = FontStyle.Normal,
            ),
            Font(
                resource = Res.font.inter_medium,
                weight = FontWeight.Medium,
                style = FontStyle.Normal,
            ),
            Font(
                resource = Res.font.inter_semibold,
                weight = FontWeight.SemiBold,
                style = FontStyle.Normal,
            ),
            Font(
                resource = Res.font.inter_bold,
                weight = FontWeight.Bold,
                style = FontStyle.Normal,
            ),
        )

    return remember(sans, scale) {
        createHarvestCircleTypography(sans = sans, scale = scale)
    }
}

/**
 * Pure typography constructor retained for token tests. Production composition supplies Inter via
 * [rememberHarvestCircleTypography]; tests may use the platform default family when font resources are absent.
 */
internal fun createHarvestCircleTypography(
    sans: FontFamily = FontFamily.Default,
    scale: Float = 1F,
): HarvestCircleTypography {
    val mono = FontFamily.Monospace

    return HarvestCircleTypography(
        display =
            TextStyle(
                fontFamily = sans,
                fontWeight = FontWeight.SemiBold,
                fontSize = (28 * scale).sp,
                lineHeight = (34 * scale).sp,
                letterSpacing = (-0.25).sp,
            ),
        pageTitle =
            TextStyle(
                fontFamily = sans,
                fontWeight = FontWeight.SemiBold,
                fontSize = (22 * scale).sp,
                lineHeight = (27 * scale).sp,
                letterSpacing = (-0.15).sp,
            ),
        sectionTitle =
            TextStyle(
                fontFamily = sans,
                fontWeight = FontWeight.SemiBold,
                fontSize = (17 * scale).sp,
                lineHeight = (22 * scale).sp,
                letterSpacing = (-0.05).sp,
            ),
        subsectionTitle =
            TextStyle(
                fontFamily = sans,
                fontWeight = FontWeight.SemiBold,
                fontSize = (13 * scale).sp,
                lineHeight = (17 * scale).sp,
            ),
        body =
            TextStyle(
                fontFamily = sans,
                fontWeight = FontWeight.Normal,
                fontSize = (13 * scale).sp,
                lineHeight = (18 * scale).sp,
            ),
        bodyStrong =
            TextStyle(
                fontFamily = sans,
                fontWeight = FontWeight.SemiBold,
                fontSize = (13 * scale).sp,
                lineHeight = (18 * scale).sp,
            ),
        bodySmall =
            TextStyle(
                fontFamily = sans,
                fontWeight = FontWeight.Normal,
                fontSize = (11 * scale).sp,
                lineHeight = (15 * scale).sp,
            ),
        label =
            TextStyle(
                fontFamily = sans,
                fontWeight = FontWeight.Medium,
                fontSize = (13 * scale).sp,
                lineHeight = (17 * scale).sp,
            ),
        labelSmall =
            TextStyle(
                fontFamily = sans,
                fontWeight = FontWeight.Medium,
                fontSize = (11 * scale).sp,
                lineHeight = (15 * scale).sp,
            ),
        code =
            TextStyle(
                fontFamily = mono,
                fontWeight = FontWeight.Normal,
                fontSize = (12 * scale).sp,
                lineHeight = (17 * scale).sp,
            ),
    )
}
