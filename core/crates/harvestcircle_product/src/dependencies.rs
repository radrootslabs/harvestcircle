//! Immutable shared dependency identity derived from Cargo's own inputs.
use toml::Value;

pub fn lib_revision(manifest: &str, lockfile: &str) -> Result<String, String> {
    let manifest: Value =
        toml::from_str(manifest).map_err(|e| format!("invalid Cargo manifest: {e}"))?;
    let dependencies = manifest
        .get("workspace")
        .and_then(|v| v.get("dependencies"))
        .and_then(Value::as_table)
        .ok_or("missing workspace dependencies")?;
    let lock: Value =
        toml::from_str(lockfile).map_err(|e| format!("invalid Cargo lockfile: {e}"))?;
    let packages = lock
        .get("package")
        .and_then(Value::as_array)
        .ok_or("missing locked packages")?;
    let mut selected: Option<String> = None;
    for (name, value) in dependencies
        .iter()
        .filter(|(name, _)| name.starts_with("radroots_"))
    {
        let table = value
            .as_table()
            .ok_or("shared dependency must be a Git table")?;
        if table.get("git").and_then(Value::as_str) != Some("https://github.com/radrootslabs/lib")
            || ["path", "branch", "tag"]
                .iter()
                .any(|key| table.contains_key(*key))
        {
            return Err(format!(
                "{name}: shared source must be canonical immutable public Git"
            ));
        }
        let revision = table
            .get("rev")
            .and_then(Value::as_str)
            .ok_or("shared dependency revision is missing")?;
        if revision.len() != 40
            || !revision
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(format!("{name}: full lowercase revision required"));
        }
        if selected.as_deref().is_some_and(|v| v != revision) {
            return Err("mixed shared dependency revisions".to_owned());
        }
        selected = Some(revision.to_owned());
        let version = table
            .get("version")
            .and_then(Value::as_str)
            .and_then(|v| v.strip_prefix('='))
            .ok_or("shared dependency version must be exact")?;
        let source = format!("git+https://github.com/radrootslabs/lib?rev={revision}#{revision}");
        if packages
            .iter()
            .filter(|p| p.get("name").and_then(Value::as_str) == Some(name.as_str()))
            .count()
            != 1
            || !packages.iter().any(|p| {
                p.get("name").and_then(Value::as_str) == Some(name.as_str())
                    && p.get("version").and_then(Value::as_str) == Some(version)
                    && p.get("source").and_then(Value::as_str) == Some(source.as_str())
            })
        {
            return Err(format!(
                "{name}: Cargo lockfile differs from selected source/version"
            ));
        }
    }
    let selected = selected.ok_or("no shared dependencies declared")?;
    let source = format!("git+https://github.com/radrootslabs/lib?rev={selected}#{selected}");
    for package in packages.iter().filter(|p| {
        p.get("name")
            .and_then(Value::as_str)
            .is_some_and(|v| v.starts_with("radroots_"))
    }) {
        if package.get("source").and_then(Value::as_str) != Some(source.as_str()) {
            return Err("locked shared packages contain mixed or redirected sources".to_owned());
        }
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn real_cargo_inputs_are_consistent_and_tampering_is_rejected() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
        let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
        let revision = lib_revision(&manifest, &lock).unwrap();
        assert_eq!(revision.len(), 40);
        assert!(lib_revision(&manifest.replace("rev =", "branch ="), &lock).is_err());
        assert!(lib_revision(&manifest.replacen(&revision, &"a".repeat(40), 1), &lock).is_err());
        assert!(lib_revision(&manifest, &lock.replace(&revision, &"b".repeat(40))).is_err());
        assert!(lib_revision(&manifest, "version = 4").is_err());
        assert!(lib_revision("[workspace.dependencies]", &lock).is_err());
        assert!(
            lib_revision(
                &manifest.replace(
                    "https://github.com/radrootslabs/lib",
                    "https://example.invalid/lib"
                ),
                &lock
            )
            .is_err()
        );
    }
}
