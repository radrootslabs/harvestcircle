//! Forward native admission/session storage. Published predecessors remain frozen.
use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v16.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    127, 167, 46, 192, 116, 71, 39, 42, 96, 72, 121, 7, 34, 77, 99, 253, 1, 102, 193, 151, 217,
    176, 46, 108, 12, 235, 95, 251, 64, 39, 227, 64,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    151, 32, 34, 19, 134, 182, 120, 186, 145, 97, 29, 231, 38, 182, 13, 55, 206, 83, 83, 43, 81,
    177, 129, 87, 50, 26, 88, 220, 187, 239, 122, 56,
];

pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL.split(";\n").filter(|s| !s.trim().is_empty());
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (
            SchemaObjectKind::Table,
            "hub_runtime_session",
            "hub_runtime_session",
        ),
        (
            SchemaObjectKind::Table,
            "hub_submissions",
            "hub_submissions",
        ),
        (
            SchemaObjectKind::Index,
            "hub_submission_runtime",
            "hub_submissions",
        ),
        (
            SchemaObjectKind::Table,
            "hub_submission_rejections",
            "hub_submission_rejections",
        ),
        (
            SchemaObjectKind::Table,
            "hub_actual_report_reviews",
            "hub_actual_report_reviews",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_runtime_session_insert_guard",
            "hub_runtime_session",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_runtime_session_update_guard",
            "hub_runtime_session",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_runtime_session_no_delete",
            "hub_runtime_session",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_submission_admission_guard",
            "hub_submissions",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_submissions_no_update",
            "hub_submissions",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_submissions_no_delete",
            "hub_submissions",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_submission_rejection_guard",
            "hub_submission_rejections",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_submission_rejections_no_update",
            "hub_submission_rejections",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_submission_rejections_no_delete",
            "hub_submission_rejections",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_submission_stock_effect_guard",
            "hub_stock_operations",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_submission_reference_effect_guard",
            "hub_reference_versions",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_submission_initialization_effect_guard",
            "hub_history",
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
        (
            SchemaObjectKind::Trigger,
            "hub_actual_report_reviews_no_delete",
            "hub_actual_report_reviews",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_submission_uncertain_stock_guard",
            "hub_stock_operations",
        ),
    ] {
        let sql = statements
            .next()
            .expect("native admission schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    assert!(
        statements.next().is_none(),
        "complete native admission schema inventory"
    );
    Ok(objects)
}
