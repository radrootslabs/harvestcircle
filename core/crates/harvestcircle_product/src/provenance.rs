use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use toml::Value;

const ROOT_KEYS: &[&str] = &[
    "schema",
    "source_product",
    "source_repository",
    "foundation_baseline",
    "canonical_radroots_repository",
    "canonical_radroots_revision",
];
const IMPORT_KEYS: &[&str] = &["component", "commit"];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceProvenance {
    root: BTreeMap<String, String>,
    imports: Vec<BTreeMap<String, String>>,
}

impl SourceProvenance {
    pub fn foundation_baseline(&self) -> &str {
        self.root
            .get("foundation_baseline")
            .expect("validated provenance has a foundation baseline")
    }

    pub fn canonical(&self) -> String {
        let mut canonical = String::new();
        for key in ROOT_KEYS {
            canonical.push_str(key);
            canonical.push('=');
            canonical.push_str(self.root.get(*key).expect("validated provenance root key"));
            canonical.push('\n');
        }
        for import in &self.imports {
            canonical.push_str("import.component=");
            canonical.push_str(import.get("component").expect("validated import component"));
            canonical.push('\n');
            canonical.push_str("import.commit=");
            canonical.push_str(import.get("commit").expect("validated import commit"));
            canonical.push('\n');
        }
        canonical
    }

    pub fn digest(&self) -> String {
        Sha256::digest(self.canonical().as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

pub fn parse(source: &str) -> Result<SourceProvenance, String> {
    if source.starts_with('\u{feff}') {
        return Err("source provenance must not contain a UTF-8 BOM".to_owned());
    }
    let table = toml::from_str::<toml::Table>(source).map_err(|error| error.to_string())?;
    let expected_root = ROOT_KEYS
        .iter()
        .copied()
        .chain(std::iter::once("import"))
        .collect::<BTreeSet<_>>();
    let actual_root = table.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if actual_root != expected_root {
        return Err("source provenance root keys do not match the contract".to_owned());
    }

    let mut root = BTreeMap::new();
    for key in ROOT_KEYS {
        let value = required_string(&table, key)?;
        validate_public_value(key, value)?;
        root.insert((*key).to_owned(), value.to_owned());
    }
    if root.get("schema").map(String::as_str) != Some("harvestcircle.source_provenance.v1") {
        return Err("source provenance schema is not supported".to_owned());
    }
    for key in ["foundation_baseline", "canonical_radroots_revision"] {
        if !is_lower_hex(root.get(key).expect("required revision"), 40) {
            return Err(format!(
                "source provenance {key} is not a canonical revision"
            ));
        }
    }

    let import_values = table
        .get("import")
        .and_then(Value::as_array)
        .ok_or_else(|| "source provenance imports must be an array of tables".to_owned())?;
    if import_values.is_empty() {
        return Err("source provenance imports must not be empty".to_owned());
    }
    let expected_import = IMPORT_KEYS.iter().copied().collect::<BTreeSet<_>>();
    let mut components = BTreeSet::new();
    let mut imports = Vec::new();
    for value in import_values {
        let import = value
            .as_table()
            .ok_or_else(|| "source provenance import must be a table".to_owned())?;
        let actual = import.keys().map(String::as_str).collect::<BTreeSet<_>>();
        if actual != expected_import {
            return Err("source provenance import keys do not match the contract".to_owned());
        }
        let component = required_string(import, "component")?;
        let commit = required_string(import, "commit")?;
        validate_public_value("component", component)?;
        if !is_lower_hex(commit, 40) {
            return Err("source provenance import commit is not canonical".to_owned());
        }
        if !components.insert(component.to_owned()) {
            return Err(format!("duplicate source provenance component {component}"));
        }
        imports.push(BTreeMap::from([
            ("component".to_owned(), component.to_owned()),
            ("commit".to_owned(), commit.to_owned()),
        ]));
    }
    imports.sort_by(|left, right| left.get("component").cmp(&right.get("component")));
    Ok(SourceProvenance { root, imports })
}

pub fn digest(source: &str) -> Result<String, String> {
    Ok(parse(source)?.digest())
}

fn required_string<'a>(table: &'a toml::Table, key: &str) -> Result<&'a str, String> {
    table
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("source provenance {key} must be a string"))
}

fn validate_public_value(key: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(format!(
            "source provenance {key} is empty or contains a control character"
        ));
    }
    Ok(())
}

fn is_lower_hex(value: &str, width: usize) -> bool {
    value.len() == width
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
