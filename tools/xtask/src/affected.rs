//! Conservative diagnostics. Selection and input equality never certify execution.
use crate::runtime_ownership::{Owner, OwnershipMap};
use crate::{
    Command, Inventory, bounded_no_follow_bytes, git_command, load_ownership, run,
    validate_product_root,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = git_command(root)
        .args(args)
        .output()
        .map_err(|_| "Git unavailable".to_owned())?;
    if !output.status.success() {
        return Err(format!("Git operation failed: {}", args[0]));
    }
    Ok(output.stdout)
}

fn revision(root: &Path, name: &str) -> Result<String, String> {
    if name.is_empty() || name.len() > 1024 || name.chars().any(char::is_control) {
        return Err("invalid revision argument".to_owned());
    }
    let expression = format!("{name}^{{commit}}");
    let bytes = git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &expression],
    )?;
    let value = std::str::from_utf8(&bytes)
        .map_err(|_| "invalid revision output")?
        .trim();
    if value.len() != 40 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("unsupported Git object identity".to_owned());
    }
    Ok(value.to_owned())
}

pub fn discover_root(start: &Path) -> Result<PathBuf, String> {
    let mut root = start
        .canonicalize()
        .map_err(|_| "unable to resolve current directory")?;
    loop {
        if root
            .join("config/product/harvestcircle-v1.properties")
            .is_file()
        {
            return validate_product_root(&root);
        }
        if !root.pop() {
            return Err("no HarvestCircle product root above current directory".to_owned());
        }
    }
}

/// Parse Git's NUL-delimited name-status stream without shell/path quoting.
fn changed_paths(bytes: &[u8]) -> Result<(BTreeSet<String>, bool), String> {
    if !bytes.is_empty() && bytes.last() != Some(&0) {
        return Err("unterminated Git name-status stream".to_owned());
    }
    let mut tokens = bytes.split(|b| *b == 0).collect::<Vec<_>>();
    if tokens.last() == Some(&&b""[..]) {
        tokens.pop();
    }
    let mut position = 0;
    let mut paths = BTreeSet::new();
    let mut ambiguous = false;
    while position < tokens.len() {
        let status = std::str::from_utf8(tokens[position]).map_err(|_| "invalid Git status")?;
        position += 1;
        let count = match status.as_bytes().first() {
            Some(b'R' | b'C')
                if status[1..].bytes().all(|b| b.is_ascii_digit()) && status.len() > 1 =>
            {
                ambiguous = true;
                2
            }
            Some(b'D') if status == "D" => {
                ambiguous = true;
                1
            }
            Some(b'A' | b'M' | b'T') if status.len() == 1 => 1,
            _ => return Err("ambiguous Git status".to_owned()),
        };
        for _ in 0..count {
            let raw = tokens.get(position).ok_or("missing Git path")?;
            let path = std::str::from_utf8(raw).map_err(|_| "Git path is not UTF-8")?;
            // Reuse the ownership parser's literal path validation, including escapes.
            // The report caller classifies every admitted path before narrow selection.
            if path.is_empty() {
                return Err("empty Git path".to_owned());
            }
            paths.insert(path.to_owned());
            position += 1;
        }
    }
    Ok((paths, ambiguous))
}

fn selection(root: &Path, map: &OwnershipMap, base: Option<&str>, head: &str) -> Value {
    let mut reasons = Vec::new();
    let mut paths = BTreeSet::new();
    let mut selected = BTreeSet::new();
    let mut merge_base = None;
    let mut resolved_head = None;
    let mut checked_out_head = None;
    let result = (|| -> Result<(), String> {
        let top = git(root, &["rev-parse", "--show-toplevel"])?;
        let top = std::str::from_utf8(&top)
            .map_err(|_| "Git root is not UTF-8")?
            .trim_end_matches('\n');
        if Path::new(top).canonicalize().ok().as_deref() != Some(root) {
            return Err(
                "capsule has no independent Git history; enclosing history is not its base"
                    .to_owned(),
            );
        }
        let current = revision(root, "HEAD")?;
        checked_out_head = Some(current.clone());
        if git(root, &["rev-parse", "--is-shallow-repository"])? != b"false\n" {
            return Err("shallow history cannot prove complete selection".to_owned());
        }
        let requested = revision(root, head)?;
        resolved_head = Some(requested.clone());
        if current != requested {
            return Err("requested head differs from checked-out source".to_owned());
        }
        let base = revision(root, base.ok_or("base not supplied")?)?;
        let common = git(root, &["merge-base", &base, &requested])?;
        let common = std::str::from_utf8(&common)
            .map_err(|_| "invalid merge base")?
            .trim()
            .to_owned();
        if common.len() != 40 {
            return Err("missing unique merge base".to_owned());
        }
        merge_base = Some(common.clone());
        for pair in [vec![common.as_str(), requested.as_str()], vec!["HEAD"]] {
            let mut args = vec![
                "diff",
                "--name-status",
                "-z",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
            ];
            args.extend(pair);
            args.push("--");
            let (changed, ambiguous) = changed_paths(&git(root, &args)?)?;
            paths.extend(changed);
            if ambiguous {
                reasons.push("rename, copy or deletion widens all runtimes".to_owned());
            }
        }
        let untracked = git(
            root,
            &[
                "ls-files",
                "--others",
                "--exclude-standard",
                "-z",
                "--",
                ".",
            ],
        )?;
        for raw in untracked.split(|b| *b == 0).filter(|p| !p.is_empty()) {
            paths.insert(
                std::str::from_utf8(raw)
                    .map_err(|_| "untracked path is not UTF-8")?
                    .to_owned(),
            );
        }
        for path in &paths {
            let classification = map.classify(path)?;
            if let Some(reason) = classification.reason {
                reasons.push(reason);
            }
            for owner in classification.owners {
                if owner == Owner::Governance {
                    selected.extend([Owner::Native, Owner::Web, Owner::Interop]);
                } else {
                    selected.insert(owner);
                }
            }
        }
        Ok(())
    })();
    if let Err(reason) = result {
        reasons.push(reason);
    }
    if !reasons.is_empty() {
        selected.extend([Owner::Native, Owner::Web, Owner::Interop]);
    }
    json!({"merge_base": merge_base, "resolved_head": resolved_head, "checked_out_head": checked_out_head, "changed_paths": paths,
        "reasons": reasons, "native": selected.contains(&Owner::Native), "web": selected.contains(&Owner::Web),
        "interop": selected.contains(&Owner::Interop), "repository_check": "REQUIRED_FRESH", "execution": "NOT_EXECUTED"})
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn tool(root: &Path, program: &str, arguments: &[&str]) -> Option<String> {
    let output = ProcessCommand::new(program)
        .args(arguments)
        .current_dir(root)
        .env("COREPACK_ENABLE_NETWORK", "0")
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JAVA_OPTS")
        .env_remove("GRADLE_OPTS")
        .env_remove("NODE_OPTIONS")
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.len() + output.stderr.len() > 65536 {
        return None;
    }
    let mut bytes = output.stdout;
    bytes.extend(output.stderr);
    let text = std::str::from_utf8(&bytes).ok()?.trim().to_owned();
    (!text.is_empty()).then_some(text)
}

fn cached_gradle(root: &Path) -> Option<String> {
    let properties = bounded_no_follow_bytes(
        root,
        Path::new("gradle/wrapper/gradle-wrapper.properties"),
        65536,
    )
    .ok()?;
    let properties = std::str::from_utf8(&properties).ok()?;
    let version = properties
        .lines()
        .find(|line| line.starts_with("distributionUrl="))?
        .rsplit_once("/gradle-")?
        .1
        .strip_suffix("-bin.zip")?;
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let cache = std::env::var_os("GRADLE_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".gradle")))?;
    let directory = cache
        .join("wrapper/dists")
        .join(format!("gradle-{version}-bin"));
    let mut candidates = std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(format!("gradle-{version}/bin/gradle")))
        .filter(|path| path.is_file())
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return None;
    }
    let executable = candidates.pop()?;
    tool(root, executable.to_str()?, &["--offline", "--version"])
}

const COMPILER_ENV: &[&str] = &[
    "CC", "CXX", "AR", "LD", "RANLIB", "CFLAGS", "CXXFLAGS", "CPPFLAGS", "LDFLAGS",
];

fn relevant_native_environment(name: &str) -> bool {
    [
        "CARGO_BUILD_",
        "CARGO_TARGET_",
        "CARGO_PROFILE_",
        "ORG_GRADLE_PROJECT_",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
        || COMPILER_ENV.iter().any(|base| {
            name == *base
                || name.starts_with(&format!("{base}_"))
                || name == format!("HOST_{base}")
                || name == format!("TARGET_{base}")
        })
}

fn environment_identity(
    fixed: &[&str],
    dynamic_native: bool,
) -> Result<serde_json::Map<String, Value>, String> {
    let mut names = fixed
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    if dynamic_native {
        for (name, _) in std::env::vars_os() {
            if let Some(name) = name.to_str() {
                if relevant_native_environment(name) {
                    names.insert(name.to_owned());
                }
            } else if name.as_encoded_bytes().starts_with(b"CARGO_BUILD_")
                || name.as_encoded_bytes().starts_with(b"CARGO_TARGET_")
                || name.as_encoded_bytes().starts_with(b"CARGO_PROFILE_")
                || name.as_encoded_bytes().starts_with(b"ORG_GRADLE_PROJECT_")
                || COMPILER_ENV.iter().any(|base| {
                    name.as_encoded_bytes()
                        .starts_with(format!("{base}_").as_bytes())
                })
            {
                return Err("non-UTF-8 build environment name".to_owned());
            }
        }
    }
    if names.len() > 128 {
        return Err("too many relevant build environment inputs".to_owned());
    }
    let mut result = serde_json::Map::new();
    let mut total = 0;
    for name in names {
        if name.len() > 256 || name.chars().any(char::is_control) {
            return Err("invalid build environment name".to_owned());
        }
        let value = std::env::var_os(&name)
            .map(|value| {
                value
                    .into_string()
                    .map_err(|_| "non-UTF-8 build environment value")
            })
            .transpose()?;
        if let Some(value) = &value {
            total += value.len();
            if value.len() > 65536 || total > 256 * 1024 {
                return Err("oversized build environment input set".to_owned());
            }
        }
        result.insert(
            name,
            value.map_or(Value::Null, |value| json!(digest(value.as_bytes()))),
        );
    }
    Ok(result)
}

fn external_configuration_present(root: &Path, native: bool) -> bool {
    if !native {
        return false;
    }
    // Inspect existence only, never read external configuration/credentials. Such
    // configuration needs its own authority; default source diagnostics cannot
    // certify its contents from a path selector or the reported compiler version.
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")));
    let gradle_home = std::env::var_os("GRADLE_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".gradle")));
    let mut candidates = Vec::new();
    if let Some(home) = cargo_home {
        candidates.extend([home.join("config"), home.join("config.toml")]);
    }
    if let Some(home) = gradle_home {
        candidates.extend([
            home.join("gradle.properties"),
            home.join("init.gradle"),
            home.join("init.gradle.kts"),
            home.join("init.d"),
        ]);
    }
    for parent in root.ancestors().skip(1) {
        candidates.extend([
            parent.join(".cargo/config"),
            parent.join(".cargo/config.toml"),
        ]);
    }
    candidates
        .iter()
        .any(|path| std::fs::symlink_metadata(path).is_ok())
}

fn fingerprint(
    root: &Path,
    map: &OwnershipMap,
    inventory: &Inventory,
    owner: Owner,
) -> Result<Value, String> {
    let mut files = Vec::new();
    for path in &inventory.paths {
        let metadata = std::fs::symlink_metadata(root.join(path))
            .map_err(|_| format!("missing input: {path}"))?;
        if metadata.is_dir() {
            continue;
        } // Archive inventories include ordinary directories.
        let owners = map.classify(path)?.owners;
        // Interop/governance are shared proof inputs, even for a runtime-only command.
        if !owners.contains(&owner)
            && !owners.contains(&Owner::Governance)
            && !owners.contains(&Owner::Interop)
        {
            continue;
        }
        let bytes = bounded_no_follow_bytes(root, Path::new(path), 32 * 1024 * 1024)?;
        #[cfg(unix)]
        let mode = {
            use std::os::unix::fs::PermissionsExt;
            metadata.permissions().mode() & 0o777
        };
        #[cfg(not(unix))]
        let mode = if metadata.permissions().readonly() {
            0o444
        } else {
            0o666
        };
        files.push(json!({"path":path,"mode":mode,"bytes":bytes.len(),"sha256":digest(&bytes)}));
    }
    let environment_names: &[&str] = if owner == Owner::Native {
        &[
            "HARVESTCIRCLE_BUILD_MODE",
            "HARVESTCIRCLE_NATIVE_FEATURES",
            "HARVESTCIRCLE_NATIVE_PROFILE",
            "SOURCE_DATE_EPOCH",
            "HARVESTCIRCLE_BUILD_RADROOTS_REVISION",
            "HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN",
            "HARVESTCIRCLE_BUILD_JAVA_TOOLCHAIN",
            "HARVESTCIRCLE_BUILD_KOTLIN_TOOLCHAIN",
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "CARGO_ENCODED_RUSTDOCFLAGS",
            "RUSTDOCFLAGS",
            "RUSTC",
            "RUSTDOC",
            "RUSTFMT",
            "RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "CARGO_HOME",
            "GRADLE_USER_HOME",
            "PATH",
            "CARGO_INCREMENTAL",
            "CC",
            "CXX",
            "AR",
            "LD",
            "RANLIB",
            "CFLAGS",
            "CXXFLAGS",
            "CPPFLAGS",
            "LDFLAGS",
            "JAVA_TOOL_OPTIONS",
            "JDK_JAVA_OPTIONS",
            "_JAVA_OPTIONS",
            "JAVA_OPTS",
            "GRADLE_OPTS",
            "JAVA_HOME",
            "RUSTUP_TOOLCHAIN",
        ]
    } else {
        &[
            "HARVESTCIRCLE_BUILD_MODE",
            "NODE_OPTIONS",
            "NODE_ENV",
            "CI",
            "COREPACK_ENABLE_NETWORK",
            "PATH",
            "PNPM_HOME",
        ]
    };
    let environment = environment_identity(environment_names, owner == Owner::Native);
    let environment_finding = environment.as_ref().err().cloned();
    let environment = environment.unwrap_or_default();
    let custom = owner == Owner::Native
        && (external_configuration_present(root, true)
            || environment
                .keys()
                .any(|name| name.starts_with("ORG_GRADLE_PROJECT_"))
            || [
                "RUSTC",
                "RUSTDOC",
                "RUSTFMT",
                "RUSTC_WRAPPER",
                "RUSTC_WORKSPACE_WRAPPER",
                "CARGO_BUILD_RUSTC",
                "CARGO_BUILD_RUSTDOC",
                "CARGO_BUILD_RUSTC_WRAPPER",
                "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
            ]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty())));
    let mut tools = serde_json::Map::new();
    if owner == Owner::Native {
        // rustup run fails if the pinned toolchain is absent; it never installs it.
        tools.insert(
            "cargo".to_owned(),
            json!(tool(
                root,
                "rustup",
                &["run", "1.97.1", "cargo", "--version"]
            )),
        );
        tools.insert(
            "rustc".to_owned(),
            json!(tool(root, "rustup", &["run", "1.97.1", "rustc", "-vV"])),
        );
        tools.insert("java".to_owned(), json!(tool(root, "java", &["-version"])));
        // Only invoke an already cached distribution, never a downloading wrapper.
        tools.insert("gradle".to_owned(), json!(cached_gradle(root)));
    } else {
        tools.insert("node".to_owned(), json!(tool(root, "node", &["--version"])));
        tools.insert(
            "pnpm".to_owned(),
            json!(tool(&root.join("web"), "corepack", &["pnpm", "--version"])),
        );
    }
    let tools_available = tools.values().all(|v| !v.is_null());
    let available = !custom && environment_finding.is_none() && tools_available;
    let input = json!({"schema":1,"files":files,"environment_sha256":environment,"tools":tools,
        "platform":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},
        "native_features_policy":"default unless explicitly supplied by HARVESTCIRCLE_NATIVE_FEATURES; no execution claim",
        "native_profile_policy":"debug unless explicitly supplied by HARVESTCIRCLE_NATIVE_PROFILE; no execution claim"});
    Ok(
        json!({"source_identity":digest(&serde_json::to_vec(&input["files"]).unwrap()),
        "input_identity":if available {Some(digest(&serde_json::to_vec(&input).unwrap()))} else {None},
        "availability":if environment_finding.is_some() {"INPUT_UNAVAILABLE"} else if !tools_available {"TOOL_UNAVAILABLE"} else if custom {"UNSUPPORTED_CUSTOM_COMMAND_INPUTS"} else {"DECLARED_INPUTS_AVAILABLE"}, "environment_finding":environment_finding, "custom_command_inputs":custom, "inputs":input,
        "scope":"repository default source-check commands; arbitrary feature/target/profile/config arguments, Make GRADLE overrides and injected init/project settings require separate evidence",
        "execution":"NOT_EXECUTED", "reuse":"NOT_ACCEPTED_BY_THIS_REPORT"}),
    )
}

pub fn report(start: &Path, base: Option<&str>, head: &str) -> Result<(String, bool), String> {
    let root = discover_root(start)?;
    let map = load_ownership(&root)?;
    // Always execute current global source policy; path selection never bypasses it.
    let safety = run(&root, Command::RepoAudit);
    let mut selected = selection(&root, &map, base, head);
    let source = selected["checked_out_head"].clone();
    let dirty = if source.is_null() {
        None
    } else {
        git(
            &root,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )
        .ok()
        .map(|bytes| !bytes.is_empty())
    };
    let mut provenance_environment = serde_json::Map::new();
    for name in [
        "HARVESTCIRCLE_BUILD_SOURCE_COMMIT",
        "HARVESTCIRCLE_BUILD_SOURCE_DIRTY",
    ] {
        let value = std::env::var_os(name)
            .map(|value| {
                value
                    .into_string()
                    .map_err(|_| "non-UTF-8 source provenance input")
            })
            .transpose()?;
        if value.as_ref().is_some_and(|value| value.len() > 65536) {
            return Err("oversized source provenance input".to_owned());
        }
        provenance_environment.insert(
            name.to_owned(),
            value.map_or(Value::Null, |value| json!(digest(value.as_bytes()))),
        );
    }
    let unavailable = |reason: String| {
        json!({"availability":"INPUT_UNAVAILABLE","finding":reason,
        "input_identity":null,"execution":"NOT_EXECUTED","reuse":"NOT_ACCEPTED_BY_THIS_REPORT"})
    };
    let (native, web) = match Inventory::load(&root) {
        Ok(inventory) => (
            fingerprint(&root, &map, &inventory, Owner::Native).unwrap_or_else(&unavailable),
            fingerprint(&root, &map, &inventory, Owner::Web).unwrap_or_else(&unavailable),
        ),
        Err(reason) => (unavailable(reason.clone()), unavailable(reason)),
    };
    if native["input_identity"].is_null() || web["input_identity"].is_null() {
        for lane in ["native", "web", "interop"] {
            selected[lane] = json!(true);
        }
        selected["reasons"].as_array_mut().unwrap().push(json!(
            "complete runtime fingerprint unavailable; checks remain required"
        ));
    }
    let report = json!({"schema":"harvestcircle.affected.v1","selection":selected,
        "source_provenance":{"head":source,"dirty":dirty,"producer_environment_sha256":provenance_environment,"artifact_evidence":"NOT_PRODUCED"},
        "repository_safety":{"status":if safety.is_ok(){"PASS_FRESH"}else{"FAIL"},"findings":safety.err().unwrap_or_default()},
        "native":native,
        "web":web});
    let green = report["repository_safety"]["status"] == "PASS_FRESH";
    Ok((serde_json::to_string_pretty(&report).unwrap() + "\n", green))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nul_status_preserves_both_literal_rename_paths() {
        let (paths, all) =
            changed_paths(b"R100\0web/old name.ts\0core/new name.rs\0M\0web/src/[id]/file.ts\0")
                .unwrap();
        assert!(all);
        assert_eq!(paths.len(), 3);
        assert!(paths.contains("web/old name.ts"));
        assert!(paths.contains("core/new name.rs"));
    }
    #[test]
    fn deletion_and_malformed_streams_cannot_be_narrowed() {
        assert!(changed_paths(b"D\0web/gone.ts\0").unwrap().1);
        for bytes in [
            &b"M\0unterminated"[..],
            &b"R100\0one\0"[..],
            &b"U\0file\0"[..],
            &b"M\0\xff\0"[..],
        ] {
            assert!(changed_paths(bytes).is_err());
        }
        assert_eq!(changed_paths(b"").unwrap(), (BTreeSet::new(), false));
    }
}
