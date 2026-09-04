package org.harvestcircle.buildlogic.contracts

import java.io.File
import java.nio.ByteBuffer
import java.nio.channels.SeekableByteChannel
import java.nio.file.Files
import java.nio.file.LinkOption
import java.nio.file.Path
import java.nio.file.StandardOpenOption
import java.nio.file.attribute.BasicFileAttributes

public object HarvestCircleArtifactContract {
    private const val MAX_CONTRACT_BYTES: Long = 64L * 1024L
    private const val PRODUCT_NAME: String = "HarvestCircle"
    private const val IDENTITY: String = "org.harvestcircle.desktop"
    private const val PRODUCT_VERSION: String = "0.1.0-alpha"
    private const val PACKAGE_VERSION: String = "1.0.0"
    private const val BUILD_VERSION: String = "1"
    private const val FILE_NAME: String = "HarvestCircle-1.0.0.dmg"

    public const val CANONICAL_JSON: String =
        "{\"artifact_policy\":{\"checksums\":\"required\",\"cyclonedx_version\":\"1.6\",\"exact_tree_source_archives\":\"required\",\"fresh_install\":\"required\",\"git_history_bundles\":\"forbidden\",\"intoto_statement\":\"required\",\"notices\":\"required\",\"reproducibility_build_count\":2,\"secret_scan\":\"required\",\"unsigned_packages\":\"required\",\"unsigned_slsa_provenance\":\"required\"},\"contract_version\":3,\"delivery\":{\"candidate_class\":\"unsigned_nonpublishing\",\"developer_id_signing\":\"forbidden\",\"developer_team_id\":\"forbidden\",\"distribution_signing\":\"forbidden\",\"embedded_platform_adhoc_signing\":\"permitted_non_distribution_only\",\"g2\":\"unauthorized\",\"notarization\":\"unauthorized\",\"production_activation\":\"unauthorized\",\"publication\":\"unauthorized\",\"signing\":\"unauthorized\"},\"implementation_owner_step\":290,\"output\":[{\"classification\":\"production\",\"id\":\"unsigned_macos_package\",\"platforms\":[\"macos_aarch64\"]}],\"package_contract\":{\"build_version\":\"1\",\"filename\":\"HarvestCircle-1.0.0.dmg\",\"format\":\"dmg\",\"identity\":\"org.harvestcircle.desktop\",\"package_version\":\"1.0.0\",\"product_name\":\"HarvestCircle\",\"product_version\":\"0.1.0-alpha\"},\"platforms\":[\"macos_aarch64\"],\"producer\":{\"command_authority\":\"gradle_wrapper\",\"kind\":\"compose_desktop_native_distributions_jpackage\",\"nix_binding\":\"forbidden\",\"nix_produced\":false,\"source_task\":\"packageDmg\"},\"repository\":\"oss/harvestcircle\",\"schema\":\"radroots.release.artifact-contract.v3\",\"source_archive\":{\"binding\":\"canonical_exact_source_revision_tree_archive\",\"compression\":\"none\",\"compression_timestamp\":\"not_applicable\",\"content\":\"exact_source_revision_tree\",\"directory_entries\":\"omitted\",\"entry_order\":\"bytewise_git_path\",\"file_mode\":\"git_index_100644_or_100755\",\"format\":\"ustar\",\"gid\":0,\"git_history\":\"forbidden\",\"gname\":\"\",\"hardlinks\":\"forbidden\",\"mtime_source\":\"candidate_source_date_epoch\",\"path_prefix\":\"none\",\"pax_headers\":\"forbidden\",\"submodules\":\"forbidden\",\"symlinks\":\"forbidden\",\"trailer\":\"two_zero_blocks\",\"uid\":0,\"uname\":\"\"},\"source_binding\":{\"dirty_tree\":\"forbidden\",\"kind\":\"exact_clean_git_commit\",\"revision_location\":\"aggregate_source_revision\"},\"sqlite\":{\"high_level_authority\":\"sqlx_only\",\"incremental_backup_adapter\":\"sealed_native_sqlx_owned_locked_handle_only\",\"native_linkage_count\":1,\"second_pool_connection_query_transaction_migration_authority\":\"forbidden\"}}"

    public fun load(
        contractFile: File,
        repositoryRoot: File,
    ) {
        val root = repositoryRoot.toPath().toAbsolutePath().normalize()
        val contractPath = contractFile.toPath().toAbsolutePath().normalize()
        require(contractPath.startsWith(root)) { "Artifact contract must be inside the repository root" }
        val bytes = readBoundedNoFollow(root, root.relativize(contractPath))
        require(bytes.contentEquals(CANONICAL_JSON.toByteArray(Charsets.UTF_8))) {
            "HarvestCircle artifact contract differs from canonical compact JSON"
        }
    }

    public fun parse(source: String) {
        require(source == CANONICAL_JSON) {
            "HarvestCircle artifact contract differs from canonical compact JSON"
        }
    }

    public fun validatePackageCoordinates(
        productName: String,
        identity: String,
        productVersion: String,
        packageVersion: String,
        buildVersion: String,
        fileName: String,
    ) {
        require(productName == PRODUCT_NAME) { "Artifact product name differs from product coordinates" }
        require(identity == IDENTITY) { "Artifact identity differs from product coordinates" }
        require(productVersion == PRODUCT_VERSION) { "Artifact product version differs from the FFI baseline" }
        require(packageVersion == PACKAGE_VERSION) { "Artifact package version differs from the FFI baseline" }
        require(buildVersion == BUILD_VERSION) { "Artifact build version differs from the package convention" }
        require(fileName == FILE_NAME) { "Artifact filename differs from the package convention" }
    }

    private fun readBoundedNoFollow(
        root: Path,
        relative: Path,
    ): ByteArray {
        require(!relative.isAbsolute && relative.nameCount > 0 && relative.normalize() == relative) {
            "Artifact contract path must be normalized and relative"
        }
        var current = root
        relative.forEachIndexed { index, component ->
            current = current.resolve(component)
            val attributes =
                Files.readAttributes(
                    current,
                    BasicFileAttributes::class.java,
                    LinkOption.NOFOLLOW_LINKS,
                )
            require(!attributes.isSymbolicLink) { "Artifact contract path must not traverse a symbolic link" }
            if (index < relative.nameCount - 1) {
                require(attributes.isDirectory) { "Artifact contract path parent must be a directory" }
            } else {
                require(attributes.isRegularFile) { "Artifact contract path must identify a regular file" }
                require(attributes.size() <= MAX_CONTRACT_BYTES) { "Artifact contract exceeds its byte limit" }
            }
        }
        return Files.newByteChannel(
            current,
            StandardOpenOption.READ,
            LinkOption.NOFOLLOW_LINKS,
        ).use(::readBounded)
    }

    private fun readBounded(channel: SeekableByteChannel): ByteArray {
        val admittedSize = channel.size()
        require(admittedSize <= MAX_CONTRACT_BYTES) { "Artifact contract exceeds its byte limit" }
        val bytes = ByteArray(admittedSize.toInt())
        val buffer = ByteBuffer.wrap(bytes)
        while (buffer.hasRemaining()) {
            require(channel.read(buffer) >= 0) { "Artifact contract was truncated while reading" }
        }
        require(channel.read(ByteBuffer.allocate(1)) == -1) { "Artifact contract grew beyond its admitted size" }
        return bytes
    }
}
