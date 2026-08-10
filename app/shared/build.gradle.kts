import org.harvestcircle.gradle.VerifySharedBoundary
import org.jetbrains.kotlin.gradle.dsl.JvmTarget
import org.jetbrains.kotlin.gradle.plugin.KotlinPlatformType

plugins {
    alias(libs.plugins.kotlin.multiplatform)
    alias(libs.plugins.detekt)
    alias(libs.plugins.ktlint)
}

providers.environmentVariable("EXT_BUILD_GRADLE_BUILD_DIR").orNull?.let { extBuildGradleRoot ->
    layout.buildDirectory.set(file(extBuildGradleRoot).resolve("app-shared"))
}

kotlin {
    jvm("desktop") {
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_21)
        }
    }

    sourceSets {
        commonMain.dependencies {
            implementation(libs.kotlinx.coroutines.core)
        }
        commonTest.dependencies {
            implementation(kotlin("test"))
            implementation(libs.kotlinx.coroutines.test)
        }
    }

    jvmToolchain(21)

    check(
        targets
            .filter { it.platformType != KotlinPlatformType.common }
            .map { it.name } == listOf("desktop"),
    ) {
        "HarvestCircle shared must declare exactly one KMP platform target named desktop"
    }
}

val verifySharedBoundary by tasks.registering(VerifySharedBoundary::class) {
    commonSources.from(
        fileTree("src/commonMain/kotlin") {
            include("**/*.kt")
        },
    )
}

tasks.named("check") {
    dependsOn(verifySharedBoundary)
}
