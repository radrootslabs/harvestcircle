package org.harvestcircle.buildlogic.plugins.tasks

import org.gradle.api.DefaultTask
import org.gradle.api.GradleException
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.provider.Property
import org.gradle.api.tasks.Input
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.work.DisableCachingByDefault

@DisableCachingByDefault(because = "Test inventory verification produces no reusable output")
public abstract class VerifyTestInventory : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    public abstract val testFiles: ConfigurableFileCollection

    @get:Input
    public abstract val sourceRoot: Property<String>

    @TaskAction
    public fun verify() {
        if (testFiles.isEmpty) {
            throw GradleException("No Kotlin tests found under ${sourceRoot.get()}")
        }
    }
}
