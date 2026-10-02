//! Public availability references kept separate from local account authority.

pub mod identity;
pub mod projection;
pub mod query;

pub use identity::{AvailabilityEventVersion, AvailabilityListingCoordinate, PublicPublisher};
pub use projection::{
    AvailabilityHeadState, AvailabilityHeadView, AvailabilityObservation,
    AvailabilityUnsupportedReason, AvailabilityVersionView,
};
pub use query::{
    AVAILABILITY_CURSOR_MAX_BYTES, AVAILABILITY_PAGE_DEFAULT_ROWS, AVAILABILITY_PAGE_MAX_ROWS,
    AVAILABILITY_QUERY_TEXT_MAX_BYTES, AvailabilityOrderKey, AvailabilityPage,
    AvailabilityPageContinuation, AvailabilityPageCursor, AvailabilityPageLimit,
    AvailabilityQueryContext, AvailabilityQueryError, AvailabilityQueryFilters,
    AvailabilityQueryFingerprint, AvailabilitySearchText,
};
