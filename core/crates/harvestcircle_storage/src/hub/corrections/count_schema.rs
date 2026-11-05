//! Forward-only count interpretation metadata. Published predecessors remain
//! separate catalog versions with their original SQL and checksums.
use radroots_service_sqlite::{
    MigrationChecksum, SchemaCatalogContractError, SchemaObject, SchemaObjectKind,
};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v21.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    29, 20, 89, 194, 124, 58, 245, 235, 41, 222, 78, 138, 119, 218, 197, 90, 34, 255, 8, 152, 106,
    114, 216, 25, 169, 15, 154, 126, 237, 45, 128, 90,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    120, 58, 111, 217, 113, 180, 209, 166, 136, 131, 65, 4, 205, 19, 200, 19, 238, 93, 220, 253,
    47, 249, 131, 62, 4, 54, 10, 250, 195, 102, 0, 231,
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
            "hub_count_interpretation_epochs",
            "hub_count_interpretation_epochs",
        ),
        (
            SchemaObjectKind::Table,
            "hub_count_interpretation_markers",
            "hub_count_interpretation_markers",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_interpretation_epoch_guard",
            "hub_count_interpretation_epochs",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_interpretation_initialize",
            "hub_stock_state",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_interpretation_epochs_no_update",
            "hub_count_interpretation_epochs",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_interpretation_epochs_no_delete",
            "hub_count_interpretation_epochs",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_interpretation_marker_guard",
            "hub_count_interpretation_markers",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_interpretation_markers_no_update",
            "hub_count_interpretation_markers",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_interpretation_markers_no_delete",
            "hub_count_interpretation_markers",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_correction_mode_guard",
            "hub_corrections",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_review_guard",
            "hub_actual_reviews",
        ),
    ] {
        let sql = statements.next().expect("complete count schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    assert!(
        statements.next().is_none(),
        "complete count schema inventory"
    );
    Ok(objects)
}
