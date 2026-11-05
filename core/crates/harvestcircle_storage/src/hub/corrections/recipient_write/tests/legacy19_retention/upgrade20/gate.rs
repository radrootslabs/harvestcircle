//! File-only qualification gate; no native database, runtime or saved input I/O.
use super::{BASE, FAILED, POPULATED, SPARSE, digest};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
};

const PREFIX: &str = "hub::corrections::recipient_write::tests::legacy19_retention::upgrade20::";

fn expected_consumer(world: &str) -> Result<String, String> {
    let name = match world {
        SPARSE => "retained_sparse19_first20_preserves_all_originals_and_pending",
        POPULATED => "retained_populated19_first20_preserves_legacy_meaning_and_pending",
        FAILED => "retained_failed19_first20_preserves_wal_world_and_unknown",
        _ => return Err("not a designated original world".into()),
    };
    Ok(format!("{PREFIX}{name}"))
}
fn read(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}
fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, String> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("missing string {field}"))
}
fn contained(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || !relative
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err("unsafe qualification path".into());
    }
    let path = root.join(relative);
    if !fs::symlink_metadata(&path)
        .map_err(|e| e.to_string())?
        .is_file()
        || !path
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(root)
    {
        return Err("qualification path is not a contained regular file".into());
    }
    Ok(path)
}
fn verify_rows(root: &Path, manifest: &Value, expected: usize) -> Result<BTreeSet<String>, String> {
    let rows = manifest["files"]
        .as_array()
        .ok_or("missing file inventory")?;
    if rows.len() != expected {
        return Err("incomplete qualification inventory".into());
    }
    let mut names = BTreeSet::new();
    for row in rows {
        let name = string(row, "path")?;
        let path = contained(root, name)?;
        if !names.insert(name.to_owned()) || digest(&path) != string(row, "sha256")? {
            return Err("duplicate or changed qualified file".into());
        }
        if let Some(bytes) = row["bytes"].as_u64()
            && fs::metadata(path).map_err(|e| e.to_string())?.len() != bytes
        {
            return Err("changed qualified length".into());
        }
    }
    Ok(names)
}
fn verify_with_counts(
    qualified: &Path,
    source: &Path,
    world: &str,
    inputs_count: usize,
    sources_count: usize,
) -> Result<(), String> {
    let bundle = qualified
        .parent()
        .ok_or("missing qualification owner")?
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let gate = read(qualified)?;
    if string(&gate, "source_base")? != BASE
        || gate["consumers"][world].as_str() != Some(expected_consumer(world)?.as_str())
    {
        return Err("qualification belongs to another source or consumer".into());
    }
    let mut manifests = Vec::new();
    for (name, field) in [
        ("inputs.json", "inputs_sha256"),
        ("sources.json", "sources_sha256"),
        ("artifacts.json", "artifacts_sha256"),
    ] {
        let path = contained(&bundle, name)?;
        if digest(&path) != string(&gate, field)? {
            return Err("changed qualification manifest".into());
        }
        manifests.push(read(&path)?);
    }
    let inputs = verify_rows(&source, &manifests[0], inputs_count)?;
    let sources = verify_rows(
        &bundle
            .join("sources")
            .canonicalize()
            .map_err(|e| e.to_string())?,
        &manifests[1],
        sources_count,
    )?;
    if !sources.is_subset(&inputs) {
        return Err("copied sources are outside qualified inputs".into());
    }
    let artifacts_count = manifests[2]["files"]
        .as_array()
        .ok_or("missing sealed artifacts")?
        .len();
    let artifacts = verify_rows(&bundle, &manifests[2], artifacts_count)?;
    if !artifacts.contains("inputs.json") || !artifacts.contains("sources.json") {
        return Err("input/source manifests are outside the artifact seal".into());
    }
    for path in sources {
        if !artifacts.contains(&format!("sources/{path}"))
            || digest(&source.join(&path)) != digest(&bundle.join("sources").join(path))
        {
            return Err("copied source differs from live qualified input".into());
        }
    }
    let executable = string(&gate, "executable")?;
    if !artifacts.contains(executable)
        || digest(&contained(&bundle, executable)?) != string(&gate, "executable_sha256")?
    {
        return Err("consumer executable is outside the sealed qualification".into());
    }
    Ok(())
}
pub(super) fn verify(qualified: &Path, world: &str) -> Result<(), String> {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .ok_or("missing source owner")?;
    verify_with_counts(qualified, source, world, 860, 59)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn save(path: &Path, value: Value) {
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn row(path: &str, root: &Path) -> Value {
        serde_json::json!({"path":path,"sha256":digest(&root.join(path)),"bytes":fs::metadata(root.join(path)).unwrap().len()})
    }
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temporary = tempfile::tempdir().unwrap();
        let source = temporary.path().join("source");
        let bundle = temporary.path().join("bundle");
        fs::create_dir_all(bundle.join("sources")).unwrap();
        fs::create_dir(&source).unwrap();
        fs::write(source.join("original.rs"), b"qualified original source").unwrap();
        fs::write(source.join("Cargo.lock"), b"qualified dependencies").unwrap();
        fs::copy(
            source.join("original.rs"),
            bundle.join("sources/original.rs"),
        )
        .unwrap();
        fs::write(bundle.join("consumer"), b"qualified executable").unwrap();
        save(
            &bundle.join("inputs.json"),
            serde_json::json!({"files":[row("original.rs",&source),row("Cargo.lock",&source)]}),
        );
        save(
            &bundle.join("sources.json"),
            serde_json::json!({"files":[row("original.rs",&bundle.join("sources"))]}),
        );
        save(
            &bundle.join("artifacts.json"),
            serde_json::json!({"files":[row("inputs.json",&bundle),row("sources.json",&bundle),row("sources/original.rs",&bundle),row("consumer",&bundle)]}),
        );
        save(
            &bundle.join("qualified-inputs.json"),
            serde_json::json!({"source_base":BASE,"executable":"consumer","executable_sha256":digest(&bundle.join("consumer")),"inputs_sha256":digest(&bundle.join("inputs.json")),"sources_sha256":digest(&bundle.join("sources.json")),"artifacts_sha256":digest(&bundle.join("artifacts.json")),"consumers":{SPARSE:expected_consumer(SPARSE).unwrap()}}),
        );
        (temporary, source, bundle.join("qualified-inputs.json"))
    }
    #[test]
    fn first20_gate_rejects_changed_live_inputs_copied_sources_and_artifacts_before_any_claim() {
        for relative in [
            "source/original.rs",
            "source/Cargo.lock",
            "bundle/sources/original.rs",
            "bundle/consumer",
            "bundle/inputs.json",
            "bundle/sources.json",
            "bundle/artifacts.json",
        ] {
            let (temporary, source, qualified) = fixture();
            assert!(verify_with_counts(&qualified, &source, SPARSE, 2, 1).is_ok());
            fs::write(
                temporary.path().join(relative),
                b"changed after qualification",
            )
            .unwrap();
            assert!(
                verify_with_counts(&qualified, &source, SPARSE, 2, 1).is_err(),
                "{relative}"
            );
        }
    }
    #[test]
    fn first20_gate_rejects_another_world_consumer_or_path_escape_before_any_claim() {
        let (_temporary, source, qualified) = fixture();
        assert!(verify_with_counts(&qualified, &source, FAILED, 2, 1).is_err());
        let mut gate = read(&qualified).unwrap();
        gate["consumers"][SPARSE] = Value::String("another consumer".into());
        save(&qualified, gate);
        assert!(verify_with_counts(&qualified, &source, SPARSE, 2, 1).is_err());
        assert!(contained(&source, "../source/original.rs").is_err());
        assert!(contained(&source, "/absolute/source.rs").is_err());
    }
}
