package org.harvestcircle.designsystem

/**
 * Marks a HarvestCircle UI API whose source or behavior contract may still change after cross-platform
 * product validation.
 */
@RequiresOptIn(
    message = "This HarvestCircle UI API is not yet contract-stable.",
    level = RequiresOptIn.Level.WARNING,
)
@Retention(AnnotationRetention.BINARY)
@Target(
    AnnotationTarget.CLASS,
    AnnotationTarget.FUNCTION,
    AnnotationTarget.PROPERTY,
)
public annotation class ExperimentalHarvestCircleUiApi
