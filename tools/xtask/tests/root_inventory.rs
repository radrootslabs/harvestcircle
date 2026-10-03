//! Exercise the public CLI against complete source copies, never the private parent.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

struct Fixture(PathBuf);
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
impl Fixture {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self::with_nonce(nonce)
    }

    fn with_nonce(nonce: u128) -> Self {
        Self::in_directory(&std::env::temp_dir(), nonce)
    }

    fn in_directory(directory: &Path, nonce: u128) -> Self {
        loop {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = directory.join(format!(
                "harvestcircle root fixture {} {nonce} {sequence}",
                std::process::id()
            ));
            match fs::create_dir(&root) {
                Ok(()) => return Self(root),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!(
                    "unable to create exclusive fixture {root:?}; fixture filesystem must exist and have sufficient writable capacity: {error}"
                ),
            }
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            if std::thread::panicking() {
                eprintln!("fixture cleanup failed for {:?}: {error}", self.0);
            } else {
                panic!("fixture cleanup failed for {:?}: {error}", self.0);
            }
        }
    }
}

#[test]
fn simultaneous_identical_timestamps_create_distinct_owned_fixtures() {
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let workers = (0..8)
        .map(|index| {
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let fixture = Fixture::with_nonce(0);
                fs::write(fixture.0.join("owner"), index.to_string()).unwrap();
                (index, fixture)
            })
        })
        .collect::<Vec<_>>();
    let fixtures = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    let paths = fixtures
        .iter()
        .map(|(_, fixture)| &fixture.0)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(paths.len(), 8);
    for (index, fixture) in &fixtures {
        assert_eq!(
            fs::read_to_string(fixture.0.join("owner")).unwrap(),
            index.to_string()
        );
    }
}

fn git(root: &Path, arguments: &[&str]) -> Output {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Git fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn copy_source(root: &Path) {
    fs::create_dir_all(root).unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    copy_archive_source(source, source, root);
}

// No Git prerequisite: these CLI tests also run from a clean source archive.
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

fn cli(root: &Path, command: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_harvestcircle_xtask"))
        .current_dir(root)
        .arg(command)
        .env("HARVESTCIRCLE_BUILD_MODE", "standalone")
        .output()
        .unwrap()
}

fn assert_pass(root: &Path, kind: &str) {
    let output = cli(root, "qualification-report");
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains(&format!("inventory={kind}"))
    );
}

#[test]
fn complete_archive_checkout_nested_parent_and_linked_worktree_pass() {
    let fixture = Fixture::new();
    let capsule = fixture.0.join("public capsule");
    copy_source(&capsule);
    assert_pass(&capsule, "archive");
    let second_archive = fixture.0.join("copied clean archive");
    fs::create_dir_all(&second_archive).unwrap();
    copy_archive_source(&capsule, &capsule, &second_archive);
    assert_pass(&second_archive, "archive");
    fs::remove_dir_all(second_archive).unwrap();
    // An intentionally unsafe private source must never enter capsule inventory.
    fs::write(
        fixture.0.join("private.rs"),
        ["-----BEGIN ", "PRIVATE KEY-----"].concat(),
    )
    .unwrap();
    git(&fixture.0, &["init", "--quiet"]);
    git(&fixture.0, &["add", "--", "private.rs", "public capsule"]);
    assert_pass(&capsule, "git");
    // A standalone capsule repository remains independently usable.
    git(&capsule, &["init", "--quiet"]);
    git(&capsule, &["add", "--", "."]);
    git(
        &capsule,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    assert_pass(&capsule, "git");
    let linked = fixture.0.join("linked capsule with spaces");
    git(
        &capsule,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            linked.to_str().unwrap(),
        ],
    );
    assert!(linked.join(".git").is_file());
    assert_pass(&linked, "git");
}

#[test]
fn cli_rejects_wrong_cwd_missing_native_markers_and_corrupt_git() {
    let fixture = Fixture::new();
    let capsule = fixture.0.join("capsule");
    copy_source(&capsule);
    for command in [
        "design-source-audit",
        "repo-audit",
        "namespace-audit",
        "provenance-check",
        "qualification-report",
    ] {
        for root in [&fixture.0, &capsule.join("core"), &capsule.join("web")] {
            let output = cli(root, command);
            assert!(!output.status.success());
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains("product root")
            );
        }
    }
    for marker in [
        "core/crates/harvestcircle_domain/src/lib.rs",
        "core/Cargo.lock",
        "core/rust-toolchain.toml",
        "tools/xtask/Cargo.lock",
        "contracts/rshr-201-step-gates.v1.json",
    ] {
        let path = capsule.join(marker);
        let saved = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        for command in [
            "design-source-audit",
            "repo-audit",
            "namespace-audit",
            "provenance-check",
            "qualification-report",
        ] {
            let output = cli(&capsule, command);
            assert!(!output.status.success(), "accepted missing {marker}");
            assert!(String::from_utf8(output.stderr).unwrap().contains(marker));
        }
        fs::write(path, saved).unwrap();
    }
    fs::write(capsule.join(".git"), "gitdir: /does-not-exist\n").unwrap();
    let output = cli(&capsule, "qualification-report");
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Git metadata")
    );
}

#[cfg(unix)]
#[test]
fn full_archive_rejects_native_symlink_parent_and_invalid_path_bytes() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let capsule = fixture.0.join("capsule");
    copy_source(&capsule);
    let real_core = fixture.0.join("outside core");
    fs::rename(capsule.join("core"), &real_core).unwrap();
    symlink(&real_core, capsule.join("core")).unwrap();
    let output = cli(&capsule, "qualification-report");
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("symbolic link")
    );
    fs::remove_file(capsule.join("core")).unwrap();
    fs::rename(real_core, capsule.join("core")).unwrap();
    let alias = fixture.0.join("aliased capsule");
    symlink(&capsule, &alias).unwrap();
    assert!(
        harvestcircle_xtask::run(&alias, harvestcircle_xtask::Command::QualificationReport)
            .is_err()
    );
    // Linux supports non-UTF-8 filenames; macOS rejects their creation with EILSEQ.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        // Container bind-mounted temporary directories may normalize invalid
        // filename bytes; native Linux tmpfs is required for this case.
        let native_fixture = Fixture::in_directory(Path::new("/dev/shm"), 0);
        let native_capsule = native_fixture.0.join("capsule");
        copy_source(&native_capsule);
        let invalid_name = std::ffi::OsString::from_vec(vec![b'x', 0xff]);
        fs::write(native_capsule.join(&invalid_name), "source")
            .expect("native Linux tmpfs must admit invalid filename bytes and have capacity");
        assert!(
            fs::read_dir(&native_capsule)
                .unwrap()
                .any(|entry| { entry.unwrap().file_name().as_bytes() == invalid_name.as_bytes() }),
            "native Linux fixture filesystem must preserve exact invalid filename bytes"
        );
        let output = cli(&native_capsule, "qualification-report");
        assert!(!output.status.success());
        assert!(String::from_utf8(output.stderr).unwrap().contains("UTF-8"));
    }
}
