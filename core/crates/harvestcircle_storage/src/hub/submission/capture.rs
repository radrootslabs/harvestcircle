use super::*;

pub(super) async fn capture_on(
    tx: &mut ServiceSqliteTransaction<'_>,
    actor: PublicKey,
    scope: HubSubmissionScope,
    operation: &DurableRequestId,
    request: HubSubmittedRequest<'_>,
) -> Result<ArchivedHubSubmission, SafeError> {
    request.archive().map_err(archive_error)?;
    if actor != scope.actor {
        return Err(reference::hub_permission_denied());
    }
    if let HubSubmittedRequest::Initialize(candidate) = &request {
        candidate
            .authorize(Some(actor))
            .map_err(|_| reference::hub_permission_denied())?;
        if let Some((current, _)) = super::super::load_on(tx).await? {
            current
                .authorize(Some(actor))
                .map_err(|_| reference::hub_permission_denied())?;
            if current != **candidate {
                return Err(stock::conflict());
            }
        }
        return request
            .capture(
                operation,
                scope,
                &HubSubmissionContext {
                    hub: **candidate,
                    catalog_revision: None,
                    stock_revision: None,
                    material: &[],
                },
            )
            .map_err(archive_error);
    }
    let hub = reference::authorize_on(tx, actor).await?;
    let catalog = reference::load_catalog_on(tx, hub.id()).await?;
    let state = stock::read::state_on(tx, hub.id()).await?;
    let (expected_stock, expected_catalog) =
        targets::expected_versions(&request).ok_or_else(corrupt_storage)?;
    if expected_catalog != catalog.revision()
        || expected_stock.is_some_and(|revision| revision != state.revision)
    {
        return Err(stock::conflict());
    }
    let mut targets = targets::Targets::from_request(&request);
    if targets.groups.len() > harvestcircle_domain::hub::portions::HUB_PHYSICAL_SCOPES {
        return Err(stock::invalid_input());
    }
    let mut material = Vec::new();
    if let Some(id) = targets.count {
        let count = counts::read::by_id_on(tx, hub.id(), id).await?.original;
        targets.groups.insert(count.scope.group);
        material.push(HubSubmittedMaterial::Count(Box::new(count)));
    }
    if let Some(id) = targets.discrepancy {
        let record = handling::discrepancy::value_on(tx, hub.id(), id)
            .await?
            .value;
        targets.populations.insert(record.trace);
        // Closed/empty historical groups remain represented by the discrepancy;
        // resolving follow-up is not a physical mutation of those groups.
        material.push(HubSubmittedMaterial::Discrepancy(Box::new(record)));
    }
    if let Some(id) = targets.hold {
        let trace:Option<Vec<u8>>=sqlx::query_scalar("SELECT substr(trace_id,1,17) FROM hub_treatment_records WHERE record_id=? AND kind='hold_place' LIMIT 1")
            .bind(id.as_bytes().as_slice()).fetch_optional(&mut *tx).await.map_err(|_|storage_unavailable())?;
        let trace = HubRecordId::from_bytes(
            trace
                .ok_or_else(stock::invalid_input)?
                .try_into()
                .map_err(|_| corrupt_storage())?,
        )
        .map_err(|_| corrupt_storage())?;
        let graph =
            handling::lineage::load_on(tx, hub.id(), trace, &catalog, state.revision).await?;
        let treatment = handling::treatment::read::state_on(tx, hub.id(), &graph.graph).await?;
        let hold = treatment
            .holds()
            .iter()
            .find(|h| h.stamp.id == id)
            .ok_or_else(corrupt_storage)?
            .clone();
        targets.populations.insert(trace);
        material.push(HubSubmittedMaterial::Hold(Box::new(hold)));
    }
    if let Some((id, expected, selected)) = targets.dispatch {
        let record = handling::dispatch::read::record_on(tx, hub.id(), id).await?;
        if record.revision != expected {
            return Err(stock::conflict());
        }
        let identification = match selected {
            Some(id) => Some(
                record
                    .versions
                    .iter()
                    .find(|v| v.id == id)
                    .cloned()
                    .ok_or_else(stock::invalid_input)?,
            ),
            None => record.versions.last().cloned(),
        };
        let current_material = record
            .draft
            .as_ref()
            .or_else(|| identification.as_ref().map(|v| &v.material))
            .ok_or_else(corrupt_storage)?;
        // Issuing/departing reviews current groups; marking old paper unused is
        // always a nonphysical closure and retains its immutable issued content.
        let needs_current_groups = matches!(&request, HubSubmittedRequest::Departure(_))
            || matches!(&request,HubSubmittedRequest::Dispatch(c) if c.kind()=="dispatch_issue");
        if needs_current_groups {
            targets
                .groups
                .extend(current_material.lines.iter().map(|l| l.group));
        }
        material.push(HubSubmittedMaterial::Dispatch(Box::new(
            HubSubmittedDispatch {
                preparation: id,
                revision: record.revision,
                draft: record.draft,
                identification,
            },
        )));
    }
    let mut work = BTreeMap::new();
    if let Some(id) = targets.work {
        let value = handling::read::work_on(tx, hub.id(), id).await?;
        let rows=sqlx::query("SELECT substr(group_id,1,17) AS group_id FROM hub_group_work WHERE work_id=? AND progress='pending' ORDER BY group_id LIMIT 257")
            .bind(id.as_bytes().as_slice()).fetch_all(&mut *tx).await.map_err(|_|storage_unavailable())?;
        if rows.len() > harvestcircle_domain::hub::movement::HUB_REQUIREMENT_GROUPS {
            return Err(corrupt_storage());
        }
        for row in rows {
            targets
                .groups
                .insert(stock::read::record_id(&row, "group_id")?);
        }
        targets.populations.insert(value.trace);
        work.insert(id, value);
    }
    if targets.groups.len() > harvestcircle_domain::hub::portions::HUB_PHYSICAL_SCOPES {
        return Err(corrupt_storage());
    }
    for id in targets.groups {
        let group = handling::read::group_on(tx, hub.id(), id, &catalog).await?;
        targets.references.extend([group.variant, group.location]);
        targets.populations.insert(group.trace);
        for allocation in &group.work {
            match work.entry(allocation.requirement.id) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(allocation.requirement.clone());
                }
                std::collections::btree_map::Entry::Occupied(entry)
                    if entry.get() != &allocation.requirement =>
                {
                    return Err(corrupt_storage());
                }
                std::collections::btree_map::Entry::Occupied(_) => {}
            }
        }
        material.push(HubSubmittedMaterial::Group(Box::new(
            HubSubmittedGroup::from(&group),
        )));
    }
    for (_, value) in work {
        material.push(HubSubmittedMaterial::Work(Box::new(value)));
    }
    for id in targets.populations {
        material.push(HubSubmittedMaterial::Population(Box::new(
            handling::read::trace_on(tx, hub.id(), id).await?,
        )));
    }
    capture_references(&catalog, targets.references, &mut material)?;
    request
        .capture(
            operation,
            scope,
            &HubSubmissionContext {
                hub,
                catalog_revision: Some(catalog.revision()),
                stock_revision: expected_stock.map(|_| state.revision),
                material: &material,
            },
        )
        .map_err(archive_error)
}

fn capture_references(
    catalog: &ReferenceCatalog,
    mut ids: BTreeSet<HubRecordId>,
    material: &mut Vec<HubSubmittedMaterial>,
) -> Result<(), SafeError> {
    use harvestcircle_domain::hub::reference::ReferenceValue;
    let mut records = BTreeMap::new();
    while let Some(id) = ids.pop_first() {
        if records.contains_key(&id) {
            continue;
        }
        let value = catalog.get(id).map_err(|_| stock::invalid_input())?.clone();
        if let ReferenceValue::Pack { product, .. } = &value.value {
            ids.insert(*product);
        }
        records.insert(id, value);
    }
    material.extend(
        records
            .into_values()
            .map(|r| HubSubmittedMaterial::Reference(Box::new(r))),
    );
    Ok(())
}
