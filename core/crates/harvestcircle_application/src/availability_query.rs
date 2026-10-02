//! Pure structural scopes and bounded owned local availability queries.
//!
//! These values need no signing capability and perform no query or external
//! effect. Runtime admission and the actual validated storage snapshot remain
//! the responsibility of the later local query adapter.

use std::fmt;

use harvestcircle_domain::{
    AvailabilityOrderKey, AvailabilityPage, AvailabilityPageContinuation, AvailabilityPageCursor,
    AvailabilityPageLimit, AvailabilityQueryContext, AvailabilityQueryError,
    AvailabilityQueryFilters, AvailabilityQueryFingerprint, PublicKey,
};

use crate::SessionGeneration;

/// Caller-supplied local owner/context/session bindings without admission proof.
#[derive(Clone, Copy, Eq, PartialEq)]
pub struct AvailabilityLocalQueryScope {
    owner: PublicKey,
    context: AvailabilityQueryContext,
    session_generation: SessionGeneration,
}

impl AvailabilityLocalQueryScope {
    #[must_use]
    pub const fn new(
        owner: PublicKey,
        context: AvailabilityQueryContext,
        session_generation: SessionGeneration,
    ) -> Self {
        Self {
            owner,
            context,
            session_generation,
        }
    }

    #[must_use]
    pub const fn owner(&self) -> PublicKey {
        self.owner
    }

    #[must_use]
    pub const fn context(&self) -> &AvailabilityQueryContext {
        &self.context
    }

    #[must_use]
    pub const fn session_generation(&self) -> SessionGeneration {
        self.session_generation
    }

    /// Compares structural bindings without admitting an account or storage.
    ///
    /// # Errors
    ///
    /// Returns `ScopeMismatch` for a different owner/context identity and
    /// `StaleQuery` for a different store/source/projection/session binding.
    pub fn validate_current(&self, current: &Self) -> Result<(), AvailabilityQueryError> {
        let original_context = self.context();
        let current_context = current.context();
        if self.owner() != current.owner()
            || original_context.context_id() != current_context.context_id()
        {
            return Err(AvailabilityQueryError::ScopeMismatch);
        }
        if original_context.store_generation() != current_context.store_generation()
            || original_context.source_revision() != current_context.source_revision()
            || original_context.projection_generation() != current_context.projection_generation()
            || self.session_generation() != current.session_generation()
        {
            return Err(AvailabilityQueryError::StaleQuery);
        }
        Ok(())
    }
}

impl fmt::Debug for AvailabilityLocalQueryScope {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AvailabilityLocalQueryScope")
            .field("source_revision", &self.context.source_revision())
            .field(
                "projection_generation",
                &self.context.projection_generation(),
            )
            .field("session_generation", &self.session_generation.value())
            .finish_non_exhaustive()
    }
}

/// Immutable complete request with an independently validated public cursor.
#[derive(Clone, Eq, PartialEq)]
pub struct ScopedAvailabilityQuery {
    scope: AvailabilityLocalQueryScope,
    filters: AvailabilityQueryFilters,
    limit: AvailabilityPageLimit,
    cursor: Option<AvailabilityPageCursor>,
    fingerprint: AvailabilityQueryFingerprint,
}

impl ScopedAvailabilityQuery {
    /// Binds the complete request and validates borrowed cursor bytes before
    /// retaining their owned representation.
    ///
    /// # Errors
    ///
    /// Returns the static cursor-contract error for an oversized, malformed or
    /// different-request cursor. This does not admit the structural local scope.
    pub fn new(
        scope: AvailabilityLocalQueryScope,
        filters: AvailabilityQueryFilters,
        limit: AvailabilityPageLimit,
        cursor: Option<&str>,
    ) -> Result<Self, AvailabilityQueryError> {
        let fingerprint = AvailabilityQueryFingerprint::new(
            scope.owner(),
            scope.context(),
            scope.session_generation().value(),
            &filters,
            limit,
        );
        let cursor = cursor
            .map(|value| AvailabilityPageCursor::parse(value, fingerprint))
            .transpose()?;
        Ok(Self {
            scope,
            filters,
            limit,
            cursor,
            fingerprint,
        })
    }

    #[must_use]
    pub const fn scope(&self) -> &AvailabilityLocalQueryScope {
        &self.scope
    }

    #[must_use]
    pub const fn filters(&self) -> &AvailabilityQueryFilters {
        &self.filters
    }

    #[must_use]
    pub const fn limit(&self) -> AvailabilityPageLimit {
        self.limit
    }

    #[must_use]
    pub const fn cursor(&self) -> Option<&AvailabilityPageCursor> {
        self.cursor.as_ref()
    }

    #[must_use]
    pub const fn fingerprint(&self) -> AvailabilityQueryFingerprint {
        self.fingerprint
    }

    /// Compares the caller's structural bindings without checking installed
    /// accounts, signing state, storage admission or performing a query.
    ///
    /// # Errors
    ///
    /// Returns `ScopeMismatch` for a different owner/context identity and
    /// `StaleQuery` for a different store/source/projection/session binding.
    pub fn validate_scope(
        &self,
        current: &AvailabilityLocalQueryScope,
    ) -> Result<(), AvailabilityQueryError> {
        self.scope.validate_current(current)
    }

    /// Moves bounded rows into a page at this query's exact projection.
    ///
    /// The caller must supply actual ordered selected rows and the correct last
    /// position for its validated snapshot. This method neither scans nor
    /// derives a continuation from item payloads. `End` describes only this
    /// local query snapshot, without network freshness/completeness guarantees.
    ///
    /// # Errors
    ///
    /// Returns `Capacity` if the supplied row count exceeds this request's limit.
    pub fn page<T>(
        &self,
        items: Vec<T>,
        next: Option<AvailabilityOrderKey>,
    ) -> Result<AvailabilityPage<T>, AvailabilityQueryError> {
        if items.len() > usize::from(self.limit.rows()) {
            return Err(AvailabilityQueryError::Capacity);
        }
        let continuation = match next {
            None => AvailabilityPageContinuation::End,
            Some(after) => AvailabilityPageContinuation::More(AvailabilityPageCursor::encode(
                self.fingerprint,
                after,
            )),
        };
        AvailabilityPage::new(
            self.limit,
            items,
            continuation,
            self.scope.context().projection_generation(),
        )
    }
}

impl fmt::Debug for ScopedAvailabilityQuery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScopedAvailabilityQuery")
            .field("scope", &self.scope)
            .field("filters", &self.filters)
            .field("limit", &self.limit)
            .field("cursor_present", &self.cursor.is_some())
            .finish_non_exhaustive()
    }
}
