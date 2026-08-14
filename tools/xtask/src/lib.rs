use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::str::FromStr;

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
}

fn namespace_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let legacy = ["stu", "dio"].concat();
    let provenance_path = format!("core/provenance/{legacy}-import-v1.toml");
    let design_provenance_path = "config/design/source_baseline_v1.toml";
    let design_audit_path = "tools/xtask/src/lib.rs";
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
        if path != &provenance_path
            && path != design_provenance_path
            && normalized.contains(&legacy)
        {
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
        if path != &provenance_path && path != design_provenance_path {
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
            for exact in approved_legacy_product_fragments(path, &legacy, &legacy_repository) {
                inspected = inspected.replace(&exact, "");
            }
            if path == design_audit_path {
                inspected = inspected
                    .replace("design-source-audit", "")
                    .replace("design_source_audit", "")
                    .replace("DesignSourceAudit", "")
                    .replace("design_source_mappings", "")
                    .replace("push_design_mapping", "")
                    .replace("current_design_source_baseline_is_exact", "")
                    .replace(
                        "design_source_baseline_rejects_snapshot_and_mapping_drift",
                        "",
                    )
                    .replace(design_provenance_path, "")
                    .replace("harvestcircle.design_source_baseline.v1", "")
                    .replace(&format!("source_product = \"{}\"", title_case(&legacy)), "")
                    .replace(&legacy_repository, "")
                    .replace(
                        "shared/src/commonMain/kotlin/com/radroots/studio/ui/shell",
                        "",
                    )
                    .replace(
                        "shared/src/commonTest/kotlin/com/radroots/studio/ui/shell",
                        "",
                    )
                    .replace("shared/src/jvmMain/kotlin/com/radroots/studio/ui/shell", "")
                    .replace(
                        "shared/src/commonMain/kotlin/com/radroots/studio/ui/dashboard",
                        "",
                    )
                    .replace("shared/src/webMain/kotlin/com/radroots/studio/ui/shell", "")
                    .replace("audit-only/dashboard-visual-inputs", "")
                    .replace("audit-only/product-copy", "")
                    .replace("audit-only/web-target", "")
                    .replace("fixture(\"design-source\")", "")
                    .replace("com.radroots.studio", "")
                    .replace(r#"source_product = \"Studio\""#, "")
                    .replace(&format!("\"{}\"", title_case(&legacy)), "");
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
        let secret_marker = ["nsec", "1"].concat();
        let inspected_secret = lowercase.replace(&format!("{secret_marker}…"), "");
        if production_kotlin && inspected_secret.contains(&secret_marker) {
            findings.push(format!("{path}: secret key literal in production Kotlin"));
        }
    }
}

fn approved_legacy_product_fragments(
    path: &str,
    legacy: &str,
    legacy_repository: &str,
) -> Vec<String> {
    match path {
        "app/shared/src/commonMain/kotlin/org/harvestcircle/product/SurfaceRegistry.kt" => vec![
            format!("round_{legacy}_screen"),
            format!("Round{}", title_case(legacy)),
        ],
        "app/shared/src/commonTest/kotlin/org/harvestcircle/product/SurfaceRegistryTest.kt" => {
            vec![format!("round_{legacy}_screen")]
        }
        "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/FoundationSettingsScreen.kt" =>
        {
            vec![legacy_repository.to_owned()]
        }
        _ => Vec::new(),
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

fn design_source_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    const PATH: &str = "config/design/source_baseline_v1.toml";
    if !inventory.paths.iter().any(|path| path == PATH) {
        findings.push(format!("{PATH}: design source baseline is missing"));
        return;
    }
    let source = read_text(root, PATH);
    let expected_snapshot = "c2fe49f3c3ea43105cb2fff4a67c7cd9c21561c71825440a91654a0c1b12e3b8";
    let required_scalars = [
        "schema = \"harvestcircle.design_source_baseline.v1\"",
        "source_product = \"Studio\"",
        "source_repository = \"https://github.com/radrootslabs/studio_app\"",
        "source_head = \"8ae5d8a0377c5673038a20b82b87c314370f0395\"",
        "source_state = \"clean\"",
        "snapshot_file_count = 102",
        "source_license = \"GPL-3.0-only\"",
        "golden_host = \"macos-aarch64\"",
        "golden_status = \"verified\"",
    ];
    for scalar in required_scalars {
        if source.lines().filter(|line| line.trim() == scalar).count() != 1 {
            findings.push(format!("{PATH}: missing or duplicate authority: {scalar}"));
        }
    }
    for (key, digest) in [
        ("snapshot_sha256", expected_snapshot),
        ("golden_source_snapshot_sha256", expected_snapshot),
    ] {
        let expected = format!("{key} = \"{digest}\"");
        if source
            .lines()
            .filter(|line| line.trim() == expected)
            .count()
            != 1
        {
            findings.push(format!("{PATH}: {key} must match the governed snapshot"));
        }
    }
    for (path, key, sha256) in [
        (
            "app/shared/src/desktopTest/resources/goldens/macos-aarch64/design-surface-light.png",
            "golden_light_sha256",
            "01fab45de6622497568b7655aac59d4e65a64e7c41a314bb2453fae56893f515",
        ),
        (
            "app/shared/src/desktopTest/resources/goldens/macos-aarch64/design-surface-dark.png",
            "golden_dark_sha256",
            "187ba53d1c71ebe2bb7c072a68697c0217d537e58c4f72a6115df4120972913c",
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
    let mappings = design_source_mappings(&source, PATH, findings);
    let required = [
        (
            "core/designsystem/src/commonMain",
            "app/design_system/src/commonMain",
            "owned-port",
        ),
        (
            "core/designsystem/src/commonTest",
            "app/design_system/src/commonTest",
            "owned-port",
        ),
        (
            "core/designsystem/src/jvmMain",
            "app/design_system/src/desktopMain",
            "owned-port",
        ),
        (
            "tools/designcatalog/src/commonMain",
            "tools/design_catalog/src/commonMain",
            "owned-port",
        ),
        (
            "tools/designcatalog/src/jvmMain",
            "tools/design_catalog/src/desktopMain",
            "owned-port",
        ),
        (
            "shared/src/commonMain/kotlin/com/radroots/studio/ui/shell",
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell",
            "visual-reference",
        ),
        (
            "shared/src/commonTest/kotlin/com/radroots/studio/ui/shell",
            "app/shared/src/commonTest/kotlin/org/harvestcircle/ui/shell",
            "test-reference",
        ),
        (
            "shared/src/jvmMain/kotlin/com/radroots/studio/ui/shell",
            "app/shared/src/desktopMain/kotlin/org/harvestcircle/ui/shell",
            "visual-reference",
        ),
        (
            "shared/src/commonMain/kotlin/com/radroots/studio/ui/dashboard",
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell",
            "visual-reference",
        ),
        (
            "shared/src/commonMain/composeResources/values/strings.xml",
            "audit-only/product-copy",
            "reject-product-copy",
        ),
        (
            "shared/src/webMain/kotlin/com/radroots/studio/ui/shell",
            "audit-only/web-target",
            "reject-platform-target",
        ),
    ];
    let expected = required
        .iter()
        .map(|(source, destination, disposition)| {
            (
                (*source).to_owned(),
                (*destination).to_owned(),
                (*disposition).to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    if mappings != expected {
        findings.push(format!(
            "{PATH}: source-to-owned mapping differs from the approved migration"
        ));
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
            "androidx.compose.material3",
            "io.github.kdroidfilter.platformtools",
            "com.radroots.studio",
        ] {
            if lowercase.contains(forbidden) {
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

fn design_source_mappings(
    source: &str,
    path: &str,
    findings: &mut Vec<String>,
) -> BTreeSet<(String, String, String)> {
    let mut mappings = BTreeSet::new();
    let mut current = Vec::new();
    for line in source.lines().map(str::trim) {
        if line == "[[mapping]]" {
            if !current.is_empty() {
                push_design_mapping(&mut mappings, &current, path, findings);
                current.clear();
            }
        } else if !line.is_empty()
            && line.contains(" = ")
            && (!current.is_empty() || line.starts_with("source = "))
        {
            current.push(line.to_owned());
        }
    }
    if !current.is_empty() {
        push_design_mapping(&mut mappings, &current, path, findings);
    }
    mappings
}

fn push_design_mapping(
    mappings: &mut BTreeSet<(String, String, String)>,
    lines: &[String],
    path: &str,
    findings: &mut Vec<String>,
) {
    let value = |key: &str| {
        lines
            .iter()
            .find_map(|line| {
                line.strip_prefix(&format!("{key} = \""))
                    .and_then(|value| value.strip_suffix('"'))
            })
            .unwrap_or_default()
            .to_owned()
    };
    let mapping = (value("source"), value("destination"), value("disposition"));
    if mapping.0.is_empty()
        || mapping.1.is_empty()
        || mapping.2.is_empty()
        || mapping.0.starts_with('/')
        || mapping.1.starts_with('/')
        || mapping.0.split('/').any(|part| part == "..")
        || mapping.1.split('/').any(|part| part == "..")
        || !mappings.insert(mapping)
    {
        findings.push(format!("{path}: invalid or duplicate source mapping"));
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

fn sha256_file(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(format!("{:x}", Sha256::digest(bytes)))
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
    fn current_design_source_baseline_is_exact() {
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
    fn design_source_baseline_rejects_snapshot_and_mapping_drift() {
        let root = fixture("design-source");
        write(
            &root,
            "config/design/source_baseline_v1.toml",
            "schema = \"harvestcircle.design_source_baseline.v1\"\n[[mapping]]\nsource = \"../escape\"\ndestination = \"/tmp\"\ndisposition = \"owned-port\"\n",
        );
        let inventory = Inventory::load(&root).expect("archive inventory");
        let mut findings = Vec::new();
        design_source_audit(&root, &inventory, &mut findings);
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("governed snapshot"))
        );
        assert!(
            findings
                .iter()
                .any(|finding| finding.contains("invalid or duplicate source mapping"))
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
    fn namespace_policy_allows_only_exact_locked_legacy_contracts_and_placeholder() {
        let root = fixture("namespace-allowlist");
        let legacy = ["stu", "dio"].concat();
        let repository = format!("https://github.com/radrootslabs/{legacy}_app");
        let registry =
            "app/shared/src/commonMain/kotlin/org/harvestcircle/product/SurfaceRegistry.kt";
        let entry =
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell/BootstrapIdentityEntry.kt";
        write(
            &root,
            registry,
            &format!(
                "val key = \"round_{legacy}_screen\"\nclass Round{}\n",
                title_case(&legacy)
            ),
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
        assert!(findings.is_empty(), "{findings:?}");
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
