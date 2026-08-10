use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

pub const REQUIRED: &[(&str, &str)] = &[
    ("schema", "harvestcircle.product.v1"),
    ("product.name", "HarvestCircle"),
    ("product.slug", "harvestcircle"),
    ("kotlin.root_namespace", "org.harvestcircle"),
    ("desktop.application_id", "org.harvestcircle.desktop"),
    ("desktop.bundle_id", "org.harvestcircle.desktop"),
    ("desktop.main_class", "org.harvestcircle.desktop.MainKt"),
    ("ffi.kotlin_package", "org.harvestcircle.ffi"),
    ("ffi.cdylib_name", "harvestcircle_ffi"),
    ("database.qualifier", "org"),
    ("database.organization", "harvestcircle"),
    ("database.application", "desktop"),
    ("database.filename", "harvestcircle.sqlite3"),
    ("keyring.service", "org.harvestcircle.desktop.nostr"),
    ("environment.prefix", "HARVESTCIRCLE_"),
    ("vendor.name", "Radroots Labs"),
    (
        "copyright.notice",
        "Copyright © 2026 HarvestCircle contributors",
    ),
];

pub fn parse(source: &str) -> Result<BTreeMap<String, String>, String> {
    if source.starts_with('\u{feff}') {
        return Err("product coordinates must not contain a UTF-8 BOM".to_owned());
    }
    let mut parsed = BTreeMap::new();
    for (index, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("line {} is not key=value", index + 1))?;
        let key = key.trim();
        let value = value.trim();
        if key.is_empty()
            || value.is_empty()
            || key.chars().any(char::is_control)
            || value.chars().any(char::is_control)
        {
            return Err(format!("line {} has an invalid key or value", index + 1));
        }
        if !REQUIRED.iter().any(|(required, _)| *required == key) {
            return Err(format!("unknown coordinate {key}"));
        }
        if parsed.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(format!("duplicate coordinate {key}"));
        }
    }
    for (key, expected) in REQUIRED {
        match parsed.get(*key) {
            Some(actual) if actual == expected => {}
            Some(_) => {
                return Err(format!(
                    "coordinate {key} does not match the approved value"
                ));
            }
            None => return Err(format!("missing coordinate {key}")),
        }
    }
    Ok(parsed)
}

pub fn canonicalize(source: &str) -> Result<String, String> {
    let parsed = parse(source)?;
    let mut canonical = String::new();
    for (key, _) in REQUIRED {
        canonical.push_str(key);
        canonical.push('=');
        canonical.push_str(parsed.get(*key).expect("required coordinate was validated"));
        canonical.push('\n');
    }
    Ok(canonical)
}

pub fn digest(source: &str) -> Result<String, String> {
    let canonical = canonicalize(source)?;
    Ok(Sha256::digest(canonical.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
