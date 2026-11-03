//! Forward actual-report schema; predecessor definitions remain immutable.
use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    133, 110, 253, 153, 65, 214, 232, 35, 206, 235, 77, 176, 233, 167, 190, 190, 130, 1, 193, 254,
    144, 159, 175, 192, 9, 223, 134, 218, 32, 176, 205, 12,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    44, 161, 19, 123, 60, 52, 96, 195, 6, 102, 128, 48, 89, 32, 88, 234, 119, 218, 25, 139, 22,
    238, 16, 19, 240, 144, 169, 23, 59, 61, 211, 52,
];
pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v18.sql");
pub(crate) const REPLACED_OBJECTS: &[&str] = &[
    "hub_actual_report_review_insert_guard",
    "hub_actual_report_review_update_guard",
];
pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL
        .split(";\n")
        .filter(|sql| !sql.trim().is_empty() && !sql.trim().starts_with("DROP TRIGGER "));
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (
            SchemaObjectKind::Table,
            "hub_actual_reports",
            "hub_actual_reports",
        ),
        (
            SchemaObjectKind::Table,
            "hub_actual_report_sources",
            "hub_actual_report_sources",
        ),
        (
            SchemaObjectKind::Index,
            "hub_actual_source_claims",
            "hub_actual_report_sources",
        ),
        (
            SchemaObjectKind::Table,
            "hub_actual_reviews",
            "hub_actual_reviews",
        ),
        (
            SchemaObjectKind::Index,
            "hub_actual_review_trace",
            "hub_actual_reviews",
        ),
        (
            SchemaObjectKind::Table,
            "hub_actual_review_lineage",
            "hub_actual_review_lineage",
        ),
        (
            SchemaObjectKind::Table,
            "hub_actual_review_work_scope",
            "hub_actual_review_work_scope",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_report_guard",
            "hub_actual_reports",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_source_guard",
            "hub_actual_report_sources",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_review_guard",
            "hub_actual_reviews",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_lineage_guard",
            "hub_actual_review_lineage",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_work_guard",
            "hub_actual_review_work_scope",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_reports_no_update",
            "hub_actual_reports",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_reports_no_delete",
            "hub_actual_reports",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_report_sources_no_update",
            "hub_actual_report_sources",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_report_sources_no_delete",
            "hub_actual_report_sources",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_reviews_no_update",
            "hub_actual_reviews",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_reviews_no_delete",
            "hub_actual_reviews",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_review_lineage_no_update",
            "hub_actual_review_lineage",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_review_lineage_no_delete",
            "hub_actual_review_lineage",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_review_work_scope_no_update",
            "hub_actual_review_work_scope",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_review_work_scope_no_delete",
            "hub_actual_review_work_scope",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_report_review_insert_guard",
            "hub_actual_report_reviews",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_actual_report_review_update_guard",
            "hub_actual_report_reviews",
        ),
    ] {
        let sql = statements.next().expect("actual-report schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    assert!(
        statements.next().is_none(),
        "complete actual-report schema inventory"
    );
    Ok(objects)
}
