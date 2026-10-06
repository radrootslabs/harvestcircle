// SPDX-License-Identifier: GPL-3.0-only
// Native test consumer, using this existing target's exact public Lib189 graph.
// No product engine, key, signing, network or parent documentation dependency.
#![forbid(unsafe_code)]

mod support;

use radroots_blossom::Sha256;
use radroots_event::wire::{EventWireError, Nip01EventWire};
use radroots_event_codec::verify::verify_nip01_event;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

const REVISION: &str = "189c49b74b4bafc142b00b76b296477931139e72";
const PROFILE: &str = "food_availability_profile";
const FOOD: &str = "contracts/interop/food_availability/";
const INPUTS: [&str; 11] = [
    "contracts/interop/food_availability/corpus.v1.json",
    "contracts/interop/food_availability/source_profile.v1.json",
    "contracts/interop/food_availability/provenance.v1.json",
    "contracts/interop/food_availability/oracle/src/main.rs",
    "contracts/interop/food_availability/oracle/Cargo.toml",
    "radroots.lib.source-lock.v1.toml",
    "web/tests/conformance/food-writer-rust.v1.json",
    "contracts/interop/food_availability/web_templates.v1.json",
    "contracts/interop/food_availability/numeric_boundaries.v1.json",
    "contracts/interop/food_availability/oracle/tests/native_manifest.rs",
    "contracts/interop/food_availability/oracle/tests/support/mod.rs",
];

type Inputs = BTreeMap<String, Vec<u8>>;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../..")
        .canonicalize()
        .unwrap()
}

fn bounded_read(base: &Path, name: &str) -> Result<Vec<u8>, String> {
    let mut path = base.to_owned();
    for component in Path::new(name).components() {
        let Component::Normal(segment) = component else {
            return Err("noncanonical fixture path".into());
        };
        path.push(segment);
        if fs::symlink_metadata(&path)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err("fixture symlink rejected".into());
        }
    }
    let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 1024 * 1024 {
        return Err("fixture must be a bounded regular file".into());
    }
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).unwrap() != metadata.len() {
        return Err("fixture changed during read".into());
    }
    Ok(bytes)
}

fn load() -> (Value, Inputs) {
    let base = root();
    let manifest =
        serde_json::from_slice(&bounded_read(&base, "contracts/interop/manifest.json").unwrap())
            .unwrap();
    let inputs = INPUTS
        .iter()
        .map(|name| (name.to_string(), bounded_read(&base, name).unwrap()))
        .collect();
    (manifest, inputs)
}

fn required(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn parse(inputs: &Inputs, name: &str) -> Result<Value, String> {
    serde_json::from_slice(inputs.get(name).ok_or("missing input")?)
        .map_err(|error| error.to_string())
}

fn descriptor(inputs: &Inputs, row: &Value) -> Result<(), String> {
    let path = row["path"].as_str().ok_or("missing raw path")?;
    let bytes = inputs.get(path).ok_or("undeclared raw path")?;
    required(
        row["sha256"] == Sha256::digest(bytes).to_hex(),
        "raw hash mismatch",
    )?;
    required(
        row["bytes"].as_u64() == Some(u64::try_from(bytes.len()).unwrap()),
        "raw length mismatch",
    )
}

fn consume(manifest: &Value, inputs: &Inputs, selected_profile: &str) -> Result<Value, String> {
    required(
        manifest["schema_version"] == 1
            && manifest["qualification"] == "FIXTURE_ONLY_NOT_DEPLOYED_TERA"
            && manifest["signing"] == "NOT_RUN_NO_KEYS",
        "qualification metadata changed",
    )?;
    required(
        manifest["oracle"]["revision"] == REVISION
            && manifest["oracle"]["repository"] == "https://github.com/radrootslabs/lib",
        "oracle identity changed",
    )?;
    let expected_inputs: BTreeSet<_> = INPUTS.iter().copied().collect();
    required(
        inputs.keys().map(String::as_str).collect::<BTreeSet<_>>() == expected_inputs,
        "input membership changed",
    )?;
    let descriptors = manifest["inputs"]
        .as_array()
        .ok_or("missing input descriptors")?;
    required(
        descriptors.len() == INPUTS.len(),
        "input descriptor count changed",
    )?;
    let paths = descriptors
        .iter()
        .map(|row| row["path"].as_str().ok_or("missing input path"))
        .collect::<Result<BTreeSet<_>, _>>()?;
    required(
        paths == expected_inputs,
        "input descriptor membership changed",
    )?;
    for row in descriptors {
        descriptor(inputs, row)?;
    }
    let source_lock = std::str::from_utf8(&inputs["radroots.lib.source-lock.v1.toml"])
        .map_err(|error| error.to_string())?;
    required(
        source_lock
            .lines()
            .filter(|line| line.starts_with("revision = "))
            .collect::<Vec<_>>()
            == [format!("revision = \"{REVISION}\"")],
        "source lock revision changed",
    )?;
    let corpus = parse(inputs, &(FOOD.to_owned() + "corpus.v1.json"))?;
    let profile = parse(inputs, &(FOOD.to_owned() + "source_profile.v1.json"))?;
    let provenance = parse(inputs, &(FOOD.to_owned() + "provenance.v1.json"))?;
    required(
        corpus["revision"] == REVISION
            && corpus["contract_version"] == "1.0.0"
            && profile["contract_version"] == "1.0.0"
            && profile["suite"] == PROFILE,
        "public profile changed",
    )?;
    required(
        corpus["source_sha256"]
            == Sha256::digest(&inputs[&(FOOD.to_owned() + "source_profile.v1.json")]).to_hex()
            && provenance["public_source"]["sha256"] == corpus["source_sha256"]
            && provenance["public_source"]["revision"] == REVISION,
        "public source provenance mismatch",
    )?;
    descriptor(
        inputs,
        &json!({"path":FOOD.to_owned()+"corpus.v1.json","sha256":provenance["corpus"]["sha256"],"bytes":provenance["corpus"]["bytes"]}),
    )?;
    let writers = parse(inputs, &(FOOD.to_owned() + "web_templates.v1.json"))?;
    let boundaries = parse(inputs, &(FOOD.to_owned() + "numeric_boundaries.v1.json"))?;
    required(
        boundaries["revision"] == REVISION
            && boundaries["profile"] == PROFILE
            && boundaries["contract_version"] == "1.0.0"
            && boundaries["schema_version"] == 1,
        "boundary profile changed",
    )?;
    let public = corpus["vectors"]
        .as_array()
        .ok_or("missing public vectors")?;
    let source = profile["vectors"]
        .as_array()
        .ok_or("missing source recipes")?;
    let writers = writers.as_array().ok_or("missing website recipes")?;
    let boundaries = boundaries["vectors"]
        .as_array()
        .ok_or("missing boundary recipes")?;
    required(
        public.len() == 40 && source.len() == 40 && writers.len() == 16 && boundaries.len() == 10,
        "fixture membership changed",
    )?;
    let mut recipes = BTreeMap::new();
    for (index, row) in public.iter().enumerate() {
        required(
            json!({"id":row["id"],"kind":row["kind"],"input":row["input"],"expected":row["expected"]})
                == source[index],
            "public recipe differs from source",
        )?;
        recipes.insert(
            row["id"].as_str().ok_or("missing public ID")?,
            ("corpus.v1.json", index, row),
        );
    }
    for (index, row) in writers.iter().enumerate() {
        required(
            recipes
                .insert(
                    row["id"].as_str().ok_or("missing writer ID")?,
                    ("web_templates.v1.json", index, row),
                )
                .is_none(),
            "duplicate writer ID",
        )?;
    }
    for (index, row) in boundaries.iter().enumerate() {
        required(
            recipes
                .insert(
                    row["id"].as_str().ok_or("missing boundary ID")?,
                    ("numeric_boundaries.v1.json", index, row),
                )
                .is_none(),
            "duplicate boundary ID",
        )?;
    }
    let cases = manifest["cases"]
        .as_array()
        .ok_or("missing manifest cases")?;
    required(
        cases.len() == recipes.len() && recipes.len() == 66,
        "manifest cases missing",
    )?;
    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    for case in cases {
        let id = case["id"].as_str().ok_or("missing case ID")?;
        required(seen.insert(id), "duplicate case ID")?;
        let (file, index, row) = recipes.get(id).ok_or("unknown case ID")?;
        required(
            case["protocol"]["profile"] == PROFILE
                && case["protocol"]["contract_version"] == "1.0.0"
                && case["oracle_revision"] == REVISION
                && case["participation"]["native_consumer"] == "REQUIRED_NATIVE_TEST",
            "case participation changed",
        )?;
        required(
            case["fixture"]["path"] == FOOD.to_owned() + file
                && case["fixture"]["case_index"].as_u64() == Some(u64::try_from(*index).unwrap())
                && case["fixture"]["case_id"] == id,
            "raw case pointer changed",
        )?;
        descriptor(inputs, &case["fixture"])?;
        let expected = if *file == "web_templates.v1.json" {
            json!({"result":"accepted","wire_parts":row["wire_parts"]})
        } else {
            row["expected"].clone()
        };
        required(
            case["expected"] == expected,
            "case expectation differs from recipe",
        )?;
        if case["protocol"]["profile"] != selected_profile {
            continue;
        }
        let (actual, expected) = match *file {
            "corpus.v1.json" => {
                for (field, raw) in row["signed_wires"]
                    .as_object()
                    .ok_or("missing signed wires")?
                {
                    let raw = raw.as_str().ok_or("invalid signed wire")?;
                    let wire =
                        Nip01EventWire::parse_json(raw).map_err(|error| error.to_string())?;
                    wire.verify_id().map_err(|error| error.to_string())?;
                    required(
                        serde_json::from_str::<Value>(raw).map_err(|error| error.to_string())?
                            == row["input"][field],
                        "signed raw differs from recipe",
                    )?;
                }
                (support::actual(row), expected)
            }
            "web_templates.v1.json" => {
                let template =
                    serde_json::from_value((*row).clone()).map_err(|error| error.to_string())?;
                let mut actual = support::consume_template(&template)?;
                actual.as_object_mut().unwrap().remove("id");
                (actual, expected)
            }
            "numeric_boundaries.v1.json" => (support::boundary(row), expected["native"].clone()),
            _ => return Err("unhandled fixture".into()),
        };
        required(
            actual == expected,
            &format!("actual pinned codec differs for {id}"),
        )?;
        results.push(json!({"id":id,"result":"PASS","actual":actual}));
    }
    required(!results.is_empty(), "zero matching native cases")?;
    Ok(
        json!({"consumer":"native_public_food_codec","revision":REVISION,"qualification":"FIXTURE_ONLY_NOT_DEPLOYED_TERA","signing":"NOT_RUN_NO_KEYS","matched_cases":results.len(),"cases":results}),
    )
}

#[test]
fn native_manifest_executes_actual_pinned_food_codec() {
    let (manifest, inputs) = load();
    let result = consume(&manifest, &inputs, PROFILE).unwrap();
    assert_eq!(result["matched_cases"], 66);
    println!("{}", serde_json::to_string(&result).unwrap());
}

#[test]
fn native_consumer_rejects_corruption_and_zero_matching_cases() {
    let (manifest, inputs) = load();
    for mutation in [
        "duplicate",
        "missing",
        "hash",
        "revision",
        "path",
        "expected",
        "participation",
    ] {
        let mut changed = manifest.clone();
        match mutation {
            "duplicate" => changed["cases"][1]["id"] = changed["cases"][0]["id"].clone(),
            "missing" => {
                changed["cases"].as_array_mut().unwrap().pop();
            }
            "hash" => changed["cases"][0]["fixture"]["sha256"] = json!("0".repeat(64)),
            "revision" => changed["oracle"]["revision"] = json!("0".repeat(40)),
            "path" => changed["cases"][0]["fixture"]["path"] = json!("web/static/private.json"),
            "expected" => changed["cases"][0]["expected"] = json!({"result":"wrong"}),
            "participation" => {
                changed["cases"][0]["participation"]["native_consumer"] = json!("PASS")
            }
            _ => unreachable!(),
        }
        assert!(consume(&changed, &inputs, PROFILE).is_err(), "{mutation}");
    }
    assert_eq!(
        consume(&manifest, &inputs, "unavailable_profile").unwrap_err(),
        "zero matching native cases"
    );
    let mut changed = inputs.clone();
    changed.get_mut(INPUTS[0]).unwrap().push(b' ');
    assert!(consume(&manifest, &changed, PROFILE).is_err());
    changed = inputs.clone();
    changed.remove(INPUTS[0]);
    assert!(consume(&manifest, &changed, PROFILE).is_err());
}

#[test]
fn actual_native_codec_rejects_noncanonical_price_and_changed_signed_bytes() {
    let (_, inputs) = load();
    let mut writers = parse(&inputs, &(FOOD.to_owned() + "web_templates.v1.json")).unwrap();
    let tags = writers[0]["wire_parts"]["tags"].as_array_mut().unwrap();
    let price = tags.iter_mut().find(|tag| tag[0] == "price").unwrap();
    price[1] = json!("1e3");
    let template = serde_json::from_value(writers[0].clone()).unwrap();
    assert!(support::consume_template(&template).is_err());
    let corpus = parse(&inputs, INPUTS[0]).unwrap();
    let mut event: Value = serde_json::from_str(
        corpus["vectors"][13]["signed_wires"]["event"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let original_signature = event["sig"].clone();
    event["sig"] = json!("0".repeat(128));
    let wire = Nip01EventWire::parse_json(&serde_json::to_string(&event).unwrap()).unwrap();
    wire.verify_id().unwrap();
    assert!(verify_nip01_event(wire.into_envelope().unwrap()).is_err());
    event["sig"] = original_signature;
    event["id"] = json!("0".repeat(64));
    // This actual public parser checks the canonical identifier before it
    // returns a wire. Assert that stage, rather than unwrapping a rejection.
    assert!(matches!(
        Nip01EventWire::parse_json(&serde_json::to_string(&event).unwrap()),
        Err(EventWireError::EventIdMismatch { .. })
    ));
}

#[test]
fn actual_native_boundaries_preserve_full_u64_and_raw_identifier_spelling() {
    let (_, inputs) = load();
    let fixture = parse(&inputs, &(FOOD.to_owned() + "numeric_boundaries.v1.json")).unwrap();
    let rows = fixture["vectors"].as_array().unwrap();
    for row in rows {
        assert_eq!(support::boundary(row), row["expected"]["native"]);
    }
    assert_eq!(
        support::boundary(&rows[6])["timestamp"],
        u64::MAX.to_string()
    );
    assert_ne!(
        support::boundary(&rows[8])["identifier"],
        support::boundary(&rows[9])["identifier"]
    );
}
