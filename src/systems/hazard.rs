//! Hazard fields (Phase EN) — circular damage zones that tick element damage +
//! status on the player while inside, for a lifetime. Ported from
//! `js/modules/world/hazard-field.js`. Plaguebearer drops a Toxic acid trail as
//! it moves (`HazardDropper`); the same `HazardField` is reusable for future
//! player weapons (Caustic / Scorched Earth) against a separate target set.

use crate::combat::element::Element;
use crate::components::{PlayerCorrode, Ship};
use crate::messages::Damage;
use crate::systems::player_status::apply_player_status;
use bevy::prelude::*;

/// Damage tick cadence (`DAMAGE_TICK_MS` 300). A tick deals `dps × this` — the
/// chunk (≥1) survives the player-damage `.round()`.
pub const HAZARD_TICK_SECS: f32 = 0.3;
/// Live-hazard cap (`MAX_HAZARDS` 24); droppers skip spawning over it.
pub const MAX_HAZARDS: usize = 24;

// Plaguebearer trail params (enemy-data.js): r70, 6 dps, 3.5 s, every 600 ms.
pub const TRAIL_RADIUS: f32 = 70.0;
pub const TRAIL_DPS: f32 = 6.0;
pub const TRAIL_LIFE: f32 = 3.5;
pub const TRAIL_DROP_INTERVAL: f32 = 0.6;

/// A circular hazard zone: ticks `dps × HAZARD_TICK_SECS` `element` damage on the
/// player while inside `radius`, until `life` expires.
#[derive(Component, Debug, Clone, Copy)]
pub struct HazardField {
    pub radius: f32,
    pub element: Element,
    pub dps: f32,
    pub life: f32,
    /// Counts down to the next damage tick.
    pub tick: f32,
}

/// Drops a [`HazardField`] at the carrier's position every `interval` s
/// (Plaguebearer's acid trail).
#[derive(Component, Debug, Clone, Copy)]
pub struct HazardDropper {
    pub timer: f32,
    pub interval: f32,
    pub radius: f32,
    pub dps: f32,
    pub life: f32,
    pub element: Element,
}

/// The Plaguebearer's Toxic-trail dropper config.
pub fn plaguebearer_dropper() -> HazardDropper {
    HazardDropper {
        timer: TRAIL_DROP_INTERVAL,
        interval: TRAIL_DROP_INTERVAL,
        radius: TRAIL_RADIUS,
        dps: TRAIL_DPS,
        life: TRAIL_LIFE,
        element: Element::Toxic,
    }
}

/// Spawn a hazard-field entity at `pos`.
pub fn spawn_hazard(
    commands: &mut Commands,
    pos: Vec2,
    radius: f32,
    dps: f32,
    life: f32,
    element: Element,
) {
    commands.spawn((
        HazardField {
            radius,
            element,
            dps,
            life,
            tick: HAZARD_TICK_SECS,
        },
        Transform::from_translation(pos.extend(-0.5)),
    ));
}

/// Tick each hazard: while the player is inside, deal a `dps × HAZARD_TICK_SECS`
/// chunk + apply the element's player status every `HAZARD_TICK_SECS`; reset the
/// tick on exit (no back-dated burst); despawn at end of life. Runs in the
/// collision group (writes `Damage` before `apply_damage`).
pub fn tick_hazards(
    time: Res<Time>,
    mut commands: Commands,
    mut dmg: MessageWriter<Damage>,
    player: Query<(Entity, &Transform, Option<&PlayerCorrode>), With<Ship>>,
    mut hazards: Query<(Entity, &Transform, &mut HazardField)>,
) {
    let dt = time.delta_secs();
    let player_data = player
        .single()
        .ok()
        .map(|(e, t, c)| (e, t.translation.truncate(), c.map_or(0, |c| c.stacks)));
    for (he, htf, mut hz) in &mut hazards {
        hz.life -= dt;
        if hz.life <= 0.0 {
            commands.entity(he).despawn();
            continue;
        }
        let inside =
            player_data.is_some_and(|(_, ppos, _)| ppos.distance(htf.translation.truncate()) <= hz.radius);
        if inside {
            hz.tick -= dt;
            if hz.tick <= 0.0 {
                if let Some((pe, _, corrode)) = player_data {
                    dmg.write(Damage {
                        target: pe,
                        amount: hz.dps * HAZARD_TICK_SECS,
                    });
                    apply_player_status(&mut commands, pe, hz.element, corrode);
                }
                hz.tick = HAZARD_TICK_SECS;
            }
        } else {
            hz.tick = HAZARD_TICK_SECS;
        }
    }
}

/// Each [`HazardDropper`] (Plaguebearer) drops a hazard at its position every
/// `interval` s, under the [`MAX_HAZARDS`] cap. Runs in the spawner phase.
pub fn drop_hazards(
    time: Res<Time>,
    mut commands: Commands,
    hazards: Query<(), With<HazardField>>,
    mut droppers: Query<(&Transform, &mut HazardDropper)>,
) {
    let dt = time.delta_secs();
    let mut budget = MAX_HAZARDS.saturating_sub(hazards.iter().count());
    for (tf, mut d) in &mut droppers {
        d.timer -= dt;
        if d.timer <= 0.0 {
            d.timer = d.interval;
            if budget > 0 {
                spawn_hazard(&mut commands, tf.translation.truncate(), d.radius, d.dps, d.life, d.element);
                budget -= 1;
            }
        }
    }
}
