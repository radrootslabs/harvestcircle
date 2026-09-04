package org.harvestcircle.buildlogic.contracts

import java.io.File
import java.nio.ByteBuffer
import java.nio.channels.SeekableByteChannel
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.Path
import java.nio.file.StandardOpenOption
import java.nio.file.attribute.BasicFileAttributes
import java.security.MessageDigest

public class RadrootsLibSourceLock private constructor(
    public val lockfile: String,
    public val lockfileSha256: String,
) {
    public companion object {
        private const val MAX_SOURCE_LOCK_BYTES: Long = 1024L * 1024L
        private const val MAX_LOCKFILE_BYTES: Long = 32L * 1024L * 1024L
        private val assignment = Regex("^([a-z0-9_]+) = \\\"([^\\\"]+)\\\"$")
        private val requiredKeys =
            listOf(
                "schema",
                "repository",
                "revision",
                "architecture",
                "workspace_catalog_sha256",
                "version",
                "source_archive_sha256",
                "lockfile",
                "lockfile_sha256",
            )
        private val fixedValues =
            mapOf(
                "schema" to "radroots.lib.source-lock.v1",
                "repository" to "https://github.com/radrootslabs/lib",
                "revision" to "ad17b7d3455a7147cfa303d976fc5c70c3a4c0cb",
                "architecture" to "radroots.crates.release.v2",
                "workspace_catalog_sha256" to "deca0c080deae187ff8186c0708903e42f41ea57f77c5f91581e23aa561164a4",
                "version" to "0.1.0-alpha",
                "source_archive_sha256" to "2cf12c24ed649c3c8dd48cebcb8583996646e116fc2472539a55748c803584db",
            )

        public fun load(
            sourceLockFile: File,
            repositoryRoot: File,
        ): RadrootsLibSourceLock {
            val root = repositoryRoot.toPath().toAbsolutePath().normalize()
            val sourceLockPath = sourceLockFile.toPath().toAbsolutePath().normalize()
            require(sourceLockPath.startsWith(root)) { "Lib source lock must be inside the repository root" }
            val relativeSourceLock = root.relativize(sourceLockPath)
            val source =
                readBoundedNoFollow(root, relativeSourceLock, MAX_SOURCE_LOCK_BYTES) { bytes ->
                    bytes.toString(Charsets.UTF_8)
                }
            val parsed = parse(source)
            val actualDigest =
                readBoundedNoFollow(root, Path.of(parsed.lockfile), MAX_LOCKFILE_BYTES) { bytes ->
                    MessageDigest
                        .getInstance("SHA-256")
                        .digest(bytes)
                        .joinToString("") { byte -> "%02x".format(byte) }
                }
            require(actualDigest == parsed.lockfileSha256) {
                "Lib source-lock digest does not match the actual bounded lockfile bytes"
            }
            return parsed
        }

        public fun parse(source: String): RadrootsLibSourceLock {
            require(!source.startsWith('\uFEFF')) { "Lib source lock must not contain a UTF-8 BOM" }
            require(source.endsWith('\n') && '\r' !in source) {
                "Lib source lock must use canonical LF-terminated UTF-8 text"
            }
            val values = linkedMapOf<String, String>()
            source.dropLast(1).split('\n').forEachIndexed { index, line ->
                val match = requireNotNull(assignment.matchEntire(line)) {
                    "Lib source-lock line ${index + 1} is not a canonical string assignment"
                }
                val key = match.groupValues[1]
                val value = match.groupValues[2]
                require(key in requiredKeys) { "Unknown Lib source-lock key: $key" }
                require(values.put(key, value) == null) { "Duplicate Lib source-lock key: $key" }
            }
            require(values.keys.toList() == requiredKeys) { "Lib source-lock keys or ordering differ" }
            fixedValues.forEach { (key, expected) ->
                require(values.getValue(key) == expected) { "Lib source-lock $key differs" }
            }
            require(values.getValue("lockfile") == "core/Cargo.lock") {
                "Lib source-lock lockfile path differs"
            }
            require(values.getValue("lockfile_sha256").isCanonicalHex(64)) {
                "Lib source-lock lockfile digest is not canonical SHA-256"
            }
            validateRelativePath(Path.of(values.getValue("lockfile")))
            return RadrootsLibSourceLock(
                lockfile = values.getValue("lockfile"),
                lockfileSha256 = values.getValue("lockfile_sha256"),
            )
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
                require(!attributes.isSymbolicLink) { "Source-lock path must not traverse a symbolic link" }
                if (index < relative.nameCount - 1) {
                    require(attributes.isDirectory) { "Source-lock path parent must be a directory" }
                } else {
                    require(attributes.isRegularFile) { "Source-lock path must identify a regular file" }
                    require(attributes.size() <= maximumBytes) { "Source-lock input exceeds its byte limit" }
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
            require(admittedSize <= maximumBytes) { "Source-lock input exceeds its byte limit" }
            val output = ByteArray(admittedSize.toInt())
            val buffer = ByteBuffer.wrap(output)
            while (buffer.hasRemaining()) {
                require(channel.read(buffer) >= 0) { "Source-lock input was truncated while reading" }
            }
            val probe = ByteBuffer.allocate(1)
            require(channel.read(probe) == -1) { "Source-lock input grew beyond its admitted size" }
            return output
        }

        private fun validateRelativePath(path: Path) {
            require(!path.isAbsolute && path.nameCount > 0 && path.normalize() == path) {
                "Lib source-lock path must be a normalized relative path"
            }
            path.forEach { component ->
                require(component.toString() !in setOf("", ".", "..")) {
                    "Lib source-lock path contains an unsafe component"
                }
            }
        }
    }
}
