use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::str::FromStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    RepoAudit,
    NamespaceAudit,
    ProvenanceCheck,
    QualificationReport,
}

impl FromStr for Command {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "repo-audit" => Ok(Self::RepoAudit),
            "namespace-audit" => Ok(Self::NamespaceAudit),
            "provenance-check" => Ok(Self::ProvenanceCheck),
            "qualification-report" => Ok(Self::QualificationReport),
            _ => Err(format!("unknown xtask command: {value}")),
        }
    }
}

pub fn run(root: &Path, command: Command) -> Result<String, Vec<String>> {
    let inventory = Inventory::load(root).map_err(|finding| vec![finding])?;
    let mut findings = Vec::new();
    match command {
        Command::RepoAudit => repo_audit(root, &inventory, &mut findings),
        Command::NamespaceAudit => namespace_audit(root, &inventory, &mut findings),
        Command::ProvenanceCheck => provenance_check(root, &inventory, &mut findings),
        Command::QualificationReport => {
            repo_audit(root, &inventory, &mut findings);
            namespace_audit(root, &inventory, &mut findings);
            provenance_check(root, &inventory, &mut findings);
        }
    }
    findings.sort();
    findings.dedup();
    if findings.is_empty() {
        let inventory_kind = if inventory.git_aware {
            "git"
        } else {
            "archive"
        };
        let command_name = match command {
            Command::RepoAudit => "repo-audit",
            Command::NamespaceAudit => "namespace-audit",
            Command::ProvenanceCheck => "provenance-check",
            Command::QualificationReport => "qualification-report",
        };
        Ok(format!(
            "harvestcircle.xtask.command={command_name}\nharvestcircle.xtask.inventory={inventory_kind}\nharvestcircle.xtask.result=pass\n"
        ))
    } else {
        Err(findings)
    }
}

#[derive(Debug)]
struct Inventory {
    paths: Vec<String>,
    git_aware: bool,
}

impl Inventory {
    fn load(root: &Path) -> Result<Self, String> {
        if root.join(".git").exists() {
            let output = ProcessCommand::new("git")
                .args([
                    "-C",
                    &root.to_string_lossy(),
                    "ls-files",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                    "-z",
                ])
                .output()
                .map_err(|error| {
                    format!("unable to enumerate tracked HarvestCircle sources: {error}")
                })?;
            if !output.status.success() {
                return Err("unable to enumerate tracked HarvestCircle sources".to_owned());
            }
            let mut paths = output
                .stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| String::from_utf8_lossy(path).replace('\\', "/"))
                .filter(|path| root.join(path).symlink_metadata().is_ok())
                .collect::<Vec<_>>();
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

fn archive_paths(root: &Path, directory: &Path, paths: &mut Vec<String>) -> Result<(), String> {
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
        paths.push(relative);
        if entry
            .file_type()
            .map_err(|error| format!("unable to classify archive entry: {error}"))?
            .is_dir()
        {
            archive_paths(root, &path, paths)?;
        }
    }
    Ok(())
}

fn repo_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let required = [
        "README.md",
        "NOTICE",
        "CONTRIBUTING.md",
        "SECURITY.md",
        "LICENSE",
        "LICENSES/GPL-3.0-only.txt",
    ];
    for required_path in required {
        if !inventory.paths.iter().any(|path| path == required_path) {
            findings.push(format!(
                "{required_path}: required public repository file is missing"
            ));
        }
    }
    for path in &inventory.paths {
        let normalized = path.to_ascii_lowercase();
        if ["docs/", "spec/", ".github/", ".act/"]
            .iter()
            .any(|prefix| normalized.starts_with(prefix))
        {
            findings.push(format!("{path}: forbidden repository root"));
        }
        if fs::symlink_metadata(root.join(path))
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            findings.push(format!(
                "{path}: symbolic links are not allowed in public sources"
            ));
        }
        if normalized.starts_with("core/target/")
            || normalized.contains("/build/")
            || normalized.contains("/generated/")
            || normalized.contains("generated/uniffi")
            || [".dylib", ".so", ".dll", ".class"]
                .iter()
                .any(|suffix| normalized.ends_with(suffix))
        {
            findings.push(format!(
                "{path}: generated build output must not be source controlled"
            ));
        }
        if [".pem", ".key", ".p12", ".pfx", ".jks", ".keystore", ".env"]
            .iter()
            .any(|suffix| normalized.ends_with(suffix))
            || normalized.contains("/credentials/")
        {
            findings.push(format!("{path}: credential or secret-shaped source path"));
        }
        if is_text(path) {
            let source = read_text(root, path);
            let markers = [
                ["-----BEGIN ", "PRIVATE KEY-----"].concat(),
                ["AWS_", "SECRET_ACCESS_KEY="].concat(),
                ["gh", "p_"].concat(),
                ["sk_", "live_"].concat(),
            ];
            if markers.iter().any(|marker| source.contains(marker)) {
                findings.push(format!(
                    "{path}: credential or private-key material in source text"
                ));
            }
        }
    }
    git_source_policy(root, findings);
}

fn namespace_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let legacy = ["stu", "dio"].concat();
    let provenance_path = format!("core/provenance/{legacy}-import-v1.toml");
    let legacy_repository = format!("https://github.com/radrootslabs/{legacy}_app");
    let temporary_namespace = ["org", "radroots", "harvestcircle"].join(".");
    let inherited_preferences = [
        ["use", "radroots", "dns"].join("_"),
        ["use", "radroots", "subnets"].join("_"),
        ["vpn", "on", "demand", "enabled"].join("_"),
        ["run", "as", "exit", "node"].join("_"),
        ["automatically", "check", "for", "updates"].join("_"),
        ["update", "channel"].join("_"),
        ["last", "update", "check", "summary"].join("_"),
        ["alternate", "server", "url"].join("_"),
    ];
    for path in &inventory.paths {
        let normalized = path.to_ascii_lowercase();
        if path != &provenance_path && normalized.contains(&legacy) {
            findings.push(format!("{path}: legacy product name in source path"));
        }
        if normalized.starts_with("app/")
            && normalized.contains("/kotlin/")
            && normalized.ends_with(".kt")
        {
            let package_path = normalized
                .split_once("/kotlin/")
                .map(|(_, value)| value)
                .unwrap_or_default();
            if !package_path.starts_with("org/harvestcircle/") {
                findings.push(format!(
                    "{path}: Kotlin source is outside the final namespace"
                ));
            }
        }
        if !is_text(path) {
            continue;
        }
        let source = read_text(root, path);
        if path != &provenance_path {
            let mut inspected = source.replace(
                if path == "core/Cargo.toml" {
                    &legacy_repository
                } else {
                    "__no_exact_allowlist__"
                },
                "",
            );
            if path == "NOTICE" {
                inspected = inspected
                    .replace(
                        &format!("Radroots {} application work", title_case(&legacy)),
                        "",
                    )
                    .replace(&provenance_path, "");
            }
            if inspected.to_ascii_lowercase().contains(&legacy) {
                findings.push(format!(
                    "{path}: legacy product name outside the exact provenance allowlist"
                ));
            }
        }
        if source.contains(&temporary_namespace)
            || source.contains(&temporary_namespace.replace('.', "/"))
        {
            findings.push(format!("{path}: temporary product namespace"));
        }
        let production_kotlin = path.ends_with(".kt")
            && path.starts_with("app/")
            && ["/src/main/", "/src/commonMain/", "/src/desktopMain/"]
                .iter()
                .any(|segment| path.contains(segment));
        let bounded_health = path
            == "app/desktop/src/main/kotlin/org/harvestcircle/desktop/Main.kt"
            && source.contains("HEALTH_CHECK_ARGUMENT")
            && source.contains("withTimeout(HEALTH_TIMEOUT_MILLIS)");
        if production_kotlin && source.contains(&["run", "Blocking"].concat()) && !bounded_health {
            findings.push(format!(
                "{path}: blocking coroutine bridge in application source"
            ));
        }
        if production_kotlin
            && (source.contains(&["Atomic", "Long"].concat())
                || source.contains(&["desktop", "-operation:"].concat()))
        {
            findings.push(format!("{path}: process-local operation counter"));
        }
        if path.starts_with("app/shared/src/commonMain/")
            && [
                ["org.harvestcircle.", "ffi"].concat(),
                ["com.sun.", "jna"].concat(),
                "java.".to_owned(),
                "javax.".to_owned(),
            ]
            .iter()
            .any(|marker| source.contains(marker))
        {
            findings.push(format!(
                "{path}: platform dependency in shared common source"
            ));
        }
        let lowercase = source.to_ascii_lowercase();
        for token in &inherited_preferences {
            if lowercase.contains(token) {
                findings.push(format!("{path}: inherited non-product preference {token}"));
            }
        }
        if production_kotlin && lowercase.contains(&["nsec", "1"].concat()) {
            findings.push(format!("{path}: secret key literal in production Kotlin"));
        }
    }
}

fn provenance_check(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let legacy = ["stu", "dio"].concat();
    let legacy_repository = format!("https://github.com/radrootslabs/{legacy}_app");
    let provenance_path = format!("core/provenance/{legacy}-import-v1.toml");
    let cargo = read_text(root, "core/Cargo.toml");
    let repository_line = format!("repository = \"{legacy_repository}\"");
    if cargo
        .lines()
        .filter(|line| line.trim() == repository_line)
        .count()
        != 1
    {
        findings.push("core/Cargo.toml: legacy repository allowlist must be exact".to_owned());
    }
    let provenance = read_text(root, &provenance_path);
    if !provenance.contains(&format!("source_repository = \"{legacy_repository}\""))
        || !provenance
            .contains("canonical_radroots_revision = \"09065a610d95e57acdc895a14c07580fa099e7c3\"")
        || !provenance
            .contains("foundation_baseline = \"a2038b3e25b9e34f0b8fd001f26a8ed10b5772cb\"")
    {
        findings.push(format!(
            "{provenance_path}: exact source provenance changed"
        ));
    }
    let coordinates = properties(&read_text(
        root,
        "config/product/harvestcircle-v1.properties",
    ));
    let uniffi = read_text(root, "core/crates/harvestcircle_ffi/uniffi.toml");
    let ffi_package = coordinates
        .get("ffi.kotlin_package")
        .map(String::as_str)
        .unwrap_or_default();
    let cdylib = coordinates
        .get("ffi.cdylib_name")
        .map(String::as_str)
        .unwrap_or_default();
    if !uniffi.contains("[crates.harvestcircle_ffi.bindings.kotlin]")
        || !uniffi.contains(&format!("package_name = \"{ffi_package}\""))
        || !uniffi.contains(&format!("cdylib_name = \"{cdylib}\""))
    {
        findings.push(
            "core/crates/harvestcircle_ffi/uniffi.toml: final FFI identity changed".to_owned(),
        );
    }
    let baseline = read_text(root, "core/compatibility/harvestcircle-ffi-v4.properties");
    if !baseline.contains("contract.id=harvestcircle-desktop-ffi-v4")
        || !baseline.contains("contract.major=4")
    {
        findings.push(
            "core/compatibility/harvestcircle-ffi-v4.properties: FFI v4 identity changed"
                .to_owned(),
        );
    }
    let shared_build = read_text(root, "app/shared/build.gradle.kts");
    if !shared_build.contains("id(\"org.harvestcircle.build.kmp-shared\")")
        || ["androidTarget", "iosArm", "iosX", "js(", "wasm"]
            .iter()
            .any(|marker| shared_build.contains(marker))
    {
        findings.push("app/shared/build.gradle.kts: shared KMP target boundary changed".to_owned());
    }
    if !inventory.paths.iter().any(|path| path == &provenance_path) {
        findings.push(format!(
            "{provenance_path}: source provenance file is missing"
        ));
    }
}

fn git_source_policy(root: &Path, findings: &mut Vec<String>) {
    let deny = read_text(root, "core/deny.toml");
    if !deny
        .lines()
        .any(|line| line.trim() == "required-git-spec = \"rev\"")
    {
        findings
            .push("core/deny.toml: cargo-deny must require revision-pinned Git sources".to_owned());
    }
    let allowed_git = quoted_values(section_value(&deny, "allow-git"));
    if allowed_git.is_empty() {
        findings.push("core/deny.toml: cargo-deny Git allowlist is empty".to_owned());
    }
    let mut inspected = false;
    for manifest in cargo_manifests(root.join("core")) {
        let source = fs::read_to_string(&manifest).unwrap_or_default();
        for (index, line) in source
            .lines()
            .enumerate()
            .filter(|(_, line)| line.contains("git"))
        {
            let Some(git) = attribute(line, "git") else {
                continue;
            };
            inspected = true;
            let relative_path =
                relative(root, &manifest).unwrap_or_else(|_| manifest.display().to_string());
            if !allowed_git.contains(&git) {
                findings.push(format!(
                    "{relative_path}:{}: Git dependency source is not allowlisted",
                    index + 1
                ));
            }
            if line.contains("branch =") || line.contains("tag =") {
                findings.push(format!(
                    "{relative_path}:{}: Git dependency uses a branch or tag",
                    index + 1
                ));
            }
            let revision = attribute(line, "rev").unwrap_or_default();
            if !is_lower_hex(&revision, 40) {
                findings.push(format!(
                    "{relative_path}:{}: Git dependency must use one full revision pin",
                    index + 1
                ));
            }
            if git == "https://github.com/rust-nostr/nostr.git"
                && revision != "5bba5163eb77107f82c4a8262cf29d7f33a73219"
            {
                findings.push("core/Cargo.toml: direct rust-nostr revision changed".to_owned());
            }
        }
    }
    if !inspected {
        findings.push("core: no revision-pinned Git dependencies were inspected".to_owned());
    }
    for line in read_text(root, "core/Cargo.lock")
        .lines()
        .filter(|line| line.starts_with("source = \"git+"))
    {
        let immutable = line.rsplit_once("?rev=").is_some_and(|(_, suffix)| {
            suffix.len() == 82
                && suffix.as_bytes().get(40) == Some(&b'#')
                && suffix.ends_with('"')
                && is_lower_hex(&suffix[..40], 40)
                && is_lower_hex(&suffix[41..81], 40)
        });
        if !immutable {
            findings.push(format!(
                "core/Cargo.lock: Git source is not immutable: {line}"
            ));
        }
    }
}

fn cargo_manifests(core: PathBuf) -> Vec<PathBuf> {
    let mut manifests = vec![core.join("Cargo.toml")];
    if let Ok(entries) = fs::read_dir(core.join("crates")) {
        for entry in entries.flatten() {
            let manifest = entry.path().join("Cargo.toml");
            if manifest.is_file() {
                manifests.push(manifest);
            }
        }
    }
    manifests.sort();
    manifests
}

fn properties(source: &str) -> std::collections::BTreeMap<String, String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                None
            } else {
                line.split_once('=')
                    .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
            }
        })
        .collect()
}

fn section_value<'a>(source: &'a str, key: &str) -> &'a str {
    source
        .split_once(key)
        .map(|(_, tail)| tail.split_once(']').map_or(tail, |(value, _)| value))
        .unwrap_or_default()
}

fn quoted_values(source: &str) -> BTreeSet<String> {
    source
        .split('"')
        .enumerate()
        .filter(|(index, _)| index % 2 == 1)
        .map(|(_, value)| value.to_owned())
        .collect()
}

fn attribute(source: &str, key: &str) -> Option<String> {
    let tail = source.split_once(&format!("{key} = \""))?.1;
    Some(tail.split_once('"')?.0.to_owned())
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn title_case(value: &str) -> String {
    let mut characters = value.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + characters.as_str()
    })
}

fn is_text(relative: &str) -> bool {
    let name = Path::new(relative)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    let extension = Path::new(relative)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    [
        "gradle",
        "json",
        "kt",
        "kts",
        "lock",
        "md",
        "properties",
        "rs",
        "sql",
        "toml",
        "txt",
        "xml",
        "yaml",
        "yml",
    ]
    .contains(&extension)
        || [
            ".gitattributes",
            ".gitignore",
            "AGENTS.md",
            "LICENSE",
            "Makefile",
            "NOTICE",
            "gradlew",
            "gradlew.bat",
        ]
        .contains(&name)
}

fn read_text(root: &Path, relative: &str) -> String {
    fs::read_to_string(root.join(relative)).unwrap_or_default()
}

fn relative(root: &Path, path: &Path) -> Result<String, String> {
    path.strip_prefix(root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .map_err(|error| format!("{} is outside {}: {error}", path.display(), root.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn commands_are_exact_and_unknown_values_fail_closed() {
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
        write(&root, ".github/workflows/remote.yml", "fixture\n");
        let inventory = Inventory {
            paths: vec![
                "README.md".to_owned(),
                ".github/workflows/remote.yml".to_owned(),
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
    fn namespace_policy_rejects_legacy_temporary_and_platform_sources() {
        let root = fixture("namespace");
        let legacy = ["stu", "dio"].concat();
        write(
            &root,
            &format!("app/{legacy}/Leak.kt"),
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
                .any(|finding| finding.contains("legacy product name"))
        );
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

    #[cfg(unix)]
    #[test]
    fn repository_policy_rejects_symbolic_links() {
        use std::os::unix::fs::symlink;
        let root = fixture("symlink");
        write(&root, "outside.txt", "outside\n");
        fs::create_dir_all(root.join("app")).expect("create app");
        symlink(root.join("outside.txt"), root.join("app/escape.txt")).expect("create symlink");
        let inventory = Inventory::load(&root).expect("archive inventory");
        let mut findings = Vec::new();
        repo_audit(&root, &inventory, &mut findings);
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("symbolic links"))
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn mutable_git_dependency_and_provenance_mutation_fail_closed() {
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
        git_source_policy(&root, &mut findings);
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
                .any(|finding| finding.contains("exact source provenance changed"))
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

    fn write(root: &Path, relative: &str, source: &str) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
        fs::write(path, source).expect("write fixture");
    }
}
