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
use bevy_prototype_lyon::prelude::*;

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

/// A danger-zone silhouette: a translucent element-tinted pool (a flat ground
/// tint that doesn't bloom — alpha lets the starfield show through) ringed by an
/// HDR glow rim that Bloom flares, so the zone reads clearly without obscuring
/// the field. Built at the true `radius`; `animate_hazards` breathes it.
fn hazard_shape(radius: f32, element: Element) -> Shape {
    let s = element.color().to_srgba();
    let fill = Color::srgba(s.red, s.green, s.blue, 0.16); // see-through ground tint
    let l = element.color().to_linear();
    let rim = Color::linear_rgb(l.red * 4.5, l.green * 4.5, l.blue * 4.5); // glowing edge
    let mut path = ShapePath::new();
    for i in 0..40 {
        let a = i as f32 / 40.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos() * radius, a.sin() * radius);
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close())
        .fill(fill)
        .stroke((rim, 2.5))
        .build()
}

/// Spawn a hazard-field entity at `pos` — now with a visible danger-zone
/// silhouette (it was an invisible damage disc before).
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
        hazard_shape(radius, element),
        Transform::from_translation(pos.extend(-0.5)),
    ));
}

/// Breathe each hazard zone — an upward-only pulse (`1.00..1.06`) so the field
/// reads as a live, pulsing pool yet never visually *under*-represents its true
/// damage radius (the gameplay radius is the fixed `HazardField.radius`; this only
/// scales the silhouette). Pure presentation; phase-desynced per entity.
pub fn animate_hazards(time: Res<Time>, mut q: Query<(Entity, &mut Transform), With<HazardField>>) {
    let t = time.elapsed_secs();
    for (e, mut tf) in &mut q {
        let phase = (e.to_bits() % 997) as f32 * 0.0063;
        let breathe = 1.0 + 0.06 * (0.5 + 0.5 * (t * 3.0 + phase).sin());
        tf.scale = Vec3::splat(breathe);
    }
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
