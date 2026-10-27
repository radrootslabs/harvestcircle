package org.harvestcircle.buildlogic.contracts

import java.io.File
import java.nio.ByteBuffer
import java.nio.channels.SeekableByteChannel
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.Path
import java.nio.file.StandardOpenOption
import java.nio.file.attribute.BasicFileAttributes
import org.tomlj.Toml

public data class CargoSources(public val libRevision: String) {
    public companion object {
        public fun load(repositoryRoot: File): CargoSources {
            val root = repositoryRoot.toPath().toAbsolutePath().normalize()
            fun read(path: String): String = readBoundedNoFollow(root, Path.of(path), 32L * 1024L * 1024L) { bytes ->
                Charsets.UTF_8.newDecoder().decode(ByteBuffer.wrap(bytes)).toString()
            }
            return parse(read("core/Cargo.toml"), read("core/Cargo.lock"))
        }

        public fun parse(manifest: String, lockfile: String): CargoSources {
            val cargo = Toml.parse(manifest)
            val lock = Toml.parse(lockfile)
            require(!cargo.hasErrors() && !lock.hasErrors()) { "Invalid Cargo TOML inputs" }
            val dependencies = requireNotNull(cargo.getTable("workspace.dependencies")) { "Missing workspace dependencies" }
            val packages = requireNotNull(lock.getArray("package")) { "Missing locked packages" }
            val locked = (0 until packages.size()).map { packages.getTable(it) }
            var selected: String? = null
            dependencies.keySet().filter { it.startsWith("radroots_") }.forEach { name ->
                val dependency = requireNotNull(dependencies.getTable(name))
                require(dependency.getString("git") == "https://github.com/radrootslabs/lib" && listOf("path", "branch", "tag").none { dependency.contains(it) }) { "Shared dependency source must be canonical immutable public Git" }
                val revision = requireNotNull(dependency.getString("rev"))
                require(revision.matches(Regex("[0-9a-f]{40}"))) { "Full lowercase revision required" }
                require(selected == null || selected == revision) { "Mixed shared dependency revisions" }
                selected = revision
                val version = requireNotNull(dependency.getString("version"))
                require(version.startsWith("=")) { "Shared dependency version must be exact" }
                val matches = locked.filter { it.getString("name") == name }
                require(matches.size == 1 && matches.single().getString("version") == version.removePrefix("=") && matches.single().getString("source") == "git+https://github.com/radrootslabs/lib?rev=$revision#$revision") { "Cargo lockfile differs from selected shared source/version" }
            }
            val revision = requireNotNull(selected) { "No shared dependencies declared" }
            require(locked.filter { it.getString("name")?.startsWith("radroots_") == true }.all { it.getString("source") == "git+https://github.com/radrootslabs/lib?rev=$revision#$revision" }) { "Mixed locked shared sources" }
            return CargoSources(revision)
        }
        private fun <T> readBoundedNoFollow(
            root: Path,
            relative: Path,
            maximumBytes: Long,
            transform: (ByteArray) -> T,
        ): T {
            validateRelativePath(relative)
            var current = root
            relative.forEachIndexed { index, component ->
                current = current.resolve(component)
                val attributes =
                    Files.readAttributes(
                        current,
                        BasicFileAttributes::class.java,
                        LinkOption.NOFOLLOW_LINKS,
                    )
                require(!attributes.isSymbolicLink) { "Cargo input path must not traverse a symbolic link" }
                if (index < relative.nameCount - 1) {
                    require(attributes.isDirectory) { "Cargo input path parent must be a directory" }
                } else {
                    require(attributes.isRegularFile) { "Cargo input path must identify a regular file" }
                    require(attributes.size() <= maximumBytes) { "Cargo input exceeds its byte limit" }
                }
            }

            return Files.newByteChannel(
                current,
                StandardOpenOption.READ,
                LinkOption.NOFOLLOW_LINKS,
            ).use { channel ->
                transform(readBounded(channel, maximumBytes))
            }
        }

        private fun readBounded(
            channel: SeekableByteChannel,
            maximumBytes: Long,
        ): ByteArray {
            val admittedSize = channel.size()
            require(admittedSize <= maximumBytes) { "Cargo input exceeds its byte limit" }
            val output = ByteArray(admittedSize.toInt())
            val buffer = ByteBuffer.wrap(output)
            while (buffer.hasRemaining()) {
                require(channel.read(buffer) >= 0) { "Cargo input was truncated while reading" }
            }
            val probe = ByteBuffer.allocate(1)
            require(channel.read(probe) == -1) { "Cargo input grew beyond its admitted size" }
            return output
        }

        private fun validateRelativePath(path: Path) {
            require(!path.isAbsolute && path.nameCount > 0 && path.normalize() == path) {
                "Cargo input path must be a normalized relative path"
            }
            path.forEach { component ->
                require(component.toString() !in setOf("", ".", "..")) {
                    "Cargo input path contains an unsafe component"
                }
            }
        }
    }
}
