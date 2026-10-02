//! Public availability references kept separate from local account authority.

pub mod identity;
pub mod projection;

pub use identity::{AvailabilityEventVersion, AvailabilityListingCoordinate, PublicPublisher};
pub use projection::{
    AvailabilityHeadState, AvailabilityHeadView, AvailabilityObservation,
    AvailabilityUnsupportedReason, AvailabilityVersionView,
};
