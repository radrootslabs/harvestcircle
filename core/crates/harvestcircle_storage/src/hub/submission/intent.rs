//! Reuse the existing domain's canonical hashes; do not invent another effect identity.
use super::*;
use harvestcircle_domain::hub::{
    HubId,
    departure::DepartureIdentities,
    dispatch::DispatchIdentities,
    movement::HandlingIdentities,
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
    }))
}
