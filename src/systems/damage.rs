//! Damage application. Consumes `Damage`, subtracts HP, and on death emits a
//! `Death` message + despawns the entity. Branches on player vs. enemy:
//!   • Enemy death  — increments `Score::kills` and despawns.
//!   • Player death — emits `Death`, flips state to `GameOver`, and despawns
//!                    the ship (kills are NOT incremented for the player).
//! `Invulnerable` i-frames gate repeated hits: any entity carrying the
//! component absorbs no damage until `tick_invulnerability` removes it.
//!
//! Phase 3 wires `Death` to drops, and explosion FX.

use crate::components::{Health, Invulnerable, Ship};
use crate::messages::{Damage, Death};
use crate::resources::Score;
use crate::states::GameState;
use bevy::prelude::*;

pub fn apply_damage(
    mut commands: Commands,
    mut dmg: MessageReader<Damage>,
    mut deaths: MessageWriter<Death>,
    mut score: ResMut<Score>,
    mut next_state: ResMut<NextState<GameState>>,
    mut q: Query<(&mut Health, Has<Ship>, Has<Invulnerable>)>,
) {
    for ev in dmg.read() {
        let Ok((mut hp, is_player, invulnerable)) = q.get_mut(ev.target) else {
            continue; // target already gone this tick
        };
        if invulnerable {
            continue; // i-frames active — eat the hit silently
        }
        hp.current -= ev.amount;
        if hp.current <= 0.0 {
            deaths.write(Death { entity: ev.target });
            if is_player {
                next_state.set(GameState::GameOver);
                commands.entity(ev.target).despawn();
                // do NOT increment kills — the player is not a kill
            } else {
                score.kills += 1;
                commands.entity(ev.target).despawn();
            }
        } else if is_player {
            // Grant i-frames so the next hit doesn't land immediately.
            // Note: `insert` is a deferred command, so multiple `Damage`
            // messages aimed at the player within the SAME tick can all land
            // before i-frames take effect — acceptable for the Phase 1 slice.
            commands
                .entity(ev.target)
                .insert(Invulnerable { seconds: 0.6 });
        }
    }
}

pub fn tick_invulnerability(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut Invulnerable)>,
) {
    for (e, mut inv) in &mut q {
        inv.seconds -= time.delta_secs();
        if inv.seconds <= 0.0 {
            commands.entity(e).remove::<Invulnerable>();
        }
    }
}
