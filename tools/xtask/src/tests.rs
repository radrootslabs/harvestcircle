use super::*;
use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn shared_required_readers_report_failures_instead_of_passing_absent_markers() {
    let root = fixture("required readers");
    write(&root, "invalid.rs", "temporary");
    fs::write(root.join("invalid.rs"), [0xff]).unwrap();
    fs::create_dir(root.join("directory.rs")).unwrap();
    write(&root, "large.rs", "");
    OpenOptions::new()
        .write(true)
        .open(root.join("large.rs"))
        .unwrap()
        .set_len(8 * 1024 * 1024 + 1)
        .unwrap();
    for path in [
        "absent.rs",
        "invalid.rs",
        "directory.rs",
        "large.rs",
        "../outside.rs",
    ] {
        let mut findings = Vec::new();
        let source = read_text(&root, path, &mut findings);
        assert!(!source.contains("forbidden marker"));
        assert!(
            !findings.is_empty(),
            "absent forbidden marker must not mean PASS: {path}"
        );
    }
    for path in ["absent.bin", "directory.rs", "large.rs", "../outside.bin"] {
        let mut findings = Vec::new();
        assert!(sha256_file(&root, path, &mut findings).is_none());
        assert!(
            !findings.is_empty(),
            "missing digest must carry a finding: {path}"
        );
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("invalid.rs", root.join("link.rs")).unwrap();
        let mut findings = Vec::new();
        read_text(&root, "link.rs", &mut findings);
        sha256_file(&root, "link.rs", &mut findings);
        assert_eq!(findings.len(), 2);
        assert!(
            findings
                .iter()
                .all(|finding| finding.contains("symbolic link"))
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn root_validation_rejects_wrong_cwd_for_every_command() {
    let root = fixture("wrong root with spaces");
    for command in [
        Command::DesignSourceAudit,
        Command::RepoAudit,
        Command::NamespaceAudit,
        Command::ProvenanceCheck,
        Command::QualificationReport,
    ] {
        let findings = run(&root, command).expect_err("wrong root must fail");
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("product root")),
            "{findings:?}"
        );
    }
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn archive_inventory_preserves_literal_backslash_identity() {
    let root = fixture("archive literal path");
    write(&root, "web\\node_modules\\unsafe.ts", "source");
    let inventory = Inventory::load(&root).expect("literal archive path");
    assert!(
        inventory
            .paths
            .iter()
            .any(|path| path == "web\\node_modules\\unsafe.ts")
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[cfg(unix)]
#[test]
fn archive_relative_paths_reject_invalid_utf8_and_escape() {
    use std::os::unix::ffi::OsStringExt;
    let root = fixture("archive path identity");
    let invalid = root.join(std::ffi::OsString::from_vec(vec![0xff]));
    assert!(
        relative(&root, &invalid)
            .expect_err("invalid UTF-8")
            .contains("UTF-8")
    );
    assert!(relative(&root, &root.join("../escape.rs")).is_err());
    assert!(relative(&root, Path::new("/unrelated/escape.rs")).is_err());
    fs::remove_dir_all(root).expect("remove fixture");
}

#[cfg(unix)]
#[test]
fn archive_inventory_rejects_symlinks_and_fifo_before_audits() {
    let root = fixture("unsafe archive");
    std::os::unix::fs::symlink("/does-not-exist", root.join("unsafe.rs")).expect("symlink");
    assert!(
        Inventory::load(&root)
            .expect_err("symlink must fail")
            .contains("symbolic link")
    );
    fs::remove_file(root.join("unsafe.rs")).expect("remove symlink");
    assert!(
        ProcessCommand::new("mkfifo")
            .arg(root.join("unsafe.rs"))
            .status()
            .expect("fifo")
            .success()
    );
    assert!(
        Inventory::load(&root)
            .expect_err("FIFO must fail")
            .contains("regular file")
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn nested_parent_inventory_is_confined_to_capsule() {
    let parent = fixture("parent with spaces");
    initialize_git_fixture(&parent);
    write(&parent, "private.rs", "not capsule source");
    write(&parent, "oss/capsule/source.rs", "capsule source");
    let inventory = Inventory::load(&parent.join("oss/capsule")).expect("nested Git inventory");
    assert!(inventory.git_aware);
    assert_eq!(inventory.paths, ["source.rs"]);
    fs::remove_dir_all(parent).expect("remove fixture");
}

#[test]
fn malformed_git_metadata_never_falls_back_to_archive() {
    let root = fixture("broken git");
    write(&root, ".git", "not a gitdir\n");
    assert!(Inventory::load(&root).is_err());
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn commands_are_exact_and_unknown_values_fail_closed() {
    assert_eq!(
        "design-source-audit".parse(),
        Ok(Command::DesignSourceAudit)
    );
    assert_eq!("repo-audit".parse(), Ok(Command::RepoAudit));
    assert_eq!("namespace-audit".parse(), Ok(Command::NamespaceAudit));
    assert_eq!("provenance-check".parse(), Ok(Command::ProvenanceCheck));
    assert_eq!(
        "qualification-report".parse(),
        Ok(Command::QualificationReport)
    );
    assert!("all".parse::<Command>().is_err());
}

#[test]
fn current_design_sources_respect_boundaries() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf();
    let inventory = Inventory::load(&root).expect("source inventory");
    let mut findings = Vec::new();
    design_source_audit(&root, &inventory, &mut findings);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn cargo_and_product_inputs_are_consistent() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_path_buf();
    let inventory = Inventory::load(&root).expect("source inventory");
    let mut findings = Vec::new();
    provenance_check(&root, &inventory, &mut findings);
    assert!(findings.is_empty(), "{findings:#?}");
}

#[test]
fn cargo_structure_reads_renamed_multiline_and_target_dependencies() {
    let manifest: toml::Value = toml::from_str(
        r#"
        [dependencies]
        harmless = "1"
        [dev-dependencies]
        simulator = { package = "nostr-sdk", version = "1" }
        [target.'cfg(unix)'.dependencies.hidden]
        package = "libsqlite3-sys"
        version = "1"
        [target.custom.dependencies]
        renamed = { package = "harvestcircle_test_bridge", version = "1" }
    "#,
    )
    .unwrap();
    let production = cargo_dependencies(&manifest, false);
    assert!(
        production
            .iter()
            .any(|(_, p)| p.get("package").and_then(toml::Value::as_str) == Some("libsqlite3-sys"))
    );
    assert!(
        production
            .iter()
            .any(|(_, p)| p.get("package").and_then(toml::Value::as_str)
                == Some("harvestcircle_test_bridge"))
    );
    assert!(!production.iter().any(|(name, _)| name == "simulator"));
    assert!(
        cargo_dependencies(&manifest, true)
            .iter()
            .any(|(name, _)| name == "simulator")
    );
}

#[test]
fn rust_runtime_rules_ignore_prose_but_reject_structural_mutations() {
    let path = "core/crates/harvestcircle_storage/src/os_keyring.rs";
    let safe = r#"impl Store { fn verify(&self) { let _ = "platform_delete("; /* PoisonError::into_inner */ read(); } }"#;
    let mut findings = Vec::new();
    audit_rust_runtime(path, safe, &mut findings);
    assert!(findings.is_empty(), "{findings:?}");
    for unsafe_source in [
        "impl Store { fn verify(&self) { platform_delete /* changed formatting */ (); } }",
        "impl Store { fn verify(&self) { self . delete (); } }",
        "fn recover() { PoisonError /* unrelated formatting */ :: into_inner(error); }",
        "mod tests { fn recover() { PoisonError::into_inner(error); } }",
        "not valid Rust!",
    ] {
        findings.clear();
        audit_rust_runtime(path, unsafe_source, &mut findings);
        assert!(!findings.is_empty(), "accepted: {unsafe_source}");
    }
    findings.clear();
    audit_rust_runtime(
        path,
        "#[cfg(test)] mod fixtures { fn recover() { PoisonError::into_inner(error); } }",
        &mut findings,
    );
    assert!(findings.is_empty());
}

#[test]
fn reusable_application_environment_rules_cover_qualified_and_renamed_access() {
    let path = "core/crates/harvestcircle_application/src/renamed.rs";
    for source in [
        "fn load() { std /* whitespace */ :: env :: var(\"setting\"); }",
        "use std::{env as host}; fn load() { host::var(\"setting\"); }",
        "use std::env::var as setting; fn load() { setting(\"setting\"); }",
    ] {
        let mut findings = Vec::new();
        audit_rust_runtime(path, source, &mut findings);
        assert!(!findings.is_empty(), "{source}");
    }
    let mut findings = Vec::new();
    audit_rust_runtime(
        path,
        "// std::env is outside this owner\nfn load() {}",
        &mut findings,
    );
    assert!(findings.is_empty());
}

#[test]
fn multiline_git_dependencies_cannot_hide_mutable_or_redirected_sources() {
    let root = fixture("multiline-git");
    write(
        &root,
        "core/deny.toml",
        "[sources]\nrequired-git-spec = 'rev'\nallow-git = ['https://example.invalid/lib']\n",
    );
    let manifest = "[dependencies.selected]\ngit = 'https://example.invalid/lib'\nrev = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'\n";
    write(&root, "core/Cargo.toml", manifest);
    write(&root, "core/Cargo.lock", "version = 4\n");
    let inventory = Inventory::load(&root).unwrap();
    let mut findings = Vec::new();
    git_source_policy(&root, &inventory, &mut findings);
    assert!(findings.is_empty(), "{findings:?}");
    write(
        &root,
        "core/Cargo.toml",
        &manifest.replace("rev =", "branch ="),
    );
    git_source_policy(&root, &inventory, &mut findings);
    assert!(findings.iter().any(|f| f.contains("branch or tag")));
    assert!(findings.iter().any(|f| f.contains("full revision pin")));
    findings.clear();
    write(
        &root,
        "core/Cargo.toml",
        &manifest.replace(
            "https://example.invalid/lib",
            "https://redirect.invalid/lib",
        ),
    );
    git_source_policy(&root, &inventory, &mut findings);
    assert!(findings.iter().any(|f| f.contains("not allowlisted")));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn sqlite_topology_rejects_alternate_duplicate_and_direct_authority() {
    let root = fixture("sqlite-topology");
    write(
        &root,
        "core/Cargo.lock",
        "[[package]]\nname = \"libsqlite3-sys\"\n[[package]]\nname = \"libsqlite3-sys\"\n[[package]]\nname = \"rusqlite\"\n",
    );
    write(
        &root,
        "core/crates/unsafe_storage/Cargo.toml",
        "[target.'cfg(unix)'.dependencies.refinery]\nversion = \"0.9\"\n",
    );
    let inventory = Inventory::load(&root).expect("archive inventory");
    let mut findings = Vec::new();
    sqlite_dependency_topology(&root, &inventory, &mut findings);
    assert!(
        findings
            .iter()
            .any(|finding| finding
                .contains("exact single SQLx-selected native SQLite topology changed")),
        "{findings:#?}"
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding
                .contains("direct alternate or native SQLite dependency is forbidden")),
        "{findings:#?}"
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn font_manifest_detects_tampering_and_resource_escape() {
    let root = fixture("font-manifest");
    let prefix = "app/design_system/src/commonMain/composeResources/";
    let license =
        "Copyright (c) 2016 The Inter Project Authors\nSIL OPEN FONT LICENSE Version 1.1\n";
    write(
        &root,
        &format!("{prefix}files/licenses/inter-OFL-1.1.txt"),
        license,
    );
    write(
        &root,
        &format!("{prefix}font/inter_regular.ttf"),
        "font bytes",
    );
    let manifest = format!(
        "license = \"inter-OFL-1.1.txt\"\nlicense_sha256 = \"{:x}\"\n[[font]]\nresource = \"../../font/inter_regular.ttf\"\nsha256 = \"{:x}\"\n",
        Sha256::digest(license.as_bytes()),
        Sha256::digest(b"font bytes")
    );
    let path = format!("{prefix}files/licenses/inter-fonts.toml");
    write(&root, &path, &manifest);
    let inventory = Inventory::load(&root).unwrap();
    let mut findings = Vec::new();
    font_source_audit(&root, &inventory, &mut findings);
    assert!(findings.is_empty(), "{findings:?}");
    write(&root, &format!("{prefix}font/inter_regular.ttf"), "changed");
    font_source_audit(&root, &inventory, &mut findings);
    assert!(findings.iter().any(|f| f.contains("digest differs")));
    findings.clear();
    write(
        &root,
        &path,
        &manifest.replace("../../font/inter_regular.ttf", "../../font/../outside.ttf"),
    );
    font_source_audit(&root, &inventory, &mut findings);
    assert!(findings.iter().any(|f| f.contains("invalid font resource")));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn archive_inventory_excludes_outputs_and_includes_source() {
    let root = fixture("archive");
    write(&root, "src/main.rs", "fn main() {}\n");
    write(&root, "target/debug/generated.bin", "output");
    write(&root, "nested/build/generated.txt", "output");
    let inventory = Inventory::load(&root).expect("archive inventory");
    assert!(!inventory.git_aware);
    assert!(inventory.paths.contains(&"src/main.rs".to_owned()));
    assert!(
        !inventory
            .paths
            .iter()
            .any(|path| path.contains("target/") || path.contains("/build/"))
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn source_policy_rejects_invalid_text_without_reading_symlinks_or_directories() {
    let root = fixture("web-invalid-text");
    fs::write(root.join("invalid.ts"), [0xff]).expect("invalid text");
    fs::create_dir(root.join("directory.js")).expect("source-shaped directory");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/does-not-exist", root.join("link.css")).expect("source symlink");
    #[cfg(unix)]
    {
        assert!(
            Inventory::load(&root)
                .expect_err("archive symlink must fail before audits")
                .contains("symbolic link")
        );
        fs::remove_file(root.join("link.css")).expect("remove source symlink");
    }
    let inventory = Inventory::load(&root).expect("archive inventory");
    let mut findings = Vec::new();
    repo_audit(&root, &inventory, &mut findings);
    assert!(
        findings
            .iter()
            .any(|finding| finding.starts_with("invalid.ts:") && finding.contains("UTF-8")),
        "{findings:#?}"
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.starts_with("directory.js:")
                && finding.contains("regular file")),
        "{findings:#?}"
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn repository_policy_rejects_secret_and_generated_shapes() {
    let root = fixture("policy");
    write(&root, "README.md", "safe\n");
    write(&root, "config/credentials/release.key", "fixture\n");
    write(&root, "core/target/generated/native.bin", "fixture\n");
    write(
        &root,
        "safe.txt",
        &["-----BEGIN ", "PRIVATE KEY-----"].concat(),
    );
    write(&root, ".act/workflows/remote.yml", "fixture\n");
    let inventory = Inventory {
        paths: vec![
            "README.md".to_owned(),
            ".act/workflows/remote.yml".to_owned(),
            "config/credentials/release.key".to_owned(),
            "core/target/generated/native.bin".to_owned(),
            "safe.txt".to_owned(),
        ],
        git_aware: true,
    };
    let mut findings = Vec::new();
    repo_audit(&root, &inventory, &mut findings);
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("secret-shaped"))
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("generated build output"))
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("forbidden repository root"))
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("private-key material"))
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn namespace_policy_rejects_temporary_and_platform_sources() {
    let root = fixture("namespace");
    write(
        &root,
        "app/other/Leak.kt",
        &format!(
            "package {}\n",
            ["org", "radroots", "harvestcircle"].join(".")
        ),
    );
    write(
        &root,
        "app/shared/src/commonMain/kotlin/org/harvestcircle/Leak.kt",
        "import com.sun.jna.Native\n",
    );
    let inventory = Inventory::load(&root).expect("archive inventory");
    let mut findings = Vec::new();
    namespace_audit(&root, &inventory, &mut findings);
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("temporary product namespace"))
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("platform dependency"))
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn public_ide_documentation_is_not_a_legacy_namespace() {
    let root = fixture("public-ide-docs");
    write(
        &root,
        "docs/android-studio.md",
        "Use Android Studio for the public KMP project.\n",
    );
    write(
        &root,
        ".github/ISSUE_TEMPLATE/ide.md",
        "Report the Android Studio version.\n",
    );
    let inventory = Inventory::load(&root).unwrap();
    let mut findings = Vec::new();
    namespace_audit(&root, &inventory, &mut findings);
    assert!(findings.is_empty(), "{findings:?}");
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn moved_kotlin_sources_retain_counter_and_secret_boundaries() {
    let root = fixture("moved-kotlin-boundaries");
    write(
        &root,
        "app/desktop/src/main/kotlin/org/harvestcircle/desktop/Moved.kt",
        "val counter = AtomicLong()\nval placeholder = \"nsec1…\"\n",
    );
    let inventory = Inventory::load(&root).unwrap();
    let mut findings = Vec::new();
    namespace_audit(&root, &inventory, &mut findings);
    assert!(
        findings
            .iter()
            .any(|f| f.contains("process-local operation counter"))
    );
    assert!(!findings.iter().any(|f| f.contains("secret key literal")));
    write(
        &root,
        "app/desktop/src/main/kotlin/org/harvestcircle/desktop/Moved.kt",
        "val secret = \"nsec1aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n",
    );
    findings.clear();
    namespace_audit(&root, &inventory, &mut findings);
    assert!(findings.iter().any(|f| f.contains("secret key literal")));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn current_compose_sources_respect_design_ownership() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root");
    let inventory = Inventory::load(&root).expect("current source inventory");
    let mut findings = Vec::new();
    product_shell_audit(&root, &inventory, &mut findings);
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn product_shell_audit_rejects_color_and_primitive_bypasses_after_a_move() {
    let root = fixture("moved-compose");
    let path = "app/desktop/src/main/kotlin/org/harvestcircle/desktop/MovedScreen.kt";
    write(
        &root,
        path,
        "import androidx.compose.runtime.Composable\nimport androidx.compose.material.Button\n@Composable fun ShellButton() { Color(0xFF000000); BasicText(\"bypass\"); BasicTextField(\"\", {}) }\n",
    );
    let inventory = Inventory::load(&root).expect("moved UI inventory");
    let mut findings = Vec::new();
    product_shell_audit(&root, &inventory, &mut findings);
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("hard-coded Compose color outside the theme adapter"))
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("BasicText bypasses the shell primitive adapter"))
    );
    assert!(findings.iter().any(|finding| {
        finding.contains("BasicTextField bypasses the shell primitive adapter")
    }));
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("Material component dependency is forbidden"))
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn public_docs_are_allowed_but_private_orchestration_is_rejected() {
    for path in [
        "docs/architecture.md",
        ".github/ISSUE_TEMPLATE/bug.md",
        "spec/format.md",
    ] {
        assert!(!is_private_state_path(path), "{path}");
    }
    for path in [
        ".act/workflows/private.yml",
        ".beads/issues.jsonl",
        "nested/sessions/history.md",
        ".harvestcircle.local.mk",
    ] {
        assert!(is_private_state_path(path), "{path}");
    }
}

#[test]
fn repository_policy_rejects_symbolic_links() {
    use std::os::unix::fs::symlink;
    let root = fixture("symlink");
    write(&root, "outside.txt", "outside\n");
    fs::create_dir_all(root.join("app")).expect("create app");
    symlink(root.join("outside.txt"), root.join("app/escape.txt")).expect("create symlink");
    assert!(
        Inventory::load(&root)
            .expect_err("symlink fails before audit dispatch")
            .contains("symbolic link")
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[cfg(unix)]
#[test]
fn git_inventory_rejects_an_intermediate_symbolic_link() {
    use std::os::unix::fs::symlink;

    let root = fixture("git-inventory-intermediate-symlink");
    initialize_git_fixture(&root);
    write(&root, "tracked/file.txt", "tracked\n");
    add_git_fixture_path(&root, Path::new("tracked/file.txt"));
    fs::rename(root.join("tracked"), root.join("actual")).expect("move tracked directory");
    symlink(root.join("actual"), root.join("tracked")).expect("create intermediate symlink");

    let error = Inventory::load(&root).expect_err("intermediate symlink must fail closed");
    assert!(error.contains("Git inventory path traverses a symbolic link"));
    fs::remove_dir_all(root).expect("remove fixture");
}

#[cfg(unix)]
#[test]
fn git_inventory_rejects_invalid_utf8_before_filesystem_traversal() {
    use std::io::Write as _;
    use std::process::Stdio;

    let root = fixture("git-inventory-invalid-utf8-symlink");
    initialize_git_fixture(&root);
    write(&root, "blob.txt", "tracked\n");
    let object = ProcessCommand::new("git")
        .arg("-C")
        .arg(&root)
        .args(["hash-object", "-w", "blob.txt"])
        .output()
        .expect("write fixture blob");
    assert!(object.status.success());
    let object = String::from_utf8(object.stdout).expect("Git object ID is UTF-8");
    let mut index_entry = format!("100644 blob {}\ttracked/", object.trim()).into_bytes();
    index_entry.push(0x80);
    index_entry.extend_from_slice(b"/file.txt\0");
    let mut update = ProcessCommand::new("git")
        .arg("-C")
        .arg(&root)
        .args(["update-index", "-z", "--index-info"])
        .stdin(Stdio::piped())
        .spawn()
        .expect("start hostile index update");
    update
        .stdin
        .take()
        .expect("hostile index stdin")
        .write_all(&index_entry)
        .expect("write hostile index entry");
    assert!(
        update
            .wait()
            .expect("finish hostile index update")
            .success()
    );
    let error = Inventory::load(&root).expect_err("invalid UTF-8 inventory path must fail closed");
    assert_eq!(error, "Git inventory path is not valid UTF-8");
    fs::remove_dir_all(root).expect("remove fixture");
}

#[cfg(unix)]
#[test]
fn git_inventory_preserves_a_literal_backslash_without_aliasing_a_separator() {
    let root = fixture("git-inventory-backslash");
    initialize_git_fixture(&root);
    write(&root, r"tracked\file.txt", "literal backslash\n");
    write(&root, "tracked/file.txt", "path separator\n");
    add_git_fixture_path(&root, Path::new(r"tracked\file.txt"));
    add_git_fixture_path(&root, Path::new("tracked/file.txt"));

    let inventory = Inventory::load(&root).expect("distinct Git paths must remain distinct");
    assert!(inventory.paths.contains(&r"tracked\file.txt".to_owned()));
    assert!(inventory.paths.contains(&"tracked/file.txt".to_owned()));
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn git_inventory_rejects_a_missing_leaf() {
    let root = fixture("git-inventory-missing-leaf");
    initialize_git_fixture(&root);
    write(&root, "tracked/file.txt", "tracked\n");
    add_git_fixture_path(&root, Path::new("tracked/file.txt"));
    fs::remove_file(root.join("tracked/file.txt")).expect("remove tracked file");

    let error = Inventory::load(&root).expect_err("missing inventory leaf must fail closed");
    assert!(error.contains("Git inventory path is missing"));
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn git_inventory_rejects_a_directory_leaf() {
    let root = fixture("git-inventory-directory-leaf");
    initialize_git_fixture(&root);
    write(&root, "tracked/file.txt", "tracked\n");
    add_git_fixture_path(&root, Path::new("tracked/file.txt"));
    fs::remove_file(root.join("tracked/file.txt")).expect("remove tracked file");
    fs::create_dir(root.join("tracked/file.txt")).expect("create directory leaf");

    let error = Inventory::load(&root).expect_err("directory inventory leaf must fail closed");
    assert!(error.contains("Git inventory path is not a regular file"));
    fs::remove_dir_all(root).expect("remove fixture");
}

#[cfg(unix)]
#[test]
fn git_inventory_rejects_a_fifo_leaf_without_opening_it() {
    let root = fixture("git-inventory-fifo-leaf");
    initialize_git_fixture(&root);
    write(&root, "tracked/file.txt", "tracked\n");
    add_git_fixture_path(&root, Path::new("tracked/file.txt"));
    fs::remove_file(root.join("tracked/file.txt")).expect("remove tracked file");
    assert!(
        ProcessCommand::new("mkfifo")
            .arg(root.join("tracked/file.txt"))
            .status()
            .expect("create FIFO leaf")
            .success()
    );

    let error = Inventory::load(&root).expect_err("FIFO inventory leaf must fail closed");
    assert!(error.contains("Git inventory path is not a regular file"));
    fs::remove_dir_all(root).expect("remove fixture");
}

#[test]
fn mutable_or_inconsistent_git_dependencies_fail_closed() {
    let root = fixture("provenance");
    write(
        &root,
        "core/deny.toml",
        "required-git-spec = \"rev\"\nallow-git = [\"https://example.invalid/lib\"]\n",
    );
    write(
        &root,
        "core/Cargo.toml",
        "[dependencies]\nlib = { git = \"https://example.invalid/lib\", branch = \"main\" }\n",
    );
    write(&root, "core/Cargo.lock", "");
    let mut findings = Vec::new();
    git_source_policy(&root, &Inventory::load(&root).unwrap(), &mut findings);
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("branch or tag"))
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("full revision pin"))
    );

    findings.clear();
    let inventory = Inventory::load(&root).expect("archive inventory");
    provenance_check(&root, &inventory, &mut findings);
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("Cargo shared dependency identity"))
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

#[cfg(unix)]
#[test]
fn bounded_cargo_input_reads_reject_final_and_intermediate_symlinks() {
    use std::os::unix::fs::symlink;

    let root = fixture("cargo-input-symlinks");
    write(&root, "actual/Cargo.lock", "version = 4\n");
    symlink(root.join("actual/Cargo.lock"), root.join("final.lock")).expect("create final symlink");
    assert!(
        bounded_no_follow_bytes(&root, Path::new("final.lock"), 1024)
            .expect_err("final symlink must fail")
            .contains("symbolic link")
    );

    symlink(root.join("actual"), root.join("core")).expect("create intermediate symlink");
    assert!(
        bounded_no_follow_bytes(&root, Path::new("core/Cargo.lock"), 1024)
            .expect_err("intermediate symlink must fail")
            .contains("symbolic link")
    );
    assert_eq!(
        bounded_no_follow_bytes(&root, Path::new("actual/Cargo.lock"), 1024)
            .expect("regular bounded file"),
        b"version = 4\n"
    );
    assert!(
        bounded_no_follow_bytes(&root, Path::new("actual/Cargo.lock"), 1)
            .expect_err("oversize file must fail")
            .contains("byte limit")
    );
    fs::remove_dir_all(root).expect("remove fixture");
}

fn fixture(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "harvestcircle-xtask-{name}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("create fixture");
    root
}

fn initialize_git_fixture(root: &Path) {
    assert!(
        ProcessCommand::new("git")
            .arg("-C")
            .arg(root)
            .args(["init", "--quiet"])
            .status()
            .expect("initialize Git fixture")
            .success()
    );
}

fn add_git_fixture_path(root: &Path, relative: &Path) {
    assert!(
        ProcessCommand::new("git")
            .arg("-C")
            .arg(root)
            .args(["add", "--"])
            .arg(relative)
            .status()
            .expect("index tracked fixture")
            .success()
    );
}

fn write(root: &Path, relative: &str, source: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
    fs::write(path, source).expect("write fixture");
}

#[test]
fn storage_visibility_and_create_only_custody_are_structural() {
    for (path, source, expected) in [
        (
            "core/crates/harvestcircle_storage/src/lib.rs",
            "mod db; pub use db::Database;",
            None,
        ),
        (
            "core/crates/harvestcircle_storage/src/lib.rs",
            "pub /* implementation */ mod db;",
            Some("must remain private"),
        ),
        (
            "core/crates/harvestcircle_storage/src/os_keyring.rs",
            "fn create() { store.create_item(label, attrs, secret, /* replace */ false, mime); }",
            None,
        ),
        (
            "core/crates/harvestcircle_storage/src/os_keyring.rs",
            "fn create() { store.create_item(label, attrs, secret, true, mime); }",
            Some("never replace"),
        ),
        (
            "core/crates/harvestcircle_storage/src/os_keyring.rs",
            "fn create() { store.set_generic_password(service, account, secret); }",
            Some("never replace"),
        ),
    ] {
        let mut findings = Vec::new();
        audit_rust_runtime(path, source, &mut findings);
        if let Some(expected) = expected {
            assert!(
                findings.iter().any(|f| f.contains(expected)),
                "{findings:?}"
            );
        } else {
            assert!(findings.is_empty(), "{findings:?}");
        }
    }
}
