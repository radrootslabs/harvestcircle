use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v9.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    6, 181, 44, 113, 158, 208, 197, 254, 252, 210, 187, 30, 153, 36, 194, 82, 104, 82, 15, 152,
    253, 187, 47, 96, 10, 141, 26, 162, 197, 11, 121, 142,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    38, 81, 76, 25, 166, 0, 136, 126, 236, 180, 130, 247, 217, 221, 237, 164, 208, 20, 216, 148,
    183, 230, 159, 156, 76, 61, 56, 146, 215, 182, 243, 131,
];

pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL.split(";\n");
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (
            SchemaObjectKind::Table,
            "hub_treatment_records",
            "hub_treatment_records",
        ),
        (
            SchemaObjectKind::Table,
            "hub_treatment_scope_anchors",
            "hub_treatment_scope_anchors",
        ),
        (
            SchemaObjectKind::Index,
            "hub_treatment_trace",
            "hub_treatment_records",
        ),
        (
            SchemaObjectKind::Index,
            "hub_treatment_one_release",
            "hub_treatment_records",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_treatment_record_guard",
            "hub_treatment_records",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_treatment_scope_guard",
            "hub_treatment_scope_anchors",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_treatment_records_no_update",
            "hub_treatment_records",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_treatment_records_no_delete",
            "hub_treatment_records",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_treatment_scope_no_update",
            "hub_treatment_scope_anchors",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_treatment_scope_no_delete",
            "hub_treatment_scope_anchors",
        ),
    ] {
        let sql = statements.next().expect("treatment schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    Ok(objects)
}
