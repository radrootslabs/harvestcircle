package org.harvestcircle.buildlogic.contracts

public data class NativeTarget(
    val libraryName: String,
    val jnaPrefix: String,
)

public fun resolveNativeTarget(
    osName: String,
    architecture: String,
    cdylibName: String,
): NativeTarget {
    val os = osName.lowercase()
    val arch = architecture.lowercase()
    return when {
        os.startsWith("mac") && arch in setOf("aarch64", "arm64") ->
            NativeTarget("lib$cdylibName.dylib", "darwin-aarch64")
        os.startsWith("mac") && arch in setOf("x86_64", "amd64") ->
            NativeTarget("lib$cdylibName.dylib", "darwin-x86-64")
        os.startsWith("windows") && arch in setOf("aarch64", "arm64") ->
            NativeTarget("$cdylibName.dll", "win32-aarch64")
        os.startsWith("windows") && arch in setOf("x86_64", "amd64") ->
            NativeTarget("$cdylibName.dll", "win32-x86-64")
        os.startsWith("linux") && arch in setOf("aarch64", "arm64") ->
            NativeTarget("lib$cdylibName.so", "linux-aarch64")
        os.startsWith("linux") && arch in setOf("x86_64", "amd64") ->
            NativeTarget("lib$cdylibName.so", "linux-x86-64")
        else -> error("Unsupported native desktop host: $osName/$architecture")
    }
}
