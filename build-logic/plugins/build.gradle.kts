import org.gradle.api.tasks.SourceSetContainer
import org.gradle.api.tasks.testing.Test

plugins {
    `java-gradle-plugin`
    alias(libs.plugins.kotlin.jvm)
}

kotlin {
    jvmToolchain(21)
}

dependencies {
    implementation(project(":contracts"))
    testImplementation(kotlin("test"))
}

val sourceSets = the<SourceSetContainer>()
val functionalTestSourceSet = sourceSets.create("functionalTest")

configurations[functionalTestSourceSet.implementationConfigurationName]
    .extendsFrom(configurations.testImplementation.get())
configurations[functionalTestSourceSet.runtimeOnlyConfigurationName]
    .extendsFrom(configurations.testRuntimeOnly.get())

dependencies {
    add(functionalTestSourceSet.implementationConfigurationName, gradleTestKit())
    add(functionalTestSourceSet.implementationConfigurationName, kotlin("test"))
}

gradlePlugin {
    plugins {
        create("harvestCircleRoot") {
            id = "org.harvestcircle.build.root"
            implementationClass = "org.harvestcircle.buildlogic.plugins.HarvestCircleRootPlugin"
        }
        create("harvestCircleKmpShared") {
            id = "org.harvestcircle.build.kmp-shared"
            implementationClass = "org.harvestcircle.buildlogic.plugins.HarvestCircleKmpSharedPlugin"
        }
        create("harvestCircleDesktopApp") {
            id = "org.harvestcircle.build.desktop-app"
            implementationClass = "org.harvestcircle.buildlogic.plugins.HarvestCircleDesktopAppPlugin"
        }
        create("harvestCircleRustFfi") {
            id = "org.harvestcircle.build.rust-ffi"
            implementationClass = "org.harvestcircle.buildlogic.plugins.HarvestCircleRustFfiPlugin"
        }
        create("harvestCirclePackaging") {
            id = "org.harvestcircle.build.packaging"
            implementationClass = "org.harvestcircle.buildlogic.plugins.HarvestCirclePackagingPlugin"
        }
    }
    testSourceSets(functionalTestSourceSet)
}

tasks.withType<Test>().configureEach {
    useJUnitPlatform()
}

val functionalTest by tasks.registering(Test::class) {
    description = "Runs convention-plugin tests against isolated Gradle builds."
    group = "verification"
    testClassesDirs = functionalTestSourceSet.output.classesDirs
    classpath = functionalTestSourceSet.runtimeClasspath
    shouldRunAfter(tasks.test)
}

tasks.check {
    dependsOn(functionalTest)
}
