use super::*;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub enum AmendedRecipient {
    Unknown,
    Known(ReferenceRecord),
}
impl AmendedRecipient {
    pub fn validate(&self, catalog: HubRevision) -> Result<(), AttributionError> {
        match self {
            Self::Unknown => Ok(()),
            Self::Known(r) => party(r, catalog),
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum CorrectedDepartureFamily {
    Normal,
    ReviewedActual,
    ExplainedCountLoss,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct OriginalRecipientAttribution {
    pub hub: HubId,
    pub family: CorrectedDepartureFamily,
    pub operation: HubRecordId,
    pub stock_revision: HubRevision,
    pub catalog_revision: HubRevision,
    pub recorded_at: UnixTimestamp,
    pub actor: PublicKey,
    pub recipient: ReferenceRecord,
    /// Every population of this immutable event, not a selected line.
    pub populations: Vec<HubRecordId>,
    /// All frozen issued versions; amendment never regenerates any of them.
    pub issued_versions: Vec<HubRecordId>,
}
impl OriginalRecipientAttribution {
    fn validate(&self) -> Result<(), AttributionError> {
        party(&self.recipient, self.catalog_revision)?;
        if self.stock_revision == HubRevision::INITIAL
            || (self.family == CorrectedDepartureFamily::Normal && self.issued_versions.is_empty())
            || (self.family != CorrectedDepartureFamily::Normal && !self.issued_versions.is_empty())
        {
            return Err(AttributionError::InvalidHistory);
        }
        self.affected(vec![AmendedRecipient::Known(self.recipient.clone())])
            .impact(&mut ImpactBuilder::new(), self.catalog_revision)
    }
    fn affected(&self, recipients: Vec<AmendedRecipient>) -> AttributionAffectedDeparture {
        AttributionAffectedDeparture {
            operation: self.operation,
            populations: self.populations.clone(),
            recipients,
            issued_versions: self.issued_versions.clone(),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct RecipientAttributionCorrection {
    pub stamp: AttributionStamp,
    pub original: OriginalRecipientAttribution,
    pub prior: Option<HubRecordId>,
    pub before: AmendedRecipient,
    pub after: AmendedRecipient,
    pub impact: CorrectionImpactSet,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct RecipientAttributionCommand {
    pub departure_operation: HubRecordId,
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub expected_prior: Option<HubRecordId>,
    /// None is explicit unknown, never an inferred party or silent old value.
    pub recipient: Option<HubRecordId>,
    pub basis: SourceProvenance,
}
pub struct RecipientAttributionContext<'a> {
    pub cut: AttributionCut<'a>,
    pub original: &'a OriginalRecipientAttribution,
    pub history: &'a [RecipientAttributionCorrection],
}
pub fn current_recipient_attribution(
    original: &OriginalRecipientAttribution,
    history: &[RecipientAttributionCorrection],
) -> Result<AmendedRecipient, AttributionError> {
    if history.len() > HUB_CORRECTION_ROWS {
        return Err(AttributionError::Capacity);
    }
    original.validate()?;
    let mut current = AmendedRecipient::Known(original.recipient.clone());
    let mut prior = None;
    let mut revision = original.stock_revision;
    let mut catalog = original.catalog_revision;
    let mut at = original.recorded_at;
    let mut operations = BTreeSet::from([original.operation]);
    let mut required = ImpactBuilder::new();
    original
        .affected(vec![current.clone()])
        .impact(&mut required, original.catalog_revision)?;
    for record in history {
        check_stamp(&record.stamp, original.hub, revision, catalog, at)?;
        if record.original != *original
            || record.prior != prior
            || record.before != current
            || record.after == record.before
            || !record
                .impact
                .contains(CorrectionImpact::Departure(original.operation))
            || original
                .populations
                .iter()
                .any(|id| !record.impact.contains(CorrectionImpact::Population(*id)))
            || original
                .issued_versions
                .iter()
                .any(|id| !record.impact.contains(CorrectionImpact::IssuedVersion(*id)))
            || !operations.insert(record.stamp.operation)
        {
            return Err(AttributionError::InvalidHistory);
        }
        record.after.validate(record.stamp.catalog_revision)?;
        if matches!((&record.before,&record.after),(AmendedRecipient::Known(a),AmendedRecipient::Known(b)) if a.id==b.id)
        {
            return Err(AttributionError::InvalidHistory);
        }
        required.recipient(original.operation, &record.after)?;
        if record.impact != required.clone().finish()? {
            return Err(AttributionError::InvalidHistory);
        }
        current = record.after.clone();
        prior = Some(record.stamp.operation);
        revision = record.stamp.stock_revision;
        catalog = record.stamp.catalog_revision;
        at = record.stamp.recorded_at;
    }
    Ok(current)
}
impl RecipientAttributionCommand {
    pub fn plan(
        &self,
        context: &RecipientAttributionContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<RecipientAttributionCorrection, AttributionError> {
        let next = context.cut.check(
            self.expected_stock_revision,
            self.expected_catalog_revision,
            now,
        )?;
        if context.history.len() >= HUB_CORRECTION_ROWS {
            return Err(AttributionError::Capacity);
        }
        if self.departure_operation != context.original.operation
            || context.original.hub != context.cut.hub
        {
            return Err(AttributionError::InvalidScope);
        }
        let before = current_recipient_attribution(context.original, context.history)?;
        let last = context.history.last();
        if self.expected_prior != last.map(|r| r.stamp.operation) {
            return Err(AttributionError::StaleRevision);
        }
        if last
            .map_or(context.original.stock_revision, |r| r.stamp.stock_revision)
            .value()
            > context.cut.stock_revision.value()
            || last
                .map_or(context.original.catalog_revision, |r| {
                    r.stamp.catalog_revision
                })
                .value()
                > context.cut.catalog.revision().value()
            || now < last.map_or(context.original.recorded_at, |r| r.stamp.recorded_at)
            || operation == context.original.operation
            || context
                .history
                .iter()
                .any(|r| r.stamp.operation == operation)
        {
            return Err(AttributionError::InvalidHistory);
        }
        let after = self
            .recipient
            .map(|id| {
                context
                    .cut
                    .catalog
                    .require_kind(id, ReferenceKind::Party)
                    .cloned()
                    .map(AmendedRecipient::Known)
                    .map_err(|_| AttributionError::InvalidReference)
            })
            .transpose()?
            .unwrap_or(AmendedRecipient::Unknown);
        if matches!(
            (&before, &after),
            (AmendedRecipient::Unknown, AmendedRecipient::Unknown)
        ) || matches!((&before,&after),(AmendedRecipient::Known(a),AmendedRecipient::Known(b)) if a.id==b.id)
        {
            return Err(AttributionError::NoChange);
        }
        let mut recipients = vec![AmendedRecipient::Known(context.original.recipient.clone())];
        recipients.extend(context.history.iter().map(|r| r.after.clone()));
        recipients.push(after.clone());
        let mut impact = ImpactBuilder::new();
        context
            .original
            .affected(recipients)
            .impact(&mut impact, context.cut.catalog.revision())?;
        Ok(RecipientAttributionCorrection {
            stamp: AttributionStamp {
                operation,
                hub: context.cut.hub,
                stock_revision: next,
                catalog_revision: context.cut.catalog.revision(),
                actor,
                recorded_at: now,
                basis: self.basis.clone(),
            },
            original: context.original.clone(),
            prior: self.expected_prior,
            before,
            after,
            impact: impact.finish()?,
        })
    }
}
