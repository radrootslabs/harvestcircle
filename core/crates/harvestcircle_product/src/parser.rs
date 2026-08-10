use std::collections::BTreeMap;

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
        if key.is_empty() || value.is_empty() || value.chars().any(char::is_control) {
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
