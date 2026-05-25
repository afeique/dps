//! EN enemy special mechanics (Phase EN) — the signature behaviors beyond the
//! shared movement/fire patterns. Each hooks a death (death-flare, split) or
//! runs on a per-enemy timer (spawner, acid trail). Built incrementally; the
//! death-hooked ones land first (they reuse the existing `Death` message).

use crate::combat::element::Element;
use crate::components::{
    AllyShield, Drone, DroneSpawner, Enemy, EnemyKind, PlayerCorrode, Ship, SupportAura,
    AURA_LINGER, SPORE_DRONE_CAP, SPORE_DRONE_INTERVAL,
};
use crate::messages::{Damage, Death};
use crate::systems::player_status::apply_player_status;
use bevy::prelude::*;

/// Ashen Detonator death-flare radius (enemy-data.js: 130 px).
pub const ASHEN_FLARE_RADIUS: f32 = 130.0;
/// Ashen Detonator death-flare damage (enemy-data.js: 12).
pub const ASHEN_FLARE_DAMAGE: f32 = 12.0;

/// On an **Ashen Detonator**'s death, a PYRO blast: if the player is within the
/// flare radius, it takes [`ASHEN_FLARE_DAMAGE`] + a burn (the `deathFlare`
/// mechanic). Runs after `apply_damage` (so it sees the `Death`); the `Damage`
/// it writes lands the next tick, like the other death-driven effects.
pub fn ashen_death_flare(
    mut commands: Commands,
    mut deaths: MessageReader<Death>,
    mut dmg: MessageWriter<Damage>,
    player: Query<(Entity, &Transform, Option<&PlayerCorrode>), With<Ship>>,
) {
    let Ok((player_e, ptf, pcorrode)) = player.single() else {
        return;
    };
    let ppos = ptf.translation.truncate();
    let corrode = pcorrode.map_or(0, |c| c.stacks);
    for d in deaths.read() {
        if d.kind != Some(EnemyKind::AshenDetonator) {
            continue;
        }
        if d.position.distance(ppos) <= ASHEN_FLARE_RADIUS {
            dmg.write(Damage {
                target: player_e,
                amount: ASHEN_FLARE_DAMAGE,
            });
            apply_player_status(&mut commands, player_e, Element::Pyro, corrode);
        }
    }
}

/// Each **Spore Carrier** births a Wasp [`Drone`] every [`SPORE_DRONE_INTERVAL`]
/// s, up to the [`SPORE_DRONE_CAP`] global live-drone cap (the `spawner`
/// mechanic). Runs in the spawner phase. `DroneSpawner` lives only on Spore
/// Carriers, so the query needs no kind filter.
pub fn spore_spawner(
    time: Res<Time>,
    mut commands: Commands,
    mut spores: Query<(&Transform, &mut DroneSpawner)>,
    drones: Query<(), With<Drone>>,
) {
    let dt = time.delta_secs();
    let mut budget = SPORE_DRONE_CAP.saturating_sub(drones.iter().count());
    for (tf, mut sp) in &mut spores {
        sp.timer -= dt;
        if sp.timer <= 0.0 {
            sp.timer = SPORE_DRONE_INTERVAL;
            if budget > 0 {
                crate::systems::enemy::spawn_drone(&mut commands, tf.translation.truncate());
                budget -= 1;
            }
        }
    }
}

/// Each **Lumen Drone** pulses its `SupportAura` every `interval` s, stamping a
/// refreshing [`AllyShield`] on every enemy within `radius` (the `shield` aura).
/// Both queries read `&Transform` immutably (the drone is in both), so no
/// conflict; the buff is applied via deferred `Commands`.
pub fn lumen_aura(
    time: Res<Time>,
    mut commands: Commands,
    mut lumens: Query<(&Transform, &mut SupportAura)>,
    allies: Query<(Entity, &Transform), With<Enemy>>,
) {
    let dt = time.delta_secs();
    for (ltf, mut aura) in &mut lumens {
        aura.timer -= dt;
        if aura.timer > 0.0 {
            continue;
        }
        aura.timer = aura.interval;
        let center = ltf.translation.truncate();
        let r2 = aura.radius * aura.radius;
        for (e, atf) in &allies {
            if atf.translation.truncate().distance_squared(center) <= r2 {
                commands.entity(e).insert(AllyShield {
                    secs: AURA_LINGER,
                    amount: aura.amount,
                });
            }
        }
    }
}

/// Count down each `AllyShield` and remove it on expiry — so when the supporting
/// Lumen Drone dies (stops refreshing), the buff lapses after `AURA_LINGER` s.
pub fn tick_ally_shield(
    time: Res<Time>,
    mut commands: Commands,
    mut q: Query<(Entity, &mut AllyShield)>,
) {
    let dt = time.delta_secs();
    for (e, mut s) in &mut q {
        s.secs -= dt;
        if s.secs <= 0.0 {
            commands.entity(e).remove::<AllyShield>();
        }
    }
}
