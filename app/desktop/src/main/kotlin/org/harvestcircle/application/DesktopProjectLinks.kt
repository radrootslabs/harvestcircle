package org.harvestcircle.application

import org.harvestcircle.ui.shell.HarvestCircleProjectLinks
import java.awt.Desktop
import java.net.URI

internal enum class ProjectLink { Source, Licence }

internal fun projectLinkUri(link: ProjectLink): URI =
    URI.create(
        when (link) {
            ProjectLink.Source -> HarvestCircleProjectLinks.SOURCE
            ProjectLink.Licence -> HarvestCircleProjectLinks.LICENCE
        },
    )

internal fun openProjectLink(link: ProjectLink) {
    if (Desktop.isDesktopSupported() && Desktop.getDesktop().isSupported(Desktop.Action.BROWSE)) {
        Desktop.getDesktop().browse(projectLinkUri(link))
    }
}
