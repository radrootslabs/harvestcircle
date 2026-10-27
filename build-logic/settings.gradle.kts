import org.gradle.api.initialization.resolve.RepositoriesMode

pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
    versionCatalogs {
        create("libs") {
            from(files("../gradle/libs.versions.toml"))
        }
    }
}

rootProject.name = "harvestcircle-build-logic"

include(":contracts", ":plugins")

// extbuild supplies host-local output locations; standalone uses Gradle defaults.
buildCache {
    local {
        providers.environmentVariable("EXT_BUILD_GRADLE_LOCAL_BUILD_CACHE").orNull?.let { cacheRoot ->
            directory = file(cacheRoot)
        }
    }
}
