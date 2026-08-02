#![doc = "Pinned `UniFFI` binding generator entry point."]

fn main() {
    uniffi::uniffi_bindgen_main();
}

#[cfg(test)]
mod tests {
    #[test]
    fn tool_is_available_to_the_workspace() {
        assert_eq!(env!("CARGO_PKG_NAME"), "radroots-studio-uniffi-bindgen");
    }
}
