//! Reuse the existing domain's canonical hashes; do not invent another effect identity.
use super::*;
use harvestcircle_domain::hub::{
    HubId,
    departure::DepartureIdentities,
    dispatch::DispatchIdentities,
    disposition::DispositionIdentities,
    movement::HandlingIdentities,
    returns::ReturnIdentities,
    stock::{OpeningIdentities, ReceiptIdentities},
};

pub(super) fn hash(
    request: &HubSubmittedRequest<'_>,
    hub: HubId,
    actor: PublicKey,
    operation: &DurableRequestId,
) -> Result<Option<[u8; 32]>, SafeError> {
    let id = HubRecordId::from_hex(&operation.as_str().replace('-', ""))
        .map_err(|_| stock::invalid_input())?;
    Ok(Some(match request {
        HubSubmittedRequest::Initialize(_) => return Ok(None),
        HubSubmittedRequest::Reference(c) => c.fingerprint(hub, actor),
        HubSubmittedRequest::Receive(c) => {
            let ids =
                ReceiptIdentities::from_operation_bytes(id).map_err(|_| stock::invalid_input())?;
            c.fingerprint(hub, actor, ids.trace, ids.group)
        }
        HubSubmittedRequest::Opening(c) => c.fingerprint(
            hub,
            actor,
            OpeningIdentities::from_operation_bytes(id).map_err(|_| stock::invalid_input())?,
        ),
        HubSubmittedRequest::Handling(c) => c.fingerprint(
            hub,
            actor,
            HandlingIdentities::from_operation_bytes(id).map_err(|_| stock::invalid_input())?,
        ),
        HubSubmittedRequest::Dispatch(c) => c.fingerprint(
            hub,
            actor,
            DispatchIdentities::from_operation_bytes(id).map_err(|_| stock::invalid_input())?,
        ),
        HubSubmittedRequest::Departure(c) => c.fingerprint(
            hub,
            actor,
            DepartureIdentities::from_operation_bytes(id).map_err(|_| stock::invalid_input())?,
        ),
        HubSubmittedRequest::CountObservation(c) => c.fingerprint(hub, actor, id),
        HubSubmittedRequest::CountPosting(c) => c.fingerprint(hub, actor, id),
        HubSubmittedRequest::ActualReport(c) => c.fingerprint(hub, actor, id),
        HubSubmittedRequest::ActualReview(c) => c.fingerprint(hub, actor, id),
        HubSubmittedRequest::ActualLink(c) => c.fingerprint(hub, actor, id),
        HubSubmittedRequest::FoundCorrection(c) => c.fingerprint(hub, actor, id),
        HubSubmittedRequest::AdditionalQuantity(c) => c.fingerprint(hub, actor, id),
        HubSubmittedRequest::SourceAttribution(c) => c.fingerprint(hub, actor, id),
        HubSubmittedRequest::RecipientAttribution(c) => c.fingerprint(hub, actor, id),
        HubSubmittedRequest::CountExplanation(c) => c.fingerprint(hub, actor, id),
        HubSubmittedRequest::CountClassification(c) => c.fingerprint(hub, actor, id),
        HubSubmittedRequest::CorrectionObservation(c) => c
            .fingerprint(hub, actor, id)
            .map_err(crate::hub::corrections::error)?,
        HubSubmittedRequest::Return(c) => c.fingerprint(
            hub,
            actor,
            ReturnIdentities::from_operation_bytes(id).map_err(|_| stock::invalid_input())?,
        ),
        HubSubmittedRequest::Disposition(c) => c.fingerprint(
            hub,
            actor,
            DispositionIdentities::from_operation_bytes(id).map_err(|_| stock::invalid_input())?,
        ),
    }))
}
