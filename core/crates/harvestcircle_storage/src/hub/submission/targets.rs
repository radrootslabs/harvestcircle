use super::*;
use harvestcircle_domain::hub::{
    movement::{DiscrepancyAction, HandlingCommand},
    reference::{ReferenceMutation, ReferenceValue, SourceAttribution},
    treatment::TreatmentAction,
};

#[derive(Default)]
pub(super) struct Targets {
    pub references: BTreeSet<HubRecordId>,
    pub groups: BTreeSet<HubRecordId>,
    pub populations: BTreeSet<HubRecordId>,
    pub work: Option<HubRecordId>,
    pub dispatch: Option<(HubRecordId, HubRevision, Option<HubRecordId>)>,
    pub count: Option<HubRecordId>,
    pub hold: Option<HubRecordId>,
    pub discrepancy: Option<HubRecordId>,
}
impl Targets {
    pub fn from_request(request: &HubSubmittedRequest<'_>) -> Self {
        let mut t = Self::default();
        match request {
            HubSubmittedRequest::Initialize(_) => {}
            HubSubmittedRequest::Reference(c) => match &c.mutation {
                ReferenceMutation::Create { value, .. } => t.reference_value(value),
                ReferenceMutation::Revise { id, value, .. } => {
                    t.references.insert(*id);
                    t.reference_value(value);
                }
            },
            HubSubmittedRequest::Receive(c) => {
                t.references.extend(
                    [
                        Some(c.variant),
                        c.supplier,
                        c.producer,
                        Some(c.receiving),
                        c.planned_location,
                    ]
                    .into_iter()
                    .flatten(),
                );
                t.source(&c.source);
            }
            HubSubmittedRequest::Opening(c) => {
                t.references.extend([c.variant, c.location]);
                t.source(&c.source);
            }
            HubSubmittedRequest::Handling(c) => match c {
                HandlingCommand::Move(c) => {
                    t.groups.insert(c.group);
                    t.references.insert(c.to);
                }
                HandlingCommand::Split(c) => {
                    t.groups.insert(c.group);
                }
                HandlingCommand::Treatment(c) => match &c.action {
                    TreatmentAction::SetAcceptance { selection, .. }
                    | TreatmentAction::PlaceHold { selection } => {
                        t.groups.extend(selection.groups())
                    }
                    TreatmentAction::ReleaseHold { hold, .. } => t.hold = Some(*hold),
                },
                HandlingCommand::NewWork(c) => {
                    t.groups.extend(&c.groups);
                    t.references.insert(c.target);
                }
                HandlingCommand::Replan(c) => {
                    t.work = Some(c.requirement);
                    t.references.insert(c.target);
                }
                HandlingCommand::Discrepancy(c) => match &c.action {
                    DiscrepancyAction::Open { trace, group, .. } => {
                        t.populations.insert(*trace);
                        t.groups.extend(group);
                    }
                    DiscrepancyAction::Resolve { discrepancy, .. } => {
                        t.discrepancy = Some(*discrepancy)
                    }
                },
            },
            HubSubmittedRequest::Dispatch(c) => {
                if let Some(selection) = c.action.selection() {
                    t.groups.extend(selection.lines().iter().map(|l| l.group));
                }
                if let Some((id, revision)) = c.action.target() {
                    t.dispatch = Some((id, revision, None));
                }
                use harvestcircle_domain::hub::dispatch::DispatchAction;
                match &c.action {
                    DispatchAction::Prepare { recipient, .. }
                    | DispatchAction::Revise { recipient, .. }
                    | DispatchAction::Replace { recipient, .. } => {
                        t.references.insert(*recipient);
                    }
                    DispatchAction::Issue { .. } | DispatchAction::MarkUnused { .. } => {}
                }
            }
            HubSubmittedRequest::Departure(c) => {
                t.dispatch = Some((
                    c.preparation,
                    c.expected_preparation_revision,
                    Some(c.identification),
                ))
            }
            HubSubmittedRequest::CountObservation(c) => {
                t.groups.insert(c.group);
            }
            HubSubmittedRequest::CountPosting(c) => t.count = Some(c.count),
        }
        t
    }
    fn reference_value(&mut self, value: &ReferenceValue) {
        if let ReferenceValue::Pack { product, .. } = value {
            self.references.insert(*product);
        }
    }
    fn source(&mut self, source: &SourceAttribution) {
        if let SourceAttribution::Referenced { source, .. } = source {
            self.references.extend(source.issuer);
        }
    }
}

pub(super) fn expected_versions(
    request: &HubSubmittedRequest<'_>,
) -> Option<(Option<HubRevision>, HubRevision)> {
    Some(match request {
        HubSubmittedRequest::Initialize(_) => return None,
        HubSubmittedRequest::Reference(c) => (None, c.expected_catalog_revision),
        HubSubmittedRequest::Receive(c) => {
            (Some(c.expected_stock_revision), c.expected_catalog_revision)
        }
        HubSubmittedRequest::Opening(c) => {
            (Some(c.expected_stock_revision), c.expected_catalog_revision)
        }
        HubSubmittedRequest::Handling(c) => {
            let (stock, catalog) = c.expected_versions();
            (Some(stock), catalog)
        }
        HubSubmittedRequest::Dispatch(c) => {
            (Some(c.expected_stock_revision), c.expected_catalog_revision)
        }
        HubSubmittedRequest::Departure(c) => {
            (Some(c.expected_stock_revision), c.expected_catalog_revision)
        }
        HubSubmittedRequest::CountObservation(c) => {
            (Some(c.expected_stock_revision), c.expected_catalog_revision)
        }
        HubSubmittedRequest::CountPosting(c) => {
            (Some(c.expected_stock_revision), c.expected_catalog_revision)
        }
    })
}
