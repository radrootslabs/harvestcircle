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
 * Controls whether a component renders the optional macOS-style keyboard focus ring.
 *
 * HarvestCircle deliberately defaults to [None]. Components remain focusable and keyboard-operable when
 * the ring is disabled; this enum controls only the visible ring. Use [WhenFocused] where a product
 * surface requires an explicit focus indicator.
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
