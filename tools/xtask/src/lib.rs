mod design;
mod inventory;
mod kotlin;
mod metadata;
mod runtime;
mod source;

use design::*;
use inventory::*;
use kotlin::*;
use metadata::*;
use runtime::*;
use source::*;

pub mod affected;
pub mod runtime_ownership;

use sha2::{Digest, Sha256};
use std::path::Path;
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
    let root = validate_product_root(root).map_err(|finding| vec![finding])?;
    let root = root.as_path();
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
    let ownership = load_ownership(root).map_err(|finding| vec![finding])?;
    ownership
        .validate_native_inputs(&inventory.paths)
        .map_err(|finding| vec![finding])?;
    let mut native_paths = Vec::new();
    let mut ownership_reasons = Vec::new();
    for path in &inventory.paths {
        let classification = ownership.classify(path).map_err(|finding| vec![finding])?;
        if classification
            .owners
            .contains(&runtime_ownership::Owner::Native)
        {
            native_paths.push(path.clone());
        }
        if let Some(reason) = classification.reason {
            ownership_reasons.push(reason);
        }
    }
    let native_inventory = Inventory {
        paths: native_paths,
        git_aware: inventory.git_aware,
    };
    // Every command retains global safety. Only native-owned inputs enter the
    // existing native implementation rules; unknown inputs widen conservatively.
    if !matches!(command, Command::RepoAudit | Command::QualificationReport) {
        global_source_audit(root, &inventory, &mut findings);
    }
    match command {
        Command::DesignSourceAudit => design_source_audit(root, &native_inventory, &mut findings),
        Command::RepoAudit => repo_audit(root, &inventory, &mut findings),
        Command::NamespaceAudit => namespace_audit(root, &native_inventory, &mut findings),
        Command::ProvenanceCheck => provenance_check(root, &native_inventory, &mut findings),
        Command::QualificationReport => {
            repo_audit(root, &inventory, &mut findings);
            namespace_audit(root, &native_inventory, &mut findings);
            provenance_check(root, &native_inventory, &mut findings);
            design_source_audit(root, &native_inventory, &mut findings);
            product_shell_audit(root, &native_inventory, &mut findings);
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
            "harvestcircle.xtask.command={command_name}\nharvestcircle.xtask.inventory={inventory_kind}\n{mode}harvestcircle.xtask.global_source_safety=checked\nharvestcircle.xtask.native_inputs={}\n{}harvestcircle.xtask.result=pass\n",
            native_inventory.paths.len(),
            ownership_reasons
                .into_iter()
                .map(|reason| format!("harvestcircle.xtask.ownership_reason={reason}\n"))
                .collect::<String>()
        ))
    } else {
        Err(findings)
    }
}

fn repo_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    global_source_audit(root, inventory, findings);
    runtime_ownership_audit(root, inventory, findings);
    git_source_policy(root, inventory, findings);
    native_runtime_boundary(root, findings);
}

fn runtime_ownership_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let result = load_ownership(root).and_then(|map| map.validate_native_inputs(&inventory.paths));
    if let Err(error) = result {
        findings.push(format!("{}: {error}", runtime_ownership::MAP_PATH));
    }
}

fn load_ownership(root: &Path) -> Result<runtime_ownership::OwnershipMap, String> {
    bounded_no_follow_bytes(root, Path::new(runtime_ownership::MAP_PATH), 64 * 1024)
        .and_then(|bytes| {
            String::from_utf8(bytes).map_err(|error| format!("invalid ownership UTF-8: {error}"))
        })
        .and_then(|source| runtime_ownership::OwnershipMap::parse(&source))
}

fn is_private_state_path(path: &str) -> bool {
    path.split('/').any(|part| {
        matches!(
            part,
            ".act" | ".beads" | ".metaswarm" | ".claude" | "handoff" | "handoffs" | "sessions"
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

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_source_safety_text(relative: &str) -> bool {
    let normalized = relative.to_ascii_lowercase();
    is_text(relative)
        || is_text(&normalized)
        || Path::new(&normalized)
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|extension| {
                ["svelte", "ts", "js", "css", "html", "sh", "py"].contains(&extension)
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
            "gradlew",
            "gradlew.bat",
        ]
        .contains(&name)
}

fn read_text(root: &Path, relative: &str, findings: &mut Vec<String>) -> String {
    match bounded_no_follow_bytes(root, Path::new(relative), 8 * 1024 * 1024).and_then(|bytes| {
        String::from_utf8(bytes).map_err(|error| format!("invalid source UTF-8: {error}"))
    }) {
        Ok(source) => source,
        Err(error) => {
            findings.push(format!(
                "{relative}: unable to inspect required source text: {error}"
            ));
            // The finding is retained even when a caller only tests absence of a marker.
            String::new()
        }
    }
}

fn sha256_file(root: &Path, relative: &str, findings: &mut Vec<String>) -> Option<String> {
    match bounded_no_follow_bytes(root, Path::new(relative), 8 * 1024 * 1024) {
        Ok(bytes) => Some(format!("{:x}", Sha256::digest(bytes))),
        Err(error) => {
            findings.push(format!(
                "{relative}: unable to inspect required digest input: {error}"
            ));
            None
        }
    }
}

#[cfg(test)]
mod tests;
