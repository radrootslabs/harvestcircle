use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v10.sql");
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    137, 55, 12, 191, 179, 51, 238, 108, 206, 238, 188, 70, 175, 3, 40, 138, 181, 183, 59, 56, 3,
    196, 229, 208, 184, 162, 245, 177, 112, 235, 105, 44,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    109, 39, 114, 216, 71, 136, 52, 57, 13, 129, 46, 222, 122, 122, 12, 109, 221, 227, 49, 227,
    246, 170, 155, 96, 109, 186, 205, 192, 212, 42, 246, 163,
];

pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL.split(";\n");
    let mut objects = Vec::new();
    for (kind, name, table) in [
        (
            SchemaObjectKind::Table,
            "hub_dispatch_materials",
            "hub_dispatch_materials",
        ),
        (
            SchemaObjectKind::Table,
            "hub_dispatch_material_lines",
            "hub_dispatch_material_lines",
        ),
        (
            SchemaObjectKind::Table,
            "hub_dispatch_identifications",
            "hub_dispatch_identifications",
        ),
        (
            SchemaObjectKind::Table,
            "hub_dispatch_changes",
            "hub_dispatch_changes",
        ),
        (
            SchemaObjectKind::Index,
            "hub_dispatch_material_trace",
            "hub_dispatch_material_lines",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_material_guard",
            "hub_dispatch_materials",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_line_guard",
            "hub_dispatch_material_lines",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_change_guard",
            "hub_dispatch_changes",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_identification_guard",
            "hub_dispatch_identifications",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_material_no_update",
            "hub_dispatch_materials",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_material_no_delete",
            "hub_dispatch_materials",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_line_no_update",
            "hub_dispatch_material_lines",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_line_no_delete",
            "hub_dispatch_material_lines",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_identification_no_update",
            "hub_dispatch_identifications",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_identification_no_delete",
            "hub_dispatch_identifications",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_change_no_update",
            "hub_dispatch_changes",
        ),
        (
            SchemaObjectKind::Trigger,
            "hub_dispatch_change_no_delete",
            "hub_dispatch_changes",
        ),
    ] {
        let sql = statements.next().expect("dispatch schema inventory");
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    Ok(objects)
}
