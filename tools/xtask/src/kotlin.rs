use super::{
    Inventory, contains_direct_call, is_production_compose, is_production_kotlin, is_text,
    read_text,
};
use std::path::Path;

pub(super) fn namespace_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    let temporary_namespace = ["org", "radroots", "harvestcircle"].join(".");
    for path in &inventory.paths {
        let normalized = path.to_ascii_lowercase();
        if normalized.starts_with("app/")
            && normalized.contains("/kotlin/")
            && normalized.ends_with(".kt")
        {
            let package_path = normalized
                .split_once("/kotlin/")
                .map(|(_, value)| value)
                .unwrap_or_default();
            if !package_path.starts_with("org/harvestcircle/") {
                findings.push(format!(
                    "{path}: Kotlin source is outside the final namespace"
                ));
            }
        }
        // Namespace rules inspect source/configuration, not public prose.
        // Global source safety still checks documentation on every command.
        if !is_text(path) || path.starts_with("docs/") || path.ends_with(".md") {
            continue;
        }
        let source = read_text(root, path, findings);
        if source.contains(&temporary_namespace)
            || source.contains(&temporary_namespace.replace('.', "/"))
        {
            findings.push(format!("{path}: temporary product namespace"));
        }
        let production_kotlin = path.ends_with(".kt")
            && path.starts_with("app/")
            && ["/src/main/", "/src/commonMain/", "/src/desktopMain/"]
                .iter()
                .any(|segment| path.contains(segment));
        let desktop_entrypoint =
            path == "app/desktop/src/main/kotlin/org/harvestcircle/desktop/Main.kt";
        if production_kotlin
            && source.contains(&["run", "Blocking"].concat())
            && !desktop_entrypoint
        {
            findings.push(format!(
                "{path}: blocking coroutine bridge in application source"
            ));
        }
        if production_kotlin
            && (source.contains(&["Atomic", "Long"].concat())
                || source.contains(&["desktop", "-operation:"].concat()))
        {
            findings.push(format!("{path}: process-local operation counter"));
        }
        if path.starts_with("app/shared/src/commonMain/")
            && [
                ["org.harvestcircle.", "ffi"].concat(),
                ["com.sun.", "jna"].concat(),
                "java.".to_owned(),
                "javax.".to_owned(),
            ]
            .iter()
            .any(|marker| source.contains(marker))
        {
            findings.push(format!(
                "{path}: platform dependency in shared common source"
            ));
        }
        let lowercase = source.to_ascii_lowercase();
        let secret_marker = ["nsec", "1"].concat();
        let inspected_secret = lowercase.replace(&format!("{secret_marker}…"), "");
        if production_kotlin && inspected_secret.contains(&secret_marker) {
            findings.push(format!("{path}: secret key literal in production Kotlin"));
        }
    }
}

pub(super) fn product_shell_audit(root: &Path, inventory: &Inventory, findings: &mut Vec<String>) {
    for path in &inventory.paths {
        if !is_production_kotlin(path) {
            continue;
        }
        let source = read_text(root, path, findings);
        let compact = source
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();
        if source
            .lines()
            .map(str::trim)
            .any(|line| line.starts_with("import androidx.compose.material"))
        {
            findings.push(format!(
                "{path}: Material component dependency is forbidden"
            ));
        }
        let approved_color_adapter = path.starts_with(
            "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/theme/color/",
        ) || path
            == "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/shell/HarvestCircleShellVisuals.kt";
        if is_production_compose(path, &source)
            && !approved_color_adapter
            && contains_direct_call(&compact, "Color(")
        {
            findings.push(format!(
                "{path}: hard-coded Compose color outside the theme adapter"
            ));
        }
        let approved_text_primitive = matches!(
            path.as_str(),
            "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/primitive/HarvestCircleText.kt"
                | "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/shell/HarvestCircleShellText.kt"
        );
        let approved_input_primitive = matches!(
            path.as_str(),
            "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/component/input/HarvestCircleTextField.kt"
                | "app/design_system/src/commonMain/kotlin/org/harvestcircle/designsystem/shell/HarvestCircleShellControls.kt"
        );
        if is_production_compose(path, &source) {
            if !approved_text_primitive && contains_direct_call(&compact, "BasicText(") {
                findings.push(format!(
                    "{path}: BasicText bypasses the shell primitive adapter"
                ));
            }
            if !approved_input_primitive && contains_direct_call(&compact, "BasicTextField(") {
                findings.push(format!(
                    "{path}: BasicTextField bypasses the shell primitive adapter"
                ));
            }
        }
    }
}
