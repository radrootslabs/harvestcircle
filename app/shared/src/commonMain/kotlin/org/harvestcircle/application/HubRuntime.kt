package org.harvestcircle.application

/** Native-authorized hub projection. This is transport data, never client authority. */
data class HubContext(
    val id: String,
    val owner: IdentityId,
    val revision: ULong,
    val sampleData: Boolean,
)

interface HarvestCircleHubRuntime {
    val demoProfile: Boolean

    suspend fun hubContext(): HubContext?

    suspend fun initializeHub(context: RequestContext): HubContext
}
