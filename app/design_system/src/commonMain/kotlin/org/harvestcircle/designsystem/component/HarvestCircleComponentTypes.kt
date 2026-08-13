package org.harvestcircle.designsystem.component

/** Visual emphasis for an action. */
public enum class HarvestCircleButtonVariant {
    Primary,
    Secondary,
    Ghost,
    Destructive,
}

/** Canonical macOS control size. */
public enum class HarvestCircleControlSize {
    Small,
    Medium,
    Large,
}

/**
 * Controls whether a component renders the macOS-style keyboard focus ring.
 *
 * HarvestCircle controls default to [WhenFocused] so keyboard focus is always visible. [None] is
 * reserved for a parent control that renders an equivalent focus indicator around the same target.
 */
public enum class HarvestCircleFocusRing {
    None,
    WhenFocused,
}

/** Content emphasis shared by text and icons. */
public enum class HarvestCircleContentTone {
    Primary,
    Secondary,
    Muted,
    Disabled,
    Inverse,
    Inherit,
}

/** Semantic text role. */
public enum class HarvestCircleTextRole {
    Display,
    PageTitle,
    SectionTitle,
    SubsectionTitle,
    Body,
    BodyStrong,
    BodySmall,
    Label,
    LabelSmall,
    Code,
}

/** Canonical icon size. */
public enum class HarvestCircleIconSize {
    Small,
    Medium,
    Large,
}
