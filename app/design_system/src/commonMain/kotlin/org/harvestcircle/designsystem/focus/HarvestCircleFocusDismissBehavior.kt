package org.harvestcircle.designsystem.focus

/**
 * Controls whether a containing HarvestCircle surface clears Compose focus after a pointer press on
 * otherwise-unhandled background space.
 */
public enum class HarvestCircleFocusDismissBehavior {
    /** Clear the currently focused component when a blank surface area is pressed. */
    ClearOnBackgroundPress,

    /** Preserve focus until another component explicitly requests or clears it. */
    KeepFocused,
}
