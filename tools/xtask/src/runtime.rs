use super::{Inventory, cargo_dependencies, read_text, read_toml};
use std::path::Path;

pub(super) fn native_runtime_boundary(root: &Path, findings: &mut Vec<String>) {
    if root
        .join("core/crates/harvestcircle_domain/src/relay.rs")
        .exists()
    {
        findings
            .push("harvestcircle_domain: duplicate relay policy surface is forbidden".to_owned());
    }
    let inventory = match Inventory::load(root) {
        Ok(inventory) => inventory,
        Err(error) => {
            findings.push(error);
            return;
        }
    };
    for path in inventory
        .paths
        .iter()
        .filter(|p| p.starts_with("core/crates/") && p.ends_with("/Cargo.toml"))
    {
        let Some(manifest) = read_toml(root, path, findings) else {
            continue;
        };
        let owner = manifest
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        for (name, dependency) in cargo_dependencies(&manifest, false) {
            let package = dependency
                .get("package")
                .and_then(toml::Value::as_str)
                .unwrap_or(&name);
            if (owner == "harvestcircle_nostr" && package == "nostr-sdk")
                || (owner != "harvestcircle_test_bridge" && package == "harvestcircle_test_bridge")
            {
                findings.push(format!("{path}: forbidden production dependency {package}"));
            }
        }
    }
    let mut guarded = vec![
        "core/crates/harvestcircle_ffi/src/commands.rs",
        "core/crates/harvestcircle_ffi/src/observer.rs",
        "core/crates/harvestcircle_application/src/app_core.rs",
        "core/crates/harvestcircle_application/src/custody.rs",
        "core/crates/harvestcircle_application/src/secrets.rs",
        "core/crates/harvestcircle_storage/src/os_keyring.rs",
        "core/crates/harvestcircle_storage/src/lib.rs",
    ];
    guarded.extend(
        inventory
            .paths
            .iter()
            .filter(|path| {
                path.starts_with("core/crates/harvestcircle_application/src/")
                    && path.ends_with(".rs")
            })
            .map(String::as_str),
    );
    guarded.sort();
    guarded.dedup();
    for path in guarded {
        let source = read_text(root, path, findings);
        audit_rust_runtime(path, &source, findings);
    }
}

pub(super) fn audit_rust_runtime(path: &str, source: &str, findings: &mut Vec<String>) {
    use syn::visit::Visit;
    let file = match syn::parse_file(source) {
        Ok(file) => file,
        Err(error) => {
            findings.push(format!("{path}: invalid Rust source: {error}"));
            return;
        }
    };
    struct RuntimeVisitor<'a> {
        path: &'a str,
        findings: &'a mut Vec<String>,
        readonly: bool,
    }
    impl<'ast> Visit<'ast> for RuntimeVisitor<'_> {
        fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
            if item.attrs.iter().any(|a| matches!(&a.meta, syn::Meta::List(list) if list.path.is_ident("cfg") && list.tokens.to_string() == "test")) {
                return;
            }
            if self.path.ends_with("/harvestcircle_storage/src/lib.rs")
                && matches!(item.vis, syn::Visibility::Public(_))
            {
                self.findings.push(format!(
                    "{}: storage implementation modules must remain private",
                    self.path
                ));
            }
            syn::visit::visit_item_mod(self, item);
        }
        fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
            fn environment(tree: &syn::UseTree, prefix: &mut Vec<String>) -> bool {
                match tree {
                    syn::UseTree::Path(path) => {
                        prefix.push(path.ident.to_string());
                        let found = prefix.windows(2).any(|p| p == ["std", "env"])
                            || environment(&path.tree, prefix);
                        prefix.pop();
                        found
                    }
                    syn::UseTree::Group(group) => {
                        group.items.iter().any(|tree| environment(tree, prefix))
                    }
                    syn::UseTree::Name(name) => {
                        prefix.last().is_some_and(|p| p == "std") && name.ident == "env"
                    }
                    syn::UseTree::Rename(name) => {
                        prefix.last().is_some_and(|p| p == "std") && name.ident == "env"
                    }
                    _ => false,
                }
            }
            if self.path.contains("/harvestcircle_application/src/")
                && environment(&item.tree, &mut Vec::new())
            {
                self.findings.push(format!(
                    "{}: application core imports the process environment",
                    self.path
                ));
            }
            syn::visit::visit_item_use(self, item);
        }
        fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
            let previous = self.readonly;
            self.readonly = self.path.ends_with("/os_keyring.rs") && item.sig.ident == "verify";
            syn::visit::visit_impl_item_fn(self, item);
            self.readonly = previous;
        }
        fn visit_path(&mut self, path: &'ast syn::Path) {
            let names = path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>();
            if self.path.contains("/harvestcircle_application/src/")
                && names.windows(2).any(|p| p == ["std", "env"])
            {
                self.findings.push(format!(
                    "{}: application core reads the process environment",
                    self.path
                ));
            }
            if names.windows(2).any(|p| p == ["PoisonError", "into_inner"])
                || (self.path.ends_with("/commands.rs") && names.iter().any(|s| s == "OnceLock"))
            {
                self.findings.push(format!(
                    "{}: forbidden runtime boundary {}",
                    self.path,
                    names.join("::")
                ));
            }
            if self.readonly
                && names.last().is_some_and(|s| {
                    matches!(
                        s.as_str(),
                        "platform_create"
                            | "platform_delete"
                            | "set_password"
                            | "delete_credential"
                    )
                })
            {
                self.findings.push(format!(
                    "{}: read-only verification mutates custody",
                    self.path
                ));
            }
            syn::visit::visit_path(self, path);
        }
        fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
            if self.path.ends_with("/os_keyring.rs") {
                let method = call.method.to_string();
                if matches!(method.as_str(), "set_password" | "set_generic_password")
                    || (method == "create_item"
                        && !matches!(call.args.iter().nth(3),
                        Some(syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Bool(value), .. })) if !value.value))
                {
                    self.findings.push(format!(
                        "{}: credential creation must never replace an existing item",
                        self.path
                    ));
                }
            }
            if self.readonly
                && matches!(
                    call.method.to_string().as_str(),
                    "put" | "delete" | "set_password" | "delete_credential"
                )
            {
                self.findings.push(format!(
                    "{}: read-only verification mutates custody",
                    self.path
                ));
            }
            syn::visit::visit_expr_method_call(self, call);
        }
    }
    RuntimeVisitor {
        path,
        findings,
        readonly: false,
    }
    .visit_file(&file);
}
