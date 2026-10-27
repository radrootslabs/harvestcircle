import org.gradle.api.initialization.resolve.RepositoriesMode

pluginManagement {
    includeBuild("build-logic")

    repositories {
        google {
            mavenContent {
                includeGroupAndSubgroups("android")
                includeGroupAndSubgroups("androidx")
                includeGroupAndSubgroups("com.android")
                includeGroupAndSubgroups("com.google")
            }
        }

        gradlePluginPortal()
        mavenCentral()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)

    repositories {
        google {
            mavenContent {
                includeGroupAndSubgroups("android")
                includeGroupAndSubgroups("androidx")
                includeGroupAndSubgroups("com.android")
                includeGroupAndSubgroups("com.google")
            }
        }

        mavenCentral()
    }
}

plugins {
    id("org.gradle.toolchains.foojay-resolver-convention") version "1.0.0"
}

rootProject.name = "harvestcircle"

enableFeaturePreview("TYPESAFE_PROJECT_ACCESSORS")

include(
    ":app:design_system",
    ":app:desktop",
    ":app:shared",
    ":tools:design_catalog",
)

// extbuild supplies host-local output locations; standalone uses Gradle defaults.
buildCache {
    local {
        providers.environmentVariable("EXT_BUILD_GRADLE_LOCAL_BUILD_CACHE").orNull?.let { cacheRoot ->
            directory = file(cacheRoot)
        }
    }
}
