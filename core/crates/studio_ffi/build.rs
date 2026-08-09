use std::fs;
use std::path::{Path, PathBuf};

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

fn main() {
    for source in CONTRACT_SOURCES {
        println!("cargo:rerun-if-changed={source}");
    }
    println!("cargo:rerun-if-changed=../studio_storage/migrations");

    let mut metadata = Vec::new();
    for source in CONTRACT_SOURCES {
        collect_public_metadata(Path::new(source), &mut metadata);
    }
    let mut migrations = fs::read_dir("../studio_storage/migrations")
        .expect("read Studio migration catalog")
        .map(|entry| entry.expect("read migration entry").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "sql"))
        .collect::<Vec<_>>();
    migrations.sort();
    for migration in migrations {
        metadata.push(format!(
            "migration:{}:{}",
            migration
                .file_name()
                .expect("migration filename")
                .to_string_lossy(),
            hex_digest(&fs::read(&migration).expect("read migration"))
        ));
    }
    metadata.sort();
    metadata.dedup();
    let normalized = metadata.join("\n");
    println!(
        "cargo:rustc-env=RADROOTS_STUDIO_FFI_CONTRACT_DIGEST={}",
        hex_digest(normalized.as_bytes())
    );
    fs::write(
        PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"))
            .join("ffi_contract_metadata.txt"),
        normalized,
    )
    .expect("write normalized FFI metadata");
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
