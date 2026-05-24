//! Passive AoE powerups (spec VI.3 — the canonical update's `tickStaticDischarge`
//! / `tickWhirlwind`): periodic/continuous area damage centred on the player,
//! granted via shop upgrades read live. They emit `Damage` to nearby enemies;
//! the existing floating-damage-number FX gives the visual feedback. (A dedicated
//! discharge ring is deferred.)

use crate::components::{Collider, Enemy, Health, Ship};
use crate::messages::{Damage, Death, PlayerHurt};
use crate::systems::shop::{
    static_discharge_damage, static_discharge_interval, UpgradeId, Upgrades,
};
use bevy::prelude::*;

/// Radius (world px) of the Static Discharge pulse around the player.
const DISCHARGE_RADIUS: f32 = 150.0;

/// Static Discharge: while owned, every `static_discharge_interval` seconds deal
/// `static_discharge_damage` to every enemy within `DISCHARGE_RADIUS` of the
/// player (spec VI.3). The timer is a `Local` accumulator (reset while unowned).
pub fn tick_static_discharge(
    time: Res<Time>,
    upgrades: Res<Upgrades>,
    mut timer: Local<f32>,
    mut dmg: MessageWriter<Damage>,
    player: Query<&Transform, With<Ship>>,
    enemies: Query<(Entity, &Transform, &Collider), With<Enemy>>,
) {
    let stacks = upgrades.owned(UpgradeId::StaticDischarge);
    if stacks == 0 {
        *timer = 0.0;
        return;
    }
    *timer += time.delta_secs();
    if *timer < static_discharge_interval(stacks) {
        return;
    }
    *timer = 0.0;

    let Ok(ptf) = player.single() else {
        return;
    };
    let ppos = ptf.translation.truncate();
    let amount = static_discharge_damage(stacks);
    for (e, etf, ec) in &enemies {
        if etf.translation.truncate().distance(ppos) <= DISCHARGE_RADIUS + ec.radius {
            dmg.write(Damage { target: e, amount });
        }
    }
}

/// HP restored by a Combat Medic proc (spec VI.3).
pub const COMBAT_MEDIC_HEAL: f32 = 10.0;
/// Cooldown (s) between Combat Medic procs (spec VI.3: 8 s).
const COMBAT_MEDIC_CD: f32 = 8.0;

/// Combat Medic: taking a hit *arms* the heal; the next enemy kill while armed and
/// off cooldown restores `COMBAT_MEDIC_HEAL` (capped at max) and starts the 8 s
/// cooldown (spec VI.3, maxStacks 1). State is `Local` (armed flag + cooldown).
pub fn tick_combat_medic(
    time: Res<Time>,
    upgrades: Res<Upgrades>,
    mut armed: Local<bool>,
    mut cooldown: Local<f32>,
    mut hurt: MessageReader<PlayerHurt>,
    mut deaths: MessageReader<Death>,
    mut player: Query<&mut Health, With<Ship>>,
) {
    *cooldown = (*cooldown - time.delta_secs()).max(0.0);
    if upgrades.owned(UpgradeId::CombatMedic) == 0 {
        *armed = false;
        return;
    }
    // Taking a hit arms the heal.
    if hurt.read().count() > 0 {
        *armed = true;
    }
    // An enemy kill while armed + ready triggers the heal.
    let killed = deaths.read().any(|d| d.kind.is_some());
    if *armed && *cooldown <= 0.0 && killed {
        if let Ok(mut hp) = player.single_mut() {
            hp.current = (hp.current + COMBAT_MEDIC_HEAL).min(hp.max);
        }
        *armed = false;
        *cooldown = COMBAT_MEDIC_CD;
    }
}
