//! Original-state consistency only; this digest is not a signature or permission.
use super::*;
use crate::hub::movement::{hash_occurrence, hash_text};
use crate::hub::reference::{
    ReferenceCommand, ReferenceMutation, SourceAttribution, SourceCertainty, SourceReferenceKind,
};
use crate::hub::stock::{OpeningHistory, PopulationOrigin};

pub fn dispatch_result_digest(
    input: &[u8; 32],
    record: &DispatchRecord,
) -> Result<[u8; 32], DispatchError> {
    record.validate()?;
    let mut hash = Sha256::new();
    hash.update(b"harvestcircle.hub.dispatch.result.v1\0");
    hash.update(input);
    hash.update(record.id.as_bytes());
    hash.update(record.hub.as_bytes());
    hash.update(record.revision.value().to_be_bytes());
    stamp(&mut hash, &record.created);
    stamp(&mut hash, &record.updated);
    hash.update([u8::from(record.draft.is_some())]);
    if let Some(draft) = &record.draft {
        material(&mut hash, record, draft);
    }
    hash.update(
        u32::try_from(record.versions.len())
            .expect("bounded versions")
            .to_be_bytes(),
    );
    for version in &record.versions {
        hash.update(version.id.as_bytes());
        hash.update(version.number.to_be_bytes());
        optional_id(&mut hash, version.previous);
        material(&mut hash, record, &version.material);
        stamp(&mut hash, &version.issued);
        hash.update([match version.status {
            IdentificationStatus::Issued => 1,
            IdentificationStatus::Superseded => 2,
            IdentificationStatus::Unused => 3,
        }]);
        hash.update([u8::from(version.closed.is_some())]);
        if let Some(closed) = &version.closed {
            stamp(&mut hash, closed);
        }
    }
    Ok(hash.finalize().into())
}

fn stamp(hash: &mut Sha256, value: &DispatchStamp) {
    hash.update(value.operation.as_bytes());
    hash.update(value.stock_revision.value().to_be_bytes());
    hash.update(value.actor.as_bytes());
    hash.update(value.recorded_at.as_seconds().to_be_bytes());
    hash_text(hash, value.observation.as_str());
}

fn material(hash: &mut Sha256, owner: &DispatchRecord, value: &DispatchMaterial) {
    hash.update(value.stock_revision.value().to_be_bytes());
    hash.update(value.catalog_revision.value().to_be_bytes());
    hash_text(hash, value.basis.id());
    hash.update(value.basis.version().to_be_bytes());
    hash_text(hash, value.basis.site_timezone());
    reference(hash, owner, value.catalog_revision, &value.recipient);
    hash_text(hash, value.observation.as_str());
    hash.update(
        u32::try_from(value.lines.len())
            .expect("bounded material")
            .to_be_bytes(),
    );
    for line in &value.lines {
        hash.update(line.group.as_bytes());
        hash.update(line.group_revision.value().to_be_bytes());
        hash.update(line.packs.value().to_be_bytes());
        hash.update([u8::from(line.identification.is_some())]);
        if let Some(identity) = &line.identification {
            hash_text(hash, identity.label.as_str());
            hash_text(hash, identity.observation.as_str());
        }
        for record in [&line.location, &line.product, &line.variant] {
            reference(hash, owner, value.catalog_revision, record);
        }
        trace(hash, owner, value.catalog_revision, &line.trace);
    }
}

fn reference(
    hash: &mut Sha256,
    owner: &DispatchRecord,
    catalog: HubRevision,
    record: &ReferenceRecord,
) {
    hash.update(record.revision.value().to_be_bytes());
    hash.update(
        ReferenceCommand {
            expected_catalog_revision: catalog,
            mutation: ReferenceMutation::Create {
                id: record.id,
                value: record.value.clone(),
            },
        }
        .fingerprint(owner.hub, owner.created.actor),
    );
}

fn optional_reference(
    hash: &mut Sha256,
    owner: &DispatchRecord,
    catalog: HubRevision,
    value: Option<&ReferenceRecord>,
) {
    hash.update([u8::from(value.is_some())]);
    if let Some(value) = value {
        reference(hash, owner, catalog, value);
    }
}

fn optional_id(hash: &mut Sha256, value: Option<HubRecordId>) {
    hash.update([u8::from(value.is_some())]);
    if let Some(value) = value {
        hash.update(value.as_bytes());
    }
}

fn trace(hash: &mut Sha256, owner: &DispatchRecord, catalog: HubRevision, value: &StockPopulation) {
    hash.update(value.id.as_bytes());
    reference(hash, owner, catalog, &value.variant);
    reference(hash, owner, catalog, &value.product);
    for value in [
        value.supplier.as_ref(),
        value.producer.as_ref(),
        value.source_issuer.as_ref(),
    ] {
        optional_reference(hash, owner, catalog, value);
    }
    match &value.source {
        SourceAttribution::Unknown => hash.update([0]),
        SourceAttribution::Referenced { source, certainty } => {
            hash.update([1]);
            hash.update(source.id.as_bytes());
            optional_id(hash, source.issuer);
            hash.update([match source.kind {
                SourceReferenceKind::ProducerBatch => 1,
                SourceReferenceKind::SupplierReference => 2,
                SourceReferenceKind::Other => 3,
            }]);
            hash_text(hash, source.code.as_str());
            hash_text(hash, source.provenance.as_str());
            hash.update([match certainty {
                SourceCertainty::ReportedPossible => 1,
                SourceCertainty::Established => 2,
            }]);
        }
    }
    hash.update(value.original_packs.value().to_be_bytes());
    reference(hash, owner, catalog, &value.initial_location);
    optional_reference(hash, owner, catalog, value.planned_location.as_ref());
    hash_occurrence(hash, value.occurred);
    hash.update(value.recorded_at.as_seconds().to_be_bytes());
    hash.update(value.actor.as_bytes());
    hash_text(hash, value.observation.as_str());
    // Receipt v1 bytes remain frozen for already issued documents.
    match &value.origin {
        PopulationOrigin::Receipt => (),
        PopulationOrigin::CountedOpening { history } => {
            hash.update(b"harvestcircle.hub.population.counted-opening.v1\0");
            match history {
                OpeningHistory::Unknown => hash.update([0]),
                OpeningHistory::ExternalReference(reference) => {
                    hash.update([1]);
                    hash_text(hash, reference.as_str());
                }
            }
        }
        PopulationOrigin::CountedSurplus {
            count,
            related_population,
        } => {
            hash.update(b"harvestcircle.hub.population.counted-surplus.v1\0");
            hash.update(count.as_bytes());
            hash.update(related_population.as_bytes());
        }
    }
}
