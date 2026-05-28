//! Reaction shockwaves: a one-shot expanding ring when an elemental reaction
//! fires (E4b) — icy cyan for SHATTER, fiery orange for OIL FLARE. Pure
//! presentation: `spawn_reaction_fx` reads `Reaction` messages from
//! `systems::reactions` and spawns a [`Shockwave`]; `tick_shockwaves` grows the
//! ring toward its peak radius and fades it out, then despawns it. Mirrors the
//! status-aura idiom (a top-level lyon ring, HDR for Bloom) rather than hanabi.

use crate::messages::{Reaction, ReactionFx};
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
