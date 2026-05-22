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

use crate::components::{
    Boss, Bulwark, Enemy, Health, Invulnerable, Lives, Shield, Ship, BULWARK_RESIST,
};
use crate::messages::{Damage, Death, PlayerHurt};
use crate::resources::{GameRng, KillStreak, Score};
use crate::states::GameState;
use crate::systems::shop::{dodge_chance, thorns_frac, UpgradeId, Upgrades};
use bevy::prelude::*;

pub fn apply_damage(
    mut commands: Commands,
    mut dmg: MessageReader<Damage>,
    mut deaths: MessageWriter<Death>,
    mut hurt: MessageWriter<PlayerHurt>,
    mut score: ResMut<Score>,
    mut streak: ResMut<KillStreak>,
    mut rng: ResMut<GameRng>,
    upgrades: Res<Upgrades>,
    mut next_state: ResMut<NextState<GameState>>,
    mut q: Query<(
        &mut Health,
        &Transform,
        Option<&Shield>,
        Option<&mut Lives>,
        Has<Ship>,
        Has<Invulnerable>,
        Option<&Enemy>,
        Option<&Boss>,
        Option<&Bulwark>,
    )>,
) {
    for ev in dmg.read() {
        let Ok((mut hp, tf, shield, mut lives, is_player, invulnerable, enemy, boss, bulwark)) =
            q.get_mut(ev.target)
        else {
            continue; // target already gone this tick
        };
        if invulnerable {
            continue; // deliberate i-frames (dash / shield-burst) — eat the hit
        }

        if is_player {
            // DODGE: chance to ignore the hit entirely — no damage, no streak
            // break (spec II.2 step 3 / III.5).
            let dodge = dodge_chance(upgrades.owned(UpgradeId::Dodge));
            if dodge > 0.0 && rng.next_f32() < dodge {
                continue;
            }
            // Any landed hit on the player breaks the kill streak (spec III.6).
            streak.break_streak();
        }

        // Shield = flat % damage reduction (player only); the JS pipeline rounds
        // the final player damage. Enemy damage stays fractional so DoT sources
        // (beams) accumulate exactly across ticks.
        let mut amount = ev.amount;
        if let Some(s) = shield {
            amount *= 1.0 - s.reduction;
        }
        // BULWARK halves what's left (spec II.2 step 7); player-only component.
        if bulwark.is_some() {
            amount *= 1.0 - BULWARK_RESIST;
        }
        if is_player {
            amount = amount.round();
        }
        hp.current -= amount;

        // Announce the landed player damage (drives THORNS in `apply_thorns`).
        if is_player && amount > 0.0 {
            hurt.write(PlayerHurt { amount });
        }

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
                    kind: None,
                    boss_tier: 0,
                });
                next_state.set(GameState::GameOver);
                commands.entity(ev.target).despawn();
            } else {
                deaths.write(Death {
                    entity: ev.target,
                    position,
                    kind: enemy.map(|e| e.kind),
                    boss_tier: boss.map_or(0, |b| b.tier),
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

/// THORNS passive (spec III.5): reflect `0.25 × stacks` of each landed player
/// hit (`PlayerHurt`) to the enemy nearest the ship. Reads `PlayerHurt` and
/// writes `Damage` (distinct types, so no reader/writer conflict with
/// `apply_damage`); the reflected `Damage` is applied the next tick.
pub fn apply_thorns(
    mut hurt: MessageReader<PlayerHurt>,
    mut dmg: MessageWriter<Damage>,
    upgrades: Res<Upgrades>,
    player: Query<&Transform, With<Ship>>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
) {
    let thorns = thorns_frac(upgrades.owned(UpgradeId::Thorns));
    let ppos = player.single().ok().map(|t| t.translation.truncate());
    for h in hurt.read() {
        if thorns <= 0.0 {
            continue;
        }
        let Some(ppos) = ppos else { continue };
        let nearest = enemies.iter().min_by(|(_, a), (_, b)| {
            let da = a.translation.truncate().distance_squared(ppos);
            let db = b.translation.truncate().distance_squared(ppos);
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        if let Some((e, _)) = nearest {
            dmg.write(Damage {
                target: e,
                amount: h.amount * thorns,
            });
        }
    }
}
