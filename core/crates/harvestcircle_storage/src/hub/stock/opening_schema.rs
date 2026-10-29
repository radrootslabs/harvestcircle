use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v13.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    240, 131, 122, 7, 57, 198, 228, 60, 143, 9, 173, 141, 67, 41, 207, 84, 114, 121, 30, 41, 154,
    163, 65, 249, 154, 243, 232, 200, 255, 139, 225, 189,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    209, 15, 98, 93, 172, 111, 247, 63, 44, 29, 26, 0, 22, 189, 134, 99, 73, 59, 131, 141, 2, 232,
    172, 98, 30, 5, 18, 67, 171, 144, 238, 196,
];

pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let sql = MIGRATION_SQL
        .strip_prefix("DROP TRIGGER hub_group_birth_guard;\n")
        .expect("opening migration replaces only the group birth guard")
        .trim_end_matches(['\n', ';']);
    let digest = SchemaObject::computed_digest(
        SchemaObjectKind::Trigger,
        "hub_group_birth_guard",
        "hub_group_identification",
        sql,
    )?;
    Ok(vec![SchemaObject::new(
        SchemaObjectKind::Trigger,
        "hub_group_birth_guard",
        "hub_group_identification",
        sql,
        digest,
    )?])
}
