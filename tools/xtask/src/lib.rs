use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::str::FromStr;

#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    DesignSourceAudit,
    RepoAudit,
    NamespaceAudit,
    ProvenanceCheck,
    QualificationReport,
}

impl FromStr for Command {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "design-source-audit" => Ok(Self::DesignSourceAudit),
            "repo-audit" => Ok(Self::RepoAudit),
            "namespace-audit" => Ok(Self::NamespaceAudit),
            "provenance-check" => Ok(Self::ProvenanceCheck),
            "qualification-report" => Ok(Self::QualificationReport),
            _ => Err(format!("unknown xtask command: {value}")),
        }
    }
}

pub fn run(root: &Path, command: Command) -> Result<String, Vec<String>> {
    let build_mode =
        std::env::var("HARVESTCIRCLE_BUILD_MODE").unwrap_or_else(|_| "standalone".to_owned());
    if command == Command::QualificationReport
        && !matches!(build_mode.as_str(), "standalone" | "governed")
    {
        return Err(vec![format!(
            "unknown qualification build mode: {build_mode}"
        )]);
    }
    let inventory = Inventory::load(root).map_err(|finding| vec![finding])?;
    let mut findings = Vec::new();
    match command {
        Command::DesignSourceAudit => design_source_audit(root, &inventory, &mut findings),
        Command::RepoAudit => repo_audit(root, &inventory, &mut findings),
        Command::NamespaceAudit => namespace_audit(root, &inventory, &mut findings),
        Command::ProvenanceCheck => provenance_check(root, &inventory, &mut findings),
        Command::QualificationReport => {
            repo_audit(root, &inventory, &mut findings);
            namespace_audit(root, &inventory, &mut findings);
            provenance_check(root, &inventory, &mut findings);
            design_source_audit(root, &inventory, &mut findings);
            product_shell_audit(root, &inventory, &mut findings);
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
            Command::DesignSourceAudit => "design-source-audit",
            Command::RepoAudit => "repo-audit",
            Command::NamespaceAudit => "namespace-audit",
            Command::ProvenanceCheck => "provenance-check",
            Command::QualificationReport => "qualification-report",
        };
        let mode = if command == Command::QualificationReport {
            format!("harvestcircle.build.mode={build_mode}\n")
        } else {
            String::new()
        };
        Ok(format!(
            "harvestcircle.xtask.command={command_name}\nharvestcircle.xtask.inventory={inventory_kind}\n{mode}harvestcircle.xtask.result=pass\n"
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

fn validate_git_inventory_path(root: &Path, relative: &Path) -> Result<(), String> {
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
        "LICENSES/OFL-1.1.txt",
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
        if is_forbidden_documentation_or_workflow_path(&normalized) {
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
    native_runtime_boundary(root, findings);
}

fn native_runtime_boundary(root: &Path, findings: &mut Vec<String>) {
    let domain_lib = root.join("core/crates/harvestcircle_domain/src/lib.rs");
    if !domain_lib.is_file() {
        return;
    }

    if root
        .join("core/crates/harvestcircle_domain/src/relay.rs")
        .exists()
        || read_text(root, "core/crates/harvestcircle_domain/src/lib.rs").contains("mod relay")
    {
        findings
            .push("harvestcircle_domain: duplicate relay policy surface is forbidden".to_owned());
    }

    let nostr_manifest = read_text(root, "core/crates/harvestcircle_nostr/Cargo.toml");
    let production_manifest = nostr_manifest
        .split_once("[dev-dependencies]")
        .map_or(nostr_manifest.as_str(), |(production, _)| production);
    if production_manifest.contains("nostr-sdk") {
        findings.push(
            "harvestcircle_nostr: production nostr-sdk connection authority is forbidden"
                .to_owned(),
        );
    }
    let nostr_client = read_text(root, "core/crates/harvestcircle_nostr/src/client.rs");
    for required in [
        "radroots_transport_nostr::{Config, NostrTransport, RelayEndpoint, RelayProfile}",
        "parse_verified_kind0",
        "FetchBounds::new(MAX_PROFILE_EVENTS_PER_FETCH",
    ] {
        if !nostr_client.contains(required) {
            findings.push(format!(
                "harvestcircle_nostr: governed transport boundary is missing {required}"
            ));
        }
    }

    for (path, forbidden) in [
        ("core/crates/harvestcircle_ffi/src/commands.rs", "OnceLock"),
        (
            "core/crates/harvestcircle_ffi/src/commands.rs",
            "PoisonError::into_inner",
        ),
        (
            "core/crates/harvestcircle_ffi/src/observer.rs",
            "PoisonError::into_inner",
        ),
        (
            "core/crates/harvestcircle_application/src/app_core.rs",
            "PoisonError::into_inner",
        ),
        (
            "core/crates/harvestcircle_application/src/custody.rs",
            "PoisonError::into_inner",
        ),
        (
            "core/crates/harvestcircle_application/src/secrets.rs",
            "PoisonError::into_inner",
        ),
    ] {
        if read_text(root, path).contains(forbidden) {
            findings.push(format!("{path}: forbidden runtime boundary {forbidden}"));
        }
    }

    let runtime = read_text(root, "core/crates/harvestcircle_ffi/src/host_runtime.rs");
    let keyring = read_text(root, "core/crates/harvestcircle_ffi/src/keyring_worker.rs");
    for (source, required, owner) in [
        (&runtime, "pub(crate) struct HostRuntime", "host runtime"),
        (&runtime, "pub(crate) async fn shutdown", "host runtime"),
        (
            &keyring,
            "const KEYRING_QUEUE_CAPACITY: usize = 8",
            "keyring worker",
        ),
        (
            &keyring,
            "pub(crate) struct BoundedKeyringWorker",
            "keyring worker",
        ),
        (
            &keyring,
            "use tokio::sync::{oneshot, watch}",
            "keyring worker",
        ),
        (&keyring, "response_receiver.await", "keyring worker"),
        (
            &keyring,
            "const KEYRING_SHUTDOWN_DEADLINE: Duration = Duration::from_secs(30)",
            "keyring worker",
        ),
        (&keyring, "OPERATION_QUEUED", "keyring worker"),
        (&keyring, "OPERATION_STARTED", "keyring worker"),
        (&keyring, "OPERATION_COMPLETED", "keyring worker"),
        (&keyring, "OPERATION_CANCELLED", "keyring worker"),
    ] {
        if !source.contains(required) {
            findings.push(format!(
                "harvestcircle_ffi: {owner} contract is missing {required}"
            ));
        }
    }
    if keyring.contains("response_receiver.recv") {
        findings
            .push("harvestcircle_ffi: keyring response blocks a Tokio runtime thread".to_owned());
    }
    if keyring.contains("std::sync::mpsc::Receiver") {
        findings.push("harvestcircle_ffi: keyring response exposes a blocking receiver".to_owned());
    }
}

fn namespace_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let legacy = ["stu", "dio"].concat();
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
        if normalized.contains(&legacy) {
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
        if source.to_ascii_lowercase().contains(&legacy) {
            findings.push(format!("{path}: legacy product name in source text"));
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
        let secret_marker = ["nsec", "1"].concat();
        let inspected_secret = lowercase.replace(&format!("{secret_marker}…"), "");
        if production_kotlin && inspected_secret.contains(&secret_marker) {
            findings.push(format!("{path}: secret key literal in production Kotlin"));
        }
    }
}

fn product_shell_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let required = [
        "app/shared/src/commonMain/kotlin/org/harvestcircle/product/SurfaceRegistry.kt",
        "app/shared/src/commonMain/kotlin/org/harvestcircle/navigation/Navigation.kt",
        "app/shared/src/commonMain/kotlin/org/harvestcircle/appearance/AppearanceState.kt",
        "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/theme/HarvestCircleTheme.kt",
        "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/component/action/HarvestCircleButton.kt",
        "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/component/feedback/HarvestCircleBadge.kt",
        "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/layout/HarvestCircleAppFrame.kt",
        "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/HarvestCircleShell.kt",
        "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/FoundationTodayScreen.kt",
        "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/FoundationNetworkScreen.kt",
        "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/FoundationSettingsScreen.kt",
        "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/ShellAccessibility.kt",
    ];
    for path in required {
        if !inventory.paths.iter().any(|candidate| candidate == path) {
            findings.push(format!("{path}: required product-shell source is missing"));
        }
    }
    let regression_matrix: &[(&str, &[&str])] = &[
        (
            "app/shared/src/commonTest/kotlin/org/harvestcircle/application/HarvestCirclePresenterTest.kt",
            &["hcSc001", "hcSc002"],
        ),
        (
            "app/shared/src/commonTest/kotlin/org/harvestcircle/application/HarvestCircleShellPresenterTest.kt",
            &["hcSc003", "HcSc004"],
        ),
        (
            "app/shared/src/commonTest/kotlin/org/harvestcircle/application/ReferenceInputPolicyTest.kt",
            &["hcSc005", "hcSc006", "hcSc007"],
        ),
        (
            "app/design_system/src/commonTest/kotlin/org/harvestcircle/designsystem/theme/HarvestCircleColorContrastTest.kt",
            &["hcSc008", "hcSc009"],
        ),
        (
            "app/shared/src/desktopTest/kotlin/org/harvestcircle/ui/shell/OwnedControlsUiTest.kt",
            &["hcSc010"],
        ),
        (
            "app/shared/src/desktopTest/kotlin/org/harvestcircle/ui/shell/ShellAccessibilityUiTest.kt",
            &["hcSc011"],
        ),
        (
            "app/shared/src/commonTest/kotlin/org/harvestcircle/application/ShellOverlaysTest.kt",
            &["hcSc012", "hcSl001", "hcSl006"],
        ),
        (
            "app/shared/src/commonTest/kotlin/org/harvestcircle/application/ReferenceInputPolicyTest.kt",
            &["hcSl001"],
        ),
        (
            "app/shared/src/commonTest/kotlin/org/harvestcircle/application/HarvestCirclePresenterTest.kt",
            &[
                "hcSl002", "hcSl003", "hcSl004", "hcSl005", "hcEx001", "hcEx002", "hcEx003",
            ],
        ),
        (
            "app/shared/src/desktopTest/kotlin/org/harvestcircle/ui/shell/BootstrapIdentityEntryTest.kt",
            &["hcEx004"],
        ),
        (
            "app/shared/src/commonTest/kotlin/org/harvestcircle/application/HarvestCircleShellPresenterTest.kt",
            &["hcSl001", "hcSl006"],
        ),
        (
            "app/shared/src/commonTest/kotlin/org/harvestcircle/application/ImportSecretDraftTest.kt",
            &["hcSl005"],
        ),
        (
            "app/shared/src/desktopTest/kotlin/org/harvestcircle/ui/shell/FoundationOverlayHostTest.kt",
            &["hcSl006"],
        ),
    ];
    for (path, markers) in regression_matrix {
        let source = read_text(root, path);
        for marker in *markers {
            if !source.contains(marker) {
                findings.push(format!(
                    "{path}: required shell-security regression marker is missing: {marker}"
                ));
            }
        }
    }
    let closure_source_contract: &[(&str, &[&str])] = &[
        (
            "app/shared/src/commonMain/kotlin/org/harvestcircle/application/ReferenceInputPolicy.kt",
            &["data object AmbiguousHex", "hasAmbiguousHexShape"],
        ),
        (
            "app/shared/src/commonMain/kotlin/org/harvestcircle/application/ImportSecretDraft.kt",
            &[
                "class ImportSecretDraft private constructor",
                "private var characters: CharArray?",
            ],
        ),
        (
            "app/shared/src/commonMain/kotlin/org/harvestcircle/application/PresentationModels.kt",
            &[
                "val importDraft: ImportSecretDraft",
                "class EditImportDraft private constructor",
            ],
        ),
        (
            "app/shared/src/commonMain/kotlin/org/harvestcircle/application/HarvestCirclePresenter.kt",
            &[
                "PendingRemovalLease",
                "removalMutex",
                "expireRemovalLease",
                "releaseClaimedRemoval",
                "PresenterClosePhase.TransferredToShutdown",
                "ImportSecretDraft",
            ],
        ),
        (
            "app/shared/src/commonMain/kotlin/org/harvestcircle/application/ShellOverlays.kt",
            &["ReferenceInputAdmission.AmbiguousHex"],
        ),
        (
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/FoundationOverlayHost.kt",
            &["val overlayBusy = (overlay as? FoundationOverlay.ConfirmAction)?.busy == true"],
        ),
        (
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/BootstrapIdentityEntry.kt",
            &[
                "The secret is held only for this import.",
                "It is cleared after it is sent to the local native runtime.",
            ],
        ),
    ];
    for (path, markers) in closure_source_contract {
        let source = read_text(root, path);
        for marker in *markers {
            if !source.contains(marker) {
                findings.push(format!(
                    "{path}: required secret-lifecycle closure source marker is missing: {marker}"
                ));
            }
        }
    }
    let presenter_tests = read_text(
        root,
        "app/shared/src/commonTest/kotlin/org/harvestcircle/application/HarvestCirclePresenterTest.kt",
    );
    for forbidden in ["Thread.sleep", "kotlinx.coroutines.delay("] {
        if presenter_tests.contains(forbidden) {
            findings.push(format!(
                "automatic-expiry tests must use virtual time, not {forbidden}"
            ));
        }
    }
    let bootstrap_entry = read_text(
        root,
        "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/BootstrapIdentityEntry.kt",
    );
    let retired_copy = [
        "The secret is sent directly to the local native runtime ",
        "and is not retained in the interface.",
    ]
    .concat();
    if bootstrap_entry.contains(&retired_copy) {
        findings.push("Bootstrap identity entry retains retired secret-custody copy".to_owned());
    }
    let locked_copy = [
        (
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/HarvestCircleShell.kt",
            &[
                "Coordinate local food with clear, signed terms.",
                "You do not need a HarvestCircle account.",
                "Open source · Nostr-based · No managed service required",
            ][..],
        ),
        (
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/FoundationTodayScreen.kt",
            &[
                "No active commitments",
                "Explore nearby buying circles or open a shared Nostr reference.",
                "Not available in this build.",
            ][..],
        ),
        (
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/FoundationNetworkScreen.kt",
            &[
                "Overview",
                "Identity",
                "Public relays",
                "Runtime",
                "No managed HarvestCircle service is configured.",
            ][..],
        ),
        (
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/FoundationSettingsScreen.kt",
            &[
                "Appearance",
                "Project",
                "Theme",
                "Text size",
                "Motion",
                "FFI contract",
                "Storage schema",
            ][..],
        ),
    ];
    for (path, expected) in locked_copy {
        let source = read_text(root, path);
        for text in expected {
            if !source.contains(text) {
                findings.push(format!(
                    "{path}: locked product-shell copy is missing: {text}"
                ));
            }
        }
    }
    for path in &inventory.paths {
        if !is_production_kotlin(path) {
            continue;
        }
        let source = read_text(root, path);
        let normalized_path = path.to_ascii_lowercase();
        let compact = source
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();
        for (shape, diagnostic) in [
            ("funShellText(", "superseded shell text adapter"),
            ("funShellButton(", "superseded shell button adapter"),
            ("funShellTextField(", "superseded shell field adapter"),
            (
                "enumclassShellTextRole",
                "superseded shell text-role adapter",
            ),
            (
                "enumclassShellButtonKind",
                "superseded shell button-kind adapter",
            ),
        ] {
            if compact.contains(shape) {
                findings.push(format!("{path}: {diagnostic}"));
            }
        }
        if source.contains("androidx.compose.material") {
            findings.push(format!(
                "{path}: Material component dependency is forbidden"
            ));
        }
        for (shape, diagnostic) in [
            (
                "dataobjectConfirmIdentityRemoval",
                "retired parameterless confirmation source shape",
            ),
            (
                "dataobjectCancelIdentityRemoval",
                "retired parameterless confirmation source shape",
            ),
            (
                "isOverlayIntent.EditReference->classifyNostrReference(",
                "parser-on-edit source shape",
            ),
            (
                "OverlayIntent.Open(FoundationOverlay.OpenNostrReference(",
                "prefilled reference ingress source shape",
            ),
            (
                "selected=true,enabled=false",
                "selected-as-disabled source shape",
            ),
            (
                "valimportDraft:String",
                "raw String import-draft custody source shape",
            ),
            (
                "dataclassEditImportDraft",
                "copyable import-draft intent source shape",
            ),
        ] {
            if compact.contains(shape) {
                findings.push(format!("{path}: {diagnostic}"));
            }
        }
        if path
            == "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/FoundationOverlayHost.kt"
            && compact.contains(
                "funFoundationOverlayHost(state:OverlayState,status:ShellStatusModel,busy:Boolean",
            )
        {
            findings.push(format!(
                "{path}: global busy state must not enter the overlay host"
            ));
        }
        if path == "app/shared/src/commonMain/kotlin/org/harvestcircle/application/ShellOverlays.kt"
            && compact.contains("state.identity.busy")
        {
            findings.push(format!(
                "{path}: unrelated identity busy state must not gate overlay admission"
            ));
        }
        if normalized_path.ends_with("/harvestcirclescreen.kt") {
            findings.push(format!("{path}: superseded product-shell screen path"));
        }
        for marker in [
            "home-screen",
            "inactive-identities",
            "WindowBackgroundColor",
            "ButtonBackgroundColor",
            "InputBackgroundColor",
        ] {
            if source.contains(marker) {
                findings.push(format!("{path}: superseded product-shell marker {marker}"));
            }
        }
        if is_production_compose(path, &source)
            && source.contains("focusRing: HarvestCircleFocusRing = HarvestCircleFocusRing.None")
        {
            findings.push(format!(
                "{path}: interactive control defaults to a hidden keyboard focus ring"
            ));
        }
        let approved_color_adapter = path.starts_with(
            "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/theme/color/",
        ) || path
            == "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/shell/HarvestCircleShellVisuals.kt";
        if is_production_compose(path, &source)
            && !approved_color_adapter
            && contains_direct_call(&compact, "Color(")
        {
            findings.push(format!(
                "{path}: hard-coded Compose color outside the theme adapter"
            ));
        }
        let approved_text_primitive = matches!(
            path.as_str(),
            "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/primitive/HarvestCircleText.kt"
                | "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/shell/HarvestCircleShellText.kt"
        );
        let approved_input_primitive = matches!(
            path.as_str(),
            "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/component/input/HarvestCircleTextField.kt"
                | "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/shell/HarvestCircleShellControls.kt"
        );
        if is_production_compose(path, &source) {
            if !approved_text_primitive && contains_direct_call(&compact, "BasicText(") {
                findings.push(format!(
                    "{path}: BasicText bypasses the shell primitive adapter"
                ));
            }
            if !approved_input_primitive && contains_direct_call(&compact, "BasicTextField(") {
                findings.push(format!(
                    "{path}: BasicTextField bypasses the shell primitive adapter"
                ));
            }
        }
        let lowercase = source.to_ascii_lowercase();
        for marker in [
            "sample farm",
            "sample commitment",
            "sample price",
            "sample event",
            "fake farm",
            "fake commitment",
            "pricecents",
            "commitmentid",
            "allocationid",
            "fulfillmentid",
        ] {
            if lowercase.contains(marker) {
                findings.push(format!(
                    "{path}: fake commercial product data marker {marker}"
                ));
            }
        }
    }
    for forbidden in [
        "app/shared/src/commonMain/kotlin/org/harvestcircle/design/HarvestCircleDesign.kt",
        "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/ShellControls.kt",
    ] {
        if inventory.paths.iter().any(|path| path == forbidden) {
            findings.push(format!(
                "{forbidden}: superseded product-shell authority returned"
            ));
        }
    }
}

fn is_forbidden_documentation_or_workflow_path(path: &str) -> bool {
    path.split('/').any(|part| {
        matches!(
            part,
            "doc" | "docs" | "spec" | "specs" | ".github" | ".act" | "workflow" | "workflows"
        )
    })
}

fn is_production_kotlin(path: &str) -> bool {
    path.starts_with("app/")
        && path.ends_with(".kt")
        && ["/src/main/", "/src/commonMain/", "/src/desktopMain/"]
            .iter()
            .any(|segment| path.contains(segment))
}

fn is_production_compose(path: &str, source: &str) -> bool {
    is_production_kotlin(path)
        && (source.contains("@Composable") || source.contains("androidx.compose."))
}

fn contains_direct_call(source: &str, call: &str) -> bool {
    source.match_indices(call).any(|(index, _)| {
        source[..index]
            .chars()
            .next_back()
            .is_none_or(|character| !character.is_ascii_alphanumeric() && character != '_')
    })
}

fn manifest_declares_dependency(source: &str, dependency: &str) -> bool {
    source.lines().map(str::trim).any(|line| {
        if line.is_empty() || line.starts_with('#') {
            return false;
        }
        if line
            .split_once('=')
            .is_some_and(|(key, _)| key.trim().trim_matches('"') == dependency)
        {
            return true;
        }
        line.strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .is_some_and(|table| {
                let segments = table.split('.').collect::<Vec<_>>();
                segments.contains(&"dependencies")
                    && segments
                        .last()
                        .is_some_and(|name| name.trim_matches('"') == dependency)
            })
    })
}

fn sqlite_dependency_topology(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let cargo_lock = read_text(root, "core/Cargo.lock");
    let package_count = |name: &str| {
        let marker = format!("name = \"{name}\"");
        cargo_lock
            .lines()
            .filter(|line| line.trim() == marker)
            .count()
    };
    if package_count("libsqlite3-sys") != 1
        || ["rusqlite", "refinery", "refinery-core", "refinery-macros"]
            .iter()
            .any(|name| package_count(name) != 0)
    {
        findings.push(
            "core/Cargo.lock: exact single SQLx-selected native SQLite topology changed".to_owned(),
        );
    }

    for path in inventory
        .paths
        .iter()
        .filter(|path| path.starts_with("core/") && path.ends_with("Cargo.toml"))
    {
        let manifest = read_text(root, path);
        if ["rusqlite", "refinery", "libsqlite3-sys"]
            .iter()
            .any(|dependency| manifest_declares_dependency(&manifest, dependency))
        {
            findings.push(format!(
                "{path}: direct alternate or native SQLite dependency is forbidden"
            ));
        }
    }
}

fn development_integration_policy(root: &Path, findings: &mut Vec<String>) {
    let makefile = read_text(root, "Makefile");
    for required in [
        "override CARGO := cargo +1.97.1",
        "api-check: doctor",
        "development-check: development-provenance-check source-check integration-check",
        "governed-development-check:",
        "governed-linux-x86_64-development-check: governed-doctor",
    ] {
        if makefile
            .lines()
            .filter(|line| line.trim() == required)
            .count()
            != 1
        {
            findings.push(format!(
                "Makefile: development integration boundary is missing {required}"
            ));
        }
    }

    let runner = read_text(root, "tools/run-linux-x86_64-development-check.sh");
    for required in [
        "rust:1.97.1-slim-trixie@sha256:fc0648ac2962539be80bd424729a20fd80f7b64bfba7e90bbd642aed6c697c5a",
        "--platform linux/amd64",
        "EXT_BUILD_RUN_ACTIVE",
        "--env JAVA_TOOL_OPTIONS=-Duser.home=/workspace/home",
        "cargo deny --manifest-path core/Cargo.toml check --config core/deny.toml licenses sources",
        "cargo test --manifest-path core/Cargo.toml --workspace --locked",
        "cargo clippy --manifest-path core/Cargo.toml --workspace --all-targets --locked -- -D warnings",
        ":app:desktop:integrationTest",
        ":app:desktop:verifyUniFfiBindings",
        "harvestcircle.linux_x86_64.development=pass",
    ] {
        if !runner.contains(required) {
            findings.push(format!(
                "tools/run-linux-x86_64-development-check.sh: faithful runner is missing {required}"
            ));
        }
    }
    for forbidden in [
        "cargo audit",
        " advisories",
        "dependencyCheck",
        "releaseReadiness",
        "unsignedReleaseReadiness",
        "verifyReleaseSupplyChainEvidence",
        "packageDmg",
        "packageDeb",
        "CycloneDX",
        "SLSA",
    ] {
        if runner.contains(forbidden) {
            findings.push(format!(
                "tools/run-linux-x86_64-development-check.sh: deferred release integration is active: {forbidden}"
            ));
        }
    }
}

fn provenance_check(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    const LIB_REVISION: &str = "ad17b7d3455a7147cfa303d976fc5c70c3a4c0cb";
    const PROVENANCE_PATH: &str = "core/provenance/harvestcircle-v1.toml";
    const SOURCE_LOCK_PATH: &str = "radroots.lib.source-lock.v1.toml";
    const MAX_SOURCE_LOCK_BYTES: u64 = 1024 * 1024;
    const MAX_CARGO_LOCK_BYTES: u64 = 32 * 1024 * 1024;
    let cargo = read_text(root, "core/Cargo.toml");
    for authority in [
        "repository = \"https://github.com/radrootslabs/harvestcircle\"".to_owned(),
        format!(
            "radroots_identity = {{ git = \"https://github.com/radrootslabs/lib\", rev = \"{LIB_REVISION}\", version = \"=0.1.0-alpha\", default-features = false }}"
        ),
        format!(
            "radroots_runtime_paths = {{ git = \"https://github.com/radrootslabs/lib\", rev = \"{LIB_REVISION}\", version = \"=0.1.0-alpha\", default-features = false }}"
        ),
        format!(
            "radroots_service_sqlite = {{ git = \"https://github.com/radrootslabs/lib\", rev = \"{LIB_REVISION}\", version = \"=0.1.0-alpha\", default-features = false }}"
        ),
        format!(
            "radroots_storage = {{ git = \"https://github.com/radrootslabs/lib\", rev = \"{LIB_REVISION}\", version = \"=0.1.0-alpha\", default-features = false }}"
        ),
        format!(
            "radroots_transport = {{ git = \"https://github.com/radrootslabs/lib\", rev = \"{LIB_REVISION}\", version = \"=0.1.0-alpha\", default-features = false }}"
        ),
        format!(
            "radroots_transport_nostr = {{ git = \"https://github.com/radrootslabs/lib\", rev = \"{LIB_REVISION}\", version = \"=0.1.0-alpha\", default-features = false }}"
        ),
    ] {
        if cargo
            .lines()
            .filter(|line| line.trim() == authority)
            .count()
            != 1
        {
            findings.push(format!(
                "core/Cargo.toml: missing exact authority: {authority}"
            ));
        }
    }
    let provenance = read_text(root, PROVENANCE_PATH);
    if !provenance.contains("source_product = \"HarvestCircle\"")
        || !provenance
            .contains("source_repository = \"https://github.com/radrootslabs/harvestcircle\"")
        || !provenance.contains(&format!("canonical_radroots_revision = \"{LIB_REVISION}\""))
        || !provenance
            .contains("foundation_baseline = \"c08d18ea569351dddeef70d4c1410708daf067b6\"")
    {
        findings.push(format!(
            "{PROVENANCE_PATH}: exact source provenance changed"
        ));
    }
    let expected_source_lock = concat!(
        "schema = \"radroots.lib.source-lock.v1\"\n",
        "repository = \"https://github.com/radrootslabs/lib\"\n",
        "revision = \"ad17b7d3455a7147cfa303d976fc5c70c3a4c0cb\"\n",
        "architecture = \"radroots.crates.release.v2\"\n",
        "workspace_catalog_sha256 = \"deca0c080deae187ff8186c0708903e42f41ea57f77c5f91581e23aa561164a4\"\n",
        "version = \"0.1.0-alpha\"\n",
        "source_archive_sha256 = \"2cf12c24ed649c3c8dd48cebcb8583996646e116fc2472539a55748c803584db\"\n",
        "lockfile = \"core/Cargo.lock\"\n",
        "lockfile_sha256 = \"d4454a053e5f5d1810170fe9987e0f2a1d365de7de3eb9c71599029e46a03fc3\"\n",
    );
    let source_lock_bytes =
        match bounded_no_follow_bytes(root, Path::new(SOURCE_LOCK_PATH), MAX_SOURCE_LOCK_BYTES) {
            Ok(bytes) => bytes,
            Err(error) => {
                findings.push(format!("{SOURCE_LOCK_PATH}: {error}"));
                Vec::new()
            }
        };
    let source_lock = String::from_utf8(source_lock_bytes).unwrap_or_default();
    if source_lock != expected_source_lock {
        findings.push(format!("{SOURCE_LOCK_PATH}: exact Lib source lock changed"));
    }
    let lockfile = exact_string_assignment(&source_lock, "lockfile");
    let declared_lockfile_sha256 = exact_string_assignment(&source_lock, "lockfile_sha256");
    let cargo_lock_bytes = lockfile
        .as_deref()
        .ok_or_else(|| "lockfile assignment is missing or duplicated".to_owned())
        .and_then(|path| bounded_no_follow_bytes(root, Path::new(path), MAX_CARGO_LOCK_BYTES));
    if let (Ok(bytes), Some(declared)) = (&cargo_lock_bytes, declared_lockfile_sha256.as_deref()) {
        let actual = format!("{:x}", Sha256::digest(bytes));
        if actual != declared {
            findings.push(format!(
                "{SOURCE_LOCK_PATH}: lockfile_sha256 does not match actual bounded no-follow bytes"
            ));
        }
    } else {
        let error = cargo_lock_bytes
            .as_ref()
            .err()
            .map(String::as_str)
            .unwrap_or("lockfile_sha256 assignment is missing or duplicated");
        findings.push(format!("{SOURCE_LOCK_PATH}: {error}"));
    }
    let cargo_lock = cargo_lock_bytes
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();
    if !cargo_lock.contains(&format!(
        "source = \"git+https://github.com/radrootslabs/lib?rev={LIB_REVISION}#{LIB_REVISION}\""
    )) {
        findings.push("core/Cargo.lock: selected Lib revision is missing".to_owned());
    }
    sqlite_dependency_topology(root, inventory, findings);
    development_integration_policy(root, findings);
    let coordinates = properties(&read_text(
        root,
        "config/product/harvestcircle-v1.properties",
    ));
    for (key, expected) in [
        ("storage.service_id", "harvestcircle"),
        ("storage.instance_id", "desktop"),
        ("storage.database_filename", "state.sqlite"),
        ("storage.lock_filename", "state.lock"),
        ("storage.application_id", "1212371505"),
        ("storage.application_id_text", "HCR1"),
        ("storage.initial_schema_version", "1"),
        ("legacy.database.filename", "harvestcircle.sqlite3"),
        ("legacy.database.disposition", "untouched_and_unsupported"),
        ("platform.macos.architecture", "aarch64"),
        ("platform.linux.architecture", "x86_64"),
        ("limit.identities", "256"),
        ("limit.unfinished_durable_operations", "1024"),
        ("limit.preference_value_utf8_bytes", "4096"),
        ("limit.relay_endpoints", "16"),
        ("limit.relay_url_bytes", "2048"),
        ("limit.events_per_relay", "64"),
        ("limit.events_total", "1024"),
        ("limit.observers", "32"),
        ("limit.actor_mailbox", "64"),
        ("limit.command_deadline_min_ms", "1"),
        ("limit.command_deadline_max_ms", "30000"),
        ("backup.member_limit", "caller_supplied_positive"),
    ] {
        if coordinates.get(key).map(String::as_str) != Some(expected) {
            findings.push(format!(
                "config/product/harvestcircle-v1.properties: {key} must remain {expected}"
            ));
        }
    }
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
    const STORAGE_API_BASELINE: &str = "core/compatibility/harvestcircle-storage-api-v1.txt";
    let storage_api = read_text(root, STORAGE_API_BASELINE);
    for required in [
        "pub struct harvestcircle_storage::HarvestCircleStorageContract",
        "pub const harvestcircle_storage::HARVESTCIRCLE_APPLICATION_ID: u32",
        "pub fn harvestcircle_storage::harvestcircle_schema_catalog()",
        "pub struct harvestcircle_storage::Database",
        "pub async fn harvestcircle_storage::Database::open",
        "pub async fn harvestcircle_storage::Database::close",
        "pub async fn harvestcircle_storage::Database::capture_online_backup",
        "pub async fn harvestcircle_storage::Database::restore_verified_backup",
        "pub struct harvestcircle_storage::VerifiedHarvestCircleBackup",
        "pub fn harvestcircle_storage::verify_harvestcircle_backup",
        "impl harvestcircle_application::ports::DurableOperationRepository for harvestcircle_storage::Database",
        "harvestcircle_application::ports::BoxFuture",
    ] {
        if !storage_api.contains(required) {
            findings.push(format!("{STORAGE_API_BASELINE}: missing {required}"));
        }
    }
    for forbidden in [
        "rusqlite::",
        "refinery::",
        "sqlx::",
        "OperationJournal",
        "harvestcircle_initial_schema_sql",
        "VerifiedServiceBackup",
        "StagedServiceRestore",
        "verify_backup_bundle",
        "stage_verified_restore",
        "finalize_staged_restore",
        "repair",
        "preflight",
    ] {
        if storage_api.contains(forbidden) {
            findings.push(format!(
                "{STORAGE_API_BASELINE}: dependency-owned API leaked: {forbidden}"
            ));
        }
    }
    for required in [
        PROVENANCE_PATH,
        SOURCE_LOCK_PATH,
        STORAGE_API_BASELINE,
        "config/verification/lanes-v3.properties",
        "tools/run-linux-x86_64-development-check.sh",
        "tools/verify-storage-api.sh",
    ] {
        if !inventory.paths.iter().any(|path| path == required) {
            findings.push(format!("{required}: governed source evidence is missing"));
        }
    }
}

fn design_source_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    const PATH: &str = "config/design/harvestcircle-v1.toml";
    if !inventory.paths.iter().any(|path| path == PATH) {
        findings.push(format!("{PATH}: design contract is missing"));
        return;
    }
    let source = read_text(root, PATH);
    let required_scalars = [
        "schema = \"harvestcircle.design.v1\"",
        "repository = \"https://github.com/radrootslabs/harvestcircle\"",
        "baseline_revision = \"c08d18ea569351dddeef70d4c1410708daf067b6\"",
        "license = \"GPL-3.0-only\"",
        "golden_host = \"macos-aarch64\"",
        "golden_status = \"verified\"",
        "design_system_root = \"app/design_system\"",
        "design_catalog_root = \"tools/design_catalog\"",
        "application_shell_root = \"app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell\"",
        "golden_test_path = \"app/shared/src/desktopTest/kotlin/org/harvestcircle/ui/shell/HarvestCircleMacGoldenTest.kt\"",
    ];
    for scalar in required_scalars {
        if source.lines().filter(|line| line.trim() == scalar).count() != 1 {
            findings.push(format!("{PATH}: missing or duplicate authority: {scalar}"));
        }
    }
    for (path, key, sha256) in [
        (
            "app/shared/src/desktopTest/resources/goldens/macos-aarch64/design-surface-light.png",
            "golden_light_sha256",
            "96e1ef5dd8b5cb14e47471a737a1e57ab0543b7f3aa79b865051e4740a2ee57a",
        ),
        (
            "app/shared/src/desktopTest/resources/goldens/macos-aarch64/design-surface-dark.png",
            "golden_dark_sha256",
            "6a85cd890109b11de6f647dca91cb616651e362aa0802c40aa6aee7f678451c9",
        ),
    ] {
        let authority = format!("{key} = \"{sha256}\"");
        if source
            .lines()
            .filter(|line| line.trim() == authority)
            .count()
            != 1
        {
            findings.push(format!("{PATH}: {key} must match the governed golden"));
        }
        if !inventory.paths.iter().any(|candidate| candidate == path)
            || sha256_file(&root.join(path)).as_deref() != Some(sha256)
        {
            findings.push(format!("{path}: macOS golden is missing or changed"));
        }
    }
    let golden_test = read_text(
        root,
        "app/shared/src/desktopTest/kotlin/org/harvestcircle/ui/shell/HarvestCircleMacGoldenTest.kt",
    );
    if !golden_test.contains("HarvestCircleShell(")
        || !golden_test.contains("liveTodayState(")
        || golden_test.contains("captureReferenceSurface(")
    {
        findings.push(
            "HarvestCircleMacGoldenTest.kt: golden must render a live application shell state"
                .to_owned(),
        );
    }
    let catalog = read_text(root, "gradle/libs.versions.toml");
    for required in [
        "compose-animation = { module = \"org.jetbrains.compose.animation:animation\", version.ref = \"compose\" }",
        "compose-components-resources = { module = \"org.jetbrains.compose.components:components-resources\", version.ref = \"compose\" }",
    ] {
        if !catalog.contains(required) {
            findings.push(format!(
                "gradle/libs.versions.toml: missing approved design dependency: {required}"
            ));
        }
    }
    for forbidden in ["compose-material3", "platformtools", "js(", "wasm"] {
        if catalog.to_ascii_lowercase().contains(forbidden) {
            findings.push(format!(
                "gradle/libs.versions.toml: forbidden design dependency or target: {forbidden}"
            ));
        }
    }
    for path in &inventory.paths {
        if !path.starts_with("app/design_system/") && !path.starts_with("tools/design_catalog/") {
            continue;
        }
        let lowercase = read_text(root, path).to_ascii_lowercase();
        for forbidden in [
            "androidx.compose.material3".to_owned(),
            "io.github.kdroidfilter.platformtools".to_owned(),
            "com.radroots.".to_owned() + &["stu", "dio"].concat(),
        ] {
            if lowercase.contains(&forbidden) {
                findings.push(format!(
                    "{path}: forbidden dependency or legacy namespace: {forbidden}"
                ));
            }
        }
    }
    for (path, sha256) in [
        (
            "app/design_system/src/commonMain/composeResources/font/inter_bold.ttf",
            "288316099b1e0a47a4716d159098005eef7c0066921f34e3200393dbdb01947f",
        ),
        (
            "app/design_system/src/commonMain/composeResources/font/inter_medium.ttf",
            "97ad806f526e41546d46365bb3a393145f75b7b1568913db74549ad8b8dba872",
        ),
        (
            "app/design_system/src/commonMain/composeResources/font/inter_regular.ttf",
            "40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82",
        ),
        (
            "app/design_system/src/commonMain/composeResources/font/inter_semibold.ttf",
            "78a843fade9d4612a5567302fb595b56976eb5fcebf4fea5a5912d638bafcde3",
        ),
    ] {
        if sha256_file(&root.join(path)).as_deref() != Some(sha256) {
            findings.push(format!(
                "{path}: Inter font digest differs from the baseline"
            ));
        }
    }
    let font_license = read_text(root, "LICENSES/OFL-1.1.txt");
    let packaged_font_license = read_text(
        root,
        "app/design_system/src/commonMain/composeResources/files/licenses/inter-OFL-1.1.txt",
    );
    if font_license.is_empty() || packaged_font_license != font_license {
        findings.push("Inter font licence is missing or differs in packaged resources".to_owned());
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

fn sha256_file(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
}

fn exact_string_assignment(source: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = \"");
    let values = source
        .lines()
        .filter_map(|line| {
            let value = line.strip_prefix(&prefix)?.strip_suffix('"')?;
            (!value.is_empty()).then(|| value.to_owned())
        })
        .collect::<Vec<_>>();
    (values.len() == 1).then(|| values[0].clone())
}

fn bounded_no_follow_bytes(root: &Path, relative: &Path, maximum: u64) -> Result<Vec<u8>, String> {
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
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
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
    fn current_design_contract_is_exact() {
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
    fn current_source_provenance_and_lib_lock_are_exact() {
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
    fn sqlite_topology_rejects_alternate_duplicate_and_direct_authority() {
        let root = fixture("sqlite-topology");
        write(
            &root,
            "core/Cargo.lock",
            "name = \"libsqlite3-sys\"\nname = \"libsqlite3-sys\"\nname = \"rusqlite\"\n",
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
            findings.iter().any(|finding| finding
                .contains("exact single SQLx-selected native SQLite topology changed")),
            "{findings:#?}"
        );
        assert!(
            findings.iter().any(|finding| finding
                .contains("direct alternate or native SQLite dependency is forbidden")),
            "{findings:#?}"
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn development_runner_rejects_release_integration_activation() {
        let root = fixture("development-runner");
        write(
            &root,
            "Makefile",
            "override CARGO := cargo +1.97.1\napi-check: doctor\ndevelopment-check: development-provenance-check source-check integration-check\ngoverned-development-check:\ngoverned-linux-x86_64-development-check: governed-doctor\n",
        );
        write(
            &root,
            "tools/run-linux-x86_64-development-check.sh",
            "dependencyCheckAnalyze\n",
        );
        let mut findings = Vec::new();
        development_integration_policy(&root, &mut findings);
        assert!(
            findings
                .iter()
                .any(|finding| { finding.contains("deferred release integration is active") }),
            "{findings:#?}"
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn design_contract_rejects_identity_and_root_drift() {
        let root = fixture("design-contract");
        write(
            &root,
            "config/design/harvestcircle-v1.toml",
            "schema = \"harvestcircle.design.v1\"\nrepository = \"https://example.invalid/other\"\ndesign_system_root = \"../escape\"\n",
        );
        let inventory = Inventory::load(&root).expect("archive inventory");
        let mut findings = Vec::new();
        design_source_audit(&root, &inventory, &mut findings);
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("missing or duplicate authority"))
        );
        fs::remove_dir_all(root).expect("remove fixture");
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

    #[test]
    fn namespace_policy_rejects_transition_identity_without_exceptions() {
        let root = fixture("namespace-no-exceptions");
        let legacy = ["stu", "dio"].concat();
        let repository = format!("https://github.com/radrootslabs/{legacy}_app");
        let entry =
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/BootstrapIdentityEntry.kt";
        write(
            &root,
            "app/shared/src/commonMain/kotlin/org/harvestcircle/product/SurfaceRegistry.kt",
            &format!("val key = \"round_{legacy}_screen\"\n"),
        );
        write(&root, entry, "val placeholder = \"nsec1…\"\n");
        write(
            &root,
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/FoundationSettingsScreen.kt",
            &format!(
                "val source = \"{repository}\"\nval licence = \"{repository}/blob/dev/LICENSE\"\n"
            ),
        );
        let inventory = Inventory::load(&root).expect("allowlist inventory");
        let mut findings = Vec::new();
        namespace_audit(&root, &inventory, &mut findings);
        assert!(
            findings
                .iter()
                .filter(|finding| finding.contains("legacy product name"))
                .count()
                >= 2,
            "{findings:?}"
        );
        assert!(
            findings
                .iter()
                .all(|finding| !finding.contains("secret key literal")),
            "{findings:?}"
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn product_shell_audit_requires_the_complete_source_contract() {
        let root = fixture("product-shell");
        write(&root, "README.md", "safe\n");
        let inventory = Inventory::load(&root).expect("product shell inventory");
        let mut findings = Vec::new();
        product_shell_audit(&root, &inventory, &mut findings);
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("required product-shell source is missing"))
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn current_product_shell_sources_pass_the_expanded_policy() {
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
    fn product_shell_audit_rejects_legacy_screen_paths_tags_and_colors() {
        let root = fixture("legacy-shell");
        let path = "app/shared/src/commonMain/kotlin/org/harvestcircle/identities/ui/HarvestCircleScreen.kt";
        write(
            &root,
            path,
            "@Composable fun Legacy() { val tag = \"home-screen\"; val color = WindowBackgroundColor }\n",
        );
        let inventory = Inventory::load(&root).expect("legacy inventory");
        let mut findings = Vec::new();
        product_shell_audit(&root, &inventory, &mut findings);
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("superseded product-shell screen path"))
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("superseded product-shell marker home-screen"))
        );
        assert!(findings.iter().any(|finding| {
            finding.contains("superseded product-shell marker WindowBackgroundColor")
        }));
        fs::remove_dir_all(root).expect("remove fixture");
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
                .any(|finding| finding
                    .contains("hard-coded Compose color outside the theme adapter"))
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
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("superseded shell button adapter"))
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn product_shell_audit_rejects_fake_commercial_data_outside_the_shell_package() {
        let root = fixture("commercial-data");
        write(
            &root,
            "app/shared/src/commonMain/kotlin/org/harvestcircle/product/FakeData.kt",
            "data class FakeData(val commitmentId: String, val priceCents: Long)\n",
        );
        let inventory = Inventory::load(&root).expect("commercial inventory");
        let mut findings = Vec::new();
        product_shell_audit(&root, &inventory, &mut findings);
        assert!(findings.iter().any(|finding| finding
            .contains("fake commercial product data marker commitmentid")));
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("fake commercial product data marker pricecents"))
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn product_shell_audit_rejects_retired_security_and_selection_shapes() {
        let root = fixture("shell-security-shapes");
        write(
            &root,
            "app/shared/src/commonMain/kotlin/org/harvestcircle/application/LegacyConfirmation.kt",
            "data object ConfirmIdentityRemoval\ndata object CancelIdentityRemoval\n",
        );
        write(
            &root,
            "app/shared/src/commonMain/kotlin/org/harvestcircle/application/UnsafeReference.kt",
            "fun reduce(intent: OverlayIntent) = when (intent) { is OverlayIntent.EditReference -> classifyNostrReference(intent.value) }\n",
        );
        write(
            &root,
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/UnsafeIngress.kt",
            "val overlay = OverlayIntent.Open(FoundationOverlay.OpenNostrReference(\"prefilled\"))\n",
        );
        write(
            &root,
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/UnsafeSelection.kt",
            "ShellTab(selected = true, enabled = false)\n",
        );
        let inventory = Inventory::load(&root).expect("security shape inventory");
        let mut findings = Vec::new();
        product_shell_audit(&root, &inventory, &mut findings);
        for expected in [
            "retired parameterless confirmation source shape",
            "parser-on-edit source shape",
            "prefilled reference ingress source shape",
            "selected-as-disabled source shape",
        ] {
            assert!(
                findings.iter().any(|finding| finding.contains(expected)),
                "missing {expected}: {findings:?}"
            );
        }
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn product_shell_audit_rejects_secret_custody_and_global_overlay_busy_shapes() {
        let root = fixture("secret-lifecycle-shapes");
        write(
            &root,
            "app/shared/src/commonMain/kotlin/org/harvestcircle/application/UnsafeDraft.kt",
            "data class EditImportDraft(val value: String)\ndata class State(val importDraft: String)\n",
        );
        write(
            &root,
            "app/shared/src/commonMain/kotlin/org/harvestcircle/application/ShellOverlays.kt",
            "fun admit(state: State) = state.identity.busy\n",
        );
        write(
            &root,
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/FoundationOverlayHost.kt",
            "fun FoundationOverlayHost(state: OverlayState, status: ShellStatusModel, busy: Boolean) = Unit\n",
        );
        let inventory = Inventory::load(&root).expect("closure shape inventory");
        let mut findings = Vec::new();
        product_shell_audit(&root, &inventory, &mut findings);
        for expected in [
            "raw String import-draft custody source shape",
            "copyable import-draft intent source shape",
            "unrelated identity busy state must not gate overlay admission",
            "global busy state must not enter the overlay host",
        ] {
            assert!(
                findings.iter().any(|finding| finding.contains(expected)),
                "missing {expected}: {findings:?}"
            );
        }
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn repository_policy_rejects_nested_documentation_and_workflow_roots() {
        let root = fixture("nested-docs");
        write(&root, "app/docs/notes.md", "fixture\n");
        write(&root, "app/.github/workflows/remote.yml", "fixture\n");
        let inventory = Inventory::load(&root).expect("nested docs inventory");
        let mut findings = Vec::new();
        repo_audit(&root, &inventory, &mut findings);
        assert!(
            findings
                .iter()
                .any(|finding| finding.starts_with("app/docs/"))
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.starts_with("app/.github/workflows/"))
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
        let error =
            Inventory::load(&root).expect_err("invalid UTF-8 inventory path must fail closed");
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
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("exact Lib source lock changed"))
        );
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[test]
    fn source_lock_digest_rejects_actual_byte_mismatch() {
        let root = fixture("source-lock-digest");
        write(
            &root,
            "radroots.lib.source-lock.v1.toml",
            concat!(
                "schema = \"radroots.lib.source-lock.v1\"\n",
                "repository = \"https://github.com/radrootslabs/lib\"\n",
                "revision = \"ad17b7d3455a7147cfa303d976fc5c70c3a4c0cb\"\n",
                "architecture = \"radroots.crates.release.v2\"\n",
                "workspace_catalog_sha256 = \"deca0c080deae187ff8186c0708903e42f41ea57f77c5f91581e23aa561164a4\"\n",
                "version = \"0.1.0-alpha\"\n",
                "source_archive_sha256 = \"2cf12c24ed649c3c8dd48cebcb8583996646e116fc2472539a55748c803584db\"\n",
                "lockfile = \"core/Cargo.lock\"\n",
                "lockfile_sha256 = \"d4454a053e5f5d1810170fe9987e0f2a1d365de7de3eb9c71599029e46a03fc3\"\n",
            ),
        );
        write(&root, "core/Cargo.toml", "");
        write(&root, "core/Cargo.lock", "version = 3\n");
        let inventory = Inventory::load(&root).expect("source-lock inventory");
        let mut findings = Vec::new();
        provenance_check(&root, &inventory, &mut findings);
        assert!(findings.iter().any(|finding| {
            finding.contains("lockfile_sha256 does not match actual bounded no-follow bytes")
        }));
        fs::remove_dir_all(root).expect("remove fixture");
    }

    #[cfg(unix)]
    #[test]
    fn bounded_source_lock_reads_reject_final_and_intermediate_symlinks() {
        use std::os::unix::fs::symlink;

        let root = fixture("source-lock-symlinks");
        write(&root, "actual/Cargo.lock", "version = 4\n");
        symlink(root.join("actual/Cargo.lock"), root.join("final.lock"))
            .expect("create final symlink");
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
}
