//! Additive native interpretation epoch; published schemas remain frozen.
use radroots_service_sqlite::{
    MigrationChecksum, SchemaCatalogContractError, SchemaObject, SchemaObjectKind,
};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v20.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    19, 223, 70, 16, 171, 140, 77, 109, 156, 231, 171, 57, 62, 111, 111, 61, 118, 249, 170, 111,
    247, 187, 60, 176, 80, 172, 67, 192, 15, 124, 157, 30,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    220, 209, 173, 251, 195, 226, 72, 128, 94, 181, 245, 170, 1, 92, 185, 135, 156, 235, 77, 203,
    7, 196, 163, 147, 115, 119, 128, 47, 222, 124, 81, 77,
];
pub(crate) const REPLACED_OBJECTS: &[&str] = &["hub_actual_review_guard"];

pub(crate) fn schema_objects() -> Result<Vec<SchemaObject>, SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL.split(";\n").filter(|sql| {
        !sql.trim().is_empty()
            && !sql.trim().starts_with("DROP TRIGGER ")
            && !sql.trim().starts_with("INSERT INTO ")
    });
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (
            SchemaObjectKind::Table,
            "hub_actual_interpretation_epochs",
            "hub_actual_interpretation_epochs",
        ),
        (
            SchemaObjectKind::Table,
            "hub_actual_interpretation_markers",
            "hub_actual_interpretation_markers",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_interpretation_epoch_guard",
            "hub_actual_interpretation_epochs",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_interpretation_initialize",
            "hub_stock_state",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_interpretation_epochs_no_update",
            "hub_actual_interpretation_epochs",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_interpretation_epochs_no_delete",
            "hub_actual_interpretation_epochs",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_interpretation_marker_guard",
            "hub_actual_interpretation_markers",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_interpretation_markers_no_update",
            "hub_actual_interpretation_markers",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_interpretation_markers_no_delete",
            "hub_actual_interpretation_markers",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_review_guard",
            "hub_actual_reviews",
        ),
    ] {
        let sql = statements.next().expect("interpretation schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    assert!(
        statements.next().is_none(),
        "complete interpretation schema inventory"
    );
    Ok(objects)
}
