//! Combat data + math ported from `js/modules/combat/*`. Pure, engine-agnostic
//! helpers + the components the damage pipeline reads. This is the start of the
//! v6.161 element/resistance/attunement overhaul — see
//! `docs/roguelite-port-plan.md` Phase E. Submodules land incrementally:
//! `element` (E1, taxonomy + resist math) first; statuses, reactions, weapon
//! data, attunements, and passives follow.

pub mod element;

// Convenience re-export for the damage pipeline (E2/E5) + weapon data (W1) that
// read these next; unused until then, so the foundation-only E1 commit is clean.
#[allow(unused_imports)]
pub use element::*;
