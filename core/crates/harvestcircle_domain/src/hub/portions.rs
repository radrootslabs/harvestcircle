//! Physically observed portions and native-revision-bound population selection.
//!
//! These values are pure checked data, not current permission or future mutation
//! authority. Native storage must load the complete immutable lineage, authorize
//! the current owner and commit every physical/work/history effect atomically.

mod lineage;
mod split;
pub use lineage::{
    CapturedPhysicalScope, PhysicalLineage, PhysicalLineageNode, PhysicalScopeAnchor,
    PhysicalScopeError, ResolvedPhysicalScope,
};
pub use split::{SplitCommand, SplitPlan};

pub const HUB_LINEAGE_GROUPS: usize = super::movement::HUB_REQUIREMENT_GROUPS;
pub const HUB_PHYSICAL_SCOPES: usize = 256;
