use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v8.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    71, 175, 118, 232, 100, 79, 23, 162, 96, 13, 54, 138, 232, 253, 32, 95, 3, 148, 176, 247, 252,
    25, 227, 157, 25, 194, 49, 142, 231, 248, 62, 0,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    221, 74, 153, 114, 250, 92, 60, 186, 80, 33, 241, 81, 40, 110, 46, 85, 100, 26, 74, 162, 45,
    236, 249, 23, 23, 80, 178, 104, 10, 236, 219, 90,
];

pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL.split(";\n");
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (
            SchemaObjectKind::Trigger,
            "hub_group_birth_guard",
            "hub_group_identification",
        ),
        (
            SchemaObjectKind::Index,
            "hub_stock_groups_trace",
            "hub_stock_groups",
        ),
    ] {
        let sql = statements.next().expect("portion schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    Ok(objects)
}
