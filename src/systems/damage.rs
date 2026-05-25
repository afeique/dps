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
    Boss, Bulwark, Enemy, Health, Invulnerable, Lives, MiniBoss, Shield, Ship, Splitter,
    BULWARK_RESIST, MAX_TANKS, SHIELD_REDUCTION_CAP, TANK_OVERFLOW_HP,
};
use crate::messages::{Damage, Death, PlayerHurt};
use crate::resources::{DamageClock, GameRng, KillStreak, LastStandUsed, Score};
use crate::states::GameState;
use crate::systems::items::{AffixKind, Equipment};
use crate::systems::shop::{dodge_chance, regen_rate, thorns_frac, UpgradeId, Upgrades, REGEN_DELAY};
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
    equipment: Res<Equipment>,
    mut last_stand: ResMut<LastStandUsed>,
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
        Has<MiniBoss>,
        Option<&Splitter>,
    )>,
) {
    for ev in dmg.read() {
        let Ok((
            mut hp,
            tf,
            shield,
            mut lives,
            is_player,
            invulnerable,
            enemy,
            boss,
            bulwark,
            mini_boss,
            splitter,
        )) = q.get_mut(ev.target)
        else {
            continue; // target already gone this tick
        };
        if invulnerable {
            continue; // deliberate i-frames (dash / shield-burst) — eat the hit
        }

        if is_player {
            // DODGE: chance to ignore the hit entirely — no damage, no streak
            // break (spec II.2 step 3 / III.5). Shop stacks + equipped item dodge
            // affixes, capped at 50 %.
            let dodge = (dodge_chance(upgrades.owned(UpgradeId::Dodge))
                + equipment.affix_total(AffixKind::Dodge) / 100.0)
                .min(0.5);
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
            // Player shield = base %DR + equipped TOUGHNESS affixes, capped.
            let mut reduction = s.reduction;
            if is_player {
                reduction = (reduction + equipment.affix_total(AffixKind::Toughness) / 100.0)
                    .min(SHIELD_REDUCTION_CAP);
            }
            amount *= 1.0 - reduction;
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
                // Last Stand: cheat death once per run — clamp to 1 HP + a 2.5 s
                // invuln window (spec III.5, before tank/death).
                if upgrades.owned(UpgradeId::LastStand) > 0 && !last_stand.0 {
                    last_stand.0 = true;
                    hp.current = 1.0;
                    commands
                        .entity(ev.target)
                        .insert(Invulnerable { seconds: 2.5 });
                    continue;
                }
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
                    mini_boss: false,
                });
                next_state.set(GameState::GameOver);
                commands.entity(ev.target).despawn();
            } else {
                // Split-on-death (Hydra, EN): spawn the lings at the death spot
                // before the parent despawns. Lings get `lings: 0` (no re-split).
                if let Some(s) = splitter {
                    for i in 0..s.lings {
                        let a = i as f32 / s.lings.max(1) as f32 * std::f32::consts::TAU;
                        let off = Vec2::new(a.cos(), a.sin()) * 20.0;
                        crate::systems::enemy::spawn_split_ling(&mut commands, position + off);
                    }
                }
                deaths.write(Death {
                    entity: ev.target,
                    position,
                    kind: enemy.map(|e| e.kind),
                    boss_tier: boss.map_or(0, |b| b.tier),
                    mini_boss,
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

/// Passive HP regen (spec II.2): track time since the player last took damage
/// (`PlayerHurt` resets it) and, once `REGEN_DELAY` has elapsed, regenerate at
/// the `Regen`-stacked rate (capped at max HP — overheal isn't generated here).
pub fn passive_regen(
    time: Res<Time>,
    mut clock: ResMut<DamageClock>,
    mut hurt: MessageReader<PlayerHurt>,
    upgrades: Res<Upgrades>,
    equipment: Res<Equipment>,
    mut player: Query<&mut Health, With<Ship>>,
) {
    let dt = time.delta_secs();
    if hurt.read().count() > 0 {
        clock.0 = 0.0;
    } else {
        clock.0 += dt;
    }
    // Shop Repair Field + equipped REGEN affixes (flat HP/s).
    let rate = regen_rate(upgrades.owned(UpgradeId::Regen)) + equipment.affix_total(AffixKind::Regen);
    if clock.0 >= REGEN_DELAY && rate > 0.0 {
        if let Ok(mut hp) = player.single_mut() {
            hp.current = (hp.current + rate * dt).min(hp.max);
        }
    }
}

/// Convert player overheal (HP above max) into spare-tank progress (spec II.2):
/// `TANK_OVERFLOW_HP` (40) of overheal = 1 tank, up to `MAX_TANKS`; then clamp
/// HP back to max. Heal sites over-fill freely and this is the single converter.
pub fn overheal_to_tanks(mut player: Query<(&mut Health, &mut Lives), With<Ship>>) {
    let Ok((mut hp, mut lives)) = player.single_mut() else {
        return;
    };
    if hp.current <= hp.max {
        return;
    }
    let excess = hp.current - hp.max;
    hp.current = hp.max;
    if lives.count < MAX_TANKS {
        lives.progress += excess / TANK_OVERFLOW_HP;
        while lives.progress >= 1.0 && lives.count < MAX_TANKS {
            lives.count += 1;
            lives.progress -= 1.0;
        }
        if lives.count >= MAX_TANKS {
            lives.progress = 0.0; // at cap — no unbounded accumulation
        }
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
    equipment: Res<Equipment>,
    player: Query<&Transform, With<Ship>>,
    enemies: Query<(Entity, &Transform), With<Enemy>>,
) {
    // Shop Thorns + equipped THORNS affixes (% of taken damage reflected).
    let thorns =
        thorns_frac(upgrades.owned(UpgradeId::Thorns)) + equipment.affix_total(AffixKind::Thorns) / 100.0;
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
