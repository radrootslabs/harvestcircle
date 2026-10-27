use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::Command as ProcessCommand;

pub(super) fn validate_product_root(root: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(root)
        .map_err(|error| format!("HarvestCircle product root cannot be inspected: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(
            "HarvestCircle product root must be a directory, not a symbolic link".to_owned(),
        );
    }
    let root = root
        .canonicalize()
        .map_err(|error| format!("HarvestCircle product root cannot be resolved: {error}"))?;
    for marker in [
        "Makefile",
        "settings.gradle.kts",
        "build.gradle.kts",
        "gradlew",
        "config/product/harvestcircle-v1.properties",
        "core/Cargo.toml",
        "core/Cargo.lock",
        "core/rust-toolchain.toml",
        "tools/xtask/Cargo.toml",
        "tools/xtask/Cargo.lock",
        "core/crates/harvestcircle_domain/Cargo.toml",
        "core/crates/harvestcircle_domain/src/lib.rs",
    ] {
        validate_git_inventory_path(&root, Path::new(marker))
            .map_err(|finding| format!("invalid HarvestCircle product root: {finding}"))?;
    }
    for (manifest, identity) in [
        (
            "core/crates/harvestcircle_domain/Cargo.toml",
            "harvestcircle_domain",
        ),
        ("tools/xtask/Cargo.toml", "harvestcircle_xtask"),
    ] {
        let source = bounded_no_follow_bytes(&root, Path::new(manifest), 1024 * 1024)
            .map_err(|finding| format!("invalid HarvestCircle product root: {finding}"))?;
        let source = std::str::from_utf8(&source)
            .map_err(|_| format!("invalid HarvestCircle product root: {manifest} is not UTF-8"))?;
        let manifest: toml::Value = toml::from_str(source)
            .map_err(|error| format!("invalid HarvestCircle product root: {manifest}: {error}"))?;
        if manifest
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
            != Some(identity)
        {
            return Err(format!(
                "invalid HarvestCircle product root: {manifest} has the wrong product identity"
            ));
        }
    }
    Ok(root)
}

pub(super) fn git_command(root: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new("git");
    command.arg("-C").arg(root);
    for variable in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    ] {
        command.env_remove(variable);
    }
    command
}

pub(super) fn has_git_context(root: &Path) -> Result<bool, String> {
    for directory in root.ancestors() {
        match fs::symlink_metadata(directory.join(".git")) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
                    return Err("Git metadata is not a regular file or directory".to_owned());
                }
                let output = git_command(root)
                    .args(["rev-parse", "--is-inside-work-tree"])
                    .output()
                    .map_err(|error| format!("unable to inspect Git context: {error}"))?;
                if !output.status.success() || output.stdout != b"true\n" {
                    return Err("invalid Git metadata for HarvestCircle inventory".to_owned());
                }
                return Ok(true);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(format!("unable to inspect Git metadata: {error}")),
        }
    }
    Ok(false)
}

#[derive(Debug)]
pub(super) struct Inventory {
    pub(super) paths: Vec<String>,
    pub(super) git_aware: bool,
}

impl Inventory {
    pub(super) fn load(root: &Path) -> Result<Self, String> {
        if has_git_context(root)? {
            let output = git_command(root)
                .args([
                    "ls-files",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                    "-z",
                    "--",
                    ".",
                ])
                .output()
                .map_err(|error| {
                    format!("unable to enumerate tracked HarvestCircle sources: {error}")
                })?;
            if !output.status.success() {
                return Err("unable to enumerate tracked HarvestCircle sources".to_owned());
            }
            let mut paths = Vec::new();
            for raw_path in output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
            {
                let path = String::from_utf8(raw_path.to_vec())
                    .map_err(|_| "Git inventory path is not valid UTF-8".to_owned())?;
                validate_git_inventory_path(root, Path::new(&path))?;
                paths.push(path);
            }
            paths.sort();
            paths.dedup();
            Ok(Self {
                paths,
                git_aware: true,
            })
        } else {
            let mut paths = Vec::new();
            archive_paths(root, root, &mut paths)?;
            paths.sort();
            paths.dedup();
            Ok(Self {
                paths,
                git_aware: false,
            })
        }
    }
}

pub(super) fn validate_git_inventory_path(root: &Path, relative: &Path) -> Result<(), String> {
    if relative.is_absolute()
        || relative.components().next().is_none()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "{}: Git inventory path must be normalized and relative",
            relative.display()
        ));
    }
    let components = relative.components().collect::<Vec<_>>();
    let mut current = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(format!(
                    "{}: Git inventory path is missing",
                    relative.display()
                ));
            }
            Err(error) => {
                return Err(format!(
                    "{}: unable to inspect Git inventory path: {error}",
                    relative.display()
                ));
            }
        };
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "{}: Git inventory path traverses a symbolic link",
                relative.display()
            ));
        }
        if index + 1 < components.len() {
            if !metadata.is_dir() {
                return Err(format!(
                    "{}: Git inventory path parent is not a directory",
                    relative.display()
                ));
            }
        } else if !metadata.is_file() {
            return Err(format!(
                "{}: Git inventory path is not a regular file",
                relative.display()
            ));
        }
    }
    Ok(())
}

pub(super) fn archive_paths(
    root: &Path,
    directory: &Path,
    paths: &mut Vec<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(directory).map_err(|error| {
        format!(
            "{}: unable to read archive inventory: {error}",
            directory.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("archive inventory entry failed: {error}"))?;
        let path = entry.path();
        let relative = relative(root, &path)?;
        let first = relative.split('/').next().unwrap_or_default();
        if matches!(
            first,
            ".git" | ".gradle" | ".kotlin" | ".idea" | "build" | "target" | "out"
        ) || relative
            .split('/')
            .any(|part| matches!(part, "build" | "target" | "out"))
        {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("unable to classify archive entry: {error}"))?;
        if file_type.is_symlink() {
            return Err(format!(
                "{relative}: archive inventory traverses a symbolic link"
            ));
        }
        if !file_type.is_dir() && !file_type.is_file() {
            return Err(format!(
                "{relative}: archive inventory path is not a regular file"
            ));
        }
        paths.push(relative.clone());
        if file_type.is_dir() {
            archive_paths(root, &path, paths)?;
        } else {
            validate_git_inventory_path(root, Path::new(&relative))?;
        }
    }
    Ok(())
}

pub(super) fn bounded_no_follow_bytes(
    root: &Path,
    relative: &Path,
    maximum: u64,
) -> Result<Vec<u8>, String> {
    if relative.is_absolute()
        || relative.components().next().is_none()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("path must be normalized and relative".to_owned());
    }
    let components = relative.components().collect::<Vec<_>>();
    let mut path = root.to_path_buf();
    let mut admitted = None;
    for (index, component) in components.iter().enumerate() {
        path.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("unable to inspect {}: {error}", relative.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "path traverses a symbolic link: {}",
                relative.display()
            ));
        }
        if index + 1 == components.len() {
            if !metadata.is_file() {
                return Err(format!(
                    "path is not a regular file: {}",
                    relative.display()
                ));
            }
            if metadata.len() > maximum {
                return Err(format!("file exceeds byte limit: {}", relative.display()));
            }
            admitted = Some(metadata);
        } else if !metadata.is_dir() {
            return Err(format!(
                "path parent is not a directory: {}",
                relative.display()
            ));
        }
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    let mut file = options.open(&path).map_err(|error| {
        format!(
            "unable to open {} without following links: {error}",
            relative.display()
        )
    })?;
    let opened = file
        .metadata()
        .map_err(|error| format!("unable to inspect opened {}: {error}", relative.display()))?;
    if !opened.is_file() || opened.len() > maximum {
        return Err(format!(
            "opened path is not a bounded regular file: {}",
            relative.display()
        ));
    }
    #[cfg(unix)]
    if admitted
        .as_ref()
        .is_some_and(|metadata| metadata.dev() != opened.dev() || metadata.ino() != opened.ino())
    {
        return Err(format!(
            "path identity changed before open: {}",
            relative.display()
        ));
    }

    let mut bytes = Vec::with_capacity(opened.len() as usize);
    file.by_ref()
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("unable to read {}: {error}", relative.display()))?;
    if bytes.len() as u64 > maximum {
        return Err(format!("file exceeds byte limit: {}", relative.display()));
    }
    let completed = file
        .metadata()
        .map_err(|error| format!("unable to revalidate {}: {error}", relative.display()))?;
    if completed.len() != bytes.len() as u64 {
        return Err(format!(
            "file changed while it was read: {}",
            relative.display()
        ));
    }
    #[cfg(unix)]
    if opened.dev() != completed.dev() || opened.ino() != completed.ino() {
        return Err(format!(
            "file identity changed while it was read: {}",
            relative.display()
        ));
    }
    Ok(bytes)
}

pub(super) fn relative(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|error| format!("{} is outside {}: {error}", path.display(), root.display()))?;
    relative
        .components()
        .map(|component| match component {
            Component::Normal(value) => value
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| "archive inventory path is not valid UTF-8".to_owned()),
            _ => Err("archive inventory path must be normalized and relative".to_owned()),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("/"))
}
