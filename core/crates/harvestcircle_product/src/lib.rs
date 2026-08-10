//! Validated, generated HarvestCircle product coordinates.

include!(concat!(env!("OUT_DIR"), "/product_coordinates.rs"));

#[cfg(test)]
mod parser;

#[cfg(test)]
mod tests {
    use super::parser::{REQUIRED, parse};
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
        let source = REQUIRED
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(parse(&source).is_ok());
        assert!(parse(&source.replacen("schema=harvestcircle.product.v1\n", "", 1)).is_err());
        assert!(parse(&format!("{source}\nschema=harvestcircle.product.v1")).is_err());
        assert!(parse(&format!("{source}\nunknown=value")).is_err());
        assert!(
            parse(&source.replace("product.slug=harvestcircle", "product.slug=other")).is_err()
        );
    }
}
