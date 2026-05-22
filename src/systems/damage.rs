//! Damage application. Consumes `Damage`, subtracts HP, and on death emits a
//! `Death` message + despawns the entity. Branches on player vs. enemy:
//!   • Enemy death  — increments `Score::kills` and despawns.
//!   • Player death — emits `Death`, flips state to `GameOver`, and despawns
//!                    the ship (kills are NOT incremented for the player).
//! `Invulnerable` i-frames still gate damage while present (any entity carrying
//! the component absorbs no damage until `tick_invulnerability` removes it), but
//! per spec II.2 they are **no longer auto-granted on a hit** — only deliberate
//! skills (dash tail, shield-burst) grant them. The player's shield is a flat
//! damage-reduction %, and a lethal hit consumes a spare health tank (refill in
//! place, no invuln) before ending the run.
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
        Option<&Shield>,
        Option<&mut Lives>,
        Has<Ship>,
        Has<Invulnerable>,
    )>,
) {
    for ev in dmg.read() {
        let Ok((mut hp, tf, shield, mut lives, is_player, invulnerable)) = q.get_mut(ev.target)
        else {
            continue; // target already gone this tick
        };
        if invulnerable {
            continue; // deliberate i-frames (dash / shield-burst) — eat the hit
        }

        // Any landed hit on the player breaks the kill streak (spec III.6).
        if is_player {
            streak.break_streak();
        }

        // Shield = flat % damage reduction (player only); the JS pipeline rounds
        // the final player damage. Enemy damage stays fractional so DoT sources
        // (beams) accumulate exactly across ticks.
        let mut amount = ev.amount;
        if let Some(s) = shield {
            amount *= 1.0 - s.reduction;
        }
        if is_player {
            amount = amount.round();
        }
        hp.current -= amount;

        if hp.current <= 0.0 {
            let position = tf.translation.truncate();
            if is_player {
                // Spare health tank? Refill HP *in place* — no respawn delay and
                // NO invulnerability (spec II.2) — instead of ending the run.
                if let Some(l) = lives.as_mut().filter(|l| l.count > 0) {
                    l.count -= 1;
                    hp.current = hp.max;
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
        }
        // No post-hit invulnerability grace (spec II.2: removed in JS 5.88.0).
        // Sustained contact is gated by the physical separation/bounce in
        // `collision::enemy_contact_player`, not by i-frames.
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
