DROP TRIGGER hub_group_birth_guard;
CREATE TRIGGER hub_group_birth_guard BEFORE INSERT ON hub_group_identification
WHEN NOT EXISTS (
    SELECT 1 FROM hub_stock_groups g
    JOIN hub_stock_operations o ON o.operation_id = NEW.created_operation_id
    JOIN hub_receipts r ON r.trace_id = g.trace_id
    JOIN hub_stock_operations original ON original.operation_id = r.operation_id
    WHERE g.group_id = NEW.group_id AND o.hub_id = original.hub_id AND (
        (NEW.parent_group_id IS NULL AND r.root_group_id = g.group_id AND r.operation_id = o.operation_id AND o.result_id = r.trace_id AND (
            (o.kind = 'receipt' AND r.origin_kind = 'receipt' AND NEW.physical_label IS NULL)
            OR (o.kind = 'opening' AND r.origin_kind = 'opening' AND NEW.physical_label IS NOT NULL AND EXISTS (
                SELECT 1 FROM hub_counted_openings entry WHERE entry.population_id = r.trace_id
            ))
        ))
        OR
        (NEW.parent_group_id IS NOT NULL AND o.kind IN ('move','split') AND o.result_id = g.group_id AND EXISTS (
            SELECT 1 FROM hub_group_identification parent
            JOIN hub_stock_groups pg ON pg.group_id = parent.group_id
            JOIN hub_stock_operations po ON po.operation_id = parent.created_operation_id
            WHERE parent.group_id = NEW.parent_group_id AND pg.trace_id = g.trace_id AND po.hub_id = o.hub_id AND po.stock_revision < o.stock_revision
        ))
    )
)
BEGIN SELECT RAISE(ABORT, 'physical group birth must retain its original population'); END;
