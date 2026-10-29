use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v14.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    18, 53, 74, 30, 143, 229, 29, 229, 38, 175, 14, 52, 119, 165, 77, 167, 145, 102, 49, 29, 197,
    216, 18, 255, 10, 169, 95, 109, 26, 183, 242, 96,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    73, 186, 159, 34, 103, 182, 116, 131, 219, 59, 27, 105, 11, 41, 127, 121, 92, 85, 113, 93, 147,
    36, 104, 163, 197, 0, 39, 91, 61, 51, 75, 35,
];

pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL.split(";\n");
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (
            SchemaObjectKind::Table,
            "hub_count_observations",
            "hub_count_observations",
        ),
        (
            SchemaObjectKind::Index,
            "hub_count_observation_group",
            "hub_count_observations",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_observation_no_update",
            "hub_count_observations",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_count_observation_no_delete",
            "hub_count_observations",
        ),
    ] {
        let sql = statements
            .next()
            .expect("count observation schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    assert!(
        statements.all(|tail| tail.trim().is_empty()),
        "complete count observation schema inventory"
    );
    Ok(objects)
}
