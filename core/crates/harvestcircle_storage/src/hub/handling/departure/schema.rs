use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v11.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    60, 11, 6, 65, 133, 21, 24, 63, 79, 174, 193, 81, 225, 173, 135, 32, 192, 204, 240, 168, 104,
    158, 69, 34, 220, 199, 246, 204, 117, 76, 14, 152,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    27, 142, 242, 126, 132, 63, 95, 248, 90, 160, 144, 146, 58, 93, 171, 217, 241, 161, 138, 180,
    10, 232, 57, 125, 207, 180, 49, 130, 216, 173, 219, 183,
];
pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL.split(";\n");
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (SchemaObjectKind::Table, "hub_departures", "hub_departures"),
        (
            SchemaObjectKind::Table,
            "hub_departure_lines",
            "hub_departure_lines",
        ),
        (
            SchemaObjectKind::Table,
            "hub_departure_work_scope",
            "hub_departure_work_scope",
        ),
        (
            SchemaObjectKind::Index,
            "hub_departure_trace_lines",
            "hub_departure_lines",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_departure_guard",
            "hub_departures",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_departure_line_guard",
            "hub_departure_lines",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_departure_scope_guard",
            "hub_departure_work_scope",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_departure_no_update",
            "hub_departures",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_departure_no_delete",
            "hub_departures",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_departure_line_no_update",
            "hub_departure_lines",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_departure_line_no_delete",
            "hub_departure_lines",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_departure_scope_no_update",
            "hub_departure_work_scope",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_departure_scope_no_delete",
            "hub_departure_work_scope",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_used_guard",
            "hub_dispatch_changes",
        ),
    ] {
        let sql = statements.next().expect("departure schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    assert!(
        statements.all(|tail| tail.trim().is_empty()),
        "complete departure schema inventory"
    );
    Ok(objects)
}
