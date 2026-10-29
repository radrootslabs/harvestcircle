//! Forward storage for original count results and removed work, not posting permission.
use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v15.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    36, 8, 132, 100, 77, 176, 124, 108, 50, 200, 228, 177, 93, 16, 178, 7, 225, 95, 65, 22, 100,
    184, 155, 15, 200, 49, 190, 21, 166, 171, 250, 221,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    134, 226, 144, 58, 130, 109, 239, 160, 242, 102, 183, 247, 198, 220, 9, 95, 123, 73, 125, 237,
    90, 53, 232, 246, 42, 10, 152, 45, 152, 176, 177, 38,
];

pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL
        .split(";\n")
        .filter(|sql| !sql.trim().is_empty() && sql.trim() != "DROP TRIGGER hub_group_birth_guard");
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (
            SchemaObjectKind::Index,
            "hub_count_observation_population",
            "hub_count_observations",
        ),
        (
            SchemaObjectKind::Table,
            "hub_count_reconciliations",
            "hub_count_reconciliations",
        ),
        (
            SchemaObjectKind::Table,
            "hub_count_reconciliation_work",
            "hub_count_reconciliation_work",
        ),
        (
            SchemaObjectKind::Index,
            "hub_count_reconciliation_work_requirement",
            "hub_count_reconciliation_work",
        ),
        (
            SchemaObjectKind::Table,
            "hub_counted_surplus",
            "hub_counted_surplus",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_reconciliation_guard",
            "hub_count_reconciliations",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_reconciliation_work_guard",
            "hub_count_reconciliation_work",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_reconciliation_no_update",
            "hub_count_reconciliations",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_reconciliation_no_delete",
            "hub_count_reconciliations",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_reconciliation_work_no_update",
            "hub_count_reconciliation_work",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_reconciliation_work_no_delete",
            "hub_count_reconciliation_work",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_counted_surplus_no_update",
            "hub_counted_surplus",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_counted_surplus_no_delete",
            "hub_counted_surplus",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_group_birth_guard",
            "hub_group_identification",
        ),
    ] {
        let sql = statements
            .next()
            .expect("count reconciliation schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    assert!(
        statements.next().is_none(),
        "complete count reconciliation schema inventory"
    );
    Ok(objects)
}
