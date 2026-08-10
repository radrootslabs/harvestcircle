package org.harvestcircle.buildlogic.contracts

import kotlin.test.Test
import kotlin.test.assertNotNull

class BuildLogicContractTest {
    @Test
    fun contractModuleLoadsWithoutGradleApi() {
        assertNotNull(BuildLogicContract)
    }
}
