use super::{Inventory, is_lower_hex, properties, read_text};
use std::{collections::BTreeSet, path::Path};

pub(super) fn read_toml(
    root: &Path,
    path: &str,
    findings: &mut Vec<String>,
) -> Option<toml::Value> {
    match toml::from_str(&read_text(root, path, findings)) {
        Ok(value) => Some(value),
        Err(error) => {
            findings.push(format!("{path}: invalid TOML: {error}"));
            None
        }
    }
}

pub(super) fn cargo_dependencies(
    manifest: &toml::Value,
    include_dev: bool,
) -> Vec<(String, toml::Value)> {
    fn collect(value: &toml::Value, include_dev: bool, output: &mut Vec<(String, toml::Value)>) {
        let Some(table) = value.as_table() else {
            return;
        };
        for (key, value) in table {
            if matches!(key.as_str(), "dependencies" | "build-dependencies")
                || (include_dev && key == "dev-dependencies")
            {
                if let Some(dependencies) = value.as_table() {
                    output.extend(
                        dependencies
                            .iter()
                            .map(|(name, value)| (name.clone(), value.clone())),
                    );
                }
            } else if key == "workspace" {
                collect(value, include_dev, output);
            } else if key == "target"
                && let Some(targets) = value.as_table()
            {
                for target in targets.values() {
                    collect(target, include_dev, output);
                }
            }
        }
    }
    let mut result = Vec::new();
    collect(manifest, include_dev, &mut result);
    result
}

pub(super) fn sqlite_dependency_topology(
    root: &Path,
    inventory: &Inventory,
    findings: &mut Vec<String>,
) {
    if let Some(lock) = read_toml(root, "core/Cargo.lock", findings) {
        let packages = lock.get("package").and_then(toml::Value::as_array);
        let count = |name: &str| {
            packages.map_or(0, |ps| {
                ps.iter()
                    .filter(|p| p.get("name").and_then(toml::Value::as_str) == Some(name))
                    .count()
            })
        };
        if count("libsqlite3-sys") != 1
            || ["rusqlite", "refinery", "refinery-core", "refinery-macros"]
                .iter()
                .any(|name| count(name) != 0)
        {
            findings.push(
                "core/Cargo.lock: exact single SQLx-selected native SQLite topology changed"
                    .to_owned(),
            );
        }
    }
    for path in inventory
        .paths
        .iter()
        .filter(|p| p.starts_with("core/") && p.ends_with("Cargo.toml"))
    {
        let Some(manifest) = read_toml(root, path, findings) else {
            continue;
        };
        if cargo_dependencies(&manifest, true)
            .iter()
            .any(|(name, value)| {
                ["rusqlite", "refinery", "libsqlite3-sys"].contains(
                    &value
                        .get("package")
                        .and_then(toml::Value::as_str)
                        .unwrap_or(name),
                )
            })
        {
            findings.push(format!(
                "{path}: direct alternate or native SQLite dependency is forbidden"
            ));
        }
    }
}

pub(super) fn provenance_check(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let manifest = read_text(root, "core/Cargo.toml", findings);
    let lock = read_text(root, "core/Cargo.lock", findings);
    if let Err(error) = harvestcircle_product::dependencies::lib_revision(&manifest, &lock) {
        findings.push(format!("Cargo shared dependency identity: {error}"));
    }
    sqlite_dependency_topology(root, inventory, findings);
    let product_source = read_text(root, "config/product/harvestcircle-v1.properties", findings);
    let coordinates = match harvestcircle_product::parser::parse(&product_source) {
        Ok(coordinates) => coordinates,
        Err(error) => {
            findings.push(format!("product coordinates: {error}"));
            return;
        }
    };
    if let Some(uniffi) = read_toml(root, "core/crates/harvestcircle_ffi/uniffi.toml", findings) {
        let kotlin = uniffi
            .get("crates")
            .and_then(|v| v.get("harvestcircle_ffi"))
            .and_then(|v| v.get("bindings"))
            .and_then(|v| v.get("kotlin"));
        if [
            ("package_name", "ffi.kotlin_package"),
            ("cdylib_name", "ffi.cdylib_name"),
        ]
        .iter()
        .any(|(key, coordinate)| {
            kotlin
                .and_then(|v| v.get(key))
                .and_then(toml::Value::as_str)
                != coordinates.get(*coordinate).map(String::as_str)
        }) {
            findings.push(
                "core/crates/harvestcircle_ffi/uniffi.toml: final FFI identity changed".to_owned(),
            );
        }
    }
    let baseline = read_text(
        root,
        "core/compatibility/harvestcircle-ffi-v5.properties",
        findings,
    );
    let baseline = properties(&baseline);
    if baseline.get("contract.id").map(String::as_str) != Some("harvestcircle-desktop-ffi-v5")
        || baseline.get("contract.major").map(String::as_str) != Some("5")
    {
        findings.push(
            "core/compatibility/harvestcircle-ffi-v5.properties: FFI v5 identity changed"
                .to_owned(),
        );
    }
    if !inventory
        .paths
        .iter()
        .any(|path| path == "scripts/check-linux-x86_64.sh")
    {
        findings
            .push("scripts/check-linux-x86_64.sh: Linux development checker is missing".to_owned());
    }
}

pub(super) fn git_source_policy(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let Some(deny) = read_toml(root, "core/deny.toml", findings) else {
        return;
    };
    let policy = deny.get("sources");
    if policy
        .and_then(|p| p.get("required-git-spec"))
        .and_then(toml::Value::as_str)
        != Some("rev")
    {
        findings
            .push("core/deny.toml: cargo-deny must require revision-pinned Git sources".to_owned());
    }
    let allowed = policy
        .and_then(|p| p.get("allow-git"))
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_str)
        .collect::<BTreeSet<_>>();
    if allowed.is_empty() {
        findings.push("core/deny.toml: cargo-deny Git allowlist is empty".to_owned());
    }
    let mut inspected = false;
    for path in inventory
        .paths
        .iter()
        .filter(|p| p.starts_with("core/") && p.ends_with("Cargo.toml"))
    {
        let Some(manifest) = read_toml(root, path, findings) else {
            continue;
        };
        for (name, dependency) in cargo_dependencies(&manifest, true) {
            let Some(git) = dependency.get("git").and_then(toml::Value::as_str) else {
                continue;
            };
            inspected = true;
            if !allowed.contains(git) {
                findings.push(format!(
                    "{path}: {name}: Git dependency source is not allowlisted"
                ));
            }
            if dependency.get("branch").is_some() || dependency.get("tag").is_some() {
                findings.push(format!(
                    "{path}: {name}: Git dependency uses a branch or tag"
                ));
            }
            if !dependency
                .get("rev")
                .and_then(toml::Value::as_str)
                .is_some_and(|v| is_lower_hex(v, 40))
            {
                findings.push(format!(
                    "{path}: {name}: Git dependency must use one full revision pin"
                ));
            }
        }
    }
    if !inspected {
        findings.push("core: no revision-pinned Git dependencies were inspected".to_owned());
    }
    if let Some(lock) = read_toml(root, "core/Cargo.lock", findings) {
        for package in lock
            .get("package")
            .and_then(toml::Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(source) = package
                .get("source")
                .and_then(toml::Value::as_str)
                .and_then(|s| s.strip_prefix("git+"))
            else {
                continue;
            };
            let immutable = source.split_once("?rev=").is_some_and(|(url, suffix)| {
                allowed.contains(url)
                    && suffix
                        .split_once('#')
                        .is_some_and(|(rev, resolved)| is_lower_hex(rev, 40) && rev == resolved)
            });
            if !immutable {
                findings.push(
                    "core/Cargo.lock: Git source is not immutable and allowlisted".to_owned(),
                );
            }
        }
    }
}
