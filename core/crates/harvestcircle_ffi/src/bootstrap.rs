//! Host-only preparation of the resolved interactive data anchor.
use radroots_runtime_paths::{RadrootsPathProfile, RuntimeContext};

pub(crate) fn prepare(context: &RuntimeContext) -> Result<(), ()> {
    if context.profile() != RadrootsPathProfile::InteractiveUser {
        return Ok(());
    }
    // The public context exposes the canonical instance path, not its root.
    // Verify the complete identity suffix before preparing only the anchor.
    let state = context.paths().state();
    let root = state.ancestors().nth(3).ok_or(())?;
    if root
        .join("services")
        .join(context.service().as_str())
        .join(context.instance().as_str())
        != state
    {
        return Err(());
    }
    let base = directories::BaseDirs::new().ok_or(())?;
    let home = base.home_dir();
    if !home.is_dir() || std::fs::canonicalize(home).map_err(|_| ())? != home {
        return Err(());
    }
    prepare_directory(root)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn prepare_directory(path: &std::path::Path) -> Result<(), ()> {
    prepare_directory_with(path, || {})
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn prepare_directory_with(
    path: &std::path::Path,
    before_validation: impl FnOnce(),
) -> Result<(), ()> {
    use rustix::fs::{Mode, OFlags, fstat, mkdirat, open, openat};
    use rustix::process::geteuid;
    use std::fs::File;
    use std::path::Component;

    // Hold every binding until final validation. Missing entries may be created
    // only below an already validated current-user directory.
    let flags =
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    if !path.is_absolute()
        || path.components().count() > 128
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
    {
        return Err(());
    }
    let mut current = File::from(open("/", flags, Mode::empty()).map_err(|_| ())?);
    let mut owned = false;
    let mut bindings = Vec::new();
    for component in path.components() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => name,
            _ => return Err(()),
        };
        let parent = current.try_clone().map_err(|_| ())?;
        let next = match openat(&parent, name, flags, Mode::empty()) {
            Ok(fd) => File::from(fd),
            Err(rustix::io::Errno::NOENT) if owned => {
                match mkdirat(&parent, name, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
                    Ok(()) => parent.sync_all().map_err(|_| ())?,
                    Err(rustix::io::Errno::EXIST) => {}
                    Err(_) => return Err(()),
                }
                File::from(openat(&parent, name, flags, Mode::empty()).map_err(|_| ())?)
            }
            Err(_) => return Err(()),
        };
        let status = fstat(&next).map_err(|_| ())?;
        let user_owned = status.st_uid == geteuid().as_raw();
        let trusted_system = !owned
            && status.st_uid == 0
            && (status.st_mode & 0o022 == 0 || status.st_mode & 0o1000 != 0);
        if !(trusted_system
            || user_owned && status.st_mode & 0o022 == 0 && status.st_mode & 0o500 == 0o500)
        {
            return Err(());
        }
        owned |= user_owned;
        let identity = (status.st_dev, status.st_ino);
        bindings.push((
            parent,
            name.to_os_string(),
            next.try_clone().map_err(|_| ())?,
            identity,
            status.st_uid,
            status.st_mode,
        ));
        current = next;
    }
    if !owned {
        return Err(());
    }
    before_validation();
    for (parent, name, held, identity, uid, mode) in bindings {
        let reopened = openat(&parent, name.as_os_str(), flags, Mode::empty()).map_err(|_| ())?;
        let actual = fstat(reopened).map_err(|_| ())?;
        let original = fstat(&held).map_err(|_| ())?;
        if (actual.st_dev, actual.st_ino) != identity
            || (original.st_dev, original.st_ino) != identity
            || actual.st_uid != uid
            || actual.st_mode != mode
            || original.st_uid != uid
            || original.st_mode != mode
        {
            return Err(());
        }
    }
    current.sync_all().map_err(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn prepare_directory(_: &std::path::Path) -> Result<(), ()> {
    Err(())
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::prepare_directory;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    #[test]
    fn fresh_anchor_is_private_and_relaunch_preserves_data() {
        let temp = tempfile::tempdir().unwrap();
        let anchor = temp
            .path()
            .canonicalize()
            .unwrap()
            .join("Library/Application Support/Radroots/data");
        prepare_directory(&anchor).unwrap();
        assert_eq!(
            fs::metadata(&anchor).unwrap().permissions().mode() & 0o777,
            0o700
        );
        fs::write(anchor.join("sentinel"), b"keep").unwrap();
        prepare_directory(&anchor).unwrap();
        assert_eq!(fs::read(anchor.join("sentinel")).unwrap(), b"keep");
    }

    #[test]
    fn rejects_symlinks_files_and_unsafe_permissions_without_repair() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::create_dir(root.join("target")).unwrap();
        symlink(root.join("target"), root.join("link")).unwrap();
        assert!(prepare_directory(&root.join("link/data")).is_err());
        assert!(!root.join("target/data").exists());
        fs::write(root.join("file"), b"keep").unwrap();
        assert!(prepare_directory(&root.join("file/data")).is_err());
        fs::set_permissions(root.join("target"), fs::Permissions::from_mode(0o777)).unwrap();
        assert!(prepare_directory(&root.join("target/data")).is_err());
        assert_eq!(
            fs::metadata(root.join("target"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o777
        );
    }

    #[test]
    fn replacement_during_provision_is_rejected_without_deleting_either_directory() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let anchor = root.join("Radroots/data");
        prepare_directory(&anchor).unwrap();
        fs::write(anchor.join("sentinel"), b"keep").unwrap();
        let outcome = super::prepare_directory_with(&anchor, || {
            fs::rename(root.join("Radroots"), root.join("original")).unwrap();
            fs::create_dir(root.join("Radroots")).unwrap();
        });
        assert!(outcome.is_err());
        assert_eq!(
            fs::read(root.join("original/data/sentinel")).unwrap(),
            b"keep"
        );
        assert!(root.join("Radroots").is_dir());
    }

    #[test]
    fn concurrent_creators_converge_without_resetting_data() {
        let temp = tempfile::tempdir().unwrap();
        let anchor = temp.path().canonicalize().unwrap().join("Radroots/data");
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| prepare_directory(&anchor).unwrap());
            }
        });
        assert!(anchor.is_dir());
    }
    #[test]
    fn default_profile_opens_and_reopens_in_an_isolated_process() {
        const CHILD: &str = "HARVESTCIRCLE_STARTUP_TEST_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let actual = crate::commands::compatibility_descriptor();
            let expectation = crate::commands::CompatibilityExpectation {
                contract_id: actual.contract_id,
                contract_major: actual.contract_major,
                minimum_contract_minor: actual.contract_minor,
                contract_hash: actual.contract_hash,
                product_coordinate_digest: actual.product_coordinate_digest,
                snapshot_schema_version: actual.snapshot_schema_version,
                minimum_schema_version: actual.minimum_schema_version,
                maximum_schema_version: actual.current_schema_version,
            };
            let executor = tokio::runtime::Runtime::new().unwrap();
            for _ in 0..2 {
                let core = crate::HarvestCircleAppCore::open_compatible(
                    expectation.clone(),
                    crate::RuntimeOpenInputDto {
                        development_mode: false,
                        explicit_data_directory: None,
                        relay_input: crate::RelayBootstrapInputDto { endpoints: vec![] },
                    },
                )
                .unwrap();
                // A second constructor must release its own workers without
                // disturbing the first owner's lock or preventing later reopen.
                let competing = crate::HarvestCircleAppCore::open_compatible(
                    expectation.clone(),
                    crate::RuntimeOpenInputDto {
                        development_mode: false,
                        explicit_data_directory: None,
                        relay_input: crate::RelayBootstrapInputDto { endpoints: vec![] },
                    },
                );
                assert!(competing.is_err());
                executor.block_on(async {
                    core.bootstrap().await.unwrap();
                    assert!(core.shutdown_v2().await.unwrap().closed);
                });
            }
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let runtime = home.join("run");
        fs::create_dir(&runtime).unwrap();
        fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "bootstrap::tests::default_profile_opens_and_reopens_in_an_isolated_process",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("HOME", &home)
            .env("XDG_RUNTIME_DIR", &runtime)
            .env_remove("XDG_DATA_HOME")
            .env_remove("XDG_STATE_HOME")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_CACHE_HOME")
            .env_remove("HARVESTCIRCLE_DEVELOPMENT_DATA_DIR")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        #[cfg(target_os = "macos")]
        let data = home.join("Library/Application Support/Radroots/data");
        #[cfg(target_os = "linux")]
        let data = home.join(".local/share/radroots");
        assert!(
            data.join("services/harvestcircle/desktop/state.sqlite")
                .is_file()
        );
    }
}
