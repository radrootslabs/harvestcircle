//! Runtime input ownership data. Output declarations never exempt source from audits.
use serde::Deserialize;
use std::collections::BTreeSet;

pub const MAP_PATH: &str = "config/repository/runtime-ownership.v1.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "snake_case")]
pub enum Owner {
    Native,
    Web,
    Interop,
    Governance,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Exact,
    Prefix,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    path: String,
    kind: Kind,
    owners: Vec<Owner>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnershipMap {
    schema_version: u32,
    owners: Vec<Owner>,
    inputs: Vec<Rule>,
    outputs: Vec<Rule>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Classification {
    pub owners: Vec<Owner>,
    pub reason: Option<String>,
}

fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':' | '%' | '*' | '?' | '[' | ']'))
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(format!(
            "invalid repository-relative ownership path: {path:?}"
        ));
    }
    Ok(())
}

impl Rule {
    fn matches(&self, path: &str) -> bool {
        path == self.path
            || (self.kind == Kind::Prefix
                && path
                    .strip_prefix(&self.path)
                    .is_some_and(|rest| rest.starts_with('/')))
    }
}

impl OwnershipMap {
    pub fn parse(source: &str) -> Result<Self, String> {
        // No escaped path aliases, including unicode-escaped separators or field names.
        if source.contains('\\') {
            return Err("ownership JSON escapes are not allowed".to_owned());
        }
        let map: Self = serde_json::from_str(source)
            .map_err(|error| format!("invalid ownership JSON: {error}"))?;
        if map.schema_version != 1 {
            return Err("unsupported ownership schema version".to_owned());
        }
        if map.owners.len() != 4 || map.owners.iter().copied().collect::<BTreeSet<_>>().len() != 4 {
            return Err(
                "ownership map must declare each of the four owners exactly once".to_owned(),
            );
        }
        for rules in [&map.inputs, &map.outputs] {
            if rules.is_empty() {
                return Err("ownership rule list is empty".to_owned());
            }
            for (index, rule) in rules.iter().enumerate() {
                validate_path(&rule.path)?;
                if rule.owners.is_empty()
                    || rule.owners.iter().copied().collect::<BTreeSet<_>>().len()
                        != rule.owners.len()
                {
                    return Err(format!("empty or duplicate owners for {}", rule.path));
                }
                for previous in &rules[..index] {
                    if previous.matches(&rule.path) || rule.matches(&previous.path) {
                        return Err(format!(
                            "overlapping ownership declarations: {} and {}",
                            previous.path, rule.path
                        ));
                    }
                }
            }
        }
        Ok(map)
    }

    pub fn classify(&self, path: &str) -> Result<Classification, String> {
        validate_path(path)?;
        match self.inputs.iter().find(|rule| rule.matches(path)) {
            Some(rule) => Ok(Classification {
                owners: rule.owners.clone(),
                reason: None,
            }),
            None => Ok(Classification {
                owners: self.owners.clone(),
                reason: Some(format!(
                    "unclassified input {path}: conservatively select all potential owners"
                )),
            }),
        }
    }

    pub fn validate_native_inputs(&self, paths: &[String]) -> Result<(), String> {
        for required in required_native_inputs() {
            if !paths.iter().any(|path| path == required) {
                return Err(format!("required native input is missing: {required}"));
            }
        }
        for path in paths {
            if required_native_inputs().contains(&path.as_str())
                || [
                    "app/",
                    "core/",
                    "gradle/",
                    "build-logic/",
                    "tools/design_catalog/",
                    "tools/xtask/",
                ]
                .iter()
                .any(|prefix| path.starts_with(prefix))
            {
                let classification = self.classify(path)?;
                if classification.reason.is_some()
                    || !classification.owners.contains(&Owner::Native)
                {
                    return Err(format!("native input has no explicit native owner: {path}"));
                }
            }
        }
        Ok(())
    }
}

fn required_native_inputs() -> Vec<&'static str> {
    vec![
        "build.gradle.kts",
        "settings.gradle.kts",
        "gradle.properties",
        "gradlew",
        "gradlew.bat",
        "radroots.lib.source-lock.v1.toml",
        "config/design/harvestcircle-v1.toml",
        "config/detekt/detekt.yml",
        "config/licenses/allowed-licenses.json",
        "config/product/harvestcircle-v1.properties",
        "config/verification/lanes-v3.properties",
        "contracts/release/harvestcircle-artifact-contract.v3.json",
        "contracts/rshr-201-step-gates.v1.json",
        "tools/rshr_201_step_gate.py",
        "tools/run-linux-x86_64-development-check.sh",
        "tools/test-build-logic-stability.sh",
        "tools/test-build-modes.sh",
        "tools/verify-storage-api.sh",
        "core/Cargo.toml",
        "core/Cargo.lock",
        "core/rust-toolchain.toml",
        "core/deny.toml",
        "core/provenance/harvestcircle-v1.toml",
        "tools/xtask/Cargo.toml",
        "tools/xtask/Cargo.lock",
        "gradle/libs.versions.toml",
        "gradle/verification-metadata.xml",
        "gradle/wrapper/gradle-wrapper.jar",
        "gradle/wrapper/gradle-wrapper.properties",
        "build-logic/settings.gradle.kts",
        "build-logic/contracts/build.gradle.kts",
        "build-logic/plugins/build.gradle.kts",
        "app/design_system/build.gradle.kts",
        "app/shared/build.gradle.kts",
        "app/desktop/build.gradle.kts",
        "tools/design_catalog/build.gradle.kts",
        "Makefile",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    const MAP: &str = include_str!("../../../config/repository/runtime-ownership.v1.json");

    #[test]
    fn rejects_malformed_unknown_and_duplicate_json() {
        for source in [
            "{",
            &MAP.replace("\"schema_version\": 1", "\"schema_version\": 2"),
            &MAP.replace(
                "\"schema_version\": 1",
                "\"schema_version\": 1, \"extra\": true",
            ),
            &MAP.replace(
                "\"schema_version\": 1",
                "\"schema_version\": 1, \"schema_version\": 1",
            ),
            &MAP.replace("\"native\"", "\"invented\""),
        ] {
            assert!(OwnershipMap::parse(source).is_err(), "accepted {source}");
        }
    }

    #[test]
    fn rejects_traversal_absolutes_backslashes_and_escapes() {
        for path in [
            "../app",
            "/app",
            "app/../core",
            "app/./src",
            "app//src",
            "app/",
            "C:/app",
            "app\\src",
            "app/*",
            "app/%2e%2e",
            "app\\u002fcore",
        ] {
            let source = MAP.replacen("\"app\"", &format!("\"{path}\""), 1);
            assert!(OwnershipMap::parse(&source).is_err(), "accepted {path}");
        }
    }

    #[test]
    fn rejects_ambiguous_declarations_and_accepts_explicit_shared_owners() {
        let map = OwnershipMap::parse(MAP).unwrap();
        let shared = map.classify("tools/xtask/Cargo.lock").unwrap();
        assert!(shared.owners.contains(&Owner::Native));
        assert!(shared.owners.contains(&Owner::Governance));
        let rule = r#"{"path":"app/src", "kind":"prefix", "owners":["web"]}"#;
        let source = MAP.replacen("\"inputs\": [", &format!("\"inputs\": [{rule},"), 1);
        assert!(OwnershipMap::parse(&source).is_err());
        assert!(
            OwnershipMap::parse(&MAP.replacen(
                "\"owners\": [\n        \"native\"",
                "\"owners\": [\n        \"native\", \"native\"",
                1
            ))
            .is_err()
        );
    }

    #[test]
    fn prefix_boundaries_and_unknown_inputs_widen_with_reason() {
        let map = OwnershipMap::parse(MAP).unwrap();
        assert_eq!(
            map.classify("app/src/test.kt").unwrap().owners,
            vec![Owner::Native]
        );
        for path in [
            "application/src/test.kt",
            "config/new.json",
            "tools/new.rs",
            "contracts/new.json",
            "new_runtime/main.rs",
        ] {
            let result = map.classify(path).unwrap();
            assert_eq!(result.owners.len(), 4);
            assert!(result.reason.unwrap().contains(path));
        }
        assert!(map.classify("../app").is_err());
        assert!(
            map.classify("web/src/main.ts")
                .unwrap()
                .owners
                .contains(&Owner::Web)
        );
        assert!(
            map.classify("contracts/interop/food.json")
                .unwrap()
                .owners
                .contains(&Owner::Interop)
        );
    }

    #[test]
    fn current_repository_native_inputs_are_explicitly_covered() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let inventory = crate::Inventory::load(&root).unwrap();
        let mut findings = Vec::new();
        crate::runtime_ownership_audit(&root, &inventory, &mut findings);
        assert!(findings.is_empty(), "{findings:?}");
    }

    #[test]
    fn outputs_do_not_exempt_tracked_inputs_and_exact_paths_do_not_expand() {
        let map = OwnershipMap::parse(MAP).unwrap();
        let output = map.classify("core/target/tracked-malicious.rs").unwrap();
        assert_eq!(output.owners, vec![Owner::Native]);
        assert!(map.classify("gradlew-extra").unwrap().reason.is_some());
        assert!(
            map.classify("config/design/harvestcircle-v1.toml/child")
                .unwrap()
                .reason
                .is_some()
        );
    }

    #[test]
    fn rejects_uncovered_and_missing_native_inputs() {
        let map = OwnershipMap::parse(MAP).unwrap();
        let paths = required_native_inputs()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert!(map.validate_native_inputs(&paths).is_ok());
        for missing in [
            "core/Cargo.lock",
            "tools/xtask/Cargo.lock",
            "core/rust-toolchain.toml",
        ] {
            let partial = paths
                .iter()
                .filter(|path| path.as_str() != missing)
                .cloned()
                .collect::<Vec<_>>();
            assert!(
                map.validate_native_inputs(&partial).is_err(),
                "accepted missing {missing}"
            );
        }
        let wrong = OwnershipMap::parse(&MAP.replace("\"core\",", "\"elsewhere\",")).unwrap();
        assert!(wrong.validate_native_inputs(&paths).is_err());
    }
}
