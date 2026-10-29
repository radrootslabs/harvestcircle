use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};
pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v12.sql");
pub(crate) const OBJECTS_SQL: &str = include_str!("schema-v12-objects.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    95, 111, 118, 77, 122, 164, 216, 112, 231, 197, 159, 64, 237, 11, 224, 229, 110, 152, 190, 108,
    133, 13, 184, 170, 117, 181, 215, 65, 233, 215, 113, 51,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    82, 251, 41, 121, 18, 107, 117, 252, 236, 29, 157, 70, 81, 213, 220, 4, 247, 92, 142, 144, 114,
    213, 81, 151, 52, 30, 51, 186, 212, 64, 244, 194,
];
pub(crate) const REPLACED_OBJECTS: &[&str] = &["hub_receipts"];
pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = OBJECTS_SQL.split(";\n");
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (
            SchemaObjectKind::Trigger,
            "hub_counted_opening_no_delete",
            "hub_counted_openings",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_counted_opening_no_update",
            "hub_counted_openings",
        ),
        (
            SchemaObjectKind::Table,
            "hub_counted_openings",
            "hub_counted_openings",
        ),
        (
            SchemaObjectKind::Index,
            "hub_population_origin_identity",
            "hub_receipts",
        ),
        (SchemaObjectKind::Table, "hub_receipts", "hub_receipts"),
    ] {
        let sql = statements.next().expect("population schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    assert!(
        statements.all(|tail| tail.trim().is_empty()),
        "complete population schema inventory"
    );
    Ok(objects)
}
