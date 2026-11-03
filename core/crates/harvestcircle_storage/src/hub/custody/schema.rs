//! Forward custody schema. Published predecessor definitions remain frozen.
use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};
pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v17.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    229, 34, 43, 181, 169, 196, 201, 201, 146, 160, 245, 251, 76, 80, 111, 139, 77, 142, 42, 255,
    93, 140, 178, 61, 241, 250, 28, 42, 244, 207, 1, 85,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    227, 181, 224, 223, 236, 16, 138, 6, 83, 207, 73, 71, 75, 158, 205, 59, 221, 63, 121, 234, 216,
    179, 14, 193, 74, 120, 150, 130, 200, 25, 41, 24,
];
pub(crate) const REPLACED_OBJECTS: &[&str] = &[
    "hub_group_birth_guard",
    "hub_departure_line_guard",
    "hub_submission_uncertain_stock_guard",
];
pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL
        .split(";\n")
        .filter(|sql| !sql.trim().is_empty() && !sql.trim().starts_with("DROP TRIGGER "));
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (SchemaObjectKind::Table, "hub_returns", "hub_returns"),
        (
            SchemaObjectKind::Index,
            "hub_return_departure_scope",
            "hub_returns",
        ),
        (
            SchemaObjectKind::Table,
            "hub_return_sources",
            "hub_return_sources",
        ),
        (
            SchemaObjectKind::Table,
            "hub_dispositions",
            "hub_dispositions",
        ),
        (
            SchemaObjectKind::Index,
            "hub_disposition_trace",
            "hub_dispositions",
        ),
        (
            SchemaObjectKind::Table,
            "hub_disposition_work_scope",
            "hub_disposition_work_scope",
        ),
        (SchemaObjectKind::Trigger, "hub_return_guard", "hub_returns"),
        (
            SchemaObjectKind::Trigger,
            "hub_return_source_guard",
            "hub_return_sources",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_disposition_guard",
            "hub_dispositions",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_disposition_scope_guard",
            "hub_disposition_work_scope",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_returns_no_update",
            "hub_returns",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_returns_no_delete",
            "hub_returns",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_return_sources_no_update",
            "hub_return_sources",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_return_sources_no_delete",
            "hub_return_sources",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispositions_no_update",
            "hub_dispositions",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispositions_no_delete",
            "hub_dispositions",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_disposition_scope_no_update",
            "hub_disposition_work_scope",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_disposition_scope_no_delete",
            "hub_disposition_work_scope",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_group_birth_guard",
            "hub_group_identification",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_departure_line_guard",
            "hub_departure_lines",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_submission_uncertain_stock_guard",
            "hub_stock_operations",
        ),
    ] {
        let sql = statements.next().expect("custody schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    assert!(
        statements.next().is_none(),
        "complete custody schema inventory"
    );
    Ok(objects)
}
