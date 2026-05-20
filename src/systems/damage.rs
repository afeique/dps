//! Damage application. Consumes `Damage`, subtracts HP, and on death emits a
//! `Death` message + despawns the entity. Phase 3 wires `Death` to drops,
//! score, and explosion FX; Phase 1 just removes the entity (and bumps kills).

use crate::components::Health;
use crate::messages::{Damage, Death};
use crate::resources::Score;
use bevy::prelude::*;

pub fn apply_damage(
    mut commands: Commands,
    mut dmg: MessageReader<Damage>,
    mut deaths: MessageWriter<Death>,
    mut score: ResMut<Score>,
    mut q: Query<&mut Health>,
) {
    for ev in dmg.read() {
        let Ok(mut hp) = q.get_mut(ev.target) else {
            continue; // target already gone this tick
        };
        hp.current -= ev.amount;
        if hp.current <= 0.0 {
            deaths.write(Death { entity: ev.target });
            score.kills += 1;
            commands.entity(ev.target).despawn();
        }
    }
}
