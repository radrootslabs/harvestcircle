#![doc = "Radroots Studio application runtime."]

#[cfg(test)]
mod tests {
    #[test]
    fn crate_is_available_to_the_workspace() {
        assert_eq!(env!("CARGO_PKG_NAME"), "radroots-studio-application");
    }
}
