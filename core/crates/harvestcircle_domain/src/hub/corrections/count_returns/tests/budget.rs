use super::*;

#[test]
fn association_reserves_one_row_in_the_complete_trace_budget_without_raising_the_limit() {
    let f = Fixture::new();
    let mut context = f.context();
    context.prior_trace_correction_rows = HUB_CORRECTION_ROWS - 1;
    let plan = f
        .command()
        .plan(&context, id(240), f.original.actor, at(200))
        .unwrap();
    assert_eq!(plan.prior_trace_correction_rows + 1, HUB_CORRECTION_ROWS);
    context.prior_trace_correction_rows = HUB_CORRECTION_ROWS;
    assert_eq!(
        f.command()
            .plan(&context, id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::Capacity)
    );
    context.prior_trace_correction_rows += 1;
    assert_eq!(
        f.command()
            .plan(&context, id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::InvalidHistory)
    );
}

#[test]
fn a_claimed_total_cannot_omit_visible_count_history_and_is_not_a_completeness_proof() {
    let f = Fixture::new();
    assert_eq!(f.context().prior_trace_correction_rows, 1);
    let mut context = f.context();
    context.prior_trace_correction_rows = 0;
    assert_eq!(
        f.command()
            .plan(&context, id(240), f.original.actor, at(200)),
        Err(CountReturnAssociationError::InvalidHistory)
    );
    // A larger descriptive scalar can represent other native families on this
    // trace. Only native reconstruction can prove their actual complete count.
    context.prior_trace_correction_rows = 3;
    assert_eq!(
        f.command()
            .plan(&context, id(240), f.original.actor, at(200))
            .unwrap()
            .prior_trace_correction_rows,
        3
    );
}
