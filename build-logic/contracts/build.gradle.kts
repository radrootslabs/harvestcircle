plugins {
    alias(libs.plugins.kotlin.jvm)
}

kotlin {
    jvmToolchain(21)
}

dependencies {
    testImplementation(kotlin("test"))
}

tasks.test {
    useJUnitPlatform()
}

providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { outputRoot ->
    layout.buildDirectory.set(file(outputRoot).resolve("build-logic-contracts"))
}
