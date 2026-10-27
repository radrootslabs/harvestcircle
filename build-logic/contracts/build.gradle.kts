plugins {
    alias(libs.plugins.kotlin.jvm)
}

kotlin {
    jvmToolchain(21)
}

dependencies {
    implementation("org.tomlj:tomlj:1.0.0")
    testImplementation(kotlin("test"))
}

tasks.test {
    useJUnitPlatform()
}

providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { outputRoot ->
    layout.buildDirectory.set(file(outputRoot).resolve("build-logic-contracts"))
}
