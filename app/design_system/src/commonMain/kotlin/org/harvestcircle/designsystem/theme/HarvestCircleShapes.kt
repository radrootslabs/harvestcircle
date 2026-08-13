package org.harvestcircle.designsystem.theme

import androidx.compose.foundation.shape.CornerBasedShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Immutable
import androidx.compose.ui.unit.dp

/** Canonical pre-Liquid-Glass macOS shape roles. */
@Immutable
public class HarvestCircleShapes internal constructor(
    public val control: CornerBasedShape,
    public val card: CornerBasedShape,
    public val panel: CornerBasedShape,
    public val dialog: CornerBasedShape,
    public val pill: CornerBasedShape,
    /** Parallel outer contour for an optional focus ring around [control]. */
    public val controlFocusRing: CornerBasedShape,
    /** Parallel outer contour for an optional focus ring around [card]. */
    public val cardFocusRing: CornerBasedShape,
    /** Parallel outer contour for an optional focus ring around [panel]. */
    public val panelFocusRing: CornerBasedShape,
    /** Parallel outer contour for an optional focus ring around [dialog]. */
    public val dialogFocusRing: CornerBasedShape,
    /** Parallel outer contour for an optional focus ring around [pill]. */
    public val pillFocusRing: CornerBasedShape,
)

internal val HarvestCircleDefaultShapes: HarvestCircleShapes =
    HarvestCircleShapes(
        control = RoundedCornerShape(6.dp),
        card = RoundedCornerShape(8.dp),
        panel = RoundedCornerShape(10.dp),
        dialog = RoundedCornerShape(12.dp),
        pill = RoundedCornerShape(percent = 50),
        // Focus frames reserve 2.dp for the ring and 2.dp for separation. Adding that 4.dp to the
        // Radius keeps the inner and outer curves parallel instead of reusing the inner radius on a
        // larger rectangle, which creates visibly mismatched corners.
        controlFocusRing = RoundedCornerShape(10.dp),
        cardFocusRing = RoundedCornerShape(12.dp),
        panelFocusRing = RoundedCornerShape(14.dp),
        dialogFocusRing = RoundedCornerShape(16.dp),
        pillFocusRing = RoundedCornerShape(percent = 50),
    )
