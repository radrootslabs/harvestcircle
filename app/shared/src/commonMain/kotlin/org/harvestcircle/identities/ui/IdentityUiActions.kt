package org.harvestcircle.identities.ui

data class HarvestCircleUiActions(
    val chooseCreateIdentity: () -> Unit = {},
    val chooseImportIdentity: () -> Unit = {},
    val cancelIdentityEntry: () -> Unit = {},
    val editImportDraft: (String) -> Unit = {},
    val generateIdentity: () -> Unit = {},
    val importSecretKey: () -> Unit = {},
    val acknowledgeGeneratedKeyBackup: () -> Unit = {},
    val cancelGeneratedKeyBackup: () -> Unit = {},
    val selectIdentity: (String) -> Unit = {},
    val activateIdentity: (String) -> Unit = {},
    val requestIdentityRemoval: (String) -> Unit = {},
    val cancelIdentityRemoval: () -> Unit = {},
    val confirmIdentityRemoval: () -> Unit = {},
    val refreshActiveProfile: () -> Unit = {},
    val retryLastCommand: () -> Unit = {},
    val dismissProblem: () -> Unit = {},
    val signOut: () -> Unit = {},
    val showIdentityChooser: () -> Unit = {},
    val hideIdentityChooser: () -> Unit = {},
)

data class HarvestCirclePlatformActions(
    val copySecret: (String) -> Unit = {},
    val openSource: () -> Unit = {},
    val openLicence: () -> Unit = {},
)
