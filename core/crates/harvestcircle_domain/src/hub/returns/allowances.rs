//! Captured current interpretation never rewrites an issued departure line.
use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReturnQuantityHead {
    pub operation: HubRecordId,
    pub stock_revision: HubRevision,
    pub projection_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReturnLineAllowance {
    pub original_group: HubRecordId,
    pub effective_departure: WholePacks,
    /// Original physical debit less recoveries funded by that exact debit.
    /// Additional stock from another debit never expands this quantity.
    pub original_debit_return_bound: WholePacks,
    pub quantity_head: Option<ReturnQuantityHead>,
    pub funding_hash: [u8; 32],
    pub unresolved_returns: bool,
}
impl ReturnLineAllowance {
    #[must_use]
    pub fn linkable_quantity(&self) -> WholePacks {
        if self.effective_departure.value() < self.original_debit_return_bound.value() {
            self.effective_departure
        } else {
            self.original_debit_return_bound
        }
    }
}

/// Descriptive native evidence. Constructors do not establish completeness,
/// current ownership, canonical funding or a fresh effect authorization.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct ReturnAllowanceCapture {
    pub hub: HubId,
    pub departure: HubRecordId,
    pub operation: HubRecordId,
    pub stock_revision: HubRevision,
    pub lines: Vec<ReturnLineAllowance>,
}
impl ReturnAllowanceCapture {
    /// Check intrinsic scope and bounds. Native callers must still prove the
    /// complete canonical heads and funding at this exact captured cut.
    pub fn validate(
        &self,
        context: &ReturnContext<'_>,
        departure: &DepartureRecord,
    ) -> Result<(), ReturnError> {
        let issued = departure
            .paper
            .versions
            .last()
            .ok_or(ReturnError::InvalidNativeContext)?;
        if self.hub != context.hub
            || self.departure != departure.id
            || self.operation != departure.operation
            || self.stock_revision != context.stock_revision
            || self.lines.len() != issued.material.lines.len()
            || self.lines.is_empty()
            || self.lines.len() > 64
        {
            return Err(ReturnError::InvalidNativeContext);
        }
        let mut groups = std::collections::BTreeSet::new();
        for allowance in &self.lines {
            let line = issued
                .material
                .lines
                .iter()
                .find(|line| line.group == allowance.original_group)
                .ok_or(ReturnError::InvalidNativeContext)?;
            if !groups.insert(allowance.original_group)
                || allowance.original_debit_return_bound.value() > line.packs.value()
                || (allowance.quantity_head.is_none()
                    && (allowance.effective_departure != line.packs
                        || allowance.original_debit_return_bound != line.packs))
                || allowance.funding_hash == [0; 32]
                || allowance.quantity_head.is_some_and(|head| {
                    head.operation == departure.operation
                        || head.stock_revision.value() <= departure.stock_revision.value()
                        || head.stock_revision.value() > self.stock_revision.value()
                        || head.projection_hash == [0; 32]
                })
            {
                return Err(ReturnError::InvalidNativeContext);
            }
        }
        Ok(())
    }

    pub(super) fn hash_into(&self, hash: &mut Sha256) -> Result<(), ReturnError> {
        hash.update(b"harvestcircle.hub.return.current-allowance.v1\0");
        hash.update(self.hub.as_bytes());
        hash.update(self.departure.as_bytes());
        hash.update(self.operation.as_bytes());
        hash.update(self.stock_revision.value().to_be_bytes());
        let mut lines = self.lines.iter().collect::<Vec<_>>();
        lines.sort_by_key(|line| line.original_group);
        hash.update(
            u32::try_from(lines.len())
                .map_err(|_| ReturnError::InvalidNativeContext)?
                .to_be_bytes(),
        );
        for line in lines {
            hash.update(line.original_group.as_bytes());
            hash.update(line.effective_departure.value().to_be_bytes());
            hash.update(line.original_debit_return_bound.value().to_be_bytes());
            hash.update([u8::from(line.quantity_head.is_some())]);
            if let Some(head) = line.quantity_head {
                hash.update(head.operation.as_bytes());
                hash.update(head.stock_revision.value().to_be_bytes());
                hash.update(head.projection_hash);
            }
            hash.update(line.funding_hash);
            hash.update([u8::from(line.unresolved_returns)]);
        }
        Ok(())
    }
}
