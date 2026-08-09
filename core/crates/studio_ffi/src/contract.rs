pub const PRODUCT_VERSION: &str = "0.1.0-alpha";
pub const FFI_CONTRACT_MAJOR: u16 = 3;
pub const FFI_CONTRACT_MINOR: u16 = 0;
pub const MINIMUM_SCHEMA_VERSION: u32 = 5;
pub const FFI_CONTRACT_HASH: &str = env!("RADROOTS_STUDIO_FFI_CONTRACT_DIGEST");

#[cfg(test)]
pub(crate) const NORMALIZED_CONTRACT_METADATA: &str =
    include_str!(concat!(env!("OUT_DIR"), "/ffi_contract_metadata.txt"));
