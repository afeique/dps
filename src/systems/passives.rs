//! Passive AoE powerups (spec VI.3 — the canonical update's `tickStaticDischarge`
//! / `tickWhirlwind`): periodic/continuous area damage centred on the player,
//! granted via shop upgrades read live. They emit `Damage` to nearby enemies;
//! the existing floating-damage-number FX gives the visual feedback. (A dedicated
//! discharge ring is deferred.)

use crate::components::{Collider, Enemy, Health, Ship};
use crate::messages::{Damage, Death, PlayerHurt};
use crate::systems::shop::{
    static_discharge_damage, static_discharge_interval, whirlwind_dps, UpgradeId, Upgrades,
};
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

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

// ── Whirlwind (orbiting damage zone, spec VI.3) ────────────────────────────────

/// Marks the orbiting Whirlwind blade (a visual; the damage is analytic).
#[derive(Component)]
pub struct WhirlwindBlade;

/// Orbit angular speed (rad/s).
pub const WHIRL_OMEGA: f32 = 4.0;
/// Orbit radius around the player (world px).
pub const WHIRL_ORBIT_R: f32 = 95.0;
/// Damage-zone radius at the orbit point (world px).
const WHIRL_ZONE_R: f32 = 50.0;

/// The current orbit-zone centre for `stacks`-owned Whirlwind at elapsed `t`,
/// relative to the player (player position is added by the caller).
fn whirl_offset(t: f32) -> Vec2 {
    Vec2::new((t * WHIRL_OMEGA).cos(), (t * WHIRL_OMEGA).sin()) * WHIRL_ORBIT_R
}

/// A hollow HDR-cyan ring sized to the damage zone (blooms into a glowing blade).
fn whirl_shape() -> Shape {
    let mut path = ShapePath::new();
    for i in 0..24 {
        let a = i as f32 / 24.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * WHIRL_ZONE_R, a.sin() * WHIRL_ZONE_R);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .stroke((Color::linear_rgb(0.5, 8.0, 9.0), 3.0))
        .build()
}

/// Whirlwind: while owned, a blade orbits the player dealing `whirlwind_dps × dt`
/// to enemies within `WHIRL_ZONE_R` of the orbit point (spec VI.3). The blade is a
/// lyon visual spawned lazily (despawned when unowned); the damage is analytic.
pub fn tick_whirlwind(
    time: Res<Time>,
    upgrades: Res<Upgrades>,
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    player: Query<&Transform, With<Ship>>,
    enemies: Query<(Entity, &Transform, &Collider), With<Enemy>>,
    mut blade: Query<(Entity, &mut Transform), (With<WhirlwindBlade>, Without<Ship>, Without<Enemy>)>,
) {
    let stacks = upgrades.owned(UpgradeId::Whirlwind);
    if stacks == 0 {
        for (e, _) in &blade {
            commands.entity(e).despawn(); // unowned — clear the visual
        }
        return;
    }
    let Ok(ptf) = player.single() else {
        return;
    };
    let center = ptf.translation.truncate() + whirl_offset(time.elapsed_secs());

    // Visual: spawn the blade if missing, else move it to the orbit point.
    if blade.is_empty() {
        commands.spawn((
            WhirlwindBlade,
            whirl_shape(),
            Transform::from_translation(center.extend(0.4)),
        ));
    } else {
        for (_, mut tf) in &mut blade {
            tf.translation = center.extend(0.4);
        }
    }

    // Damage: continuous DoT to enemies in the zone.
    let tick = whirlwind_dps(stacks) * time.delta_secs();
    for (e, etf, ec) in &enemies {
        if etf.translation.truncate().distance(center) <= WHIRL_ZONE_R + ec.radius {
            dmg.write(Damage { target: e, amount: tick });
        }
    }
}
