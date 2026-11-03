use super::*;

impl FoundStockCommand {
    pub fn plan(
        &self,
        context: &FoundStockContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<FoundStockPlan, PhysicalCorrectionError> {
        if context.uncertain_submissions {
            return Err(PhysicalCorrectionError::UncertainSubmission);
        }
        if self.expected_stock_revision != context.stock_revision
            || self.expected_catalog_revision != context.catalog.revision()
        {
            return Err(PhysicalCorrectionError::StaleRevision);
        }
        if self.allocations.is_empty()
            || self.allocations.len() > HUB_FOUND_ALLOCATIONS
            || context
                .lineage
                .nodes()
                .len()
                .checked_add(self.allocations.len())
                .is_none_or(|n| n > HUB_LINEAGE_GROUPS)
            || context.active_identifications.len() > crate::hub::movement::HUB_GROUP_CAPACITY
        {
            return Err(PhysicalCorrectionError::Capacity);
        }
        let target = context.target.original();
        if self.target_operation != target.operation
            || self.target_group != target.group
            || context.lineage.trace() != target.trace
            || context.lineage.variant() != target.variant
            || context.lineage.hub() != context.hub
            || context.lineage.revision() != context.stock_revision
        {
            return Err(PhysicalCorrectionError::InvalidScope);
        }
        if now < context.latest_recorded_at
            || operation == target.operation
            || context
                .target
                .amendments()
                .iter()
                .any(|a| a.operation == operation)
        {
            return Err(PhysicalCorrectionError::InvalidHistory);
        }
        let revision = context
            .stock_revision
            .next()
            .map_err(|_| PhysicalCorrectionError::InvalidHistory)?;
        context
            .catalog
            .require_kind(target.variant, ReferenceKind::Pack)
            .map_err(|_| PhysicalCorrectionError::InvalidScope)?;
        context
            .treatment
            .project(context.lineage, context.catalog.handling_basis())
            .map_err(|_| PhysicalCorrectionError::Treatment)?;
        if context
            .treatment
            .decisions()
            .iter()
            .any(|d| d.stamp.recorded_at > context.latest_recorded_at)
            || context
                .treatment
                .initial_reviews()
                .iter()
                .any(|r| r.stamp.recorded_at > context.latest_recorded_at)
            || context.treatment.holds().iter().any(|h| {
                h.stamp.recorded_at > context.latest_recorded_at
                    || h.release
                        .as_ref()
                        .is_some_and(|r| r.recorded_at > context.latest_recorded_at)
            })
        {
            return Err(PhysicalCorrectionError::InvalidHistory);
        }
        let funding = validate_funding(context, now)?;
        if context.funding.iter().any(|f| {
            f.original.key.operation == operation
                || f.credits.iter().any(|c| c.operation == operation)
                || f.returns.iter().any(|r| r.operation == operation)
        }) {
            return Err(PhysicalCorrectionError::Duplicate);
        }
        let mut labels = context
            .active_identifications
            .iter()
            .map(|i| i.label.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        if labels.len() != context.active_identifications.len() {
            return Err(PhysicalCorrectionError::IdentityConflict);
        }
        for source in context.funding {
            if let Some(identity) = &source.original.before.identification {
                labels.insert(identity.label.as_str().to_owned());
            }
        }
        let mut available = BTreeMap::new();
        let mut total = zero();
        let mut allocations = Vec::new();
        let mut nodes = context.lineage.nodes().to_vec();
        let mut children = Vec::new();
        for (index, input) in self.allocations.iter().enumerate() {
            if input.packs.value() == 0 {
                return Err(PhysicalCorrectionError::InvalidQuantity);
            }
            let source = *funding
                .get(&input.debit)
                .ok_or(PhysicalCorrectionError::InvalidScope)?;
            if source.original.key.operation == operation
                || source.returns.iter().any(|r| r.operation == operation)
            {
                return Err(PhysicalCorrectionError::Duplicate);
            }
            let current_available = available.entry(input.debit).or_insert(source.available()?);
            let before = *current_available;
            *current_available = current_available
                .checked_sub(input.packs)
                .map_err(|_| PhysicalCorrectionError::InvalidQuantity)?;
            if source
                .original
                .before
                .identification
                .as_ref()
                .is_some_and(|i| i.label == input.identification.label)
                || !labels.insert(input.identification.label.as_str().to_owned())
            {
                return Err(PhysicalCorrectionError::IdentityConflict);
            }
            let location = context
                .catalog
                .require_kind(input.location, ReferenceKind::Location)
                .cloned()
                .map_err(|_| PhysicalCorrectionError::InvalidScope)?;
            let allocation = u8::try_from(index).map_err(|_| PhysicalCorrectionError::Capacity)?;
            let id = found_stock_group_id(operation, allocation)?;
            if context.lineage.node(id).is_some() || children.contains(&id) {
                return Err(PhysicalCorrectionError::IdentityConflict);
            }
            let group = PhysicalStockGroup {
                id,
                trace: target.trace,
                variant: target.variant,
                parent: Some(input.debit.group),
                identification: Some(input.identification.clone()),
                location: location.id,
                packs: input.packs,
                revision: HubRevision::INITIAL,
                causal_floor: Some(now),
                work: Vec::new(),
            };
            group
                .validate()
                .map_err(|_| PhysicalCorrectionError::InvalidHistory)?;
            nodes.push(PhysicalLineageNode {
                id,
                parent: group.parent,
                trace: target.trace,
                variant: target.variant,
                created_revision: revision,
                packs: input.packs,
                recovery: Some(crate::hub::portions::PhysicalRecoveryOrigin {
                    debit_operation: source.original.key.operation,
                    debit_revision: source.original.stock_revision,
                }),
            });
            children.push(id);
            allocations.push(FoundStockAllocation {
                credit: FoundFundingCredit {
                    operation,
                    allocation,
                    debit: input.debit,
                    packs: input.packs,
                    stock_revision: revision,
                    recorded_at: now,
                },
                funding: source.original.clone(),
                available_before: before,
                group,
                cohort_evidence: input.cohort_evidence.clone(),
                location,
            });
            total = add(total, input.packs)?;
        }
        let after = context
            .target
            .effective()
            .checked_sub(total)
            .map_err(|_| PhysicalCorrectionError::InvalidQuantity)?;
        let amendment = DepartureQuantityAmendment {
            operation,
            stock_revision: revision,
            before: context.target.effective(),
            after,
            delta: DepartureQuantityDelta::IdentifiedFoundRemainder(total),
        };
        let mut history = context.target.amendments().to_vec();
        history.push(amendment);
        DepartureQuantityAccount::restore(target, history, context.target.established_returns())
            .map_err(quantity_error)?;
        let after_lineage =
            PhysicalLineage::new(context.hub, target.trace, target.variant, revision, nodes)
                .map_err(|_| PhysicalCorrectionError::InvalidHistory)?;
        let review_required = InitialGroupReview {
            stamp: TreatmentStamp {
                id: treatment_record_id(operation)
                    .map_err(|_| PhysicalCorrectionError::IdentityConflict)?,
                stock_revision: revision,
                actor,
                basis: context.catalog.handling_basis(),
                recorded_at: now,
                occurred: Occurrence::Known(now),
                observation: self.basis.clone(),
            },
            scope: after_lineage
                .capture(&children)
                .map_err(|_| PhysicalCorrectionError::InvalidHistory)?,
        };
        let mut initial_reviews = context.treatment.initial_reviews().to_vec();
        initial_reviews.push(review_required.clone());
        let after_treatment = TreatmentState::restore_with_initial_reviews(
            &after_lineage,
            context.treatment.decisions().to_vec(),
            context.treatment.holds().to_vec(),
            initial_reviews,
        )
        .map_err(|_| PhysicalCorrectionError::Treatment)?;
        after_treatment
            .project(&after_lineage, context.catalog.handling_basis())
            .map_err(|_| PhysicalCorrectionError::Treatment)?;
        Ok(FoundStockPlan {
            operation,
            hub: context.hub,
            actor,
            recorded_at: now,
            original_target: target,
            amendment,
            allocations,
            review_required,
            after_lineage,
            after_treatment,
            basis: self.basis.clone(),
        })
    }
}

fn validate_funding<'a>(
    context: &'a FoundStockContext<'_>,
    now: UnixTimestamp,
) -> Result<BTreeMap<PhysicalDebitKey, &'a PhysicalDebitFunding>, PhysicalCorrectionError> {
    if context.funding.len() > crate::hub::stock::HUB_STOCK_EVENT_CAPACITY {
        return Err(PhysicalCorrectionError::Capacity);
    }
    let mut rows = 0_usize;
    for funding in context.funding {
        rows = rows
            .checked_add(funding.credits.len())
            .and_then(|n| n.checked_add(funding.returns.len()))
            .ok_or(PhysicalCorrectionError::Capacity)?;
        if rows > crate::hub::stock::HUB_STOCK_EVENT_CAPACITY {
            return Err(PhysicalCorrectionError::Capacity);
        }
    }
    let target = context.target.original();
    let mut scopes = BTreeMap::new();
    let initial_reviews = context
        .treatment
        .initial_reviews()
        .iter()
        .map(|r| (r.stamp.id, r))
        .collect::<BTreeMap<_, _>>();
    let mut base = 0;
    let mut physical = zero();
    let mut returned = zero();
    let mut additional = BTreeMap::new();
    let mut recovered = BTreeMap::new();
    let mut credits = BTreeSet::new();
    let mut returns = BTreeSet::new();
    let mut revisions = BTreeMap::new();
    let mut operations = BTreeMap::new();
    let mut debit_operations = BTreeSet::new();
    let mut credit_operations = BTreeSet::new();
    let mut register = |operation,
                        revision: HubRevision,
                        at: UnixTimestamp|
     -> Result<(), PhysicalCorrectionError> {
        if revision.value() > context.stock_revision.value()
            || at > now
            || at > context.latest_recorded_at
            || revisions
                .insert(revision.value(), operation)
                .is_some_and(|prior| prior != operation)
            || operations
                .insert(operation, (revision, at))
                .is_some_and(|prior| prior != (revision, at))
        {
            return Err(PhysicalCorrectionError::InvalidHistory);
        }
        Ok(())
    };
    for funding in context.funding {
        let original = &funding.original;
        if original.target != target {
            return Err(PhysicalCorrectionError::InvalidScope);
        }
        if scopes.insert(original.key, funding).is_some() {
            return Err(PhysicalCorrectionError::Duplicate);
        }
        let parent = context
            .lineage
            .node(original.key.group)
            .ok_or(PhysicalCorrectionError::InvalidScope)?;
        if parent.parent != original.before.parent
            || parent.created_revision.value() >= original.stock_revision.value()
            || parent.packs.value()
                > original
                    .before
                    .packs
                    .checked_sub(original.packs)
                    .map_err(|_| PhysicalCorrectionError::InvalidHistory)?
                    .value()
        {
            return Err(PhysicalCorrectionError::InvalidHistory);
        }
        debit_operations.insert(original.key.operation);
        register(
            original.key.operation,
            original.stock_revision,
            original.recorded_at,
        )?;
        if original.kind == PhysicalDebitKind::AdditionalQuantity {
            let amount = additional.entry(original.key.operation).or_insert(zero());
            *amount = add(*amount, original.packs)?;
        } else {
            base += 1;
        }
        physical = add(
            physical,
            original
                .packs
                .checked_sub(funding.recovered)
                .map_err(|_| PhysicalCorrectionError::InvalidQuantity)?,
        )?;
        returned = add(returned, funding.returned)?;
        for credit in &funding.credits {
            credit_operations.insert(credit.operation);
            let child_id = found_stock_group_id(credit.operation, credit.allocation)?;
            let child = context
                .lineage
                .node(child_id)
                .ok_or(PhysicalCorrectionError::InvalidHistory)?;
            let single = context
                .lineage
                .restore_scope(
                    credit.stock_revision,
                    vec![crate::hub::portions::PhysicalScopeAnchor {
                        group: child_id,
                        created_revision: credit.stock_revision,
                        captured_packs: credit.packs,
                    }],
                )
                .map_err(|_| PhysicalCorrectionError::InvalidHistory)?;
            if context
                .lineage
                .resolve(&single)
                .map_err(|_| PhysicalCorrectionError::InvalidHistory)?
                .packs()
                .value()
                > credit.packs.value()
            {
                return Err(PhysicalCorrectionError::InvalidHistory);
            }
            let review_id = treatment_record_id(credit.operation)
                .map_err(|_| PhysicalCorrectionError::InvalidHistory)?;
            let review = initial_reviews
                .get(&review_id)
                .ok_or(PhysicalCorrectionError::InvalidHistory)?;
            if child.parent != Some(credit.debit.group)
                || child.recovery
                    != Some(crate::hub::portions::PhysicalRecoveryOrigin {
                        debit_operation: original.key.operation,
                        debit_revision: original.stock_revision,
                    })
                || child.created_revision != credit.stock_revision
                || child.packs.value() > credit.packs.value()
                || review.stamp.stock_revision != credit.stock_revision
                || review.stamp.recorded_at != credit.recorded_at
                || !review.scope.anchors().iter().any(|a| {
                    a.group == child_id
                        && a.captured_packs == credit.packs
                        && a.created_revision == credit.stock_revision
                })
            {
                return Err(PhysicalCorrectionError::InvalidHistory);
            }

            if !credits.insert((credit.operation, credit.allocation))
                || scopes.contains_key(&PhysicalDebitKey {
                    operation: credit.operation,
                    group: credit.debit.group,
                })
            {
                return Err(PhysicalCorrectionError::Duplicate);
            }
            register(credit.operation, credit.stock_revision, credit.recorded_at)?;
            let amount = recovered.entry(credit.operation).or_insert(zero());
            *amount = add(*amount, credit.packs)?;
        }
        for reservation in &funding.returns {
            if !returns.insert(reservation.operation) {
                return Err(PhysicalCorrectionError::Duplicate);
            }
            register(
                reservation.operation,
                reservation.stock_revision,
                reservation.recorded_at,
            )?;
        }
    }
    let mut previous_at = None;
    for operation in revisions.values() {
        let at = operations
            .get(operation)
            .ok_or(PhysicalCorrectionError::InvalidHistory)?
            .1;
        if previous_at.is_some_and(|prior| prior > at) {
            return Err(PhysicalCorrectionError::InvalidHistory);
        }
        previous_at = Some(at);
    }
    if debit_operations
        .iter()
        .any(|op| credit_operations.contains(op) || returns.contains(op))
        || credit_operations.iter().any(|op| returns.contains(op))
    {
        return Err(PhysicalCorrectionError::Duplicate);
    }
    if base != 1
        || physical != context.target.effective()
        || returned != context.target.established_returns()
    {
        return Err(PhysicalCorrectionError::InvalidHistory);
    }
    for change in context.target.amendments() {
        match change.delta {
            DepartureQuantityDelta::AdditionalDeparture(packs) => {
                if additional.remove(&change.operation) != Some(packs) {
                    return Err(PhysicalCorrectionError::InvalidHistory);
                }
            }
            DepartureQuantityDelta::IdentifiedFoundRemainder(packs) => {
                let review_id = treatment_record_id(change.operation)
                    .map_err(|_| PhysicalCorrectionError::InvalidHistory)?;
                let review = initial_reviews
                    .get(&review_id)
                    .ok_or(PhysicalCorrectionError::InvalidHistory)?;
                let captured = review
                    .scope
                    .anchors()
                    .iter()
                    .try_fold(zero(), |total, a| add(total, a.captured_packs))?;
                if captured != packs || recovered.remove(&change.operation) != Some(packs) {
                    return Err(PhysicalCorrectionError::InvalidHistory);
                }
            }
        }
        if operations
            .get(&change.operation)
            .is_none_or(|(revision, _)| *revision != change.stock_revision)
        {
            return Err(PhysicalCorrectionError::InvalidHistory);
        }
    }
    if !additional.is_empty() || !recovered.is_empty() {
        return Err(PhysicalCorrectionError::InvalidHistory);
    }
    Ok(scopes)
}
