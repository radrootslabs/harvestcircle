//! Additive association metadata, qualified before producer registration.
use radroots_service_sqlite::{SchemaCatalogContractError, SchemaObject, SchemaObjectKind};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v22.sql");
pub(crate) const MIGRATION_CHECKSUM: radroots_service_sqlite::MigrationChecksum =
    radroots_service_sqlite::MigrationChecksum::from_bytes([
        92, 226, 161, 130, 217, 41, 231, 54, 241, 82, 164, 185, 108, 59, 58, 45, 1, 238, 160, 32,
        140, 129, 239, 71, 26, 253, 207, 69, 253, 149, 17, 158,
    ]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    71, 156, 29, 243, 80, 237, 131, 17, 226, 185, 205, 197, 94, 29, 239, 144, 247, 8, 113, 229,
    122, 60, 39, 228, 174, 175, 242, 92, 42, 141, 196, 236,
];

pub(crate) fn schema_objects() -> Result<Vec<SchemaObject>, SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL
        .split(";\n")
        .filter(|sql| !sql.trim().is_empty() && !sql.trim().starts_with("INSERT INTO "));
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (
            SchemaObjectKind::Table,
            "hub_count_return_epochs",
            "hub_count_return_epochs",
        ),
        (
            SchemaObjectKind::Table,
            "hub_count_return_associations",
            "hub_count_return_associations",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_return_epoch_guard",
            "hub_count_return_epochs",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_return_initialize",
            "hub_count_interpretation_epochs",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_return_epochs_no_update",
            "hub_count_return_epochs",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_return_epochs_no_delete",
            "hub_count_return_epochs",
        ),
        (
            SchemaObjectKind::Table,
            "hub_count_return_intake_modes",
            "hub_count_return_intake_modes",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_return_intake_mode_guard",
            "hub_count_return_intake_modes",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_return_intake_modes_no_update",
            "hub_count_return_intake_modes",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_return_intake_modes_no_delete",
            "hub_count_return_intake_modes",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_return_intake_body_guard",
            "hub_returns",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_return_association_guard",
            "hub_count_return_associations",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_return_associations_no_update",
            "hub_count_return_associations",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_return_associations_no_delete",
            "hub_count_return_associations",
        ),
    ] {
        let sql = statements
            .next()
            .expect("complete count return schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    assert!(
        statements.next().is_none(),
        "complete count return schema inventory"
    );
    Ok(objects)
}

#[cfg(test)]
mod tests;
