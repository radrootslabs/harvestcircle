//! Bounded caller-supplied discovery requests, progress and owned valid results.
//!
//! These immutable values perform no I/O, admission, event validation, deadline
//! enforcement or resource reservation. Their counters describe supplied work;
//! they do not establish exhaustive network knowledge or current local storage.

use std::fmt::{self, Write};

use harvestcircle_domain::SafeError;
use harvestcircle_domain::error::AvailabilityFailure;
pub use radroots_transport::outcome::FetchTargetState;
pub use radroots_transport::target::TargetFingerprint;

use crate::{AvailabilityLocalQueryScope, RequestId};

pub const MAX_DISCOVERY_TARGETS: usize = 16;
pub const MAX_DISCOVERY_FETCH_CALLS: u8 = 4;
pub const DISCOVERY_FETCH_RAW_RESERVATION_BYTES: u64 = 8_388_608;
pub const MAX_DISCOVERY_RETURNED_PER_TARGET: u16 = 64;
pub const MAX_DISCOVERY_RETURNED_EVENTS: u16 = 1_024;
pub const MAX_DISCOVERY_METADATA_BYTES: usize = 16_384;

// Fixed keys, finite labels and bounded decimals fit in 384 envelope bytes.
// Each target's 64 hex bytes and three progress objects fit in 384 bytes,
// including its separator. The complete structural bound is therefore 6,528.
const METADATA_ENVELOPE_MAX_BYTES: usize = 384;
const METADATA_TARGET_MAX_BYTES: usize = 384;
const _: () = assert!(
    METADATA_ENVELOPE_MAX_BYTES + MAX_DISCOVERY_TARGETS * METADATA_TARGET_MAX_BYTES
        <= MAX_DISCOVERY_METADATA_BYTES
);

/// Structural source selection and local bindings without effect authority.
#[derive(Clone, Eq, PartialEq)]
pub struct AvailabilityDiscoveryRequest {
    request_id: RequestId,
    scope: AvailabilityLocalQueryScope,
    targets: Vec<TargetFingerprint>,
    deadline_millis: u64,
}

impl AvailabilityDiscoveryRequest {
    /// Retains the original bounded unique target vector and structural scope.
    ///
    /// # Errors
    ///
    /// Returns capacity for more than sixteen targets, or invalid input for
    /// duplicate canonical fingerprints or a deadline outside 1..=30,000 ms.
    pub fn new(
        request_id: RequestId,
        scope: AvailabilityLocalQueryScope,
        targets: Vec<TargetFingerprint>,
        deadline_millis: u64,
    ) -> Result<Self, SafeError> {
        if targets.len() > MAX_DISCOVERY_TARGETS {
            return Err(AvailabilityFailure::Capacity.into());
        }
        if targets
            .iter()
            .enumerate()
            .any(|(index, target)| targets[..index].contains(target))
            || !(1..=30_000).contains(&deadline_millis)
        {
            return Err(AvailabilityFailure::InvalidInput.into());
        }
        Ok(Self {
            request_id,
            scope,
            targets,
            deadline_millis,
        })
    }

    #[must_use]
    pub const fn request_id(&self) -> RequestId {
        self.request_id
    }

    #[must_use]
    pub const fn scope(&self) -> &AvailabilityLocalQueryScope {
        &self.scope
    }

    #[must_use]
    pub fn targets(&self) -> &[TargetFingerprint] {
        &self.targets
    }

    #[must_use]
    pub const fn deadline_millis(&self) -> u64 {
        self.deadline_millis
    }
}

impl fmt::Debug for AvailabilityDiscoveryRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilityDiscoveryRequest")
            .field("selected_targets", &self.targets.len())
            .field("deadline_millis", &self.deadline_millis)
            .finish_non_exhaustive()
    }
}

/// Exact shared target state, with an explicit absence of requested work.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvailabilityDiscoveryState {
    NotRequested,
    Requested(FetchTargetState),
}

impl AvailabilityDiscoveryState {
    const fn metadata_label(self) -> &'static str {
        match self {
            Self::NotRequested => "not_requested",
            Self::Requested(FetchTargetState::Complete) => "complete",
            Self::Requested(FetchTargetState::Partial) => "partial",
            Self::Requested(FetchTargetState::Unavailable) => "unavailable",
            Self::Requested(FetchTargetState::FailedRetryable) => "failed_retryable",
            Self::Requested(FetchTargetState::FailedTerminal) => "failed_terminal",
            Self::Requested(FetchTargetState::Cancelled) => "cancelled",
        }
    }
}

/// Finite target-local stopping information without arbitrary diagnostic text.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AvailabilityDiscoveryStopReason {
    None,
    BudgetExhausted,
    DeadlineExpired,
    Stopped,
}

impl AvailabilityDiscoveryStopReason {
    const fn metadata_label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::BudgetExhausted => "budget_exhausted",
            Self::DeadlineExpired => "deadline_expired",
            Self::Stopped => "stopped",
        }
    }
}

/// One stream's consistent target-local facts, including earlier results.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvailabilityDiscoveryProgress {
    state: AvailabilityDiscoveryState,
    reason: AvailabilityDiscoveryStopReason,
    returned: u16,
}

impl AvailabilityDiscoveryProgress {
    /// Preserves complete-empty, interrupted and unrequested distinctions.
    ///
    /// # Errors
    ///
    /// Returns capacity for more than sixty-four returned events, or invalid
    /// input for a reason/count inconsistent with the supplied stream state.
    pub fn new(
        state: AvailabilityDiscoveryState,
        reason: AvailabilityDiscoveryStopReason,
        returned: u16,
    ) -> Result<Self, SafeError> {
        if returned > MAX_DISCOVERY_RETURNED_PER_TARGET {
            return Err(AvailabilityFailure::Capacity.into());
        }
        let valid = match state {
            AvailabilityDiscoveryState::NotRequested
            | AvailabilityDiscoveryState::Requested(FetchTargetState::Unavailable) => {
                reason == AvailabilityDiscoveryStopReason::None && returned == 0
            }
            AvailabilityDiscoveryState::Requested(
                FetchTargetState::Complete
                | FetchTargetState::FailedRetryable
                | FetchTargetState::FailedTerminal,
            ) => reason == AvailabilityDiscoveryStopReason::None,
            AvailabilityDiscoveryState::Requested(FetchTargetState::Partial) => {
                matches!(
                    reason,
                    AvailabilityDiscoveryStopReason::None
                        | AvailabilityDiscoveryStopReason::BudgetExhausted
                        | AvailabilityDiscoveryStopReason::DeadlineExpired
                )
            }
            AvailabilityDiscoveryState::Requested(FetchTargetState::Cancelled) => {
                reason == AvailabilityDiscoveryStopReason::Stopped
            }
        };
        if !valid {
            return Err(AvailabilityFailure::InvalidInput.into());
        }
        Ok(Self {
            state,
            reason,
            returned,
        })
    }

    #[must_use]
    pub const fn state(&self) -> AvailabilityDiscoveryState {
        self.state
    }

    #[must_use]
    pub const fn reason(&self) -> AvailabilityDiscoveryStopReason {
        self.reason
    }

    #[must_use]
    pub const fn returned(&self) -> u16 {
        self.returned
    }

    fn append_metadata(self, json: &mut String) {
        write!(
            json,
            "{{\"state\":\"{}\",\"reason\":\"{}\",\"returned\":{}}}",
            self.state.metadata_label(),
            self.reason.metadata_label(),
            self.returned
        )
        .expect("writing finite metadata to String cannot fail");
    }
}

/// Three independent streams sharing one sixty-four-event target allowance.
#[derive(Clone, Eq, PartialEq)]
pub struct AvailabilityDiscoveryTargetOutcome {
    target: TargetFingerprint,
    listings: AvailabilityDiscoveryProgress,
    profiles: AvailabilityDiscoveryProgress,
    deletions: AvailabilityDiscoveryProgress,
}

impl AvailabilityDiscoveryTargetOutcome {
    /// Retains admitted progress only after checking the shared target sum.
    ///
    /// # Errors
    ///
    /// Returns capacity if the three returned counts together exceed sixty-four.
    pub fn new(
        target: TargetFingerprint,
        listings: AvailabilityDiscoveryProgress,
        profiles: AvailabilityDiscoveryProgress,
        deletions: AvailabilityDiscoveryProgress,
    ) -> Result<Self, SafeError> {
        if listings.returned() + profiles.returned() + deletions.returned()
            > MAX_DISCOVERY_RETURNED_PER_TARGET
        {
            return Err(AvailabilityFailure::Capacity.into());
        }
        Ok(Self {
            target,
            listings,
            profiles,
            deletions,
        })
    }

    #[must_use]
    pub const fn target(&self) -> &TargetFingerprint {
        &self.target
    }

    #[must_use]
    pub const fn listings(&self) -> AvailabilityDiscoveryProgress {
        self.listings
    }

    #[must_use]
    pub const fn profiles(&self) -> AvailabilityDiscoveryProgress {
        self.profiles
    }

    #[must_use]
    pub const fn deletions(&self) -> AvailabilityDiscoveryProgress {
        self.deletions
    }

    const fn returned(&self) -> u16 {
        self.listings.returned() + self.profiles.returned() + self.deletions.returned()
    }

    fn has_complete_stream(&self) -> bool {
        [self.listings, self.profiles, self.deletions]
            .iter()
            .any(|progress| {
                progress.state()
                    == AvailabilityDiscoveryState::Requested(FetchTargetState::Complete)
            })
    }
}

impl fmt::Debug for AvailabilityDiscoveryTargetOutcome {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilityDiscoveryTargetOutcome")
            .field("listings", &self.listings)
            .field("profiles", &self.profiles)
            .field("deletions", &self.deletions)
            .finish_non_exhaustive()
    }
}

/// Bounded supplied operation accounting without actual transport reservation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AvailabilityDiscoveryUsage {
    fetch_calls: u8,
    returned_events: u16,
}

impl AvailabilityDiscoveryUsage {
    /// Admits at most four fetch calls and 1,024 aggregate returned events.
    ///
    /// # Errors
    ///
    /// Returns capacity for either exceeded bound, or invalid input for returned
    /// events with zero supplied calls. Reported reservations are never refunded.
    pub fn new(fetch_calls: u8, returned_events: u16) -> Result<Self, SafeError> {
        if fetch_calls > MAX_DISCOVERY_FETCH_CALLS
            || returned_events > MAX_DISCOVERY_RETURNED_EVENTS
        {
            return Err(AvailabilityFailure::Capacity.into());
        }
        if fetch_calls == 0 && returned_events != 0 {
            return Err(AvailabilityFailure::InvalidInput.into());
        }
        Ok(Self {
            fetch_calls,
            returned_events,
        })
    }

    #[must_use]
    pub const fn fetch_calls(&self) -> u8 {
        self.fetch_calls
    }

    #[must_use]
    pub const fn returned_events(&self) -> u16 {
        self.returned_events
    }

    #[must_use]
    pub const fn reserved_raw_bytes(&self) -> u64 {
        self.fetch_calls as u64 * DISCOVERY_FETCH_RAW_RESERVATION_BYTES
    }
}

/// Original owned valid items retained across independently interrupted targets.
///
/// Item admission remains the caller's responsibility. No item serialization,
/// event evidence, freshness or persistence is inferred from these values.
pub struct AvailabilityDiscoveryOutcome<T> {
    request: AvailabilityDiscoveryRequest,
    usage: AvailabilityDiscoveryUsage,
    targets: Vec<AvailabilityDiscoveryTargetOutcome>,
    items: Vec<T>,
}

impl<T> AvailabilityDiscoveryOutcome<T> {
    /// Binds exactly one outcome to each selected target without cloning items.
    ///
    /// # Errors
    ///
    /// Returns capacity for oversized vectors, scope mismatch for a different
    /// target set, or invalid input for duplicate outcomes, inconsistent counts,
    /// completion without a call, or fabricated work for an empty selection.
    pub fn new(
        request: AvailabilityDiscoveryRequest,
        usage: AvailabilityDiscoveryUsage,
        targets: Vec<AvailabilityDiscoveryTargetOutcome>,
        items: Vec<T>,
    ) -> Result<Self, SafeError> {
        if targets.len() > MAX_DISCOVERY_TARGETS
            || items.len() > usize::from(MAX_DISCOVERY_RETURNED_EVENTS)
        {
            return Err(AvailabilityFailure::Capacity.into());
        }
        if targets.iter().enumerate().any(|(index, target)| {
            targets[..index]
                .iter()
                .any(|previous| previous.target() == target.target())
        }) {
            return Err(AvailabilityFailure::InvalidInput.into());
        }
        if targets.len() != request.targets().len()
            || targets
                .iter()
                .any(|target| !request.targets().contains(target.target()))
        {
            return Err(AvailabilityFailure::ScopeMismatch.into());
        }
        // At most sixteen independently admitted sums of at most sixty-four.
        let returned: u16 = targets
            .iter()
            .map(AvailabilityDiscoveryTargetOutcome::returned)
            .sum();
        if returned != usage.returned_events()
            || items.len() > usize::from(returned)
            || (usage.fetch_calls() == 0
                && targets
                    .iter()
                    .any(AvailabilityDiscoveryTargetOutcome::has_complete_stream))
            || (request.targets().is_empty() && usage.fetch_calls() != 0)
        {
            return Err(AvailabilityFailure::InvalidInput.into());
        }
        Ok(Self {
            request,
            usage,
            targets,
            items,
        })
    }

    #[must_use]
    pub const fn request(&self) -> &AvailabilityDiscoveryRequest {
        &self.request
    }

    #[must_use]
    pub const fn usage(&self) -> AvailabilityDiscoveryUsage {
        self.usage
    }

    #[must_use]
    pub fn targets(&self) -> &[AvailabilityDiscoveryTargetOutcome] {
        &self.targets
    }

    #[must_use]
    pub fn items(&self) -> &[T] {
        &self.items
    }

    #[must_use]
    pub fn into_items(self) -> Vec<T> {
        self.items
    }

    #[must_use]
    pub fn listings_state(&self) -> AvailabilityDiscoveryState {
        self.stream_state(AvailabilityDiscoveryTargetOutcome::listings)
    }

    #[must_use]
    pub fn profiles_state(&self) -> AvailabilityDiscoveryState {
        self.stream_state(AvailabilityDiscoveryTargetOutcome::profiles)
    }

    #[must_use]
    pub fn deletions_state(&self) -> AvailabilityDiscoveryState {
        self.stream_state(AvailabilityDiscoveryTargetOutcome::deletions)
    }

    fn stream_state(
        &self,
        progress: fn(&AvailabilityDiscoveryTargetOutcome) -> AvailabilityDiscoveryProgress,
    ) -> AvailabilityDiscoveryState {
        let Some(first) = self.targets.first() else {
            return AvailabilityDiscoveryState::NotRequested;
        };
        let state = progress(first).state();
        if self
            .targets
            .iter()
            .all(|target| progress(target).state() == state)
        {
            state
        } else {
            AvailabilityDiscoveryState::Requested(FetchTargetState::Partial)
        }
    }

    /// Serializes only finite metadata, preserving the supplied target order.
    ///
    /// The fixed envelope and sixteen bounded target objects fit within 6,528
    /// ASCII bytes, below the 16 KiB contract. Only shared admitted fingerprints,
    /// finite labels and bounded decimals enter JSON; items and scope do not.
    #[must_use]
    pub fn metadata_json(&self) -> String {
        let mut json = String::with_capacity(
            METADATA_ENVELOPE_MAX_BYTES + self.targets.len() * METADATA_TARGET_MAX_BYTES,
        );
        write!(
            json,
            concat!(
                "{{\"version\":1,\"fetch_calls\":{},\"reserved_raw_bytes\":{},",
                "\"returned_events\":{},\"retained_items\":{},\"listings\":\"{}\",",
                "\"profiles\":\"{}\",\"deletions\":\"{}\",\"targets\":["
            ),
            self.usage.fetch_calls(),
            self.usage.reserved_raw_bytes(),
            self.usage.returned_events(),
            self.items.len(),
            self.listings_state().metadata_label(),
            self.profiles_state().metadata_label(),
            self.deletions_state().metadata_label()
        )
        .expect("writing finite metadata to String cannot fail");
        for (index, target) in self.targets.iter().enumerate() {
            if index != 0 {
                json.push(',');
            }
            json.push_str("{\"target\":\"");
            json.push_str(target.target().as_str());
            json.push_str("\",\"listings\":");
            target.listings().append_metadata(&mut json);
            json.push_str(",\"profiles\":");
            target.profiles().append_metadata(&mut json);
            json.push_str(",\"deletions\":");
            target.deletions().append_metadata(&mut json);
            json.push('}');
        }
        json.push_str("]}");
        json
    }
}

impl<T> fmt::Debug for AvailabilityDiscoveryOutcome<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilityDiscoveryOutcome")
            .field("fetch_calls", &self.usage.fetch_calls())
            .field("reserved_raw_bytes", &self.usage.reserved_raw_bytes())
            .field("returned_events", &self.usage.returned_events())
            .field("retained_items", &self.items.len())
            .field("selected_targets", &self.targets.len())
            .field("listings", &self.listings_state())
            .field("profiles", &self.profiles_state())
            .field("deletions", &self.deletions_state())
            .finish_non_exhaustive()
    }
}
