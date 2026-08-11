package org.harvestcircle.buildlogic.plugins.tasks

import org.gradle.api.DefaultTask
import org.gradle.api.file.ConfigurableFileCollection
import org.gradle.api.tasks.InputFiles
import org.gradle.api.tasks.PathSensitive
import org.gradle.api.tasks.PathSensitivity
import org.gradle.api.tasks.TaskAction
import org.gradle.work.DisableCachingByDefault

@DisableCachingByDefault(because = "Source-boundary verification produces no reusable output")
abstract class VerifySharedBoundary : DefaultTask() {
    @get:InputFiles
    @get:PathSensitive(PathSensitivity.RELATIVE)
    abstract val commonSources: ConfigurableFileCollection

    @TaskAction
    fun verify() {
        val forbidden =
            listOf(
                "org.harvestcircle." + "ffi",
                "com.sun." + "jna",
                "java." + "awt",
                "javax." + "swing",
                "java." + "io",
                "java." + "nio",
            )
        val findings =
            commonSources.files
                .filter { it.isFile && it.extension == "kt" }
                .flatMap { file ->
                    forbidden
                        .filter(file.readText()::contains)
                        .map { token -> "${file.name}: prohibited common-source dependency $token" }
                }
        check(findings.isEmpty()) { findings.sorted().joinToString("\n") }
    }
}
