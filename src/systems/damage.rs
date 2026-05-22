//! Damage application. Consumes `Damage`, subtracts HP, and on death emits a
//! `Death` message + despawns the entity. Branches on player vs. enemy:
//!   • Enemy death  — increments `Score::kills` and despawns.
//!   • Player death — emits `Death`, flips state to `GameOver`, and despawns
//!                    the ship (kills are NOT incremented for the player).
//! `Invulnerable` i-frames gate repeated hits: any entity carrying the
//! component absorbs no damage until `tick_invulnerability` removes it.
//!
//! Phase 3 wires `Death` to drops, and explosion FX.

use crate::components::{Health, Invulnerable, Lives, Shield, Ship};
use crate::messages::{Damage, Death};
use crate::resources::{KillStreak, Score};
use crate::states::GameState;
use bevy::prelude::*;

pub fn apply_damage(
    mut commands: Commands,
    mut dmg: MessageReader<Damage>,
    mut deaths: MessageWriter<Death>,
    mut score: ResMut<Score>,
    mut streak: ResMut<KillStreak>,
    mut next_state: ResMut<NextState<GameState>>,
    mut q: Query<(
        &mut Health,
        &Transform,
        Option<&mut Shield>,
        Option<&mut Lives>,
        Has<Ship>,
        Has<Invulnerable>,
    )>,
) {
    for ev in dmg.read() {
        let Ok((mut hp, tf, mut shield, mut lives, is_player, invulnerable)) = q.get_mut(ev.target)
        else {
            continue; // target already gone this tick
        };
        if invulnerable {
            continue; // i-frames active — eat the hit silently
        }

        // Any landed hit on the player breaks the kill streak (spec III.6).
        if is_player {
            streak.break_streak();
        }

        // Shield absorbs first; the remainder hits Health.
        let mut remaining = ev.amount;
        if let Some(s) = shield.as_mut() {
            let absorbed = remaining.min(s.current);
            s.current -= absorbed;
            remaining -= absorbed;
        }
        hp.current -= remaining;

        if hp.current <= 0.0 {
            let position = tf.translation.truncate();
            if is_player {
                // Spare ship? Respawn in place (refill HP + shield, longer
                // i-frames) instead of ending the run.
                if let Some(l) = lives.as_mut().filter(|l| l.count > 0) {
                    l.count -= 1;
                    hp.current = hp.max;
                    if let Some(s) = shield.as_mut() {
                        s.current = s.max;
                    }
                    commands
                        .entity(ev.target)
                        .insert(Invulnerable { seconds: 1.5 });
                    continue;
                }
                deaths.write(Death {
                    entity: ev.target,
                    position,
                });
                next_state.set(GameState::GameOver);
                commands.entity(ev.target).despawn();
            } else {
                deaths.write(Death {
                    entity: ev.target,
                    position,
                });
                score.kills += 1;
                streak.on_kill();
                commands.entity(ev.target).despawn();
            }
        } else if is_player {
            // Grant i-frames so the next hit doesn't land immediately. The
            // deferred `insert` means several same-tick hits can still land —
            // acceptable.
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

/// Wind down the kill-streak buff window. The streak *count* persists (it only
/// resets when the player takes damage); once this timer lapses the multiplier
/// reverts to 1.0 until the next kill (spec III.6).
pub fn tick_streak(time: Res<Time>, mut streak: ResMut<KillStreak>) {
    if streak.timer > 0.0 {
        streak.timer = (streak.timer - time.delta_secs()).max(0.0);
    }
}
