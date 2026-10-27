use super::{
    Inventory, bounded_no_follow_bytes, is_private_state_path, is_source_safety_text, read_text,
};
use std::{fs, path::Path};

pub(super) fn global_source_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let required = [
        "README.md",
        "CONTRIBUTING.md",
        "SECURITY.md",
        "LICENSE",
        "app/design_system/src/commonMain/composeResources/files/licenses/inter-OFL-1.1.txt",
    ];
    for required_path in required {
        if !inventory.paths.iter().any(|path| path == required_path) {
            findings.push(format!(
                "{required_path}: required public repository file is missing"
            ));
        }
        if read_text(root, required_path, findings).trim().is_empty() {
            findings.push(format!(
                "{required_path}: required public repository text is empty"
            ));
        }
    }
    for path in &inventory.paths {
        let normalized = path.to_ascii_lowercase();
        if is_private_state_path(&normalized) {
            findings.push(format!("{path}: forbidden repository root"));
        }
        let symbolic_link = fs::symlink_metadata(root.join(path))
            .is_ok_and(|metadata| metadata.file_type().is_symlink());
        if symbolic_link {
            findings.push(format!(
                "{path}: symbolic links are not allowed in public sources"
            ));
        }
        let generated_output = is_generated_output_path(&normalized);
        if generated_output {
            findings.push(format!(
                "{path}: generated build output must not be source controlled"
            ));
        }
        let secret_path = normalized
            .split('/')
            .any(|part| part == ".env" || part.starts_with(".env."))
            || [".pem", ".key", ".p12", ".pfx", ".jks", ".keystore", ".env"]
                .iter()
                .any(|suffix| normalized.ends_with(suffix))
            || normalized.contains("/credentials/");
        if secret_path {
            findings.push(format!("{path}: credential or secret-shaped source path"));
        }
        // Output and secret-shaped paths fail by identity; do not inspect their contents.
        if is_source_safety_text(path) && !symbolic_link && !generated_output && !secret_path {
            let source = match bounded_no_follow_bytes(root, Path::new(path), 8 * 1024 * 1024)
                .and_then(|bytes| {
                    String::from_utf8(bytes)
                        .map_err(|error| format!("invalid source UTF-8: {error}"))
                }) {
                Ok(source) => source,
                Err(error) => {
                    findings.push(format!("{path}: unable to inspect source text: {error}"));
                    continue;
                }
            };
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
}

pub(super) fn is_generated_output_path(path: &str) -> bool {
    path.split('/')
        .any(|part| matches!(part, "node_modules" | ".svelte-kit" | "build" | "target"))
        || path.contains("/generated/")
        || path.contains("generated/uniffi")
        || [".dylib", ".so", ".dll", ".class", ".wasm", ".node"]
            .iter()
            .any(|suffix| path.ends_with(suffix))
}
