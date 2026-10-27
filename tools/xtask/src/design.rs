use super::{Inventory, is_lower_hex, is_text, read_text, read_toml, sha256_file};
use std::{collections::BTreeSet, path::Path};

pub(super) fn design_source_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    const PATH: &str = "config/design/harvestcircle-v1.toml";
    if !inventory.paths.iter().any(|path| path == PATH) {
        findings.push(format!("{PATH}: design contract is missing"));
        return;
    }
    let source = read_text(root, PATH, findings);
    let manifest = match toml::from_str::<toml::Value>(&source) {
        Ok(manifest) => manifest,
        Err(error) => {
            findings.push(format!("{PATH}: invalid design manifest: {error}"));
            return;
        }
    };
    for (key, expected) in [
        ("schema", "harvestcircle.design.v1"),
        (
            "repository",
            "https://github.com/radrootslabs/harvestcircle",
        ),
        ("license", "GPL-3.0-only"),
        ("golden_host", "macos-aarch64"),
        ("design_system_root", "app/design_system"),
        ("design_catalog_root", "tools/design_catalog"),
        (
            "application_shell_root",
            "app/shared/src/commonMain/kotlin/org/harvestcircle/ui/shell",
        ),
        (
            "golden_test_path",
            "app/shared/src/desktopTest/kotlin/org/harvestcircle/ui/shell/HarvestCircleMacGoldenTest.kt",
        ),
    ] {
        if manifest.get(key).and_then(toml::Value::as_str) != Some(expected) {
            findings.push(format!("{PATH}: invalid design authority: {key}"));
        }
    }
    for (path, key) in [
        (
            "app/shared/src/desktopTest/resources/goldens/macos-aarch64/design-surface-light.png",
            "golden_light_sha256",
        ),
        (
            "app/shared/src/desktopTest/resources/goldens/macos-aarch64/design-surface-dark.png",
            "golden_dark_sha256",
        ),
    ] {
        let expected = manifest
            .get(key)
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        if !is_lower_hex(expected, 64)
            || !inventory.paths.iter().any(|candidate| candidate == path)
            || sha256_file(root, path, findings).as_deref() != Some(expected)
        {
            findings.push(format!(
                "{path}: golden bytes differ from the design manifest"
            ));
        }
    }
    if let Some(catalog) = read_toml(root, "gradle/libs.versions.toml", findings) {
        let libraries = catalog.get("libraries").and_then(toml::Value::as_table);
        for module in [
            "org.jetbrains.compose.animation:animation",
            "org.jetbrains.compose.components:components-resources",
        ] {
            if !libraries.is_some_and(|entries| {
                entries
                    .values()
                    .any(|v| v.get("module").and_then(toml::Value::as_str) == Some(module))
            }) {
                findings.push(format!(
                    "gradle/libs.versions.toml: missing design dependency {module}"
                ));
            }
        }
        if libraries.is_some_and(|entries| {
            entries.values().any(|v| {
                v.get("module")
                    .and_then(toml::Value::as_str)
                    .is_some_and(|m| m.contains("compose.material") || m.contains("platformtools"))
            })
        }) {
            findings.push("gradle/libs.versions.toml: forbidden design dependency".to_owned());
        }
    }
    for path in &inventory.paths {
        if !is_text(path)
            || (!path.starts_with("app/design_system/")
                && !path.starts_with("tools/design_catalog/"))
        {
            continue;
        }
        let lowercase = read_text(root, path, findings).to_ascii_lowercase();
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
    font_source_audit(root, inventory, findings);
}

pub(super) fn font_source_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    const PREFIX: &str = "app/design_system/src/commonMain/composeResources/";
    let manifest = read_text(
        root,
        &format!("{PREFIX}files/licenses/inter-fonts.toml"),
        findings,
    );
    let manifest = match toml::from_str::<toml::Value>(&manifest) {
        Ok(manifest) => manifest,
        Err(error) => {
            findings.push(format!("font manifest: invalid TOML: {error}"));
            return;
        }
    };
    let license = manifest
        .get("license")
        .and_then(toml::Value::as_str)
        .unwrap_or_default();
    if license.is_empty() || license.contains(['/', '\\']) || license.starts_with('.') {
        findings.push("font manifest: invalid licence resource".to_owned());
        return;
    }
    let license_path = format!("{PREFIX}files/licenses/{license}");
    let text = read_text(root, &license_path, findings);
    if !text.contains("Copyright (c) 2016 The Inter Project Authors")
        || !text.contains("SIL OPEN FONT LICENSE Version 1.1")
        || sha256_file(root, &license_path, findings).as_deref()
            != manifest.get("license_sha256").and_then(toml::Value::as_str)
    {
        findings.push("font manifest: complete Inter licence or digest differs".to_owned());
    }
    let mut declared = BTreeSet::new();
    for font in manifest
        .get("font")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
    {
        let resource = font
            .get("resource")
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        let Some(filename) = resource.strip_prefix("../../font/") else {
            findings.push("font manifest: font resource escapes its owner".to_owned());
            continue;
        };
        if filename.is_empty() || filename.contains(['/', '\\']) || filename.starts_with('.') {
            findings.push("font manifest: invalid font resource".to_owned());
            continue;
        }
        let path = format!("{PREFIX}font/{filename}");
        if !declared.insert(path.clone())
            || sha256_file(root, &path, findings).as_deref()
                != font.get("sha256").and_then(toml::Value::as_str)
        {
            findings.push(format!("{path}: duplicate font or manifest digest differs"));
        }
    }
    let actual = inventory
        .paths
        .iter()
        .filter(|p| p.starts_with(&format!("{PREFIX}font/")) && p.ends_with(".ttf"))
        .cloned()
        .collect::<BTreeSet<_>>();
    if declared.is_empty() || declared != actual {
        findings.push("font manifest: declared fonts differ from tracked resources".to_owned());
    }
}
