use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v6.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    131, 6, 139, 13, 213, 23, 209, 224, 113, 217, 237, 62, 140, 55, 243, 87, 114, 5, 100, 222, 62,
    164, 64, 1, 227, 192, 29, 251, 63, 249, 182, 26,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    199, 218, 148, 223, 236, 68, 83, 117, 82, 224, 15, 117, 107, 204, 88, 155, 198, 222, 32, 102,
    65, 143, 101, 116, 197, 9, 113, 62, 193, 36, 211, 210,
];

pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL.split(";\n");
    let mut objects = Vec::new();
    for name in [
        "hub_stock_state",
        "hub_stock_operations",
        "hub_source_references",
        "hub_receipts",
        "hub_stock_groups",
        "hub_stock_positions",
        "hub_putaway_work",
        "hub_stock_events",
    ] {
        let sql = statements.next().expect("stock migration table inventory");
        let kind = SchemaObjectKind::Table;
        let digest = SchemaObject::computed_digest(kind, name, name, sql)?;
        objects.push(SchemaObject::new(kind, name, name, sql, digest)?);
    }
    for (name, table) in [
        ("hub_stock_operations_no_update", "hub_stock_operations"),
        ("hub_stock_operations_no_delete", "hub_stock_operations"),
        ("hub_source_references_no_update", "hub_source_references"),
        ("hub_source_references_no_delete", "hub_source_references"),
        ("hub_receipts_no_update", "hub_receipts"),
        ("hub_receipts_no_delete", "hub_receipts"),
        ("hub_stock_events_no_update", "hub_stock_events"),
        ("hub_stock_events_no_delete", "hub_stock_events"),
    ] {
        let sql = statements
            .next()
            .expect("stock migration trigger inventory");
        let kind = SchemaObjectKind::Trigger;
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    Ok(objects)
}
