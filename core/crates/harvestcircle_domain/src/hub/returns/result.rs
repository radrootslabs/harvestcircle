//! A seal of original captured facts; never permission to execute archived input.
use super::*;
use crate::hub::{custody_result, departure::departure_result_digest};

/// Seals only this intake's generic population facts. Stock/document readers can
/// verify a returned population without recursively loading every older return.
/// Full return/linkage reads must additionally verify `return_result_digest`.
pub fn return_population_digest(
    input: &[u8; 32],
    hub: HubId,
    catalog: HubRevision,
    ids: ReturnIdentities,
    population: &crate::hub::stock::StockPopulation,
) -> Result<[u8; 32], ReturnError> {
    use crate::hub::movement::{hash_occurrence, hash_text};
    use crate::hub::stock::PopulationOrigin;
    ids.validate()?;
    if population.id != ids.population
        || population.origin
            != (PopulationOrigin::Returned {
                returned: ids.returned,
            })
        || population.original_packs.value() == 0
        || population.supplier.is_some()
        || population.producer.is_some()
        || population.product.value.kind() != ReferenceKind::Product
        || !matches!(population.variant.value, ReferenceValue::Pack {product, ..} if product == population.product.id)
        || population.initial_location.value.kind() != ReferenceKind::Location
        || population.planned_location.as_ref().is_some_and(|target| {
            target.value.kind() != ReferenceKind::Location
                || target.id == population.initial_location.id
        })
    {
        return Err(ReturnError::InvalidNativeContext);
    }
    population.occurred.validate(population.recorded_at)?;
    let issuer = match &population.source {
        SourceAttribution::Unknown => None,
        SourceAttribution::Referenced { source, .. } => source.issuer,
    };
    if issuer != population.source_issuer.as_ref().map(|value| value.id)
        || population
            .source_issuer
            .as_ref()
            .is_some_and(|value| value.value.kind() != ReferenceKind::Party)
    {
        return Err(ReturnError::InvalidNativeContext);
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.return.population-result.v1\0");
    hash.update(input);
    hash.update(hub.as_bytes());
    hash.update(catalog.value().to_be_bytes());
    for id in [ids.operation, ids.returned, ids.population] {
        hash.update(id.as_bytes());
    }
    hash.update(population.actor.as_bytes());
    hash.update(population.recorded_at.as_seconds().to_be_bytes());
    hash.update(population.original_packs.value().to_be_bytes());
    hash_occurrence(&mut hash, population.occurred);
    hash_text(&mut hash, population.observation.as_str());
    hash_source(&mut hash, &population.source);
    for value in [
        &population.product,
        &population.variant,
        &population.initial_location,
    ] {
        if value.revision.value() > catalog.value() {
            return Err(ReturnError::InvalidNativeContext);
        }
        custody_result::reference(&mut hash, value, hub, population.actor, catalog);
    }
    for value in [&population.source_issuer, &population.planned_location] {
        hash.update([u8::from(value.is_some())]);
        if let Some(value) = value {
            if value.revision.value() > catalog.value() {
                return Err(ReturnError::InvalidNativeContext);
            }
            custody_result::reference(&mut hash, value, hub, population.actor, catalog);
        }
    }
    Ok(hash.finalize().into())
}

/// The context must be the complete immutable original capture when reading an
/// original, or fresh protected native data when planning a new effect.
pub fn return_result_digest(
    input: &[u8; 32],
    command: &ReturnCommand,
    context: &ReturnContext<'_>,
    record: &ReturnPlan,
) -> Result<[u8; 32], ReturnError> {
    if command.fingerprint(context.hub, record.intake.actor, record.identities) != *input
        || command.plan(
            context,
            record.identities,
            record.intake.actor,
            record.intake.recorded_at,
        )? != *record
    {
        return Err(ReturnError::InvalidNativeContext);
    }
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.return.result.v1\0");
    hash.update(input);
    hash.update(context.hub.as_bytes());
    hash.update(record.intake.actor.as_bytes());
    hash.update(record.intake.recorded_at.as_seconds().to_be_bytes());
    hash.update(record.stock_revision.value().to_be_bytes());
    hash.update(record.catalog_revision.value().to_be_bytes());
    // Replanning above checks every derived group, treatment, work and linkage
    // fact. Seal all captured inputs as well as current reference content, so
    // equal revision numbers cannot conceal a changed historical label/value.
    for value in [
        &record.intake.product,
        &record.intake.variant,
        &record.intake.location,
    ] {
        custody_result::reference(
            &mut hash,
            value,
            context.hub,
            record.intake.actor,
            record.catalog_revision,
        );
    }
    for value in [
        &record.intake.returned_by,
        &record.intake.source_issuer,
        &record.intake.planned_location,
    ] {
        hash.update([u8::from(value.is_some())]);
        if let Some(value) = value {
            custody_result::reference(
                &mut hash,
                value,
                context.hub,
                record.intake.actor,
                record.catalog_revision,
            );
        }
    }
    hash.update([u8::from(context.departure.is_some())]);
    if let Some(departure) = context.departure {
        hash.update(departure_result_digest(&[0; 32], departure)?);
    }
    let mut totals = context.established_return_totals.to_vec();
    totals.sort_by_key(|total| total.original_group);
    hash.update(
        u32::try_from(totals.len())
            .map_err(|_| ReturnError::InvalidNativeContext)?
            .to_be_bytes(),
    );
    for total in totals {
        hash.update(total.original_group.as_bytes());
        hash.update(total.packs.value().to_be_bytes());
    }
    Ok(hash.finalize().into())
}
