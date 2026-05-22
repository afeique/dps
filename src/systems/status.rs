//! Status effects on enemies (spec III.3/III.6): `Burning` damage-over-time and
//! `Stunned` fire-suppression. Both tick down in `FixedUpdate` and remove
//! themselves on expiry. Burn emits `Damage` (so it must run in the collision
//! group, before `apply_damage`); stun is read by `enemy::firing` (stunned
//! enemies skip firing).

use crate::components::{Burning, Stunned};
use crate::messages::Damage;
use bevy::prelude::*;

/// Apply each `Burning`'s `dps × dt` as `Damage` and expire it.
pub fn tick_burning(
    time: Res<Time>,
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    mut q: Query<(Entity, &mut Burning)>,
) {
    let dt = time.delta_secs();
    for (e, mut burn) in &mut q {
        dmg.write(Damage {
            target: e,
            amount: burn.dps * dt,
        });
        burn.secs -= dt;
        if burn.secs <= 0.0 {
            commands.entity(e).remove::<Burning>();
        }
    }
}

/// Count down each `Stunned` and remove it on expiry.
pub fn tick_stun(time: Res<Time>, mut commands: Commands, mut q: Query<(Entity, &mut Stunned)>) {
    let dt = time.delta_secs();
    for (e, mut stun) in &mut q {
        stun.secs -= dt;
        if stun.secs <= 0.0 {
            commands.entity(e).remove::<Stunned>();
        }
    }
}
