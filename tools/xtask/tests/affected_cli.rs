//! Actual CLI, history and input-mutation regressions in complete standalone copies.
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "harvestcircle affected {} {}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        copy_archive_source(source, source, &path);
        Self(path)
    }
    fn init(&self) -> String {
        git(&self.0, &["init", "-b", "master"]);
        self.commit()
    }
    fn commit(&self) -> String {
        git(&self.0, &["add", "."]);
        git(
            &self.0,
            &[
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-m",
                "fixture",
            ],
        );
        String::from_utf8(git(&self.0, &["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .to_owned()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn git(root: &Path, args: &[&str]) -> Output {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}
fn report(root: &Path, base: Option<&str>, env: &[(&str, &str)]) -> (Output, Value) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_harvestcircle_xtask"));
    command
        .current_dir(root)
        .arg("affected-report")
        .env_remove("HARVESTCIRCLE_AFFECTED_BASE");
    if let Some(base) = base {
        command.args(["--base", base]);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command.output().unwrap();
    let value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stderr)));
    (output, value)
}
fn assert_all(value: &Value) {
    for lane in ["native", "web", "interop"] {
        assert_eq!(value["selection"][lane], true, "{}", value["selection"]);
    }
}
fn append(root: &Path, path: &str) {
    let p = root.join(path);
    let mut text = fs::read_to_string(&p).unwrap();
    text.push('\n');
    fs::write(p, text).unwrap();
}

#[test]
fn web_only_changes_keep_native_source_identity_but_not_head_provenance() {
    let fixture = Fixture::new();
    let base = fixture.init();
    let (first, before) = report(&fixture.0, Some(&base), &[]);
    assert!(first.status.success());
    append(&fixture.0, "web/src/app.html");
    fixture.commit();
    let (second, after) = report(&fixture.0.join("web/src"), Some(&base), &[]);
    assert!(second.status.success());
    assert_eq!(after["selection"]["web"], true);
    if after["native"]["input_identity"].is_null() || after["web"]["input_identity"].is_null() {
        assert_all(&after);
    } else {
        assert_eq!(after["selection"]["native"], false);
    }
    assert_eq!(
        before["native"]["source_identity"],
        after["native"]["source_identity"]
    );
    assert_ne!(
        before["web"]["source_identity"],
        after["web"]["source_identity"]
    );
    assert_ne!(
        before["source_provenance"]["head"],
        after["source_provenance"]["head"]
    );
    assert_eq!(after["native"]["execution"], "NOT_EXECUTED");
    assert_eq!(after["native"]["reuse"], "NOT_ACCEPTED_BY_THIS_REPORT");
}
#[test]
fn old_and_new_rename_paths_deletion_unknown_and_missing_base_widen() {
    let fixture = Fixture::new();
    fixture.init();
    fs::write(
        fixture.0.join("web/old name.ts"),
        "export const fixture = 1;\n",
    )
    .unwrap();
    fixture.commit();
    fs::rename(
        fixture.0.join("web/old name.ts"),
        fixture.0.join("web/new name.ts"),
    )
    .unwrap();
    fixture.commit();
    // Rename detection requires an actual old-path base, not a pre-add base.
    let previous = String::from_utf8(git(&fixture.0, &["rev-parse", "HEAD^"]).stdout).unwrap();
    let (_, renamed) = report(&fixture.0, Some(previous.trim()), &[]);
    assert_all(&renamed);
    let paths = renamed["selection"]["changed_paths"].as_array().unwrap();
    assert!(paths.contains(&Value::from("web/old name.ts")));
    assert!(paths.contains(&Value::from("web/new name.ts")));
    let deletion_base = fixture.commit();
    fs::remove_file(fixture.0.join("web/new name.ts")).unwrap();
    let (deleted, value) = report(&fixture.0, Some(&deletion_base), &[]);
    assert!(!deleted.status.success());
    assert_all(&value);
    assert_eq!(value["repository_safety"]["status"], "FAIL");
    fixture.commit();
    fs::write(
        fixture.0.join("unknown input.txt"),
        "ordinary unknown input\n",
    )
    .unwrap();
    let (_, unknown) = report(&fixture.0, Some(&deletion_base), &[]);
    assert_all(&unknown);
    for base in [None, Some("missing-revision"), Some("bad\nrevision")] {
        let (_, v) = report(&fixture.0, base, &[]);
        assert_all(&v);
        assert!(!v["selection"]["reasons"].as_array().unwrap().is_empty());
    }
}
#[test]
fn native_lock_shared_fixture_make_and_environment_invalidate_inputs() {
    let fixture = Fixture::new();
    let base = fixture.init();
    let (_, before) = report(&fixture.0, Some(&base), &[]);
    append(&fixture.0, "core/Cargo.lock");
    let (_, native) = report(&fixture.0, Some(&base), &[]);
    assert_eq!(native["selection"]["native"], true);
    assert_ne!(
        before["native"]["source_identity"],
        native["native"]["source_identity"]
    );
    append(&fixture.0, "Makefile");
    let (_, both) = report(&fixture.0, Some(&base), &[]);
    assert_all(&both);
    let (_, environment) = report(
        &fixture.0,
        Some(&base),
        &[("RUSTFLAGS", "--cfg harvestcircle_input_probe")],
    );
    assert_ne!(
        both["native"]["inputs"]["environment_sha256"],
        environment["native"]["inputs"]["environment_sha256"]
    );
    assert_eq!(
        both["native"]["source_identity"],
        environment["native"]["source_identity"]
    );
    let shared = fixture.0.join("contracts/interop/affected fixture.json");
    fs::write(shared, "{}\n").unwrap();
    let (_, shared) = report(&fixture.0, Some(&base), &[]);
    assert_all(&shared);
    assert_ne!(
        both["web"]["source_identity"],
        shared["web"]["source_identity"]
    );
}
#[test]
fn archive_nested_parent_and_shallow_history_are_explicit_conservative_fallbacks() {
    let fixture = Fixture::new();
    let (archive, v) = report(&fixture.0, None, &[]);
    assert!(archive.status.success());
    assert_all(&v);
    let parent = fixture.0.parent().unwrap().join(format!(
        "harvestcircle parent {}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&parent).unwrap();
    let capsule = parent.join("capsule");
    fs::rename(&fixture.0, &capsule).unwrap();
    git(&parent, &["init", "-b", "master"]);
    let (_, v) = report(&capsule, None, &[]);
    assert_all(&v);
    assert!(v["source_provenance"]["head"].is_null());
    assert!(
        v["selection"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("enclosing history"))
    );
    fs::rename(&capsule, &fixture.0).unwrap();
    fs::remove_dir_all(parent).unwrap();
    let base = fixture.init();
    fs::write(fixture.0.join(".git/shallow"), format!("{base}\n")).unwrap();
    let (_, v) = report(&fixture.0, Some(&base), &[]);
    assert_all(&v);
    assert!(
        v["selection"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("shallow"))
    );
}
#[test]
fn forced_generated_input_never_escapes_fresh_policy_and_missing_tools_never_claim_reuse() {
    let fixture = Fixture::new();
    let base = fixture.init();
    let tools = Fixture(std::env::temp_dir().join(format!(
        "harvestcircle tool absence {} {}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    fs::create_dir(&tools.0).unwrap();
    let git_executable = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|path| path.join("git"))
        .find(|path| path.is_file())
        .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(git_executable, tools.0.join("git")).unwrap();
    #[cfg(not(unix))]
    fs::copy(git_executable, tools.0.join("git.exe")).unwrap();
    let (_, v) = report(
        &fixture.0,
        Some(&base),
        &[("PATH", tools.0.to_str().unwrap())],
    );
    assert_all(&v);
    assert_eq!(v["native"]["availability"], "TOOL_UNAVAILABLE");
    assert_eq!(v["web"]["availability"], "TOOL_UNAVAILABLE");
    assert!(v["native"]["input_identity"].is_null());
    fs::create_dir_all(fixture.0.join("web/build")).unwrap();
    fs::write(
        fixture.0.join("web/build/forced.js"),
        "ordinary generated output\n",
    )
    .unwrap();
    git(&fixture.0, &["add", "-f", "web/build/forced.js"]);
    let (output, v) = report(&fixture.0, Some(&base), &[]);
    assert!(!output.status.success());
    assert_eq!(v["repository_safety"]["status"], "FAIL");
}

#[test]
fn linked_worktree_has_own_history_and_malformed_paths_fail_fresh_policy() {
    let fixture = Fixture::new();
    let base = fixture.init();
    let linked = Fixture(std::env::temp_dir().join(format!(
        "harvestcircle affected worktree {} {}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    git(
        &fixture.0,
        &[
            "worktree",
            "add",
            "--detach",
            linked.0.to_str().unwrap(),
            "HEAD",
        ],
    );
    append(&linked.0, "web/src/app.html");
    let (output, v) = report(&linked.0.join("web"), Some(&base), &[]);
    assert!(output.status.success());
    assert_eq!(v["selection"]["merge_base"], base);
    assert_eq!(v["source_provenance"]["head"], base);
    assert_eq!(v["source_provenance"]["dirty"], true);
    fs::write(
        linked.0.join("web/literal%2fescape.ts"),
        "export const fixture = true;\n",
    )
    .unwrap();
    let (output, v) = report(&linked.0, Some(&base), &[]);
    assert!(!output.status.success());
    assert_all(&v);
    assert_eq!(v["repository_safety"]["status"], "FAIL");
}
#[cfg(unix)]
#[test]
fn executable_mode_and_profile_feature_inputs_change_identity() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let base = fixture.init();
    let (_, before) = report(&fixture.0, Some(&base), &[]);
    fs::set_permissions(
        fixture.0.join("Makefile"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let (_, after) = report(&fixture.0, Some(&base), &[]);
    assert_all(&after);
    assert_ne!(
        before["native"]["source_identity"],
        after["native"]["source_identity"]
    );
    assert_ne!(
        before["web"]["source_identity"],
        after["web"]["source_identity"]
    );
    let (_, configured) = report(
        &fixture.0,
        Some(&base),
        &[
            ("HARVESTCIRCLE_NATIVE_FEATURES", "feature_probe"),
            ("HARVESTCIRCLE_NATIVE_PROFILE", "release"),
            ("HARVESTCIRCLE_BUILD_MODE", "governed"),
        ],
    );
    assert_eq!(
        after["native"]["source_identity"],
        configured["native"]["source_identity"]
    );
    assert_ne!(
        after["native"]["inputs"]["environment_sha256"],
        configured["native"]["inputs"]["environment_sha256"]
    );
}

#[test]
fn every_consumed_non_head_native_build_input_invalidates_the_fingerprint() {
    let fixture = Fixture::new();
    let base = fixture.init();
    let (_, before) = report(&fixture.0, Some(&base), &[]);
    for name in [
        "SOURCE_DATE_EPOCH",
        "HARVESTCIRCLE_BUILD_RADROOTS_REVISION",
        "HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN",
        "HARVESTCIRCLE_BUILD_JAVA_TOOLCHAIN",
        "HARVESTCIRCLE_BUILD_KOTLIN_TOOLCHAIN",
    ] {
        let (_, after) = report(&fixture.0, Some(&base), &[(name, "input_probe")]);
        assert_eq!(
            before["native"]["source_identity"],
            after["native"]["source_identity"]
        );
        assert_ne!(
            before["native"]["inputs"]["environment_sha256"],
            after["native"]["inputs"]["environment_sha256"],
            "{name}"
        );
        if !before["native"]["input_identity"].is_null() {
            assert_ne!(
                before["native"]["input_identity"], after["native"]["input_identity"],
                "{name}"
            );
        }
    }
    let (_, provenance) = report(
        &fixture.0,
        Some(&base),
        &[
            ("HARVESTCIRCLE_BUILD_SOURCE_COMMIT", "source_probe"),
            ("HARVESTCIRCLE_BUILD_SOURCE_DIRTY", "source_probe"),
        ],
    );
    assert_eq!(
        before["native"]["source_identity"],
        provenance["native"]["source_identity"]
    );
    assert_ne!(
        before["source_provenance"]["producer_environment_sha256"],
        provenance["source_provenance"]["producer_environment_sha256"]
    );
}

#[test]
fn compiler_cargo_and_jvm_families_are_bounded_hashed_and_never_qualify_custom_artifacts() {
    let fixture = Fixture::new();
    let base = fixture.init();
    let (_, before) = report(&fixture.0, Some(&base), &[]);
    for name in [
        "RUSTC_WRAPPER",
        "CARGO_BUILD_TARGET",
        "CARGO_PROFILE_RELEASE_OPT_LEVEL",
        "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
        "CFLAGS_x86_64_unknown_linux_gnu",
        "HOST_CXXFLAGS",
        "TARGET_CC",
        "CARGO_ENCODED_RUSTDOCFLAGS",
        "JAVA_OPTS",
        "_JAVA_OPTIONS",
        "ORG_GRADLE_PROJECT_nativeOs",
    ] {
        let (_, after) = report(&fixture.0, Some(&base), &[(name, "controlled_input_probe")]);
        assert_eq!(
            before["native"]["source_identity"],
            after["native"]["source_identity"]
        );
        assert_ne!(
            before["native"]["inputs"]["environment_sha256"],
            after["native"]["inputs"]["environment_sha256"],
            "{name}"
        );
        assert!(
            !serde_json::to_string(&after)
                .unwrap()
                .contains("controlled_input_probe")
        );
        assert_eq!(after["native"]["reuse"], "NOT_ACCEPTED_BY_THIS_REPORT");
        if !before["native"]["input_identity"].is_null() {
            assert_ne!(
                before["native"]["input_identity"], after["native"]["input_identity"],
                "{name}"
            );
        }
    }
    let oversized = "x".repeat(65537);
    let (_, after) = report(&fixture.0, Some(&base), &[("RUSTFLAGS", &oversized)]);
    assert_all(&after);
    assert_eq!(after["native"]["availability"], "INPUT_UNAVAILABLE");
    assert!(after["native"]["input_identity"].is_null());
    assert_eq!(
        after["native"]["source_identity"],
        before["native"]["source_identity"]
    );
}

fn copy_archive_source(source: &Path, directory: &Path, destination: &Path) {
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let relative = path.strip_prefix(source).unwrap();
        let parts = relative
            .components()
            .map(|part| part.as_os_str().to_str().unwrap())
            .collect::<Vec<_>>();
        if matches!(
            parts[0],
            ".git" | ".gradle" | ".kotlin" | ".idea" | "build" | "target" | "out"
        ) || parts
            .iter()
            .any(|part| matches!(*part, "build" | "target" | "out"))
            || (matches!(parts[0], "build-logic" | "buildSrc")
                && parts
                    .get(1)
                    .is_some_and(|part| matches!(*part, ".gradle" | ".kotlin")))
            || (parts[0] == "web"
                && (parts.contains(&"node_modules")
                    || parts.get(1).is_some_and(|part| {
                        matches!(
                            *part,
                            ".svelte-kit" | "coverage" | "test-results" | "playwright-report"
                        )
                    })))
        {
            continue;
        }
        let file_type = entry.file_type().unwrap();
        assert!(
            !file_type.is_symlink(),
            "fixture source symlink: {relative:?}"
        );
        if file_type.is_dir() {
            fs::create_dir_all(destination.join(relative)).unwrap();
            copy_archive_source(source, &path, destination);
        } else {
            assert!(
                file_type.is_file(),
                "fixture source is not regular: {relative:?}"
            );
            fs::create_dir_all(destination.join(relative).parent().unwrap()).unwrap();
            fs::copy(&path, destination.join(relative)).unwrap();
        }
    }
}
