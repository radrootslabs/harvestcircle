//! Validated, generated HarvestCircle product coordinates and provenance.

pub mod parser;
pub mod provenance;

include!(concat!(env!("OUT_DIR"), "/product_coordinates.rs"));

#[cfg(test)]
mod tests {
    use super::parser::{REQUIRED_KEYS, canonicalize, digest, generate_rust_constants, parse};
    use super::provenance;
    use super::{
        DESKTOP_APPLICATION_ID, DEVELOPMENT_DATA_DIR_ENVIRONMENT, FFI_CDYLIB_NAME,
        KOTLIN_ROOT_NAMESPACE, PRODUCT_COORDINATE_DIGEST, PRODUCT_NAME,
    };

    #[test]
    fn generated_coordinates_are_exact_and_digest_is_sha256() {
        assert_eq!(PRODUCT_NAME, "HarvestCircle");
        assert_eq!(KOTLIN_ROOT_NAMESPACE, "org.harvestcircle");
        assert_eq!(DESKTOP_APPLICATION_ID, "org.harvestcircle.desktop");
        assert_eq!(FFI_CDYLIB_NAME, "harvestcircle_ffi");
        assert_eq!(
            DEVELOPMENT_DATA_DIR_ENVIRONMENT,
            "HARVESTCIRCLE_DEVELOPMENT_DATA_DIR"
        );
        assert_eq!(PRODUCT_COORDINATE_DIGEST.len(), 64);
        assert!(
            PRODUCT_COORDINATE_DIGEST
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
    }

    #[test]
    fn parser_rejects_missing_duplicate_unknown_and_changed_coordinates() {
        let source = include_str!("../../../../config/product/harvestcircle-v1.properties");
        assert!(parse(source).is_ok());
        assert!(parse(&source.replacen("schema=harvestcircle.product.v1\n", "", 1)).is_err());
        assert!(parse(&format!("{source}\nschema=harvestcircle.product.v1")).is_err());
        assert!(parse(&format!("{source}\nunknown=value")).is_err());
        assert!(
            parse(&source.replace("product.slug=harvestcircle", "product.slug=OTHER")).is_err()
        );
        assert!(
            parse(&source.replace(
                "database.filename=harvestcircle.sqlite3",
                "database.filename=../other.sqlite3"
            ))
            .is_err()
        );
        assert!(parse(&format!("\u{feff}{source}")).is_err());
    }

    #[test]
    fn every_coordinate_value_is_manifest_owned_and_generated() {
        let source = include_str!("../../../../config/product/harvestcircle-v1.properties");
        let mutations = [
            ("product.name", "Harvest Circle Test"),
            ("product.slug", "harvestcircle_test"),
            ("kotlin.root_namespace", "org.example"),
            ("desktop.application_id", "org.example.desktop"),
            ("desktop.bundle_id", "org.example.bundle"),
            ("desktop.main_class", "org.example.MainKt"),
            ("ffi.kotlin_package", "org.example.ffi"),
            ("ffi.cdylib_name", "example_ffi"),
            ("database.qualifier", "com"),
            ("database.organization", "example"),
            ("database.application", "test"),
            ("database.filename", "example.sqlite3"),
            ("keyring.service", "org.example.desktop.nostr"),
            ("environment.prefix", "EXAMPLE_"),
            ("vendor.name", "Example Cooperative"),
            ("copyright.notice", "Copyright Example contributors"),
        ];
        assert_eq!(mutations.len() + 1, REQUIRED_KEYS.len());
        for (key, replacement) in mutations {
            let original = parse(source).unwrap().remove(key).unwrap();
            let mutated = source.replace(
                &format!("{key}={original}"),
                &format!("{key}={replacement}"),
            );
            let parsed = parse(&mutated).expect("valid manifest-owned coordinate mutation");
            assert_eq!(parsed.get(key).map(String::as_str), Some(replacement));
            assert_ne!(digest(&mutated).unwrap(), digest(source).unwrap());
            assert!(
                generate_rust_constants(&mutated)
                    .unwrap()
                    .contains(&format!("= {replacement:?};"))
            );
        }
    }

    #[test]
    fn product_digest_is_semantic_and_line_ending_independent() {
        let source = include_str!("../../../../config/product/harvestcircle-v1.properties");
        let expected = digest(source).expect("canonical product digest");
        assert_eq!(
            expected,
            "93bf10e334e989b20ba5fb8ed05e5d55b83f4502efba5f893aef4dc1a66c8223"
        );
        assert_eq!(digest(&source.replace('\n', "\r\n")).unwrap(), expected);
        assert_eq!(
            digest(&format!("\n# comment\n{source}\n")).unwrap(),
            expected
        );
        assert_eq!(
            digest(
                &source
                    .lines()
                    .map(|line| line.replacen('=', " = ", 1))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
            .unwrap(),
            expected
        );
        assert!(canonicalize(&format!("\u{feff}{source}")).is_err());
    }

    #[test]
    fn provenance_digest_is_semantic_and_strict() {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../provenance")
                .join(format!("{}-import-v1.toml", ["stu", "dio"].concat())),
        )
        .expect("read canonical provenance fixture");
        let expected = provenance::digest(&source).expect("canonical provenance digest");
        assert_eq!(
            expected,
            "db238195b4a5938a8d4d9ac5681c4b125e65c57aa8133ad03e59da4e4bd062bc"
        );
        assert_eq!(
            provenance::digest(&source.replace('\n', "\r\n")).unwrap(),
            expected
        );
        assert_eq!(provenance::digest(source.trim_end()).unwrap(), expected);
        assert_eq!(
            provenance::digest(&format!("# comment\n{source}")).unwrap(),
            expected
        );
        let reordered = source.replace(
            "component = \"domain\"\ncommit = \"a4d7deebec3e2ce2c1daa455de6d79857839aed0\"",
            "commit = \"a4d7deebec3e2ce2c1daa455de6d79857839aed0\"\ncomponent = \"domain\"",
        );
        assert_eq!(provenance::digest(&reordered).unwrap(), expected);
        assert!(provenance::digest(&format!("\u{feff}{source}")).is_err());
        assert!(provenance::digest(&format!("unknown = \"value\"\n{source}")).is_err());
        assert_ne!(
            provenance::digest(&source.replace(
                "a4d7deebec3e2ce2c1daa455de6d79857839aed0",
                "b4d7deebec3e2ce2c1daa455de6d79857839aed0"
            ))
            .unwrap(),
            expected
        );
    }
}
