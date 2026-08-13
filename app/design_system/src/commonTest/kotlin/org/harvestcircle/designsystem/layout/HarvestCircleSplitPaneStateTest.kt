package org.harvestcircle.designsystem.layout

import org.harvestcircle.designsystem.ExperimentalHarvestCircleUiApi
import kotlin.test.Test
import kotlin.test.assertEquals

@OptIn(ExperimentalHarvestCircleUiApi::class)
class HarvestCircleSplitPaneStateTest {
    @Test
    fun fractionIsClampedAtConstructionAndMutationBoundaries() {
        val state =
            HarvestCircleSplitPaneState(
                initialFraction = 0.05f,
                minimumFraction = 0.2f,
                maximumFraction = 0.8f,
            )

        assertEquals(0.2f, state.fraction)

        state.setFraction(0.95f)

        assertEquals(0.8f, state.fraction)
    }
}
