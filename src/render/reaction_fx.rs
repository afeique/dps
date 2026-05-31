//! Reaction shockwaves: a one-shot expanding ring when an elemental reaction
//! fires (E4b) — icy cyan for SHATTER, fiery orange for OIL FLARE. Pure
//! presentation: `spawn_reaction_fx` reads `Reaction` messages from
//! `systems::reactions` and spawns a [`Shockwave`]; `tick_shockwaves` grows the
//! ring toward its peak radius and fades it out, then despawns it. Mirrors the
//! status-aura idiom (a top-level lyon ring, HDR for Bloom) rather than hanabi.

use crate::messages::{Death, Reaction, ReactionFx};
use crate::systems::enemy::element_for;
use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;

/// Peak radius (px) the ring expands to over its lifetime.
const SHATTER_PEAK: f32 = 110.0;
const FLARE_PEAK: f32 = 100.0;
/// Shockwave lifetime in seconds.
const WAVE_SECS: f32 = 0.35;

/// An expanding-ring shockwave. `age` counts up to `WAVE_SECS`; the ring scales
/// from ~0 to `peak` and fades as it goes.
#[derive(Component)]
pub struct Shockwave {
    pub age: f32,
    pub peak: f32,
}

/// A unit-radius (1 px) 32-gon ring; the entity's Transform scale grows it to the
/// live radius each tick. HDR-emissive so Bloom makes it pop. Shared with the
/// level-up aura (`render::level_up_aura`), which reuses `Shockwave`/`tick_shockwaves`.
pub fn unit_ring(color: Color) -> Shape {
    let mut path = ShapePath::new();
    for i in 0..32 {
        let a = i as f32 / 32.0 * std::f32::consts::TAU;
        let p = Vec2::new(a.cos(), a.sin());
        path = if i == 0 { path.move_to(p) } else { path.line_to(p) };
    }
    ShapeBuilder::with(&path.close()).stroke((color, 2.5)).build()
}

/// Spawn a shockwave at each resolved reaction.
pub fn spawn_reaction_fx(mut commands: Commands, mut reactions: MessageReader<Reaction>) {
    for r in reactions.read() {
        let (color, peak) = match r.kind {
            ReactionFx::Shatter => (Color::linear_rgb(2.0, 8.0, 9.0), SHATTER_PEAK),
            ReactionFx::Flare => (Color::linear_rgb(9.0, 3.0, 0.4), FLARE_PEAK),
        };
        commands.spawn((
            Shockwave { age: 0.0, peak },
            unit_ring(color),
            Transform::from_translation(r.center.extend(1.8)).with_scale(Vec3::splat(1.0)),
        ));
    }
}

/// HDR-scale a base (sRGB) color so Bloom flares the ring into a glow.
fn hdr(c: Color, gain: f32) -> Color {
    let l = c.to_linear();
    Color::linear_rgb(l.red * gain, l.green * gain, l.blue * gain)
}

/// Layered, element-tinted wavefront rings on each *enemy* death — the port of
/// rainboids' `createDebris` staggered `explosionRingColored` rings. The hanabi
/// burst (`render::explosion`) is the white-hot fire core; these add an
/// element-colored double wavefront — a bright inner ring + a dimmer, larger
/// outer ring — so every kill reads its damage element (pyro orange, cryo blue,
/// volt violet, …). Bosses get a third, white-hot, largest ring; the whole burst
/// scales with boss tier / mini-boss promotion. Player death (`kind == None`) is
/// skipped — it already has the explosion + screen shake + flash. Reuses
/// [`Shockwave`]/[`tick_shockwaves`], so no bespoke grow-fade lifecycle.
pub fn spawn_death_rings(mut commands: Commands, mut deaths: MessageReader<Death>) {
    for d in deaths.read() {
        let Some(kind) = d.kind else { continue }; // skip the player's own death
        let base = element_for(kind).color();
        let scale = (1.0 + 0.6 * d.boss_tier as f32) * if d.mini_boss { 1.3 } else { 1.0 };
        let z = 1.75; // just above the hanabi burst / asteroid rings

        let mut ring = |peak: f32, color: Color| {
            commands.spawn((
                Shockwave { age: 0.0, peak },
                unit_ring(color),
                Transform::from_translation(d.position.extend(z)).with_scale(Vec3::splat(1.0)),
            ));
        };
        // Four staggered wavefront rings (rainboids' `explosionRingColored` x4):
        // a white-hot flash, the bright element ring, a mid ring, and a dim
        // outer wavefront — so every kill blooms a layered chromatic shock.
        // Kept modest in size (the layered fireball reads as the "body").
        ring(26.0 * scale, Color::linear_rgb(9.0, 9.0, 9.0)); // white-hot flash
        ring(46.0 * scale, hdr(base, 6.0)); // bright element wavefront
        ring(72.0 * scale, hdr(base, 4.0)); // mid wavefront
        ring(102.0 * scale, hdr(base, 2.5)); // dim outer wavefront
        if d.boss_tier > 0 {
            ring(140.0 * scale, Color::linear_rgb(7.0, 7.0, 8.0)); // white-hot boss shock
        }
    }
}

/// Expand + fade each shockwave; despawn when its lifetime elapses.
pub fn tick_shockwaves(
    time: Res<Time>,
    mut commands: Commands,
    mut waves: Query<(Entity, &mut Shockwave, &mut Transform)>,
) {
    let dt = time.delta_secs();
    for (e, mut wave, mut tf) in &mut waves {
        wave.age += dt;
        if wave.age >= WAVE_SECS {
            commands.entity(e).despawn();
            continue;
        }
        // Ease-out radius (fast start, settles toward peak).
        let t = (wave.age / WAVE_SECS).clamp(0.0, 1.0);
        let radius = wave.peak * (1.0 - (1.0 - t) * (1.0 - t));
        tf.scale = Vec3::splat(radius.max(1.0));
    }
}
