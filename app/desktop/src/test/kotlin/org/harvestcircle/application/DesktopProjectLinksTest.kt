package org.harvestcircle.application

import org.harvestcircle.ui.shell.HarvestCircleProjectLinks
import kotlin.test.Test
import kotlin.test.assertEquals

class DesktopProjectLinksTest {
    @Test
    fun projectLinksAreFixedAndCannotAcceptArbitraryUris() {
        assertEquals(HarvestCircleProjectLinks.SOURCE, projectLinkUri(ProjectLink.Source).toString())
        assertEquals(HarvestCircleProjectLinks.LICENCE, projectLinkUri(ProjectLink.Licence).toString())
    }
}
