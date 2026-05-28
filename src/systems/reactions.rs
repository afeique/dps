//! Elemental synergy reactions (Phase E4b) — resolves the [`PendingReactions`]
//! queue that `collision::bullet_hits_enemy` fills, applying SHATTER and OIL
//! FLARE AoE (ported from `_triggerStatusReactions`, collision-system.js:2629).
//!
//! Runs in the collision group after `bullet_hits_enemy` (so its `Damage` is
//! consumed by `apply_damage` the same tick). The shatter chain is resolved with
//! a local worklist: an *already-frozen* neighbor that gets caught in a shatter
//! re-shatters at `depth + 1`, capped at [`MAX_SHATTER_DEPTH`]. Neighbors frozen
//! *by* this tick's shatter (via deferred `Commands`) are not yet `Frozen` in the
//! query, so they don't chain this tick — matching the JS ordering.

use crate::combat::element::{Element, Resistances};
use crate::combat::reaction::{
    PendingReactions, ReactionSeed, FLARE_BURN_SECS, FLARE_RADIUS, MAX_SHATTER_DEPTH,
    SHATTER_DAMAGE, SHATTER_RADIUS, SHATTER_REFREEZE_SECS,
};
use crate::components::{Burning, Enemy, Frozen, Ship};
use crate::messages::{Damage, Reaction, ReactionFx};
use bevy::prelude::*;

pub fn resolve_reactions(
    mut commands: Commands,
    mut pending: ResMut<PendingReactions>,
    mut dmg: MessageWriter<Damage>,
    mut fx: MessageWriter<Reaction>,
    enemies: Query<
        (Entity, &Transform, Option<&Resistances>, Has<Frozen>),
        (With<Enemy>, Without<Ship>),
    >,
) {
    if pending.0.is_empty() {
        return;
    }
    // Drain into a worklist; the shatter chain re-queues into it (bounded by
    // depth < MAX so it terminates). Query data is stable across the run since
    // our inserts are deferred Commands.
    let mut work: Vec<ReactionSeed> = std::mem::take(&mut pending.0);
    while let Some(seed) = work.pop() {
        match seed {
            ReactionSeed::Shatter {
                source,
                center,
                depth,
            } => {
                if depth >= MAX_SHATTER_DEPTH {
                    continue;
                }
                fx.write(Reaction {
                    center,
                    kind: ReactionFx::Shatter,
                });
                let r2 = SHATTER_RADIUS * SHATTER_RADIUS;
                for (e, tf, res, frozen) in &enemies {
                    if e == source {
                        continue; // the cracked enemy isn't in its own AoE
                    }
                    let p = tf.translation.truncate();
                    if p.distance_squared(center) > r2 {
                        continue;
                    }
                    // SHATTER_DAMAGE is CRYO → scaled by the neighbor's cryo resist.
                    let cryo = res.map_or(1.0, |r| r.multiplier(Element::Cryo));
                    dmg.write(Damage {
                        target: e,
                        amount: SHATTER_DAMAGE * cryo,
                    });
                    commands.entity(e).insert(Frozen {
                        secs: SHATTER_REFREEZE_SECS,
                    });
                    // Only *already*-frozen neighbors chain (8 ≥ threshold).
                    if frozen {
                        work.push(ReactionSeed::Shatter {
                            source: e,
                            center: p,
                            depth: depth + 1,
                        });
                    }
                }
            }
            ReactionSeed::Flare { center, damage } => {
                fx.write(Reaction {
                    center,
                    kind: ReactionFx::Flare,
                });
                let r2 = FLARE_RADIUS * FLARE_RADIUS;
                for (e, tf, _res, _frozen) in &enemies {
                    if tf.translation.truncate().distance_squared(center) > r2 {
                        continue;
                    }
                    commands.entity(e).insert(Burning {
                        dps: damage,
                        secs: FLARE_BURN_SECS,
                    });
                }
            }
        }
    }
}
