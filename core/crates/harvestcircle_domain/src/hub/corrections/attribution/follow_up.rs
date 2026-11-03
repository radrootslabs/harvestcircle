use super::*;

/// A local record of manual work, never proof of external delivery/acknowledgement.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub enum CorrectionManualWork {
    Outstanding,
    WorkRecorded,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CorrectionFollowUpObservation {
    pub stamp: AttributionStamp,
    pub correction: HubRecordId,
    pub target: CorrectionImpact,
    pub prior: Option<HubRecordId>,
    pub state: CorrectionManualWork,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct CorrectionFollowUpCommand {
    pub correction: HubRecordId,
    pub target: CorrectionImpact,
    pub expected_stock_revision: HubRevision,
    pub expected_catalog_revision: HubRevision,
    pub expected_prior: Option<HubRecordId>,
    pub state: CorrectionManualWork,
    pub basis: SourceProvenance,
}
pub struct CorrectionFollowUpContext<'a> {
    pub cut: AttributionCut<'a>,
    pub correction: &'a AttributionStamp,
    pub impact: &'a CorrectionImpactSet,
    /// Complete ordered observations for this correction, across all its targets.
    pub history: &'a [CorrectionFollowUpObservation],
}
impl CorrectionFollowUpContext<'_> {
    pub fn validate(&self) -> Result<(), AttributionError> {
        if self.history.len() > HUB_CORRECTION_ROWS {
            return Err(AttributionError::Capacity);
        }
        if self.correction.hub != self.cut.hub
            || self.correction.stock_revision.value() > self.cut.stock_revision.value()
            || self.correction.catalog_revision.value() > self.cut.catalog.revision().value()
        {
            return Err(AttributionError::InvalidHistory);
        }
        let mut operations = BTreeSet::from([self.correction.operation]);
        let mut targets = std::collections::BTreeMap::new();
        let mut revision = self.correction.stock_revision;
        let mut catalog = self.correction.catalog_revision;
        let mut at = self.correction.recorded_at;
        for row in self.history {
            check_stamp(&row.stamp, self.cut.hub, revision, catalog, at)?;
            if row.correction != self.correction.operation
                || !self.impact.contains(row.target)
                || row.prior != targets.insert(row.target, row.stamp.operation)
                || !operations.insert(row.stamp.operation)
                || row.stamp.stock_revision.value() > self.cut.stock_revision.value()
                || row.stamp.catalog_revision.value() > self.cut.catalog.revision().value()
            {
                return Err(AttributionError::InvalidHistory);
            }
            revision = row.stamp.stock_revision;
            catalog = row.stamp.catalog_revision;
            at = row.stamp.recorded_at;
        }
        Ok(())
    }
    pub fn current(
        &self,
        target: CorrectionImpact,
    ) -> Result<CorrectionManualWork, AttributionError> {
        self.validate()?;
        if !self.impact.contains(target) {
            return Err(AttributionError::InvalidScope);
        }
        Ok(self
            .history
            .iter()
            .rev()
            .find(|r| r.target == target)
            .map_or(CorrectionManualWork::Outstanding, |r| r.state))
    }
}
impl CorrectionFollowUpCommand {
    pub fn plan(
        &self,
        context: &CorrectionFollowUpContext<'_>,
        operation: HubRecordId,
        actor: PublicKey,
        now: UnixTimestamp,
    ) -> Result<CorrectionFollowUpObservation, AttributionError> {
        let next = context.cut.check(
            self.expected_stock_revision,
            self.expected_catalog_revision,
            now,
        )?;
        context.validate()?;
        if context.history.len() >= HUB_CORRECTION_ROWS {
            return Err(AttributionError::Capacity);
        }
        if self.correction != context.correction.operation || !context.impact.contains(self.target)
        {
            return Err(AttributionError::InvalidScope);
        }
        let previous = context
            .history
            .iter()
            .rev()
            .find(|r| r.target == self.target);
        if self.expected_prior != previous.map(|r| r.stamp.operation) {
            return Err(AttributionError::StaleRevision);
        }
        if operation == self.correction
            || context
                .history
                .iter()
                .any(|r| r.stamp.operation == operation)
            || now
                < context
                    .history
                    .last()
                    .map_or(context.correction.recorded_at, |r| r.stamp.recorded_at)
        {
            return Err(AttributionError::InvalidHistory);
        }
        Ok(CorrectionFollowUpObservation {
            stamp: AttributionStamp {
                operation,
                hub: context.cut.hub,
                stock_revision: next,
                catalog_revision: context.cut.catalog.revision(),
                actor,
                recorded_at: now,
                basis: self.basis.clone(),
            },
            correction: self.correction,
            target: self.target,
            prior: self.expected_prior,
            state: self.state,
        })
    }
}
