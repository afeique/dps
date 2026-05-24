//! Passive AoE powerups (spec VI.3 — the canonical update's `tickStaticDischarge`
//! / `tickWhirlwind`): periodic/continuous area damage centred on the player,
//! granted via shop upgrades read live. They emit `Damage` to nearby enemies;
//! the existing floating-damage-number FX gives the visual feedback. (A dedicated
//! discharge ring is deferred.)

use crate::components::{Collider, Enemy, Ship};
use crate::messages::Damage;
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
