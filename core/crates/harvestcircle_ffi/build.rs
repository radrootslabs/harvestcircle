use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use harvestcircle_product::{parser as product_manifest, provenance as source_provenance};
use quote::ToTokens;
use sha2::{Digest, Sha256};
use syn::{ImplItem, Item, Visibility};

const CONTRACT_SOURCES: &[&str] = &[
    "src/commands.rs",
    "src/contract.rs",
    "src/dto.rs",
    "src/lib.rs",
    "src/observer.rs",
];
const BASELINE_PATH: &str = "../../compatibility/harvestcircle-ffi-v4.properties";
const PRODUCT_MANIFEST_PATH: &str = "../../../config/product/harvestcircle-v1.properties";
const SOURCE_PROVENANCE_PATH: &str = "../../provenance/harvestcircle-v1.toml";
const STORAGE_CONTRACT_PATH: &str = "../harvestcircle_storage/src/contract.rs";
const BASELINE_KEYS: &[&str] = &[
    "schema",
    "contract.id",
    "contract.major",
    "contract.minor",
    "contract.hash",
    "product.coordinate_digest",
    "snapshot.schema",
    "storage.schema.minimum",
    "storage.schema.current",
    "product.version",
    "package.version",
    "source.provenance_digest",
    "source.foundation_baseline",
];

fn main() {
    for name in [
        "HARVESTCIRCLE_BUILD_SOURCE_COMMIT",
        "HARVESTCIRCLE_BUILD_SOURCE_DIRTY",
        "HARVESTCIRCLE_BUILD_RADROOTS_REVISION",
        "HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN",
        "HARVESTCIRCLE_BUILD_JAVA_TOOLCHAIN",
        "HARVESTCIRCLE_BUILD_KOTLIN_TOOLCHAIN",
        "SOURCE_DATE_EPOCH",
    ] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    for source in CONTRACT_SOURCES {
        println!("cargo:rerun-if-changed={source}");
    }
    println!("cargo:rerun-if-changed={STORAGE_CONTRACT_PATH}");
    println!("cargo:rerun-if-changed={BASELINE_PATH}");
    println!("cargo:rerun-if-changed={PRODUCT_MANIFEST_PATH}");
    println!("cargo:rerun-if-changed={SOURCE_PROVENANCE_PATH}");

    let baseline = parse_baseline(
        &fs::read_to_string(BASELINE_PATH).expect("read HarvestCircle FFI baseline"),
    );
    validate_baseline_inputs(&baseline);

    let mut metadata = Vec::new();
    for source in CONTRACT_SOURCES {
        collect_public_metadata(Path::new(source), &mut metadata);
    }
    metadata.push(format!(
        "storage-contract:{}",
        hex_digest(&fs::read(STORAGE_CONTRACT_PATH).expect("read storage contract"))
    ));
    metadata.sort();
    metadata.dedup();
    let normalized = metadata.join("\n");
    let contract_digest = hex_digest(normalized.as_bytes());
    assert_eq!(
        required(&baseline, "contract.hash"),
        contract_digest,
        "HarvestCircle FFI baseline hash is stale"
    );
    emit(
        "HARVESTCIRCLE_FFI_CONTRACT_ID",
        required(&baseline, "contract.id"),
    );
    emit(
        "HARVESTCIRCLE_PRODUCT_VERSION",
        required(&baseline, "product.version"),
    );
    emit(
        "HARVESTCIRCLE_PACKAGE_VERSION",
        required(&baseline, "package.version"),
    );
    emit(
        "HARVESTCIRCLE_PRODUCT_COORDINATE_DIGEST",
        required(&baseline, "product.coordinate_digest"),
    );
    emit(
        "HARVESTCIRCLE_SOURCE_PROVENANCE_DIGEST",
        required(&baseline, "source.provenance_digest"),
    );
    emit(
        "HARVESTCIRCLE_SOURCE_FOUNDATION_BASELINE",
        required(&baseline, "source.foundation_baseline"),
    );
    emit("HARVESTCIRCLE_FFI_CONTRACT_DIGEST", &contract_digest);
    let provenance_source =
        fs::read_to_string(SOURCE_PROVENANCE_PATH).expect("read source provenance as UTF-8");
    let provenance =
        source_provenance::parse(&provenance_source).expect("canonicalize source provenance");
    let mut build_provenance = Vec::new();
    for (output, input, default) in [
        (
            "HARVESTCIRCLE_BUILD_SOURCE_COMMIT",
            "HARVESTCIRCLE_BUILD_SOURCE_COMMIT",
            provenance.foundation_baseline(),
        ),
        (
            "HARVESTCIRCLE_BUILD_SOURCE_DIRTY",
            "HARVESTCIRCLE_BUILD_SOURCE_DIRTY",
            "unknown",
        ),
        (
            "HARVESTCIRCLE_BUILD_RADROOTS_REVISION",
            "HARVESTCIRCLE_BUILD_RADROOTS_REVISION",
            provenance.canonical_radroots_revision(),
        ),
        (
            "HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN",
            "HARVESTCIRCLE_BUILD_RUST_TOOLCHAIN",
            "1.97.1",
        ),
        (
            "HARVESTCIRCLE_BUILD_JAVA_TOOLCHAIN",
            "HARVESTCIRCLE_BUILD_JAVA_TOOLCHAIN",
            "unknown",
        ),
        (
            "HARVESTCIRCLE_BUILD_KOTLIN_TOOLCHAIN",
            "HARVESTCIRCLE_BUILD_KOTLIN_TOOLCHAIN",
            "unknown",
        ),
        (
            "HARVESTCIRCLE_BUILD_SOURCE_DATE_EPOCH",
            "SOURCE_DATE_EPOCH",
            "0",
        ),
    ] {
        let value = std::env::var(input).unwrap_or_else(|_| default.to_owned());
        assert_build_value(input, &value);
        emit(output, &value);
        build_provenance.push(format!("{input}={value}"));
    }
    emit(
        "HARVESTCIRCLE_BUILD_PROVENANCE_DIGEST",
        &hex_digest(build_provenance.join("\n").as_bytes()),
    );
    fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"))
            .join("ffi_contract_metadata.txt"),
        normalized,
    )
    .expect("write normalized FFI metadata");
}

fn assert_build_value(name: &str, value: &str) {
    assert!(!value.is_empty() && value.len() <= 128, "invalid {name}");
    assert!(
        value.bytes().all(|byte| byte.is_ascii_graphic()),
        "invalid {name}"
    );
    if name == "HARVESTCIRCLE_BUILD_SOURCE_DIRTY" {
        assert!(
            matches!(value, "true" | "false" | "unknown"),
            "invalid {name}"
        );
    }
    if name == "SOURCE_DATE_EPOCH" {
        let _: u64 = value
            .parse()
            .expect("SOURCE_DATE_EPOCH must be an unsigned integer");
    }
}

fn parse_baseline(source: &str) -> BTreeMap<String, String> {
    let mut values = BTreeMap::new();
    for (index, raw_line) in source.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .unwrap_or_else(|| panic!("invalid FFI baseline line {}", index + 1));
        let key = key.trim();
        let value = value.trim();
        assert!(
            !key.is_empty() && !value.is_empty(),
            "empty FFI baseline entry"
        );
        assert!(
            values.insert(key.to_owned(), value.to_owned()).is_none(),
            "duplicate FFI baseline key: {key}"
        );
    }
    let actual = values.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let expected = BASELINE_KEYS.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(
        actual, expected,
        "FFI baseline keys differ from the contract"
    );
    values
}

fn validate_baseline_inputs(baseline: &BTreeMap<String, String>) {
    assert_eq!(required(baseline, "schema"), "harvestcircle.ffi.v4");
    assert_eq!(
        required(baseline, "contract.id"),
        "harvestcircle-desktop-ffi-v4"
    );
    assert_eq!(required(baseline, "contract.major"), "4");
    assert_eq!(required(baseline, "contract.minor"), "3");
    assert_eq!(required(baseline, "snapshot.schema"), "1");
    assert_eq!(required(baseline, "storage.schema.minimum"), "1");
    assert_eq!(required(baseline, "storage.schema.current"), "1");
    assert_eq!(
        required(baseline, "product.version"),
        env!("CARGO_PKG_VERSION")
    );

    let product_source =
        fs::read_to_string(PRODUCT_MANIFEST_PATH).expect("read product manifest as UTF-8");
    let product_digest =
        product_manifest::digest(&product_source).expect("canonicalize product manifest");
    assert_eq!(
        required(baseline, "product.coordinate_digest"),
        product_digest
    );
    let provenance_source =
        fs::read_to_string(SOURCE_PROVENANCE_PATH).expect("read source provenance as UTF-8");
    let provenance =
        source_provenance::parse(&provenance_source).expect("canonicalize source provenance");
    assert_eq!(
        required(baseline, "source.provenance_digest"),
        provenance.digest()
    );
    assert_eq!(
        provenance.foundation_baseline(),
        required(baseline, "source.foundation_baseline")
    );

    assert_eq!(required(baseline, "storage.schema.current"), "1");
}

fn required<'a>(baseline: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    baseline
        .get(key)
        .unwrap_or_else(|| panic!("missing FFI baseline key: {key}"))
        .as_str()
}

fn emit(name: &str, value: &str) {
    println!("cargo:rustc-env={name}={value}");
}

fn collect_public_metadata(path: &Path, output: &mut Vec<String>) {
    let source = fs::read_to_string(path).expect("read FFI source");
    let file = syn::parse_file(&source).expect("parse FFI source");
    for item in file.items {
        match item {
            Item::Const(item) if is_public(&item.vis) => push_tokens("const", item, output),
            Item::Enum(item) if is_public(&item.vis) => push_tokens("enum", item, output),
            Item::Fn(item) if is_public(&item.vis) => push_tokens("fn", item.sig, output),
            Item::Struct(item) if is_public(&item.vis) => push_tokens("struct", item, output),
            Item::Trait(item) if is_public(&item.vis) => push_tokens("trait", item, output),
            Item::Impl(item) => {
                let owner = item.self_ty.to_token_stream().to_string();
                for member in item.items {
                    if let ImplItem::Fn(function) = member
                        && is_public(&function.vis)
                    {
                        output.push(format!("method:{owner}:{}", function.sig.to_token_stream()));
                    }
                }
            }
            _ => {}
        }
    }
}

fn push_tokens(kind: &str, value: impl ToTokens, output: &mut Vec<String>) {
    output.push(format!("{kind}:{}", value.to_token_stream()));
}

const fn is_public(visibility: &Visibility) -> bool {
    matches!(visibility, Visibility::Public(_))
}

fn hex_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
