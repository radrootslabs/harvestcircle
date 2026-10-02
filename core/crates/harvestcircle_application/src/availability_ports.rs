//! Trusted local availability admission, read ports and independent refresh.
//!
//! This service validates structural requests and returned evidence. Concrete
//! adapters must admit the local account/data at the actual I/O boundary and
//! bind reads atomically to the admitted storage snapshot. They also own scan,
//! byte, deadline and response-buffer limits. No storage, network, signer,
//! scheduler, native admission or runtime resource enforcement is implemented
//! here, and local rows remain outside the application snapshot.

use std::sync::Arc;
use std::time::{Duration, Instant};

use harvestcircle_domain::error::AvailabilityFailure;
use harvestcircle_domain::{
    AvailabilityEventVersion, AvailabilityHeadView, AvailabilityListingCoordinate,
    AvailabilityPage, AvailabilityPageContinuation, AvailabilityPageCursor,
    AvailabilityVersionView, SafeError,
};

use crate::{
    AvailabilityDiscoveryOutcome, AvailabilityDiscoveryRequest, AvailabilityLocalQueryScope,
    BoxFuture, CommandContext, CommandReceipt, CommandResult, CommandSubmission, RequestId,
    ScopedAvailabilityQuery,
};

/// Trusted local account/data authority, independent of signing capability.
///
/// A concrete runtime adapter must establish OS/account/data admission and
/// return the selected owner's persisted context/store/source/projection and
/// session bindings. A caller-created structural scope is not admission proof.
pub trait AvailabilityLocalAdmission: Send + Sync {
    fn current_scope<'a>(&'a self)
    -> BoxFuture<'a, Result<AvailabilityLocalQueryScope, SafeError>>;
}

/// Local storage reads without transport or signing dependencies.
///
/// Concrete adapters must enforce admission and snapshot selection at I/O,
/// order/filter rows and continuations consistently, and enforce the actual
/// scan, byte, deadline and response-buffer budgets. Returned errors retain
/// their existing safe codes and messages when the scope remains current.
pub trait AvailabilityLocalReadPort: Send + Sync {
    fn read_page<'a>(
        &'a self,
        query: &'a ScopedAvailabilityQuery,
    ) -> BoxFuture<'a, Result<AvailabilityPage<AvailabilityHeadView>, SafeError>>;

    fn read_head<'a>(
        &'a self,
        scope: &'a AvailabilityLocalQueryScope,
        coordinate: &'a AvailabilityListingCoordinate,
    ) -> BoxFuture<'a, Result<AvailabilityHeadView, SafeError>>;

    /// `None` describes missing retained evidence, not exhaustive relay data.
    fn read_version<'a>(
        &'a self,
        scope: &'a AvailabilityLocalQueryScope,
        coordinate: &'a AvailabilityListingCoordinate,
        version: AvailabilityEventVersion,
    ) -> BoxFuture<'a, Result<Option<AvailabilityVersionView>, SafeError>>;
}

/// Nonblocking refresh admission using the existing bounded command protocol.
///
/// The later runtime adapter owns execution, supervision, deadline enforcement,
/// join/account-switch handling and resource accounting. Dropping a caller's
/// ticket establishes neither cancellation nor task-permit release.
pub trait AvailabilityRefreshPort: Send + Sync {
    fn submit(
        &self,
        context: CommandContext,
        request: AvailabilityDiscoveryRequest,
    ) -> CommandSubmission<AvailabilityDiscoveryOutcome<AvailabilityEventVersion>>;
}

/// Trusted per-instance monotonic clock in the command deadline's time domain.
///
/// Composition must supply monotonic `std::time::Instant` values. This seam
/// creates no timer and does not admit caller-controlled presentation clocks.
pub trait AvailabilityMonotonicClock: Send + Sync {
    fn now(&self) -> Instant;
}

struct SystemClock;

impl AvailabilityMonotonicClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Narrow orchestration over trusted admission and independently usable ports.
pub struct AvailabilityQueryService {
    admission: Arc<dyn AvailabilityLocalAdmission>,
    reader: Arc<dyn AvailabilityLocalReadPort>,
    refresh: Arc<dyn AvailabilityRefreshPort>,
    clock: Arc<dyn AvailabilityMonotonicClock>,
}

impl AvailabilityQueryService {
    #[must_use]
    pub fn new(
        admission: Arc<dyn AvailabilityLocalAdmission>,
        reader: Arc<dyn AvailabilityLocalReadPort>,
        refresh: Arc<dyn AvailabilityRefreshPort>,
    ) -> Self {
        Self::new_with_clock(admission, reader, refresh, Arc::new(SystemClock))
    }

    #[must_use]
    pub fn new_with_clock(
        admission: Arc<dyn AvailabilityLocalAdmission>,
        reader: Arc<dyn AvailabilityLocalReadPort>,
        refresh: Arc<dyn AvailabilityRefreshPort>,
        clock: Arc<dyn AvailabilityMonotonicClock>,
    ) -> Self {
        Self {
            admission,
            reader,
            refresh,
            clock,
        }
    }

    /// Reads an admitted local page and preserves its owned rows/continuation.
    ///
    /// # Errors
    ///
    /// Returns trusted admission/scope or port errors, capacity for too many
    /// rows, stale query for a different projection, and the existing cursor
    /// error for a continuation belonging to a different complete request.
    pub async fn read_page(
        &self,
        query: &ScopedAvailabilityQuery,
    ) -> Result<AvailabilityPage<AvailabilityHeadView>, SafeError> {
        validate_admission(self.admission.as_ref(), query.scope()).await?;
        let result = self.reader.read_page(query).await;
        validate_admission(self.admission.as_ref(), query.scope()).await?;
        let page = result?;
        if page.items().len() > usize::from(query.limit().rows()) {
            return Err(AvailabilityFailure::Capacity.into());
        }
        if page.projection_generation() != query.scope().context().projection_generation() {
            return Err(AvailabilityFailure::StaleQuery.into());
        }
        if let AvailabilityPageContinuation::More(cursor) = page.continuation() {
            AvailabilityPageCursor::parse(cursor.as_str(), query.fingerprint())?;
        }
        Ok(page)
    }

    /// Reads the current selected head, retaining missing/suppression evidence.
    ///
    /// # Errors
    ///
    /// Returns trusted admission/scope or port errors, or scope mismatch for
    /// a returned coordinate differing from the request, including absence.
    pub async fn read_head(
        &self,
        scope: &AvailabilityLocalQueryScope,
        coordinate: &AvailabilityListingCoordinate,
    ) -> Result<AvailabilityHeadView, SafeError> {
        validate_admission(self.admission.as_ref(), scope).await?;
        let result = self.reader.read_head(scope, coordinate).await;
        validate_admission(self.admission.as_ref(), scope).await?;
        let head = result?;
        if head.listing_coordinate() != Some(coordinate) {
            return Err(AvailabilityFailure::ScopeMismatch.into());
        }
        Ok(head)
    }

    /// Reads exact retained historical evidence without reconstructing a head.
    ///
    /// # Errors
    ///
    /// Returns trusted admission/scope or port errors, or scope mismatch for
    /// a retained version with a different coordinate or event ID.
    pub async fn read_version(
        &self,
        scope: &AvailabilityLocalQueryScope,
        coordinate: &AvailabilityListingCoordinate,
        version: AvailabilityEventVersion,
    ) -> Result<Option<AvailabilityVersionView>, SafeError> {
        validate_admission(self.admission.as_ref(), scope).await?;
        let result = self.reader.read_version(scope, coordinate, version).await;
        validate_admission(self.admission.as_ref(), scope).await?;
        let retained = result?;
        if retained.as_ref().is_some_and(|view| {
            view.listing_coordinate() != Some(coordinate) || view.version() != version
        }) {
            return Err(AvailabilityFailure::ScopeMismatch.into());
        }
        Ok(retained)
    }

    /// Admits and submits refresh independently, preserving the whole context.
    ///
    /// The initial remaining absolute deadline must fit the request's relative
    /// budget. Awaited admission never renews it. Expiry before/after admission
    /// produces a correlated timeout receipt without invoking the refresh port.
    /// This method does not wait for accepted work or automatically execute it.
    ///
    /// # Errors
    ///
    /// Returns invalid input for ID/deadline disagreement, or the trusted
    /// admission/scope error. Submission/receipt failures use the original ID.
    pub async fn submit_refresh(
        &self,
        request: AvailabilityDiscoveryRequest,
        context: CommandContext,
    ) -> Result<AvailabilityRefreshOperation, SafeError> {
        if request.request_id() != context.request_id() {
            return Err(AvailabilityFailure::InvalidInput.into());
        }
        let initial_now = self.clock.now();
        let expired_before = context.is_expired(initial_now);
        if !expired_before
            && context.deadline().duration_since(initial_now)
                > Duration::from_millis(request.deadline_millis())
        {
            return Err(AvailabilityFailure::InvalidInput.into());
        }
        validate_admission(self.admission.as_ref(), request.scope()).await?;
        let submission = if expired_before || context.is_expired(self.clock.now()) {
            CommandSubmission::Rejected(CommandReceipt::new(
                request.request_id(),
                CommandResult::TimedOut,
            ))
        } else {
            // Only this already bounded structural request is cloned. Local
            // view/page datasets are never cloned by service orchestration.
            self.refresh.submit(context, request.clone())
        };
        Ok(AvailabilityRefreshOperation::new(
            request,
            submission,
            self.admission.clone(),
        ))
    }
}

/// Original request correlation and the existing independent command ticket.
///
/// Dropping this wrapper only loses the caller's reply receiver; it does not
/// cancel queued/running work or establish release of runtime task capacity.
pub struct AvailabilityRefreshOperation {
    request: AvailabilityDiscoveryRequest,
    submission: CommandSubmission<AvailabilityDiscoveryOutcome<AvailabilityEventVersion>>,
    admission: Arc<dyn AvailabilityLocalAdmission>,
}

impl AvailabilityRefreshOperation {
    fn new(
        request: AvailabilityDiscoveryRequest,
        submission: CommandSubmission<AvailabilityDiscoveryOutcome<AvailabilityEventVersion>>,
        admission: Arc<dyn AvailabilityLocalAdmission>,
    ) -> Self {
        let submission = if submission.request_id() == request.request_id() {
            submission
        } else {
            // Reject both foreign accepted and rejected submissions now,
            // without ever awaiting a foreign ticket. Dropping that ticket
            // does not remove its envelope or cancel actual adapter work.
            drop(submission);
            CommandSubmission::Rejected(failed_receipt(
                request.request_id(),
                AvailabilityFailure::InvalidInput.into(),
            ))
        };
        Self {
            request,
            submission,
            admission,
        }
    }

    #[must_use]
    pub const fn request_id(&self) -> RequestId {
        self.request.request_id()
    }

    /// Collects only this command's receipt and rechecks trusted scope.
    ///
    /// A late authority denial/mismatch wins before publishing any completion
    /// or failure. Foreign IDs or changed outcome bindings become fixed typed
    /// failures under the original ID. Otherwise all command variants and valid
    /// partial results remain intact, even after the original effect deadline:
    /// collecting a receipt creates no work and cannot renew that deadline.
    pub async fn receipt(
        self,
    ) -> CommandReceipt<AvailabilityDiscoveryOutcome<AvailabilityEventVersion>> {
        let Self {
            request,
            submission,
            admission,
        } = self;
        let receipt = match submission {
            CommandSubmission::Accepted(ticket) => ticket.receipt().await,
            CommandSubmission::Rejected(receipt) => receipt,
        };
        if let Err(error) = validate_admission(admission.as_ref(), request.scope()).await {
            return failed_receipt(request.request_id(), error);
        }
        if receipt.request_id() != request.request_id() {
            return failed_receipt(
                request.request_id(),
                AvailabilityFailure::InvalidInput.into(),
            );
        }
        if let CommandResult::Completed(outcome) = receipt.result()
            && let Err(error) = validate_outcome_request(&request, outcome.request())
        {
            return failed_receipt(request.request_id(), error);
        }
        receipt
    }
}

async fn validate_admission(
    admission: &dyn AvailabilityLocalAdmission,
    expected: &AvailabilityLocalQueryScope,
) -> Result<(), SafeError> {
    let current = admission.current_scope().await?;
    expected.validate_current(&current).map_err(SafeError::from)
}

fn validate_outcome_request(
    expected: &AvailabilityDiscoveryRequest,
    returned: &AvailabilityDiscoveryRequest,
) -> Result<(), SafeError> {
    if expected.request_id() != returned.request_id()
        || expected.deadline_millis() != returned.deadline_millis()
    {
        return Err(AvailabilityFailure::InvalidInput.into());
    }
    expected.scope().validate_current(returned.scope())?;
    // Request/outcome constructors already retain unique bounded target sets
    // and exact counts. Set agreement does not discard either supplied order.
    if expected.targets().len() != returned.targets().len()
        || returned
            .targets()
            .iter()
            .any(|target| !expected.targets().contains(target))
    {
        return Err(AvailabilityFailure::ScopeMismatch.into());
    }
    Ok(())
}

fn failed_receipt(
    request_id: RequestId,
    error: SafeError,
) -> CommandReceipt<AvailabilityDiscoveryOutcome<AvailabilityEventVersion>> {
    CommandReceipt::new(request_id, CommandResult::Failed(error))
}
