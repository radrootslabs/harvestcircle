use radroots_service_sqlite::{MigrationChecksum, SchemaObject, SchemaObjectKind};

pub(crate) const MIGRATION_SQL: &str = include_str!("schema-v7.sql");
// Frozen after the forward migration and complete schema inventory are reviewed.
pub(crate) const MIGRATION_CHECKSUM: MigrationChecksum = MigrationChecksum::from_bytes([
    110, 238, 204, 232, 30, 225, 102, 194, 61, 159, 210, 204, 168, 86, 119, 220, 84, 52, 220, 84,
    233, 103, 57, 86, 41, 52, 195, 147, 128, 233, 231, 95,
]);
pub(crate) const SCHEMA_DIGEST: [u8; 32] = [
    212, 135, 51, 189, 109, 152, 250, 114, 102, 146, 123, 37, 179, 129, 108, 130, 153, 71, 99, 108,
    244, 28, 203, 16, 138, 116, 80, 200, 46, 190, 98, 171,
];

pub(crate) fn schema_objects()
-> Result<Vec<SchemaObject>, radroots_service_sqlite::SchemaCatalogContractError> {
    let mut statements = MIGRATION_SQL.split(";\n");
    let mut objects = Vec::new();
    for name in [
        "hub_group_identification",
        "hub_work_requirements",
        "hub_group_work",
        "hub_movement_details",
        "hub_work_changes",
        "hub_work_actions",
        "hub_action_groups",
        "hub_discrepancies",
    ] {
        let sql = statements.next().expect("handling table inventory");
        let kind = SchemaObjectKind::Table;
        let digest = SchemaObject::computed_digest(kind, name, name, sql)?;
        objects.push(SchemaObject::new(kind, name, name, sql, digest)?);
    }
    for (name, table) in [
        ("hub_group_physical_label", "hub_group_identification"),
        ("hub_group_single_active_position", "hub_stock_positions"),
        ("hub_work_trace", "hub_work_requirements"),
        ("hub_group_work_requirement", "hub_group_work"),
        ("hub_stock_event_operation", "hub_stock_events"),
        ("hub_stock_event_group_sequence", "hub_stock_events"),
    ] {
        let sql = statements.next().expect("handling index inventory");
        let kind = SchemaObjectKind::Index;
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    for (name, table) in [
        (
            "hub_group_identification_no_update",
            "hub_group_identification",
        ),
        (
            "hub_group_identification_no_delete",
            "hub_group_identification",
        ),
        (
            "hub_work_requirement_identity_immutable",
            "hub_work_requirements",
        ),
        ("hub_work_requirements_no_delete", "hub_work_requirements"),
        ("hub_movement_details_no_update", "hub_movement_details"),
        ("hub_movement_details_no_delete", "hub_movement_details"),
        ("hub_work_changes_no_update", "hub_work_changes"),
        ("hub_work_changes_no_delete", "hub_work_changes"),
        ("hub_work_actions_no_update", "hub_work_actions"),
        ("hub_work_actions_no_delete", "hub_work_actions"),
        ("hub_action_groups_no_update", "hub_action_groups"),
        ("hub_action_groups_no_delete", "hub_action_groups"),
        ("hub_discrepancy_identity_immutable", "hub_discrepancies"),
        ("hub_discrepancies_no_delete", "hub_discrepancies"),
        ("hub_group_identity_immutable", "hub_stock_groups"),
        ("hub_work_progress_monotonic", "hub_work_requirements"),
        ("hub_group_work_identity_immutable", "hub_group_work"),
        ("hub_group_work_no_delete", "hub_group_work"),
    ] {
        let sql = statements.next().expect("handling trigger inventory");
        let kind = SchemaObjectKind::Trigger;
        let digest = SchemaObject::computed_digest(kind, name, table, sql)?;
        objects.push(SchemaObject::new(kind, name, table, sql, digest)?);
    }
    Ok(objects)
}
